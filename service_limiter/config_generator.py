"""Configuration generator for OS-specific limits."""

import json
import os

class ConfigGenerator:
    def __init__(self, output_dir):
        self.output_dir = output_dir
        os.makedirs(output_dir, exist_ok=True)

    def generate(self, limited_service):
        """Generate a config dict for a service (to be implemented per OS)."""
        # This is a placeholder. In a real implementation, we would generate:
        # - For Linux: systemd override snippets (CPUQuota, MemoryMax, etc.)
        # - For Windows: Job Object settings or via registry/WMI.
        # For now, we just return a dict representing the desired limits.
        return {
            'service_name': limited_service.name,
            'cpu_limit_percent': 80,   # example
            'memory_limit_mb': 512,
            'io_read_limit_kbps': 1024,
            'io_write_limit_kbps': 512
        }

    def write_config(self, service_name, config):
        """Write the config to a file in the output directory."""
        # For simplicity, we'll write JSON files. In reality, we might write .conf, .ini, or .xml.
        safe_name = "".join(c if c.isalnum() else "_" for c in service_name)
        path = os.path.join(self.output_dir, f"{safe_name}.json")
        with open(path, 'w') as f:
            json.dump(config, f, indent=2)
        return path
