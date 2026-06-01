//! Notification track Tauri commands (P5a).
//!
//! Thin wrappers over [`super::NotifyState`]: ingest OSC notifications, expose
//! ring/unread state, clear on focus, and build byte-exact OSC sequences (the
//! spec handed to P13's `weft notify`). Commands are registered once in
//! `command_registry` (last-wins `invoke_handler` constraint).
//!
//! Event surface for P5b (Wave-3): every state change emits
//! [`NOTIFICATION_CHANGED_EVENT`] with a [`manager::PaneRingState`] payload. The
//! pane renderer + sidebar subscribe to draw/clear the ring; `notify_clear` is the
//! focus/click hook. Errors cross IPC as `String` (rejected promise).

use serde::Serialize;
use tauri::{AppHandle, Emitter, Runtime, State};

use super::manager::{Notification, PaneRingState};
use super::osc::{self, parse_notification};
use super::scanner::RawOsc;
use super::NotifyState;

/// Event name the frontend (P5b) listens on for ring/highlight updates.
pub const NOTIFICATION_CHANGED_EVENT: &str = "notification-changed";

/// Emit the pane's current ring state so subscribers (P5b) can redraw. Best
/// effort: a failed emit must not fail the command.
fn emit_changed<R: Runtime>(app: &AppHandle<R>, state: PaneRingState) {
    let _ = app.emit(NOTIFICATION_CHANGED_EVENT, state);
}

/// Ingest an OSC whose framing xterm.js already stripped (the production path via
/// `parser.registerOscHandler`). `code` is 9 / 99 / 777; `data` is the payload
/// after `<code>;`. Returns the stored notification, or `None` if the payload
/// carried nothing displayable (then no event is emitted).
#[tauri::command]
pub fn notify_ingest_osc<R: Runtime>(
    app: AppHandle<R>,
    state: State<'_, NotifyState>,
    pane_id: String,
    code: u16,
    data: String,
) -> Option<Notification> {
    let parsed = parse_notification(&RawOsc { code, data })?;
    let notification = state.manager.record(pane_id.clone(), parsed);
    emit_changed(&app, state.manager.ring_state(&pane_id));
    Some(notification)
}

/// Ingest raw PTY bytes for `pane_id` (backend/SSH tap). A per-pane scanner pulls
/// OSC framing out — sequences may straddle batches — and every recovered
/// notification is recorded. Returns the notifications stored from this chunk.
#[tauri::command]
pub fn notify_ingest_bytes<R: Runtime>(
    app: AppHandle<R>,
    state: State<'_, NotifyState>,
    pane_id: String,
    bytes: Vec<u8>,
) -> Vec<Notification> {
    let oscs = state.scan(&pane_id, &bytes);
    let mut recorded = Vec::new();
    for osc in &oscs {
        if let Some(parsed) = parse_notification(osc) {
            recorded.push(state.manager.record(pane_id.clone(), parsed));
        }
    }
    if !recorded.is_empty() {
        emit_changed(&app, state.manager.ring_state(&pane_id));
    }
    recorded
}

/// The pane's ring/unread snapshot (P5b polls this on mount / reads event payloads).
#[tauri::command]
pub fn notify_pane_state(state: State<'_, NotifyState>, pane_id: String) -> PaneRingState {
    state.manager.ring_state(&pane_id)
}

/// Global unread count (number of panes with an unread notification) — drives the
/// app/dock badge.
#[tauri::command]
pub fn notify_unread_count(state: State<'_, NotifyState>) -> usize {
    state.manager.unread_count()
}

/// All current notifications, newest first (notifications panel).
#[tauri::command]
pub fn notify_list(state: State<'_, NotifyState>) -> Vec<Notification> {
    state.manager.snapshot()
}

/// Mark the pane's notification read (ring off, keep history). Emits a state change.
#[tauri::command]
pub fn notify_mark_read<R: Runtime>(
    app: AppHandle<R>,
    state: State<'_, NotifyState>,
    pane_id: String,
) -> bool {
    let changed = state.manager.mark_read(&pane_id);
    if changed {
        emit_changed(&app, state.manager.ring_state(&pane_id));
    }
    changed
}

/// Clear the pane's notification (focus/click hook — turns the ring off). Emits a
/// state change so P5b clears the highlight.
#[tauri::command]
pub fn notify_clear<R: Runtime>(
    app: AppHandle<R>,
    state: State<'_, NotifyState>,
    pane_id: String,
) -> bool {
    let changed = state.manager.clear(&pane_id);
    if changed {
        emit_changed(&app, state.manager.ring_state(&pane_id));
    }
    changed
}

/// Which OSC flavor to build for `notify_build_osc` / `weft notify` (P13).
#[derive(Debug, Clone, Copy, Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum OscFlavor {
    /// OSC 9 (iTerm2): body only.
    Osc9,
    /// OSC 777 (rxvt): title + body.
    Osc777,
    /// OSC 99 (kitty): single field (defaults to body when title is empty).
    Osc99,
}

/// Build a byte-exact OSC notification sequence. This is the shared format P13's
/// `weft notify` subcommand emits; exposing it as a command lets the frontend (or
/// a test harness) generate sequences without a separate codepath.
#[tauri::command]
pub fn notify_build_osc(flavor: OscFlavor, title: String, body: String) -> Vec<u8> {
    match flavor {
        OscFlavor::Osc9 => osc::build_osc9(&body),
        OscFlavor::Osc777 => osc::build_osc777(&title, &body),
        OscFlavor::Osc99 => {
            if title.is_empty() {
                osc::build_osc99("body", &body)
            } else {
                osc::build_osc99("title", &title)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // The command bodies delegate to the manager/osc modules (covered by their
    // own unit tests). Here we lock the pure command-shaped helpers that do not
    // need an AppHandle: parse-to-record wiring and the build_osc dispatch.

    #[test]
    fn build_osc_dispatch_matches_flavor() {
        assert_eq!(
            notify_build_osc(OscFlavor::Osc9, String::new(), "b".into()),
            b"\x1b]9;b\x07"
        );
        assert_eq!(
            notify_build_osc(OscFlavor::Osc777, "t".into(), "b".into()),
            b"\x1b]777;notify;t;b\x07"
        );
        assert_eq!(
            notify_build_osc(OscFlavor::Osc99, "t".into(), "b".into()),
            b"\x1b]99;p=title;t\x1b\\"
        );
        // Empty title → OSC 99 falls back to the body field.
        assert_eq!(
            notify_build_osc(OscFlavor::Osc99, String::new(), "b".into()),
            b"\x1b]99;p=body;b\x1b\\"
        );
    }

    #[test]
    fn ingest_then_state_round_trips_through_manager() {
        // Manager-level wiring without a Tauri AppHandle: parse + record + read.
        let state = NotifyState::new();
        let parsed = parse_notification(&RawOsc {
            code: 777,
            data: "notify;Title;Body".into(),
        })
        .unwrap();
        state.manager.record("pane-1".into(), parsed);
        let ring = state.manager.ring_state("pane-1");
        assert!(ring.has_ring);
        assert_eq!(ring.latest.unwrap().title, "Title");
        assert_eq!(state.manager.unread_count(), 1);
    }

    #[test]
    fn scan_path_recovers_split_sequence() {
        let state = NotifyState::new();
        assert!(state.scan("pane-1", b"\x1b]9;par").is_empty());
        let oscs = state.scan("pane-1", b"tial\x07");
        assert_eq!(oscs.len(), 1);
        assert_eq!(oscs[0].data, "partial");
    }
}
