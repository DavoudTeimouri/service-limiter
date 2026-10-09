//! Windows service discovery and process measurement.

use std::process::{Command, Stdio};
use std::time::Duration;

/// Preferred shell: PowerShell 7+, falling back to the deprecated 5.1 host.
pub fn powershell() -> Option<&'static str> {
    ["pwsh", "powershell"].into_iter().find(|s| {
        Command::new(s)
            .args(["-NoProfile", "-Command", "$PSVersionTable"])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .is_ok()
    })
}

/// MainPID of a service, from Win32_Service. 0 means not running.
pub fn main_pid(service: &str) -> Option<u32> {
    let script = format!("(Get-CimInstance Win32_Service -Filter \"Name='{service}\").ProcessId");
    let out = run_ps(&script).ok()?;
    out.trim().parse().ok()
}

/// Running services, via CIM. Returns Err when no PowerShell is available.
pub fn discover_services() -> Result<Vec<crate::models::ServiceDescriptor>, String> {
    let script = "Get-CimInstance Win32_Service | \
                  Select-Object Name,DisplayName,State,StartMode,PathName,ProcessId,StartName | \
                  ConvertTo-Json -Compress";
    let raw = run_ps(script)?;

    // A single service serialises as an object rather than an array.
    let parsed: serde_json::Value =
        serde_json::from_str(&raw).map_err(|e| format!("cannot parse CIM output: {e}"))?;
    let items = match parsed {
        serde_json::Value::Array(a) => a,
        other => vec![other],
    };

    Ok(items
        .iter()
        .filter_map(|v| {
            let name = v.get("Name")?.as_str()?;
            let pid = v
                .get("ProcessId")
                .and_then(serde_json::Value::as_u64)
                .unwrap_or(0);
            Some(crate::models::ServiceDescriptor {
                name: name.to_string(),
                display_name: v
                    .get("DisplayName")
                    .and_then(|d| d.as_str())
                    .unwrap_or(name)
                    .to_string(),
                status: v.get("State").and_then(|s| s.as_str()).unwrap_or("").into(),
                start_type: v
                    .get("StartMode")
                    .and_then(|s| s.as_str())
                    .unwrap_or("")
                    .into(),
                path: v
                    .get("PathName")
                    .and_then(|s| s.as_str())
                    .unwrap_or("")
                    .into(),
                account: v
                    .get("StartName")
                    .and_then(|s| s.as_str())
                    .unwrap_or("")
                    .into(),
                child_processes: if pid > 0 {
                    vec![pid as u32]
                } else {
                    Vec::new()
                },
            })
        })
        .collect())
}

fn run_ps(script: &str) -> Result<String, String> {
    let shell =
        powershell().ok_or_else(|| "no PowerShell found (tried pwsh, powershell)".to_string())?;
    let out = Command::new(shell)
        .args(["-NoProfile", "-Command", script])
        .output()
        .map_err(|e| format!("{shell} failed: {e}"))?;
    if !out.status.success() {
        return Err(format!(
            "{shell} exited {}: {}",
            out.status,
            String::from_utf8_lossy(&out.stderr).trim()
        ));
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

pub const DEFAULT_INTERVAL: Duration = Duration::from_millis(250);
