//! Notification manager (P5a core) — UI-agnostic per-pane notification store.
//!
//! Mirrors cmux's `TerminalNotificationStore` record/dedup semantics (pinned SHA
//! `c4911439e3e99784bd5d6379096f315034a5259c`) reduced to the Wave-1 core:
//!
//!   - keyed by **pane id** (the leaf surface that emitted the OSC);
//!   - **replace-dedup**: a new notification for a pane replaces that pane's prior
//!     one (cmux removes existing same-key entries before inserting the new at the
//!     front — so a pane holds at most its latest notification);
//!   - **unread count**: number of panes whose latest notification is unread;
//!   - **ring state**: a pane "has a ring" while it holds an unread notification.
//!     This is the event surface P5b (Wave-3) consumes to draw the pane ring +
//!     sidebar highlight, and [`clear`](NotificationManager::clear) is the
//!     focus/click hook that turns the ring off.
//!
//! Arrival order uses a monotonic `seq` counter (not a wall clock) so ordering is
//! deterministic and the type stays testable without a clock dependency.

use std::collections::HashMap;
use std::sync::Mutex;

use serde::Serialize;
use uuid::Uuid;

use super::osc::ParsedNotification;

/// Pane identifier. String to match the PTY/coalescer track (frontend passes the
/// pane's UUID as a string over IPC).
pub type PaneKey = String;

/// A stored notification, normalized and stamped with an id + arrival order.
/// `camelCase` wire format so the frontend (P5b ring/sidebar) consumes it directly.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Notification {
    pub id: Uuid,
    pub pane_id: PaneKey,
    pub title: String,
    pub subtitle: String,
    pub body: String,
    /// Monotonic arrival order across all panes (newer = larger).
    pub seq: u64,
    pub is_read: bool,
}

/// Per-pane ring/unread snapshot for the UI (P5b subscribes to these).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PaneRingState {
    pub pane_id: PaneKey,
    /// True while the pane holds an unread notification (draw the ring).
    pub has_ring: bool,
    pub latest: Option<Notification>,
}

#[derive(Default)]
struct Inner {
    /// At most one (the latest) notification per pane — replace-dedup.
    latest: HashMap<PaneKey, Notification>,
    next_seq: u64,
}

/// `.manage()`d notification state. Thread-safe; command handlers call it from
/// arbitrary IPC threads.
#[derive(Default)]
pub struct NotificationManager {
    inner: Mutex<Inner>,
}

impl NotificationManager {
    pub fn new() -> Self {
        Self::default()
    }

    /// Record a parsed notification for `pane`, replacing any prior one for that
    /// pane (dedup). The new notification starts **unread** (ring on). Returns the
    /// stored notification (with assigned id + seq).
    pub fn record(&self, pane: PaneKey, parsed: ParsedNotification) -> Notification {
        let mut inner = self.inner.lock().unwrap();
        let seq = inner.next_seq;
        inner.next_seq += 1;
        let notification = Notification {
            id: Uuid::new_v4(),
            pane_id: pane.clone(),
            title: parsed.title,
            subtitle: parsed.subtitle,
            body: parsed.body,
            seq,
            is_read: false,
        };
        inner.latest.insert(pane, notification.clone());
        notification
    }

    /// The pane's latest notification, if any.
    pub fn latest(&self, pane: &str) -> Option<Notification> {
        self.inner.lock().unwrap().latest.get(pane).cloned()
    }

    /// Number of panes whose latest notification is unread (drives the global
    /// unread badge). Each pane contributes at most one (replace-dedup).
    pub fn unread_count(&self) -> usize {
        self.inner
            .lock()
            .unwrap()
            .latest
            .values()
            .filter(|n| !n.is_read)
            .count()
    }

    /// Whether the pane currently shows a ring (holds an unread notification).
    pub fn has_ring(&self, pane: &str) -> bool {
        self.inner
            .lock()
            .unwrap()
            .latest
            .get(pane)
            .map(|n| !n.is_read)
            .unwrap_or(false)
    }

    /// Mark the pane's notification read (ring off, but keep it in history).
    /// Returns whether anything changed.
    pub fn mark_read(&self, pane: &str) -> bool {
        let mut inner = self.inner.lock().unwrap();
        match inner.latest.get_mut(pane) {
            Some(n) if !n.is_read => {
                n.is_read = true;
                true
            }
            _ => false,
        }
    }

    /// Drop the pane's notification entirely (clear-on-focus — the P5b hook for
    /// turning the ring off when a pane is focused/clicked). Returns whether
    /// anything was removed.
    pub fn clear(&self, pane: &str) -> bool {
        self.inner.lock().unwrap().latest.remove(pane).is_some()
    }

    /// Drop every pane's notification.
    pub fn clear_all(&self) {
        self.inner.lock().unwrap().latest.clear();
    }

    /// Ring/unread snapshot for one pane (UI subscribe point).
    pub fn ring_state(&self, pane: &str) -> PaneRingState {
        let inner = self.inner.lock().unwrap();
        let latest = inner.latest.get(pane).cloned();
        PaneRingState {
            pane_id: pane.to_string(),
            has_ring: latest.as_ref().map(|n| !n.is_read).unwrap_or(false),
            latest,
        }
    }

    /// All current notifications, newest first (for a notifications panel).
    pub fn snapshot(&self) -> Vec<Notification> {
        let mut all: Vec<Notification> = self
            .inner
            .lock()
            .unwrap()
            .latest
            .values()
            .cloned()
            .collect();
        all.sort_by(|a, b| b.seq.cmp(&a.seq));
        all
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parsed(title: &str, body: &str) -> ParsedNotification {
        ParsedNotification {
            title: title.into(),
            subtitle: String::new(),
            body: body.into(),
        }
    }

    #[test]
    fn record_sets_unread_and_ring() {
        let mgr = NotificationManager::new();
        let n = mgr.record("pane-a".into(), parsed("T", "B"));
        assert!(!n.is_read);
        assert_eq!(mgr.unread_count(), 1);
        assert!(mgr.has_ring("pane-a"));
        assert_eq!(mgr.latest("pane-a").unwrap().body, "B");
    }

    #[test]
    fn second_notification_replaces_first_for_same_pane() {
        let mgr = NotificationManager::new();
        mgr.record("pane-a".into(), parsed("first", "1"));
        mgr.record("pane-a".into(), parsed("second", "2"));
        // Dedup: only the latest is kept, unread count stays 1.
        assert_eq!(mgr.unread_count(), 1);
        assert_eq!(mgr.latest("pane-a").unwrap().title, "second");
        assert_eq!(mgr.snapshot().len(), 1);
    }

    #[test]
    fn distinct_panes_count_independently() {
        let mgr = NotificationManager::new();
        mgr.record("pane-a".into(), parsed("A", ""));
        mgr.record("pane-b".into(), parsed("B", ""));
        assert_eq!(mgr.unread_count(), 2);
        assert!(mgr.has_ring("pane-a"));
        assert!(mgr.has_ring("pane-b"));
    }

    #[test]
    fn mark_read_turns_off_ring_but_keeps_history() {
        let mgr = NotificationManager::new();
        mgr.record("pane-a".into(), parsed("T", "B"));
        assert!(mgr.mark_read("pane-a"));
        assert!(!mgr.has_ring("pane-a"));
        assert_eq!(mgr.unread_count(), 0);
        // Still present in history.
        assert!(mgr.latest("pane-a").is_some());
        // Idempotent: second mark_read reports no change.
        assert!(!mgr.mark_read("pane-a"));
    }

    #[test]
    fn clear_removes_pane_entry() {
        let mgr = NotificationManager::new();
        mgr.record("pane-a".into(), parsed("T", "B"));
        assert!(mgr.clear("pane-a"));
        assert!(!mgr.has_ring("pane-a"));
        assert_eq!(mgr.unread_count(), 0);
        assert!(mgr.latest("pane-a").is_none());
        // Clearing an empty pane is a no-op.
        assert!(!mgr.clear("pane-a"));
    }

    #[test]
    fn clear_all_empties_every_pane() {
        let mgr = NotificationManager::new();
        mgr.record("pane-a".into(), parsed("A", ""));
        mgr.record("pane-b".into(), parsed("B", ""));
        mgr.clear_all();
        assert_eq!(mgr.unread_count(), 0);
        assert!(mgr.snapshot().is_empty());
    }

    #[test]
    fn ring_state_reports_latest_and_ring() {
        let mgr = NotificationManager::new();
        mgr.record("pane-a".into(), parsed("T", "B"));
        let state = mgr.ring_state("pane-a");
        assert!(state.has_ring);
        assert_eq!(state.latest.unwrap().title, "T");
        // An unknown pane reports no ring.
        let none = mgr.ring_state("pane-x");
        assert!(!none.has_ring);
        assert!(none.latest.is_none());
    }

    #[test]
    fn snapshot_is_newest_first() {
        let mgr = NotificationManager::new();
        mgr.record("pane-a".into(), parsed("old", ""));
        mgr.record("pane-b".into(), parsed("new", ""));
        let snap = mgr.snapshot();
        assert_eq!(snap[0].title, "new");
        assert_eq!(snap[1].title, "old");
    }
}
