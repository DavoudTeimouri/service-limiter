# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]
### Added
### Changed
### Fixed
### Removed

## [1.0.0] - 2026-09-30
### Added
- Real Linux service profiling using psutil + systemctl MainPID (replaces fabricated data)
- Real Windows service discovery via Get-CimInstance Win32_Service (with PID)
- Real Windows profiling reusing the Linux psutil measurement logic
- systemd I/O bandwidth limits using correct directives: IOReadBandwidthMax / IOWriteBandwidthMax per-device
- Windows Job Object limits via Add-Type P/Invoke (CreateJobObject / SetInformationJobObject / AssignProcessToJobObject)
- generate command now writes files to --output directory matching apply's expected layout
- Profile loading from package data (importlib.resources), available after pip install
- Exit codes: 0 OK, 1 violation, 2 error, 3 needs-root
- Audit log: JSONL append to ~/.service-limiter/audit.log
- Safety gates for apply: root check, backup existing drop-in, y/N confirmation, systemd-analyze verify before daemon-reload
- --yes flag to skip confirmation prompt
- Missing profile is a hard error (no silent fallback to defaults)
- TUI pairs services to profiles by name (was by index, showed wrong numbers)
- 20 assert-based tests locking in regressions (IO directives, TUI pairing, profile loading, exit codes)
- CI workflow: matrix Python 3.8/3.11/3.12 on Linux + Windows

### Changed
- Package metadata: explicit packages + package-data so profiles ship in the wheel
- Removed dead code: config_generator.py (duplicate of orchestrator), dummy-service fabrication, hardcoded profiles
- Package installs now (fixed flat-layout error from profiles/ at repo root)
- ServiceDescriptor constructor accepts optional fields with defaults (fixes Windows 3-arg crash)
- WindowsResourceProfiler returns Dict[str, ResourceProfile] (was List, crashed orchestrator)
- apply --dry-run preserved; apply_windows uses pwsh if available, falls back to powershell

### Fixed
- Silent no-op I/O limits: systemd accepted the drop-in but ignored ReadBandwidthMax/WriteBandwidthMax (wrong directives)
- generate --output was accepted and ignored, breaking the documented generate->apply workflow
- Windows PowerShell script used non-existent cmdlets (New-JobObject, Set-JobObject, Get-JobObject, Remove-JobObject)
- CPUQuota=80% means 80% of ONE CPU (systemd semantics), not 80% of machine (policy semantics)
- dummy-service fabrication on non-systemd hosts reported fake results as real
- All 25 tracked .pyc/egg-info/backup files removed; .gitignore added
- Windows discovery called ServiceDescriptor with 3 of 6 required args (TypeError)
- Profiling stub returned [] on Windows, crashed orchestrator when .values() called
- psutil declared but never imported; now used for real measurement

### Removed
- config_generator.py (dead duplicate, also source of SyntaxWarning)
- dummy-service fallback and its hardcoded profile numbers
- orchestrator.py.backup and all committed bytecode/egg-info

## [0.1.0] - 2026-09-24
### Added
- Initial commit: service-limiter skeleton
- Cross-platform service discovery (Windows WMI/PowerShell, Linux systemctl)
- Resource profiling using psutil
- Policy engine with default thresholds (CPU 80%, Memory 512MB, Read IO 1MB/s, Write IO 512KB/s)
- OS-specific configuration generation (Linux systemd overrides, Windows Job Object PowerShell scripts)
- CLI with commands: analyze, generate, apply, tui (Linux only)
- TUI for Linux using curses
- Profile support (e.g., web-server.json)
- Human-in-the-loop design for applying configurations
- Documentation: README, USER_GUIDE, ARCHITECTURE, STRUCTURE, GITHUB_SETUP

