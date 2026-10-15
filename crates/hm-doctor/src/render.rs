//! Template filling and the small formatters cards use.

use std::collections::BTreeMap;
use std::path::{Component, Path};

use hm_core::signature::Level;

/// Facts for one card; a missing or empty fact leaves its sentence out.
pub type Facts = BTreeMap<&'static str, String>;

/// Fills `{fact}` placeholders. `"a || b"` uses `b` when `a` has a missing fact.
/// Returns `None` when every alternative has a missing fact.
pub fn render_line(template: &str, facts: &Facts) -> Option<String> {
    template
        .split(" || ")
        .find_map(|alternative| fill(alternative, facts))
}

pub fn render_lines(templates: &[String], facts: &Facts) -> Vec<String> {
    templates
        .iter()
        .filter_map(|template| render_line(template, facts))
        .collect()
}

fn fill(template: &str, facts: &Facts) -> Option<String> {
    let mut out = String::with_capacity(template.len());
    let mut rest = template;
    while let Some(start) = rest.find('{') {
        out.push_str(&rest[..start]);
        let after = &rest[start + 1..];
        let end = after.find('}')?;
        let value = facts.get(&after[..end]).filter(|value| !value.is_empty())?;
        out.push_str(value);
        rest = &after[end + 1..];
    }
    out.push_str(rest);
    Some(out)
}

/// "less than a minute" / "12 minutes" (beginner), "12 min" (builder), "12m5s" (expert).
pub fn duration(secs: u64, level: Level) -> String {
    let (days, hours, minutes, seconds) = (
        secs / 86_400,
        secs % 86_400 / 3_600,
        secs % 3_600 / 60,
        secs % 60,
    );
    match level {
        Level::Beginner => {
            let plural =
                |n: u64, unit: &str| format!("{n} {unit}{}", if n == 1 { "" } else { "s" });
            if secs < 60 {
                "less than a minute".into()
            } else if secs < 3_600 {
                plural(minutes, "minute")
            } else if secs < 86_400 {
                plural(hours, "hour")
            } else {
                plural(days, "day")
            }
        }
        Level::Builder => {
            if secs < 60 {
                format!("{seconds} s")
            } else if secs < 3_600 {
                format!("{minutes} min")
            } else if secs < 86_400 {
                format!("{hours} h {minutes} min")
            } else {
                format!("{days} d {hours} h")
            }
        }
        Level::Expert => {
            if days > 0 {
                format!("{days}d{hours}h{minutes}m{seconds}s")
            } else if hours > 0 {
                format!("{hours}h{minutes}m{seconds}s")
            } else if minutes > 0 {
                format!("{minutes}m{seconds}s")
            } else {
                format!("{seconds}s")
            }
        }
    }
}

/// Binary units: "512 KB", "182 MB", "1.4 GB".
pub fn bytes(n: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = KB * 1024;
    const GB: u64 = MB * 1024;
    if n >= GB {
        format!("{:.1} GB", n as f64 / GB as f64)
    } else if n >= MB {
        format!("{} MB", n / MB)
    } else if n >= KB {
        format!("{} KB", n / KB)
    } else {
        format!("{n} B")
    }
}

/// Unix seconds → `2026-10-15T07:12:03Z`.
pub fn utc(secs: u64) -> String {
    let days = (secs / 86_400) as i64;
    let rem = secs % 86_400;
    let (year, month, day) = civil_from_days(days);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}Z",
        rem / 3_600,
        rem % 3_600 / 60,
        rem % 60
    )
}

/// Days since 1970-01-01 → (year, month, day), after Howard Hinnant's algorithm.
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let month = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let year = yoe as i64 + era * 400 + i64::from(month <= 2);
    (year, month, day)
}

/// Replaces the home directory prefix with `~`, joined in the path's own style:
/// `~\code\shop` for a Windows path (drive or UNC prefix), `~/code/shop` otherwise.
pub fn tilde(path: &Path, home: Option<&Path>) -> String {
    match home.and_then(|home| path.strip_prefix(home).ok()) {
        Some(rest) if rest.as_os_str().is_empty() => "~".into(),
        Some(rest) => {
            let separator = match path.components().next() {
                Some(Component::Prefix(_)) => '\\',
                _ => '/',
            };
            format!("~{separator}{}", rest.display())
        }
        None => path.display().to_string(),
    }
}

/// Joins argv for display, quoting arguments that contain spaces.
pub fn join_argv(argv: &[String]) -> String {
    argv.iter()
        .map(|arg| {
            if arg.contains(' ') {
                format!("\"{arg}\"")
            } else {
                arg.clone()
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn facts(pairs: &[(&'static str, &str)]) -> Facts {
        pairs.iter().map(|(k, v)| (*k, v.to_string())).collect()
    }

    #[test]
    fn missing_facts_drop_the_sentence() {
        let f = facts(&[("name", "web"), ("url", "")]);
        assert_eq!(
            render_line("{name} runs.", &f).as_deref(),
            Some("web runs.")
        );
        assert_eq!(render_line("Open {url}", &f), None);
        assert_eq!(render_line("Port {port}", &f), None);
        assert_eq!(
            render_line("No facts here.", &f).as_deref(),
            Some("No facts here.")
        );
    }

    #[test]
    fn alternatives_fall_back_in_order() {
        let f = facts(&[("name", "db")]);
        assert_eq!(
            render_line("{name} is in {project}. || {name} has no project.", &f).as_deref(),
            Some("db has no project.")
        );
    }

    #[test]
    fn durations_read_naturally_per_level() {
        assert_eq!(duration(30, Level::Beginner), "less than a minute");
        assert_eq!(duration(60, Level::Beginner), "1 minute");
        assert_eq!(duration(7_380, Level::Beginner), "2 hours");
        assert_eq!(duration(7_380, Level::Builder), "2 h 3 min");
        assert_eq!(duration(7_385, Level::Expert), "2h3m5s");
        assert_eq!(duration(90_000, Level::Builder), "1 d 1 h");
    }

    #[test]
    fn formats_bytes_and_times() {
        assert_eq!(bytes(190_840_832), "182 MB");
        assert_eq!(bytes(1_503_238_553), "1.4 GB");
        assert_eq!(utc(0), "1970-01-01T00:00:00Z");
        assert_eq!(utc(1_760_500_000), "2025-10-15T03:46:40Z");
        assert_eq!(utc(1_709_164_800), "2024-02-29T00:00:00Z");
    }

    #[test]
    fn tilde_shortens_home_paths() {
        let home = Path::new("/home/me");
        assert_eq!(tilde(Path::new("/home/me/shop"), Some(home)), "~/shop");
        assert_eq!(tilde(Path::new("/home/me"), Some(home)), "~");
        assert_eq!(tilde(Path::new("/srv/app"), Some(home)), "/srv/app");
        #[cfg(windows)]
        assert_eq!(
            tilde(
                Path::new(r"C:\Users\me\code\shop"),
                Some(Path::new(r"C:\Users\me"))
            ),
            r"~\code\shop"
        );
    }
}
