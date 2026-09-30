"""Windows-specific service discovery and resource profiling."""

import json
import logging
import subprocess
import time
from typing import Dict, List

import psutil

from ..models.resource_profile import ResourceProfile
from ..models.service_descriptor import ServiceDescriptor

logger = logging.getLogger(__name__)


def _powershell() -> str:
    """Prefer PowerShell 7+, fall back to the deprecated 5.1 host."""
    for exe in ("pwsh", "powershell"):
        if subprocess.run([exe, "-NoProfile", "-Command", "$PSVersionTable.PSVersion"],
                          capture_output=True).returncode == 0:
            return exe
    raise FileNotFoundError("No PowerShell interpreter found (tried pwsh, powershell)")


class WindowsServiceDiscovery:
    def discover_services(self) -> List[ServiceDescriptor]:
        """Discover services with their PIDs via CIM.

        Get-CimInstance Win32_Service is used rather than the deprecated
        Get-WmiObject, and ProcessId is requested so profiling can attach.
        """
        services: List[ServiceDescriptor] = []
        ps = (
            "Get-CimInstance Win32_Service | "
            "Select-Object Name,DisplayName,State,StartMode,PathName,ProcessId,StartName | "
            "ConvertTo-Json -Compress"
        )
        try:
            result = subprocess.run(
                [_powershell(), "-NoProfile", "-Command", ps],
                capture_output=True, text=True, check=True,
            )
        except subprocess.CalledProcessError as e:
            logger.error("Failed to list services via PowerShell: %s", e.stderr or e)
            return services
        except FileNotFoundError:
            logger.error("PowerShell not found: cannot enumerate services.")
            return services

        try:
            data = json.loads(result.stdout)
        except json.JSONDecodeError as e:
            logger.error("Failed to parse JSON from PowerShell: %s", e)
            return services
        if isinstance(data, dict):          # a single service serialises as an object
            data = [data]

        for svc in data:
            pid = svc.get("ProcessId") or 0
            services.append(ServiceDescriptor(
                name=svc.get("Name", ""),
                display_name=svc.get("DisplayName") or svc.get("Name", ""),
                status=svc.get("State", ""),
                start_type=svc.get("StartMode", ""),
                path=svc.get("PathName", "") or "",
                account=svc.get("StartName", "") or "",
                child_processes=[pid] if pid else [],
            ))
        return services


class WindowsResourceProfiler:
    def __init__(self, interval: float = 0.25):
        self.interval = interval

    def profile_resources(self, services: List[ServiceDescriptor]) -> Dict[str, ResourceProfile]:
        """Measure real CPU/memory/IO for each service process tree."""
        from .linux import _sample  # identical measurement, no OS-specific code

        profiles: Dict[str, ResourceProfile] = {}
        for service in services:
            if not service.child_processes:
                continue
            measured = _sample(service.child_processes[0], self.interval)
            if measured is None:
                continue
            measured.service_name = service.name
            profiles[service.name] = measured
        return profiles
