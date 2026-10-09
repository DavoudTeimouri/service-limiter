# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]
### Added
- Audit log restored in the Rust build: JSONL at `~/.service-limiter/audit.log`, covering `analyze`, `generate`, `apply`, and the failure paths `analyze_error`, `generate_error`, `apply_error`, `apply_denied`. Failed runs are recorded, not just successful ones. `SERVICE_LIMITER_HOME` relocates it. Values are JSON-escaped so a crafted service name cannot forge a line
- `ResourceProfile.cpu_percent_percore`: the raw per-core figure, alongside the normalized machine share

### Changed
- `cpu_percent` is now a consistent whole-machine share. Both the measurement (divided by core count) and the generated limits (multiplied by it) are normalized, so the limit matches the policy that produced it

### Fixed
- CPU limits were far tighter than the policy that set them: `CPUQuota={cpu_percent}%` was emitted while `cpu_percent` read as a whole-machine share, but systemd measures `CPUQuota` per-CPU. On a 32-core host a `50` policy throttled a service to 1.6% of the machine. Windows `CpuRate` had the same mismatch. Core count comes from `available_parallelism()`, which respects the cgroup CPU quota, so a container capped at 1 CPU scales correctly instead of emitting a quota the cgroup can never grant

### Removed

## [2.0.0] - 2026-10-09

Rust rewrite. No interpreter, no runtime dependencies, ~1 MB static binary.
See [MIGRATING.md](MIGRATING.md) if you used the Python 1.x CLI.

### Breaking changes
- Install method changed: `pip install service-limiter` no longer works. Use a release binary (`service-limiter-linux-x86_64.tar.gz`, `service-limiter-linux-aarch64.tar.gz`, `service-limiter-windows-x86_64.zip`, each with a `.sha256`) or `cargo install --git https://github.com/DavoudTeimouri/service-limiter.git`. The PyPI package stays frozen at 1.0.0
- The Python implementation, `pyproject.toml`, and the curses TUI are removed
- `--durable` is accepted but not implemented in the Rust build; it warns instead of silently doing nothing new
- The audit log is not written yet. `~/.service-limiter/audit.log` was written by 1.x and is not written by 2.0.0

### Unchanged on purpose
- Command name and all flags: `service-limiter {analyze,generate,apply}` with `--profile`, `--output`, `--config`, `--dry-run`, `--yes`
- Exit codes: 0 ok, 1 violation, 2 error, 3 needs-root
- Profile JSON schema
- Staging layout `<output>/<service>/override.conf` and `<output>/<service>/Set-JobLimits.ps1`
- Generated systemd drop-in text, byte for byte

### Added
- Rust implementation: clap CLI, serde models, sysinfo-based process-tree measurement, `windows-sys` Job Object FFI on Windows
- `platform::sample_tree` shared by both platforms, so measurement cannot drift between them
- `src/job_limits.ps1` is a real file embedded with `include_str!`, shared verbatim with 1.x rather than duplicated
- Integration tests pinning the exit-code contract
- `apply` validates a drop-in with `systemd-analyze verify` and refuses before touching `/etc` if systemd rejects it
- Sample rates divide by measured elapsed time rather than the requested interval
- Release binaries for linux-x86_64, linux-aarch64, and windows-x86_64 with sha256 checksums, built by CI
- CI runs fmt, clippy `-D warnings`, and tests on both Linux and Windows; a cross-compile check covers the Windows target on Linux

### Changed
- CPU and memory limits below 1 clamp to 1, so `CPUQuota=0%` (which systemd reads as never run) is never emitted
- Profiles are compiled into the binary with `include_str!`; a missing profile lists what is available
- `analyze` skips and reports services it cannot measure rather than recording zero usage
- Platform dispatch is `cfg`-gated rather than runtime `if/elif` ladders

### Fixed
- Windows memory limit never applied: 1.x set `0x20` (`JOB_OBJECT_LIMIT_PRIORITY_CLASS`) and `0x400` (`DIE_ON_UNHANDLED_EXCEPTION`) while commenting them as `JOB_MEMORY|JOB_IO_RATE`. The real `JOB_OBJECT_LIMIT_JOB_MEMORY` is `0x00000200`
- Windows I/O limits were written into `JOBOBJECT_EXTENDED_LIMIT_INFORMATION.IoInfo`, which holds read-only `IO_COUNTERS` accounting values, making the limit a silent no-op. 2.0.0 uses `JOBOBJECT_IO_RATE_CONTROL_INFORMATION`
- Windows applied no CPU limit at all: 1.x computed `cpu_rate` in Python and never interpolated it
- Windows `apply` no longer reports success when `AssignProcessToJobObject` fails; it exits non-zero
- The test that let the flag bugs ship only grepped generated text for API names. 2.0.0 asserts the actual flag value, that `IO_COUNTERS` is never written, that `CpuRate` is interpolated and clamped, and that every struct the script instantiates is declared

### Removed
- Python implementation (`service_limiter/`), `pyproject.toml`, and its test suite, after the Rust suite matched it
- The curses TUI: Linux-only, read-only, and duplicating what `analyze` prints
- Root `profiles/` duplicate that was never loaded
- `architecture.md`, `STRUCTURE.md`, `GITHUB_SETUP.md`, and `USER_GUIDE.md` (superseded by README and LOGIC.md)
- `SharedState` (a mutable bag with a single consumer)

### Known limitations
- `--durable` and the audit log are not implemented in the Rust build yet
- `CPUQuota` is a share of one CPU while `cpu_percent` reads as a whole-machine share; still unresolved
- Linux I/O limits require an `io_device` in the profile
- Windows measures only the service's main PID; Linux walks the child tree
- Windows limits are not durable unless a future release adds a handle holder

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

