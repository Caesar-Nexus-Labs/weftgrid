//! Shared DTOs for the browser-import track (P11a core).
//!
//! These are the portable contract every import sub-module (catalog/detect,
//! cookies, history, dedupe) and the Tauri command surface speak. They are also
//! the "decrypted cookies/history" output surface P11b (Wave-3) seeds into a P6
//! profile.
//!
//! Security: a cookie `value` is a credential. [`ImportedCookie`] has a custom
//! `Debug` that REDACTS the value so it never leaks into logs/error chains.

use std::fmt;

use serde::{Deserialize, Serialize};

/// Browser engine family — selects the decrypt + history-read strategy.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum BrowserFamily {
    /// Chrome/Edge/Brave/Arc/… — encrypted `Cookies` SQLite + `History`.
    Chromium,
    /// Firefox/Zen/LibreWolf/Floorp/Waterfox — plaintext `cookies.sqlite` + `places.sqlite`.
    Firefox,
    /// Safari/Orion/Ladybird — cookie import NOT supported (binarycookies); history only.
    Webkit,
}

/// One detected, installed browser the user may import from.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BrowserInfo {
    /// Stable catalog id (e.g. `"chrome"`, `"firefox"`, `"floorp"`).
    pub id: String,
    pub display_name: String,
    pub family: BrowserFamily,
    /// 1 = most common. Drives picker ordering (cmux parity).
    pub tier: u8,
    /// Absolute path to the cookie store, if found (chromium `Cookies` / firefox `cookies.sqlite`).
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub cookie_db: Option<String>,
    /// Absolute path to the chromium key store (`Local State`), if found.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub key_store: Option<String>,
    /// Absolute path to the history DB, if found (chromium `History` / firefox `places.sqlite`).
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub history_db: Option<String>,
}

/// A single decrypted cookie. `value` is sensitive — see custom `Debug`.
#[derive(Clone, PartialEq, Serialize, Deserialize)]
pub struct ImportedCookie {
    pub name: String,
    pub domain: String,
    pub path: String,
    /// Decrypted cookie value — A SECRET. Never logged (Debug redacts).
    pub value: String,
    pub secure: bool,
    pub http_only: bool,
    pub same_site: i64,
    /// Expiry as Unix epoch seconds; `None` = session cookie.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub expires: Option<u64>,
}

impl fmt::Debug for ImportedCookie {
    /// Redacts `value` so cookie secrets never reach logs/panics/error chains.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ImportedCookie")
            .field("name", &self.name)
            .field("domain", &self.domain)
            .field("path", &self.path)
            .field("value", &"<redacted>")
            .field("secure", &self.secure)
            .field("http_only", &self.http_only)
            .field("same_site", &self.same_site)
            .field("expires", &self.expires)
            .finish()
    }
}

/// One browsing-history record (plaintext, low-risk).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HistoryEntry {
    pub url: String,
    pub title: String,
    pub visit_count: i64,
    /// Last visit time normalized to Unix epoch milliseconds across engines.
    pub last_visit_ms: i64,
}

/// Why a browser's cookies were skipped instead of imported. No secrets.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CookieSkipReason {
    /// Chrome v20 app-bound encryption — DELIBERATELY skipped. NOT bypassed, NO
    /// admin elevation (that is infostealer behavior; see phase H-import).
    AppBound,
    /// OS keyring/keychain locked or unavailable (Linux libsecret/kwallet).
    KeyringLocked,
    /// Cookie store missing / unreadable.
    Unavailable,
    /// Decryption failed for another reason.
    DecryptFailed,
}

impl CookieSkipReason {
    /// User-facing, secret-free explanation for the consent/import UI.
    pub fn message(self) -> &'static str {
        match self {
            CookieSkipReason::AppBound => {
                "Cookies use Chrome v20 app-bound encryption and were skipped (not bypassed)."
            }
            CookieSkipReason::KeyringLocked => {
                "OS keyring is locked or unavailable; unlock it and retry."
            }
            CookieSkipReason::Unavailable => "Cookie store not found or unreadable.",
            CookieSkipReason::DecryptFailed => "Could not decrypt cookies.",
        }
    }
}

/// Outcome of a per-browser cookie import: the decrypted cookies plus, when some
/// were skipped, a secret-free warning + reason. This is a P11b seed-input.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CookieImportResult {
    pub browser_id: String,
    pub cookies: Vec<ImportedCookie>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub warning: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub skipped_reason: Option<CookieSkipReason>,
}

impl CookieImportResult {
    /// A clean success carrying decrypted cookies.
    pub fn imported(browser_id: impl Into<String>, cookies: Vec<ImportedCookie>) -> Self {
        CookieImportResult {
            browser_id: browser_id.into(),
            cookies,
            warning: None,
            skipped_reason: None,
        }
    }

    /// A skip-with-warning result (no cookies). Carries a secret-free message.
    pub fn skipped(browser_id: impl Into<String>, reason: CookieSkipReason) -> Self {
        CookieImportResult {
            browser_id: browser_id.into(),
            cookies: Vec::new(),
            warning: Some(reason.message().to_string()),
            skipped_reason: Some(reason),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cookie_debug_redacts_secret_value() {
        let c = ImportedCookie {
            name: "sid".into(),
            domain: "example.com".into(),
            path: "/".into(),
            value: "super-secret-token".into(),
            secure: true,
            http_only: true,
            same_site: 1,
            expires: Some(123),
        };
        let dbg = format!("{c:?}");
        assert!(dbg.contains("<redacted>"));
        assert!(!dbg.contains("super-secret-token"));
        // Non-secret fields stay visible for diagnostics.
        assert!(dbg.contains("example.com"));
    }

    #[test]
    fn skipped_result_carries_reason_and_message_no_cookies() {
        let r = CookieImportResult::skipped("chrome", CookieSkipReason::AppBound);
        assert!(r.cookies.is_empty());
        assert_eq!(r.skipped_reason, Some(CookieSkipReason::AppBound));
        assert!(r.warning.unwrap().contains("app-bound"));
    }

    #[test]
    fn imported_result_has_no_warning() {
        let r = CookieImportResult::imported("firefox", Vec::new());
        assert!(r.warning.is_none());
        assert!(r.skipped_reason.is_none());
    }
}
