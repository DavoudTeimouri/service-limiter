# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]
### Added
### Changed
### Fixed
### Removed

## [2.1.0-rc.1] - 2026-10-22

Pre-release. Test it before it is promoted to a stable release.

### Added
- `--durable` now works on Windows. The Job Object limits used to lapse the moment the apply script exited, because the handle closed with it. The script now takes a `-Durable` switch that keeps the handle open, and `apply` keeps that holder process alive and prints its PID. The script also writes a status file, because in durable mode it never exits and its exit code can never be waited for -- without that, a failed apply would have been reported as a success
- `generate --io-device PATH`: sets the block device for the I/O bandwidth limits without editing the profile. A path that does not exist is refused, because systemd ignores a limit on a missing device and the drop-in would install cleanly while enforcing nothing
- `service-limiter revert --config DIR`: undoes an `apply`. Restores the `.bak` that `apply` wrote, or deletes the drop-in when there was no original, then reloads systemd and restarts the unit. Supports `--dry-run` and `--yes`
- `service-limiter status --config DIR`: shows which staged services currently have limits installed and whether a restorable backup exists
- `analyze` now prints the measured CPU, memory and I/O for every service, and lists the services over the policy separately with the values that put them there. It previously printed only the first five service names with no measurements, so the user could not tell which services were the problem
- Audit log restored in the Rust build: JSONL at `~/.service-limiter/audit.log`, covering `analyze`, `generate`, `apply`, and the failure paths `analyze_error`, `generate_error`, `apply_error`, `apply_denied`. Failed runs are recorded, not just successful ones. `SERVICE_LIMITER_HOME` relocates it. Values are JSON-escaped so a crafted service name cannot forge a line
- `ResourceProfile.cpu_percent_percore`: the raw per-core figure, alongside the normalized machine share
- A docs-freshness test fails the build when README.md or LOGIC.md names a `platform::` symbol that no longer exists, or when a CLI subcommand is not documented

### Changed
- Both platform modules are now compiled on every target, and the platform is chosen at runtime. A `#[cfg]`-gated module is compiled by exactly one CI job, so the Windows code was never type-checked by the Linux job and the Windows-only build breaks it found (a missing `HashMap` import, an unused variable, an unused `mut`) went unnoticed until a Windows runner hit them. The same applies to the Linux and Windows installers in `apply.rs`, which are plain `std::process` and `std::fs` and compile anywhere
- Services are measured over several sampling windows instead of one, and the reported figure is the worst window rather than the average. A single 250 ms window missed short spikes, and averaging would have averaged away a sustained one
- All services share each sampling window. Sampling each service separately cost a full process-table scan and a sleep per service, so profiling 50 services spent over 12 seconds asleep
- Service PIDs are resolved in one batch (`systemctl show` with all units, or one CIM query) instead of one process per service. On Windows this removed a PowerShell startup per service, which cost hundreds of milliseconds each
- `cpu_percent` is now a consistent whole-machine share. Both the measurement (divided by core count) and the generated limits (multiplied by it) are normalized, so the limit matches the policy that produced it
- `generate` removes staged configs for services that are no longer over the policy. Leaving them behind meant `apply` kept installing limits for a service that had come back under the limit. Files this tool did not create are never touched
- The Windows apply path is now compiled on every platform, not just Windows. It is plain `std::process`, so building it on Linux type-checks it in CI, where it was previously never compiled at all
- The audit log's JSON escaping uses `serde_json` instead of a hand-rolled escaper, which is where a JSON-injection bug hides. The key order is unchanged

### Fixed
- `apply --dry-run` and `revert --dry-run` demanded root. A dry run changes nothing, so it must not need privileges; requiring sudo to preview a change is the wrong contract and made the preview unusable from a normal shell
- `apply --dry-run` did not validate the staged config. It now runs the same `systemd-analyze` gate as a real apply and names the offending key, so a broken config is reported before it is installed rather than after
- `apply` ignored `systemctl`'s exit code, so a failed `daemon-reload` or `restart` was reported as "restarted {service}". Both are now checked and a non-zero exit fails the service
- `apply` overwrote the existing `override.conf.bak` on every run, so a second `apply` destroyed the only copy of the original config. An existing backup is now left untouched
- The `systemd-analyze` validation gate reported "systemd rejected this config" even when the real cause was that `systemd-analyze` was not installed. The two cases are now distinguished
- The test that asserts an invalid drop-in is rejected passed for the wrong reason when `systemd-analyze` was missing, and did not check that the offending key was named. Both are fixed
- The process-tree walk used a `Vec` for its visited set, making it quadratic in the size of the process table. It is now a `HashSet`
- The Windows documentation claimed that only the main PID was measured. The measurement walks the full child tree on both platforms; what differs is which processes the *limit* covers
- CPU limits were far tighter than the policy that set them: `CPUQuota={cpu_percent}%` was emitted while `cpu_percent` read as a whole-machine share, but systemd measures `CPUQuota` per-CPU. On a 32-core host a `50` policy throttled a service to 1.6% of the machine. Windows `CpuRate` had the same mismatch. Core count comes from `available_parallelism()`, which respects the cgroup CPU quota, so a container capped at 1 CPU scales correctly instead of emitting a quota the cgroup can never grant

### Removed
- The `windows-sys` dependency. It was declared but never referenced: the Windows path shells out to PowerShell, which calls the Win32 Job Object APIs itself through `Add-Type`. It pulled in a large dependency tree for nothing

## [2.0.0] - 2026-10-09

Rust rewrite. No interpreter, no runtime dependencies, ~1 MB static binary.
See [MIGRATING.md](MIGRATING.md) if you used the Python 1.x CLI.

### Breaking changes
- Install method changed: `pip install service-limiter` no longer works. Use a release binary (`service-limiter-linux-x86_64.tar.gz`, `service-limiter-linux-aarch64.tar.gz`, `service-limiter-windows-x86_64.zip`, each with a `.sha256`) or `cargo install --git https://github.com/DavoudTeimouri/service-limiter.git`. The PyPI package stays frozen at 1.0.0
- The Python implementation, `pyproject.toml`, and the curses TUI are removed
- `--durable` is accepted but not implemented in the Rust build; it warns instead of silently doing nothing new

### Unchanged on purpose
- Command name and all flags: `service-limiter {analyze,generate,apply}` with `--profile`, `--output`, `--config`, `--dry-run`, `--yes`
- Exit codes: 0 ok, 1 violation, 2 error, 3 needs-root
- Profile JSON schema
- Staging layout `<output>/<service>/override.conf` and `<output>/<service>/Set-JobLimits.ps1`
- Generated systemd drop-in text, byte for byte

### Added
- Rust implementation: clap CLI, serde models, sysinfo-based process-tree measurement, PowerShell Job Object control on Windows
- `platform::sample_trees` shared by both platforms, so measurement cannot drift between them
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
- `--durable` is not implemented in the Rust build yet; limits lapse when the process exits
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

