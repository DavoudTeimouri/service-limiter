//! Data types shared across the pipeline. Mirrors service_limiter/models/.

use serde::{Deserialize, Serialize};

/// A discovered OS service.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ServiceDescriptor {
    pub name: String,
    pub display_name: String,
    pub status: String,
    pub start_type: String,
    pub path: String,
    pub account: String,
    /// PIDs of the service main process and any children found at runtime.
    pub child_processes: Vec<u32>,
}

impl ServiceDescriptor {
    /// `display_name` falls back to `name`, which is why this cannot be a
    /// plain `#[derive(Default)]`.
    pub fn new(name: impl Into<String>) -> Self {
        let name = name.into();
        Self { display_name: name.clone(), name, ..Default::default() }
    }
}

/// One measurement of a service's resource usage over the sample window.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ResourceProfile {
    pub service_name: String,
    /// Summed across the process tree; exceeds 100 on multiple cores.
    pub cpu_percent: f32,
    pub memory_mb: f64,
    pub io_read_kbps: f64,
    pub io_write_kbps: f64,
}

/// Thresholds a profile is judged against.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Policy {
    pub name: String,
    pub cpu_percent: f32,
    pub memory_mb: f64,
    pub io_read_kbps: f64,
    pub io_write_kbps: f64,
    /// Block device for systemd I/O limits. Without it they are skipped.
    #[serde(default)]
    pub io_device: Option<String>,
}

impl Default for Policy {
    fn default() -> Self {
        Self {
            name: "default".into(),
            cpu_percent: 80.0,
            memory_mb: 512.0,
            io_read_kbps: 1024.0,
            io_write_kbps: 512.0,
            io_device: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_name_defaults_to_name() {
        assert_eq!(ServiceDescriptor::new("sshd").display_name, "sshd");
        assert!(ServiceDescriptor::new("sshd").child_processes.is_empty());
    }

    #[test]
    fn descriptor_round_trips() {
        let mut s = ServiceDescriptor::new("a");
        s.child_processes = vec![10, 11];
        let json = serde_json::to_string(&s).unwrap();
        assert_eq!(serde_json::from_str::<ServiceDescriptor>(&json).unwrap(), s);
    }
}
