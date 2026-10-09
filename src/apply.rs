//! Installing staged configs. The only privileged path.

use std::fs;
use std::io::Write;
use std::path::Path;
#[cfg(target_os = "linux")]
use std::path::PathBuf;

use std::process::Command;
#[cfg(target_os = "windows")]
use std::process::Stdio;

/// What happened, so main can map it to an exit code.
#[derive(Debug, Default)]
pub struct Outcome {
    pub applied: usize,
    pub skipped: usize,
    pub failed: usize,
}

impl Outcome {
    pub fn is_ok(&self) -> bool {
        self.failed == 0
    }
}

/// Apply `<config>/<service>/<file>` for the current platform.
pub fn run(config_dir: &Path, dry_run: bool, assume_yes: bool) -> std::io::Result<Outcome> {
    // cfg-gated per OS rather than a fn-pointer table: one target is compiled
    // at a time, so there is nothing to dispatch on at runtime.
    #[cfg(target_os = "linux")]
    let filename = "override.conf";
    #[cfg(target_os = "windows")]
    let filename = "Set-JobLimits.ps1";
    #[cfg(not(any(target_os = "linux", target_os = "windows")))]
    {
        eprintln!("error: Unsupported platform: {}", std::env::consts::OS);
        return Ok(Outcome::default());
    }

    let mut entries: Vec<_> = fs::read_dir(config_dir)?.filter_map(Result::ok).collect();
    entries.sort_by_key(std::fs::DirEntry::file_name);

    let mut out = Outcome::default();
    for entry in entries {
        let service_dir = entry.path();
        if !service_dir.is_dir() {
            continue;
        }
        let service = service_dir
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let staged = service_dir.join(filename);

        if !staged.is_file() {
            eprintln!(
                "warning: no {filename} for {service} in {}",
                service_dir.display()
            );
            out.failed += 1;
            continue;
        }

        if dry_run {
            println!("[DRY-RUN] would apply {service} from {}", staged.display());
            out.applied += 1;
            continue;
        }

        if !assume_yes && !confirm(&service) {
            println!("skipped {service}");
            out.skipped += 1;
            continue;
        }

        #[cfg(target_os = "linux")]
        let installed = install_linux(&service, &staged);
        #[cfg(target_os = "windows")]
        let installed = install_windows(&service, &staged);

        match installed {
            Ok(true) => out.applied += 1,
            Ok(false) => out.skipped += 1,
            Err(e) => {
                eprintln!("error: {service}: {e}");
                out.failed += 1;
            }
        }
    }
    Ok(out)
}

fn confirm(service: &str) -> bool {
    print!("Apply limits to {service}? [y/N] ");
    let _ = std::io::stdout().flush();
    let mut answer = String::new();
    if std::io::stdin().read_line(&mut answer).is_err() {
        return false;
    }
    matches!(answer.trim().to_lowercase().as_str(), "y" | "yes")
}

#[cfg(target_os = "linux")]
#[allow(dead_code)]
fn install_linux(service: &str, staged: &Path) -> std::io::Result<bool> {
    let drop_in_dir: PathBuf = PathBuf::from(format!("/etc/systemd/system/{service}.service.d"));
    let target = drop_in_dir.join("override.conf");

    let content = fs::read_to_string(staged)?;

    // Validate BEFORE touching /etc. systemd ignores unknown keys silently, so
    // a bad directive would install cleanly and do nothing.
    if let Err(problems) = verify_with_systemd_analyze(&content) {
        eprintln!("error: refusing {service}: systemd rejected this config:");
        for p in &problems {
            eprintln!("  {p}");
        }
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!("systemd rejected the drop-in: {}", problems.join("; ")),
        ));
    }

    fs::create_dir_all(&drop_in_dir)?;
    if target.exists() {
        // Back up so apply is reversible; there is no revert command yet.
        fs::copy(&target, target.with_extension("conf.bak"))?;
        println!(
            "backed up existing config to {}",
            target.with_extension("conf.bak").display()
        );
    }
    fs::write(&target, &content)?;
    println!("installed {}", target.display());

    Command::new("systemctl").args(["daemon-reload"]).status()?;
    Command::new("systemctl")
        .args(["restart", service])
        .status()?;
    println!("restarted {service}");
    Ok(true)
}

/// Ask systemd whether it understands every line, by verifying a probe unit.
#[cfg(target_os = "linux")]
///
/// Returns the problems systemd complained about, or Ok(()) when clean.
fn verify_with_systemd_analyze(content: &str) -> Result<(), Vec<String>> {
    let mut probe = tempfile::NamedTempFile::with_suffix(".service")
        .map_err(|e| vec![format!("cannot create probe file: {e}")])?;
    probe
        .write_all(b"[Service]\nExecStart=/bin/true\n")
        .and_then(|()| probe.write_all(content.as_bytes()))
        .map_err(|e| vec![format!("cannot write probe file: {e}")])?;

    let out = Command::new("systemd-analyze")
        .arg("verify")
        .arg(probe.path())
        .output()
        .map_err(|e| vec![format!("systemd-analyze unavailable: {e}")])?;

    let text = String::from_utf8_lossy(&out.stderr);
    let problems: Vec<String> = text
        .lines()
        .filter(|l| l.contains("Unknown key") || l.contains("Invalid"))
        .map(str::to_owned)
        .collect();

    if problems.is_empty() {
        Ok(())
    } else {
        Err(problems)
    }
}

#[cfg(target_os = "windows")]
fn install_windows(service: &str, staged: &Path) -> std::io::Result<bool> {
    // Prefer pwsh (PowerShell 7+); fall back to the deprecated 5.1 host.
    let shell = ["pwsh", "powershell"]
        .into_iter()
        .find(|s| {
            Command::new(s)
                .arg("-NoProfile")
                .arg("-Command")
                .arg("$PSVersionTable")
                .stdout(Stdio::null())
                .status()
                .is_ok()
        })
        .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::NotFound, "no PowerShell found"))?;

    let out = Command::new(shell)
        .args(["-NoProfile", "-ExecutionPolicy", "Bypass", "-File"])
        .arg(staged)
        .output()?;
    if !out.status.success() {
        return Err(std::io::Error::other(
            String::from_utf8_lossy(&out.stderr).into_owned(),
        ));
    }
    println!("{}", String::from_utf8_lossy(&out.stdout).trim());
    eprintln!(
        "warning: Job Object limits lapse when the script exits. Use --durable to keep them."
    );
    let _ = service;
    Ok(true)
}
