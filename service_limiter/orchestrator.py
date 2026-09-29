"""Orchestrator for service limiter application."""

import logging
import json
import os
from typing import Dict, Any, List
from .platform.detector import PlatformDetector
from .models.shared_state import SharedState
from .models.service_descriptor import ServiceDescriptor
from .models.resource_profile import ResourceProfile
from .policy_engine import PolicyEngine
from .config_generator import ConfigGenerator

logger = logging.getLogger(__name__)

class Orchestrator:
    def __init__(self, profile_name: str = None):
        self.detector = PlatformDetector()
        self.shared_state = SharedState()
        self.policy_engine = PolicyEngine()
        self.config_generator = ConfigGenerator()
        self.profile = None
        if profile_name:
            self.profile = self._load_profile(profile_name)

    def _load_profile(self, profile_name: str) -> Dict[str, Any]:
        """Load a profile from the profiles directory."""
        try:
            # Get the absolute path to the profiles directory
            base_dir = os.path.dirname(__file__)
            profiles_dir = os.path.join(base_dir, '..', 'profiles')
            profile_path = os.path.join(profiles_dir, f'{profile_name}.json')
            with open(profile_path, 'r') as f:
                return json.load(f)
        except FileNotFoundError:
            logger.warning(f"Profile {profile_name} not found, using default policies.")
            return None
        except json.JSONDecodeError as e:
            logger.error(f"Invalid JSON in profile {profile_name}: {e}")
            return None

    def run_analysis(self) -> Dict[str, Any]:
        """Run full analysis pipeline."""
        logger.info("Starting service analysis")
        # 1. Detect platform
        platform_info = self.detector.detect()
        self.shared_state.update_platform(platform_info)
        logger.info(f"Platform: {platform_info}")

        # 2. Discover services
        services = self._discover_services()
        self.shared_state.update_services(services)
        logger.info(f"Discovered {len(services)} services")

        # 3. Profile resources
        service_to_profile = self._profile_resources(services)
        profiles_list = list(service_to_profile.values())
        self.shared_state.update_profiles(profiles_list)
        logger.info(f"Profiled {len(profiles_list)} services")

        # 4. Apply policies
        if self.profile:
            policy = self.profile
            logger.info(f"Using profile: {self.profile}")
        else:
            # Use the first default policy
            policy = self.policy_engine.policies[0]
            logger.info(f"Using default policy: {policy}")

        limited_services = self._apply_policies(services, service_to_profile, policy)
        self.shared_state.update_limited_services(limited_services)
        logger.info(f"Applied policies to {len(limited_services)} services")

        # 5. Generate configurations
        configs = self._generate_configs(limited_services, policy)
        self.shared_state.update_configs(configs)
        logger.info(f"Generated {len(configs)} configurations")

        return self.shared_state.to_dict()

    def _discover_services(self) -> List[ServiceDescriptor]:
        """Discover services on the platform."""
        platform_info = self.detector.detect()
        if platform_info['is_linux']:
            try:
                from .platform.linux import LinuxServiceDiscovery
                discovery = LinuxServiceDiscovery()
                return discovery.discover_services()
            except ImportError as e:
                logger.error(f"Failed to import LinuxServiceDiscovery: {e}")
                return []
        elif platform_info['is_windows']:
            try:
                from .platform.windows import WindowsServiceDiscovery
                discovery = WindowsServiceDiscovery()
                return discovery.discover_services()
            except ImportError as e:
                logger.error(f"Failed to import WindowsServiceDiscovery: {e}")
                return []
        else:
            logger.warning(f"Unsupported platform for discovery: {platform_info['system']}")
            return []

    def _profile_resources(self, services: List[ServiceDescriptor]) -> Dict[str, ResourceProfile]:
        """Profile resource usage for each service.
        Returns a dictionary mapping service name to ResourceProfile.
        """
        if not services:
            return {}
        platform_info = self.detector.detect()
        if platform_info['is_linux']:
            try:
                from .platform.linux import LinuxResourceProfiler
                profiler = LinuxResourceProfiler()
                return profiler.profile_resources(services)
            except ImportError as e:
                logger.error(f"Failed to import LinuxResourceProfiler: {e}")
                return {}
        elif platform_info['is_windows']:
            try:
                from .platform.windows import WindowsResourceProfiler
                profiler = WindowsResourceProfiler()
                return profiler.profile_resources(services)
            except ImportError as e:
                logger.error(f"Failed to import WindowsResourceProfiler: {e}")
                return {}
        else:
            logger.warning(f"Unsupported platform for profiling: {platform_info['system']}")
            return {}

    def _apply_policies(self, services: List[ServiceDescriptor], service_to_profile: Dict[str, ResourceProfile], policy: Dict[str, Any]) -> List[ServiceDescriptor]:
        """Apply resource policies to each service profile.
        Returns a list of services that exceed the policy (i.e., need limiting).
        """
        limited = []
        for service in services:
            profile = service_to_profile.get(service.name)
            if profile is None:
                # If we don't have a profile for this service, skip it.
                continue
            is_over, violations = self.policy_engine.evaluate(profile.to_dict(), policy)
            if is_over:
                limited.append(service)
                logger.debug(f"Service {service.name} exceeds policy: {violations}")
        return limited

    def _generate_configs(self, limited_services: List[ServiceDescriptor], policy: Dict[str, Any]) -> Dict[str, Any]:
        """Generate configuration files for services that need limiting.
        Returns a dictionary mapping service name to config data.
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
    Write-Warning "Failed to set CPU rate limit. This requires Windows 8 or later."
}}

# Set memory limit
try {{
    Set-JobObject -Job $Job -MemoryLimit {memory_limit}
}} catch {{
    Write-Warning "Failed to set memory limit."
}}

# Set IO read bandwidth limit (if supported)
try {{
    Set-JobObject -Job $Job -IOReadBandwidth {read_io_limit}
}} catch {{
    Write-Warning "Failed to set IO read bandwidth limit. This requires Windows 8 or later."
}}

# Set IO write bandwidth limit (if supported)
try {{
    Set-JobObject -Job $Job -IOWriteBandwidth {write_io_limit}
}} catch {{
    Write-Warning "Failed to set IO write bandwidth limit. This requires Windows 8 or later."
}}

# Output the job object handle for the user to assign processes
Write-Output "Job Object created. Assign processes to this job object using the handle: $Job.Handle"
Write-Output "To get the service's process IDs, run: Get-WmiObject -Query \"SELECT ProcessId FROM Win32_Service WHERE Name='{service.name}'\""
Write-Output "Then for each PID, run: $job.AssignProcess(<PID>)"
"""
        return {
            'content': ps_script,
            'file_path': f"C:\\ServiceLimiter\\{service.name}-JobLimits.ps1",
            'directory': "C:\\ServiceLimiter"
        }
