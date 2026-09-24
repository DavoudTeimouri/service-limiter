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


## Applying Generated Configurations

The service limiter generates JSON configuration files that specify the desired resource limits for each service.
These JSON files can be used to create the actual OS-specific configuration mechanisms.

### Applying on Linux (systemd)

For each service, the JSON file contains:
- service_name
- cpu_limit_percent
- memory_limit_mb
- io_read_limit_kbps
- io_write_limit_kbps

To apply these limits using systemd override files:

1. Create a drop-in directory for the service:
   ```bash
   sudo mkdir -p /etc/systemd/system/<service_name>.service.d
   ```

2. Create an override.conf file in that directory with the following content:
   ```ini
   [Service]
   CPUQuota=<cpu_limit_percent>%
   MemoryMax=<memory_limit_mb>M
   ReadBandwidthMax=<io_read_limit_kbps>K
   WriteBandwidthMax=<io_write_limit_kbps>K
   ```

   Example for a service named "nginx" with 80% CPU, 512MB memory, 1MB/s read IO, 512KB/s write IO:
   ```ini
   [Service]
   CPUQuota=80%
   MemoryMax=512M
   ReadBandwidthMax=1024K
   WriteBandwidthMax=512K
   ```

3. Reload systemd and restart the service:
   ```bash
   sudo systemctl daemon-reload
   sudo systemctl restart <service_name>
   ```

### Applying on Windows (using Job Objects)

Windows does not have a built-in systemd equivalent, but you can use Job Objects to limit processes.
The generated JSON can be used to create a PowerShell script that sets up a Job Object with the desired limits.

Note: Assigning an existing service to a Job Object requires modifying the service's startup configuration or using a tool to move the service's processes into the Job Object after startup.
A simpler approach is to create a scheduled task or a service that starts within the Job Object.

Steps to apply limits via Job Object (advanced):

1. Create a Job Object with the desired limits using PowerShell (see the script below).
2. Assign the service's processes to the Job Object.
   - You can find the service's process IDs using: `Get-WmiObject -Query "SELECT ProcessId FROM Win32_Service WHERE Name='<service_name>'"`
   - Then for each PID, use: `$job.AssignProcess(<PID>)`
   - Alternatively, you can configure the service to run within the Job Object by modifying the service's executable path to launch via a wrapper that starts in the Job Object (requires changing the service configuration).

Example PowerShell script to create a Job Object (replace placeholders with values from JSON):

```powershell
# Define Job Object name
$JobName = "ServiceLimiter_<service_name>"

# Try to get existing Job Object
$Job = Get-WmiObject -Query "SELECT * FROM Win32_JobObject WHERE Name = '$JobName'" -ErrorAction SilentlyContinue
if (-not $Job) {
    # Create new Job Object
    $Job = [wmiclass]"Win32_JobObject".CreateInstance()
    $Job.Name = $JobName
    $Job.Put()
    $Job = Get-WmiObject -Query "SELECT * FROM Win32_JobObject WHERE Name = '$JobName'"
}

# Set CPU rate control (percentage of CPU time)
$CpuSetting = [wmiclass]"Win32_JobObject_CPU_Rate_Control_Information".CreateInstance()
$CpuSetting.Control = 1  # Enable CPU rate control
$CpuSetting.Rate = <cpu_limit_percent>  # e.g., 80 for 80%
$Job.CPUTRateControl = $CpuSetting
$Job.Put()

# Set job-wide memory limit (in bytes)
$memory_limit_bytes = <memory_limit_mb> * 1024 * 1024
$Job.SetJobLimits($memory_limit_bytes, 0)  # Second parameter is per-process memory limit (0 means no limit)

Write-Host "Job Object '$JobName' created with CPU limit <cpu_limit_percent>% and memory limit <memory_limit_mb> MB."
Write-Host "To assign the service's processes to this Job Object:"
Write-Host "  1. Get the service's process IDs: Get-WmiObject -Query \"SELECT ProcessId FROM Win32_Service WHERE Name='<service_name>'\""
Write-Host "  2. For each PID, run: $job.AssignProcess(<PID>)"
Write-Host "  3. Alternatively, restart the service within the Job Object (requires modifying the service to start in the Job Object)."
```

### Note

The service limiter application currently generates JSON files as a starting point.
Future versions may include automated application of these limits via systemd drop-ins and Job Object assignment.
