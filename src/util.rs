//! Errors, time formatting, validation and flat frontmatter helpers.

use std::time::{SystemTime, UNIX_EPOCH};

pub struct Fail {
    pub code: i32,
    pub msg: String,
}

pub type R<T> = Result<T, Fail>;

/// User error: exit 1.
pub fn die<T>(m: impl AsRef<str>) -> R<T> {
    Err(Fail {
        code: 1,
        msg: format!("triage: {}", m.as_ref()),
    })
}

/// Usage error: exit 2.
pub fn usage_err<T>(m: impl AsRef<str>) -> R<T> {
    Err(Fail {
        code: 2,
        msg: format!("triage: {}\nrun 'triage help' for usage", m.as_ref()),
    })
}

// ------------------------------------------------------------------ time

/// (year, month, day) for days since 1970-01-01.
pub fn civil_from_days(z: i64) -> (i64, u32, u32) {
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

pub fn format_date(secs: i64) -> String {
    let (y, m, d) = civil_from_days(secs.div_euclid(86_400));
    format!("{y:04}-{m:02}-{d:02}")
}

pub fn format_ts(secs: i64) -> String {
    let t = secs.rem_euclid(86_400);
    format!(
        "{}T{:02}:{:02}:{:02}Z",
        format_date(secs),
        t / 3600,
        t % 3600 / 60,
        t % 60
    )
}

fn unix_now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

pub fn now_utc() -> String {
    format_ts(unix_now())
}

pub fn today() -> String {
    format_date(unix_now())
}

// ------------------------------------------------------------ validation

/// POSIX `[[:space:]]` for ASCII.
pub fn is_space(c: char) -> bool {
    matches!(c, ' ' | '\t' | '\n' | '\x0b' | '\x0c' | '\r')
}

pub fn upper(s: &str) -> String {
    s.to_ascii_uppercase()
}

/// `^[A-Za-z0-9][A-Za-z0-9._-]*$`
pub fn valid_id(s: &str) -> bool {
    let b = s.as_bytes();
    !b.is_empty()
        && b[0].is_ascii_alphanumeric()
        && b[1..]
            .iter()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, b'.' | b'_' | b'-'))
}

/// `^([id]/)?id$`
pub fn valid_rel(s: &str) -> bool {
    match s.split_once('/') {
        Some((o, i)) => valid_id(o) && valid_id(i),
        None => valid_id(s),
    }
}

/// `^https?://[^[:space:]]+$`
pub fn valid_url(s: &str) -> bool {
    let rest = s
        .strip_prefix("https://")
        .or_else(|| s.strip_prefix("http://"));
    match rest {
        Some(r) => !r.is_empty() && !r.chars().any(is_space),
        None => false,
    }
}

/// `^[0-9a-fA-F]{4,40}$`
pub fn valid_sha(s: &str) -> bool {
    (4..=40).contains(&s.len()) && s.bytes().all(|c| c.is_ascii_hexdigit())
}

/// `^YYYY-MM-DD$` with month 01-12 and day 01-31.
pub fn valid_date(s: &str) -> bool {
    let b = s.as_bytes();
    if b.len() != 10 || b[4] != b'-' || b[7] != b'-' {
        return false;
    }
    let digits = |r: std::ops::Range<usize>| b[r].iter().all(|c| c.is_ascii_digit());
    if !(digits(0..4) && digits(5..7) && digits(8..10)) {
        return false;
    }
    let m: u32 = s[5..7].parse().unwrap_or(0);
    let d: u32 = s[8..10].parse().unwrap_or(0);
    (1..=12).contains(&m) && (1..=31).contains(&d)
}

// ----------------------------------------------------------------- lists

/// Trimmed, non-empty items of a comma list (newlines also separate).
pub fn list_items(s: &str) -> Vec<String> {
    s.split([',', '\n'])
        .map(|i| i.trim_matches(|c| c == ' ' || c == '\t'))
        .filter(|i| !i.is_empty())
        .map(String::from)
        .collect()
}

fn dedup_join(items: Vec<String>) -> String {
    let mut seen: Vec<String> = Vec::new();
    for i in items {
        if !seen.contains(&i) {
            seen.push(i);
        }
    }
    seen.join(", ")
}

pub fn list_norm(s: &str) -> String {
    dedup_join(list_items(s))
}

/// Uppercase the issue part of each related item; keep the org part.
pub fn related_norm(s: &str) -> String {
    dedup_join(
        list_items(s)
            .into_iter()
            .map(|i| match i.find('/') {
                Some(n) => format!("{}{}", &i[..=n], i[n + 1..].to_ascii_uppercase()),
                None => i.to_ascii_uppercase(),
            })
            .collect(),
    )
}

pub fn norm_field(key: &str, v: &str) -> String {
    match key {
        "tracker" => list_norm(v),
        "related" => related_norm(v),
        _ => v.to_string(),
    }
}

/// Error messages when `v` is invalid for `key`; empty when valid.
pub fn field_error(key: &str, v: &str) -> Vec<String> {
    let mut out = Vec::new();
    match key {
        "project" => {
            if !v.is_empty() && !valid_id(v) {
                out.push(format!("invalid project '{v}'"));
            }
        }
        "url" => {
            if !v.is_empty() && !valid_url(v) {
                out.push(format!("invalid url '{v}' (expected http(s)://...)"));
            }
        }
        "status" => {
            if !matches!(v, "open" | "closed") {
                out.push(format!("invalid status '{v}' (open|closed)"));
            }
        }
        "verdict" => {
            if !matches!(v, "unknown" | "bug" | "expected" | "investigate") {
                out.push(format!(
                    "invalid verdict '{v}' (unknown|bug|expected|investigate)"
                ));
            }
        }
        "confidence" => {
            if !matches!(v, "" | "low" | "medium" | "high") {
                out.push(format!(
                    "invalid confidence '{v}' (low|medium|high or empty)"
                ));
            }
        }
        "commit" => {
            if !v.is_empty() && !valid_sha(v) {
                out.push(format!("invalid commit '{v}' (expected a hex sha)"));
            }
        }
        "repo" => {
            if v.contains('\n') {
                out.push("repo must be a single line".to_string());
            }
        }
        "tracker" => {
            for item in list_items(v) {
                if item.chars().any(is_space) {
                    out.push(format!("invalid tracker item '{item}' (no spaces)"));
                }
            }
        }
        "related" => {
            for item in list_items(v) {
                if !valid_rel(&item) {
                    out.push(format!(
                        "invalid related item '{item}' (ORG/ISSUE or ISSUE)"
                    ));
                }
            }
        }
        _ => {}
    }
    out
}

// ----------------------------------------------------------- frontmatter

pub const FM_KEYS: [&str; 14] = [
    "schema",
    "issue",
    "org",
    "project",
    "url",
    "status",
    "verdict",
    "confidence",
    "tracker",
    "related",
    "repo",
    "commit",
    "created",
    "updated",
];

/// Records split like awk: by `\n`, a trailing partial line counts.
pub fn lines(s: &str) -> Vec<&str> {
    let mut v: Vec<&str> = s.split('\n').collect();
    if v.last() == Some(&"") {
        v.pop();
    }
    v
}

/// Value of one flat frontmatter key; empty when missing.
pub fn fm_get(content: &str, key: &str) -> String {
    let prefix = format!("{key}:");
    for (n, line) in lines(content).into_iter().enumerate() {
        if n == 0 {
            if line != "---" {
                break;
            }
            continue;
        }
        if line == "---" {
            break;
        }
        if let Some(v) = line.strip_prefix(&prefix) {
            return v.strip_prefix(' ').unwrap_or(v).to_string();
        }
    }
    String::new()
}

/// Replace the value of every matching frontmatter line.
pub fn fm_put(content: &str, key: &str, value: &str) -> String {
    let prefix = format!("{key}:");
    let mut out = String::with_capacity(content.len() + value.len());
    let mut infm = false;
    for (n, line) in lines(content).into_iter().enumerate() {
        if n == 0 {
            infm = line == "---";
            out.push_str(line);
        } else if infm && line == "---" {
            infm = false;
            out.push_str(line);
        } else if infm && line.starts_with(&prefix) {
            if value.is_empty() {
                out.push_str(&prefix);
            } else {
                out.push_str(&format!("{prefix} {value}"));
            }
        } else {
            out.push_str(line);
        }
        out.push('\n');
    }
    out
}

/// `^[a-z]+:( .*)?$`
fn fm_line_ok(line: &str) -> bool {
    let b = line.as_bytes();
    let n = b.iter().take_while(|c| c.is_ascii_lowercase()).count();
    n > 0 && b.get(n) == Some(&b':') && (b.len() == n + 1 || b[n + 1] == b' ')
}

/// Structural problems of the frontmatter block.
pub fn fm_struct_problems(content: &str) -> Vec<String> {
    let mut out = Vec::new();
    let ls = lines(content);
    if ls.is_empty() {
        out.push("frontmatter: state.md is empty".to_string());
        return out;
    }
    if ls[0] != "---" {
        out.push("frontmatter: missing opening ---".to_string());
        return out;
    }
    let mut seen: Vec<&str> = Vec::new();
    let mut closed = false;
    for (i, line) in ls.iter().enumerate().skip(1) {
        if *line == "---" {
            closed = true;
            break;
        }
        if !fm_line_ok(line) {
            out.push(format!("frontmatter: malformed line {}: {}", i + 1, line));
            continue;
        }
        let k = &line[..line.find(':').unwrap_or(line.len())];
        if !FM_KEYS.contains(&k) {
            out.push(format!("frontmatter: unknown key {k}"));
            continue;
        }
        if seen.contains(&k) {
            out.push(format!("frontmatter: duplicate key {k}"));
        }
        seen.push(k);
    }
    if !closed {
        out.push("frontmatter: missing closing ---".to_string());
        return out;
    }
    for k in FM_KEYS {
        if !seen.contains(&k) {
            out.push(format!("frontmatter: missing key {k}"));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn date_formatting() {
        assert_eq!(format_date(0), "1970-01-01");
        assert_eq!(format_ts(0), "1970-01-01T00:00:00Z");
        assert_eq!(format_ts(951_782_400 + 86_399), "2000-02-29T23:59:59Z");
        assert_eq!(format_date(1_788_000_000), "2026-08-29");
        assert_eq!(format_ts(4_102_444_799), "2099-12-31T23:59:59Z");
    }

    #[test]
    fn id_validation() {
        assert!(valid_id("CORE-88"));
        assert!(valid_id("a.b_c-d"));
        assert!(!valid_id(""));
        assert!(!valid_id("-x"));
        assert!(!valid_id("../evil"));
        assert!(!valid_id("bad id"));
        assert!(valid_rel("acme/CORE-1"));
        assert!(valid_rel("CORE-1"));
        assert!(!valid_rel("a/b/c"));
        assert!(!valid_rel("/x"));
    }

    #[test]
    fn value_validation() {
        assert!(valid_date("2026-10-17"));
        assert!(!valid_date("2026-13-01"));
        assert!(!valid_date("2026-00-10"));
        assert!(!valid_date("yesterday"));
        assert!(valid_sha("abc1"));
        assert!(!valid_sha("abc"));
        assert!(!valid_sha("xyz123"));
        assert!(valid_url("https://x.example/i/1"));
        assert!(!valid_url("ftp://x"));
        assert!(!valid_url("https://a b"));
    }

    const DOC: &str = "---\nschema: 1\nproject:\nurl: https://x/y\n---\n\nbody\nurl: no\n";

    #[test]
    fn frontmatter_get_put() {
        assert_eq!(fm_get(DOC, "schema"), "1");
        assert_eq!(fm_get(DOC, "project"), "");
        assert_eq!(fm_get(DOC, "url"), "https://x/y");
        assert_eq!(fm_get(DOC, "missing"), "");
        assert_eq!(fm_get("no frontmatter\nurl: x\n", "url"), "");
        let p = fm_put(DOC, "project", "core");
        assert!(p.contains("\nproject: core\n"));
        assert!(p.ends_with("body\nurl: no\n"));
        assert!(fm_put(&p, "project", "").contains("\nproject:\n"));
    }

    #[test]
    fn frontmatter_structure() {
        assert_eq!(fm_struct_problems(""), ["frontmatter: state.md is empty"]);
        assert_eq!(
            fm_struct_problems("x\n"),
            ["frontmatter: missing opening ---"]
        );
        assert_eq!(
            fm_struct_problems("---\nschema: 1\n"),
            ["frontmatter: missing closing ---"]
        );
        let p = fm_struct_problems("---\nschema: 1\nschema: 1\nzzz: 1\nbad\n---\n");
        assert!(p.contains(&"frontmatter: duplicate key schema".to_string()));
        assert!(p.contains(&"frontmatter: unknown key zzz".to_string()));
        assert!(p.contains(&"frontmatter: malformed line 5: bad".to_string()));
        assert!(p.contains(&"frontmatter: missing key issue".to_string()));
    }

    #[test]
    fn lists() {
        assert_eq!(list_norm(" a, b ,a,, c"), "a, b, c");
        assert_eq!(
            related_norm("core-2, acme/core-2, CORE-2"),
            "CORE-2, acme/CORE-2"
        );
        assert!(field_error("tracker", "a b").len() == 1);
        assert!(field_error("related", "a/b/c").len() == 1);
    }
}
