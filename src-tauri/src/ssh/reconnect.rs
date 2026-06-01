//! Reconnect supervisor — deterministic state machine (P10a).
//!
//! A single SSH session backs the terminal shell + SOCKS broker; when it drops
//! (laptop sleep, wifi-switch, NAT/idle timeout — all routine) everything dies
//! with it. This module owns the *decision* logic: when to retry, how long to
//! back off, and when to give up. The actual re-auth/re-establish (network) is
//! driven by `client`/`commands`; this stays pure so it is unit-testable without
//! a live host.
//!
//! Status is surfaced to the UI verbatim (camelCase) so a workspace can render
//! "reconnecting" / "failed" instead of silently looking broken. We never fake a
//! continuous session: a successful reconnect is a *new* transport.

use std::time::Duration;

use serde::{Deserialize, Serialize};

/// Connection status surfaced to the UI. Serialized camelCase (`reconnecting`,
/// `failed`) so the frontend renders it directly.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "camelCase")]
pub enum ConnectionStatus {
    /// First connect in progress (no session yet).
    Connecting,
    /// Live session established.
    Connected,
    /// Session dropped; retrying. `attempt` is 1-based; `nextBackoffMs` is how
    /// long the supervisor waits before this attempt.
    #[serde(rename_all = "camelCase")]
    Reconnecting { attempt: u32, next_backoff_ms: u64 },
    /// Retries exhausted (or fatal auth failure). Terminal — needs user action.
    #[serde(rename_all = "camelCase")]
    Failed { reason: String },
    /// Cleanly torn down by the user (not an error).
    Disconnected,
}

/// Backoff + retry-limit policy. Exponential backoff with a cap, bounded attempts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReconnectPolicy {
    /// Hard cap on reconnect attempts before transitioning to `Failed`.
    pub max_attempts: u32,
    /// Delay before the first retry; doubles each attempt up to `max_delay`.
    pub base_delay: Duration,
    /// Ceiling for the exponential backoff.
    pub max_delay: Duration,
}

impl Default for ReconnectPolicy {
    fn default() -> Self {
        // 500ms, 1s, 2s, 4s, 8s, then give up — covers a sleep/wifi blip without
        // hammering a host that is genuinely gone.
        ReconnectPolicy {
            max_attempts: 5,
            base_delay: Duration::from_millis(500),
            max_delay: Duration::from_secs(8),
        }
    }
}

impl ReconnectPolicy {
    /// Backoff for a 1-based attempt number: `base * 2^(attempt-1)`, capped at
    /// `max_delay`. Attempt 0 is treated as attempt 1 (no negative shift).
    pub fn backoff_for(&self, attempt: u32) -> Duration {
        let shift = attempt.saturating_sub(1).min(31);
        let scaled = self
            .base_delay
            .checked_mul(1u32 << shift)
            .unwrap_or(self.max_delay);
        scaled.min(self.max_delay)
    }
}

/// Drives [`ConnectionStatus`] transitions per [`ReconnectPolicy`]. Pure: feed it
/// events (`record_*`), read `status()`. No timers, no IO — the caller sleeps for
/// `backoff` and performs the network work.
#[derive(Debug, Clone)]
pub struct ReconnectSupervisor {
    policy: ReconnectPolicy,
    status: ConnectionStatus,
    attempt: u32,
}

impl ReconnectSupervisor {
    pub fn new(policy: ReconnectPolicy) -> Self {
        ReconnectSupervisor {
            policy,
            status: ConnectionStatus::Connecting,
            attempt: 0,
        }
    }

    pub fn status(&self) -> &ConnectionStatus {
        &self.status
    }

    /// Initial connect (or a reconnect) succeeded: live again, attempt counter
    /// reset so the next drop starts a fresh backoff sequence.
    pub fn record_connected(&mut self) {
        self.attempt = 0;
        self.status = ConnectionStatus::Connected;
    }

    /// Session dropped. Moves to `Reconnecting{attempt:1}` if retries remain,
    /// else `Failed`. Returns the backoff to wait before retrying, or `None` if
    /// no retry will happen.
    pub fn record_drop(&mut self) -> Option<Duration> {
        self.advance_attempt("connection dropped")
    }

    /// A reconnect attempt itself failed (re-auth/re-establish error). Advances
    /// to the next attempt or `Failed`. Returns the next backoff, or `None`.
    pub fn record_attempt_failed(&mut self) -> Option<Duration> {
        self.advance_attempt("reconnect attempt failed")
    }

    /// A non-retryable error (e.g. credential rejected): go straight to `Failed`.
    pub fn record_fatal(&mut self, reason: impl Into<String>) {
        self.status = ConnectionStatus::Failed {
            reason: reason.into(),
        };
    }

    /// User-initiated teardown.
    pub fn record_disconnected(&mut self) {
        self.status = ConnectionStatus::Disconnected;
    }

    fn advance_attempt(&mut self, reason: &str) -> Option<Duration> {
        self.attempt += 1;
        if self.attempt > self.policy.max_attempts {
            self.status = ConnectionStatus::Failed {
                reason: format!(
                    "{reason}: retries exhausted ({} attempts)",
                    self.attempt - 1
                ),
            };
            return None;
        }
        let backoff = self.policy.backoff_for(self.attempt);
        self.status = ConnectionStatus::Reconnecting {
            attempt: self.attempt,
            next_backoff_ms: backoff.as_millis() as u64,
        };
        Some(backoff)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backoff_is_exponential_and_capped() {
        let p = ReconnectPolicy {
            max_attempts: 10,
            base_delay: Duration::from_millis(500),
            max_delay: Duration::from_secs(8),
        };
        assert_eq!(p.backoff_for(1), Duration::from_millis(500));
        assert_eq!(p.backoff_for(2), Duration::from_secs(1));
        assert_eq!(p.backoff_for(3), Duration::from_secs(2));
        assert_eq!(p.backoff_for(4), Duration::from_secs(4));
        // capped
        assert_eq!(p.backoff_for(5), Duration::from_secs(8));
        assert_eq!(p.backoff_for(20), Duration::from_secs(8));
    }

    #[test]
    fn connect_then_drop_then_recover() {
        let mut sup = ReconnectSupervisor::new(ReconnectPolicy::default());
        assert_eq!(*sup.status(), ConnectionStatus::Connecting);

        sup.record_connected();
        assert_eq!(*sup.status(), ConnectionStatus::Connected);

        let backoff = sup.record_drop().expect("retry scheduled");
        assert_eq!(backoff, Duration::from_millis(500));
        assert_eq!(
            *sup.status(),
            ConnectionStatus::Reconnecting {
                attempt: 1,
                next_backoff_ms: 500
            }
        );

        // reconnect succeeds → counter resets so the next drop starts at attempt 1
        sup.record_connected();
        assert_eq!(*sup.status(), ConnectionStatus::Connected);
        let backoff = sup.record_drop().expect("retry scheduled again");
        assert_eq!(backoff, Duration::from_millis(500));
    }

    #[test]
    fn retries_exhaust_to_failed() {
        let policy = ReconnectPolicy {
            max_attempts: 3,
            base_delay: Duration::from_millis(100),
            max_delay: Duration::from_secs(1),
        };
        let mut sup = ReconnectSupervisor::new(policy);
        sup.record_connected();

        assert!(sup.record_drop().is_some()); // attempt 1
        assert!(sup.record_attempt_failed().is_some()); // attempt 2
        assert!(sup.record_attempt_failed().is_some()); // attempt 3
                                                        // attempt 4 > max_attempts (3) → Failed, no further backoff
        assert!(sup.record_attempt_failed().is_none());
        assert!(matches!(sup.status(), ConnectionStatus::Failed { .. }));
    }

    #[test]
    fn fatal_skips_retries() {
        let mut sup = ReconnectSupervisor::new(ReconnectPolicy::default());
        sup.record_connected();
        sup.record_fatal("auth rejected");
        assert_eq!(
            *sup.status(),
            ConnectionStatus::Failed {
                reason: "auth rejected".into()
            }
        );
    }

    #[test]
    fn status_serializes_camel_case_for_ui() {
        let s = ConnectionStatus::Reconnecting {
            attempt: 2,
            next_backoff_ms: 1000,
        };
        let json = serde_json::to_value(&s).unwrap();
        assert_eq!(json["state"], "reconnecting");
        assert_eq!(json["attempt"], 2);
        assert_eq!(json["nextBackoffMs"], 1000);

        let connected = serde_json::to_value(ConnectionStatus::Connected).unwrap();
        assert_eq!(connected["state"], "connected");
    }
}
