# User Guide

## Quick Start

```bash
pip install service-limiter
service-limiter analyze
service-limiter generate --profile web-server --output ./config
service-limiter apply --config ./config --dry-run
```

## Supported Platforms

- Windows 10/11 (x64) - CLI only
- Linux (x64, systemd) - CLI and TUI

## Installation

### Prerequisites
- Python 3.8+
- pip

### From Source
```bash
git clone https://github.com/DavoudTeimouri/service-limiter.git
cd service-limiter
pip install -e .
```

### Using PyInstaller (Windows and Linux)
```bash
pyinstaller --onefile service_limiter/cli/main.py
```

## CLI Usage

### Analyze Services
```bash
service-limiter analyze
```
Discovers running services and profiles their resource usage on the current platform.

### Generate Configuration
```bash
service-limiter generate --profile web-server --output ./config
```
Generates OS-specific configuration files based on the selected profile. The output directory contains platform-appropriate configuration files (systemd overrides for Linux, PowerShell scripts for Windows).

### Apply Configuration
```bash
service-limiter apply --config ./config --dry-run
```
Applies the generated configuration (requires admin/root). Use `--dry-run` to preview changes before applying.

### TUI Usage (Linux only)
```bash
service-limiter tui
```
Launches a terminal user interface for interactive service analysis, profiling, and configuration.

## Configuration Profiles

Profiles are JSON files that define resource limits per service type. The tool includes a built-in 'web-server' profile, and users can create custom profiles in the `profiles/` directory.

Example profile (web-server.json):
```json
{
  "cpu_percent": 50,
  "memory_mb": 256,
  "io_read_kbps": 512,
  "io_write_kbps": 256
}
```

### Profile Fields
- `cpu_percent`: Maximum CPU usage percentage (0-100)
- `memory_mb`: Maximum memory usage in megabytes
- `io_read_kbps`: Maximum read I/O in kilobytes per second
- `io_write_kbps`: Maximum write I/O in kilobytes per second

## Policy Engine

Service Limiter includes a policy engine that evaluates resource usage against defined limits. The engine checks CPU, memory, read I/O, and write I/O. By default, it uses:
- CPU: 80%
- Memory: 512 MB
- Read I/O: 1 MB/s (1024 KB/s)
- Write I/O: 512 KB/s

These defaults can be overridden by creating a custom policy profile or by using different profiles for different service types.

## OS-Specific Configuration Generation

Service Limiter generates platform-appropriate configuration files:

### Linux
- Creates systemd override files in `/etc/systemd/system/<service>.d/`
- Uses `MemoryLimit`, `CPUQuota`, and `IOReadBandwidthMax`/`IOWriteBandwidthMax` directives

### Windows
- Generates PowerShell scripts that create and configure Job Objects
- Sets memory limits, CPU limits, and I/O bandwidth limits via Job Object APIs

## Human-in-the-Loop

For safety, changes that would affect more than 80% of system resources or more than 100 services require explicit confirmation. In the TUI, this appears as a confirmation prompt. When using the CLI, the `--apply` flag (or equivalent) must be used to bypass the dry-run and apply changes.

## Safety Features

- Least-privilege: only reads service state, writes to `~/.service-limiter/` for logs and user configurations.
- Backup original configs before modification (where applicable).
- Dry-run mode allows previewing changes before application.

## Configuration

Profiles are stored in the `profiles/` directory. To create a custom profile, copy an existing profile and modify the values.

```json
{
  "cpu_percent": 50,
  "memory_mb": 256,
  "io_read_kbps": 512,
  "io_write_kbps": 256
}
```

Place the file in `profiles/custom-profile.json` and use it with:
```bash
service-limiter generate --profile custom-profile --output ./config
```

## Documentation

- [README.md](README.md) - Overview, features, and installation
- [ARCHITECTURE.md](architecture.md) - High-level design and components
- [STRUCTURE.md](structure.md) - Detailed code structure and module descriptions
- [GITHUB_SETUP.md](github-setup.md) - Instructions for setting up development environment

## License

MIT © [Your Name](https://github.com/yourname)

## [CHANGELOG.md](CHANGELOG.md)

