//! Linux service discovery and process-tree measurement.

use std::process::Command;
use std::time::Duration;

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
            let display = if parts.len() > 4 {
                parts[4..].join(" ")
            } else {
                unit.to_string()
            };
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
        .args([
            "list-units",
            "--type=service",
            "--state=running",
            "--no-legend",
            "--no-pager",
        ])
        .output()
        .map_err(|e| format!("systemctl not available: {e}"))?;
    Ok(parse_list_units(&String::from_utf8_lossy(&out.stdout)))
}

/// MainPIDs for many units, in ONE `systemctl show` invocation.
///
/// `systemctl show` accepts several units and prints a `Key=Value` block per
/// unit, so N units cost one process instead of N.
///
/// Parses `Key=Value` lines rather than `--value` output: with `--value` the
/// property order is not guaranteed, so matching values to units would be
/// guesswork. `Id=` gives the unit name back, which is how each MainPID is
/// matched to its unit.
pub fn main_pids(units: &[String]) -> std::collections::HashMap<String, u32> {
    let mut out = std::collections::HashMap::new();
    if units.is_empty() {
        return out;
    }

    let mut cmd = Command::new("systemctl");
    cmd.args(["show", "--property=Id,MainPID"]);
    for unit in units {
        cmd.arg(unit);
    }
    let Ok(output) = cmd.output() else {
        return out;
    };

    // Blocks look like:  Id=sshd.service \n MainPID=1234
    let text = String::from_utf8_lossy(&output.stdout);
    let mut current: Option<String> = None;
    for line in text.lines() {
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        match key.trim() {
            "Id" => {
                current = Some(value.trim().trim_end_matches(".service").to_string());
            }
            "MainPID" => {
                let Ok(pid) = value.trim().parse::<u32>() else {
                    continue;
                };
                // A unit with no live process reports MainPID=0. Skip it, the
                // same way main_pid does.
                if pid != 0 {
                    if let Some(name) = current.take() {
                        out.insert(name, pid);
                    }
                }
            }
            _ => {}
        }
    }
    out
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
        assert_eq!(
            svcs[1].display_name,
            "Regular background program processing daemon"
        );
    }

    #[test]
    fn trailing_summary_line_is_not_a_service() {
        let names: Vec<String> = parse_list_units(SYSTEMCTL_OUTPUT)
            .into_iter()
            .map(|s| s.name)
            .collect();
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
        let me = std::process::id();
        let samples = crate::platform::sample_trees(&[me], Duration::from_millis(50));
        let sample = samples
            .get(&me)
            .copied()
            .expect("current process must be samplable");
        assert!(sample.rss_bytes > 0, "{sample:?}");
        assert!(sample.elapsed_secs > 0.0, "{sample:?}");
    }

    #[test]
    fn missing_pid_returns_none() {
        let samples = crate::platform::sample_trees(&[u32::MAX], Duration::from_millis(10));
        assert!(samples.get(&u32::MAX).is_none());
    }
}
