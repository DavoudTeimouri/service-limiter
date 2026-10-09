//! Platform dispatch via cfg, not traits: one implementation is compiled per
//! target, so a trait would add dispatch with no polymorphism.

#[cfg(target_os = "linux")]
pub mod linux;
#[cfg(target_os = "windows")]
pub mod windows;

#[cfg(target_os = "linux")]
pub use linux::{discover_services, main_pid, sample_tree, DEFAULT_INTERVAL};
#[cfg(target_os = "windows")]
pub use windows::{discover_services, main_pid, sample_tree, DEFAULT_INTERVAL};

/// A single measurement of a process tree.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Sample {
    /// Summed across the tree; exceeds 100 on multiple cores.
    pub cpu_percent: f32,
    pub rss_bytes: u64,
    pub read_bytes: u64,
    pub write_bytes: u64,
}
