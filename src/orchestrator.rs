//! The analysis pipeline: discover -> measure -> evaluate -> generate.

use std::collections::BTreeMap;

use crate::config_gen::{generate_linux_config, generate_windows_config, GeneratedConfig};
use crate::models::{Policy, ResourceProfile, ServiceDescriptor};
use crate::policy;

/// Everything one run produced.
#[derive(Debug, Default)]
pub struct Analysis {
    pub services: Vec<ServiceDescriptor>,
    pub profiles: BTreeMap<String, ResourceProfile>,
    pub over_policy: Vec<String>,
    pub configs: BTreeMap<String, GeneratedConfig>,
}

pub fn run(policy: &Policy) -> Result<Analysis, String> {
    let services = crate::platform::discover_services()?;
    let profiles = profile_services(&services);
    let mut result = Analysis {
        services,
        profiles,
        ..Default::default()
    };

    for svc in &result.services {
        let Some(measured) = result.profiles.get(&svc.name) else {
            continue;
        };
        let (over, _) = policy::evaluate(measured, policy);
        if over {
            result.over_policy.push(svc.name.clone());
        }
    }

    for name in &result.over_policy {
        let svc = ServiceDescriptor::new(name.clone());
        let cfg = match std::env::consts::OS {
            "linux" => generate_linux_config(&svc, policy),
            "windows" => generate_windows_config(&svc, policy),
            other => {
                return Err(format!("Unsupported platform: {other}"));
            }
        };
        result.configs.insert(name.clone(), cfg);
    }

    Ok(result)
}

/// Measure each service that has a live main process.
///
/// A service without a readable PID is skipped rather than reported as zero
/// usage: zero would read as "well within policy", which is a different and
/// wrong claim.
fn profile_services(services: &[ServiceDescriptor]) -> BTreeMap<String, ResourceProfile> {
    let mut out = BTreeMap::new();
    for svc in services {
        let Some(pid) = crate::platform::main_pid(&svc.name) else {
            eprintln!("skipping {}: no live MainPID", svc.name);
            continue;
        };
        let Some(sample) = crate::platform::sample_tree(pid, crate::platform::DEFAULT_INTERVAL)
        else {
            eprintln!("skipping {}: could not sample pid {pid}", svc.name);
            continue;
        };
        let secs = sample.elapsed_secs;
        out.insert(
            svc.name.clone(),
            ResourceProfile {
                service_name: svc.name.clone(),
                cpu_percent: sample.cpu_percent,
                memory_mb: sample.rss_bytes as f64 / 1024.0 / 1024.0,
                io_read_kbps: sample.read_bytes as f64 / 1024.0 / secs,
                io_write_kbps: sample.write_bytes as f64 / 1024.0 / secs,
            },
        );
    }
    out
}
