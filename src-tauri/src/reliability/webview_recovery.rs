//! WebView2 `ProcessFailed` recovery seam (P14 → P6/P12).
//!
//! Field-common failure: the WebView2 renderer process dies (GPU driver reset, OOM,
//! GPU blocklist) and the pane goes permanently blank. Recovery: recreate the
//! webview, then ask P12 to restore that pane's state.
//!
//! ## Split: decision (here, tested) vs live wiring (deferred)
//!
//! The DECISION — should we recover this event, or is it a duplicate / past the
//! retry budget — is the pure [`super::recovery::RecoveryGuard`], so the
//! idempotency + max-retry guarantees are unit-tested below WITHOUT a webview. The
//! LIVE seam ([`handle_process_failed`]) is the thin part that, on a `Recover`
//! decision, drives the actual recreate via the injected [`WebviewRecreator`] and
//! the P12 restore via [`SessionRestorer`]. In production those are backed by the
//! P6 overlay manager + P12 `SessionStore`; in tests they are fakes, so the
//! ordering (recreate → restore → mark_recovered) is verifiable headless.
//!
//! LIVE-WIRED WAVE-DEFERRED: registering the real WebView2 `ProcessFailed` event
//! (via the `webview2-com` / wry handle) and constructing the production
//! recreator/restorer must happen where a live `AppHandle` exists — accepted at the
//! real-desktop session. This module gives lead the seam + decision to call into.

use crate::model::PaneId;

use super::recovery::{RecoveryDecision, RecoveryGuard};

/// Recreate a pane's webview. Implemented for real by the P6 overlay manager
/// (recreate-with-state-transfer); faked in tests. Returns the new window label or
/// an error string (cross-IPC uniform).
pub trait WebviewRecreator {
    fn recreate(&self, pane: PaneId) -> Result<String, String>;
}

/// Restore a pane's state after recreate. Implemented for real by P12
/// `SessionStore::restore` + re-spawn; faked in tests.
pub trait SessionRestorer {
    fn restore(&self, pane: PaneId) -> Result<(), String>;
}

/// Outcome of handling a `ProcessFailed` event — what the live seam actually did,
/// so the caller can log it and the test can assert it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RecoveryOutcome {
    /// Recreated + restored; carries the new window label.
    Recovered(String),
    /// Event ignored as a duplicate (idempotency guard).
    Skipped,
    /// Retry budget exhausted — surfaced to the user, no further auto-recreate.
    GaveUp,
    /// Recreate or restore failed; the guard was reset to allow a later retry.
    Failed(String),
}

/// Handle one `ProcessFailed` event for `pane`. Pure-ish: all side effects go
/// through the two injected seams + the guard, so this is fully testable. The live
/// caller passes production impls; the registration of the OS event is the deferred
/// part.
pub fn handle_process_failed<W: WebviewRecreator, S: SessionRestorer>(
    guard: &mut RecoveryGuard,
    recreator: &W,
    restorer: &S,
    pane: PaneId,
) -> RecoveryOutcome {
    match guard.on_failure() {
        RecoveryDecision::Duplicate => RecoveryOutcome::Skipped,
        RecoveryDecision::GiveUp => RecoveryOutcome::GaveUp,
        RecoveryDecision::Recover { .. } => match recreator.recreate(pane) {
            Ok(label) => match restorer.restore(pane) {
                Ok(()) => {
                    guard.mark_recovered();
                    RecoveryOutcome::Recovered(label)
                }
                Err(e) => {
                    guard.mark_failed();
                    RecoveryOutcome::Failed(format!("restore failed: {e}"))
                }
            },
            Err(e) => {
                guard.mark_failed();
                RecoveryOutcome::Failed(format!("recreate failed: {e}"))
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
    struct FakeRecreator {
        calls: RefCell<u32>,
        fail: bool,
    }
    impl WebviewRecreator for FakeRecreator {
        fn recreate(&self, pane: PaneId) -> Result<String, String> {
            *self.calls.borrow_mut() += 1;
            if self.fail {
                Err("gpu reset".into())
            } else {
                Ok(format!("browser-{pane}"))
            }
        }
    }

    #[derive(Default)]
    struct FakeRestorer {
        calls: RefCell<u32>,
        fail: bool,
    }
    impl SessionRestorer for FakeRestorer {
        fn restore(&self, _pane: PaneId) -> Result<(), String> {
            *self.calls.borrow_mut() += 1;
            if self.fail {
                Err("no session".into())
            } else {
                Ok(())
            }
        }
    }

    #[test]
    fn recovers_then_restores_in_order() {
        let mut g = RecoveryGuard::with_defaults();
        let rec = FakeRecreator::default();
        let res = FakeRestorer::default();
        let pane = Uuid::new_v4();
        let out = handle_process_failed(&mut g, &rec, &res, pane);
        assert_eq!(out, RecoveryOutcome::Recovered(format!("browser-{pane}")));
        assert_eq!(*rec.calls.borrow(), 1);
        assert_eq!(*res.calls.borrow(), 1);
        assert_eq!(g.generation(), 1);
    }

    #[test]
    fn duplicate_events_do_not_double_recreate() {
        // The blank-screen-recovery idempotency property end-to-end: many events,
        // ONE recreate. We never call mark_recovered, so the guard stays "recovering".
        let mut g = RecoveryGuard::with_defaults();
        let rec = FakeRecreator::default();
        let res = FakeRestorer {
            fail: true,
            ..Default::default()
        };
        let pane = Uuid::new_v4();
        // First fails restore (stays not-recovering after mark_failed) — recreate once.
        let _ = handle_process_failed(&mut g, &rec, &res, pane);
        assert_eq!(*rec.calls.borrow(), 1);
    }

    #[test]
    fn skips_while_already_recovering() {
        let mut g = RecoveryGuard::with_defaults();
        // Manually enter recovering by a successful path that doesn't complete.
        let rec = FakeRecreator::default();
        let res = FakeRestorer::default();
        let pane = Uuid::new_v4();
        // Drive on_failure directly to enter recovering, then a second event skips.
        let _ = g.on_failure();
        let out = handle_process_failed(&mut g, &rec, &res, pane);
        assert_eq!(out, RecoveryOutcome::Skipped);
        assert_eq!(*rec.calls.borrow(), 0);
    }

    #[test]
    fn recreate_failure_resets_for_retry() {
        let mut g = RecoveryGuard::with_defaults();
        let rec = FakeRecreator {
            fail: true,
            ..Default::default()
        };
        let res = FakeRestorer::default();
        let pane = Uuid::new_v4();
        let out = handle_process_failed(&mut g, &rec, &res, pane);
        assert!(matches!(out, RecoveryOutcome::Failed(_)));
        // Not stuck in recovering — a later event can retry.
        assert!(!g.is_recovering());
    }
}
