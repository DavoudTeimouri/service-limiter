# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]
### Added
- `apply --durable` (Windows): holds the Job Object handle open in a background process so limits do not lapse when the script exits
- Logged every analyze/generate/apply event to `~/.service-limiter/audit.log` as JSONL
- CONTRIBUTING.md, SECURITY.md, CODE_OF_CONDUCT.md, issue and pull request templates
- LOGIC.md: durable reference for how the tool works and the domain traps it respects

### Changed
- Windows Job Object limits now use the documented Win32 information classes instead of a single mislabelled flag field
- `apply` without `--durable` now says out loud that Windows limits will lapse
- README rewritten against the real CLI: corrected systemd directives, added exit codes, audit log, and an honest platform matrix
- USER_GUIDE.md folded into README and removed (was ~90% duplicate)
- profiles are loaded from package data; the repo-root profiles/ duplicate is deleted

### Fixed
- Windows memory limit never applied: LimitFlags used 0x20 (JOB_OBJECT_LIMIT_PRIORITY_CLASS) and 0x400 (DIE_ON_UNHANDLED_EXCEPTION) while commented JOB_MEMORY|JOB_IO_RATE. Real JOB_OBJECT_LIMIT_JOB_MEMORY is 0x00000200
- Windows I/O limits were written into JOBOBJECT_EXTENDED_LIMIT_INFORMATION.IoInfo, which holds read-only IO_COUNTERS accounting counters and is a silent no-op. Now uses JOBOBJECT_IO_RATE_CONTROL_INFORMATION
- Windows applied no CPU limit at all: cpu_rate was computed in Python but never interpolated into the script
- PowerShell script no longer claims success when AssignProcessToJobObject fails; it exits 2
- `tui` on a non-Linux host exited 0 on failure; now exits 2
- `generate` exited a bare literal 1 when nothing exceeded the policy, which aborted `set -e` scripts on a clean run; now exits 0
- Every SLJob struct the script instantiates is now declared in the C# shim
- 5 new Windows tests assert the real flag value, that IO_COUNTERS is never written, that CpuRate is interpolated, that it clamps at 10000, and that structs are declared. The previous test only grepped for API names, which is how these bugs shipped

### Removed
- architecture.md, STRUCTURE.md, GITHUB_SETUP.md (stale; described a ConfigGenerator, subagents, and a Textual TUI that no longer exist)
- USER_GUIDE.md (folded into README)
- Repo-root profiles/ directory (byte-identical duplicate that is never loaded)

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
- Documented the CPUQuota per-single-CPU semantics (not fixed in code; see Known issues in LOGIC.md)
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

