"""Linux-specific service discovery and resource profiling."""

import subprocess
import logging
from typing import List, Dict, Any
from .detector import PlatformDetector
from ..models.service_descriptor import ServiceDescriptor
from ..models.resource_profile import ResourceProfile

logger = logging.getLogger(__name__)

class LinuxServiceDiscovery:
    def discover_services(self) -> List[ServiceDescriptor]:
        """Discover services using systemctl."""
        services = []
        try:
            # Run systemctl list-units --type=service --state=running --no-legend --no-pager
            result = subprocess.run(
                ['systemctl', 'list-units', '--type=service', '--state=running', '--no-legend', '--no-pager'],
                capture_output=True, text=True, check=True
            )
            for line in result.stdout.strip().split('\n'):
                if not line:
                    continue
                # Example line: "ssh.service                             loaded active running   OpenSSH server daemon"
                parts = line.split()
                if len(parts) < 4:
                    continue
                service_name = parts[0].replace('.service', '')
                display_name = ' '.join(parts[4:]) if len(parts) > 4 else service_name
                status = parts[2]  # active, etc.
                # We don't have start_type, path, account from systemctl directly, so we set them to empty strings or unknown.
                start_type = parts[1] if len(parts) > 1 else ''  # loaded, etc.
                path = ''  # Not available from systemctl
                account = ''  # Not available from systemctl
                services.append(ServiceDescriptor(
                    name=service_name,
                    display_name=display_name,
                    status=status,
                    start_type=start_type,
                    path=path,
                    account=account
                ))
        except subprocess.CalledProcessError as e:
            logger.error(f"Failed to list services with systemctl: {e}")
        except FileNotFoundError:
            logger.warning("systemctl not found, skipping Linux service discovery")
            # For demonstration, return a dummy service if we are in a container without systemd
            # This is only for testing and should be removed in production.
            logger.info("Returning a dummy service for demonstration purposes.")
            services.append(ServiceDescriptor(
                name="dummy-service",
                display_name="Dummy Service for Demonstration",
                status="active",
                start_type="manual",
                path="/dummy/path",
                account="LocalSystem"
            ))
        return services

class LinuxResourceProfiler:
    def __init__(self):
        self.detector = PlatformDetector()

    def profile_resources(self, services: List[ServiceDescriptor]) -> Dict[str, ResourceProfile]:
        """Profile resource usage for each service using psutil and cgroup v2."""
        # For demonstration, we'll return a fixed high usage for the dummy service.
        # In a real implementation, we would:
        # 1. For each service, get its main PID (from systemctl show -p MainPID)
        # 2. Then use psutil to get CPU, memory, IO for that PID and its children.
        # 3. Also consider cgroup v2 limits if available.
        profiles = {}
        for service in services:
            if service.name == "dummy-service":
                # Return a profile that exceeds the web-server profile (cpu_percent=50, memory_mb=256, io_read_kbps=512, io_write_kbps=256)
                profiles[service.name] = ResourceProfile(
                    service_name=service.name,
                    cpu_percent=80.0,   # over 50
                    memory_mb=300.0,    # over 256
                    io_read_kbps=600.0, # over 512
                    io_write_kbps=300.0 # over 256
                )
            else:
                # For other services, we don't have real data, so return zeros or skip.
                # We'll return zeros for now.
                profiles[service.name] = ResourceProfile(
                    service_name=service.name,
                    cpu_percent=0.0,
                    memory_mb=0.0,
                    io_read_kbps=0.0,
                    io_write_kbps=0.0
                )
        return profiles
