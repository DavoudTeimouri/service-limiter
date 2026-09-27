# Service Limiter

> A cross-platform tool for discovering services, profiling resource usage, and applying policies to limit CPU, memory, and I/O.

[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](https://opensource.org/licenses/MIT)

## Why This Exists

System administrators and developers need to control resource usage of services to prevent noisy neighbors and ensure system stability. Service Limiter provides a unified way to analyze, generate, and apply resource policies across Windows and Linux.

## Features

- Cross-platform service discovery (Windows WMI/PowerShell, Linux systemctl)
- Resource profiling using `psutil`
- Policy-based limiting with default thresholds (CPU 80%, Memory 512MB, IO 1MB/s read, 512KB/s write)
- OS-specific configuration generation:
  - Linux: Systemd override files
  - Windows: PowerShell scripts to create and configure Job Objects
- CLI and TUI (Linux only) for interactive analysis
- Profile support for easy configuration of different service types
- Human-in-the-loop design for applying configurations

## Quick Start

```bash
pip install service-limiter
service-limiter analyze
service-limiter generate --profile web-server --output ./config
service-limiter apply --config ./config --dry-run
```

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

## Usage

### Analyze Services
```bash
service-limiter analyze
```
Discovers running services and profiles their resource usage.

Sample output:
```
+----------------------+-------+--------+---------+----------+----------+--------+--------+
| Service              | PID   | CPU %  | Mem MB  | Read KB/s| Write KB/s| Policy | Status |
+----------------------+-------+--------+---------+----------+----------+--------+--------+
| Dhcp                 | 1234  | 0.5    | 45.2    | 10.5     | 5.2      | default| OK     |
| WinDefend            | 5678  | 2.1    | 120.5   | 50.0     | 20.0     | default| VIOLATION (CPU, Mem) |
+----------------------+-------+--------+---------+----------+----------+--------+--------+
```

### Generate Configuration
```bash
service-limiter generate --profile web-server --output ./config
```
Generates a configuration file based on the 'web-server' profile.

Output directory structure:
```
./config/
├── linux/
│   └── <service_name>.override
└── windows/
    └── <service_name>_limits.ps1
```

### Apply Configuration
```bash
service-limiter apply --config ./config
```
Applies the generated configuration (requires admin/root). Use `--dry-run` to preview changes.

### TUI (Linux only)
```bash
service-limiter tui
```
Launches a terminal user interface for interactive analysis and configuration.

## Configuration Profiles

Profiles are JSON files that define resource limits per service type. The tool includes a built-in 'web-server' profile, and users can create custom profiles.

Example profile (web-server.json):
```json
{
  "cpu_percent": 50,
  "memory_mb": 256,
  "io_read_kbps": 512,
  "io_write_kbps": 256
}
```

## Policy Engine

Service Limiter includes a policy engine that evaluates resource usage against defined limits. The engine checks CPU, memory, read I/O, and write I/O. By default, it uses:
- CPU: 80%
- Memory: 512 MB
- Read I/O: 1 MB/s
- Write I/O: 512 KB/s

These defaults can be overridden by creating a custom policy profile.

## Human-in-the-Loop

For safety, changes that would affect more than 80% of resources or more than 100 services require explicit confirmation in the TUI or via the CLI.

## OS-Specific Details

### Linux
- Uses systemd override files located in `/etc/systemd/system/<service>.d/`
- Applies limits via `MemoryLimit`, `CPUQuota`, `IOReadBandwidthMax`, and `IOWriteBandwidthMax` directives
- Requires `systemctl daemon-reload` and `systemctl restart <service>` after applying

### Windows
- Generates PowerShell scripts that create and configure Job Objects
- Sets memory limits, CPU limits, and I/O bandwidth limits via Job Object APIs
- Must be run as Administrator

## Sample Output

See the **Usage** section above for sample outputs of each command.

## Troubleshooting

- **Command not found**: Ensure you have installed the package with `pip install -e .` or used PyInstaller.
- **No services detected**: On Linux, ensure you are running on a system with systemd. On Windows, ensure WMI is accessible.
- **Permission denied**: Apply operations require root (Linux) or Administrator (Windows) privileges.
- **Profile not found**: Ensure the profile JSON file exists in the `profiles/` directory.

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md)

## License

MIT © [Your Name](https://github.com/yourname)

## [CHANGELOG.md](CHANGELOG.md)

