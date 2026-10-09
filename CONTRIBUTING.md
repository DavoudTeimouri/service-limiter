# Contributing

## Setup

```bash
git clone git@github.com:DavoudTeimouri/service-limiter.git
cd service-limiter
python3 -m venv .venv && source .venv/bin/activate
pip install -e .[dev]
```

## Tests

The suite is stdlib `unittest`, no pytest needed:

```bash
python3 -m unittest discover -s tests -v
```

## Before you push

- `python3 -m unittest discover -s tests` passes
- `ruff check .` clean
- If you changed a generated config, the matching test in `tests/test_service_limiter.py`
  changed too
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
