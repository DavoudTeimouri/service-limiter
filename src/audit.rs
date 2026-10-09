//! Append-only audit log.
//!
//! One JSON object per line at `~/.service-limiter/audit.log`. This is the
//! record that a privileged `apply` actually ran, so it must be written even
//! when the run fails or is denied.

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::PathBuf;

/// Where the log lives. `$SERVICE_LIMITER_HOME` overrides it, which also keeps
/// tests off the developer's real home directory.
pub fn log_path() -> PathBuf {
    let base = std::env::var_os("SERVICE_LIMITER_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            let home = std::env::var_os("HOME")
                .or_else(|| std::env::var_os("USERPROFILE"))
                .map(PathBuf::from)
                .unwrap_or_else(|| PathBuf::from("."));
            home.join(".service-limiter")
        });
    base.join("audit.log")
}

/// Append one event.
///
/// Deliberately infallible from the caller's point of view: if the log cannot be
/// written we warn on stderr but never fail the operation. Losing an audit line
/// is bad; refusing to apply a limit because of it is worse.
pub fn record(event: &str, fields: &[(&str, String)]) {
    let path = log_path();
    let line = render(event, fields);

    if let Some(parent) = path.parent() {
        if let Err(e) = fs::create_dir_all(parent) {
            eprintln!("warning: cannot create {}: {e}", parent.display());
            return;
        }
    }

    match OpenOptions::new().create(true).append(true).open(&path) {
        Ok(mut f) => {
            if let Err(e) = writeln!(f, "{line}") {
                eprintln!("warning: cannot write {}: {e}", path.display());
            }
        }
        Err(e) => eprintln!("warning: cannot open {}: {e}", path.display()),
    }
}

/// Build the JSON line. Kept separate so it is testable without touching disk.
fn render(event: &str, fields: &[(&str, String)]) -> String {
    let mut out = String::from("{\"event\": ");
    out.push_str(&json_string(event));
    out.push_str(", \"time\": ");
    out.push_str(&json_string(&timestamp()));

    for (key, value) in fields {
        out.push_str(", ");
        out.push_str(&json_string(key));
        out.push_str(": ");
        out.push_str(&json_string(value));
    }
    out.push('}');
    out
}

/// Local time as `YYYY-MM-DDTHH:MM:SS+ZZZZ`.
///
/// ponytail: no chrono dependency. UTC via SystemTime, formatted by hand with
/// civil-from-days. Add chrono if sub-second precision or parsing is ever needed.
fn timestamp() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);

    let days = secs.div_euclid(86_400);
    let tod = secs.rem_euclid(86_400);
    let (y, m, d) = civil_from_days(days);
    format!(
        "{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}+0000",
        tod / 3600,
        (tod % 3600) / 60,
        tod % 60
    )
}

/// Howard Hinnant's days-from-civil, inverted.
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

/// Minimal JSON string escaping. Values here are paths and service names, but
/// a service name could contain a quote or a backslash, so escape properly.
fn json_string(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_a_json_object() {
        let line = render(
            "apply",
            &[("config", "./config".into()), ("ok", "true".into())],
        );
        assert!(line.starts_with("{\"event\": \"apply\""), "{line}");
        assert!(line.contains("\"config\": \"./config\""), "{line}");
        assert!(line.contains("\"time\": \""), "{line}");
        assert!(line.ends_with('}'), "{line}");
    }

    #[test]
    fn escapes_hostile_values() {
        // A crafted service name must not break out of the JSON string.
        let line = render("apply", &[("name", "ev\"il\\path\ninjected".into())]);
        assert!(line.contains("ev\\\"il\\\\path\\ninjected"), "{line}");
        assert_eq!(line.matches('{').count(), 1, "no extra object: {line}");
    }

    #[test]
    fn timestamp_is_iso_shaped() {
        let t = timestamp();
        assert_eq!(t.len(), 24, "{t}");
        assert!(t.starts_with("20"), "{t}");
        assert!(t.contains('T'), "{t}");
        assert!(t.ends_with("+0000"), "{t}");
    }

    #[test]
    fn civil_conversion_matches_known_dates() {
        // Verified against python's datetime.date(1970,1,1) + timedelta.
        assert_eq!(civil_from_days(0), (1970, 1, 1));
        assert_eq!(civil_from_days(19_723), (2024, 1, 1));
        assert_eq!(civil_from_days(20_701), (2026, 9, 5));
    }

    #[test]
    fn leap_day_round_trips() {
        // Values verified against python's datetime.date(1970,1,1) + timedelta.
        assert_eq!(civil_from_days(19_785), (2024, 3, 3));
        assert_eq!(civil_from_days(20_726), (2026, 9, 30));
    }

    #[test]
    fn writes_one_line_per_event() {
        let dir = tempfile::tempdir().unwrap();
        // SAFETY: single-threaded test; the env var is read by log_path() only.
        unsafe { std::env::set_var("SERVICE_LIMITER_HOME", dir.path()) };

        record("analyze", &[("discovered", "3".into())]);
        record("apply", &[("ok", "true".into())]);

        let body = std::fs::read_to_string(dir.path().join("audit.log")).unwrap();
        let lines: Vec<&str> = body.lines().collect();
        assert_eq!(lines.len(), 2, "{body}");
        assert!(lines[0].contains("\"event\": \"analyze\""), "{body}");
        assert!(lines[1].contains("\"ok\": \"true\""), "{body}");

        unsafe { std::env::remove_var("SERVICE_LIMITER_HOME") };
    }
}
