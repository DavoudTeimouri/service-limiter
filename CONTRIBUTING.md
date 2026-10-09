# Contributing

## Setup

```bash
git clone git@github.com:DavoudTeimouri/service-limiter.git
cd service-limiter
cargo build
```

## Tests

No test framework beyond `cargo test`:

```bash
cargo test
```

## Before you push

- `cargo test` passes
- `cargo clippy --all-targets -- -D warnings` clean
- `cargo fmt --check` clean
- Cross-compile check if you touched platform code:
  `cargo check --target x86_64-pc-windows-msvc`
- If you changed a generated config, the matching test in `src/config_gen.rs` changed too
- If you changed behaviour, update `LOGIC.md` in the same PR — that file is the
  reference for how the tool works, and every claim in it is tied to a test or a
  command you can run

## Adding a limit or a directive

1. Change the generator in `service_limiter/orchestrator.py`.
2. Add or update the exact-string test in `tests/test_service_limiter.py`.
3. Update `LOGIC.md`.

On Linux, verify the real drop-in rather than trusting the string:

```bash
systemd-analyze verify ./yourservice.service
```

Any `Unknown key` means systemd is silently ignoring that directive.

## Commit messages

Plain imperative subject, a short body explaining why. One logical change per commit.

## Reporting bugs

Open an issue with: what you ran, what you expected, what happened, and your OS plus
Python version.
