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

/// MainPIDs for many services, in ONE PowerShell invocation.
///
/// One CIM query for the whole set, keyed by name in the output. Spawning a
/// PowerShell process per service meant N startups, each costing hundreds of
/// milliseconds, so listing 50 services took over 30 seconds.
pub fn main_pids(services: &[String]) -> HashMap<String, u32> {
    let mut out = HashMap::new();
    if services.is_empty() {
        return out;
    }

    // One CIM query for the whole set, keyed by name in the output.
    let script = "Get-CimInstance Win32_Service | \
                  Select-Object Name,ProcessId | \
                  ConvertTo-Json -Compress";
    let Ok(raw) = run_ps(script) else {
        return out;
    };
    let Ok(parsed) = serde_json::from_str::<serde_json::Value>(&raw) else {
        return out;
    };
    let items = match parsed {
        serde_json::Value::Array(a) => a,
        other => vec![other],
    };
    let mut all: HashMap<String, u32> = HashMap::new();
    for v in &items {
        if let (Some(name), Some(pid)) = (
            v.get("Name").and_then(|n| n.as_str()),
            v.get("ProcessId").and_then(|p| p.as_u64()),
        ) {
            all.insert(name.to_string(), pid as u32);
        }
    }

    // Keep only the requested services, and only ones that are actually running.
    for name in services {
        if let Some(pid) = all.get(name) {
            if *pid > 0 {
                out.insert(name.clone(), *pid);
            }
        }
    }
    out
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
