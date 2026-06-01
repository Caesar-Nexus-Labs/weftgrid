//! Overlay-window crash recovery seam (P14 → P6).
//!
//! The browser pane is a borderless child `WebviewWindow` (P6 overlay). If that
//! window is destroyed out from under us (crash, OS close), the pane loses its
//! browser surface. Recovery: recreate the overlay and re-sync its bounds to the
//! anchor leaf.
//!
//! Same split as [`super::webview_recovery`]: the DECISION (idempotency + retry
//! budget) is the pure [`RecoveryGuard`]; the LIVE part — recreate the overlay and
//! re-apply the last known physical bounds — goes through the injected
//! [`OverlayRebuilder`] seam (production = P6 `OverlayManager`; tests = fake). So the
//! recover-once-then-resync ordering is verifiable headless.
//!
//! LIVE-WIRED WAVE-DEFERRED: detecting the overlay's disappearance (a Tauri
//! `WindowEvent::Destroyed` / `CloseRequested` subscription on the overlay label)
//! and constructing the P6-backed rebuilder need a live `AppHandle` — accepted at
//! the real-desktop session.

use crate::model::PaneId;

use super::recovery::{RecoveryDecision, RecoveryGuard};

/// Recreate a pane's overlay window and re-sync its bounds. Implemented for real by
/// the P6 overlay manager; faked in tests.
pub trait OverlayRebuilder {
    /// Recreate the overlay for `pane`; returns the new window label.
    fn recreate_overlay(&self, pane: PaneId) -> Result<String, String>;
    /// Re-apply the pane's last-known bounds to the (freshly created) overlay.
    fn resync_bounds(&self, pane: PaneId) -> Result<(), String>;
}

/// What the overlay-recovery seam did (for logging + test assertions).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OverlayRecoveryOutcome {
    /// Recreated + bounds re-synced; carries the new label.
    Recovered(String),
    /// Duplicate event ignored.
    Skipped,
    /// Retry budget exhausted.
    GaveUp,
    /// Recreate or resync failed; guard reset for a later retry.
    Failed(String),
}

/// Handle one overlay-crash event for `pane`: recreate then re-sync bounds, gated by
/// the guard. Pure over the injected rebuilder + guard.
pub fn handle_overlay_crash<O: OverlayRebuilder>(
    guard: &mut RecoveryGuard,
    rebuilder: &O,
    pane: PaneId,
) -> OverlayRecoveryOutcome {
    match guard.on_failure() {
        RecoveryDecision::Duplicate => OverlayRecoveryOutcome::Skipped,
        RecoveryDecision::GiveUp => OverlayRecoveryOutcome::GaveUp,
        RecoveryDecision::Recover { .. } => match rebuilder.recreate_overlay(pane) {
            Ok(label) => match rebuilder.resync_bounds(pane) {
                Ok(()) => {
                    guard.mark_recovered();
                    OverlayRecoveryOutcome::Recovered(label)
                }
                Err(e) => {
                    guard.mark_failed();
                    OverlayRecoveryOutcome::Failed(format!("resync failed: {e}"))
                }
            },
            Err(e) => {
                guard.mark_failed();
                OverlayRecoveryOutcome::Failed(format!("recreate failed: {e}"))
            }
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use uuid::Uuid;

    #[derive(Default)]
    struct FakeRebuilder {
        recreate_calls: RefCell<u32>,
        resync_calls: RefCell<u32>,
        recreate_fail: bool,
        resync_fail: bool,
    }
    impl OverlayRebuilder for FakeRebuilder {
        fn recreate_overlay(&self, pane: PaneId) -> Result<String, String> {
            *self.recreate_calls.borrow_mut() += 1;
            if self.recreate_fail {
                Err("spawn failed".into())
            } else {
                Ok(format!("browser-{pane}"))
            }
        }
        fn resync_bounds(&self, _pane: PaneId) -> Result<(), String> {
            *self.resync_calls.borrow_mut() += 1;
            if self.resync_fail {
                Err("no bounds".into())
            } else {
                Ok(())
            }
        }
    }

    #[test]
    fn recreates_then_resyncs() {
        let mut g = RecoveryGuard::with_defaults();
        let rb = FakeRebuilder::default();
        let pane = Uuid::new_v4();
        let out = handle_overlay_crash(&mut g, &rb, pane);
        assert_eq!(out, OverlayRecoveryOutcome::Recovered(format!("browser-{pane}")));
        assert_eq!(*rb.recreate_calls.borrow(), 1);
        assert_eq!(*rb.resync_calls.borrow(), 1);
    }

    #[test]
    fn resync_failure_does_not_leave_recovering() {
        let mut g = RecoveryGuard::with_defaults();
        let rb = FakeRebuilder { resync_fail: true, ..Default::default() };
        let pane = Uuid::new_v4();
        let out = handle_overlay_crash(&mut g, &rb, pane);
        assert!(matches!(out, OverlayRecoveryOutcome::Failed(_)));
        assert!(!g.is_recovering());
    }

    #[test]
    fn exhausts_budget_then_gives_up() {
        let mut g = RecoveryGuard::new(super::super::recovery::BackoffSchedule::default(), 2, std::time::Duration::from_secs(60));
        let rb = FakeRebuilder { recreate_fail: true, ..Default::default() };
        let pane = Uuid::new_v4();
        assert!(matches!(handle_overlay_crash(&mut g, &rb, pane), OverlayRecoveryOutcome::Failed(_)));
        assert!(matches!(handle_overlay_crash(&mut g, &rb, pane), OverlayRecoveryOutcome::Failed(_)));
        assert_eq!(handle_overlay_crash(&mut g, &rb, pane), OverlayRecoveryOutcome::GaveUp);
    }
}
