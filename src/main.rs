//! service-limiter: discover OS services, measure real usage, apply OS-native limits.
//!
//! See LOGIC.md for how this works and the platform traps it respects.

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
    /// `apply` needs root.
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
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    let result = match cli.command {
        Command::Analyze { profile } => analyze(profile.as_deref()),
        Command::Generate { profile, output } => generate(&profile, &output),
        Command::Apply { config, dry_run, yes, durable } => {
            apply(&config, dry_run, yes, durable)
        }
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
            return exit::ERROR;
        }
    };

    println!(
        "Discovered {} services, profiled {}, {} over policy",
        result.services.len(),
        result.profiles.len(),
        result.over_policy.len()
    );
    for svc in result.services.iter().take(5) {
        println!("  - {}: {} ({})", svc.name, svc.display_name, svc.status);
    }

    // Zero measurable services means we could not actually do the job, which
    // must not look like a clean run.
    if result.profiles.is_empty() {
        eprintln!("error: no service could be profiled (no live MainPID, or needs root)");
        return exit::ERROR;
    }
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
            return exit::ERROR;
        }
    };

    if result.configs.is_empty() {
        println!("No configs generated: no service exceeded the policy.");
        return exit::OK;
    }

    let filename = match std::env::consts::OS {
        "linux" => "override.conf",
        "windows" => "Set-JobLimits.ps1",
        other => {
            eprintln!("error: Unsupported platform: {other}");
            return exit::ERROR;
        }
    };

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
    println!("Generated {} configs in {}", result.configs.len(), output.display());
    exit::OK
}

fn apply(config: &std::path::Path, dry_run: bool, yes: bool, durable: bool) -> u8 {
    if !config.is_dir() {
        eprintln!("error: config directory {} does not exist. Run 'generate' first.", config.display());
        return exit::ERROR;
    }

    #[cfg(unix)]
    if !unsafe { libc::geteuid() } == 0 {
        eprintln!("error: apply needs root: re-run with sudo.");
        return exit::NEEDS_ROOT;
    }

    let _ = (yes, durable); // apply is implemented in the next migration step
    eprintln!("error: apply is not implemented in the Rust port yet; use the Python 1.x build.");
    exit::ERROR
}
