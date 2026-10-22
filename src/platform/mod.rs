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

/// Number of sampling windows.
///
/// One window can miss a service that spikes between samples. Several
/// windows catch it, at a linear cost in total runtime. The reported figure
/// is the worst window, not the average: a limiter that averages away a
/// sustained spike would under-limit exactly when it matters.
pub const DEFAULT_WINDOWS: usize = 3;

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
pub fn sample_trees(roots: &[u32], interval: std::time::Duration) -> HashMap<u32, Sample> {
    sample_trees_over(roots, interval, DEFAULT_WINDOWS)
}

/// Measure several process trees over `windows` shared sampling windows.
///
/// Every tree shares each window, so the numbers stay comparable, and the
/// whole set costs `windows` sleeps and `windows + 1` refreshes regardless
/// of how many services there are.
///
/// The reported value for each metric is the worst window, not the average.
pub fn sample_trees_over(
    roots: &[u32],
    interval: std::time::Duration,
    windows: usize,
) -> HashMap<u32, Sample> {
    let mut out: HashMap<u32, Sample> = HashMap::new();
    if roots.is_empty() || windows == 0 {
        return out;
    }

    // with_disk_usage() is required for disk_usage() to be non-zero; the default
    // refresh kind also pulls cmd/exe/environ data this tool never reads.
    let kinds = ProcessRefreshKind::everything().with_disk_usage();
    let mut sys = System::new();

    // Prime, then measure the delta across each window. The first refresh
    // establishes the baseline; a single refresh returns garbage deltas.
    sys.refresh_processes_specifics(ProcessesToUpdate::All, kinds);
    let mut last = Instant::now();

    for _ in 0..windows {
        std::thread::sleep(interval);
        sys.refresh_processes_specifics(ProcessesToUpdate::All, kinds);
        let elapsed = last.elapsed().as_secs_f64().max(f64::EPSILON);
        last = Instant::now();

        for root in roots {
            let Some(sample) = sample_from(&sys, *root, elapsed) else {
                continue;
            };
            match out.get_mut(root) {
                Some(best) => best.keep_worst(&sample),
                None => {
                    out.insert(*root, sample);
                }
            }
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

    let mut cpu = 0.0f32;
    let mut rss = 0u64;
    let mut read = 0u64;
    let mut write = 0u64;
    for pid in tree {
        if let Some(p) = sys.process(pid) {
            cpu += p.cpu_usage();
            rss += p.memory();
            let disk = p.disk_usage();
            read += disk.read_bytes;
            write += disk.written_bytes;
        }
    }

    // Store rates, not raw byte counts: the windows have different lengths,
    // so comparing raw counts across windows would compare different spans.
    Some(Sample {
        cpu_percent: cpu,
        rss_bytes: rss,
        read_bps: read as f64 / elapsed_secs,
        write_bps: write as f64 / elapsed_secs,
        elapsed_secs,
    })
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
    // A HashSet, not a Vec: `contains` on a Vec is a linear scan, so the walk
    // was quadratic in the size of the process table.
    let mut seen: std::collections::HashSet<Pid> = std::collections::HashSet::new();
    let mut stack = vec![root];
    while let Some(pid) = stack.pop() {
        if !seen.insert(pid) {
            continue;
        }
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
    ///
    /// NOTE on units: this is **per-core**, not a fraction of the machine.
    /// One saturated thread reads ~161%. It is NOT the convention systemd's
    /// `CPUQuota=` uses; see LOGIC.md 3.1.
    pub cpu_percent: f32,
    pub rss_bytes: u64,
    /// Bytes per second read across the tree.
    pub read_bps: f64,
    /// Bytes per second written across the tree.
    pub write_bps: f64,
    /// Seconds the sample window actually took, which may differ from the
    /// requested interval. Reported for transparency; the rates above are
    /// already divided by it.
    pub elapsed_secs: f64,
}

impl Sample {
    /// Keep the worse of two windows, metric by metric.
    ///
    /// The reported figure is the worst window, not the average. A limiter
    /// that averaged away a sustained spike would under-limit exactly when
    /// it matters, and a service that bursts would slip under the policy.
    pub fn keep_worst(&mut self, other: &Sample) {
        if other.cpu_percent > self.cpu_percent {
            self.cpu_percent = other.cpu_percent;
        }
        if other.rss_bytes > self.rss_bytes {
            self.rss_bytes = other.rss_bytes;
        }
        if other.read_bps > self.read_bps {
            self.read_bps = other.read_bps;
        }
        if other.write_bps > self.write_bps {
            self.write_bps = other.write_bps;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn sample(cpu: f32, rss: u64, read: f64, write: f64) -> Sample {
        Sample {
            cpu_percent: cpu,
            rss_bytes: rss,
            read_bps: read,
            write_bps: write,
            elapsed_secs: 0.25,
        }
    }

    /// The worst window wins, per metric. Averaging would let a burst slip
    /// under the policy.
    #[test]
    fn keep_worst_takes_the_max_of_each_metric() {
        let mut best = sample(10.0, 100, 5.0, 5.0);
        best.keep_worst(&sample(50.0, 50, 1.0, 9.0));

        assert_eq!(best.cpu_percent, 50.0);
        assert_eq!(best.rss_bytes, 100, "memory must not shrink");
        assert_eq!(best.read_bps, 5.0, "read must not shrink");
        assert_eq!(best.write_bps, 9.0);
    }

    /// A later, worse window replaces the earlier one metric by metric.
    #[test]
    fn keep_worst_handles_a_worse_later_window() {
        let mut best = sample(10.0, 100, 5.0, 5.0);
        best.keep_worst(&sample(50.0, 500, 50.0, 50.0));

        assert_eq!(best.cpu_percent, 50.0);
        assert_eq!(best.rss_bytes, 500);
        assert_eq!(best.read_bps, 50.0);
        assert_eq!(best.write_bps, 50.0);
    }

    /// Zero windows means no measurement, not a crash.
    #[test]
    fn zero_windows_yields_nothing() {
        let out = sample_trees_over(&[std::process::id()], Duration::from_millis(1), 0);
        assert!(out.is_empty());
    }

    /// Several windows must still find the process, and must cost more than
    /// one window (the sleeps are the point).
    #[test]
    fn more_windows_still_find_the_process() {
        let me = std::process::id();
        let out = sample_trees_over(&[me], Duration::from_millis(20), 3);
        let s = out.get(&me).expect("current process must be samplable");
        assert!(s.rss_bytes > 0, "{s:?}");
        assert!(s.elapsed_secs > 0.0, "{s:?}");
    }
}
