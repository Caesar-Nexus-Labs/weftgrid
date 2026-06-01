//! CDP extras (P7, Windows-only, OPTIONAL superset). NEVER participates in
//! snapshot/ref — the single inject-JS DOM-walk is the sole parity backend. CDP
//! is reserved for things JS does poorly (network interception, advanced eval).
//!
//! No new Rust dependency is added here. The trait defines the seam; the Windows
//! placeholder returns [`CdpError::NotWired`] until a real CDP client (e.g.
//! `chromiumoxide`, which would be lead-approved) lands. On non-Windows the type
//! is absent so callers can `#[cfg(windows)]`-gate without stubbing a fake impl.
//!
//! Security (red-team C2/H-CDP): when wired, the CDP endpoint MUST be ephemeral
//! (port 0) + loopback only, discovered via `/json/version`; prefer
//! `--remote-debugging-pipe` (no TCP) — that spike is a PoC gate, not done here.

use std::fmt;

/// Errors from the CDP extras layer. Distinct from inject errors so callers can
/// tell "JS path failed" from "CDP superset unavailable".
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CdpError {
    /// CDP is defined but not yet connected to a real client (current state).
    NotWired,
    /// CDP is unsupported on this OS (non-Windows).
    Unsupported,
    /// A wired CDP call failed.
    Failed(String),
}

impl fmt::Display for CdpError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CdpError::NotWired => write!(f, "CDP extras not yet wired"),
            CdpError::Unsupported => write!(f, "CDP extras unsupported on this platform"),
            CdpError::Failed(m) => write!(f, "CDP extras failed: {m}"),
        }
    }
}

impl std::error::Error for CdpError {}

/// Superset-only CDP operations. Snapshot/ref are deliberately absent — those are
/// owned by the inject-JS backend on both OS to guarantee `eN` parity.
pub trait CdpExtras {
    /// Advanced `Runtime.evaluate` (returns-by-value, await support) beyond what
    /// `evaluate_script` exposes. Optional superset.
    fn advanced_eval(&self, expression: &str) -> Result<serde_json::Value, CdpError>;

    /// Begin intercepting network requests matching a URL pattern. Optional.
    fn network_intercept(&self, url_pattern: &str) -> Result<(), CdpError>;
}

/// Windows placeholder. Compiles without any CDP dependency and returns
/// [`CdpError::NotWired`] for every op so the rest of the system can treat CDP as
/// "present but inert" until a real client is approved/added.
#[cfg(windows)]
#[derive(Debug, Default, Clone)]
pub struct WindowsCdpExtras;

#[cfg(windows)]
impl CdpExtras for WindowsCdpExtras {
    fn advanced_eval(&self, _expression: &str) -> Result<serde_json::Value, CdpError> {
        Err(CdpError::NotWired)
    }

    fn network_intercept(&self, _url_pattern: &str) -> Result<(), CdpError> {
        Err(CdpError::NotWired)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cdp_error_displays() {
        assert_eq!(CdpError::NotWired.to_string(), "CDP extras not yet wired");
        assert_eq!(
            CdpError::Failed("x".into()).to_string(),
            "CDP extras failed: x"
        );
    }

    #[cfg(windows)]
    #[test]
    fn windows_placeholder_is_inert() {
        let cdp = WindowsCdpExtras;
        assert_eq!(cdp.advanced_eval("1+1"), Err(CdpError::NotWired));
        assert_eq!(cdp.network_intercept("*"), Err(CdpError::NotWired));
    }
}
