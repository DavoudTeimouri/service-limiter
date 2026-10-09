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
    let mem = (policy.memory_mb as u64).max(1);

    // `cpu_percent` is a share of the whole machine, but systemd's CPUQuota is
    // measured against ONE CPU, so 50% of a 4-core box is 200%. Converting here
    // is what makes the policy mean what the operator wrote. Before this, the
    // tool measured machine-share and enforced per-core, so the limit was far
    // tighter than the policy it came from. See LOGIC.md 3.1.
    let cores = crate::platform::cpu_count() as f64;
    let quota = (policy.cpu_percent as f64 * cores).round().max(1.0) as u64;

    let mut content = format!("[Service]\nCPUQuota={quota}%\nMemoryMax={mem}M\n");

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
    // CpuRate is in units of 1/10000 of ONE CPU, and `cpu_percent` is a share of
    // the whole machine, so scale by core count exactly as the systemd path does.
    // 50% of a 4-core box is 200% of one core = 20000/10000.
    let cores = crate::platform::cpu_count() as f64;
    let quota = (policy.cpu_percent as f64 * cores).min(100.0);
    let cpu_rate = ((quota * 100.0).round() as u64).min(10000);

    let content = include_str!("job_limits.ps1")
        .replace("{service}", &service.name)
        .replace("{mem_bytes}", &mem_bytes.to_string())
        .replace("{cpu_rate}", &cpu_rate.to_string())
        .replace("{read_bytes}", &read_bytes.to_string());

    GeneratedConfig {
        content,
        file_path: format!("C:\\ServiceLimiter\\{}\\Set-JobLimits.ps1", service.name),
        directory: "C:\\ServiceLimiter".to_string(),
        warnings: Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn policy() -> Policy {
        Policy {
            name: "t".into(),
            cpu_percent: 50.0,
            memory_mb: 256.0,
            io_read_kbps: 512.0,
            io_write_kbps: 256.0,
            io_device: None,
        }
    }

    /// CPUQuota is per-CPU, so the expected value depends on the host's core
    /// count. Asserting the relationship rather than a literal keeps this test
    /// correct on any machine, including CI runners.
    #[test]
    fn cpu_quota_is_machine_share_converted_to_per_cpu() {
        let cores = crate::platform::cpu_count() as f64;
        let cfg = generate_linux_config(&ServiceDescriptor::new("sshd"), &policy());
        let expected = (50.0 * cores).round() as u64;
        assert!(
            cfg.content.contains(&format!("CPUQuota={expected}%")),
            "expected CPUQuota={expected}% on {cores} cores, got:\n{}",
            cfg.content
        );
        assert!(cfg.content.contains("MemoryMax=256M"), "{}", cfg.content);
    }

    /// A 100% policy must allow the whole machine, i.e. N cores' worth.
    #[test]
    fn full_machine_policy_allows_all_cores() {
        let cores = crate::platform::cpu_count() as f64;
        let full = Policy {
            cpu_percent: 100.0,
            ..policy()
        };
        let cfg = generate_linux_config(&ServiceDescriptor::new("sshd"), &full);
        let expected = (100.0 * cores).round() as u64;
        assert!(
            cfg.content.contains(&format!("CPUQuota={expected}%")),
            "100% policy should be {expected}% on {cores} cores, got:\n{}",
            cfg.content
        );
    }

    /// A sub-1 limit must not truncate to CPUQuota=0%, which systemd reads as
    /// "never run".
    #[test]
    fn sub_one_cpu_limit_never_becomes_zero() {
        let tiny = Policy {
            cpu_percent: 0.1,
            ..policy()
        };
        let cfg = generate_linux_config(&ServiceDescriptor::new("sshd"), &tiny);
        assert!(
            cfg.content.contains("CPUQuota=1%"),
            "must clamp to at least 1%, got:\n{}",
            cfg.content
        );
        assert!(!cfg.content.contains("CPUQuota=0%"));
    }

    #[test]
    fn io_directives_use_real_systemd_names() {
        let p = Policy {
            io_device: Some("/dev/sda1".into()),
            ..policy()
        };
        let c = generate_linux_config(&ServiceDescriptor::new("sshd"), &p).content;
        assert!(c.contains("IOReadBandwidthMax=/dev/sda1 524288"), "{c}");
        assert!(c.contains("IOWriteBandwidthMax=/dev/sda1 262144"), "{c}");
        assert!(c.contains("IOAccounting=yes"), "{c}");
    }

    #[test]
    fn bogus_legacy_io_directives_are_absent() {
        // `ReadBandwidthMax` is a substring of `IOReadBandwidthMax`, so the
        // newline anchor is what makes this assertion meaningful.
        let p = Policy {
            io_device: Some("/dev/sda1".into()),
            ..policy()
        };
        let c = generate_linux_config(&ServiceDescriptor::new("sshd"), &p).content;
        assert!(!c.contains("\nReadBandwidthMax="), "{c}");
        assert!(!c.contains("\nWriteBandwidthMax="), "{c}");
    }

    #[test]
    fn io_skipped_with_warning_when_no_device() {
        let cfg = generate_linux_config(&ServiceDescriptor::new("sshd"), &policy());
        assert!(
            !cfg.content.contains("IOReadBandwidthMax"),
            "{}",
            cfg.content
        );
        assert!(
            cfg.warnings.iter().any(|w| w.contains("io_device")),
            "{:?}",
            cfg.warnings
        );
    }

    #[test]
    fn drop_in_path_is_service_dot_service_d() {
        let cfg = generate_linux_config(&ServiceDescriptor::new("sshd"), &policy());
        assert_eq!(
            cfg.file_path,
            "/etc/systemd/system/sshd.service.d/override.conf"
        );
    }

    #[test]
    fn windows_script_has_no_invented_cmdlets() {
        let s = generate_windows_config(&ServiceDescriptor::new("Spooler"), &policy()).content;
        for fake in [
            "New-JobObject",
            "Set-JobObject",
            "Get-JobObject",
            "Remove-JobObject",
            "Get-WmiObject",
        ] {
            assert!(!s.contains(fake), "invented cmdlet {fake} present");
        }
    }

    #[test]
    fn windows_script_uses_real_win32_classes() {
        let s = generate_windows_config(&ServiceDescriptor::new("Spooler"), &policy()).content;
        for api in [
            "CreateJobObject",
            "SetInformationJobObject",
            "AssignProcessToJobObject",
            "Add-Type",
        ] {
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
        assert!(
            !s.contains("0x00000400"),
            "DIE_ON_UNHANDLED_EXCEPTION flag present"
        );
    }

    #[test]
    fn windows_io_limits_never_go_into_io_counters() {
        let s = generate_windows_config(&ServiceDescriptor::new("Spooler"), &policy()).content;
        assert!(!s.contains("IoInfo.ReadTransferCount ="), "{s}");
        assert!(!s.contains("IoInfo.WriteTransferCount ="), "{s}");
        assert!(s.contains("JOBOBJECT_IO_RATE_CONTROL_INFORMATION"), "{s}");
    }

    /// Windows CpuRate is also per-CPU, so it gets the same core scaling.
    #[test]
    fn windows_cpu_rate_is_applied_and_clamped() {
        let cores = crate::platform::cpu_count() as f64;
        let s = generate_windows_config(&ServiceDescriptor::new("Spooler"), &policy()).content;
        let expected = ((50.0 * cores).min(100.0) * 100.0).round() as u64;
        assert!(
            s.contains(&format!("$cpu.CpuRate = {expected}")),
            "50% of {cores} cores should be {expected}, got:\n{s}"
        );

        // Cannot exceed 10000 (one full core), whatever the policy says.
        let over = Policy {
            cpu_percent: 250.0,
            ..policy()
        };
        let s = generate_windows_config(&ServiceDescriptor::new("Spooler"), &over).content;
        assert!(s.contains("$cpu.CpuRate = 10000"), "cpu rate not clamped");
    }

    #[test]
    fn windows_filename_matches_what_apply_expects() {
        let cfg = generate_windows_config(&ServiceDescriptor::new("Spooler"), &policy());
        assert!(
            cfg.file_path.ends_with("Set-JobLimits.ps1"),
            "{}",
            cfg.file_path
        );
    }
}
