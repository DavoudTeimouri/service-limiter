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
    },
    /// Write configs for every service over the policy.
    Generate {
        #[arg(long)]
        profile: String,
        #[arg(long, default_value = "./config")]
        output: PathBuf,
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
        Command::Analyze { profile } => analyze(profile.as_deref()),
        Command::Generate { profile, output } => generate(&profile, &output),
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

fn analyze(profile_name: Option<&str>) -> u8 {
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
            let (_, violations) = policy::evaluate(p, &policy);
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

    // Zero measurable services means we could not actually do the job, which
    // must not look like a clean run.
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

fn generate(profile_name: &str, output: &std::path::Path) -> u8 {
    let policy = match resolve_policy(Some(profile_name)) {
        Ok(p) => p,
        Err(code) => return code,
    };
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
    if durable {
        eprintln!("warning: --durable is not yet implemented in the Rust port; limits will lapse.");
    }
    if !config.is_dir() {
        eprintln!(
            "error: config directory {} does not exist. Run 'generate' first.",
            config.display()
        );
        return exit::ERROR;
    }

    #[cfg(unix)]
    if unsafe { libc::geteuid() } != 0 {
        eprintln!("error: apply needs root: re-run with sudo.");
        return exit::NEEDS_ROOT;
    }

    match apply::run(config, dry_run, yes) {
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

    #[cfg(unix)]
    if unsafe { libc::geteuid() } != 0 {
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
}
