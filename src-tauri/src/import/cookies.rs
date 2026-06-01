//! Cookie decryption via `rookie` (P11a core).
//!
//! `rookie` does the cross-platform decrypt (Windows DPAPI, Linux libsecret/kwallet,
//! macOS Keychain) for chromium `v10`/`v11` and firefox plaintext cookies. This
//! module is a thin wrapper that:
//!   - dispatches a catalog [`BrowserInfo`] to the right `rookie` call,
//!   - maps `rookie::Cookie` → our [`ImportedCookie`],
//!   - classifies any failure into a secret-free [`CookieSkipReason`] → **skip + warn**.
//!
//! ## Chrome v20 app-bound encryption — SKIPPED, NEVER bypassed
//! v20 cookies are sealed with app-bound encryption. Bypassing it is exactly an
//! infostealer signature, needs admin elevation, and is a maintenance treadmill
//! (phase H-import) — so we DO NOT. `rookie` must be built WITHOUT its `appbound`
//! feature (see report: `rookie = { default-features = false }`); then v20
//! ciphertext simply fails to decrypt with the legacy key and surfaces as an
//! error we classify as [`CookieSkipReason::AppBound`]. No elevation, no bypass.
//!
//! ## Live vs. unit-testable
//! Real decryption touches the user's actual browser stores / OS keyring and is
//! not CI-reproducible, so [`import_cookies`] is gated behind the
//! `WEFTGRID_IMPORT_LIVE` env var (off in CI). The pure error→reason
//! classification and the rookie→DTO mapping are unit-tested with fixtures.

use super::types::{
    BrowserFamily, BrowserInfo, CookieImportResult, CookieSkipReason, ImportedCookie,
};

/// Env var that must be set (to any value) to permit live cookie decryption.
/// Off in CI so tests never read the developer's real browser stores.
pub const LIVE_ENV: &str = "WEFTGRID_IMPORT_LIVE";

fn live_enabled() -> bool {
    std::env::var_os(LIVE_ENV).is_some()
}

/// Map one `rookie::Cookie` to our DTO. Normalizes nothing else — `rookie` already
/// gives epoch-seconds expiry (`None` = session).
fn map_cookie(c: rookie::enums::Cookie) -> ImportedCookie {
    ImportedCookie {
        name: c.name,
        domain: c.domain,
        path: c.path,
        value: c.value,
        secure: c.secure,
        http_only: c.http_only,
        same_site: c.same_site,
        expires: c.expires,
    }
}

/// Classify a decrypt failure into a secret-free skip reason. We inspect the
/// error's text (rookie uses `eyre` string errors) for known signatures; anything
/// unrecognized is a generic decrypt failure. NEVER includes cookie data.
pub fn classify_error(msg: &str) -> CookieSkipReason {
    let m = msg.to_lowercase();
    if m.contains("appbound")
        || m.contains("app-bound")
        || m.contains("app_bound")
        || m.contains("v20")
    {
        CookieSkipReason::AppBound
    } else if m.contains("keyring")
        || m.contains("keychain")
        || m.contains("kwallet")
        || m.contains("libsecret")
        || m.contains("locked")
        || m.contains("secret service")
    {
        CookieSkipReason::KeyringLocked
    } else if m.contains("can't find")
        || m.contains("not found")
        || m.contains("no such file")
        || m.contains("no cookies")
    {
        CookieSkipReason::Unavailable
    } else {
        CookieSkipReason::DecryptFailed
    }
}

/// Decrypt cookies for one detected browser, scoped to `domains` (empty = all).
///
/// Returns either the decrypted cookies or a skip-with-warning result (e.g. v20
/// app-bound, locked keyring) — it never errors for an expected skip, so the UI
/// can show a per-browser warning and continue. Gated by [`LIVE_ENV`]: without it
/// set, returns an `Unavailable` skip (so CI never reads real stores).
pub fn import_cookies(browser: &BrowserInfo, domains: &[String]) -> CookieImportResult {
    if !live_enabled() {
        return CookieImportResult::skipped(&browser.id, CookieSkipReason::Unavailable);
    }
    // Webkit (Safari/Orion/Ladybird) cookie stores are a binary format rookie
    // does not decrypt on this path → explicit skip+warn (cmux parity).
    if browser.family == BrowserFamily::Webkit {
        return CookieImportResult::skipped(&browser.id, CookieSkipReason::Unavailable);
    }
    // No resolved cookie store → nothing to read (skip+warn, never guess paths).
    let Some(cookie_db) = browser.cookie_db.as_deref() else {
        return CookieImportResult::skipped(&browser.id, CookieSkipReason::Unavailable);
    };
    let domain_arg = if domains.is_empty() {
        None
    } else {
        Some(domains.to_vec())
    };
    // `rookie::any_browser` picks the right decrypt by trying each family — keeping
    // the call path-driven (not per-browser fns) lets forks reuse one codepath.
    match rookie::any_browser(cookie_db, domain_arg, browser.key_store.as_deref()) {
        Ok(raw) => {
            let cookies = raw.into_iter().map(map_cookie).collect();
            CookieImportResult::imported(&browser.id, cookies)
        }
        Err(e) => CookieImportResult::skipped(&browser.id, classify_error(&e.to_string())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn webkit_browser() -> BrowserInfo {
        BrowserInfo {
            id: "safari".into(),
            display_name: "Safari".into(),
            family: BrowserFamily::Webkit,
            tier: 1,
            cookie_db: Some("/x/Cookies.binarycookies".into()),
            key_store: None,
            history_db: None,
        }
    }

    #[test]
    fn classifies_app_bound_v20_as_skip_not_bypass() {
        // Any v20/app-bound signature → AppBound skip. We assert we DO NOT escalate.
        assert_eq!(
            classify_error("Chrome cookies from version v130 ... appbound encryption"),
            CookieSkipReason::AppBound
        );
        assert_eq!(
            classify_error("v20 key type unsupported"),
            CookieSkipReason::AppBound
        );
    }

    #[test]
    fn classifies_locked_keyring() {
        assert_eq!(
            classify_error("failed to talk to org.freedesktop.secrets: keyring is locked"),
            CookieSkipReason::KeyringLocked
        );
        assert_eq!(
            classify_error("kwallet not available"),
            CookieSkipReason::KeyringLocked
        );
    }

    #[test]
    fn classifies_missing_store_as_unavailable() {
        assert_eq!(
            classify_error("can't find cookies file"),
            CookieSkipReason::Unavailable
        );
        assert_eq!(
            classify_error("No cookies found."),
            CookieSkipReason::Unavailable
        );
    }

    #[test]
    fn classifies_unknown_as_generic_decrypt_failure() {
        assert_eq!(
            classify_error("totally weird error"),
            CookieSkipReason::DecryptFailed
        );
    }

    #[test]
    fn maps_rookie_cookie_to_dto_preserving_fields() {
        let raw = rookie::enums::Cookie {
            domain: "example.com".into(),
            path: "/".into(),
            secure: true,
            expires: Some(42),
            name: "sid".into(),
            value: "secret".into(),
            http_only: true,
            same_site: 1,
        };
        let mapped = map_cookie(raw);
        assert_eq!(mapped.name, "sid");
        assert_eq!(mapped.domain, "example.com");
        assert!(mapped.http_only);
        assert_eq!(mapped.expires, Some(42));
    }

    #[test]
    fn skips_when_live_env_unset() {
        // CI default: no live env → Unavailable skip, no store read attempted.
        std::env::remove_var(LIVE_ENV);
        let r = import_cookies(&webkit_browser(), &[]);
        assert_eq!(r.skipped_reason, Some(CookieSkipReason::Unavailable));
        assert!(r.cookies.is_empty());
    }

    #[test]
    fn webkit_is_skipped_even_when_live() {
        // Safari/Orion/Ladybird binarycookies are unsupported → skip+warn (parity).
        std::env::set_var(LIVE_ENV, "1");
        let r = import_cookies(&webkit_browser(), &[]);
        std::env::remove_var(LIVE_ENV);
        assert_eq!(r.skipped_reason, Some(CookieSkipReason::Unavailable));
    }
}
