//! Cookie dedupe + domain filter (P11a core).
//!
//! Pure transforms over `Vec<ImportedCookie>`, ported from cmux's `dedupeCookies`
//! (key = `name|domain|path`, keep the entry with the **latest expiry**) and its
//! `domainFilters` scoping. No I/O — fully unit-testable.

use std::collections::HashMap;

use super::types::ImportedCookie;

/// Dedupe key: cmux parity = `name|domain|path` (an exact triple, not substring).
fn dedupe_key(c: &ImportedCookie) -> (String, String, String) {
    (c.name.clone(), c.domain.clone(), c.path.clone())
}

/// Collapse duplicate cookies, keeping the one with the latest expiry per
/// `name|domain|path`. A `None` (session) expiry sorts below any concrete expiry;
/// a later concrete expiry wins. Insertion order of survivors is preserved.
pub fn dedupe(cookies: Vec<ImportedCookie>) -> Vec<ImportedCookie> {
    // index into `out` per key, so survivors keep first-seen order.
    let mut index: HashMap<(String, String, String), usize> = HashMap::new();
    let mut out: Vec<ImportedCookie> = Vec::with_capacity(cookies.len());

    for cookie in cookies {
        let key = dedupe_key(&cookie);
        match index.get(&key) {
            Some(&pos) => {
                // Replace only if the newcomer has a strictly later expiry.
                // `None` (session) is treated as the earliest, so a concrete
                // expiry always supersedes it; between two concretes the larger wins.
                let existing = expiry_rank(&out[pos]);
                let incoming = expiry_rank(&cookie);
                if incoming > existing {
                    out[pos] = cookie;
                }
            }
            None => {
                index.insert(key, out.len());
                out.push(cookie);
            }
        }
    }
    out
}

/// Order session (`None`) below any concrete expiry for "latest wins".
fn expiry_rank(c: &ImportedCookie) -> i128 {
    match c.expires {
        // shift so even expiry 0 outranks a session cookie.
        Some(e) => e as i128,
        None => -1,
    }
}

/// Keep only cookies whose `domain` matches one of `domains` (case-insensitive
/// substring, cmux `host_key LIKE '%domain%'` parity). Empty filter = keep all.
pub fn filter_domains(cookies: Vec<ImportedCookie>, domains: &[String]) -> Vec<ImportedCookie> {
    if domains.is_empty() {
        return cookies;
    }
    let needles: Vec<String> = domains.iter().map(|d| d.to_lowercase()).collect();
    cookies
        .into_iter()
        .filter(|c| {
            let host = c.domain.to_lowercase();
            needles.iter().any(|n| host.contains(n))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cookie(name: &str, domain: &str, path: &str, expires: Option<u64>) -> ImportedCookie {
        ImportedCookie {
            name: name.into(),
            domain: domain.into(),
            path: path.into(),
            value: "v".into(),
            secure: false,
            http_only: false,
            same_site: 0,
            expires,
        }
    }

    #[test]
    fn dedupe_collapses_same_name_domain_path_keeping_latest_expiry() {
        let input = vec![
            cookie("sid", "example.com", "/", Some(100)),
            cookie("sid", "example.com", "/", Some(500)), // later expiry wins
            cookie("sid", "example.com", "/", Some(300)),
        ];
        let out = dedupe(input);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].expires, Some(500));
    }

    #[test]
    fn dedupe_keeps_distinct_keys() {
        let input = vec![
            cookie("a", "example.com", "/", Some(1)),
            cookie("a", "other.com", "/", Some(1)), // diff domain
            cookie("a", "example.com", "/app", Some(1)), // diff path
            cookie("b", "example.com", "/", Some(1)), // diff name
        ];
        assert_eq!(dedupe(input).len(), 4);
    }

    #[test]
    fn dedupe_concrete_expiry_supersedes_session_cookie() {
        let input = vec![
            cookie("sid", "example.com", "/", None),
            cookie("sid", "example.com", "/", Some(0)),
        ];
        let out = dedupe(input);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].expires, Some(0));
    }

    #[test]
    fn dedupe_preserves_first_seen_order_of_survivors() {
        let input = vec![
            cookie("z", "z.com", "/", Some(1)),
            cookie("a", "a.com", "/", Some(1)),
        ];
        let out = dedupe(input);
        assert_eq!(out[0].name, "z");
        assert_eq!(out[1].name, "a");
    }

    #[test]
    fn empty_domain_filter_keeps_all() {
        let input = vec![cookie("a", "example.com", "/", None)];
        assert_eq!(filter_domains(input, &[]).len(), 1);
    }

    #[test]
    fn domain_filter_matches_substring_case_insensitive() {
        let input = vec![
            cookie("a", "login.Example.com", "/", None),
            cookie("b", "other.org", "/", None),
        ];
        let out = filter_domains(input, &["example.com".into()]);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].name, "a");
    }
}
