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
        return Err(std::io::Error::new(
            std::io::ErrorKind::Unsupported,
            format!("unsupported platform: {}", std::env::consts::OS),
        ));
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

/// The installed path for a service, if this platform has one.
#[cfg(target_os = "linux")]
fn installed_path(service: &str) -> PathBuf {
    PathBuf::from(format!("/etc/systemd/system/{service}.service.d/override.conf"))
}

/// Windows has no persistent installed artifact: limits live in a Job Object
/// held by the running process, so they vanish when it exits.
#[cfg(target_os = "windows")]
fn installed_path(_service: &str) -> std::path::PathBuf {
    std::path::PathBuf::new()
}

/// Remove installed limits for every staged service.
///
/// `apply` is one-way: it writes a drop-in and restarts the unit, and nothing
/// put it back. This restores the file `apply` backed up, or removes the
/// drop-in when there was no original.
pub fn revert(config_dir: &Path, dry_run: bool, assume_yes: bool) -> std::io::Result<Outcome> {
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

        if !assume_yes && !confirm_revert(&service) {
            println!("skipped {service}");
            out.skipped += 1;
            continue;
        }

        #[cfg(target_os = "linux")]
        {
            let target = installed_path(&service);
            if !target.exists() {
                eprintln!("note: {service}: nothing installed at {}", target.display());
                out.skipped += 1;
                continue;
            }

            if dry_run {
                println!("[DRY-RUN] would revert {service} at {}", target.display());
                out.applied += 1;
                continue;
            }

            match revert_linux(&service, &target) {
                Ok(()) => out.applied += 1,
                Err(e) => {
                    eprintln!("error: {service}: {e}");
                    out.failed += 1;
                }
            }
        }

        #[cfg(target_os = "windows")]
        {
            eprintln!(
                "warning: {service}: Windows limits are held by a running process and \
                 cannot be removed from disk. Stop the service-limiter process \
                 that applied them."
            );
            out.skipped += 1;
        }
    }
    Ok(out)
}

/// Restore the pre-apply config, or delete the drop-in when there was none.
#[cfg(target_os = "linux")]
fn revert_linux(service: &str, target: &Path) -> std::io::Result<()> {
    let backup = target.with_extension("conf.bak");

    if backup.exists() {
        fs::copy(&backup, target)?;
        fs::remove_file(&backup)?;
        println!("restored {} from backup", target.display());
    } else {
        fs::remove_file(target)?;
        println!("removed {}", target.display());
    }

    // Remove the drop-in directory too when it is now empty; systemd treats an
    // empty .d directory as harmless but it is clutter.
    if let Some(dir) = target.parent() {
        if fs::read_dir(dir).map(|mut d| d.next().is_none()).unwrap_or(false) {
            let _ = fs::remove_dir(dir);
        }
    }

    let reload = Command::new("systemctl")
        .args(["daemon-reload"])
        .status()
        .map_err(|e| std::io::Error::other(format!("cannot run systemctl: {e}")))?;
    if !reload.success() {
        return Err(std::io::Error::other("systemctl daemon-reload failed"));
    }

    let restart = Command::new("systemctl")
        .args(["restart", service])
        .status()
        .map_err(|e| std::io::Error::other(format!("cannot run systemctl: {e}")))?;
    if !restart.success() {
        return Err(std::io::Error::other(format!(
            "systemctl restart {service} failed"
        )));
    }
    println!("restarted {service}");
    Ok(())
}

/// Report which staged services currently have limits installed.
pub fn status(config_dir: &Path) -> std::io::Result<Outcome> {
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

        #[cfg(target_os = "linux")]
        {
            let target = installed_path(&service);
            if target.exists() {
                let backup = target.with_extension("conf.bak");
                let restorable = if backup.exists() { " (backup present)" } else { "" };
                println!("{service}: installed at {}{restorable}", target.display());
                out.applied += 1;
            } else {
                println!("{service}: not installed");
            }
        }

        #[cfg(target_os = "windows")]
        {
            println!("{service}: cannot be determined; Windows limits live in a Job Object");
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

/// Confirmation for revert, worded so the user knows what is being undone.
fn confirm_revert(service: &str) -> bool {
    print!("Remove limits from {service}? [y/N] ");
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
    if let Err(failure) = verify_with_systemd_analyze(&content) {
        eprintln!("error: refusing {service}: {failure}");
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            failure,
        ));
    }

    fs::create_dir_all(&drop_in_dir)?;
    if target.exists() {
        // Back up so apply is reversible. Never overwrite an existing backup:
        // a second apply would otherwise copy the already-overridden file over
        // the original and destroy the only clean copy.
        let backup = target.with_extension("conf.bak");
        if backup.exists() {
            println!(
                "existing config already backed up to {} (left untouched)",
                backup.display()
            );
        } else {
            fs::copy(&target, &backup)?;
            println!("backed up existing config to {}", backup.display());
        }
    }
    fs::write(&target, &content)?;
    println!("installed {}", target.display());

    // systemctl's exit code is the only signal that the unit exists and the
    // restart worked. Ignoring it meant a failed restart was reported as
    // "restarted {service}" and the user saw success on a broken host.
    let reload = Command::new("systemctl")
        .args(["daemon-reload"])
        .status()
        .map_err(|e| std::io::Error::other(format!("cannot run systemctl: {e}")))?;
    if !reload.success() {
        return Err(std::io::Error::other(format!(
            "systemctl daemon-reload failed with exit code {}",
            reload.code().unwrap_or(-1)
        )));
    }

    let restart = Command::new("systemctl")
        .args(["restart", service])
        .status()
        .map_err(|e| std::io::Error::other(format!("cannot run systemctl: {e}")))?;
    if !restart.success() {
        return Err(std::io::Error::other(format!(
            "systemctl restart {service} failed with exit code {}",
            restart.code().unwrap_or(-1)
        )));
    }
    println!("restarted {service}");
    Ok(true)
}

/// Ask systemd whether it understands every line, by verifying a probe unit.
///
/// Returns a human-readable reason on failure: either that systemd rejected
/// the content, or that the validator itself could not run. Those are
/// different problems and must not be reported the same way -- "systemd
/// rejected this config" is wrong and misleading when the real cause is that
/// `systemd-analyze` is not installed.
#[cfg(target_os = "linux")]
fn verify_with_systemd_analyze(content: &str) -> Result<(), String> {
    let mut probe = tempfile::NamedTempFile::with_suffix(".service")
        .map_err(|e| format!("cannot create probe file: {e}"))?;
    probe
        .write_all(b"[Service]\nExecStart=/bin/true\n")
        .and_then(|()| probe.write_all(content.as_bytes()))
        .map_err(|e| format!("cannot write probe file: {e}"))?;

    let out = Command::new("systemd-analyze")
        .arg("verify")
        .arg(probe.path())
        .output()
        .map_err(|e| {
            format!("cannot run systemd-analyze to validate the config: {e}")
        })?;

    let text = String::from_utf8_lossy(&out.stderr);
    let problems: Vec<String> = text
        .lines()
        .filter(|l| l.contains("Unknown key") || l.contains("Invalid"))
        .map(str::to_owned)
        .collect();

    if problems.is_empty() {
        Ok(())
    } else {
        Err(format!(
            "systemd rejected this config:\n  {}",
            problems.join("\n  ")
        ))
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
