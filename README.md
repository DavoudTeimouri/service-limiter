# Service Limiter

> Discover OS services, measure real CPU/memory/IO, generate and apply OS-native limits.

[![CI](https://github.com/DavoudTeimouri/service-limiter/actions/workflows/ci.yml/badge.svg)](https://github.com/DavoudTeimouri/service-limiter/actions/workflows/ci.yml)
[![release](https://img.shields.io/github/v/release/DavoudTeimouri/service-limiter?include_prereleases)](https://github.com/DavoudTeimouri/service-limiter/releases)
[![license](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)
![platform](https://img.shields.io/badge/platform-Linux%20%7C%20Windows-555)

Rust, single static binary, no runtime dependencies.

For how the tool works and the domain traps it has to respect, read
**[LOGIC.md](LOGIC.md)**.

---

## Why This Exists

One service can starve the machine. You want a CPU, memory, and I/O ceiling per
service, enforced by the operating system rather than by hoping the application is
polite.

Service Limiter measures what a service *actually* uses, not what it claims, and
writes the limit in the native mechanism: systemd drop-ins on Linux, Job Objects on
Windows. It never edits an application's own configuration, and it never changes
anything without your confirmation.

---

## What It Does / Does Not Do

**Does**: discover services, measure real usage over a sample window, compare against
a policy, generate the OS-native config, install it with backups and validation, log
every action.

**Does not**: persist Windows limits across a service restart (see below), manage
cgroups, or install software.

---

## Install

Prebuilt binaries are attached to each
[release](https://github.com/DavoudTeimouri/service-limiter/releases):

| Platform | Asset |
|---|---|
| Linux x86_64 | `service-limiter-linux-x86_64.tar.gz` |
| Linux aarch64 | `service-limiter-linux-aarch64.tar.gz` |
| Windows x86_64 | `service-limiter-windows-x86_64.zip` |

Verify before installing - every asset ships a `.sha256`:

```bash
sha256sum -c service-limiter-linux-x86_64.tar.gz.sha256
sudo tar -xzf service-limiter-linux-x86_64.tar.gz -C /usr/local/bin
```

Or install with cargo:

```bash
cargo install --git https://github.com/DavoudTeimouri/service-limiter.git
```

From source:

```bash
git clone https://github.com/DavoudTeimouri/service-limiter.git
cd service-limiter
cargo build --release
```

**Coming from the Python 1.x CLI?** See [MIGRATING.md](MIGRATING.md).

---

## Quick Start

```bash
# 1. See what services exist and what they use
service-limiter analyze

# 2. Write configs for anything over the policy
service-limiter generate --profile web-server --output ./config

# 3. Preview
sudo service-limiter apply --config ./config --dry-run

# 4. Apply (asks before each service)
sudo service-limiter apply --config ./config
```

On Windows, run an **elevated** terminal instead of using `sudo`.

---

## Platform Support

| Capability | Linux (systemd, x64/arm64) | Windows (10/11, x64) |
|---|---|---|
| Discovery | `systemctl list-units` | `Get-CimInstance Win32_Service` |
| Profiling | main PID + **full child tree** | main PID (children not walked) |
| CPU limit | `CPUQuota`, per **single** CPU | Job Object `CpuRate` |
| Memory limit | `MemoryMax` | Job Object `JOB_OBJECT_LIMIT_JOB_MEMORY` |
| I/O limit | `IOReadBandwidthMax`, only when the profile has `io_device` | Job Object I/O rate control |
| Durable | **Yes**, systemd owns the drop-in | **No by default**, see below |
| macOS / BSD | unsupported | unsupported |

**Windows limits are not durable by default.** A Job Object's limits live only while a
handle to it is open. `apply` runs a one-shot script, so its limits lapse when the
script exits, and a service restart drops them entirely. Use `--durable` to keep a
process alive holding the handle:

```powershell
service-limiter apply --config .\config --yes --durable
```

Windows has no supported native per-service Job Object limit surface, so this is a
holder-process workaround rather than a real fix. See [LOGIC.md](LOGIC.md).

---

## Commands

### `service-limiter analyze [--profile NAME]`

Discovers services and measures them. Prints a count and the first few services.

```bash
service-limiter analyze --profile web-server
```

### `service-limiter generate --profile NAME [--output DIR]`

Writes configs for every service over the policy.

```bash
service-limiter generate --profile web-server --output ./config
```

Creates `./config/<service>/override.conf` on Linux or
`./config/<service>/Set-JobLimits.ps1` on Windows. Exits 0 even when nothing exceeded
the policy, so check stdout for the count.

### `service-limiter apply --config DIR [--dry-run] [--yes] [--durable]`

Installs staged configs.

| Flag | Effect |
|---|---|
| `--dry-run` | print what would happen, change nothing |
| `--yes` | skip the per-service confirmation |
| `--durable` | Windows only, hold the Job Object handle open |

On Linux, in order: verify the drop-in with `systemd-analyze verify` (**before**
touching `/etc`), back up any existing `override.conf` to `.bak`, then
`daemon-reload` + `restart`.

---

## Configuration Profiles

Profiles are JSON files bundled with the package. `--profile NAME` loads `NAME.json`
from `service_limiter/profiles/`.

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
| `cpu_percent` | yes | CPU share of the whole machine, 0–100 |
| `memory_mb` | yes | max RSS in MB |
| `io_read_kbps` | yes | read bandwidth in KB/s |
| `io_write_kbps` | yes | write bandwidth in KB/s |
| `io_device` | **no** | block device; without it I/O limits are skipped |

A missing or malformed profile is a **hard error**, never a silent fallback.

`cpu_percent` is a share of the **whole machine**. Both the measurement and the
generated limit are normalized to match, so `50` on a 4-core box emits
`CPUQuota=200%` (systemd counts per-CPU). Core count follows the cgroup CPU
quota, so a container capped at 1 CPU reports 1. See
[LOGIC.md §3.1](LOGIC.md).

---

## Exit Codes

| Code | Meaning |
|---|---|
| `0` | ran clean, nothing over policy |
| `1` | at least one service exceeded the policy |
| `2` | error, no systemd, nothing measurable, or bad config |
| `3` | `apply` needs root (Linux) |

Zero measurable services exits `2`, so exit `0` always means the tool really worked.

---

## Audit Log

Every run appends one JSON line to `~/.service-limiter/audit.log`, including
runs that fail:

```json
{"event": "analyze", "time": "2026-10-09T09:51:42+0000", "discovered": "2", "profiled": "2", "over_policy": "0"}
{"event": "apply", "time": "...", "config": "./config", "dry_run": "true", "applied": "1", "skipped": "0", "failed": "0"}
{"event": "apply_denied", "time": "...", "reason": "not_root", "config": "./config"}
{"event": "analyze_error", "time": "...", "error": "systemctl not available: ...", "profile": "default"}
```

Set `SERVICE_LIMITER_HOME` to write it elsewhere. If the log cannot be written
the tool warns and continues: losing an audit line is bad, refusing to apply a
limit because of it is worse.

---

## Troubleshooting

- **"systemctl not found"** - this host does not run systemd, which Linux support requires.
- **Exit 2 from `analyze`** - no service could be profiled. Either no live MainPID, or reading process stats needs root.
- **"Profile not found"** - the error lists the available names.
- **I/O limits missing from the drop-in** - your profile has no `io_device`, and systemd needs a block device path per limit.
- **"AssignProcessToJobObject failed" on Windows** - the service is probably already inside a job object, common under IIS or a service host.
- **Windows limits vanish** - expected, use `--durable`.

---

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md). Security issues: see [SECURITY.md](SECURITY.md).

## License

MIT (c) [Davoud Teimouri](https://github.com/DavoudTeimouri) - see [LICENSE](LICENSE).

## Changelog

See [CHANGELOG.md](CHANGELOG.md).
