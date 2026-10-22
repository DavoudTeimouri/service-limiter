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
///
/// All services are measured over one shared sampling window. Measuring them
/// one at a time cost a full process-table scan and a sleep per service, so
/// profiling 50 services took over 12 seconds of pure sleep.
fn profile_services(services: &[ServiceDescriptor]) -> BTreeMap<String, ResourceProfile> {
    let mut out = BTreeMap::new();

    // Resolve every PID first, in one batch. Asking per service spawned one
    // process (systemctl) or one PowerShell per service, which dominated
    // runtime on hosts with many services.
    let names: Vec<String> = services.iter().map(|s| s.name.clone()).collect();
    let pids = crate::platform::main_pids(&names);

    let mut roots: Vec<(String, u32)> = Vec::new();
    for svc in services {
        let Some(&pid) = pids.get(&svc.name) else {
            eprintln!("skipping {}: no live MainPID", svc.name);
            continue;
        };
        roots.push((svc.name.clone(), pid));
    }

    let pids: Vec<u32> = roots.iter().map(|(_, pid)| *pid).collect();
    let samples = crate::platform::sample_trees(&pids, crate::platform::DEFAULT_INTERVAL);

    // The sampler reports per-core usage; the policy field is a share of the
    // whole machine, so normalize here rather than in the generator.
    let cores = crate::platform::cpu_count() as f32;
    for (name, pid) in roots {
        let Some(sample) = samples.get(&pid) else {
            eprintln!("skipping {name}: could not sample pid {pid}");
            continue;
        };
        let secs = sample.elapsed_secs;
        out.insert(
            name.clone(),
            ResourceProfile {
                service_name: name,
                cpu_percent: sample.cpu_percent / cores,
                cpu_percent_percore: sample.cpu_percent,
                memory_mb: sample.rss_bytes as f64 / 1024.0 / 1024.0,
                io_read_kbps: sample.read_bytes as f64 / 1024.0 / secs,
                io_write_kbps: sample.write_bytes as f64 / 1024.0 / secs,
            },
        );
    }
    out
}
