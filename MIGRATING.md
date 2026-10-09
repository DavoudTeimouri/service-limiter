# Migrating from Python 1.x

service-limiter 2.0.0 is a Rust rewrite. The commands and flags are unchanged;
how you install it and where Windows limits come from are not.

## Install method changed

```bash
# was
pip install service-limiter

# now - either a release binary
sudo tar -xzf service-limiter-linux-x86_64.tar.gz -C /usr/local/bin

# or
cargo install --git https://github.com/DavoudTeimouri/service-limiter.git
```

The PyPI package stays frozen at 1.0.0 and gets no further fixes.

There is no interpreter requirement, no `psutil`, and no `pip` on the target
machine. The binary is ~1 MB.

## Identical

- Command name: `service-limiter {analyze,generate,apply}`
- Flags: `--profile`, `--output`, `--config`, `--dry-run`, `--yes`
- Exit codes: `0` ok, `1` violation, `2` error, `3` needs root
- Profile JSON schema, unchanged
- Staging layout: `<output>/<service>/override.conf` and
  `<output>/<service>/Set-JobLimits.ps1`
- Generated systemd drop-in text, byte for byte

## What changed

| Change | Impact |
|---|---|
| Python removed | `pip install` no longer works; `pyproject.toml` is gone |
| No TUI | the curses TUI was dropped; `analyze` prints the same data |
| `--durable` not implemented | Windows limits still lapse after `apply`; the flag warns |
| Audit log not implemented | `~/.service-limiter/audit.log` is no longer written |
| Windows limits actually work | 1.x set the wrong Job Object flags and applied nothing; see CHANGELOG |
| `generate` exit code | 1.x exited 1 when nothing exceeded policy; 2.0 exits 0 |

## Do this

1. Replace the binary.
2. Re-run `service-limiter analyze` and confirm it reports services and
   measurements. On Windows, expect real numbers for the first time.
3. Regenerate configs (`generate`); do not reuse 1.x output on Windows, because
   those scripts contained the broken flags.
4. Re-apply. Linux drop-ins from 1.x are still valid, but regenerate anyway so
   `io_device` handling is consistent.

## Config on disk

systemd drop-ins already installed under
`/etc/systemd/system/<service>.service.d/override.conf` are untouched by the
upgrade and keep working. Backups written as `override.conf.bak` are left in
place; there is still no `revert` command, so restore one by hand if needed.
