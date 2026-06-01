//! Notification (OSC) track (P5a owner: `src-tauri/src/notify/**`).
//!
//! Wave-1 **core** (no UI dependency): parse OSC 9 / 99 / 777 from a terminal
//! byte stream into structured notifications, keyed per pane, with replace-dedup
//! and unread/ring state. Hard fork pinned to cmux SHA
//! `c4911439e3e99784bd5d6379096f315034a5259c` (see `osc.rs`).
//!
//! Two ingest paths feed the same [`manager::NotificationManager`]:
//!   - **`notify_ingest_osc`** — xterm.js `parser.registerOscHandler` already
//!     strips OSC framing, so the TS side passes `(paneId, code, data)`. This is
//!     the phase's primary production path.
//!   - **`notify_ingest_bytes`** — a backend tap can hand raw PTY bytes straight
//!     in; a per-pane [`scanner::OscScanner`] extracts framing (sequences may
//!     straddle coalesced batches). Used by Rust-side / SSH ingestion.
//!
//! P5b (Wave-3) consumes the `notification-changed` event (payload:
//! [`manager::PaneRingState`]) to draw the pane ring + sidebar highlight, and
//! calls `notify_clear` on focus/click to turn the ring off. P13 owns the `weft
//! notify` CLI and calls the byte-exact builders in [`osc`].

use std::collections::HashMap;
use std::sync::Mutex;

use tauri::{Builder, Runtime};

pub mod commands;
pub mod manager;
pub mod osc;
pub mod scanner;

use manager::{NotificationManager, PaneKey};
use scanner::OscScanner;

/// `.manage()`d notification state: the pane-keyed store plus per-pane stream
/// scanners (only the raw-bytes ingest path uses the scanners).
#[derive(Default)]
pub struct NotifyState {
    pub manager: NotificationManager,
    scanners: Mutex<HashMap<PaneKey, OscScanner>>,
}

impl NotifyState {
    pub fn new() -> Self {
        Self::default()
    }

    /// Feed raw stream bytes for `pane` through that pane's scanner, returning the
    /// OSCs that completed. Scanner state persists across calls so a sequence
    /// split over two batches still parses.
    pub fn scan(&self, pane: &str, bytes: &[u8]) -> Vec<scanner::RawOsc> {
        let mut scanners = self.scanners.lock().unwrap();
        scanners.entry(pane.to_string()).or_default().push(bytes)
    }
}

/// Additive setup: register the `.manage()`d [`NotifyState`]. No `invoke_handler`
/// — commands are listed once in `command_registry` (last-wins constraint).
pub fn register<R: Runtime>(builder: Builder<R>) -> Builder<R> {
    builder.manage(NotifyState::new())
}
