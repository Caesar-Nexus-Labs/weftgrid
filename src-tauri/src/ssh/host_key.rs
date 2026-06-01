//! SSH host-key policy (P10a).
//!
//! Accept-new (like cmux): the first fingerprint seen for a host is trusted and
//! remembered; a later *different* fingerprint for the same host is rejected
//! (possible MITM). Reconnect reuses the same store, so a key change across a
//! reconnect is caught — the spec requires reconnect not to skip the host-key
//! check.
//!
//! Pure + in-memory so the decision logic is unit-tested without a live server.
//! Persistence to a known_hosts file is out of P10a-core scope (no owner here);
//! the store is per-session, which still gives accept-new + change-detection
//! within a session and across its reconnects.

use std::collections::HashMap;
use std::sync::Mutex;

/// Outcome of checking a server key fingerprint against what we've seen.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HostKeyDecision {
    /// First time we've seen this host — trust-on-first-use, now remembered.
    AcceptNew,
    /// Fingerprint matches the one remembered for this host.
    Match,
    /// Fingerprint differs from the remembered one — reject (possible MITM).
    Changed {
        remembered: String,
        presented: String,
    },
}

impl HostKeyDecision {
    /// Whether the connection should proceed. `Changed` is the only rejection.
    pub fn is_trusted(&self) -> bool {
        !matches!(self, HostKeyDecision::Changed { .. })
    }
}

/// Remembers the first fingerprint per host for accept-new + change detection.
#[derive(Default)]
pub struct HostKeyStore {
    known: Mutex<HashMap<String, String>>,
}

impl HostKeyStore {
    pub fn new() -> Self {
        Self::default()
    }

    /// Seed a known fingerprint (e.g. from a prior session / config).
    pub fn remember(&self, host: impl Into<String>, fingerprint: impl Into<String>) {
        self.known
            .lock()
            .unwrap()
            .insert(host.into(), fingerprint.into());
    }

    /// Check a presented fingerprint, recording it on first use.
    pub fn check(&self, host: &str, fingerprint: &str) -> HostKeyDecision {
        let mut known = self.known.lock().unwrap();
        match known.get(host) {
            None => {
                known.insert(host.to_string(), fingerprint.to_string());
                HostKeyDecision::AcceptNew
            }
            Some(remembered) if remembered == fingerprint => HostKeyDecision::Match,
            Some(remembered) => HostKeyDecision::Changed {
                remembered: remembered.clone(),
                presented: fingerprint.to_string(),
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn first_seen_is_accept_new() {
        let store = HostKeyStore::new();
        let d = store.check("host", "SHA256:abc");
        assert_eq!(d, HostKeyDecision::AcceptNew);
        assert!(d.is_trusted());
    }

    #[test]
    fn same_key_matches() {
        let store = HostKeyStore::new();
        store.check("host", "SHA256:abc");
        assert_eq!(store.check("host", "SHA256:abc"), HostKeyDecision::Match);
    }

    #[test]
    fn changed_key_is_rejected() {
        let store = HostKeyStore::new();
        store.check("host", "SHA256:abc");
        let d = store.check("host", "SHA256:xyz");
        assert_eq!(
            d,
            HostKeyDecision::Changed {
                remembered: "SHA256:abc".into(),
                presented: "SHA256:xyz".into()
            }
        );
        assert!(!d.is_trusted(), "a changed host key must be rejected");
    }

    #[test]
    fn distinct_hosts_are_independent() {
        let store = HostKeyStore::new();
        assert_eq!(store.check("a", "fp1"), HostKeyDecision::AcceptNew);
        assert_eq!(store.check("b", "fp2"), HostKeyDecision::AcceptNew);
        assert_eq!(store.check("a", "fp1"), HostKeyDecision::Match);
    }

    #[test]
    fn seeded_fingerprint_matches() {
        let store = HostKeyStore::new();
        store.remember("host", "SHA256:seed");
        assert_eq!(store.check("host", "SHA256:seed"), HostKeyDecision::Match);
    }
}
