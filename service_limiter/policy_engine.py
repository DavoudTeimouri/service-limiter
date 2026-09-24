"""Policy engine for evaluating resource limits."""

class PolicyEngine:
    def __init__(self):
        # Default policies (can be loaded from config)
        self.default_policies = {
            'cpu_percent': 80,   # max 80% CPU
            'memory_mb': 512,    # max 512 MB
            'io_read_kbps': 1024,# max 1 MB/s read
            'io_write_kbps': 512 # max 512 KB/s write
        }

    def evaluate(self, profile, policy=None):
        """Evaluate if a resource profile is within the given policy.
        Returns (bool, reason) where bool is True if within policy.
        """
        if policy is None:
            policy = self.default_policies
        # Check each metric
        if profile.cpu_percent > policy.get('cpu_percent', float('inf')):
            return False, f"CPU usage {profile.cpu_percent}% > limit {policy.get('cpu_percent')}%"
        if profile.memory_mb > policy.get('memory_mb', float('inf')):
            return False, f"Memory usage {profile.memory_mb}MB > limit {policy.get('memory_mb')}MB"
        if profile.io_read_kbps > policy.get('io_read_kbps', float('inf')):
            return False, f"IO read {profile.io_read_kbps}KB/s > limit {policy.get('io_read_kbps')}KB/s"
        if profile.io_write_kbps > policy.get('io_write_kbps', float('inf')):
            return False, f"IO write {profile.io_write_kbps}KB/s > limit {policy.get('io_write_kbps')}KB/s"
        return True, "Within policy"

    def get_exceeding_metrics(self, profile, policy=None):
        """Return a dict of metrics that exceed the policy and by how much."""
        if policy is None:
            policy = self.default_policies
        exceeding = {}
        if profile.cpu_percent > policy.get('cpu_percent', float('inf')):
            exceeding['cpu_percent'] = profile.cpu_percent - policy.get('cpu_percent', 0)
        if profile.memory_mb > policy.get('memory_mb', float('inf')):
            exceeding['memory_mb'] = profile.memory_mb - policy.get('memory_mb', 0)
        if profile.io_read_kbps > policy.get('io_read_kbps', float('inf')):
            exceeding['io_read_kbps'] = profile.io_read_kbps - policy.get('io_read_kbps', 0)
        if profile.io_write_kbps > policy.get('io_write_kbps', float('inf')):
            exceeding['io_write_kbps'] = profile.io_write_kbps - policy.get('io_write_kbps', 0)
        return exceeding

