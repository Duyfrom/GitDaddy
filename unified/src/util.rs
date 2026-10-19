use std::cmp::Ordering;
use std::path::Path;
use std::sync::OnceLock;
use std::time::SystemTime;

fn home_dir() -> Option<&'static str> {
    static HOME: OnceLock<Option<String>> = OnceLock::new();
    HOME.get_or_init(|| {
        dirs::home_dir()
            .map(|p| p.to_string_lossy().trim_end_matches(['\\', '/']).to_string())
            .filter(|p| !p.is_empty())
    })
    .as_deref()
}

/// Shortens `C:\Users\me\Projects\app` to `~\Projects\app`.
pub fn abbreviate_home(path: &str) -> String {
    let Some(home) = home_dir() else {
        return path.to_string();
    };
    let matches_home =
        path.len() >= home.len() && path.is_char_boundary(home.len()) && path[..home.len()].eq_ignore_ascii_case(home);
    if !matches_home {
        return path.to_string();
    }
    let suffix = &path[home.len()..];
    if suffix.is_empty() {
        "~".to_string()
    } else if suffix.starts_with(['\\', '/']) {
        format!("~{suffix}")
    } else {
        path.to_string()
    }
}

/// Last path component, falling back to the whole path for drive roots like `D:\`.
pub fn folder_name(path: &str) -> String {
    Path::new(path).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| path.to_string())
}

/// Case-insensitive comparison that orders embedded numbers by value ("repo2" < "repo10").
pub fn natural_cmp(a: &str, b: &str) -> Ordering {
    let mut a_chars = a.chars().peekable();
    let mut b_chars = b.chars().peekable();
    loop {
        match (a_chars.peek().copied(), b_chars.peek().copied()) {
            (None, None) => return a.cmp(b),
            (None, Some(_)) => return Ordering::Less,
            (Some(_), None) => return Ordering::Greater,
            (Some(ca), Some(cb)) if ca.is_ascii_digit() && cb.is_ascii_digit() => {
                let mut na = String::new();
                while let Some(c) = a_chars.peek().copied().filter(char::is_ascii_digit) {
                    na.push(c);
                    a_chars.next();
                }
                let mut nb = String::new();
                while let Some(c) = b_chars.peek().copied().filter(char::is_ascii_digit) {
                    nb.push(c);
                    b_chars.next();
                }
                let ta = na.trim_start_matches('0');
                let tb = nb.trim_start_matches('0');
                let ord = ta.len().cmp(&tb.len()).then_with(|| ta.cmp(tb));
                if ord != Ordering::Equal {
                    return ord;
                }
            }
            (Some(ca), Some(cb)) => {
                let ord = ca.to_lowercase().cmp(cb.to_lowercase());
                if ord != Ordering::Equal {
                    return ord;
                }
                a_chars.next();
                b_chars.next();
            }
        }
    }
}

pub fn plural<'a>(count: usize, one: &'a str, many: &'a str) -> &'a str {
    if count == 1 { one } else { many }
}

pub fn format_clock(time: SystemTime, with_seconds: bool) -> String {
    let local: chrono::DateTime<chrono::Local> = time.into();
    local.format(if with_seconds { "%H:%M:%S" } else { "%H:%M" }).to_string()
}

/// "just now", "5m ago", "3h ago", "2d ago", "3w ago", "4mo ago", "2y ago" for an RFC 3339 time.
pub fn relative_time(iso: &str) -> String {
    let Ok(time) = chrono::DateTime::parse_from_rfc3339(iso) else {
        return String::new();
    };
    let secs = (chrono::Utc::now() - time.with_timezone(&chrono::Utc)).num_seconds().max(0);
    const MIN: i64 = 60;
    const HOUR: i64 = 60 * MIN;
    const DAY: i64 = 24 * HOUR;
    match secs {
        s if s < MIN => "just now".into(),
        s if s < HOUR => format!("{}m ago", s / MIN),
        s if s < DAY => format!("{}h ago", s / HOUR),
        s if s < 7 * DAY => format!("{}d ago", s / DAY),
        s if s < 30 * DAY => format!("{}w ago", s / (7 * DAY)),
        s if s < 365 * DAY => format!("{}mo ago", s / (30 * DAY)),
        s => format!("{}y ago", s / (365 * DAY)),
    }
}

/// 950 → "950", 1234 → "1.2k", 45600 → "46k", 1_250_000 → "1.3M".
pub fn compact_count(n: u64) -> String {
    match n {
        0..=999 => n.to_string(),
        1_000..=9_999 => format!("{:.1}k", n as f64 / 1_000.0),
        10_000..=999_999 => format!("{}k", (n as f64 / 1_000.0).round()),
        _ => format!("{:.1}M", n as f64 / 1_000_000.0),
    }
}

pub fn first_line(text: &str) -> Option<String> {
    text.lines().map(str::trim).find(|l| !l.is_empty()).map(str::to_string)
}

pub fn last_line(text: &str) -> Option<String> {
    text.lines().map(str::trim).rfind(|l| !l.is_empty()).map(str::to_string)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn natural_order() {
        let mut names = vec!["repo10", "Repo2", "alpha", "repo1"];
        names.sort_by(|a, b| natural_cmp(a, b));
        assert_eq!(names, ["alpha", "repo1", "Repo2", "repo10"]);
    }

    #[test]
    fn compact_counts() {
        assert_eq!(compact_count(950), "950");
        assert_eq!(compact_count(1234), "1.2k");
        assert_eq!(compact_count(45_600), "46k");
        assert_eq!(compact_count(1_260_000), "1.3M");
        assert_eq!(relative_time("not a date"), "");
    }

    #[test]
    fn first_and_last_lines() {
        assert_eq!(first_line("\n  a \nb\n").as_deref(), Some("a"));
        assert_eq!(last_line("a\nb\n\n").as_deref(), Some("b"));
        assert_eq!(first_line("  \n"), None);
    }
}
