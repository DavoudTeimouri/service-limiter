//! OS-native config text generation.
//!
//! Mirrors `Orchestrator._generate_linux_config` / `_generate_windows_config`.
//! See LOGIC.md for why the exact strings here matter: systemd silently ignores
//! unknown keys, so a wrong directive is a silent no-op.

use crate::models::{Policy, ServiceDescriptor};

/// Where a generated config goes, and its content.
#[derive(Debug, Clone)]
pub struct GeneratedConfig {
    pub content: String,
    /// Final on-system destination. Kept for callers that report it; apply
    /// recomputes paths per OS because it also needs the drop-in directory.
    #[allow(dead_code)]
    pub file_path: String,
    #[allow(dead_code)]
    pub directory: String,
    /// Human-readable notes for the operator (e.g. skipped I/O limits).
    pub warnings: Vec<String>,
}

/// systemd drop-in text.
///
/// `IOReadBandwidthMax` / `IOWriteBandwidthMax` are per-device and need a block
/// device path; without `policy.io_device` they are omitted rather than emitted
/// as something systemd ignores.
pub fn generate_linux_config(service: &ServiceDescriptor, policy: &Policy) -> GeneratedConfig {
    // Truncating a sub-1 limit to 0 would emit CPUQuota=0%, which systemd reads
    // as "never run", so clamp rather than round down to nothing.
    let cpu = (policy.cpu_percent as u32).max(1);
    let mem = (policy.memory_mb as u64).max(1);
    let mut content = format!("[Service]\nCPUQuota={cpu}%\nMemoryMax={mem}M\n");

    let mut warnings = Vec::new();
    match policy.io_device.as_deref().filter(|d| !d.is_empty()) {
        Some(device) => {
            content.push_str("IOAccounting=yes\n");
            content.push_str(&format!(
                "IOReadBandwidthMax={} {}\n",
                device,
                (policy.io_read_kbps as u64) * 1024
            ));
            content.push_str(&format!(
                "IOWriteBandwidthMax={} {}\n",
                device,
                (policy.io_write_kbps as u64) * 1024
            ));
        }
        None => warnings.push(
            "Policy has no 'io_device'; skipping I/O bandwidth limits \
             (systemd requires a block device path)."
                .to_string(),
        ),
    }

    GeneratedConfig {
        content,
        file_path: format!(
            "/etc/systemd/system/{}.service.d/override.conf",
            service.name
        ),
        directory: format!("/etc/systemd/system/{}.service.d", service.name),
        warnings,
    }
}

/// PowerShell script that applies Job Object limits via Add-Type P/Invoke.
///
/// Limits are set through three separate Win32 information classes. Writing
/// I/O limits into `IO_COUNTERS` would be a silent no-op: those are read-only
/// accounting counters.
pub fn generate_windows_config(service: &ServiceDescriptor, policy: &Policy) -> GeneratedConfig {
    let mem_bytes = (policy.memory_mb as u64) * 1024 * 1024;
    let read_bytes = (policy.io_read_kbps as u64) * 1024;
    // CpuRate is in units of 1/10000 of a CPU; 100% == 10000.
    let cpu_rate = ((policy.cpu_percent as u64) * 100).min(10000);

    let content = include_str!("job_limits.ps1")
        .replace("{service}", &service.name)
        .replace("{mem_bytes}", &mem_bytes.to_string())
        .replace("{cpu_rate}", &cpu_rate.to_string())
        .replace("{read_bytes}", &read_bytes.to_string());

    GeneratedConfig {
        content,
        file_path: format!(
            "C:\\ServiceLimiter\\{}\\Set-JobLimits.ps1",
            service.name
        ),
        directory: "C:\\ServiceLimiter".to_string(),
        warnings: Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn policy() -> Policy {
        Policy { name: "t".into(), cpu_percent: 50.0, memory_mb: 256.0,
                 io_read_kbps: 512.0, io_write_kbps: 256.0, io_device: None }
    }

    #[test]
    fn memory_and_cpu_directives() {
        let cfg = generate_linux_config(&ServiceDescriptor::new("sshd"), &policy());
        assert!(cfg.content.contains("CPUQuota=50%"), "{}", cfg.content);
        assert!(cfg.content.contains("MemoryMax=256M"), "{}", cfg.content);
    }

    #[test]
    fn io_directives_use_real_systemd_names() {
        let p = Policy { io_device: Some("/dev/sda1".into()), ..policy() };
        let c = generate_linux_config(&ServiceDescriptor::new("sshd"), &p).content;
        assert!(c.contains("IOReadBandwidthMax=/dev/sda1 524288"), "{c}");
        assert!(c.contains("IOWriteBandwidthMax=/dev/sda1 262144"), "{c}");
        assert!(c.contains("IOAccounting=yes"), "{c}");
    }

    #[test]
    fn bogus_legacy_io_directives_are_absent() {
        // `ReadBandwidthMax` is a substring of `IOReadBandwidthMax`, so the
        // newline anchor is what makes this assertion meaningful.
        let p = Policy { io_device: Some("/dev/sda1".into()), ..policy() };
        let c = generate_linux_config(&ServiceDescriptor::new("sshd"), &p).content;
        assert!(!c.contains("\nReadBandwidthMax="), "{c}");
        assert!(!c.contains("\nWriteBandwidthMax="), "{c}");
    }

    #[test]
    fn io_skipped_with_warning_when_no_device() {
        let cfg = generate_linux_config(&ServiceDescriptor::new("sshd"), &policy());
        assert!(!cfg.content.contains("IOReadBandwidthMax"), "{}", cfg.content);
        assert!(cfg.warnings.iter().any(|w| w.contains("io_device")), "{:?}", cfg.warnings);
    }

    #[test]
    fn drop_in_path_is_service_dot_service_d() {
        let cfg = generate_linux_config(&ServiceDescriptor::new("sshd"), &policy());
        assert_eq!(cfg.file_path, "/etc/systemd/system/sshd.service.d/override.conf");
    }

    #[test]
    fn windows_script_has_no_invented_cmdlets() {
        let s = generate_windows_config(&ServiceDescriptor::new("Spooler"), &policy()).content;
        for fake in ["New-JobObject", "Set-JobObject", "Get-JobObject",
                     "Remove-JobObject", "Get-WmiObject"] {
            assert!(!s.contains(fake), "invented cmdlet {fake} present");
        }
    }

    #[test]
    fn windows_script_uses_real_win32_classes() {
        let s = generate_windows_config(&ServiceDescriptor::new("Spooler"), &policy()).content;
        for api in ["CreateJobObject", "SetInformationJobObject",
                    "AssignProcessToJobObject", "Add-Type"] {
            assert!(s.contains(api), "missing {api}");
        }
    }

    #[test]
    fn windows_memory_flag_is_the_real_one() {
        // 0x20 is PRIORITY_CLASS, 0x400 is DIE_ON_UNHANDLED_EXCEPTION.
        // Neither limits memory; JOB_OBJECT_LIMIT_JOB_MEMORY is 0x00000200.
        let s = generate_windows_config(&ServiceDescriptor::new("Spooler"), &policy()).content;
        assert!(s.contains("0x00000200"), "JOB_MEMORY flag missing");
        assert!(!s.contains("0x00000020"), "PRIORITY_CLASS flag present");
        assert!(!s.contains("0x00000400"), "DIE_ON_UNHANDLED_EXCEPTION flag present");
    }

    #[test]
    fn windows_io_limits_never_go_into_io_counters() {
        let s = generate_windows_config(&ServiceDescriptor::new("Spooler"), &policy()).content;
        assert!(!s.contains("IoInfo.ReadTransferCount ="), "{s}");
        assert!(!s.contains("IoInfo.WriteTransferCount ="), "{s}");
        assert!(s.contains("JOBOBJECT_IO_RATE_CONTROL_INFORMATION"), "{s}");
    }

    #[test]
    fn windows_cpu_rate_is_applied_and_clamped() {
        let s = generate_windows_config(&ServiceDescriptor::new("Spooler"), &policy()).content;
        assert!(s.contains("$cpu.CpuRate = 5000"), "50% * 100 not interpolated");

        let over = Policy { cpu_percent: 250.0, ..policy() };
        let s = generate_windows_config(&ServiceDescriptor::new("Spooler"), &over).content;
        assert!(s.contains("$cpu.CpuRate = 10000"), "cpu rate not clamped");
    }

    #[test]
    fn windows_filename_matches_what_apply_expects() {
        let cfg = generate_windows_config(&ServiceDescriptor::new("Spooler"), &policy());
        assert!(cfg.file_path.ends_with("Set-JobLimits.ps1"), "{}", cfg.file_path);
    }
}
