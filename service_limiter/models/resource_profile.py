"""Resource profile model."""

class ResourceProfile:
    def __init__(self, service_name: str, cpu_percent: float, memory_mb: float,
                 io_read_kbps: float, io_write_kbps: float):
        self.service_name = service_name
        self.cpu_percent = cpu_percent
        self.memory_mb = memory_mb
        self.io_read_kbps = io_read_kbps
        self.io_write_kbps = io_write_kbps

    def to_dict(self):
        return {
            'service_name': self.service_name,
            'cpu_percent': self.cpu_percent,
            'memory_mb': self.memory_mb,
            'io_read_kbps': self.io_read_kbps,
            'io_write_kbps': self.io_write_kbps
        }
