//! Consent gate for browser import (P11a core).
//!
//! Importing ANOTHER browser's cookies/history reads another app's private data,
//! so it is gated on explicit user consent. The consent flag itself lives in the
//! P12 config (`Config.import_consent`, persisted by the config track); this
//! module is the pure, testable gate the command layer calls BEFORE any read.
//!
//! Wiring: `import_cookies` / `import_history` read `import_consent` from the
//! shared `ConfigState` (P12) and pass it here. A `false` flag short-circuits the
//! command — no catalog path is ever touched, no file is opened.

/// Error returned when an import is attempted without consent. Carries a
/// user-facing, secret-free message for the UI.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConsentDenied;

impl ConsentDenied {
    /// Secret-free message the UI shows to prompt the consent dialog.
    pub const MESSAGE: &'static str =
        "Browser import requires consent. Enable it in Settings before importing.";
}

impl std::fmt::Display for ConsentDenied {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(Self::MESSAGE)
    }
}

impl std::error::Error for ConsentDenied {}

/// Gate any import read on consent. Returns `Ok(())` only when `granted`.
///
/// Call this FIRST in every cookie/history command — before resolving paths or
/// opening files — so a non-consenting user never has another app's data read.
pub fn ensure_consent(granted: bool) -> Result<(), ConsentDenied> {
    if granted {
        Ok(())
    } else {
        Err(ConsentDenied)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blocks_read_when_consent_not_granted() {
        assert_eq!(ensure_consent(false), Err(ConsentDenied));
    }

    #[test]
    fn allows_read_when_consent_granted() {
        assert!(ensure_consent(true).is_ok());
    }

    #[test]
    fn denial_message_is_secret_free_and_actionable() {
        let msg = ConsentDenied.to_string();
        assert!(msg.contains("consent"));
        assert!(msg.contains("Settings"));
    }
}
