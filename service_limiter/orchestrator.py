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

logger = logging.getLogger(__name__)

class Orchestrator:
    def __init__(self, profile_name: str = None):
        self.detector = PlatformDetector()
        self.shared_state = SharedState()
        self.policy_engine = PolicyEngine()
        self.profile = None
        if profile_name:
            self.profile = self._load_profile(profile_name)

    def _load_profile(self, profile_name: str) -> Dict[str, Any]:
        """Load a profile JSON shipped inside the package."""
        from importlib.resources import files
        profile_path = files("service_limiter") / "profiles" / f"{profile_name}.json"
        try:
            return json.loads(profile_path.read_text())
        except FileNotFoundError:
            raise SystemExit(
                f"Profile '{profile_name}' not found. Available: "
                f"{', '.join(self.available_profiles())}"
            )
        except json.JSONDecodeError as e:
            raise SystemExit(f"Invalid JSON in profile '{profile_name}': {e}")

    @staticmethod
    def available_profiles() -> List[str]:
        """Names of bundled profiles, without the .json suffix."""
        from importlib.resources import files
        profiles_dir = files("service_limiter") / "profiles"
        return sorted(p.name[:-5] for p in profiles_dir.iterdir() if p.name.endswith(".json"))

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
        # systemd I/O bandwidth limits are PER DEVICE and need the unified cgroup
        # hierarchy (io.max). Correct directive names are IOReadBandwidthMax /
        # IOWriteBandwidthMax, each taking "<device> <bytes-per-second>".
        # The old ReadBandwidthMax/WriteBandwidthMax were not real keys: systemd
        # ignored them silently, so no I/O limit was ever applied.
        device = policy.get('io_device', '')
        if device:
            read_bps = policy.get('io_read_kbps', 1024) * 1024
            write_bps = policy.get('io_write_kbps', 512) * 1024
            content += f"""IOAccounting=yes
IOReadBandwidthMax={device} {read_bps}
IOWriteBandwidthMax={device} {write_bps}
"""
        else:
            # ponytail: no io_device in the policy -> I/O limits are skipped rather
            # than emitted invalid. Add an io_device field to profiles to enable.
            logger.warning(
                "Policy has no 'io_device'; skipping I/O bandwidth limits "
                "(systemd requires a block device path)."
            )
        return {
            'content': content,
            'file_path': f"/etc/systemd/system/{service.name}.service.d/override.conf",
            'directory': f"/etc/systemd/system/{service.name}.service.d"
        }

    def _generate_windows_config(self, service: ServiceDescriptor, policy: Dict[str, Any]) -> Dict[str, Any]:
        """Generate a PowerShell script that limits a service via a Job Object.

        Windows Job Objects are a Win32 kernel API. PowerShell ships NO built-in
        cmdlets for them (New-JobObject / Set-JobObject / Get-JobObject /
        Remove-JobObject do not exist), so the script P/Invokes kernel32 via
        Add-Type and assigns the service's own process to the new job.
        """
        mem_bytes = policy.get('memory_mb', 512) * 1024 * 1024
        read_bps = policy.get('io_read_kbps', 1024) * 1024
        write_bps = policy.get('io_write_kbps', 512) * 1024
        # JOB_OBJECT_LIMIT_RATE_HARD expresses CPU as units of 1/10000 of a CPU.
        cpu_rate = int(policy.get('cpu_percent', 80)) * 100

        ps_script = """# Generated by service-limiter: Job Object limits for service '{service}'
# Run as Administrator (elevated). Requires Windows 8 or newer.
$ErrorActionPreference = 'Stop'

Add-Type -TypeDefinition @"
using System;
using System.ComponentModel;
using System.Runtime.InteropServices;
public static class SLJob {{
    [StructLayout(LayoutKind.Sequential)]
    public struct IO_COUNTERS {{
        public ulong ReadOperationCount, WriteOperationCount, OtherOperationCount;
        public ulong ReadTransferCount, WriteTransferCount, OtherTransferCount;
    }}
    [StructLayout(LayoutKind.Sequential)]
    public struct JOBOBJECT_BASIC_LIMIT_INFORMATION {{
        public long PerProcessUserTimeLimit, PerJobUserTimeLimit;
        public uint LimitFlags;
        public UIntPtr MinimumWorkingSetSize, MaximumWorkingSetSize;
        public uint ActiveProcessLimit;
        public UIntPtr Affinity;
        public uint PriorityClass, SchedulingClass;
    }}
    [StructLayout(LayoutKind.Sequential)]
    public struct JOBOBJECT_EXTENDED_LIMIT_INFORMATION {{
        public JOBOBJECT_BASIC_LIMIT_INFORMATION BasicLimitInformation;
        public IO_COUNTERS IoInfo;
        public UIntPtr ProcessMemoryLimit, JobMemoryLimit, PeakProcessMemoryUsed, PeakJobMemoryUsed;
    }}

    [DllImport("kernel32.dll", CharSet = CharSet.Unicode, SetLastError = true)]
    public static extern IntPtr CreateJobObject(IntPtr attrs, string name);
    [DllImport("kernel32.dll", SetLastError = true)]
    public static extern bool SetInformationJobObject(IntPtr job, int cls, IntPtr info, uint len);
    [DllImport("kernel32.dll", SetLastError = true)]
    public static extern bool AssignProcessToJobObject(IntPtr job, IntPtr process);
    [DllImport("kernel32.dll", SetLastError = true)]
    public static extern IntPtr OpenProcess(uint access, bool inherit, uint pid);
}}
"@

$svc = Get-CimInstance Win32_Service -Filter "Name='{service}'"
if (-not $svc) {{ Write-Error "Service '{service}' not found"; exit 1 }}
if ($svc.ProcessId -eq 0) {{ Write-Error "Service '{service}' is not running"; exit 1 }}

$job = [SLJob]::CreateJobObject([IntPtr]::Zero, $null)
if ($job -eq [IntPtr]::Zero) {{ throw "CreateJobObject failed: $([Runtime.InteropServices.Marshal]::GetLastWin32Error())" }}

$info = New-Object SLJob+JOBOBJECT_EXTENDED_LIMIT_INFORMATION
$info.BasicLimitInformation.LimitFlags = 0x00000020 -bor 0x00000400   # JOB_MEMORY | JOB_IO_RATE
$info.JobMemoryLimit = [UIntPtr]{mem_bytes}
$info.IoInfo.ReadTransferCount = {read_bps}
$info.IoInfo.WriteTransferCount = {write_bps}

$len = [Runtime.InteropServices.Marshal]::SizeOf($info)
$ptr = [Runtime.InteropServices.Marshal]::AllocHGlobal($len)
try {{
    [Runtime.InteropServices.Marshal]::StructureToPtr($info, $ptr, $false)
    if (-not [SLJob]::SetInformationJobObject($job, 9, $ptr, $len)) {{
        throw "SetInformationJobObject failed: $([Runtime.InteropServices.Marshal]::GetLastWin32Error())"
    }}
}} finally {{
    [Runtime.InteropServices.Marshal]::FreeHGlobal($ptr)
}}

$PROCESS_QUERY_SET_INFORMATION = 0x0400
$PROCESS_TERMINATE = 0x0001
$h = [SLJob]::OpenProcess($PROCESS_QUERY_SET_INFORMATION -bor $PROCESS_TERMINATE, $false, $svc.ProcessId)
if ($h -eq [IntPtr]::Zero) {{ throw "OpenProcess failed: $([Runtime.InteropServices.Marshal]::GetLastWin32Error())" }}
if (-not [SLJob]::AssignProcessToJobObject($job, $h)) {{
    Write-Warning "AssignProcessToJobObject failed: $([Runtime.InteropServices.Marshal]::GetLastWin32Error())"
    Write-Warning "The service may already run inside a job object (common under IIS or a service host)."
}} else {{
    Write-Output "Applied memory={{mem_bytes}}B IO={{read_bps}}/{{write_bps}}B/s to PID $($svc.ProcessId)"
    Write-Output "Note: the job handle is released when this script exits; limits lapse with it."
}}
""".format(service=service.name, mem_bytes=mem_bytes, read_bps=read_bps,
           write_bps=write_bps)

        return {
            'content': ps_script,
            'file_path': f"C:\\ServiceLimiter\\{service.name}\\Set-JobLimits.ps1",
            'directory': "C:\\ServiceLimiter"
        }
