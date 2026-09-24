"""Configuration generator for OS-specific limits."""

import logging
import json
import os
from typing import Dict, Any, List
from .platform.detector import PlatformDetector
from .models.service_descriptor import ServiceDescriptor

logger = logging.getLogger(__name__)

class ConfigGenerator:
    """Generates OS-specific configuration files for services that need limiting."""

    def generate(self, limited_services: List[ServiceDescriptor], policy: Dict[str, Any]) -> Dict[str, Any]:
        """
        Generate configuration files for the given limited services.
        Returns a dictionary mapping service name to config data (which is another dict).
        The config data will be written to files by the apply step.
        """
        platform_info = PlatformDetector().detect()
        configs = {}
        for service in limited_services:
            if platform_info['is_linux']:
                configs[service.name] = self._generate_linux_config(service, policy)
            elif platform_info['is_windows']:
                configs[service.name] = self._generate_windows_config(service, policy)
            else:
                logger.warning(f"Unsupported platform for config generation: {platform_info['system']}")
                configs[service.name] = {}
        return configs

    def _generate_linux_config(self, service: ServiceDescriptor, policy: Dict[str, Any]) -> Dict[str, Any]:
        """Generate Linux systemd override config."""
        # We'll create a dictionary that represents the content of the override.conf file.
        # The apply step will write this to /etc/systemd/system/<service>.service.d/override.conf
        content = f"""[Service]
CPUQuota={policy.get('cpu_percent', 80)}%
MemoryMax={policy.get('memory_mb', 512)}M
"""
        # Note: IO limits in systemd are a bit more complex. We'll use ReadBandwidthMax and WriteBandwidthMax.
        # These are in bytes per second. We have kbps in policy, so convert to bytes per second: * 1024
        read_bps = policy.get('io_read_kbps', 1024) * 1024
        write_bps = policy.get('io_write_kbps', 512) * 1024
        content += f"""ReadBandwidthMax={read_bps}
WriteBandwidthMax={write_bps}
"""
        return {
            'content': content,
            'file_path': f"/etc/systemd/system/{service.name}.service.d/override.conf",
            'directory': f"/etc/systemd/system/{service.name}.service.d"
        }

    def _generate_windows_config(self, service: ServiceDescriptor, policy: Dict[str, Any]) -> Dict[str, Any]:
        """Generate Windows PowerShell script to create and configure a Job Object."""
        # We'll generate a PowerShell script that creates a Job Object and sets limits.
        # Note: Applying the Job Object to a service is complex and requires knowing the service's PID.
        # We'll generate a script that creates the Job Object and then the user must assign the service's processes to it.
        # Alternatively, we can try to get the service's PID and assign it in the script, but that is more complex.
        # We'll generate a script that creates the Job Object and sets the limits, and then outputs the Job Object handle.
        # The user can then use that handle to assign processes.
        # We'll also note that the script should be run as Administrator.
        cpu_limit = policy.get('cpu_percent', 80)  # percent
        memory_limit = policy.get('memory_mb', 512) * 1024 * 1024  # convert MB to bytes
        read_io_limit = policy.get('io_read_kbps', 1024) * 1024  # convert KB/s to bytes/s
        write_io_limit = policy.get('io_write_kbps', 512) * 1024  # convert KB/s to bytes/s

        # Note: Windows Job Object limits are in different units:
        # - CPU limit: we can set a limit per process in the job object? Actually, we can set a CPU rate control (Windows 8+).
        #   We'll use the CPU rate control: set the CPU rate to (cpu_limit / 100) * 100? Actually, the CPU rate is in percent of a CPU.
        #   We'll set the CPU rate to cpu_limit (so 80 means 80% of a CPU).
        # - Memory limit: we can set the job memory limit.
        # - IO limits: we can set IO rate control (Windows 8+).
        # We'll generate a script that uses the Set-JobObject cmdlet if available, or use the Win32 API via PowerShell.
        # For simplicity, we'll use the Set-JobObject cmdlet (requires Windows 8+).
        # We'll create a job object, set the limits, and then return the job object handle.

        ps_script = f"""# Generated PowerShell script to create a Job Object for service '{service.name}'
# This script creates a Job Object and sets resource limits.
# To apply the Job Object to a service, you need to assign the service's processes to this Job Object.
# Run this script as Administrator.

$JobName = "ServiceLimiter_{service.name}"

# Try to remove the job object if it already exists
try {{
    Get-JobObject -Name $JobName | Remove-JobObject
}} catch {{
    # Ignore if it doesn't exist
}}

# Create a new job object
$Job = New-JobObject -Name $JobName

# Set CPU rate control (if supported)
try {{
    Set-JobObject -Job $Job -CPURate {cpu_limit}
}} catch {{
    Write-Warning "Failed to set CPU rate limit. This requires Windows 8 or later." }}

# Set memory limit
try {{
    Set-JobObject -Job $Job -MemoryLimit {memory_limit}
}} catch {{
    Write-Warning "Failed to set memory limit." }}

# Set IO read bandwidth limit (if supported)
try {{
    Set-JobObject -Job $Job -IOReadBandwidth {read_io_limit}
}} catch {{
    Write-Warning "Failed to set IO read bandwidth limit. This requires Windows 8 or later." }}

# Set IO write bandwidth limit (if supported)
try {{
    Set-JobObject -Job $Job -IOWriteBandwidth {write_io_limit}
}} catch {{
    Write-Warning "Failed to set IO write bandwidth limit. This requires Windows 8 or later." }}

# Output the job object handle for the user to assign processes
Write-Output "Job Object created. Assign processes to this job object using the handle: $Job.Handle"
Write-Output "To get the service's process IDs, run: Get-WmiObject -Query \"SELECT ProcessId FROM Win32_Service WHERE Name='{service.name}'\""
Write-Output "Then for each PID, run: \$job.AssignProcess(<PID>)"
"""
        return {
            'content': ps_script,
            'file_path': f"C:\\ServiceLimiter\\{service.name}-JobLimits.ps1",
            'directory': "C:\\ServiceLimiter"
        }

