"""Platform detection for Windows and Linux."""

import platform
import sys
from typing import Dict, Any

class PlatformDetector:
    def detect(self) -> Dict[str, Any]:
        """Detect OS and architecture."""
        system = platform.system()
        machine = platform.machine()
        return {
            'system': system,
            'machine': machine,
            'is_windows': system == 'Windows',
            'is_linux': system == 'Linux',
            'is_x64': machine.endswith('64') or machine in ('AMD64', 'x86_64'),
        }
