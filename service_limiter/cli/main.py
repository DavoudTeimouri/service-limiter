"""Command-line interface for service limiter."""

import argparse
import logging
import os
import platform
import subprocess
import sys
from ..orchestrator import Orchestrator

logger = logging.getLogger(__name__)

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

    # TUI command (Linux only)
    tui_parser = subparsers.add_parser('tui', help='Launch TUI (Linux only)')

    args = parser.parse_args()
    logging.basicConfig(level=logging.INFO)

    if args.command == 'analyze':
        orchestrator = Orchestrator(profile_name=args.profile)
        result = orchestrator.run_analysis()
        print(f"Analysis complete: {len(result.get('services', []))} services")
        # Print a summary of the first few services
        services = result.get('services', [])
        if services:
            print("First few services:")
            for svc in services[:5]:
                print(f"  - {svc.get('name', 'Unknown')}: {svc.get('display_name', '')} ({svc.get('status', '')})")
        else:
            print("No services found.")
    elif args.command == 'generate':
        orchestrator = Orchestrator(profile_name=args.profile)
        result = orchestrator.run_analysis()
        print(f"Generated {len(result.get('configs', {}))} configs")
    elif args.command == 'apply':
        apply_configs(args.config, args.dry_run)
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

def apply_configs(config_dir, dry_run=False):
    """Apply the generated configurations in config_dir."""
    if not os.path.isdir(config_dir):
        logger.error(f"Config directory {config_dir} does not exist.")
        return

    system = platform.system()
    if system == 'Linux':
        apply_linux(config_dir, dry_run)
    elif system == 'Windows':
        apply_windows(config_dir, dry_run)
    else:
        logger.error(f"Unsupported platform: {system}")

def apply_linux(config_dir, dry_run):
    """Apply Linux systemd override configs."""
    # Check if we are root
    if os.geteuid() != 0:
        logger.error("Root privileges are required to apply Linux configurations.")
        return

    # Iterate over subdirectories in config_dir (each is a service name)
    for item in os.listdir(config_dir):
        service_dir = os.path.join(config_dir, item)
        if not os.path.isdir(service_dir):
            continue
        service_name = item
        override_conf = os.path.join(service_dir, 'override.conf')
        if not os.path.isfile(override_conf):
            logger.warning(f"No override.conf found for service {service_name} in {service_dir}")
            continue

        # The target drop-in directory
        drop_in_dir = f"/etc/systemd/system/{service_name}.service.d"
        target_conf = os.path.join(drop_in_dir, 'override.conf')

        if dry_run:
            logger.info(f"[DRY-RUN] Would create directory {drop_in_dir} if it doesn't exist")
            logger.info(f"[DRY-RUN] Would copy {override_conf} to {target_conf}")
            logger.info(f"[DRY-RUN] Would run: systemctl daemon-reload")
            logger.info(f"[DRY-RUN] Would run: systemctl restart {service_name}")
        else:
            try:
                # Create the drop-in directory if it doesn't exist
                os.makedirs(drop_in_dir, exist_ok=True)
                # Copy the override.conf
                with open(override_conf, 'r') as f_in:
                    content = f_in.read()
                with open(target_conf, 'w') as f_out:
                    f_out.write(content)
                logger.info(f"Installed override.conf for {service_name} to {target_conf}")
                # Reload systemd
                subprocess.run(['systemctl', 'daemon-reload'], check=True)
                logger.info("Reloaded systemd")
                # Restart the service
                subprocess.run(['systemctl', 'restart', service_name], check=True)
                logger.info(f"Restarted service {service_name}")
            except subprocess.CalledProcessError as e:
                logger.error(f"Failed to apply config for {service_name}: {e}")
            except Exception as e:
                logger.error(f"Error applying config for {service_name}: {e}")

def apply_windows(config_dir, dry_run):
    """Apply Windows Job Object configs via PowerShell script."""
    # Note: We cannot easily check for admin privileges in a cross-platform way without external libraries.
    # We'll try to run the PowerShell script and see if it fails due to permissions.
    # Iterate over subdirectories in config_dir (each is a service name)
    for item in os.listdir(config_dir):
        service_dir = os.path.join(config_dir, item)
        if not os.path.isdir(service_dir):
            continue
        service_name = item
        ps_script = os.path.join(service_dir, 'Set-JobLimits.ps1')
        if not os.path.isfile(ps_script):
            logger.warning(f"No Set-JobLimits.ps1 found for service {service_name} in {service_dir}")
            continue

        if dry_run:
            logger.info(f"[DRY-RUN] Would run PowerShell script: {ps_script}")
            logger.info(f"[DRY-RUN] Would then assign the service's processes to the Job Object")
        else:
            try:
                # Run the PowerShell script
                result = subprocess.run(['powershell', '-NoProfile', '-ExecutionPolicy', 'Bypass', '-File', ps_script],
                                        capture_output=True, text=True, check=True)
                logger.info(f"Ran PowerShell script for {service_name}: {result.stdout}")
                if result.stderr:
                    logger.warning(f"PowerShell script stderr: {result.stderr}")
                # TODO: Assign the service's processes to the Job Object.
                # This is complex and requires knowing the service's process IDs.
                # We'll leave it as a note for the user.
                logger.info(f"Job Object created for {service_name}. You must now assign the service's processes to this Job Object.")
                logger.info(f"To get the service's process IDs, run: Get-WmiObject -Query \"SELECT ProcessId FROM Win32_Service WHERE Name='{service_name}'\"")
                logger.info(f"Then for each PID, run: $job.AssignProcess(<PID>)")
            except subprocess.CalledProcessError as e:
                logger.error(f"Failed to run PowerShell script for {service_name}: {e}")
                logger.error(f"stdout: {e.stdout}")
                logger.error(f"stderr: {e.stderr}")
            except Exception as e:
                logger.error(f"Error applying config for {service_name}: {e}")

if __name__ == '__main__':
    main()
