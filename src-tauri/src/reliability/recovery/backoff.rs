//! Exponential backoff schedule (P14 recovery pacing).
//!
//! Used by CDP socket-loss reconnect and the generic [`super::RecoveryGuard`]: the
//! Nth retry waits `base * factor^N`, capped at `max`. Pure and deterministic — no
//! jitter (a single desktop client reconnecting to its own loopback CDP socket has
//! no thundering-herd concern, so jitter would only make tests nondeterministic).

use std::time::Duration;

/// Exponential backoff with a ceiling. `delay_for(n)` is the wait before attempt
/// index `n` (0-based): `n == 0` → `base`, growing by `factor` each step up to `max`.
#[derive(Debug, Clone, PartialEq)]
pub struct BackoffSchedule {
    base: Duration,
    max: Duration,
    factor: f64,
}

impl Default for BackoffSchedule {
    fn default() -> Self {
        BackoffSchedule {
            base: Duration::from_millis(200),
            max: Duration::from_secs(30),
            factor: 2.0,
        }
    }
}

impl BackoffSchedule {
    pub fn new(base: Duration, max: Duration, factor: f64) -> Self {
        BackoffSchedule { base, max, factor }
    }

    /// Delay before the `attempt`-indexed retry (0-based), capped at `max`.
    /// Computed in f64 seconds then clamped, so a large `attempt` saturates at the
    /// cap instead of overflowing the multiply. The exponent is capped at 1024 so a
    /// huge `attempt` can't wrap when cast to `i32` (which would shrink the delay).
    pub fn delay_for(&self, attempt: u32) -> Duration {
        let base_secs = self.base.as_secs_f64();
        let max_secs = self.max.as_secs_f64();
        let exp = attempt.min(1024) as i32;
        let grown = base_secs * self.factor.powi(exp);
        if !grown.is_finite() || grown >= max_secs {
            return self.max;
        }
        Duration::from_secs_f64(grown)
    }

    /// The first `count` delays as a vec — handy for asserting a whole schedule in
    /// one go and for a caller that wants to pre-materialise the plan.
    pub fn schedule(&self, count: u32) -> Vec<Duration> {
        (0..count).map(|n| self.delay_for(n)).collect()
    }

    pub fn max(&self) -> Duration {
        self.max
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn grows_exponentially_until_cap() {
        let b = BackoffSchedule::new(Duration::from_millis(100), Duration::from_secs(5), 2.0);
        assert_eq!(b.delay_for(0), Duration::from_millis(100));
        assert_eq!(b.delay_for(1), Duration::from_millis(200));
        assert_eq!(b.delay_for(2), Duration::from_millis(400));
        assert_eq!(b.delay_for(3), Duration::from_millis(800));
        // 100ms * 2^6 = 6.4s > 5s cap → clamped.
        assert_eq!(b.delay_for(6), Duration::from_secs(5));
    }

    #[test]
    fn large_attempt_saturates_at_cap_without_overflow() {
        let b = BackoffSchedule::default();
        assert_eq!(b.delay_for(1000), b.max());
        assert_eq!(b.delay_for(u32::MAX), b.max());
    }

    #[test]
    fn schedule_materialises_first_n() {
        let b = BackoffSchedule::new(Duration::from_secs(1), Duration::from_secs(8), 2.0);
        assert_eq!(
            b.schedule(5),
            vec![
                Duration::from_secs(1),
                Duration::from_secs(2),
                Duration::from_secs(4),
                Duration::from_secs(8), // capped
                Duration::from_secs(8),
            ]
        );
    }
}
