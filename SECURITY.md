# Security Policy

## Supported Versions

| Version | Supported |
|---|---|
| 1.0.x | yes |
| < 1.0 | no |

## Reporting a Vulnerability

Please report security issues privately rather than opening a public issue.

Use GitHub's private vulnerability reporting:
**Security -> Report a vulnerability** on this repository. If that is unavailable,
email the maintainer via the address on the GitHub profile.

Include: affected version, OS, reproduction steps, and the impact (what an attacker
gains).

You can expect an acknowledgement within 7 days and a fix or mitigation plan within
30 days.

## Scope

This tool runs with root/Administrator privileges when applying limits. Areas of
interest:

- **Privilege escalation** - can `apply` be made to write outside
  `/etc/systemd/system/<service>.service.d/` or read a file it should not?
- **Config injection** - a crafted service name or profile that escapes into the
  generated systemd drop-in or PowerShell script. Service names are validated
  before being interpolated.
- **Audit log tampering** - the log is plain JSONL with no integrity protection. It is
  a local record, not tamper-evident.
- **Limit bypass** - silently emitting directives the OS ignores, so an operator
  believes a limit is enforced when it is not. This is a real failure mode that has
  happened in this project and is why tests assert exact generated text.

## Known Limitations (not vulnerabilities, but worth knowing)

- Windows Job Object limits are not durable unless `apply --durable` is used, and a
  service restart drops them.
- `--durable` spawns a background process holding the job handle. It is not
  supervised and will not survive a reboot.
- The audit log has no rotation and no locking.
