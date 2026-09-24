"""Windows-specific service discovery and resource profiling."""

import subprocess
import logging
from typing import List, Dict, Any
from .detector import PlatformDetector
from ..models.service_descriptor import ServiceDescriptor
from ..models.resource_profile import ResourceProfile

logger = logging.getLogger(__name__)

class WindowsServiceDiscovery:
    def discover_services(self) -> List[ServiceDescriptor]:
        """Discover services using PowerShell Get-Service."""
        services = []
        try:
            # Run PowerShell command to get services
            # We'll get Name, DisplayName, Status
            ps_command = "Get-Service | Select-Object Name, DisplayName, Status | ConvertTo-Json"
            result = subprocess.run(
                ['powershell', '-NoProfile', '-Command', ps_command],
                capture_output=True, text=True, check=True
            )
            # Parse JSON output
            import json
            services_data = json.loads(result.stdout)
            # Ensure it's a list
            if isinstance(services_data, dict):
                services_data = [services_data]
            for svc in services_data:
                name = svc.get('Name', '')
                display_name = svc.get('DisplayName', name)
                status = svc.get('Status', '')
                services.append(ServiceDescriptor(
                    name=name,
                    display_name=display_name,
                    status=status
                ))
        except subprocess.CalledProcessError as e:
            logger.error(f"Failed to list services with PowerShell: {e}")
        except FileNotFoundError:
            logger.warning("PowerShell not found, skipping Windows service discovery")
        except json.JSONDecodeError as e:
            logger.error(f"Failed to parse JSON from PowerShell: {e}")
        return services

class WindowsResourceProfiler:
    def __init__(self):
        self.detector = PlatformDetector()

    def profile_resources(self, services: List[ServiceDescriptor]) -> List[ResourceProfile]:
        """Profile resource usage for each service using psutil and WMI (if available) or PowerShell.
        This is a complex task and requires mapping service names to PIDs.
        For now, we return empty profiles and note that this is a placeholder.
        """
        logger.warning("Windows resource profiling is not fully implemented; returning empty profiles.")
        return []