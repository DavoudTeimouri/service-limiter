# Changelog

All notable changes to this project will be documented in this file.

## [0.1.0] - 2026-09-24

### Added
- Initial commit of service-limiter skeleton.
- Cross-platform service discovery (Windows WMI/PowerShell, Linux systemctl).
- Resource profiling using psutil.
- Policy engine with default limits (CPU 80%, Memory 512MB, IO 1MB/s read, 512KB/s write).
- Configuration generation for Linux (systemd overrides) and Windows (Job Object PowerShell scripts).
- CLI with commands: analyze, generate, apply, tui (Linux only).
- TUI for Linux using curses.
- Profile support (e.g., web-server.json).
- Human-in-the-loop design for applying configurations.
- Updated README and USER_GUIDE with technical writer feedback for clarity and completeness.
- Documentation: README, USER_GUIDE, ARCHITECTURE, STRUCTURE, GITHUB_SETUP.

