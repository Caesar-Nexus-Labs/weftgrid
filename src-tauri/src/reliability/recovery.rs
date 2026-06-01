//! Recovery DECISION logic (P14) — pure state machines, no live AppHandle.
//!
//! Every fragile subsystem (WebView2 `ProcessFailed`, overlay crash, CDP socket
//! loss, PTY death) shares the same recovery questions: should we act on THIS event
//! (or is it a duplicate)? how long to wait before the next attempt? when do we
//! give up to avoid an infinite recreate→crash→recreate loop? This module answers
//! them as pure functions / small state machines so the dangerous parts are fully
//! unit-tested; the live hooks ([`super::webview_recovery`] etc.) own only the thin
//! "now actually recreate the window" seam and delegate the decision here.
//!
//! Submodules:
//!   - [`backoff`] — exponential backoff schedule with a cap.
//!   - [`RecoveryGuard`] — idempotency + max-retry + cooldown gate.
//!   - [`pty`] — classify a child process exit as graceful vs unexpected.

use std::time::Duration;

pub mod backoff;
pub mod pty;

pub use backoff::BackoffSchedule;
// Re-exported as the track's public recovery API; consumed via the submodule path
// internally (pty_watchdog), exposed here for external callers.
#[allow(unused_imports)]
pub use pty::{classify_exit, PtyExit, PtyExitClass};

/// Outcome of asking the guard whether to recover after a failure event.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RecoveryDecision {
    /// Proceed with a recovery attempt; wait `delay` first (backoff). The attempt
    /// number (1-based) is included for logging/diagnostics.
    Recover { attempt: u32, delay: Duration },
    /// Ignore this event — a recovery for the same generation is already in flight
    /// (idempotency guard: the event fired more than once for one real failure).
    Duplicate,
    /// Stop trying. The retry budget is exhausted; the subsystem must degrade and
    /// surface the failure to the user instead of looping forever.
    GiveUp,
}

/// Idempotency + max-retry + cooldown gate for ONE recoverable resource (a webview,
/// an overlay, a CDP session, a pane).
///
/// Two failure modes it prevents:
///   1. **Double-recreate**: an OS event (e.g. `ProcessFailed`) can fire several
///      times for a single crash. The guard tracks a monotonically increasing
///      `generation`; once recovery starts for the current generation, repeat
///      events return [`RecoveryDecision::Duplicate`] until the recovery completes
///      and bumps the generation.
///   2. **Infinite loop**: recreate→crash→recreate. The guard counts attempts and
///      returns [`RecoveryDecision::GiveUp`] once `max_retries` is hit, until a
///      `cooldown` window of stability resets the counter.
#[derive(Debug, Clone)]
pub struct RecoveryGuard {
    schedule: BackoffSchedule,
    max_retries: u32,
    cooldown: Duration,
    /// Attempts spent since the last reset.
    attempts: u32,
    /// True while a recovery is in flight (set on `Recover`, cleared on
    /// `mark_recovered`). Repeat failure events while set are duplicates.
    recovering: bool,
    /// Bumped each time a recovery completes — lets a caller correlate which
    /// generation an event belongs to if it wants stricter dedup.
    generation: u64,
}

impl RecoveryGuard {
    /// `max_retries` attempts within `cooldown` before giving up; `schedule` paces
    /// the waits between attempts.
    pub fn new(schedule: BackoffSchedule, max_retries: u32, cooldown: Duration) -> Self {
        RecoveryGuard {
            schedule,
            max_retries,
            cooldown,
            attempts: 0,
            recovering: false,
            generation: 0,
        }
    }

    /// Sensible default: 5 attempts, exponential 200ms→30s, 60s cooldown.
    pub fn with_defaults() -> Self {
        Self::new(BackoffSchedule::default(), 5, Duration::from_secs(60))
    }

    pub fn generation(&self) -> u64 {
        self.generation
    }

    pub fn is_recovering(&self) -> bool {
        self.recovering
    }

    /// A failure event arrived. Decide what to do:
    ///   - already recovering → [`RecoveryDecision::Duplicate`] (idempotency),
    ///   - budget exhausted → [`RecoveryDecision::GiveUp`],
    ///   - otherwise → [`RecoveryDecision::Recover`] with the backoff delay, and the
    ///     guard enters the recovering state until [`Self::mark_recovered`] /
    ///     [`Self::mark_failed`].
    pub fn on_failure(&mut self) -> RecoveryDecision {
        if self.recovering {
            return RecoveryDecision::Duplicate;
        }
        if self.attempts >= self.max_retries {
            return RecoveryDecision::GiveUp;
        }
        let delay = self.schedule.delay_for(self.attempts);
        self.attempts += 1;
        self.recovering = true;
        RecoveryDecision::Recover {
            attempt: self.attempts,
            delay,
        }
    }

    /// The recovery attempt succeeded. Leaves the recovering state and bumps the
    /// generation. The attempt counter is NOT reset here: a webview that recovers
    /// then immediately crashes again must still count toward the retry budget — the
    /// counter only resets after a [`Self::cooldown_elapsed`] of stability.
    pub fn mark_recovered(&mut self) {
        self.recovering = false;
        self.generation += 1;
    }

    /// The recovery attempt itself failed (e.g. recreate threw). Leave the
    /// recovering state so the next event can try again (subject to the budget).
    pub fn mark_failed(&mut self) {
        self.recovering = false;
    }

    /// Reset the attempt budget after the resource has been stable for at least
    /// `cooldown`. Callers pass the elapsed-since-last-failure; returns whether the
    /// counter was reset. This is what lets a long-lived session recover again later
    /// without carrying stale attempt counts forever.
    pub fn cooldown_elapsed(&mut self, stable_for: Duration) -> bool {
        if stable_for >= self.cooldown {
            self.attempts = 0;
            true
        } else {
            false
        }
    }

    pub fn cooldown(&self) -> Duration {
        self.cooldown
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn guard() -> RecoveryGuard {
        RecoveryGuard::new(
            BackoffSchedule::new(Duration::from_millis(100), Duration::from_secs(10), 2.0),
            3,
            Duration::from_secs(60),
        )
    }

    #[test]
    fn first_failure_recovers_with_zero_initial_delay() {
        let mut g = guard();
        match g.on_failure() {
            RecoveryDecision::Recover { attempt, delay } => {
                assert_eq!(attempt, 1);
                // attempt index 0 → base delay.
                assert_eq!(delay, Duration::from_millis(100));
            }
            other => panic!("expected Recover, got {other:?}"),
        }
        assert!(g.is_recovering());
    }

    #[test]
    fn repeat_event_while_recovering_is_duplicate() {
        let mut g = guard();
        assert!(matches!(g.on_failure(), RecoveryDecision::Recover { .. }));
        // ProcessFailed fired twice for one crash → second is a no-op.
        assert_eq!(g.on_failure(), RecoveryDecision::Duplicate);
        assert_eq!(g.on_failure(), RecoveryDecision::Duplicate);
    }

    #[test]
    fn idempotency_guard_prevents_double_recreate() {
        // The core safety property: N events while recovering yield exactly ONE
        // Recover decision (so exactly one recreate happens).
        let mut g = guard();
        let mut recover_count = 0;
        for _ in 0..10 {
            if matches!(g.on_failure(), RecoveryDecision::Recover { .. }) {
                recover_count += 1;
            }
        }
        assert_eq!(recover_count, 1);
    }

    #[test]
    fn max_retries_then_give_up() {
        let mut g = guard(); // max_retries = 3
        for expected in 1..=3 {
            match g.on_failure() {
                RecoveryDecision::Recover { attempt, .. } => assert_eq!(attempt, expected),
                other => panic!("attempt {expected}: expected Recover, got {other:?}"),
            }
            g.mark_failed(); // recovery attempt failed → allow next
        }
        // Budget exhausted → stop the loop.
        assert_eq!(g.on_failure(), RecoveryDecision::GiveUp);
    }

    #[test]
    fn cooldown_resets_budget() {
        let mut g = guard();
        for _ in 0..3 {
            g.on_failure();
            g.mark_failed();
        }
        assert_eq!(g.on_failure(), RecoveryDecision::GiveUp);
        // Not enough stable time → still giving up.
        assert!(!g.cooldown_elapsed(Duration::from_secs(30)));
        assert_eq!(g.on_failure(), RecoveryDecision::GiveUp);
        // Stable past the cooldown → budget resets, recovery allowed again.
        assert!(g.cooldown_elapsed(Duration::from_secs(60)));
        assert!(matches!(g.on_failure(), RecoveryDecision::Recover { .. }));
    }

    #[test]
    fn mark_recovered_bumps_generation_and_clears_state() {
        let mut g = guard();
        assert_eq!(g.generation(), 0);
        g.on_failure();
        g.mark_recovered();
        assert_eq!(g.generation(), 1);
        assert!(!g.is_recovering());
    }

    #[test]
    fn recovered_then_immediate_recrash_still_counts() {
        // recover→crash→recover→crash must keep counting toward the budget, not
        // reset on each success — otherwise a tight crash loop never gives up.
        let mut g = guard(); // max 3
        for _ in 0..3 {
            assert!(matches!(g.on_failure(), RecoveryDecision::Recover { .. }));
            g.mark_recovered();
        }
        assert_eq!(g.on_failure(), RecoveryDecision::GiveUp);
    }
}
