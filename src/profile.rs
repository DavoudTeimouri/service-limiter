//! Bundled limit profiles, compiled into the binary.

use crate::models::Policy;

/// Profiles shipped with the tool.
pub const BUNDLED: &[(&str, &str)] = &[
    ("web-server", include_str!("../profiles/web-server.json")),
];

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

    let mut policy: Policy = serde_json::from_str(raw)
        .map_err(|e| format!("Invalid JSON in profile '{name}': {e}"))?;
    policy.name = name.to_string();
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
        assert!(err.contains("web-server"), "error should list what exists: {err}");
    }

    #[test]
    fn available_lists_web_server() {
        assert!(available().contains(&"web-server"));
    }
}
