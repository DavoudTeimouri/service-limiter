//! Installing staged configs. The only privileged path.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use std::process::Command;
use std::process::Stdio;

/// What happened, so main can map it to an exit code.
#[derive(Debug, Default)]
pub struct Outcome {
    pub applied: usize,
    pub skipped: usize,
    pub failed: usize,
    /// Drift detected between staged config and installed limits — not a
    /// failure of this operation, but worth tracking separately so `status`
    /// exit code doesn't conflate "drift found" with "apply failed".
    pub drifted: usize,
}

impl Outcome {
    pub fn is_ok(&self) -> bool {
        self.failed == 0
    }
}

/// Apply `<config>/<service>/<file>` for the current platform.
///
/// `durable` keeps the limits in place after the tool exits. On Linux that is
/// already true (systemd owns the limit), so it is a no-op there. On Windows
/// the limits live in a Job Object whose handle the process holds, so it
/// means "keep a holder process alive".
pub fn run(
    config_dir: &Path,
    dry_run: bool,
    assume_yes: bool,
    durable: bool,
) -> std::io::Result<Outcome> {
    // The filename and the installer are chosen at runtime rather than by
    // `cfg`, so both platform paths are compiled and linted on every target.
    // A `#[cfg]`-gated path is only ever checked by one CI job, which is how
    // the Windows build broke twice before this was changed.
    if !matches!(std::env::consts::OS, "linux" | "windows") {
        eprintln!("error: Unsupported platform: {}", std::env::consts::OS);
        return Err(std::io::Error::new(
            std::io::ErrorKind::Unsupported,
            format!("unsupported platform: {}", std::env::consts::OS),
        ));
    }
    let filename = if cfg!(target_os = "windows") {
        "Set-JobLimits.ps1"
    } else {
        "override.conf"
    };

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
            // A dry run still reports a config systemd would reject, so the
            // user learns their config is broken before running for real.
            match validate_staged(&staged, filename) {
                Ok(()) => {
                    println!("[DRY-RUN] would apply {service} from {}", staged.display());
                    out.applied += 1;
                }
                Err(VerifyError::Rejected(reason)) => {
                    eprintln!("error: {service}: {reason}");
                    out.failed += 1;
                }
                Err(VerifyError::Unavailable(reason)) => {
                    // The validator is missing, not the config. That is worth
                    // a warning, not a failure.
                    eprintln!("warning: {service}: {reason}");
                    println!("[DRY-RUN] would apply {service} from {}", staged.display());
                    out.applied += 1;
                }
            }
            continue;
        }

        if !assume_yes && !confirm(&service) {
            println!("skipped {service}");
            out.skipped += 1;
            continue;
        }

        let installed = if cfg!(target_os = "windows") {
            install_windows(&service, &staged, durable)
        } else {
            // --durable is a Windows concern: systemd owns the limit and it
            // persists on its own.
            let _ = durable;
            install_linux(&service, &staged)
        };
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
///
/// Windows has no persistent installed artifact: limits live in a Job Object
/// held by the running process, so they vanish when it exits.
fn installed_path(service: &str) -> PathBuf {
    if cfg!(target_os = "windows") {
        return PathBuf::new();
    }
    PathBuf::from(format!(
        "/etc/systemd/system/{service}.service.d/override.conf"
    ))
}

/// Where the durable holder's PID is recorded, so `revert` can stop it.
///
/// Lives in the tool's home rather than the config directory: the holder
/// outlives the staging tree, and `revert` must still find it after the user
/// has deleted the config directory.
fn holder_pid_path(service: &str) -> PathBuf {
    crate::audit::home().join(format!("holder-{service}.pid"))
}

/// The scheduled-task name for a service's boot-time holder.
///
/// A namespaced name (`Folder\Name`) keeps the tool's tasks out of everyone
/// else's task list, and one task per service (not one task for all) means
/// `revert` can remove exactly the service it is reverting.
fn task_name(service: &str) -> String {
    format!("ServiceLimiter\\{service}")
}

/// `schtasks` arguments that re-create the durable holder at boot.
///
/// Built (and testable) as data rather than run inline, so both platform
/// paths stay compilable everywhere and the contract is visible without a
/// Windows host. `-StatusFile` is deliberately absent: at boot nobody waits
/// on it, and the task runs detached.
fn schedule_holder_args(shell: &str, staged: &Path, service: &str) -> Vec<String> {
    vec![
        "/Create".to_string(),
        "/F".to_string(),
        "/SC".to_string(),
        "ONSTART".to_string(),
        "/RU".to_string(),
        "".to_string(),
        "/TN".to_string(),
        task_name(service),
        "/TR".to_string(),
        format!(
            "\\\"{shell}\\\" -NoProfile -ExecutionPolicy Bypass -File '{}' -Durable",
            staged.display().to_string().replace('\'', "''")
        ),
    ]
}

/// Register the boot-time holder task. A failure is a hard error: the whole
/// point of `--durable` is that the limits survive, and silently keeping a
/// holder that dies at the next reboot would promise less than the user asked
/// for with no indication anything was lost.
fn schedule_holder(shell: &str, staged: &Path, service: &str) -> std::io::Result<()> {
    let out = Command::new("schtasks")
        .args(schedule_holder_args(shell, staged, service))
        .output()
        .map_err(|e| std::io::Error::other(format!("cannot run schtasks: {e}")))?;
    if !out.status.success() {
        return Err(std::io::Error::other(format!(
            "schtasks /Create failed: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        )));
    }
    println!(
        "survives reboot: scheduled task '{}' as SYSTEM, re-created at boot",
        task_name(service)
    );
    Ok(())
}

/// Remove the boot-time holder task. Best effort: reverting must not fail
/// just because a task is already gone.
fn unschedule_holder(service: &str) {
    let out = Command::new("schtasks")
        .args(["/Delete", "/TN", &task_name(service), "/F"])
        .output();
    match out {
        Ok(o) if o.status.success() => {
            println!("removed scheduled task '{}'", task_name(service))
        }
        Ok(o) => eprintln!(
            "note: could not remove scheduled task '{}': {}",
            task_name(service),
            String::from_utf8_lossy(&o.stderr).trim()
        ),
        Err(e) => eprintln!("note: cannot run schtasks: {e}"),
    }
}

/// Whether the boot-time holder task is registered, for `status`.
fn holder_task_registered(service: &str) -> bool {
    Command::new("schtasks")
        .args(["/Query", "/TN", &task_name(service)])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|s| s.success())
}

/// Stop the durable holder for a service, if one is running.
///
/// The holder is the process that holds the Job Object handle open. Killing
/// it closes the handle, which is what releases the limits.
fn stop_holder(service: &str) -> std::io::Result<bool> {
    let path = holder_pid_path(service);
    let Ok(text) = fs::read_to_string(&path) else {
        return Ok(false);
    };
    let pid = text.trim();
    if pid.is_empty() {
        let _ = fs::remove_file(&path);
        return Ok(false);
    }

    let out = Command::new("taskkill")
        .args(["/PID", pid, "/F"])
        .output()
        .map_err(|e| std::io::Error::other(format!("cannot run taskkill: {e}")))?;
    let _ = fs::remove_file(&path);

    if !out.status.success() {
        return Err(std::io::Error::other(format!(
            "could not stop holder process {pid}: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        )));
    }
    println!("stopped holder process {pid}; limits released");
    Ok(true)
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

        if cfg!(target_os = "windows") {
            // On Windows the limits live in a Job Object held by the durable
            // holder process. Stopping it closes the handle and releases them.
            match stop_holder(&service) {
                Ok(true) => out.applied += 1,
                Ok(false) => {
                    eprintln!(
                        "note: {service}: no durable holder is running, so no limits \
                         to release. Windows limits also lapse on their own when \
                         the holder exits."
                    );
                    out.skipped += 1;
                }
                Err(e) => {
                    eprintln!("error: {service}: {e}");
                    out.failed += 1;
                }
            }
            // Also remove the boot-time task so limits can't return at the
            // next reboot. Best effort: reverting must not fail if the task
            // is already gone.
            unschedule_holder(&service);
            continue;
        }

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
    Ok(out)
}

/// Restore the pre-apply config, or delete the drop-in when there was none.
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
        if fs::read_dir(dir)
            .map(|mut d| d.next().is_none())
            .unwrap_or(false)
        {
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

        if cfg!(target_os = "windows") {
            let pid_file = holder_pid_path(&service);
            match fs::read_to_string(&pid_file) {
                Ok(text) => println!(
                    "{service}: durable holder running, PID {} (stop it with 'revert')",
                    text.trim()
                ),
                Err(_) => println!("{service}: no durable holder; Windows limits lapse with it"),
            }
            // The boot-time task survives the holder process; report it so the
            // operator knows limits will return at the next reboot.
            if holder_task_registered(&service) {
                println!(
                    "  boot task '{}' registered: survives reboot",
                    task_name(&service)
                );
            }
            continue;
        }

        let target = installed_path(&service);
        if !target.exists() {
            println!("{service}: not installed");
            continue;
        }
        let backup = target.with_extension("conf.bak");
        let restorable = if backup.exists() {
            " (backup present)"
        } else {
            ""
        };
        println!("{service}: installed at {}{restorable}", target.display());

        // The installed file is what the OS enforces (effective); the staged
        // file is what the operator last generated (asked). They can drift:
        // an old drop-in the tool never rewrote, a hand edit, or a `generate`
        // with a different profile than the one applied. Saying "installed"
        // alone would hide all three.
        let staged = service_dir.join("override.conf");
        let asked = fs::read_to_string(&staged).ok();
        let effective = fs::read_to_string(&target).ok();
        match (asked.as_deref(), effective.as_deref()) {
            (Some(asked), Some(effective)) => match diff_limits(asked, effective) {
                None => println!("  limits match the staged config"),
                Some(diff) => {
                    println!("  DRIFTED: effective limits differ from the staged config");
                    for line in diff {
                        println!("    {line}");
                    }
                    println!("    re-run 'apply --yes' to install the staged config");
                    out.drifted += 1;
                }
            },
            // No staged config (or unreadable) says nothing about drift.
            _ => {
                for (key, value) in parse_limits(effective.as_deref().unwrap_or("")) {
                    println!("  effective {key}={value}");
                }
            }
        }
        out.applied += 1;
    }
    Ok(out)
}

/// Every `Key=Value` line in the drop-in, so drift detection covers any
/// directive the operator staged — not only the four this tool writes.
fn parse_limits(content: &str) -> Vec<(String, String)> {
    content
        .lines()
        .filter_map(|line| {
            let (key, value) = line.trim().split_once('=')?;
            let key = key.trim();
            let value = value.trim();
            (!key.is_empty() && !value.is_empty()).then(|| (key.to_string(), value.to_string()))
        })
        .collect()
}

/// Keys whose installed value differs from the staged value, or which are
/// missing from one side. `None` means the effective set matches the asked set.
fn diff_limits(asked: &str, effective: &str) -> Option<Vec<String>> {
    let asked = parse_limits(asked);
    let effective = parse_limits(effective);
    let mut keys: Vec<&str> = asked.iter().map(|(k, _)| k.as_str()).collect();
    keys.extend(effective.iter().map(|(k, _)| k.as_str()));
    keys.sort_unstable();
    keys.dedup();
    let value_of = |set: &[(String, String)], key: &str| -> String {
        set.iter()
            .find(|(k, _)| k.as_str() == key)
            .map(|(_, v)| v.clone())
            .unwrap_or_else(|| "(absent)".to_string())
    };
    let diff: Vec<String> = keys
        .into_iter()
        .filter(|key| value_of(&asked, key) != value_of(&effective, key))
        .map(|key| {
            format!(
                "{key}: staged {}, installed {}",
                value_of(&asked, key),
                value_of(&effective, key)
            )
        })
        .collect();
    (!diff.is_empty()).then_some(diff)
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

/// Install a systemd drop-in and restart the unit.
///
/// Not `#[cfg(target_os = "linux")]`: the body is plain `std::process` and
/// `std::fs`, so compiling it on every target means CI type-checks and
/// lints the Linux path even on a Windows runner.
fn install_linux(service: &str, staged: &Path) -> std::io::Result<bool> {
    let drop_in_dir: PathBuf = PathBuf::from(format!("/etc/systemd/system/{service}.service.d"));
    let target = drop_in_dir.join("override.conf");

    let content = fs::read_to_string(staged)?;

    // Validate BEFORE touching /etc. systemd ignores unknown keys silently, so
    // a bad directive would install cleanly and do nothing.
    if let Err(failure) = validate_staged(staged, "override.conf") {
        eprintln!("error: refusing {service}: {failure}");
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            failure.to_string(),
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

/// Why a staged config did not validate.
///
/// The two cases are different problems and must not be reported the same
/// way: "systemd rejected this config" is wrong when the real cause is that
/// `systemd-analyze` is not installed.
#[derive(Debug)]
pub enum VerifyError {
    /// systemd understood the config and refused it.
    Rejected(String),
    /// The validator itself could not run.
    Unavailable(String),
}

impl std::fmt::Display for VerifyError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            VerifyError::Rejected(m) | VerifyError::Unavailable(m) => write!(f, "{m}"),
        }
    }
}

/// Check a staged config before it is installed.
///
/// Only the systemd drop-in can be checked this way; the Windows script is
/// validated by PowerShell when it runs.
fn validate_staged(staged: &Path, filename: &str) -> Result<(), VerifyError> {
    if filename != "override.conf" {
        return Ok(());
    }
    let content = fs::read_to_string(staged)
        .map_err(|e| VerifyError::Unavailable(format!("cannot read {}: {e}", staged.display())))?;
    verify_with_systemd_analyze(&content)
}

/// Ask systemd whether it understands every line, by verifying a probe unit.
///
/// The probe mirrors a real drop-in exactly: one `[Unit]` description plus
/// the drop-in body (which starts with `[Service]`). Prepending a second
/// `[Service]` section made the probe merge sections differently than the
/// file we actually install, so a directive could pass the gate here and be
/// silently ignored on the real unit.
fn verify_with_systemd_analyze(content: &str) -> Result<(), VerifyError> {
    let mut probe = tempfile::NamedTempFile::with_suffix(".service")
        .map_err(|e| VerifyError::Unavailable(format!("cannot create probe file: {e}")))?;
    probe
        .write_all(b"[Unit]\nDescription=service-limiter probe\n")
        .and_then(|()| probe.write_all(content.as_bytes()))
        .map_err(|e| VerifyError::Unavailable(format!("cannot write probe file: {e}")))?;

    let out = Command::new("systemd-analyze")
        .arg("verify")
        .arg(probe.path())
        .output()
        .map_err(|e| {
            VerifyError::Unavailable(format!(
                "cannot run systemd-analyze to validate the config: {e}"
            ))
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
        Err(VerifyError::Rejected(format!(
            "systemd rejected this config:\n  {}",
            problems.join("\n  ")
        )))
    }
}

/// Apply the Job Object limits to a Windows service.
///
/// Not `#[cfg(target_os = "windows")]`: the body is plain `std::process`,
/// so compiling it everywhere means the Windows path is type-checked by the
/// Linux build and CI. It only *runs* on Windows.
fn install_windows(service: &str, staged: &Path, durable: bool) -> std::io::Result<bool> {
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

    if durable {
        // The script must stay alive to hold the Job Object handle, so it can
        // never be waited on. It reports through a status file instead, and
        // that is the only way to know whether it worked.
        let status = std::env::temp_dir().join(format!(
            "service-limiter-{}-{}.status",
            std::process::id(),
            service
        ));
        let _ = fs::remove_file(&status);

        let child = Command::new(shell)
            .args(["-NoProfile", "-ExecutionPolicy", "Bypass", "-File"])
            .arg(staged)
            .arg("-Durable")
            .arg("-StatusFile")
            .arg(&status)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()?;

        // Without this the call would report success even if the script
        // failed to apply anything.
        match wait_for_status(&status, std::time::Duration::from_secs(30)) {
            Some(line) if line.starts_with("OK:") => {
                println!("{}", line.trim_start_matches("OK:"));
                // Record the holder's PID so `revert` can stop it. Without
                // this, the only way to release the limits was to find the
                // process by hand.
                let pid_file = holder_pid_path(service);
                if let Some(parent) = pid_file.parent() {
                    let _ = fs::create_dir_all(parent);
                }
                if let Err(e) = fs::write(&pid_file, child.id().to_string()) {
                    eprintln!(
                        "warning: {service}: cannot record the holder PID at {}: {e}",
                        pid_file.display()
                    );
                }
                // A holder dies at the next reboot, and with it the limits.
                // Registering the task is what makes `--durable` true to its
                // name; a failure here fails the apply. If registration fails
                // after the holder is already running, stop the holder so we
                // never leave one that won't survive reboot (orphaned limits).
                match schedule_holder(shell, staged, service) {
                    Ok(()) => {
                        println!(
                            "Durable limits active. Holder process PID {}; stop it with \
                             'Stop-Process -Id {}' (or 'revert') to release the limits.",
                            child.id(),
                            child.id()
                        );
                        Ok(true)
                    }
                    Err(e) => {
                        let _ = Command::new("taskkill")
                            .args(["/PID", &child.id().to_string(), "/F"])
                            .status();
                        let _ = fs::remove_file(&pid_file);
                        Err(std::io::Error::other(format!(
                            "{service}: durable limits applied but boot task not registered: {e}"
                        )))
                    }
                }
            }
            Some(line) => Err(std::io::Error::other(line)),
            None => Err(std::io::Error::other(format!(
                "{service}: timed out waiting for the Job Object limits to be applied"
            ))),
        }
    } else {
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
}

/// Wait for a status file to appear and return its first line.
///
/// The file is written by the PowerShell script. Polling is used because the
/// script may never exit in durable mode, so its exit code is not available.
fn wait_for_status(path: &Path, timeout: std::time::Duration) -> Option<String> {
    let deadline = std::time::Instant::now() + timeout;
    loop {
        if let Ok(text) = fs::read_to_string(path) {
            if let Some(line) = text.lines().next() {
                let line = line.trim();
                if !line.is_empty() {
                    return Some(line.to_string());
                }
            }
        }
        if std::time::Instant::now() >= deadline {
            return None;
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The status file is the only way to learn the outcome in durable mode,
    /// so reading it has to work.
    #[test]
    fn wait_for_status_reads_the_first_line() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("status");
        std::fs::write(&path, "OK: Applied memory=100B\n").unwrap();

        let line = wait_for_status(&path, std::time::Duration::from_secs(1)).unwrap();
        assert_eq!(line, "OK: Applied memory=100B");
    }

    /// An error line must come back as an error, not as a success.
    #[test]
    fn wait_for_status_reports_errors_verbatim() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("status");
        std::fs::write(&path, "ERROR: Service not running\n").unwrap();

        let line = wait_for_status(&path, std::time::Duration::from_secs(1)).unwrap();
        assert!(line.starts_with("ERROR:"), "{line}");
    }

    /// A file that never appears must time out, not hang forever.
    #[test]
    fn wait_for_status_times_out() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("never-written");
        assert!(wait_for_status(&path, std::time::Duration::from_millis(150)).is_none());
    }

    /// An empty file is not a result yet; keep waiting.
    #[test]
    fn wait_for_status_ignores_an_empty_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("status");
        std::fs::write(&path, "   \n").unwrap();
        assert!(wait_for_status(&path, std::time::Duration::from_millis(150)).is_none());
    }

    /// The holder PID must land in the tool's home, not the config
    /// directory: the holder outlives the staging tree, and `revert` must
    /// still find it after the config directory is deleted.
    #[test]
    fn holder_pid_path_is_in_the_tool_home() {
        let path = holder_pid_path("sshd");
        assert!(path.starts_with(crate::audit::home()), "{path:?}");
        assert!(path.to_string_lossy().contains("sshd"), "{path:?}");
    }

    /// Drift detection catches any changed directive, not just the four the
    /// tool writes by default.
    #[test]
    fn diff_limits_catches_changed_and_missing_keys() {
        let asked = "[Service]\nCPUQuota=200%\nMemoryMax=256M\n";
        let effective = "[Service]\nCPUQuota=100%\nMemoryMax=256M\nTasksMax=500\n";

        let diff = diff_limits(asked, effective).expect("drift should be reported");
        assert!(diff
            .iter()
            .any(|d| d.contains("CPUQuota: staged 200%, installed 100%")));
        assert!(diff
            .iter()
            .any(|d| d.contains("TasksMax: staged (absent), installed 500")));
        // MemoryMax is unchanged, so it must not appear.
        assert!(!diff.iter().any(|d| d.contains("MemoryMax")));
    }

    /// Identical configs produce no drift.
    #[test]
    fn diff_limits_no_drift_when_identical() {
        let asked = "[Service]\nCPUQuota=200%\nMemoryMax=256M\n";
        let effective = "[Service]\nCPUQuota=200%\nMemoryMax=256M\n";
        assert!(diff_limits(asked, effective).is_none());
    }

    /// parse_limits captures every Key=Value line, not just known ones.
    #[test]
    fn parse_limits_captures_all_keys() {
        let content = "[Service]\nCPUQuota=50%\nCustomKey=weird\nMemoryMax=256M\n";
        let pairs = parse_limits(content);
        assert!(pairs.iter().any(|(k, v)| k == "CustomKey" && v == "weird"));
        assert!(pairs.iter().any(|(k, v)| k == "CPUQuota" && v == "50%"));
    }
}
