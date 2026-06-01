//! Command palette search track (P16 owner: `src-tauri/src/palette/**`,
//! `src/command-palette/**`).
//!
//! Fuzzy ranking for the command palette via the `nucleo` crate IN-PROCESS (the
//! cmux FFI/cdylib dance was a Swift-can't-call-Rust artifact — irrelevant here).
//! Exposed as a Tauri command `palette_search(query, corpus, boosts) → ranked ids
//! + matched indices` (see [`nucleo_search`]). weft.json command defs + the trust
//! gate are owned by P12; this track reads them (TS layer calls `weft_defs_get` /
//! `weft_trust_check` / `weft_trust_grant`). The Rust side here is pure fuzzy.
//!
//! `register` is additive-only (no `invoke_handler` — commands are listed once in
//! `command_registry` per the last-wins constraint). The command to add to
//! `generate_handler!` is `palette::nucleo_search::palette_search`.

pub mod nucleo_search;

use tauri::{Builder, Runtime};

/// Additive setup placeholder. The search command needs no managed state (it is a
/// pure function over its arguments), so this stays a no-op fold.
pub fn register<R: Runtime>(builder: Builder<R>) -> Builder<R> {
    builder
}
