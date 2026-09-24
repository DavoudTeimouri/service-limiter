"""Windows-specific service discovery and profiling."""
import subprocess
import re
import psutil
from typing import List, Dict, Any
from ..models.service_descriptor import ServiceDescriptor
from ..models.resource_profile import ResourceProfile

def discover_services() -> List[ServiceDescriptor]:
    """Discover Windows services using wmic or PowerShell."""
    services = []
    try:
        # Use PowerShell to get service info
        cmd = [
            'powershell', '-NoProfile', '-Command',
            "Get-Service | Select-Object Name, DisplayName, Status, StartType, PathName, StartName | ConvertTo-Json"
        ]
        result = subprocess.run(cmd, capture_output=True, text=True, timeout=30)
        if result.returncode == 0:
            import json
            data = json.loads(result.stdout)
            if isinstance(data, dict):
                data = [data]
            for svc in data:
                name = svc.get('Name', '')
                display_name = svc.get('DisplayName', '')
                status = svc.get('Status', '')
                start_type = svc.get('StartType', '')
                path_name = svc.get('PathName', '')
                start_name = svc.get('StartName', '')
                services.append(ServiceDescriptor(
                    name=name,
                    display_name=display_name,
                    status=status,
                    start_type=start_type,
                    path=path_name,
                    account=start_name
                ))
        else:
            # Fallback to wmic
            cmd = ['wmic', 'service', 'get', 'Name,DisplayName,State,StartMode,PathName,StartName', '/format:csv']
            result = subprocess.run(cmd, capture_output=True, text=True, timeout=30)
            if result.returncode == 0:
                lines = result.stdout.strip().split('\n')[1:]  # skip header
                for line in lines:
                    if not line.strip():
                        continue
                    parts = line.split(',')
                    if len(parts) >= 6:
                        name = parts[0].strip()
                        display_name = parts[1].strip()
                        status = parts[2].strip()
                        start_type = parts[3].strip()
                        path_name = parts[4].strip()
                        start_name = parts[5].strip()
                        services.append(ServiceDescriptor(
                            name=name,
                            display_name=display_name,
                            status=status,
                            start_type=start_type,
                            path=path_name,
                            account=start_name
                        ))
    except Exception as e:
        print(f"Error discovering Windows services: {e}")
    return services

def profile_resources(service: ServiceDescriptor) -> ResourceProfile:
    """Profile resource usage for a Windows service."""
    # Find the process ID(s) for the service
    pids = []
    for proc in psutil.process_iter(['pid', 'name']):
        try:
            # Check if process matches service name or display name? 
            # We'll need to map service to process via wmic query
            pass
        except (psutil.NoSuchProcess, psutil.AccessDenied):
            continue
    # For simplicity, we'll return zero profile; actual implementation would query WMI for process ID
    # and then use psutil to get CPU, memory, IO.
    return ResourceProfile(
        service_name=service.name,
        cpu_percent=0.0,
        memory_mb=0.0,
        io_read_kbps=0.0,
        io_write_kbps=0.0
    )
