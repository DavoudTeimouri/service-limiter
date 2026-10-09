# Service Limiter — Project Logic

Authoritative description of **how the tool works and why**. Written before the Rust
rewrite so both implementations share one reference.

**Status**: describes the Rust implementation (2.0.0). The previous Python
implementation was 1.0.0; both are covered under
[Compatibility contract](#9-compatibility-contract) and [MIGRATING.md](MIGRATING.md).

Every claim here is verified by a named test or a runnable command. See
[Keeping this file honest](#keeping-this-file-honest).

---

## 1. What the tool does

Discovers OS services, measures their **real** CPU / memory / I/O usage, compares that
against a policy, and generates OS-native limits. On Linux it writes systemd drop-ins.
On Windows it generates a Job Object PowerShell script.

It **generates and stages** configs. `apply` installs them. Nothing is changed on disk
until `apply` runs.

---

## 2. Pipeline

```
analyze / generate
        │
        ├─ 1. detect platform          platform/detector.py
        │
        ├─ 2. discover services       platform/{linux,windows}.py
        │     Linux:   systemctl list-units --type=service --state=running
        │     Windows: Get-CimInstance Win32_Service
        │                             → [ServiceDescriptor]
        │
        ├─ 3. profile resources       platform/{linux,windows}.py
        │     resolve MainPID / service PID
        │     sample process tree across an interval
        │                             → Dict[name, ResourceProfile]
        │
        ├─ 4. evaluate policy         policy_engine.py
        │     strict > on all four metrics
        │                             → services over policy
        │
        └─ 5. generate configs        orchestrator.py
              Linux:   systemd drop-in text
              Windows: Job Object PowerShell text
                                        → Dict[name, config]
```

Steps 1–4 read only. Nothing writes to a system path until `apply`.

### 2.1 Platform dispatch

`cfg(target_os = "linux" | "windows")` gates the modules, and each is re-exported
under the same name:

```rust
#[cfg(target_os = "linux")]
pub use linux::{discover_services, main_pid, DEFAULT_INTERVAL};
#[cfg(target_os = "windows")]
pub use windows::{discover_services, main_pid, DEFAULT_INTERVAL};
```

Call sites read `platform::discover_services()` with no match and no `cfg` noise,
and no trait is needed because there is only ever one implementation compiled.

### 2.2 Discovery

**Linux** — one `systemctl list-units` call:

```
sshd.service loaded active running OpenSSH server daemon
  2 loaded units listed.
```

| Field | Source |
|---|---|
| `name` | `parts[0]` minus `.service` |
| `start_type` | `parts[1]` |
| `status` | `parts[2]` |
| `display_name` | `" ".join(parts[4:])` |

Lines without a `.service` suffix are skipped. This is load-bearing: systemctl emits the
trailing `N loaded units listed.` summary even with `--no-legend`, and parsing it
produced a bogus service named `2`.
→ `platform::linux::tests::trailing_summary_line_is_not_a_service`

When `systemctl` is missing, discovery returns `[]` and logs an error. It never invents a
placeholder service. (a missing `systemctl` is an `Err`, not a fabricated service)

**Windows** — `Get-CimInstance Win32_Service` selecting `Name, DisplayName, State,
StartMode, PathName, ProcessId, StartName`, `ConvertTo-Json -Compress`.
A single service serialises as an object, so a dict result is wrapped into a list.
The `ProcessId` is stored in `ServiceDescriptor.child_processes`, which is how the
Windows profiler reaches the process without a second query.

`Get-CimInstance`, not `Get-WmiObject` — the latter is deprecated.

### 2.3 Profiling — the measurement

The only place real numbers are produced. Shared by both platforms: `sysinfo`
already abstracts `/proc` and the Windows process APIs, so
`platform::sample_tree` in `src/platform/mod.rs` is the single implementation.

```rust
pub fn sample_tree(root: u32, interval: Duration) -> Option<Sample>
```

1. `refresh_processes_specifics` to prime the process table.
2. Sleep `interval` (default **250 ms**).
3. Refresh again and diff.

**Why prime first**: `cpu_usage()` and `disk_usage()` are deltas between
refreshes. The first call establishes the baseline; a single call returns
garbage.

3. Collect the process tree: build a `ppid -> children` map and walk it with an
   **iterative** DFS. Iterative so a deep tree cannot blow the stack, and a
   `seen` set guards against pid-reuse cycles. A service's limits apply to its
   whole tree, not just the main process.
4. Sum `cpu_usage()`, `memory()` (RSS), and `disk_usage()` read/written bytes.

**Units**:

| Metric | Unit |
|---|---|
| `cpu_percent` | summed percent across the tree; per-core, so it can exceed 100 on multi-core |
| `memory_mb` | RSS bytes ÷ 1024² |
| `io_read_kbps` | read bytes ÷ 1024 ÷ **measured elapsed seconds** |
| `io_write_kbps` | write bytes ÷ 1024 ÷ **measured elapsed seconds** |

I/O is divided by `Sample::elapsed_secs` — the window the sampler actually
observed, not the requested `interval`. Dividing by the nominal value skews the
rate whenever the process was descheduled.

**Caveats** — honest limits of this method:
- A single interval is a point sample, not a trend. Short-lived spikes are missed.
- Children that spawn and exit inside the window are missed.
- CPU percent is process-tree summed, not cgroup accounting, so it does not match
  `systemd-cgtop`.
- Windows measures the service's main PID only; child processes are not walked,
  so a Windows reading covers strictly less than the Linux reading for the same
  service.
- A process the sampler cannot stat is skipped. Root-owned services generally
  need elevation.

A service that cannot be measured is **skipped and reported**, never recorded as
zero usage — zero would read as "well within policy", which is a different and
wrong claim.

---

## 3. Policy semantics

`Policy::default()` in `src/models.rs`:

```rust
Policy { name: "default", cpu_percent: 80.0, memory_mb: 512.0,
         io_read_kbps: 1024.0, io_write_kbps: 512.0, io_device: None }
```

`policy::evaluate(&profile, &policy) -> (bool, Vec<String>)` applies **strict `>`** to
all four keys and reports every violation, not just the first. Exactly at the limit
passes. → `policy::tests::limit_is_inclusive`

Profiles are JSON overrides with the same four keys. A missing or malformed profile is a
**hard error**, never a silent fallback to defaults: a limiter that quietly applies
different limits than the operator asked for is worse than refusing to run.
→ `profile::tests::missing_profile_is_an_error_not_a_fallback`

### 3.1 CPU units — how `cpu_percent` is interpreted

`cpu_percent` is a **share of the whole machine, 0–100**. One saturated core of a 4-core
box is 25.

Both the measurement and the enforcement are normalized, so a limit means what the
operator wrote:

| Layer | Native unit | Normalized to |
|---|---|---|
| Sampler (`sysinfo`) | per-core; 100 == one saturated core | ÷ `cpu_count()` |
| systemd `CPUQuota=` | per-CPU; 100 == one core | × `cpu_count()` |
| Windows `CpuRate` | per-CPU, in 1/10000 units | × `cpu_count()`, clamped at 10000 |

So `cpu_percent: 50` on a 4-core box emits `CPUQuota=200%`.

> **This was wrong until 2.0.1.** Earlier versions emitted `CPUQuota={cpu_percent}%`
> while `cpu_percent` read as a whole-machine share, so the measurement and the
> enforced limit described different quantities and the limit was far tighter than
> the policy it came from — on a 32-core box a `50` policy throttled a service to
> 1.6% of the machine.

**`cpu_count()` is `available_parallelism()`, not `nproc`.** That respects the cgroup
CPU quota, which is the right denominator: a container capped at 1 CPU reports 1
even on a 32-core host. Scaling by host cores would emit a quota the cgroup could
never grant, so the limit could never bind. On this container: `nproc` says 2
(scheduler affinity), cgroup `cpu.max` says 1, so `cpu_count()` returns 1.

`ResourceProfile` carries both figures: `cpu_percent` (machine share, the one the
policy compares) and `cpu_percent_percore` (raw, better for diagnosing which core is
hot).

Tests assert the *relationship*, not a literal, so they hold on any machine:
`config_gen::tests::cpu_quota_is_machine_share_converted_to_per_cpu`.

---

## 4. Generated configuration

### 4.1 Linux — systemd drop-in

Target: `/etc/systemd/system/<service>.service.d/override.conf`

```
[Service]
CPUQuota=50%
MemoryMax=256M
IOAccounting=yes
IOReadBandwidthMax=/dev/sda1 524288
IOWriteBandwidthMax=/dev/sda1 262144
```

| Directive | Value |
|---|---|
| `CPUQuota` | `cpu_percent × cpu_count`% — systemd counts per-CPU, see §3.1 |
| `MemoryMax` | `<memory_mb>M` |
| `IOAccounting` | `yes`, required for I/O accounting |
| `IOReadBandwidthMax` | `<io_device> <io_read_kbps × 1024>` bytes/s |
| `IOWriteBandwidthMax` | `<io_device> <io_write_kbps × 1024>` bytes/s |

**`MemoryMax`, not `MemoryLimit`** — `MemoryLimit=` is deprecated; systemd 257 warns
*"Unit uses MemoryLimit=; please use MemoryMax= instead."*

**I/O limits are skipped unless the policy has `io_device`.** systemd requires a
block-device path per limit. Without one the generator logs a warning and omits the
directives rather than emitting something systemd ignores.
→ `config_gen::tests::io_skipped_with_warning_when_no_device`

> **Silent no-op, fixed.** The earlier code emitted `ReadBandwidthMax=` and
> `WriteBandwidthMax=`. Those are **not** systemd keys. `systemd-analyze verify` reports
> `Unknown key 'ReadBandwidthMax' in section [Service], ignoring.` — `daemon-reload`
> succeeded, the service restarted, and **no I/O limit was ever applied**. Verified
> against systemd 257 by diffing old vs new output.
> → `config_gen::tests::io_directives_use_real_systemd_names`, `config_gen::tests::bogus_legacy_io_directives_are_absent`

### 4.2 Windows — Job Object script

`generate` stages `Set-JobLimits.ps1`. `apply` runs it via `pwsh` (preferred) or
`powershell`. Must be elevated.

The script `Add-Type`s a C# shim and P/Invokes `kernel32`:

| API | Purpose |
|---|---|
| `CreateJobObject` | create the job |
| `SetInformationJobObject` | class 9, `JOBOBJECT_EXTENDED_LIMIT_INFORMATION` |
| `AssignProcessToJobObject` | bind the service's PID |
| `OpenProcess` | open the service process |

Limits set: `JOB_OBJECT_LIMIT_JOB_MEMORY` via `JobMemoryLimit`,
read/write transfer caps via `IoInfo`. Service PID resolved with
`Get-CimInstance Win32_Service -Filter "Name='<service>'"`.

> **The old cmdlets never existed.** The previous generator called
> `New-JobObject`, `Set-JobObject`, `Get-JobObject`, `Remove-JobObject`. PowerShell ships
> **no such cmdlets** — Job Objects are a Win32 kernel API. Every call failed, and the
> script's own `catch` blocks converted each failure into `Write-Warning`, so it
> *looked* successful while creating nothing.
> (`Get-Job` exists but is an unrelated PowerShell background job.)
> → `config_gen::tests::windows_script_has_no_invented_cmdlets`, `config_gen::tests::windows_memory_flag_is_the_real_one`

**Known weak enforcement — unresolved.** A Job Object limit **lapses when its handle is
released**, and the script exits. The generated script says so itself. Also, if the
service already runs inside a job object (common under IIS or a service host),
`AssignProcessToJobObject` fails and only a warning is emitted.

Durable options, none implemented: a long-running broker that holds the handle, a service
wrapper that owns the lifetime, or scheduled re-apply. Any Rust rewrite should pick one
explicitly rather than emit a one-shot script.

---

## 5. Safety model

`apply` is the only privileged path. Gates, in order:

| Gate | Behaviour |
|---|---|
| Config dir exists | else exit `2` |
| Root (Linux) | `os.geteuid() != 0` → exit `3` |
| Per-service confirm | `y/N`, unless `--yes` |
| `--dry-run` | prints planned writes, touches nothing, exits early |
| `systemd-analyze verify` | rejects on `Unknown key` / `Invalid`; **refuses before touching `/etc`** |
| Backup | existing `override.conf` copied to `.bak` before overwrite |
| Audit log | JSONL appended to `~/.service-limiter/audit.log` |

Ordering matters: validation runs **before** the first write to a system path, so a
malformed drop-in never lands in `/etc` half-applied.

### 5.1 Exit codes

| Code | Meaning |
|---|---|
| `0` | ran clean, nothing over policy |
| `1` | at least one service exceeded the policy |
| `2` | runtime/environment failure — no systemd, nothing profiled, unreadable config |
| `3` | `apply` requires elevation |

`2` matters: exit `0` must never be reachable when the tool could not actually measure
anything. Zero measurable services is a failure, not a clean run.

### 5.2 Audit log

One JSON object per line at `~/.service-limiter/audit.log`:

```json
{"event": "analyze", "time": "2026-10-09T09:51:42+0000", "discovered": "2", "profiled": "2", "over_policy": "0"}
{"event": "generate", "time": "...", "output": "./config", "count": "1"}
{"event": "apply", "time": "...", "config": "./config", "dry_run": "true", "applied": "1", "skipped": "0", "failed": "0"}
{"event": "apply_denied", "time": "...", "reason": "not_root", "config": "./config"}
{"event": "analyze_error", "time": "...", "error": "systemctl not available: ...", "profile": "default"}
```

**Events**: `analyze`, `generate`, `apply`, and the failure paths
`analyze_error`, `generate_error`, `apply_error`, `apply_denied`. A run that
fails is recorded too — an audit log that only shows successes hides exactly the
runs you would want to review.

Two deliberate properties:

- **Failure to write never fails the operation.** An unwritable log warns on
  stderr; it does not stop a limit from being applied. Losing an audit line is
  bad, refusing to apply because of it is worse.
- **Values are JSON-escaped.** A crafted service name cannot break out of the
  string and forge a log line. Covered by `audit::tests::escapes_hostile_values`.

Set `SERVICE_LIMITER_HOME` to relocate the log (also keeps tests off the real
home directory).

No rotation, no locking — a deliberate simplification. Add rotation when the
file actually grows large.

Timestamps are UTC, formatted by hand (Howard Hinnant's `civil_from_days`) to
avoid a date dependency. `audit::tests::civil_conversion_matches_known_dates`
pins the conversion against known dates.

---

## 6. Profile JSON

```json
{
  "cpu_percent": 50,
  "memory_mb": 256,
  "io_read_kbps": 512,
  "io_write_kbps": 256,
  "io_device": "/dev/sda1"
}
```

| Key | Required | Meaning |
|---|---|---|
| `cpu_percent` | yes | CPU share of the whole machine, 0–100, see §3.1 |
| `memory_mb` | yes | max RSS, MB |
| `io_read_kbps` | yes | read bandwidth, KB/s |
| `io_write_kbps` | yes | write bandwidth, KB/s |
| `io_device` | **no** | block device path; without it I/O limits are skipped |

Loaded via `importlib.resources` from inside the package, so profiles work identically
from a wheel, a venv, or a source checkout.

> **Packaging trap, fixed.** Profiles originally sat at the repo root while the loader
> read `../profiles`, and no `package-data` was declared. Two consequences: the package
> would not build at all (setuptools *"Multiple top-level packages discovered in a
> flat-layout: ['profiles', 'service_limiter']"*), and a pip-installed user had no
> profiles to load. Fixed with explicit `packages` + `package-data`.

> **Duplicate, still present.** `profiles/web-server.json` and
> `service_limiter/profiles/web-server.json` are byte-identical duplicates. Only the
> package copy is loaded; the root one is redundant and should go.

---

## 7. Module map

| Module | Responsibility |
|---|---|
| `main.rs` | clap CLI, subcommands, exit codes |
| `orchestrator.rs` | the pipeline; builds configs for services over policy |
| `policy.rs` | thresholds and `evaluate()` |
| `config_gen.rs` | systemd drop-in + Windows script text |
| `apply.rs` | privileged install path, validation, backup, confirmation |
| `audit.rs` | JSONL audit log at `~/.service-limiter/audit.log` |
| `platform/mod.rs` | `cfg` dispatch and the shared process-tree sampler |
| `platform/linux.rs` | systemctl discovery, MainPID, output parser |
| `platform/windows.rs` | CIM discovery, PowerShell shell detection |
| `models.rs` | `ServiceDescriptor`, `ResourceProfile`, `Policy` |
| `profile.rs` | bundled profiles, compiled in via `include_str!` |
| `job_limits.ps1` | the Windows script template, shared verbatim with 1.x |

**Platform dispatch is `cfg`, not traits.** One implementation is compiled per
target, so a trait would add dispatch with no polymorphism to choose between.

**The Windows script template is a real file**, `src/job_limits.ps1`, embedded
with `include_str!`. It was byte-identical to what Python generated, so it is
shared rather than duplicated, and the two implementations cannot drift on the
Win32 flag values.

## 8. TUI

Removed in 2.0.0. The curses TUI was Linux-only, read-only, and displayed the
same data `analyze` already prints. Porting it to ratatui would have cost two
dependencies and ~200 lines to reproduce existing output.

## 9. Compatibility contract

The Rust rewrite must preserve all of this. Anything that changes it is a breaking change
belonging in `2.0.0` with a CHANGELOG entry.

**CLI** — `service-limiter {analyze,generate,apply,tui}`

| Flag | Command | Meaning |
|---|---|---|
| `--profile NAME` | analyze, generate | profile from bundled JSON |
| `--output DIR` | generate | staging directory (default `./config`) |
| `--config DIR` | apply | staged directory to install |
| `--dry-run` | apply | print, change nothing |
| `--yes` | apply | skip confirmation |

**Guarantees**
1. Exit codes `0` / `1` / `2` / `3` keep their meanings (§5.1).
2. Generated systemd text stays byte-compatible where valid (§4.1).
3. Profile JSON schema unchanged (§6).
4. Layout `<output>/<service>/override.conf` and `<output>/<service>/Set-JobLimits.ps1`.
5. Audit log stays JSONL at `~/.service-limiter/audit.log`.
6. A missing profile is fatal, not a fallback.
7. Zero measurable services exits `2`.

**Deliberately changed in 2.0.0**: install method (`pip` → `cargo install` /
release binary), TUI removed, `--durable` not yet implemented, audit log not yet
implemented. See [MIGRATING.md](MIGRATING.md).

---

## 10. Keeping this file honest

| Claim | Test |
|---|---|
| systemd directives correct | `config_gen::tests::io_directives_use_real_systemd_names` |
| legacy directives gone | `config_gen::tests::bogus_legacy_io_directives_are_absent` |
| summary line not a service | `platform::linux::tests::trailing_summary_line_is_not_a_service` |
| CPU quota normalized per-CPU | `config_gen::tests::cpu_quota_is_machine_share_converted_to_per_cpu` |
| 100% policy allows all cores | `config_gen::tests::full_machine_policy_allows_all_cores` |
| `CPUQuota=0%` never emitted | `config_gen::tests::sub_one_cpu_limit_never_becomes_zero` |
| audit line is valid JSON | `audit::tests::renders_a_json_object` |
| audit escapes hostile values | `audit::tests::escapes_hostile_values` |
| audit writes one line per event | `audit::tests::writes_one_line_per_event` |
| audit timestamps | `audit::tests::civil_conversion_matches_known_dates` |
| drop-in path | `config_gen::tests::drop_in_path_is_service_dot_service_d` |
| I/O skipped without a device | `config_gen::tests::io_skipped_with_warning_when_no_device` |
| no invented cmdlets | `config_gen::tests::windows_script_has_no_invented_cmdlets` |
| real Win32 classes used | `config_gen::tests::windows_script_uses_real_win32_classes` |
| Windows memory flag is 0x200 | `config_gen::tests::windows_memory_flag_is_the_real_one` |
| IO_COUNTERS never written | `config_gen::tests::windows_io_limits_never_go_into_io_counters` |
| CPU rate applied and clamped | `config_gen::tests::windows_cpu_rate_is_applied_and_clamped` |
| profiler returns a map | enforced by the type system; no test needed |
| policy boundaries | `policy::tests::{limit_is_inclusive, over_limit_reports_every_resource}` |
| profiles load from the binary | `profile::tests::bundled_profile_loads` |
| missing profile is fatal | `profile::tests::missing_profile_is_an_error_not_a_fallback` |
| measurement returns real data | `platform::linux::tests::samples_this_process_with_real_usage` |
| exit codes | `tests/cli.rs` |
| dry-run changes nothing | `tests/cli.rs::apply_dry_run_touches_nothing` |
| invalid drop-in is refused | `tests/cli.rs::apply_rejects_a_config_systemd_would_ignore` |

Run them:

```bash
cargo test
```

Claims **not** covered by a test, and how to verify by hand:

| Claim | Verify |
|---|---|
| systemd accepts the drop-in | `systemd-analyze verify <unit>` must print no `Unknown key` |
| Windows script runs | elevated PowerShell on a real Windows host |
| Windows limits actually bind | `QueryInformationJobObject` readback on a real host |
| CPU sampling is accurate | compare `analyze` against `systemd-cgtop` / Task Manager |

## 11. Known open problems

| # | Problem | Impact | Notes |
|---|---|---|---|
| 1 | ~~`CPUQuota` per-CPU vs whole-machine `cpu_percent`~~ | **fixed in 2.0.1**, both sides now normalized | §3.1 |
| 2 | Windows Job Object limits lapse when the script exits | enforcement is transient | §4.2 |
| 3 | I/O limits need a device path, so profiles without `io_device` get none | I/O silently unenforced | §4.1 |
| 4 | Single-interval sampling | misses short spikes | §2.3 |
| 5 | No `revert` / `status` command | `apply` is a one-way door | backups exist but must be restored by hand |
| 6 | Windows path compiles but is unverified at runtime | discovery + Job Object untested on a real host | needs a Windows runner |
| 7 | `profiles/` duplicated at repo root | redundant file | §6 |
| 8 | `--durable` and the audit log are not in the Rust build yet | Windows limits lapse; no local record of applies | MIGRATING.md |