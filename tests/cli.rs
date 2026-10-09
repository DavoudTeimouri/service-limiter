//! CLI contract tests: exit codes and staging layout are the public interface.

use std::path::Path;
use std::process::Command;

fn bin() -> &'static str {
    env!("CARGO_BIN_EXE_service-limiter")
}

fn run(args: &[&str]) -> std::process::Output {
    Command::new(bin()).args(args).output().expect("binary runs")
}

fn code(out: &std::process::Output) -> i32 {
    out.status.code().expect("exited normally")
}

/// Exit codes are documented in the README and depended on by scripts.
#[test]
fn missing_config_dir_is_error_not_ok() {
    let out = run(&["apply", "--config", "/nonexistent/path/xyz"]);
    assert_eq!(code(&out), 2, "stderr: {}", String::from_utf8_lossy(&out.stderr));
}

#[test]
fn unknown_profile_lists_available_ones() {
    let out = run(&["generate", "--profile", "no-such-profile", "--output", "/tmp/x"]);
    assert_eq!(code(&out), 2);
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("web-server"), "should list what exists: {err}");
}

#[test]
fn help_and_version_work() {
    assert_eq!(code(&run(&["--help"])), 0);
    assert_eq!(code(&run(&["--version"])), 0);
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

    let out = run(&["apply", "--config", dir.path().to_str().unwrap(), "--dry-run"]);
    assert_eq!(code(&out), 0, "stderr: {}", String::from_utf8_lossy(&out.stderr));
    assert!(String::from_utf8_lossy(&out.stdout).contains("DRY-RUN"));
    // The staged tree must be unchanged; nothing may be written to /etc.
    assert!(svc.join("override.conf").exists());
    assert!(!Path::new("/etc/systemd/system/sshd.service.d/override.conf").exists());
}

#[cfg(target_os = "linux")]
#[test]
fn apply_rejects_a_config_systemd_would_ignore() {
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
}
