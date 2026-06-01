//! PTY-death watchdog seam (P14 → P3).
//!
//! P3 detects EOF/exit on a pane's child; P14 decides whether that death warrants a
//! user-facing recovery offer. The pure classification lives in
//! [`super::recovery::pty`] ([`PtyExit`]/[`classify_exit`]); this module turns a
//! classified death into the recovery EVENT the UI consumes — "pane X died
//! unexpectedly, respawn?" — without silently hanging on a blocked read (the
//! researcher §2 gotcha).
//!
//! The event-building is pure + tested. The LIVE seam ([`emit_pane_recovery`]) is
//! the thin part that pushes the event through an injected [`PaneRecoveryNotifier`]
//! (production = a Tauri `Emitter` to the pane UI; tests = a fake collector).
//!
//! LIVE-WIRED WAVE-DEFERRED: observing the actual child exit (P3 already owns the
//! reader thread + `child.wait()`; P14 hooks its non-graceful path) and the real
//! `AppHandle`-backed notifier are accepted at the real-desktop session — they need
//! P3 coordination + a live app.

use serde::Serialize;

use crate::model::PaneId;

use super::recovery::pty::{classify_exit, PtyExit, PtyExitClass};

/// Event payload handed to the pane UI when a PTY dies unexpectedly. `serde` so it
/// can cross the Tauri event bridge unchanged once live-wired.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PaneRecoveryEvent {
    pub pane_id: PaneId,
    /// Human-readable cause for the UI ("process exited on signal 9", etc.).
    pub reason: String,
    /// Whether the UI should show a respawn affordance (always true for an
    /// unexpected death — that's why the event was emitted).
    pub offer_respawn: bool,
}

/// Tauri event name the pane UI listens on for unexpected-death recovery offers.
pub const PANE_RECOVERY_EVENT: &str = "pane-recovery";

/// Build a recovery event for an unexpected death, or `None` if the exit was
/// graceful (no prompt). Pure — the watchdog calls this with the observed exit.
pub fn recovery_event_for(pane: PaneId, exit: PtyExit) -> Option<PaneRecoveryEvent> {
    match classify_exit(exit) {
        PtyExitClass::Graceful => None,
        PtyExitClass::Unexpected => Some(PaneRecoveryEvent {
            pane_id: pane,
            reason: describe_exit(exit),
            offer_respawn: true,
        }),
    }
}

/// Human-readable description of a death cause for the UI/log.
fn describe_exit(exit: PtyExit) -> String {
    match exit {
        PtyExit::Signal(sig) => format!("process terminated by signal {sig}"),
        PtyExit::Code(code) if (129..=159).contains(&code) => {
            format!("process died on signal {} (exit {code})", code - 128)
        }
        PtyExit::Code(code) => format!("process exited unexpectedly (code {code})"),
    }
}

/// Push a pane-recovery event to the UI. Implemented for real by a Tauri `Emitter`;
/// faked in tests.
pub trait PaneRecoveryNotifier {
    fn notify(&self, event: PaneRecoveryEvent);
}

/// Live seam: on an observed exit, build the recovery event (if unexpected) and emit
/// it through `notifier`. Returns whether an event was emitted (so the caller can
/// log graceful vs recovered). Pure over the injected notifier + classifier.
pub fn emit_pane_recovery<N: PaneRecoveryNotifier>(
    notifier: &N,
    pane: PaneId,
    exit: PtyExit,
) -> bool {
    match recovery_event_for(pane, exit) {
        Some(event) => {
            notifier.notify(event);
            true
        }
        None => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use uuid::Uuid;

    #[derive(Default)]
    struct FakeNotifier {
        events: RefCell<Vec<PaneRecoveryEvent>>,
    }
    impl PaneRecoveryNotifier for FakeNotifier {
        fn notify(&self, event: PaneRecoveryEvent) {
            self.events.borrow_mut().push(event);
        }
    }

    #[test]
    fn graceful_exit_emits_nothing() {
        let pane = Uuid::new_v4();
        assert!(recovery_event_for(pane, PtyExit::Code(0)).is_none());
        let n = FakeNotifier::default();
        assert!(!emit_pane_recovery(&n, pane, PtyExit::Code(0)));
        assert!(n.events.borrow().is_empty());
    }

    #[test]
    fn signal_death_emits_respawn_offer() {
        let pane = Uuid::new_v4();
        let n = FakeNotifier::default();
        assert!(emit_pane_recovery(&n, pane, PtyExit::Signal(9)));
        let events = n.events.borrow();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].pane_id, pane);
        assert!(events[0].offer_respawn);
        assert!(events[0].reason.contains("signal 9"));
    }

    #[test]
    fn signal_encoded_code_describes_underlying_signal() {
        let pane = Uuid::new_v4();
        let ev = recovery_event_for(pane, PtyExit::Code(137)).unwrap();
        // 137 - 128 = 9 (SIGKILL).
        assert!(ev.reason.contains("signal 9"), "{}", ev.reason);
    }

    #[test]
    fn plain_nonzero_exit_is_graceful_no_event() {
        let pane = Uuid::new_v4();
        assert!(recovery_event_for(pane, PtyExit::Code(1)).is_none());
    }

    #[test]
    fn event_serializes_camel_case() {
        let pane = Uuid::new_v4();
        let ev = recovery_event_for(pane, PtyExit::Signal(11)).unwrap();
        let json = serde_json::to_string(&ev).unwrap();
        assert!(json.contains("\"paneId\""));
        assert!(json.contains("\"offerRespawn\":true"));
    }
}
