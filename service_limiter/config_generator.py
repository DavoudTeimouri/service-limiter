"""Configuration generator for OS-specific limits."""

import json
import os
import platform

class ConfigGenerator:
    def __init__(self, output_dir):
        self.output_dir = output_dir
        os.makedirs(output_dir, exist_ok=True)

    def generate(self, limited_service):
        """Generate a config dict for a service (to be implemented per OS)."""
        # This is a placeholder. In a real implementation, we would generate:
        # - For Linux: systemd override snippets (CPUQuota, MemoryMax, etc.)
        # - For Windows: Job Object settings or via registry/WMI.
        # For now, we just return a dict representing the desired limits.
        return {
            'service_name': limited_service.name,
            'cpu_limit_percent': 80,   # example
            'memory_limit_mb': 512,
            'io_read_limit_kbps': 1024,
            'io_write_limit_kbps': 512
        }

    def write_config(self, service_name, config):
        """Write the config to a file in the output directory.
        For Linux: generates a systemd override file.
        For Windows: generates a PowerShell script to set Job Object limits.
        """
        system = platform.system()
        if system == 'Linux':
            return self._write_linux_config(service_name, config)
        elif system == 'Windows':
            return self._write_windows_config(service_name, config)
        else:
            raise NotImplementedError(f"Unsupported platform: {system}")

    def _write_linux_config(self, service_name, config):
        """Generate systemd override file for Linux."""
        # Create directory: <output_dir>/<service_name>/ (to avoid conflicts)
        service_dir = os.path.join(self.output_dir, service_name)
        os.makedirs(service_dir, exist_ok=True)
        # File name: override.conf
        file_name = 'override.conf'
        path = os.path.join(service_dir, file_name)
        # Build the content
        cpu_quota = config.get('cpu_limit_percent', 80)
        memory_max = config.get('memory_limit_mb', 512)
        io_read = config.get('io_read_limit_kbps', 1024)
        io_write = config.get('io_write_limit_kbps', 512)
        # Note: CPUQuota is a percentage (e.g., 80% -> 80)
        # MemoryMax: in bytes, we can use M for megabytes (e.g., 512M)
        # IO: ReadBandwidthMax and WriteBandwidthMax in bytes per second, we use K for kilobytes per second (e.g., 1024K = 1MB/s)
        lines = [
            '[Service]',
            f'CPUQuota={cpu_quota}%',
            f'MemoryMax={memory_max}M',
            f'ReadBandwidthMax={io_read}K',
            f'WriteBandwidthMax={io_write}K',
        ]
        content = '\n'.join(lines)
        with open(path, 'w') as f:
            f.write(content)
        return path

    def _write_windows_config(self, service_name, config):
        """Generate PowerShell script to set Job Object limits for Windows."""
        # Create directory: <output_dir>/<service_name>/
        service_dir = os.path.join(self.output_dir, service_name)
        os.makedirs(service_dir, exist_ok=True)
        file_name = 'Set-JobLimits.ps1'
        path = os.path.join(service_dir, file_name)
        cpu_limit = config.get('cpu_limit_percent', 80)
        memory_limit_mb = config.get('memory_limit_mb', 512)
        # Convert memory limit to bytes
        memory_limit_bytes = memory_limit_mb * 1024 * 1024
        # Build the PowerShell script using a template to avoid f-string issues with '#'
        template = '''# Generated PowerShell script to set Job Object limits for service: {service_name}
# This script creates a Job Object named "ServiceLimiter_{service_name}" and sets CPU and memory limits.
# Note: This script does not assign the service to the Job Object. You must assign the service's processes to the Job Object.
# After running this script, you can use the following steps to assign the service:
#   1. Get the process IDs of the service (using Get-Service or Get-WmiObject -Query "SELECT ProcessId FROM Win32_Service WHERE Name='{service_name}'")
#   2. For each PID, use the command: $job.AssignProcess($pid)
#   3. Alternatively, restart the service within the Job Object by creating a new process in the Job Object that starts the service.

# Define Job Object name
$JobName = "ServiceLimiter_{service_name}"

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
$CpuSetting.Rate = {cpu_limit}  # e.g., 80 for 80%
$Job.CPUTRateControl = $CpuSetting
$Job.Put()

# Set job-wide memory limit (in bytes)
$Job.SetJobLimits({memory_limit_bytes}, 0)  # Second parameter is per-process memory limit (0 means no limit)

Write-Host "Job Object '$JobName' created with CPU limit {cpu_limit}% and memory limit {memory_limit_mb} MB."
Write-Host "To assign the service's processes to this Job Object:"
Write-Host "  1. Get the service's process IDs: Get-WmiObject -Query \"SELECT ProcessId FROM Win32_Service WHERE Name='{service_name}'\""
Write-Host "  2. For each PID, run: $job.AssignProcess(<PID>)"
Write-Host "  3. Alternatively, restart the service within the Job Object (requires modifying the service to start in the Job Object)."
'''
        script = template.format(
            service_name=service_name,
            cpu_limit=cpu_limit,
            memory_limit_mb=memory_limit_mb,
            memory_limit_bytes=memory_limit_bytes
        )
        with open(path, 'w') as f:
            f.write(script)
        return path
