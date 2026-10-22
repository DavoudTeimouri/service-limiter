//! service-limiter: discover OS services, measure real usage, apply OS-native limits.
//!
//! See LOGIC.md for how this works and the platform traps it respects.

mod apply;
mod audit;
mod config_gen;
mod models;
mod orchestrator;
mod platform;
mod policy;
mod profile;

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Parser, Subcommand};
use models::Policy;

/// Exit codes are part of the interface; scripts depend on them.
mod exit {
    /// Ran clean, nothing over policy.
    pub const OK: u8 = 0;
    /// At least one service exceeded the policy.
    pub const VIOLATION: u8 = 1;
    /// Runtime/environment failure, including "nothing was measurable".
    pub const ERROR: u8 = 2;
    /// `apply` needs root (Linux only; Windows has no geteuid).
    #[allow(dead_code)]
    pub const NEEDS_ROOT: u8 = 3;
}

#[derive(Parser)]
#[command(name = "service-limiter", version, about, long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Discover services and measure their resource usage.
    Analyze {
        #[arg(long)]
        profile: Option<String>,
        /// Emit JSON instead of the human-readable report.
        #[arg(long)]
        json: bool,
    },
    /// Write configs for every service over the policy.
    Generate {
        #[arg(long)]
        profile: String,
        #[arg(long, default_value = "./config")]
        output: PathBuf,
        /// Block device for the I/O bandwidth limits, e.g. /dev/sda1.
        /// Overrides the profile's `io_device`.
        #[arg(long)]
        io_device: Option<PathBuf>,
    },
    /// Install staged configs.
    Apply {
        #[arg(long)]
        config: PathBuf,
        /// Print what would happen and change nothing.
        #[arg(long)]
        dry_run: bool,
        /// Skip the per-service confirmation.
        #[arg(long)]
        yes: bool,
        /// Windows only: hold the Job Object handle open so limits persist.
        #[arg(long)]
        durable: bool,
    },
    /// Remove installed limits, restoring any backed-up config.
    Revert {
        #[arg(long)]
        config: PathBuf,
        /// Print what would happen and change nothing.
        #[arg(long)]
        dry_run: bool,
        /// Skip the per-service confirmation.
        #[arg(long)]
        yes: bool,
    },
    /// Show which services currently have limits installed.
    Status {
        #[arg(long)]
        config: PathBuf,
    },
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    let result = match cli.command {
        Command::Analyze { profile, json } => analyze(profile.as_deref(), json),
        Command::Generate {
            profile,
            output,
            io_device,
        } => generate(&profile, &output, io_device.as_deref()),
        Command::Apply {
            config,
            dry_run,
            yes,
            durable,
        } => apply(&config, dry_run, yes, durable),
        Command::Revert {
            config,
            dry_run,
            yes,
        } => revert(&config, dry_run, yes),
        Command::Status { config } => status(&config),
    };
    ExitCode::from(result)
}

fn resolve_policy(name: Option<&str>) -> Result<Policy, u8> {
    match name {
        Some(n) => profile::load(n).map_err(|e| {
            eprintln!("error: {e}");
            exit::ERROR
        }),
        None => Ok(Policy::default()),
    }
}

fn analyze(profile_name: Option<&str>, json: bool) -> u8 {
    let policy = match resolve_policy(profile_name) {
        Ok(p) => p,
        Err(code) => return code,
    };
    let result = match orchestrator::run(&policy) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("error: {e}");
            audit::record(
                "analyze_error",
                &[("error", e.clone()), ("profile", policy.name.clone())],
            );
            return exit::ERROR;
        }
    };

    // Zero measurable services means we could not actually do the job, which
    // must not look like a clean run. Checked before any output, so a JSON
    // consumer never gets a report that claims success.
    if result.profiles.is_empty() {
        eprintln!("error: no service could be profiled (no live MainPID, or needs root)");
        audit::record(
            "analyze",
            &[
                ("discovered", result.services.len().to_string()),
                ("profiled", "0".to_string()),
            ],
        );
        return exit::ERROR;
    }

    if json {
        print_json(&result, &policy);
    } else {
        print_report(&result, &policy);
    }

    audit::record(
        "analyze",
        &[
            ("discovered", result.services.len().to_string()),
            ("profiled", result.profiles.len().to_string()),
            ("over_policy", result.over_policy.len().to_string()),
        ],
    );
    if result.over_policy.is_empty() {
        exit::OK
    } else {
        exit::VIOLATION
    }
}

/// The human-readable report.
fn print_report(result: &orchestrator::Analysis, policy: &models::Policy) {
    println!(
        "Discovered {} services, profiled {}, {} over policy",
        result.services.len(),
        result.profiles.len(),
        result.over_policy.len()
    );

    // Show the services that are actually over policy, with the numbers that
    // put them there. The old output listed the first five services with no
    // measurements, so the user could not tell which ones were the problem.
    if !result.over_policy.is_empty() {
        println!("\nOver policy:");
        for name in &result.over_policy {
            let Some(p) = result.profiles.get(name) else {
                continue;
            };
            let (_, violations) = policy::evaluate(p, policy);
            println!("  {name}:");
            for v in violations {
                println!("    {v}");
            }
        }
    }

    println!("\nMeasured:");
    for p in result.profiles.values() {
        let mark = if result.over_policy.contains(&p.service_name) {
            "!"
        } else {
            " "
        };
        println!(
            " {mark} {:<24} cpu {:>6.1}%  mem {:>8.1}MB  read {:>8.1}KB/s  write {:>8.1}KB/s",
            p.service_name, p.cpu_percent, p.memory_mb, p.io_read_kbps, p.io_write_kbps
        );
    }
}

/// The same data as JSON, for scripting and monitoring.
///
/// A monitoring tool that only prints prose cannot be consumed by anything
/// else, so the measurements are also available machine-readably.
fn print_json(result: &orchestrator::Analysis, policy: &models::Policy) {
    let doc = json_doc(result, policy);
    println!("{}", serde_json::to_string_pretty(&doc).unwrap_or_default());
}

/// The JSON document. Separate from the printing so it can be tested without
/// a live systemd.
fn json_doc(result: &orchestrator::Analysis, policy: &models::Policy) -> serde_json::Value {
    let violations: BTreeMap<String, Vec<String>> = result
        .over_policy
        .iter()
        .filter_map(|name| {
            let p = result.profiles.get(name)?;
            let (_, v) = policy::evaluate(p, policy);
            Some((name.clone(), v))
        })
        .collect();

    serde_json::json!({
        "discovered": result.services.len(),
        "profiled": result.profiles.len(),
        "over_policy": result.over_policy.len(),
        "policy": {
            "name": policy.name,
            "cpu_percent": policy.cpu_percent,
            "memory_mb": policy.memory_mb,
            "io_read_kbps": policy.io_read_kbps,
            "io_write_kbps": policy.io_write_kbps,
            "io_device": policy.io_device,
        },
        "violations": violations,
        "services": result.services,
        "profiles": result.profiles,
    })
}

fn generate(
    profile_name: &str,
    output: &std::path::Path,
    io_device: Option<&std::path::Path>,
) -> u8 {
    let mut policy = match resolve_policy(Some(profile_name)) {
        Ok(p) => p,
        Err(code) => return code,
    };

    // The CLI override wins over the profile. Validate it: systemd ignores a
    // limit on a device that does not exist, so a typo would produce a
    // drop-in that installs cleanly and enforces nothing.
    if let Some(device) = io_device {
        if !device.exists() {
            eprintln!(
                "error: I/O device {} does not exist. Check the path, or drop \
                 --io-device to skip the I/O limits.",
                device.display()
            );
            return exit::ERROR;
        }
        policy.io_device = Some(device.display().to_string());
    }

    let result = match orchestrator::run(&policy) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("error: {e}");
            audit::record(
                "generate_error",
                &[("error", e.clone()), ("profile", policy.name.clone())],
            );
            return exit::ERROR;
        }
    };

    let filename = match std::env::consts::OS {
        "linux" => "override.conf",
        "windows" => "Set-JobLimits.ps1",
        other => {
            eprintln!("error: Unsupported platform: {other}");
            return exit::ERROR;
        }
    };

    if result.configs.is_empty() {
        // Stale configs still have to go: every service came back under the
        // policy, so nothing should be left staged for `apply` to install.
        for (name, path) in stale_entries(output, &result.configs, filename) {
            eprintln!("Removing stale config for {name} ({})", path.display());
            let _ = std::fs::remove_file(&path);
            if let Some(dir) = path.parent() {
                let _ = std::fs::remove_dir(dir);
            }
        }
        println!("No configs generated: no service exceeded the policy.");
        audit::record("generate", &[("count", "0".to_string())]);
        return exit::OK;
    }

    for (name, cfg) in &result.configs {
        for warning in &cfg.warnings {
            eprintln!("warning: {name}: {warning}");
        }
        let dir = output.join(name);
        if let Err(e) = std::fs::create_dir_all(&dir) {
            eprintln!("error: cannot create {}: {e}", dir.display());
            return exit::ERROR;
        }
        let path = dir.join(filename);
        if let Err(e) = std::fs::write(&path, &cfg.content) {
            eprintln!("error: cannot write {}: {e}", path.display());
            return exit::ERROR;
        }
        println!("Wrote {}", path.display());
    }

    // Remove staged configs for services that are no longer over policy.
    // Leaving them behind meant `apply` kept installing limits for a service
    // that had since come back under the policy.
    let stale = stale_entries(output, &result.configs, filename);
    for (name, path) in stale {
        eprintln!("Removing stale config for {name} ({})", path.display());
        if let Err(e) = std::fs::remove_file(&path) {
            eprintln!("error: cannot remove {}: {e}", path.display());
            return exit::ERROR;
        }
        // Drop the now-empty service directory too.
        if let Some(dir) = path.parent() {
            let _ = std::fs::remove_dir(dir);
        }
    }
    println!(
        "Generated {} configs in {}",
        result.configs.len(),
        output.display()
    );
    audit::record(
        "generate",
        &[
            ("output", output.display().to_string()),
            ("count", result.configs.len().to_string()),
        ],
    );
    exit::OK
}

fn apply(config: &std::path::Path, dry_run: bool, yes: bool, durable: bool) -> u8 {
    // On Linux systemd owns the limit, so it survives this process exiting and
    // a reboot. --durable is a Windows concern and is a no-op here.
    #[cfg(target_os = "linux")]
    if durable {
        eprintln!("note: --durable is Windows-only; systemd limits already persist.");
    }
    if !config.is_dir() {
        eprintln!(
            "error: config directory {} does not exist. Run 'generate' first.",
            config.display()
        );
        return exit::ERROR;
    }

    // A dry run changes nothing, so it must not need root. Requiring sudo to
    // preview a change is the wrong contract, and it made `apply --dry-run`
    // unusable from a normal shell.
    #[cfg(unix)]
    if !dry_run && unsafe { libc::geteuid() } != 0 {
        eprintln!("error: apply needs root: re-run with sudo.");
        return exit::NEEDS_ROOT;
    }

    match apply::run(config, dry_run, yes, durable) {
        Ok(outcome) => {
            println!(
                "applied {}, skipped {}, failed {}",
                outcome.applied, outcome.skipped, outcome.failed
            );
            audit::record(
                "apply",
                &[
                    ("config", config.display().to_string()),
                    ("dry_run", dry_run.to_string()),
                    ("applied", outcome.applied.to_string()),
                    ("skipped", outcome.skipped.to_string()),
                    ("failed", outcome.failed.to_string()),
                ],
            );
            if outcome.is_ok() {
                exit::OK
            } else {
                exit::ERROR
            }
        }
        Err(e) => {
            eprintln!("error: {e}");
            audit::record(
                "apply_error",
                &[
                    ("config", config.display().to_string()),
                    ("error", e.to_string()),
                ],
            );
            exit::ERROR
        }
    }
}

/// Staged config files for services that are no longer over the policy.
///
/// Only files this tool creates are considered, so an unrelated file the user
/// put in the output directory is never touched.
fn stale_entries(
    output: &std::path::Path,
    current: &std::collections::BTreeMap<String, config_gen::GeneratedConfig>,
    filename: &str,
) -> Vec<(String, std::path::PathBuf)> {
    let mut stale = Vec::new();
    let Ok(entries) = std::fs::read_dir(output) else {
        return stale;
    };
    for entry in entries.filter_map(Result::ok) {
        let dir = entry.path();
        if !dir.is_dir() {
            continue;
        }
        let Some(name) = dir.file_name().map(|n| n.to_string_lossy().into_owned()) else {
            continue;
        };
        if current.contains_key(&name) {
            continue;
        }
        let path = dir.join(filename);
        if path.is_file() {
            stale.push((name, path));
        }
    }
    stale
}

/// Undo an `apply`: restore backed-up configs, or remove the drop-ins.
fn revert(config: &std::path::Path, dry_run: bool, yes: bool) -> u8 {
    if !config.is_dir() {
        eprintln!(
            "error: config directory {} does not exist. Run 'generate' first.",
            config.display()
        );
        return exit::ERROR;
    }

    // A dry run changes nothing, so it must not need root.
    #[cfg(unix)]
    if !dry_run && unsafe { libc::geteuid() } != 0 {
        eprintln!("error: revert needs root: re-run with sudo.");
        return exit::NEEDS_ROOT;
    }

    match apply::revert(config, dry_run, yes) {
        Ok(outcome) => {
            println!(
                "reverted {}, skipped {}, failed {}",
                outcome.applied, outcome.skipped, outcome.failed
            );
            audit::record(
                "revert",
                &[
                    ("config", config.display().to_string()),
                    ("dry_run", dry_run.to_string()),
                    ("reverted", outcome.applied.to_string()),
                    ("skipped", outcome.skipped.to_string()),
                    ("failed", outcome.failed.to_string()),
                ],
            );
            if outcome.is_ok() {
                exit::OK
            } else {
                exit::ERROR
            }
        }
        Err(e) => {
            eprintln!("error: {e}");
            audit::record(
                "revert_error",
                &[
                    ("config", config.display().to_string()),
                    ("error", e.to_string()),
                ],
            );
            exit::ERROR
        }
    }
}

/// Report which staged services currently have limits installed.
fn status(config: &std::path::Path) -> u8 {
    if !config.is_dir() {
        eprintln!(
            "error: config directory {} does not exist. Run 'generate' first.",
            config.display()
        );
        return exit::ERROR;
    }

    match apply::status(config) {
        Ok(outcome) => {
            audit::record(
                "status",
                &[
                    ("config", config.display().to_string()),
                    ("installed", outcome.applied.to_string()),
                ],
            );
            exit::OK
        }
        Err(e) => {
            eprintln!("error: {e}");
            exit::ERROR
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg() -> config_gen::GeneratedConfig {
        config_gen::GeneratedConfig {
            content: String::new(),
            file_path: String::new(),
            directory: String::new(),
            warnings: Vec::new(),
        }
    }

    /// A staged config for a service that is no longer over the policy must be
    /// reported as stale, and a file the tool did not create must be ignored.
    #[test]
    fn stale_entries_finds_only_our_own_files() {
        let dir = tempfile::tempdir().unwrap();

        // Stale: a service dir with our filename, not in the current set.
        let stale = dir.path().join("ghost");
        std::fs::create_dir_all(&stale).unwrap();
        std::fs::write(stale.join("override.conf"), "[Service]\n").unwrap();

        // Current: in the generated set, so not stale.
        let current = dir.path().join("sshd");
        std::fs::create_dir_all(&current).unwrap();
        std::fs::write(current.join("override.conf"), "[Service]\n").unwrap();

        // Not ours: a service dir with some other file inside.
        let foreign = dir.path().join("other");
        std::fs::create_dir_all(&foreign).unwrap();
        std::fs::write(foreign.join("notes.txt"), "mine").unwrap();

        let mut set = std::collections::BTreeMap::new();
        set.insert("sshd".to_string(), cfg());

        let found = stale_entries(dir.path(), &set, "override.conf");
        let names: Vec<String> = found.iter().map(|(n, _)| n.clone()).collect();
        assert_eq!(names, vec!["ghost".to_string()], "{names:?}");
    }

    /// A missing output directory is not an error; there is nothing stale.
    #[test]
    fn stale_entries_tolerates_a_missing_directory() {
        let set = std::collections::BTreeMap::new();
        let found = stale_entries(
            std::path::Path::new("/nonexistent/path/xyz"),
            &set,
            "override.conf",
        );
        assert!(found.is_empty());
    }

    /// The JSON report must carry the measurements, the policy, and the
    /// violations. A monitoring tool that only prints prose cannot be
    /// consumed by anything else.
    #[test]
    fn json_report_carries_the_measurements() {
        let policy = models::Policy {
            name: "web-server".into(),
            cpu_percent: 50.0,
            memory_mb: 256.0,
            io_read_kbps: 512.0,
            io_write_kbps: 256.0,
            io_device: None,
        };
        let mut profiles = std::collections::BTreeMap::new();
        profiles.insert(
            "sshd".to_string(),
            models::ResourceProfile {
                service_name: "sshd".into(),
                cpu_percent: 80.0,
                cpu_percent_percore: 160.0,
                memory_mb: 300.0,
                io_read_kbps: 10.0,
                io_write_kbps: 5.0,
            },
        );
        let result = orchestrator::Analysis {
            services: vec![models::ServiceDescriptor::new("sshd")],
            profiles,
            over_policy: vec!["sshd".to_string()],
            configs: std::collections::BTreeMap::new(),
        };

        let doc = json_doc(&result, &policy);
        assert_eq!(doc["discovered"], 1);
        assert_eq!(doc["profiled"], 1);
        assert_eq!(doc["over_policy"], 1);
        assert_eq!(doc["policy"]["name"], "web-server");
        assert_eq!(doc["policy"]["cpu_percent"], 50.0);
        assert_eq!(doc["profiles"]["sshd"]["cpu_percent"], 80.0);
        assert_eq!(doc["profiles"]["sshd"]["memory_mb"], 300.0);
        // The violation must name the service and the metric that broke.
        let v = &doc["violations"]["sshd"];
        assert!(v.is_array(), "{doc}");
        assert!(v[0].as_str().unwrap_or_default().contains("CPU"), "{doc}");
    }

    /// A service under the policy must not appear in the violations map.
    #[test]
    fn json_report_has_no_violations_for_healthy_services() {
        let policy = models::Policy {
            name: "p".into(),
            cpu_percent: 50.0,
            memory_mb: 256.0,
            io_read_kbps: 512.0,
            io_write_kbps: 256.0,
            io_device: None,
        };
        let mut profiles = std::collections::BTreeMap::new();
        profiles.insert(
            "quiet".to_string(),
            models::ResourceProfile {
                service_name: "quiet".into(),
                cpu_percent: 1.0,
                cpu_percent_percore: 4.0,
                memory_mb: 10.0,
                io_read_kbps: 1.0,
                io_write_kbps: 1.0,
            },
        );
        let result = orchestrator::Analysis {
            services: vec![models::ServiceDescriptor::new("quiet")],
            profiles,
            over_policy: Vec::new(),
            configs: std::collections::BTreeMap::new(),
        };

        let doc = json_doc(&result, &policy);
        assert_eq!(doc["over_policy"], 0);
        assert!(doc["violations"].as_object().unwrap().is_empty(), "{doc}");
    }
}
