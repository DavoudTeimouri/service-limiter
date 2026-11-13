//! Bundled limit profiles, compiled into the binary.

use crate::models::Policy;

/// Profiles shipped with the tool.
pub const BUNDLED: &[(&str, &str)] = &[("web-server", include_str!("../profiles/web-server.json"))];

/// Names of the bundled profiles.
pub fn available() -> Vec<&'static str> {
    BUNDLED.iter().map(|(name, _)| *name).collect()
}

/// Load a bundled profile by name.
///
/// A missing or malformed profile is an error, never a silent fallback to
/// defaults: a limiter that quietly applies different limits than the operator
/// asked for is worse than refusing to run.
pub fn load(name: &str) -> Result<Policy, String> {
    let raw = BUNDLED
        .iter()
        .find(|(n, _)| *n == name)
        .map(|(_, body)| *body)
        .ok_or_else(|| {
            format!(
                "Profile '{name}' not found. Available: {}",
                available().join(", ")
            )
        })?;

    let mut policy: Policy =
        serde_json::from_str(raw).map_err(|e| format!("Invalid JSON in profile '{name}': {e}"))?;
    policy.name = name.to_string();
    Ok(policy)
}

/// Load a user-supplied profile from a file, same schema as the bundled ones.
///
/// `name` comes from the file stem, so the audit log and the report name the
/// profile the way the operator wrote it. Like `load`, a missing or malformed
/// file is an error, never a silent fallback to defaults.
pub fn load_file(path: &std::path::Path) -> Result<Policy, String> {
    let raw = std::fs::read_to_string(path)
        .map_err(|e| format!("cannot read {}: {e}", path.display()))?;
    let mut policy: Policy = serde_json::from_str(&raw).map_err(|e| {
        format!(
            "Invalid JSON in profile {}: {e} (expects cpu_percent, memory_mb, \
             io_read_kbps, io_write_kbps)",
            path.display()
        )
    })?;
    if policy.name.is_empty() {
        policy.name = path
            .file_stem()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| "custom".to_string());
    }
    Ok(policy)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bundled_profile_loads() {
        let p = load("web-server").unwrap();
        assert_eq!(p.memory_mb, 256.0);
        assert_eq!(p.cpu_percent, 50.0);
    }

    #[test]
    fn missing_profile_is_an_error_not_a_fallback() {
        let err = load("does-not-exist").unwrap_err();
        assert!(err.contains("not found"), "{err}");
        assert!(
            err.contains("web-server"),
            "error should list what exists: {err}"
        );
    }

    #[test]
    fn available_lists_web_server() {
        assert!(available().contains(&"web-server"));
    }

    /// A user-supplied profile file loads and its stem becomes the name.
    #[test]
    fn load_file_reads_custom_profile() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("my-limit.json");
        std::fs::write(
            &path,
            r#"{"cpu_percent": 75, "memory_mb": 512, "io_read_kbps": 1024, "io_write_kbps": 512}"#,
        )
        .unwrap();

        let p = load_file(&path).unwrap();
        assert_eq!(p.name, "my-limit");
        assert_eq!(p.cpu_percent, 75.0);
        assert_eq!(p.memory_mb, 512.0);
    }

    /// A missing profile file is an error, not a fallback.
    #[test]
    fn load_file_missing_is_an_error() {
        let err = load_file(std::path::Path::new("/no/such/file.json")).unwrap_err();
        assert!(err.contains("cannot read"), "{err}");
    }
}
