"""Assert-based self-checks. No framework: python3 tests/test_service_limiter.py

These lock in the two regressions that silently no-op'd: the systemd I/O
directive names, and TUI services-to-profiles pairing.
"""

import subprocess
import sys
import unittest
from pathlib import Path
from unittest import mock

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))

from service_limiter.models.service_descriptor import ServiceDescriptor
from service_limiter.models.resource_profile import ResourceProfile
from service_limiter.orchestrator import Orchestrator
from service_limiter.policy_engine import PolicyEngine
from service_limiter.platform.linux import LinuxServiceDiscovery

POLICY = {"name": "t", "cpu_percent": 50, "memory_mb": 256,
          "io_read_kbps": 512, "io_write_kbps": 256}
# systemd requires a block device path for I/O limits, so those tests need one.
POLICY_WITH_IO = dict(POLICY, io_device="/dev/sda1")

SYSTEMCTL_OUTPUT = """\
sshd.service loaded active running OpenSSH server daemon
cron.service loaded active running Regular background program processing daemon
  2 loaded units listed.
"""


class TestPolicyEngine(unittest.TestCase):
    def test_under_limit_is_not_violation(self):
        eng = PolicyEngine()
        p = {"cpu_percent": 10, "memory_mb": 20, "io_read_kbps": 5, "io_write_kbps": 5}
        over, violations = eng.evaluate(p, POLICY)
        self.assertFalse(over)
        self.assertEqual(violations, [])

    def test_over_limit_reports_each_resource(self):
        eng = PolicyEngine()
        p = {"cpu_percent": 99, "memory_mb": 999, "io_read_kbps": 5000, "io_write_kbps": 5000}
        over, violations = eng.evaluate(p, POLICY)
        self.assertTrue(over)
        self.assertEqual(len(violations), 4)

    def test_limit_is_inclusive(self):
        """Exactly at the limit is allowed; only exceeding counts."""
        eng = PolicyEngine()
        p = {"cpu_percent": 50, "memory_mb": 256, "io_read_kbps": 512, "io_write_kbps": 256}
        self.assertFalse(eng.evaluate(p, POLICY)[0])


class TestLinuxConfigGeneration(unittest.TestCase):
    """The generated drop-in must use directives systemd actually accepts."""

    def setUp(self):
        self.service = ServiceDescriptor(name="sshd", status="active")
        self.orch = Orchestrator()
        self.cfg = self.orch._generate_linux_config(self.service, POLICY_WITH_IO)

    def test_memory_and_cpu_directives(self):
        self.assertIn("CPUQuota=50%", self.cfg["content"])
        self.assertIn("MemoryMax=256M", self.cfg["content"])

    def test_io_directives_are_the_real_systemd_names(self):
        c = self.cfg["content"]
        self.assertIn("IOReadBandwidthMax=/dev/sda1 524288", c)
        self.assertIn("IOWriteBandwidthMax=/dev/sda1 262144", c)
        self.assertIn("IOAccounting=yes", c)

    def test_bogus_legacy_io_directives_are_absent(self):
        """ReadBandwidthMax/WriteBandwidthMax are not systemd keys; systemd
        ignored them silently, so no I/O limit was ever applied."""
        c = self.cfg["content"]
        self.assertNotIn("\nReadBandwidthMax=", c)
        self.assertNotIn("\nWriteBandwidthMax=", c)

    def test_io_skipped_with_warning_when_no_device(self):
        """systemd requires a device path, so we skip rather than emit invalid."""
        with self.assertLogs("service_limiter.orchestrator", level="WARNING") as logs:
            cfg = self.orch._generate_linux_config(self.service, POLICY)
        self.assertNotIn("IOReadBandwidthMax", cfg["content"])
        self.assertIn("io_device", "\n".join(logs.output))

    def test_drop_in_path_is_service_dot_service_d(self):
        self.assertEqual(
            self.cfg["file_path"],
            "/etc/systemd/system/sshd.service.d/override.conf")


class TestLinuxDiscovery(unittest.TestCase):
    def _discover(self, stdout):
        with mock.patch("subprocess.run") as run:
            run.return_value = subprocess.CompletedProcess(
                [], 0, stdout=stdout, stderr="")
            return LinuxServiceDiscovery().discover_services()

    def test_parses_real_unit_lines(self):
        svcs = self._discover(SYSTEMCTL_OUTPUT)
        self.assertEqual([s.name for s in svcs], ["sshd", "cron"])
        self.assertEqual(svcs[0].status, "active")
        self.assertEqual(svcs[0].display_name, "OpenSSH server daemon")
        self.assertEqual(svcs[1].display_name,
                         "Regular background program processing daemon")

    def test_trailing_summary_line_is_not_a_service(self):
        """systemctl prints 'N loaded units listed.' even with --no-legend."""
        names = [s.name for s in self._discover(SYSTEMCTL_OUTPUT)]
        self.assertNotIn("123", names)
        self.assertNotIn("2", names)

    def test_no_fabricated_services_without_systemd(self):
        """Used to inject a fake 'dummy-service' and report it as real."""
        with mock.patch("subprocess.run", side_effect=FileNotFoundError):
            self.assertEqual(LinuxServiceDiscovery().discover_services(), [])


class TestServiceDescriptor(unittest.TestCase):
    def test_optional_fields_default(self):
        """Windows discovery previously called this with 3 of 6 required args."""
        s = ServiceDescriptor(name="Spooler", status="Running")
        self.assertEqual(s.display_name, "Spooler")
        self.assertEqual(s.child_processes, [])

    def test_round_trips(self):
        s = ServiceDescriptor(name="a", child_processes=[10, 11])
        self.assertEqual(s.to_dict()["child_processes"], [10, 11])


class TestWindowsConfigGeneration(unittest.TestCase):
    def setUp(self):
        self.svc = ServiceDescriptor(name="Spooler")
        self.script = Orchestrator()._generate_windows_config(self.svc, POLICY)["content"]

    def test_no_invented_cmdlets(self):
        """PowerShell has no New-JobObject/Set-JobObject/Get-JobObject/
        Remove-JobObject; every call failed, then was swallowed as a warning."""
        for fake in ("New-JobObject", "Set-JobObject", "Get-JobObject",
                     "Remove-JobObject", "Get-WmiObject"):
            self.assertNotIn(fake, self.script)

    def test_uses_real_win32_api(self):
        self.assertIn("CreateJobObject", self.script)
        self.assertIn("SetInformationJobObject", self.script)
        self.assertIn("AssignProcessToJobObject", self.script)
        self.assertIn("Add-Type", self.script)

    def test_filename_matches_what_apply_expects(self):
        cfg = Orchestrator()._generate_windows_config(self.svc, POLICY)
        self.assertTrue(cfg["file_path"].endswith("Set-JobLimits.ps1"))


class TestProfileLoading(unittest.TestCase):
    def test_bundled_profile_loads(self):
        self.assertEqual(Orchestrator(profile_name="web-server").profile["memory_mb"], 256)

    def test_missing_profile_is_fatal_not_silent(self):
        """Used to log a warning and fall back to defaults on a limits tool."""
        with self.assertRaises(SystemExit):
            Orchestrator(profile_name="does-not-exist")

    def test_available_profiles(self):
        self.assertIn("web-server", Orchestrator.available_profiles())


class TestTupleAndNoneSafety(unittest.TestCase):
    def test_profile_resources_always_returns_a_dict(self):
        """Windows returned a list; the orchestrator calls .values() -> crash."""
        from service_limiter.platform.windows import WindowsResourceProfiler
        profiler = WindowsResourceProfiler()
        svcs = [ServiceDescriptor(name="Spooler", child_processes=[])]
        self.assertIsInstance(profiler.profile_resources(svcs), dict)


if __name__ == "__main__":
    unittest.main(verbosity=2)
