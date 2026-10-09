//! Threshold comparison. Mirrors service_limiter/policy_engine.py.

use crate::models::{Policy, ResourceProfile};

/// Compare a measurement against a policy.
///
/// Every metric is checked and every violation is reported, not just the first.
/// A value exactly at its limit passes: only *exceeding* it is a violation.
pub fn evaluate(profile: &ResourceProfile, policy: &Policy) -> (bool, Vec<String>) {
    let mut violations = Vec::new();

    if profile.cpu_percent > policy.cpu_percent {
        violations.push(format!(
            "CPU {}% > {}%",
            profile.cpu_percent, policy.cpu_percent
        ));
    }
    if profile.memory_mb > policy.memory_mb {
        violations.push(format!(
            "Memory {}MB > {}MB",
            profile.memory_mb, policy.memory_mb
        ));
    }
    if profile.io_read_kbps > policy.io_read_kbps {
        violations.push(format!(
            "Read IO {}KB/s > {}KB/s",
            profile.io_read_kbps, policy.io_read_kbps
        ));
    }
    if profile.io_write_kbps > policy.io_write_kbps {
        violations.push(format!(
            "Write IO {}KB/s > {}KB/s",
            profile.io_write_kbps, policy.io_write_kbps
        ));
    }

    (!violations.is_empty(), violations)
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

    #[test]
    fn under_limit_is_not_a_violation() {
        let p = ResourceProfile {
            cpu_percent: 10.0,
            memory_mb: 20.0,
            io_read_kbps: 5.0,
            io_write_kbps: 5.0,
            ..Default::default()
        };
        let (over, v) = evaluate(&p, &policy());
        assert!(!over);
        assert!(v.is_empty());
    }

    #[test]
    fn over_limit_reports_every_resource() {
        let p = ResourceProfile {
            cpu_percent: 99.0,
            memory_mb: 999.0,
            io_read_kbps: 5000.0,
            io_write_kbps: 5000.0,
            ..Default::default()
        };
        let (over, v) = evaluate(&p, &policy());
        assert!(over);
        assert_eq!(v.len(), 4);
    }

    #[test]
    fn limit_is_inclusive() {
        let p = ResourceProfile {
            cpu_percent: 50.0,
            memory_mb: 256.0,
            io_read_kbps: 512.0,
            io_write_kbps: 256.0,
            ..Default::default()
        };
        assert!(!evaluate(&p, &policy()).0);
    }
}
