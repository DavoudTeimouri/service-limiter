"""Command-line interface for service limiter."""

import argparse
import logging
import platform
from .orchestrator import Orchestrator

logger = logging.getLogger(__name__)

def main():
    parser = argparse.ArgumentParser(description='Service Limiter')
    subparsers = parser.add_subparsers(dest='command')

    # Analyze command
    analyze_parser = subparsers.add_parser('analyze', help='Analyze services')
    analyze_parser.add_argument('--profile', help='Profile to use')

    # Generate command
    generate_parser = subparsers.add_parser('generate', help='Generate config')
    generate_parser.add_argument('--profile', required=True, help='Profile name')
    generate_parser.add_argument('--output', default='./config', help='Output directory')

    # Apply command
    apply_parser = subparsers.add_parser('apply', help='Apply config')
    apply_parser.add_argument('--config', required=True, help='Config file or directory')

    # TUI command (Linux only)
    tui_parser = subparsers.add_parser('tui', help='Launch TUI (Linux only)')

    args = parser.parse_args()
    logging.basicConfig(level=logging.INFO)

    orchestrator = Orchestrator()

    if args.command == 'analyze':
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
        # Placeholder
        print(f"Generating config for profile {args.profile} to {args.output}")
    elif args.command == 'apply':
        print(f"Applying config from {args.config}")
    elif args.command == 'tui':
        if platform.system() != 'Linux':
            logger.error("TUI is only available on Linux")
            return
        # Placeholder for TUI
        print("Launching TUI...")
        print("Note: TUI implementation is not yet complete.")
    else:
        parser.print_help()

if __name__ == '__main__':
    main()
