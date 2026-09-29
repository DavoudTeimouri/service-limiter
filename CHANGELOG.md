# Changelog
All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]
### Added
### Changed
### Fixed
### Removed

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
- Added description and topics to the GitHub repository.
- Added detailed README and USER_GUIDE with samples and OS-specific details.
### Changed
### Fixed
### Removed

