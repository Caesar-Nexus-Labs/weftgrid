//! Browser import track (P11a core owner: `src-tauri/src/import/**`).
//!
//! Wave-1 **core** (no UI/pane dependency): enumerate installed browsers, decrypt
//! their cookies via `rookie` (skip+warn on Chrome v20 app-bound — never bypass),
//! read history via `rusqlite`, dedupe + domain-filter. Every read is gated on the
//! P12 consent flag (`Config.import_consent`).
//!
//! Module map:
//! - [`types`] — shared DTOs (`BrowserInfo`, `ImportedCookie`, `HistoryEntry`,
//!   `CookieImportResult`). `ImportedCookie`'s `Debug` redacts the value.
//! - [`catalog`] — 22-browser table + installed-browser detection + path resolve.
//! - [`cookies`] — `rookie` wrapper; v20 → skip+warn, graceful locked-keyring.
//! - [`history`] — read-only `rusqlite` SELECT of `urls` / `moz_places`.
//! - [`dedupe`] — `name|domain|path` dedupe (latest expiry) + domain filter.
//! - [`consent`] — pure gate; the command layer feeds it `import_consent`.
//! - [`commands`] — Tauri surface (`import_list_browsers`, `import_cookies`, `import_history`).
//!
//! ## Output surface for P11b (Wave-3) seeding
//! `import_cookies` returns a [`types::CookieImportResult`] carrying the decrypted,
//! deduped [`types::ImportedCookie`]s (HttpOnly/SameSite preserved) plus a
//! secret-free per-browser warning when cookies were skipped. P11b consumes that
//! cookie list to seed a P6 profile (Win: CDP `Network.setCookie`; Linux:
//! WebKitWebsiteDataManager) at profile CREATION time. P11a does NOT seed.
//!
//! No `invoke_handler` here — commands are listed once in `command_registry`
//! (last-wins constraint). `register` is additive (no state of its own; consent
//! lives in the config track's `ConfigState`).

use tauri::{Builder, Runtime};

pub mod catalog;
pub mod commands;
pub mod consent;
pub mod cookies;
pub mod dedupe;
pub mod history;
pub mod types;

/// Additive setup. The import track holds no `.manage()`d state of its own — the
/// consent flag it gates on lives in the config track's `ConfigState`.
pub fn register<R: Runtime>(builder: Builder<R>) -> Builder<R> {
    builder
}
