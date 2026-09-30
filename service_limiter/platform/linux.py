"""Linux-specific service discovery and resource profiling."""

import logging
import subprocess
import time
from typing import Dict, List, Optional

import psutil

from ..models.resource_profile import ResourceProfile
from ..models.service_descriptor import ServiceDescriptor

logger = logging.getLogger(__name__)


def _main_pid(unit: str) -> Optional[int]:
    """MainPID of a systemd unit, or None when it has no live process."""
    try:
        result = subprocess.run(
            ["systemctl", "show", unit, "-p", "MainPID", "--value"],
            capture_output=True, text=True, check=True,
        )
        pid = int(result.stdout.strip())
        return pid or None
    except (subprocess.CalledProcessError, ValueError, FileNotFoundError) as e:
        logger.debug("MainPID unavailable for %s: %s", unit, e)
        return None


def _sample(pid: int, interval: float) -> Optional[ResourceProfile]:
    """CPU/memory/IO totals for a process tree.

    CPU is sampled twice: psutil returns a meaningless 0.0 on a process
    objects first cpu_percent() call.
    """
    try:
        root = psutil.Process(pid)
    except (psutil.NoSuchProcess, psutil.AccessDenied):
        return None

    procs = [root]
    try:
        procs += root.children(recursive=True)
    except (psutil.NoSuchProcess, psutil.AccessDenied):
        pass

    for p in procs:
        try:
            p.cpu_percent(None)  # prime the counter
        except (psutil.NoSuchProcess, psutil.AccessDenied):
            continue
    psutil.cpu_percent(None)
    time.sleep(interval)

    cpu = rss = read_bps = write_bps = 0.0
    for p in procs:
        try:
            with p.oneshot():
                cpu += p.cpu_percent(None)
                rss += p.memory_info().rss
                io = p.io_counters()
                read_bps += io.read_bytes / 1024 / interval
                write_bps += io.write_bytes / 1024 / interval
        except (psutil.NoSuchProcess, psutil.AccessDenied, psutil.ZombieProcess):
            continue

    return ResourceProfile(
        service_name="",
        cpu_percent=round(cpu, 2),
        memory_mb=round(rss / 1024 / 1024, 2),
        io_read_kbps=round(read_bps, 2),
        io_write_kbps=round(write_bps, 2),
    )


class LinuxServiceDiscovery:
    def discover_services(self) -> List[ServiceDescriptor]:
        """Discover running services using systemctl."""
        services: List[ServiceDescriptor] = []
        try:
            result = subprocess.run(
                ["systemctl", "list-units", "--type=service", "--state=running",
                 "--no-legend", "--no-pager"],
                capture_output=True, text=True, check=True,
            )
        except subprocess.CalledProcessError as e:
            logger.error("Failed to list services with systemctl: %s", e)
            return services
        except FileNotFoundError:
            logger.error("systemctl not found: this host does not run systemd.")
            return services

        for line in result.stdout.strip().splitlines():
            parts = line.split()
            # A unit line looks like:
            #   ssh.service loaded active running OpenSSH server daemon
            # systemctl also emits a trailing "N loaded units listed." summary,
            # which must not be parsed as a service.
            if len(parts) < 4 or not parts[0].endswith(".service"):
                continue
            unit = parts[0][:-len(".service")]
            services.append(ServiceDescriptor(
                name=unit,
                display_name=" ".join(parts[4:]) or unit,
                status=parts[2],
                start_type=parts[1],
                path="",
                account="",
            ))
        return services


class LinuxResourceProfiler:
    def __init__(self, interval: float = 0.25):
        self.interval = interval

    def profile_resources(self, services: List[ServiceDescriptor]) -> Dict[str, ResourceProfile]:
        """Measure real CPU/memory/IO for each service process tree."""
        profiles: Dict[str, ResourceProfile] = {}
        for service in services:
            pid = _main_pid(service.name)
            if pid is None:
                logger.debug("No live MainPID for %s, skipping", service.name)
                continue
            measured = _sample(pid, self.interval)
            if measured is None:
                logger.debug("Could not sample %s (pid %s)", service.name, pid)
                continue
            measured.service_name = service.name
            try:
                service.child_processes = [
                    p.pid for p in psutil.Process(pid).children(recursive=True)
                ]
            except (psutil.NoSuchProcess, psutil.AccessDenied):
                service.child_processes = []
            profiles[service.name] = measured
        return profiles
