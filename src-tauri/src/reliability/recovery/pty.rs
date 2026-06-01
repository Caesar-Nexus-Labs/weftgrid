//! PTY-death classification (P14 — extends P3's graceful-exit handling).
//!
//! P3 owns the normal PTY lifecycle; P14 only needs to DECIDE, from a child's exit
//! status, whether the death was expected (user typed `exit`, clean shell teardown)
//! or unexpected (killed by a signal, crashed, nonzero from an unexpected SIGHUP).
//! An unexpected death is what triggers the user-facing "pane died — respawn?"
//! recovery; a graceful one is silent. This is a pure function over the exit code /
//! signal so it is fully unit-testable without spawning a real process — the live
//! watchdog (in [`super::super::pty_watchdog`]) feeds it the status it observed.

/// How a PTY child process exited, from the recovery layer's point of view.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PtyExitClass {
    /// Clean, expected termination — no recovery prompt. Exit code 0, or a code the
    /// user clearly caused (e.g. `exit 1` in a shell). We treat any plain exit-code
    /// path as graceful: the shell ran and returned, the user can re-open a pane.
    Graceful,
    /// Killed by a signal, or a code from the "process crashed / was force-killed"
    /// band. Surfaces the respawn offer so the user isn't left with a silent dead
    /// pane (the blocking-read hang class of bug).
    Unexpected,
}

/// Observed exit status of a PTY child, normalised across platforms.
///
/// `portable_pty::ExitStatus` exposes a raw code; on unix a signal death is encoded
/// by the OS as `128 + signal`. We model both explicitly so the classifier doesn't
/// have to re-derive platform encoding, and so the watchdog can pass whichever it
/// actually has.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PtyExit {
    /// Process returned this exit code normally.
    Code(i32),
    /// Process was terminated by this signal number (unix).
    Signal(i32),
}

/// Classify a child exit into graceful vs unexpected.
///
/// Rules (conservative — when ambiguous, prefer surfacing recovery over hiding a
/// real crash, but don't nag on ordinary nonzero shell exits):
///   - signal death → always [`PtyExitClass::Unexpected`] (crash / OOM-kill / forced).
///   - exit code in the `128 + n` signal-encoded band → [`PtyExitClass::Unexpected`]
///     (a shell reporting its child died on a signal).
///   - any other plain exit code (incl. 0 and ordinary nonzero) → [`PtyExitClass::Graceful`].
pub fn classify_exit(exit: PtyExit) -> PtyExitClass {
    match exit {
        PtyExit::Signal(_) => PtyExitClass::Unexpected,
        PtyExit::Code(code) => {
            // 129..=159 == 128 + (1..=31): the shell convention for "child died on
            // signal N". SIGHUP(1)/SIGKILL(9)/SIGSEGV(11)/SIGTERM(15) all land here.
            if (129..=159).contains(&code) {
                PtyExitClass::Unexpected
            } else {
                PtyExitClass::Graceful
            }
        }
    }
}

/// Whether this exit should trigger the user-facing respawn offer.
pub fn should_offer_respawn(exit: PtyExit) -> bool {
    classify_exit(exit) == PtyExitClass::Unexpected
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clean_exit_is_graceful() {
        assert_eq!(classify_exit(PtyExit::Code(0)), PtyExitClass::Graceful);
        assert!(!should_offer_respawn(PtyExit::Code(0)));
    }

    #[test]
    fn ordinary_nonzero_exit_is_graceful() {
        // `grep` no-match returns 1; `exit 2` from a script. The user caused these;
        // we don't nag with a respawn prompt.
        assert_eq!(classify_exit(PtyExit::Code(1)), PtyExitClass::Graceful);
        assert_eq!(classify_exit(PtyExit::Code(2)), PtyExitClass::Graceful);
        assert_eq!(classify_exit(PtyExit::Code(127)), PtyExitClass::Graceful);
    }

    #[test]
    fn signal_death_is_unexpected() {
        assert_eq!(classify_exit(PtyExit::Signal(9)), PtyExitClass::Unexpected);
        assert_eq!(classify_exit(PtyExit::Signal(11)), PtyExitClass::Unexpected);
        assert!(should_offer_respawn(PtyExit::Signal(15)));
    }

    #[test]
    fn signal_encoded_exit_code_is_unexpected() {
        // 128 + 9 (SIGKILL), 128 + 11 (SIGSEGV), 128 + 1 (SIGHUP).
        assert_eq!(classify_exit(PtyExit::Code(137)), PtyExitClass::Unexpected);
        assert_eq!(classify_exit(PtyExit::Code(139)), PtyExitClass::Unexpected);
        assert_eq!(classify_exit(PtyExit::Code(129)), PtyExitClass::Unexpected);
        assert!(should_offer_respawn(PtyExit::Code(143)));
    }

    #[test]
    fn boundary_codes_around_signal_band() {
        // 128 itself is not a signal encoding; 160 is past the 31-signal range.
        assert_eq!(classify_exit(PtyExit::Code(128)), PtyExitClass::Graceful);
        assert_eq!(classify_exit(PtyExit::Code(160)), PtyExitClass::Graceful);
    }
}
