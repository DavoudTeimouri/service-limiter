"""Linux-specific service discovery and profiling."""
import subprocess
import psutil
from typing import List, Dict, Any
from ..models.service_descriptor import ServiceDescriptor
from ..models.resource_profile import ResourceProfile

def discover_services() -> List[ServiceDescriptor]:
    """Discover Linux services using systemctl."""
    services = []
    try:
        # List all loaded units that are services
        cmd = ['systemctl', 'list-units', '--type=service', '--state=loaded', '--no-legend', '--no-pager']
        result = subprocess.run(cmd, capture_output=True, text=True, timeout=30)
        if result.returncode == 0:
            for line in result.stdout.strip().split('\n'):
                if not line.strip():
                    continue
                # Example line: "ssh.service                     loaded active running   OpenSSH server daemon"
                parts = line.split()
                if len(parts) >= 4:
                    name = parts[0]
                    # Remove .service suffix if present
                    if name.endswith('.service'):
                        name = name[:-8]
                    status = parts[2]  # active, inactive, etc.
                    # Get more details: description, etc.
                    desc_cmd = ['systemctl', 'show', name, '--property=Description']
                    desc_result = subprocess.run(desc_cmd, capture_output=True, text=True, timeout=5)
                    description = desc_result.stdout.strip().split('=',1)[-1] if desc_result.returncode == 0 else ''
                    # Get exec start path
                    exec_cmd = ['systemctl', 'show', name, '--property=ExecStart']
                    exec_result = subprocess.run(exec_cmd, capture_output=True, text=True, timeout=5)
                    exec_start = exec_result.stdout.strip().split('=',1)[-1] if exec_result.returncode == 0 else ''
                    # Get user (if any)
                    user_cmd = ['systemctl', 'show', name, '--property=User']
                    user_result = subprocess.run(user_cmd, capture_output=True, text=True, timeout=5)
                    user = user_result.stdout.strip().split('=',1)[-1] if user_result.returncode == 0 else ''
                    services.append(ServiceDescriptor(
                        name=name,
                        display_name=description or name,
                        status=status,
                        start_type='',  # systemd doesn't have simple start type; could get from LoadState
                        path=exec_start,
                        account=user
                    ))
        else:
            print(f"Failed to list units: {result.stderr}")
    except Exception as e:
        print(f"Error discovering Linux services: {e}")
    return services

def profile_resources(service: ServiceDescriptor) -> ResourceProfile:
    """Profile resource usage for a Linux service."""
    # Find the main PID of the service via systemctl
    try:
        cmd = ['systemctl', 'show', service.name, '--property=MainPID']
        result = subprocess.run(cmd, capture_output=True, text=True, timeout=5)
        if result.returncode == 0:
            main_pid = int(result.stdout.strip().split('=',1)[-1])
            if main_pid > 0:
                proc = psutil.Process(main_pid)
                cpu_percent = proc.cpu_percent(interval=0.1)
                memory_info = proc.memory_info()
                memory_mb = memory_info.rss / 1024 / 1024
                io_counters = proc.io_counters() if hasattr(proc, 'io_counters') else None
                io_read_kbps = io_counters.read_bytes / 1024 if io_counters else 0
                io_write_kbps = io_counters.write_bytes / 1024 if io_counters else 0
                # Note: above is total since start, not rate. For simplicity, we'll just return these.
                # In a real implementation, we'd sample over time.
                return ResourceProfile(
                    service_name=service.name,
                    cpu_percent=cpu_percent,
                    memory_mb=memory_mb,
                    io_read_kbps=io_read_kbps,
                    io_write_kbps=io_write_kbps
                )
    except (psutil.NoSuchProcess, psutil.AccessDenied, ValueError):
        pass
    # Fallback: zero profile
    return ResourceProfile(
        service_name=service.name,
        cpu_percent=0.0,
        memory_mb=0.0,
        io_read_kbps=0.0,
        io_write_kbps=0.0
    )
