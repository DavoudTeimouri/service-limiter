//! Platform dispatch via cfg, not traits: one implementation is compiled per
//! target, so a trait would add dispatch with no polymorphism.

use std::collections::HashMap;
use std::time::Instant;

use sysinfo::{Pid, ProcessRefreshKind, ProcessesToUpdate, System};

#[cfg(target_os = "linux")]
pub mod linux;
#[cfg(target_os = "windows")]
pub mod windows;

#[cfg(target_os = "linux")]
pub use linux::{discover_services, main_pids, DEFAULT_INTERVAL};
#[cfg(target_os = "windows")]
pub use windows::{discover_services, main_pids, DEFAULT_INTERVAL};

/// Logical CPUs a service can actually use here.
///
/// `available_parallelism` respects the cgroup CPU quota, which is the right
/// denominator: a container capped at 1 CPU reports 1 even if the host has 32.
/// Scaling `cpu_percent` by the *host* core count would emit a CPUQuota above
/// what the cgroup can ever grant, so the limit could never bind.
///
/// Note this can be lower than `nproc` (which reports scheduler affinity). On
/// this host: affinity 2, cgroup quota 1, `cpu_count()` 1.
pub fn cpu_count() -> usize {
    std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(1)
}

/// Measure several process trees over ONE shared sampling window.
///
/// Sampling each tree separately costs a full process-table scan and a sleep
/// per service: for N services that is 2N scans and N sleeps, so measuring
/// 50 services takes over 12 seconds of pure sleep. One shared window costs
/// 2 scans and 1 sleep regardless of N.
///
/// The trade-off: every tree is measured over the same window, so their
/// numbers are directly comparable. Per-tree sampling gave each service its
/// own window, which made the results incomparable in a different way.
///
/// NOTE on units: `cpu_percent` is **per-core**, not a fraction of the machine.
/// Measured on this host: one saturated thread reads ~161%. So 100% == one core.
/// This is the same convention psutil uses, and it is NOT the convention
/// systemd's `CPUQuota=` uses (where 100% == one core too, but the policy field
/// `cpu_percent` reads as a whole-machine share). See LOGIC.md 3.1.
pub fn sample_trees(
    roots: &[u32],
    interval: std::time::Duration,
) -> HashMap<u32, Sample> {
    let mut out = HashMap::new();
    if roots.is_empty() {
        return out;
    }

    // with_disk_usage() is required for disk_usage() to be non-zero; the default
    // refresh kind also pulls cmd/exe/environ data this tool never reads.
    let kinds = ProcessRefreshKind::everything().with_disk_usage();
    let mut sys = System::new();

    // Prime, then measure the delta across the window.
    sys.refresh_processes_specifics(ProcessesToUpdate::All, kinds);
    let started = Instant::now();
    std::thread::sleep(interval);
    sys.refresh_processes_specifics(ProcessesToUpdate::All, kinds);
    let elapsed = started.elapsed().as_secs_f64().max(f64::EPSILON);

    for root in roots {
        if let Some(sample) = sample_from(&sys, *root, elapsed) {
            out.insert(*root, sample);
        }
    }
    out
}

/// Sum one tree's usage out of an already-refreshed process table.
fn sample_from(sys: &System, root: u32, elapsed_secs: f64) -> Option<Sample> {
    let tree = descendents(sys, Pid::from_u32(root));
    if tree.is_empty() {
        return None;
    }

    let mut sample = Sample {
        elapsed_secs,
        ..Default::default()
    };
    for pid in tree {
        if let Some(p) = sys.process(pid) {
            sample.cpu_percent += p.cpu_usage();
            sample.rss_bytes += p.memory();
            let disk = p.disk_usage();
            sample.read_bytes += disk.read_bytes;
            sample.write_bytes += disk.written_bytes;
        }
    }
    Some(sample)
}

/// The process plus every descendant, via an iterative DFS.
///
/// Iterative so a deep tree cannot blow the stack; `seen` guards pid reuse.
fn descendents(sys: &System, root: Pid) -> Vec<Pid> {
    let mut children: HashMap<Pid, Vec<Pid>> = HashMap::new();
    for (pid, p) in sys.processes() {
        if let Some(ppid) = p.parent() {
            children.entry(ppid).or_default().push(*pid);
        }
    }

    let mut found = Vec::new();
    let mut seen: Vec<Pid> = Vec::new();
    let mut stack = vec![root];
    while let Some(pid) = stack.pop() {
        if seen.contains(&pid) {
            continue;
        }
        seen.push(pid);
        if sys.process(pid).is_some() {
            found.push(pid);
        }
        if let Some(kids) = children.get(&pid) {
            stack.extend(kids.iter().copied());
        }
    }
    found
}

/// A single measurement of a process tree.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Sample {
    /// Summed across the tree; exceeds 100 on multiple cores.
    pub cpu_percent: f32,
    pub rss_bytes: u64,
    pub read_bytes: u64,
    pub write_bytes: u64,
    /// Seconds the sample window actually took, which may differ from the
    /// requested interval. Callers divide by this, not the nominal value.
    pub elapsed_secs: f64,
}
