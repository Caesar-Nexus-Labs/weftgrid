//! CDP socket-loss reconnect seam (P14 → P7 extras).
//!
//! P7's CDP extras (Windows-only superset) talk to the browser over a debugging
//! socket. If that socket drops, the recovery is: retry the connect with
//! exponential backoff up to a budget; if it never comes back, DEGRADE — mark CDP
//! extras unavailable so automation reports a clean error instead of crashing the
//! app (CDP is never on the snapshot/ref parity path, so the terminal keeps working).
//!
//! This module is mostly pure: [`plan_reconnect`] turns "we've tried N times" into
//! the next [`ReconnectStep`] (wait this long / give up), driven by the shared
//! [`BackoffSchedule`]. The single side-effecting seam is the actual socket connect,
//! injected as a closure so the retry loop is testable with a fake that fails K
//! times then succeeds — no real socket.
//!
//! LIVE-WIRED WAVE-DEFERRED: the real connect closure (a `chromiumoxide` /
//! raw-CDP client, which is a lead-approved dependency NOT yet added — see P7
//! `cdp_extras.rs` `NotWired`) and wiring socket-loss detection into the live
//! client. Until that client exists this is the reconnect POLICY + degrade decision,
//! ready for the real connect to drop in.

use std::time::Duration;

use super::recovery::BackoffSchedule;

/// One step of the reconnect loop, computed from the attempt count.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReconnectStep {
    /// Wait `delay`, then attempt reconnect number `attempt` (1-based).
    Retry { attempt: u32, delay: Duration },
    /// Out of retries — degrade: mark CDP extras unavailable, surface a clean error.
    Degrade,
}

/// Reconnect policy: how many attempts before degrading + the backoff pacing.
#[derive(Debug, Clone)]
pub struct ReconnectPolicy {
    schedule: BackoffSchedule,
    max_attempts: u32,
}

impl Default for ReconnectPolicy {
    fn default() -> Self {
        ReconnectPolicy {
            schedule: BackoffSchedule::default(),
            max_attempts: 5,
        }
    }
}

impl ReconnectPolicy {
    pub fn new(schedule: BackoffSchedule, max_attempts: u32) -> Self {
        ReconnectPolicy {
            schedule,
            max_attempts,
        }
    }

    /// Decide the next step given how many attempts have already been made
    /// (`attempts_done`, 0-based count of prior tries). Pure.
    pub fn plan_reconnect(&self, attempts_done: u32) -> ReconnectStep {
        if attempts_done >= self.max_attempts {
            ReconnectStep::Degrade
        } else {
            ReconnectStep::Retry {
                attempt: attempts_done + 1,
                delay: self.schedule.delay_for(attempts_done),
            }
        }
    }

    pub fn max_attempts(&self) -> u32 {
        self.max_attempts
    }
}

/// Final result of a reconnect run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReconnectResult {
    /// Reconnected on this attempt (1-based).
    Reconnected { attempt: u32 },
    /// Exhausted the budget; CDP extras degrade to unavailable.
    Degraded,
}

/// Drive the reconnect loop using `connect` as the (injected) socket-connect seam:
/// it returns `Ok(())` on a successful connect, `Err` to keep retrying. Returns
/// either a successful attempt number or [`ReconnectResult::Degraded`].
///
/// NOTE: this does NOT sleep — it reports the planned delays via `on_wait` so a
/// caller (or test) controls timing. Production passes a closure that sleeps
/// `delay`; tests pass a no-op and assert the attempt count, keeping the test fast
/// and deterministic.
pub fn run_reconnect<C, W>(
    policy: &ReconnectPolicy,
    mut connect: C,
    mut on_wait: W,
) -> ReconnectResult
where
    C: FnMut(u32) -> Result<(), String>,
    W: FnMut(Duration),
{
    let mut attempts_done = 0;
    loop {
        match policy.plan_reconnect(attempts_done) {
            ReconnectStep::Degrade => return ReconnectResult::Degraded,
            ReconnectStep::Retry { attempt, delay } => {
                on_wait(delay);
                match connect(attempt) {
                    Ok(()) => return ReconnectResult::Reconnected { attempt },
                    Err(_) => attempts_done += 1,
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    fn fast_policy(max: u32) -> ReconnectPolicy {
        ReconnectPolicy::new(
            BackoffSchedule::new(Duration::from_millis(10), Duration::from_secs(1), 2.0),
            max,
        )
    }

    #[test]
    fn plan_retries_then_degrades() {
        let p = fast_policy(3);
        assert_eq!(
            p.plan_reconnect(0),
            ReconnectStep::Retry { attempt: 1, delay: Duration::from_millis(10) }
        );
        assert_eq!(
            p.plan_reconnect(2),
            ReconnectStep::Retry { attempt: 3, delay: Duration::from_millis(40) }
        );
        // 3 done == max → degrade.
        assert_eq!(p.plan_reconnect(3), ReconnectStep::Degrade);
    }

    #[test]
    fn reconnects_after_transient_failures() {
        let p = fast_policy(5);
        let attempts = Cell::new(0u32);
        let waits = Cell::new(0u32);
        let result = run_reconnect(
            &p,
            |_attempt| {
                let n = attempts.get() + 1;
                attempts.set(n);
                if n < 3 {
                    Err("socket closed".into())
                } else {
                    Ok(())
                }
            },
            |_d| waits.set(waits.get() + 1),
        );
        assert_eq!(result, ReconnectResult::Reconnected { attempt: 3 });
        assert_eq!(attempts.get(), 3);
        assert_eq!(waits.get(), 3);
    }

    #[test]
    fn degrades_when_socket_never_recovers() {
        let p = fast_policy(4);
        let calls = Cell::new(0u32);
        let result = run_reconnect(
            &p,
            |_| {
                calls.set(calls.get() + 1);
                Err("refused".into())
            },
            |_| {},
        );
        assert_eq!(result, ReconnectResult::Degraded);
        // Tried exactly max_attempts times before degrading — never loops forever.
        assert_eq!(calls.get(), 4);
    }

    #[test]
    fn first_attempt_success_no_extra_tries() {
        let p = fast_policy(5);
        let calls = Cell::new(0u32);
        let result = run_reconnect(&p, |_| { calls.set(calls.get() + 1); Ok(()) }, |_| {});
        assert_eq!(result, ReconnectResult::Reconnected { attempt: 1 });
        assert_eq!(calls.get(), 1);
    }
}
