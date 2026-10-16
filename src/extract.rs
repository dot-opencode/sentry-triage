//! Best-effort extraction from legacy text for `triage import`.
//! Hand-written equivalents of the POSIX ERE patterns used by the bash CLI.

use crate::util::is_space;

fn is_host(c: u8) -> bool {
    c.is_ascii_alphanumeric() || c == b'.' || c == b'-'
}

fn is_path(c: u8) -> bool {
    c.is_ascii_alphanumeric() || b"._~:/?#@&+=%-".contains(&c)
}

fn contains(hay: &[u8], needle: &[u8]) -> bool {
    hay.windows(needle.len()).any(|w| w == needle)
}

/// First `https://[host].sentry.io/<path>issues/<path>` URL, trailing
/// `.,;:!` stripped.
pub fn sentry_url(text: &str) -> Option<String> {
    let b = text.as_bytes();
    let pre = b"https://";
    for i in 0..b.len() {
        if !b[i..].starts_with(pre) {
            continue;
        }
        let hs = i + pre.len();
        let mut he = hs;
        while he < b.len() && is_host(b[he]) {
            he += 1;
        }
        if he < b.len() && b[he] == b'/' && he - hs > 10 && b[hs..he].ends_with(b".sentry.io") {
            let ps = he + 1;
            let mut pe = ps;
            while pe < b.len() && is_path(b[pe]) {
                pe += 1;
            }
            if contains(&b[ps..pe], b"issues/") {
                let s = String::from_utf8_lossy(&b[i..pe]);
                return Some(s.trim_end_matches(['.', ',', ';', ':', '!']).to_string());
            }
        }
    }
    None
}

/// All `https?://host/(issue|browse)/KEY-123` URLs, deduplicated, in order.
pub fn tracker_urls(text: &str) -> Vec<String> {
    let b = text.as_bytes();
    let mut out: Vec<String> = Vec::new();
    let mut i = 0;
    while i < b.len() {
        let hs = if b[i..].starts_with(b"https://") {
            i + 8
        } else if b[i..].starts_with(b"http://") {
            i + 7
        } else {
            i += 1;
            continue;
        };
        match tracker_end(b, hs) {
            Some(end) => {
                let u = String::from_utf8_lossy(&b[i..end]).to_string();
                if !out.contains(&u) {
                    out.push(u);
                }
                i = end;
            }
            None => i += 1,
        }
    }
    out
}

fn tracker_end(b: &[u8], hs: usize) -> Option<usize> {
    let mut he = hs;
    while he < b.len() && is_host(b[he]) {
        he += 1;
    }
    if he == hs || he >= b.len() || b[he] != b'/' {
        return None;
    }
    let rest = &b[he + 1..];
    let kind = if rest.starts_with(b"issue/") {
        6
    } else if rest.starts_with(b"browse/") {
        7
    } else {
        return None;
    };
    let mut p = he + 1 + kind;
    if p >= b.len() || !b[p].is_ascii_alphabetic() {
        return None;
    }
    while p < b.len() && b[p].is_ascii_alphanumeric() {
        p += 1;
    }
    if p >= b.len() || b[p] != b'-' {
        return None;
    }
    p += 1;
    let ds = p;
    while p < b.len() && b[p].is_ascii_digit() {
        p += 1;
    }
    (p > ds).then_some(p)
}

/// `^[-*[:space:]]*(kw1|kw2)[^:]*:` case-insensitive.
fn kw_line(line: &str, kws: &[&str]) -> bool {
    let t = line.trim_start_matches(|c: char| c == '-' || c == '*' || is_space(c));
    let l = t.to_ascii_lowercase();
    kws.iter()
        .any(|k| l.starts_with(k) && l[k.len()..].contains(':'))
}

const TOKENS: [&str; 3] = ["bug_candidate", "ignore_candidate", "needs_investigation"];

fn first_token<'a>(lines: impl Iterator<Item = &'a str>) -> Option<&'static str> {
    for line in lines {
        let best = TOKENS
            .iter()
            .filter_map(|t| line.find(t).map(|p| (p, *t)))
            .min_by_key(|(p, _)| *p);
        if let Some((_, t)) = best {
            return Some(t);
        }
    }
    None
}

/// Verdict from legacy classification markers; `unknown` when none.
pub fn verdict(text: &str) -> &'static str {
    let cls = first_token(
        text.split('\n')
            .filter(|l| kw_line(l, &["classification", "verdict"])),
    )
    .or_else(|| first_token(text.split('\n')));
    match cls {
        Some("bug_candidate") => "bug",
        Some("ignore_candidate") => "expected",
        Some("needs_investigation") => "investigate",
        _ => "unknown",
    }
}

/// Confidence from the first `Confidence:` line; empty when none.
pub fn confidence(text: &str) -> &'static str {
    match text.split('\n').find(|l| kw_line(l, &["confidence"])) {
        Some(l) => {
            let l = l.to_ascii_lowercase();
            if l.contains("high") {
                "high"
            } else if l.contains("medium") {
                "medium"
            } else if l.contains("low") {
                "low"
            } else {
                ""
            }
        }
        None => "",
    }
}

/// `^[A-Za-z0-9][A-Za-z0-9_]*(-[A-Za-z0-9_]+)+$`
pub fn issue_dir_name(name: &str) -> bool {
    let w = |c: char| c.is_ascii_alphanumeric() || c == '_';
    let mut parts = name.split('-');
    let first = parts.next().unwrap_or("");
    let ok_first = first
        .chars()
        .next()
        .is_some_and(|c| c.is_ascii_alphanumeric())
        && first.chars().all(w);
    let mut n = 0;
    for p in parts {
        if p.is_empty() || !p.chars().all(w) {
            return false;
        }
        n += 1;
    }
    ok_first && n > 0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sentry_urls() {
        assert_eq!(
            sentry_url("- Sentry URL: https://acme.sentry.io/issues/PROD-AB.\n").as_deref(),
            Some("https://acme.sentry.io/issues/PROD-AB")
        );
        assert_eq!(
            sentry_url("https://sentry.io/issues/1 https://a.sentry.io/organizations/x/issues/2/")
                .as_deref(),
            Some("https://a.sentry.io/organizations/x/issues/2/")
        );
        assert_eq!(sentry_url("http://a.sentry.io/issues/1"), None);
        assert_eq!(sentry_url("https://a.sentry.io/other/1"), None);
    }

    #[test]
    fn tracker_links() {
        let t = "x https://t.example.com/issue/ACME-123 y http://j.example.com/browse/AB-9 \
                 https://t.example.com/issue/ACME-123 https://t.example.com/issue/9-1";
        assert_eq!(
            tracker_urls(t),
            [
                "https://t.example.com/issue/ACME-123",
                "http://j.example.com/browse/AB-9"
            ]
        );
    }

    #[test]
    fn classification() {
        assert_eq!(verdict("- Classification: bug_candidate\n"), "bug");
        assert_eq!(verdict("note ignore_candidate\n"), "expected");
        assert_eq!(
            verdict("see bug_candidate\nVerdict: needs_investigation"),
            "investigate"
        );
        assert_eq!(verdict("nothing"), "unknown");
        assert_eq!(confidence("- Confidence: High\n"), "high");
        assert_eq!(confidence("Confidence (est): MEDIUM-low"), "medium");
        assert_eq!(confidence("confidence"), "");
    }

    #[test]
    fn dir_names() {
        assert!(issue_dir_name("CORE-1"));
        assert!(issue_dir_name("DATA_STORE-1"));
        assert!(issue_dir_name("A-B-C"));
        assert!(!issue_dir_name("notes"));
        assert!(!issue_dir_name("_A-1"));
        assert!(!issue_dir_name("A--1"));
        assert!(!issue_dir_name("A-"));
    }
}
