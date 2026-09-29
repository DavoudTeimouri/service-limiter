"""Policy engine for evaluating resource limits."""

from typing import Dict, Any, List, Tuple


class PolicyEngine:
    """Evaluates resource usage against policies."""

    def __init__(self):
        # Default policies: CPU 80%, Memory 512MB, Read IO 1MB/s, Write IO 512KB/s
        self.policies = [
            {
                "name": "default",
                "cpu_percent": 80,
                "memory_mb": 512,
                "io_read_kbps": 1024,   # 1 MB/s
                "io_write_kbps": 512,   # 512 KB/s
            }
        ]

    def evaluate(self, profile: Dict[str, Any], policy: Dict[str, Any] = None) -> Tuple[bool, List[str]]:
        """
        Evaluate a resource profile against a policy.
        Returns (is_over_limit, list_of_violations).
        """
        if policy is None:
            # Use the first default policy
            policy = self.policies[0]

        violations = []
        # Check CPU
        if profile.get('cpu_percent', 0) > policy.get('cpu_percent', 80):
            violations.append(f"CPU {profile.get('cpu_percent', 0)}% > {policy.get('cpu_percent', 80)}%")
        # Check Memory
        if profile.get('memory_mb', 0) > policy.get('memory_mb', 512):
            violations.append(f"Memory {profile.get('memory_mb', 0)}MB > {policy.get('memory_mb', 512)}MB")
        # Check Read IO
        if profile.get('io_read_kbps', 0) > policy.get('io_read_kbps', 1024):
            violations.append(f"Read IO {profile.get('io_read_kbps', 0)}KB/s > {policy.get('io_read_kbps', 1024)}KB/s")
        # Check Write IO
        if profile.get('io_write_kbps', 0) > policy.get('io_write_kbps', 512):
            violations.append(f"Write IO {profile.get('io_write_kbps', 0)}KB/s > {policy.get('io_write_kbps', 512)}KB/s")

        is_over = len(violations) > 0
        return is_over, violations

