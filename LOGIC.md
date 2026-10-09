# Service Limiter — Project Logic

Authoritative description of **how the tool works and why**. Written before the Rust
rewrite so both implementations share one reference.

**Status**: describes the Python implementation at tag `v1.0.0`.
The Rust rewrite must preserve every guarantee listed under
[Compatibility contract](#compatibility-contract) — the test suite enforces them.

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

`Orchestrator` branches on `PlatformDetector().detect()` at three call sites
(`_discover_services`, `_profile_resources`, `_generate_configs`). Each does
`if is_linux: import .linux … elif is_windows: import .windows …`.

Modules are imported **lazily inside the branch** so a Linux host never parses the
Windows module. That is deliberate: `windows.py` is unverified on Linux and vice versa.

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
→ `test_trailing_summary_line_is_not_a_service`

When `systemctl` is missing, discovery returns `[]` and logs an error. It never invents a
placeholder service. → `test_no_fabricated_services_without_systemd`

**Windows** — `Get-CimInstance Win32_Service` selecting `Name, DisplayName, State,
StartMode, PathName, ProcessId, StartName`, `ConvertTo-Json -Compress`.
A single service serialises as an object, so a dict result is wrapped into a list.
The `ProcessId` is stored in `ServiceDescriptor.child_processes`, which is how the
Windows profiler reaches the process without a second query.

`Get-CimInstance`, not `Get-WmiObject` — the latter is deprecated.

### 2.3 Profiling — the measurement

The only place real numbers are produced. Shared by both platforms
(`windows.py` imports `_sample` from `linux.py`).

```python
def _sample(pid: int, interval: float) -> Optional[ResourceProfile]
```

1. `psutil.Process(pid)` → `NoSuchProcess`/`AccessDenied` → return `None`.
2. Collect `.children(recursive=True)` — a service's limits apply to its whole tree.
3. Prime `cpu_percent(None)` on every process.
4. `psutil.cpu_percent(None)`, then `sleep(interval)` (default **0.25 s**).
5. Sum `cpu_percent()`, `memory_info().rss`, `io_counters()` read/write.

**Why prime first**: psutil returns a meaningless `0.0` on a process object's first
`cpu_percent()` call, because it has no prior sample to diff against.

**Units**:

| Metric | Unit |
|---|---|
| `cpu_percent` | summed percent across the tree; can exceed 100 on multi-core |
| `memory_mb` | RSS bytes ÷ 1024² |
| `io_read_kbps` | read_bytes ÷ 1024 ÷ interval |
| `io_write_kbps` | write_bytes ÷ 1024 ÷ interval |

**Caveats** — honest limits of this method:
- A single interval is a point sample, not a trend. Short-lived spikes are missed.
- Children that spawn and exit inside the window are missed.
- CPU percent is process-tree summed, not cgroup accounting, so it does not match
  `systemd-cgtop`.
- Root-owned processes require elevation to sample.

---

## 3. Policy semantics

`policy_engine.py` holds one hardcoded default:

```python
{"name": "default", "cpu_percent": 80, "memory_mb": 512,
 "io_read_kbps": 1024, "io_write_kbps": 512}
```

`evaluate(profile, policy) -> (is_over, [violations])` applies **strict `>`** to all four
keys. Exactly at the limit passes. → `test_limit_is_inclusive`

Profiles are JSON overrides with the same four keys. A missing or malformed profile is a
**hard error**, never a silent fallback to defaults: a limiter that quietly applies
different limits than the operator asked for is worse than refusing to run.
→ `test_missing_profile_is_fatal_not_silent`

### 3.1 The CPU unit trap

> `CPUQuota=80%` means **80% of one CPU**, not 80% of the machine.
> systemd: *"The percentage specifies how much CPU time the unit shall get at maximum,
> relative to the total CPU time available on one CPU."*

So a service using 4 of 8 cores passes a `cpu_percent: 50` check, yet the generated limit
throttles it to half of one core. **The policy measures and the config enforces different
quantities.** Unresolved by design as of v1.0.0.

Options, for whoever picks this up:
- Keep `cpu_percent` as whole-machine share and emit `CPUQuota={cpu_percent × cpu_count}%`.
- Rename the field to something explicit like `cpu_percent_single_core` and document 0–100 per core.

A test asserts the current literal behaviour, so changing this **must** update
`test_memory_and_cpu_directives`.

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
| `CPUQuota` | `<cpu_percent>%` — per single CPU, see §3.1 |
| `MemoryMax` | `<memory_mb>M` |
| `IOAccounting` | `yes`, required for I/O accounting |
| `IOReadBandwidthMax` | `<io_device> <io_read_kbps × 1024>` bytes/s |
| `IOWriteBandwidthMax` | `<io_device> <io_write_kbps × 1024>` bytes/s |

**`MemoryMax`, not `MemoryLimit`** — `MemoryLimit=` is deprecated; systemd 257 warns
*"Unit uses MemoryLimit=; please use MemoryMax= instead."*

**I/O limits are skipped unless the policy has `io_device`.** systemd requires a
block-device path per limit. Without one the generator logs a warning and omits the
directives rather than emitting something systemd ignores.
→ `test_io_skipped_with_warning_when_no_device`

> **Silent no-op, fixed.** The earlier code emitted `ReadBandwidthMax=` and
> `WriteBandwidthMax=`. Those are **not** systemd keys. `systemd-analyze verify` reports
> `Unknown key 'ReadBandwidthMax' in section [Service], ignoring.` — `daemon-reload`
> succeeded, the service restarted, and **no I/O limit was ever applied**. Verified
> against systemd 257 by diffing old vs new output.
> → `test_io_directives_are_the_real_systemd_names`, `test_bogus_legacy_io_directives_are_absent`

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
> → `test_no_invented_cmdlets`, `test_uses_real_win32_api`

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
{"event": "apply", "time": "2026-09-30T15:04:22+0000", "config": "./config", "dry_run": false, "ok": true}
{"event": "apply_denied", "time": "...", "reason": "not_root", "config": "./config"}
```

No rotation, no locking — a deliberate simplification. Add rotation only when the file
actually grows large.

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
| `cpu_percent` | yes | CPU share, see the §3.1 trap |
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
| `cli/main.py` | argparse, subcommands, exit codes, audit, apply gates |
| `orchestrator.py` | pipeline, profile loading, config **text** generation |
| `policy_engine.py` | thresholds and `evaluate()` |
| `platform/detector.py` | OS + arch |
| `platform/linux.py` | systemctl discovery, `_sample()`, profiling |
| `platform/windows.py` | CIM discovery, profiling (reuses `_sample`) |
| `models/` | `ServiceDescriptor`, `ResourceProfile`, `SharedState` |
| `tui.py` | curses UI, Linux only |

**Platform discovery, profiling, and config generation all live in `orchestrator.py`.**
`config_generator.py` was a byte-identical duplicate with a `SyntaxWarning`; deleted.

`SharedState` is a mutable bag threaded through the pipeline whose only consumer is
`to_dict()`. A dataclass or plain return value would do.

---

## 8. TUI

curses, Linux only. Menu: Analyze Services / View Service Profiles / Apply Limits
(placeholder) / Quit. Services are paired to profiles **by name**.

> **Index-pairing bug, fixed.** The TUI zipped `services[i]` with `profiles[i]` by
> position. When any profile was missing, every subsequent row showed **the wrong
> service's numbers under the right name**. Profiles are now looked up by
> `service_name`.

---

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

**Deliberately changed**: install method (`pip` → `cargo install` / prebuilt binary).
TUI stays curses-based unless there is a reason not to.

---

## 10. Keeping this file honest

| Claim | Check |
|---|---|
| systemd directives correct | `test_io_directives_are_the_real_systemd_names` |
| legacy directives gone | `test_bogus_legacy_io_directives_are_absent` |
| summary line not a service | `test_trailing_summary_line_is_not_a_service` |
| no fabricated services | `test_no_fabricated_services_without_systemd` |
| CPU/memory text | `test_memory_and_cpu_directives` |
| drop-in path | `test_drop_in_path_is_service_dot_service_d` |
| no invented cmdlets | `test_no_invented_cmdlets` |
| real Win32 APIs used | `test_uses_real_win32_api` |
| Windows filename matches apply | `test_filename_matches_what_apply_expects` |
| profiler returns a map | `test_profile_resources_always_returns_a_dict` |
| policy boundaries | `test_limit_is_inclusive`, `test_over_limit_reports_each_resource` |
| profiles load from package | `test_bundled_profile_loads` |
| missing profile fatal | `test_missing_profile_is_fatal_not_silent` |

Run them:

```bash
python3 -m unittest discover -s tests -v
```

Claims **not** covered by a test, and how to verify by hand:

| Claim | Verify |
|---|---|
| systemd accepts the drop-in | `systemd-analyze verify <unit>` — must print no `Unknown key` |
| Windows script runs | elevated PowerShell on a real Windows host |
| CPU sampling is accurate | compare `analyze` against `systemd-cgtop` / Task Manager |
| Job Object limits actually bind | needs a Windows host; see the §4.2 weak-enforcement note |

---

## 11. Known open problems

| # | Problem | Impact | Notes |
|---|---|---|---|
| 1 | `CPUQuota` is per-single-CPU, `cpu_percent` reads as whole-machine | limits differ from the policy the operator wrote | §3.1 |
| 2 | Windows Job Object limits lapse when the script exits | enforcement is transient | §4.2 |
| 3 | I/O limits need a device path, so profiles without `io_device` get none | I/O silently unenforced | §4.1 |
| 4 | Single-interval sampling | misses short spikes | §2.3 |
| 5 | No `revert` / `status` command | `apply` is a one-way door | backups exist but must be restored by hand |
| 6 | Windows path unverified on a real host | discovery + Job Object untested | needs a Windows runner |
| 7 | `profiles/` duplicated at repo root | redundant file | §6 |
| 8 | TUI "Apply Limits" is a placeholder | menu entry promises a feature that does nothing | |