"""Command-line interface for service limiter."""

import argparse
import json
import logging
import os
import platform
import shutil
import subprocess
import sys
import tempfile
import time
from ..orchestrator import Orchestrator

logger = logging.getLogger(__name__)

# Filename the generator stages and the applier looks for, per platform.
PLATFORM_CONFIG_FILENAME = {'Linux': 'override.conf', 'Windows': 'Set-JobLimits.ps1'}

# Exit codes, so the tool is usable from scripts and CI.
EXIT_OK = 0            # ran fine, nothing violated the policy
EXIT_VIOLATION = 1     # at least one service exceeds the policy
EXIT_ERROR = 2         # runtime/environment failure (no systemd, unreadable config)
EXIT_NEEDS_ROOT = 3    # apply requires elevation

AUDIT_DIR = os.path.join(
    os.environ.get("HOME") or os.path.expanduser("~"), ".service-limiter")


def audit(event, **fields):
    """Append one JSON line to ~/.service-limiter/audit.log.

    ponytail: plain JSONL, no rotation or locking. Add rotation if the file
    ever grows large enough to matter.
    """
    record = {"event": event, "time": time.strftime("%Y-%m-%dT%H:%M:%S%z"), **fields}
    try:
        os.makedirs(AUDIT_DIR, exist_ok=True)
        with open(os.path.join(AUDIT_DIR, "audit.log"), "a") as f:
            f.write(json.dumps(record) + "\n")
    except OSError as e:
        logger.warning("Could not write audit log: %s", e)

def main():
    parser = argparse.ArgumentParser(description='Service Limiter')
    subparsers = parser.add_subparsers(dest='command')

    # Analyze command
    analyze_parser = subparsers.add_parser('analyze', help='Analyze services')
    analyze_parser.add_argument('--profile', help='Profile to use (name of JSON file in profiles/ without .json)')

    # Generate command
    generate_parser = subparsers.add_parser('generate', help='Generate config')
    generate_parser.add_argument('--profile', required=True, help='Profile name')
    generate_parser.add_argument('--output', default='./config', help='Output directory')

    # Apply command
    apply_parser = subparsers.add_parser('apply', help='Apply config')
    apply_parser.add_argument('--config', required=True, help='Config directory (output from generate)')
    apply_parser.add_argument('--dry-run', action='store_true', help='Only show what would be done')
    apply_parser.add_argument('--yes', action='store_true', help='Skip the confirmation prompt')

    # TUI command (Linux only)
    tui_parser = subparsers.add_parser('tui', help='Launch TUI (Linux only)')

    args = parser.parse_args()
    logging.basicConfig(level=logging.INFO)

    if args.command == 'analyze':
        orchestrator = Orchestrator(profile_name=args.profile)
        result = orchestrator.run_analysis()
        services = result.get('services', [])
        limited = result.get('limited_services', [])
        profiled = len(result.get('profiles', []))
        print(f"Discovered {len(services)} services, profiled {profiled}, "
              f"{len(limited)} over policy")
        for svc in services[:5]:
            print(f"  - {svc.get('name', 'Unknown')}: {svc.get('display_name', '')} ({svc.get('status', '')})")
        if profiled == 0:
            # Nothing measurable means we cannot claim the tool worked.
            logger.error("No service could be profiled (no live MainPID or no systemd).")
            audit("analyze", discovered=len(services), profiled=0)
            sys.exit(EXIT_ERROR)
        audit("analyze", discovered=len(services), profiled=profiled, over_policy=len(limited))
        sys.exit(EXIT_VIOLATION if limited else EXIT_OK)
    elif args.command == 'generate':
        orchestrator = Orchestrator(profile_name=args.profile)
        result = orchestrator.run_analysis()
        configs = result.get('configs', {})
        if not configs:
            print("No configs generated: no service exceeded the policy.")
            sys.exit(1)
        # Stage configs as <output>/<service>/<file>, which is exactly the layout
        # apply_linux/apply_windows scan. Previously --output was accepted and ignored.
        for name, cfg in configs.items():
            service_dir = os.path.join(args.output, name)
            os.makedirs(service_dir, exist_ok=True)
            filename = PLATFORM_CONFIG_FILENAME.get(platform.system(), 'override.conf')
            target = os.path.join(service_dir, filename)
            with open(target, 'w') as f:
                f.write(cfg['content'])
            print(f"Wrote {target}")
        print(f"Generated {len(configs)} configs in {args.output}")
        audit("generate", output=args.output, count=len(configs))
        sys.exit(EXIT_OK)
    elif args.command == 'apply':
        apply_configs(args.config, args.dry_run, args.yes)
    elif args.command == 'tui':
        if platform.system() != 'Linux':
            logger.error("TUI is only available on Linux")
            return
        # Import and run the TUI
        try:
            from ..tui import main as tui_main
            import curses
            curses.wrapper(tui_main)
        except ImportError as e:
            logger.error(f"Failed to import TUI module: {e}")
            print("Error: TUI module not found. Please ensure the installation is complete.")
        except Exception as e:
            logger.error(f"Error running TUI: {e}")
    else:
        parser.print_help()

def apply_configs(config_dir, dry_run=False, assume_yes=False):
    """Apply the generated configurations in config_dir."""
    if not os.path.isdir(config_dir):
        logger.error(f"Config directory {config_dir} does not exist. Run 'generate' first.")
        sys.exit(EXIT_ERROR)

    system = platform.system()
    if system == 'Linux':
        if os.geteuid() != 0:
            logger.error("apply needs root: re-run with sudo.")
            audit("apply_denied", reason="not_root", config=config_dir)
            sys.exit(EXIT_NEEDS_ROOT)
        ok = apply_linux(config_dir, dry_run, assume_yes)
    elif system == 'Windows':
        ok = apply_windows(config_dir, dry_run, assume_yes)
    else:
        logger.error(f"Unsupported platform: {system}")
        ok = False
    audit("apply", config=config_dir, dry_run=dry_run, ok=ok)
    sys.exit(EXIT_OK if ok else EXIT_ERROR)

def apply_linux(config_dir, dry_run, assume_yes=False):
    """Apply Linux systemd override configs."""
    all_ok = True
    for item in sorted(os.listdir(config_dir)):
        service_dir = os.path.join(config_dir, item)
        if not os.path.isdir(service_dir):
            continue
        service_name = item
        override_conf = os.path.join(service_dir, 'override.conf')
        if not os.path.isfile(override_conf):
            logger.warning(f"No override.conf found for service {service_name} in {service_dir}")
            all_ok = False
            continue

        # The target drop-in directory
        drop_in_dir = f"/etc/systemd/system/{service_name}.service.d"
        target_conf = os.path.join(drop_in_dir, 'override.conf')

        if dry_run:
            logger.info(f"[DRY-RUN] Would write {target_conf}")
            logger.info(f"[DRY-RUN] Would run: systemctl daemon-reload")
            logger.info(f"[DRY-RUN] Would run: systemctl restart {service_name}")
            continue

        if not assume_yes:
            reply = input(f"Apply limits to {service_name} and restart it? [y/N] ")
            if reply.strip().lower() not in ("y", "yes"):
                logger.info(f"Skipped {service_name}")
                continue

        try:
            with open(override_conf) as f_in:
                content = f_in.read()

            # Validate before touching /etc: systemd silently ignores unknown keys.
            with tempfile.NamedTemporaryFile("w", suffix=".service", delete=False) as tf:
                tf.write("[Service]\n" + content)
                probe = tf.name
            check = subprocess.run(["systemd-analyze", "verify", probe],
                                    capture_output=True, text=True)
            os.unlink(probe)
            bad = [ln for ln in (check.stderr or "").splitlines()
                   if "Unknown key" in ln or "Invalid" in ln]
            if bad:
                logger.error(f"Refusing {service_name}: systemd rejected:\n  " + "\n  ".join(bad))
                all_ok = False
                continue

            os.makedirs(drop_in_dir, exist_ok=True)
            # Back up any existing drop-in so apply is reversible.
            if os.path.exists(target_conf):
                backup = target_conf + ".bak"
                shutil.copy2(target_conf, backup)
                logger.info(f"Backed up existing config to {backup}")
            with open(target_conf, "w") as f_out:
                f_out.write(content)
            logger.info(f"Installed override.conf for {service_name} to {target_conf}")

            subprocess.run(["systemctl", "daemon-reload"], check=True)
            subprocess.run(["systemctl", "restart", service_name], check=True)
            logger.info(f"Restarted service {service_name}")
        except subprocess.CalledProcessError as e:
            logger.error(f"Failed to apply config for {service_name}: {e}")
            all_ok = False
        except Exception as e:
            logger.error(f"Error applying config for {service_name}: {e}")
            all_ok = False

    return all_ok

def apply_windows(config_dir, dry_run, assume_yes=False):
    """Apply Windows Job Object configs via the generated PowerShell script."""
    all_ok = True
    for item in sorted(os.listdir(config_dir)):
        service_dir = os.path.join(config_dir, item)
        if not os.path.isdir(service_dir):
            continue
        service_name = item
        ps_script = os.path.join(service_dir, PLATFORM_CONFIG_FILENAME["Windows"])
        if not os.path.isfile(ps_script):
            logger.warning(f"No {PLATFORM_CONFIG_FILENAME['Windows']} found for {service_name} in {service_dir}")
            all_ok = False
            continue

        if dry_run:
            logger.info(f"[DRY-RUN] Would run: <pwsh> -ExecutionPolicy Bypass -File {ps_script}")
            continue

        if not assume_yes:
            reply = input(f"Apply limits to {service_name} via Job Object? [y/N] ")
            if reply.strip().lower() not in ("y", "yes"):
                logger.info(f"Skipped {service_name}")
                continue

        from ..platform.windows import _powershell
        try:
            shell = _powershell()
            result = subprocess.run(
                [shell, "-NoProfile", "-ExecutionPolicy", "Bypass", "-File", ps_script],
                capture_output=True, text=True, check=True)
            logger.info(result.stdout.strip() or f"Applied limits to {service_name}")
            if result.stderr.strip():
                logger.warning(result.stderr.strip())
        except (subprocess.CalledProcessError, FileNotFoundError) as e:
            logger.error(f"Failed to apply config for {service_name}: {e}")
            all_ok = False

    return all_ok


if __name__ == '__main__':
    main()
