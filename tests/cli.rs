//! CLI contract tests: exit codes and staging layout are the public interface.

use std::path::Path;
use std::process::Command;

fn bin() -> &'static str {
    env!("CARGO_BIN_EXE_service-limiter")
}

fn run(args: &[&str]) -> std::process::Output {
    Command::new(bin())
        .args(args)
        .output()
        .expect("binary runs")
}

fn code(out: &std::process::Output) -> i32 {
    out.status.code().expect("exited normally")
}

/// Exit codes are documented in the README and depended on by scripts.
#[test]
fn missing_config_dir_is_error_not_ok() {
    let out = run(&["apply", "--config", "/nonexistent/path/xyz"]);
    assert_eq!(
        code(&out),
        2,
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

#[test]
fn unknown_profile_lists_available_ones() {
    let out = run(&[
        "generate",
        "--profile",
        "no-such-profile",
        "--output",
        "/tmp/x",
    ]);
    assert_eq!(code(&out), 2);
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("web-server"), "should list what exists: {err}");
}

#[test]
fn help_and_version_work() {
    assert_eq!(code(&run(&["--help"])), 0);
    assert_eq!(code(&run(&["--version"])), 0);
}

#[test]
fn revert_and_status_are_registered_commands() {
    // A command that exists in the enum but is not wired to a handler would
    // silently do nothing; clap must at least accept and route it.
    for cmd in ["revert", "status"] {
        let out = run(&[cmd, "--help"]);
        assert_eq!(code(&out), 0, "{cmd} --help failed");
    }
}

#[test]
fn status_of_a_missing_config_dir_is_an_error() {
    let out = run(&["status", "--config", "/nonexistent/path/xyz"]);
    assert_eq!(code(&out), 2);
}

#[test]
fn revert_of_a_missing_config_dir_is_an_error() {
    let out = run(&["revert", "--config", "/nonexistent/path/xyz"]);
    assert_eq!(code(&out), 2);
}

/// `revert --dry-run` must not touch the filesystem.
#[cfg(target_os = "linux")]
#[test]
fn revert_dry_run_changes_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let svc = dir.path().join("sshd");
    std::fs::create_dir_all(&svc).unwrap();
    std::fs::write(
        svc.join("override.conf"),
        "[Service]\nCPUQuota=50%\nMemoryMax=256M\n",
    )
    .unwrap();

    let out = run(&[
        "revert",
        "--config",
        dir.path().to_str().unwrap(),
        "--dry-run",
    ]);
    assert_eq!(
        code(&out),
        0,
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    // The staged config must survive a dry run untouched.
    assert!(svc.join("override.conf").exists());
}

/// A service that drops back under the policy must not stay staged.
///
/// Otherwise `apply` keeps installing limits for a service that no longer
/// needs them, which is the opposite of what the user asked for.
#[cfg(target_os = "linux")]
#[test]
fn generate_removes_stale_configs() {
    // `generate` discovers services through systemctl, so it cannot run at
    // all without it. Skip rather than assert on a path that never executes.
    if !Command::new("systemctl")
        .arg("--version")
        .output()
        .is_ok_and(|o| o.status.success())
    {
        eprintln!("skipping: systemctl not available");
        return;
    }

    let staging = tempfile::tempdir().unwrap();

    // Pre-stage a config for a service that is not over policy any more.
    let stale_dir = staging.path().join("ghostservice");
    std::fs::create_dir_all(&stale_dir).unwrap();
    std::fs::write(
        stale_dir.join("override.conf"),
        "[Service]\nCPUQuota=50%\n",
    )
    .unwrap();

    // A file the user put there by hand must survive.
    let keep = staging.path().join("notes.txt");
    std::fs::write(&keep, "mine").unwrap();

    // The generated set is empty here (no service exceeds the default
    // policy in this container), so generate must clear the stale entry.
    let out = run(&[
        "generate",
        "--profile",
        "web-server",
        "--output",
        staging.path().to_str().unwrap(),
    ]);
    // Either outcome is fine as long as the stale file is gone.
    let _ = code(&out);

    assert!(
        !stale_dir.join("override.conf").exists(),
        "stale config survived generate"
    );
    assert!(keep.exists(), "generate deleted a file it did not create");
}

#[cfg(target_os = "linux")]
#[test]
fn apply_dry_run_touches_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let svc = dir.path().join("sshd");
    std::fs::create_dir_all(&svc).unwrap();
    std::fs::write(
        svc.join("override.conf"),
        "[Service]\nCPUQuota=50%\nMemoryMax=256M\n",
    )
    .unwrap();

    let out = run(&[
        "apply",
        "--config",
        dir.path().to_str().unwrap(),
        "--dry-run",
    ]);
    assert_eq!(
        code(&out),
        0,
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(String::from_utf8_lossy(&out.stdout).contains("DRY-RUN"));
    // The staged tree must be unchanged; nothing may be written to /etc.
    assert!(svc.join("override.conf").exists());
    assert!(!Path::new("/etc/systemd/system/sshd.service.d/override.conf").exists());
}

#[cfg(target_os = "linux")]
#[test]
fn apply_rejects_a_config_systemd_would_ignore() {
    // Only meaningful where systemd-analyze exists. Without it the gate
    // refuses everything for a different reason, and this test would pass
    // for the wrong reason.
    if !Command::new("systemd-analyze")
        .arg("--version")
        .output()
        .is_ok_and(|o| o.status.success())
    {
        eprintln!("skipping: systemd-analyze not available");
        return;
    }

    let dir = tempfile::tempdir().unwrap();
    let svc = dir.path().join("bogus");
    std::fs::create_dir_all(&svc).unwrap();
    // ReadBandwidthMax is not a systemd key: systemd ignores it silently.
    std::fs::write(
        svc.join("override.conf"),
        "[Service]\nMemoryMax=256M\nReadBandwidthMax=1024\n",
    )
    .unwrap();

    let out = run(&["apply", "--config", dir.path().to_str().unwrap(), "--yes"]);
    // Either it is refused (3) or it fails the apply (2); it must never report success.
    assert_ne!(code(&out), 0, "must not silently accept an invalid drop-in");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("ReadBandwidthMax"),
        "must name the offending key, got: {stderr}"
    );
}

/// A VALID drop-in must survive the systemd-analyze gate.
///
/// This gate decides whether `apply` works at all. The probe file is
/// `[Service]\nExecStart=/bin/true\n` followed by the drop-in body, which
/// itself starts with `[Service]` -- so the probe carries two [Service]
/// sections. If systemd rejects that shape, every valid config would be
/// refused and `apply` would be completely broken.
///
/// Asserts the gate ACCEPTS valid content. It cannot assert the service was
/// installed, because that needs root and a real unit.
#[cfg(target_os = "linux")]
#[test]
fn valid_dropin_passes_the_systemd_analyze_gate() {
    // Needs systemd-analyze to say anything meaningful about the probe.
    if !Command::new("systemd-analyze")
        .arg("--version")
        .output()
        .is_ok_and(|o| o.status.success())
    {
        eprintln!("skipping: systemd-analyze not available");
        return;
    }

    let dir = tempfile::tempdir().unwrap();
    let svc = dir.path().join("sshd");
    std::fs::create_dir_all(&svc).unwrap();
    let dropin = "[Service]\nCPUQuota=50%\nMemoryMax=256M\n";
    std::fs::write(svc.join("override.conf"), dropin).unwrap();

    // Reproduce the probe exactly the way apply builds it.
    let probe = dir.path().join("probe.service");
    let mut body = String::from("[Service]\nExecStart=/bin/true\n");
    body.push_str(dropin);
    std::fs::write(&probe, &body).unwrap();

    let out = Command::new("systemd-analyze")
        .arg("verify")
        .arg(&probe)
        .output()
        .expect("systemd-analyze available");

    let stderr = String::from_utf8_lossy(&out.stderr);
    let problems: Vec<&str> = stderr
        .lines()
        .filter(|l| l.contains("Unknown key") || l.contains("Invalid"))
        .collect();

    assert!(
        problems.is_empty(),
        "a VALID drop-in was rejected by the gate: {problems:?}\nfull stderr:\n{stderr}"
    );
}
