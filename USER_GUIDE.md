# User Guide

## Supported Platforms
- Windows 10/11 (x64)
- Linux (x64, systemd)

## Installation

### From Source
```bash
git clone https://github.com/yourname/service-limiter.git
cd service-limiter
pip install -e .
```

### Using PyInstaller (Windows)
```bash
pyinstaller --onefile service_limiter/cli/main.py
```

### Using PyInstaller (Linux)
```bash
pyinstaller --onefile service_limiter/cli/main.py
```

## CLI Usage

### Analyze Services
```bash
service-limiter analyze --help
```

### Generate Configuration
```bash
service-limiter generate --profile web-server --output ./config
```

### Apply Configuration (requires admin/root)
```bash
service-limiter apply --config ./config/web-server.json
```

## TUI Usage (Linux only)
```bash
service-limiter tui
```
Navigate menus to analyze, profile, and limit services.

## Configuration Profiles
Profiles are JSON files defining resource limits per service type.
Example:
```json
{
  "service_name_pattern": "web.*",
  "cpu_limit_percent": 80,
  "memory_limit_mb": 512,
  "io_read_limit_kbps": 1024,
  "io_write_limit_kbps": 512
}
```

## Safety Features
- Human-in-the-loop for changes affecting >80% resources or >100 services.
- Least-privilege: only reads service state, writes to ~/.service-limiter/.
- Backup original configs before modification.
