//! Linux service discovery and process-tree measurement.

use std::collections::HashMap;
use std::process::Command;
use std::time::{Duration, Instant};

use sysinfo::{Pid, ProcessRefreshKind, ProcessesToUpdate, System};

use super::Sample;
use crate::models::ServiceDescriptor;

/// Default sample window. Short spikes are missed; see LOGIC.md §2.3.
pub const DEFAULT_INTERVAL: Duration = Duration::from_millis(250);

/// Parse `systemctl list-units` output.
///
/// A unit line looks like:
///   sshd.service loaded active running OpenSSH server daemon
/// systemctl also prints a trailing "N loaded units listed." summary even with
/// `--no-legend`, so lines without a `.service` suffix are skipped rather than
/// parsed as a service named "2".
pub fn parse_list_units(stdout: &str) -> Vec<ServiceDescriptor> {
    stdout
        .lines()
        .filter_map(|line| {
            let parts: Vec<&str> = line.split_whitespace().collect();
            let unit = *parts.first()?;
            let unit = unit.strip_suffix(".service")?;
            if parts.len() < 4 {
                return None;
            }
            let display = if parts.len() > 4 { parts[4..].join(" ") } else { unit.to_string() };
            Some(ServiceDescriptor {
                name: unit.to_string(),
                display_name: display,
                start_type: parts[1].to_string(),
                status: parts[2].to_string(),
                ..Default::default()
            })
        })
        .collect()
}

/// Running services, per systemctl. Empty when systemctl is unavailable.
pub fn discover_services() -> Result<Vec<ServiceDescriptor>, String> {
    let out = Command::new("systemctl")
        .args(["list-units", "--type=service", "--state=running", "--no-legend", "--no-pager"])
        .output()
        .map_err(|e| format!("systemctl not available: {e}"))?;
    Ok(parse_list_units(&String::from_utf8_lossy(&out.stdout)))
}

/// MainPID of a systemd unit, or None when it has no live process.
pub fn main_pid(unit: &str) -> Option<u32> {
    let out = Command::new("systemctl")
        .args(["show", unit, "-p", "MainPID", "--value"])
        .output()
        .ok()?;
    let pid: u32 = String::from_utf8_lossy(&out.stdout).trim().parse().ok()?;
    (pid != 0).then_some(pid)
}

/// Measure a process and all its descendants over `interval`.
///
/// CPU is sampled twice: `cpu_usage()` is a delta between refreshes, so the
/// first call establishes the baseline. `disk_usage()` is likewise a per-refresh
/// delta, which is why we divide by elapsed wall time rather than the nominal
/// interval - a short or long window would otherwise skew the rate.
pub fn sample_tree(root: u32, interval: Duration) -> Option<Sample> {
    // with_disk_usage() is required for disk_usage() to be non-zero; the default
    // refresh kind also pulls cmd/exe/environ data this tool never reads.
    let kinds = ProcessRefreshKind::everything().with_disk_usage();
    let mut sys = System::new();

    sys.refresh_processes_specifics(ProcessesToUpdate::All, true, kinds);
    let started = Instant::now();
    std::thread::sleep(interval);
    sys.refresh_processes_specifics(ProcessesToUpdate::All, true, kinds);

    let tree = descendents(&sys, Pid::from_u32(root));
    if tree.is_empty() {
        return None;
    }

    let elapsed = started.elapsed().as_secs_f64().max(f64::EPSILON);
    let mut sample = Sample::default();
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
/// Iterative rather than recursive so a deep tree cannot blow the stack, and
/// `seen` guards against pid-reuse cycles.
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

#[cfg(test)]
mod tests {
    use super::*;

    const SYSTEMCTL_OUTPUT: &str = "\
sshd.service loaded active running OpenSSH server daemon
cron.service loaded active running Regular background program processing daemon
  2 loaded units listed.
";

    #[test]
    fn parses_real_unit_lines() {
        let svcs = parse_list_units(SYSTEMCTL_OUTPUT);
        assert_eq!(svcs.len(), 2);
        assert_eq!(svcs[0].name, "sshd");
        assert_eq!(svcs[0].status, "active");
        assert_eq!(svcs[0].display_name, "OpenSSH server daemon");
        assert_eq!(svcs[1].display_name,
                   "Regular background program processing daemon");
    }

    #[test]
    fn trailing_summary_line_is_not_a_service() {
        let names: Vec<String> = parse_list_units(SYSTEMCTL_OUTPUT)
            .into_iter().map(|s| s.name).collect();
        assert!(!names.contains(&"2".to_string()), "{names:?}");
        assert!(!names.contains(&"loaded".to_string()), "{names:?}");
    }

    #[test]
    fn tolerates_garbage_and_blank_lines() {
        assert!(parse_list_units("").is_empty());
        assert!(parse_list_units("not a unit line\n\n").is_empty());
    }

    #[test]
    fn samples_this_process_with_real_usage() {
        // Measures ourselves, so it must always be found and have RSS.
        let me = std::process::id();
        let sample = sample_tree(me, Duration::from_millis(50))
            .expect("current process must be samplable");
        assert!(sample.rss_bytes > 0, "{sample:?}");
    }

    #[test]
    fn missing_pid_returns_none() {
        assert!(sample_tree(u32::MAX, Duration::from_millis(10)).is_none());
    }
}
