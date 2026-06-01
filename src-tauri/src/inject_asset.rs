//! Embedded page-context inject bundle (P2 Keystone 2 proof).
//!
//! The inject script is authored in TS (`inject/snapshot.ts`), built to an IIFE
//! JS string by `npm run build:inject`, and embedded here at compile time. P7
//! injects this into browser panes. P2 only proves the embed wiring.

/// The built inject bundle as a string, embedded at compile time.
pub const INJECT_SNAPSHOT_JS: &str = include_str!("../assets/inject/snapshot.js");

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inject_bundle_is_embedded_and_non_empty() {
        assert!(!INJECT_SNAPSHOT_JS.trim().is_empty());
        // Stub marker present (P7 replaces the body but keeps a non-empty IIFE).
        assert!(INJECT_SNAPSHOT_JS.contains("weftgrid-inject-stub"));
    }
}
