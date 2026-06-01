//! Sidebar-owned managed state (P15b): the transient metadata store + the
//! default-off scan toggles.
//!
//! The frozen P2 model has per-panel maps only for git-branch / PR / ports. The
//! pushed `set_status` / `set_progress` / `log` reports have NO per-panel model
//! field, so persisting them in the `Workspace` would mean mutating the frozen
//! shape. Instead the sidebar owns a TRANSIENT store keyed by `PanelId` — it lives
//! for the session, is rebuilt from fresh pushes, and never touches the durable
//! model. That is why `report_receiver` reports them `StoredTransient` rather than
//! `Unstored`.
//!
//! Scan toggles gate the two app-driven expensive exceptions (port scan + git
//! status poll). They default OFF so the basic sidebar never pays subprocess cost;
//! a scan runner must check the gate before spawning. Persisting the toggle into
//! the P12 `Config` is a Wave-3 seam (see module-level docs in `report_receiver`).

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

use crate::model::PanelId;

use super::MetadataReport;

/// Cap on retained log lines per panel so a chatty producer can't grow the store
/// unbounded; only the most recent lines matter for the sidebar.
const LOG_TAIL_MAX: usize = 100;

/// The session-transient metadata for one panel (no durable model home).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct TransientMeta {
    /// Latest agent/process status line (`set_status`).
    pub status: Option<String>,
    /// Latest progress fraction in 0.0..=1.0 (`set_progress`).
    pub progress: Option<f64>,
    /// Bounded tail of recent log lines (`log`), oldest-first.
    pub log_tail: Vec<String>,
}

/// `.manage()`d sidebar state: the transient store + the two default-off scan
/// toggles. One mutex guards the map (writes are infrequent pushes); the toggles
/// are atomics (read on every would-be scan, no map contention).
pub struct SidebarState {
    transient: Mutex<HashMap<PanelId, TransientMeta>>,
    scan_ports: AtomicBool,
    watch_git_status: AtomicBool,
}

impl Default for SidebarState {
    fn default() -> Self {
        SidebarState {
            transient: Mutex::new(HashMap::new()),
            // Expensive app-driven scans are OFF until the user opts in.
            scan_ports: AtomicBool::new(false),
            watch_git_status: AtomicBool::new(false),
        }
    }
}

impl SidebarState {
    /// Record a transient report (status/progress/log) for a panel. Model-backed
    /// variants are a no-op here — `report_receiver` routes those to `apply_report`
    /// — so this is safe to call with any report and never creates an empty entry
    /// for a non-transient variant.
    pub fn record_transient(&self, panel_id: PanelId, report: &MetadataReport) {
        // Match first so a model-backed variant does not create an empty entry.
        let mut map = self
            .transient
            .lock()
            .expect("sidebar transient mutex poisoned");
        match report {
            MetadataReport::Status(s) => map.entry(panel_id).or_default().status = Some(s.clone()),
            MetadataReport::Progress(p) => map.entry(panel_id).or_default().progress = Some(*p),
            MetadataReport::Log(line) => {
                let entry = map.entry(panel_id).or_default();
                entry.log_tail.push(line.clone());
                if entry.log_tail.len() > LOG_TAIL_MAX {
                    let overflow = entry.log_tail.len() - LOG_TAIL_MAX;
                    entry.log_tail.drain(0..overflow);
                }
            }
            // Model-backed — not held here (no entry created).
            MetadataReport::GitBranch(_)
            | MetadataReport::PullRequest(_)
            | MetadataReport::Ports(_) => {}
        }
    }

    /// Snapshot of a panel's transient metadata (clone — the caller never holds the
    /// lock, preserving the snapshot boundary).
    pub fn transient_for(&self, panel_id: PanelId) -> Option<TransientMeta> {
        self.transient
            .lock()
            .expect("sidebar transient mutex poisoned")
            .get(&panel_id)
            .cloned()
    }

    /// Drop a panel's transient metadata (panel closed). No-op if absent.
    pub fn forget_panel(&self, panel_id: PanelId) {
        self.transient
            .lock()
            .expect("sidebar transient mutex poisoned")
            .remove(&panel_id);
    }

    // --- expensive-scan toggles (default-off) ---

    /// Whether the app-driven port scan may run (default false).
    pub fn port_scan_enabled(&self) -> bool {
        self.scan_ports.load(Ordering::Relaxed)
    }

    /// Enable/disable the app-driven port scan.
    pub fn set_port_scan(&self, enabled: bool) {
        self.scan_ports.store(enabled, Ordering::Relaxed);
    }

    /// Whether the app-driven git-status poll may run (default false).
    pub fn git_watch_enabled(&self) -> bool {
        self.watch_git_status.load(Ordering::Relaxed)
    }

    /// Enable/disable the app-driven git-status poll.
    pub fn set_git_watch(&self, enabled: bool) {
        self.watch_git_status.store(enabled, Ordering::Relaxed);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    #[test]
    fn scan_toggles_default_off() {
        let state = SidebarState::default();
        assert!(!state.port_scan_enabled());
        assert!(!state.git_watch_enabled());
    }

    #[test]
    fn scan_toggles_flip_independently() {
        let state = SidebarState::default();
        state.set_port_scan(true);
        assert!(state.port_scan_enabled());
        assert!(!state.git_watch_enabled());
        state.set_git_watch(true);
        state.set_port_scan(false);
        assert!(!state.port_scan_enabled());
        assert!(state.git_watch_enabled());
    }

    #[test]
    fn records_status_and_progress_for_panel() {
        let state = SidebarState::default();
        let pid = Uuid::new_v4();
        state.record_transient(pid, &MetadataReport::Status("running".into()));
        state.record_transient(pid, &MetadataReport::Progress(0.5));
        let meta = state.transient_for(pid).unwrap();
        assert_eq!(meta.status.as_deref(), Some("running"));
        assert_eq!(meta.progress, Some(0.5));
        assert!(meta.log_tail.is_empty());
    }

    #[test]
    fn latest_status_overwrites_previous() {
        let state = SidebarState::default();
        let pid = Uuid::new_v4();
        state.record_transient(pid, &MetadataReport::Status("starting".into()));
        state.record_transient(pid, &MetadataReport::Status("done".into()));
        assert_eq!(state.transient_for(pid).unwrap().status.as_deref(), Some("done"));
    }

    #[test]
    fn log_appends_and_caps_to_tail_max() {
        let state = SidebarState::default();
        let pid = Uuid::new_v4();
        for i in 0..(LOG_TAIL_MAX + 25) {
            state.record_transient(pid, &MetadataReport::Log(format!("line {i}")));
        }
        let meta = state.transient_for(pid).unwrap();
        assert_eq!(meta.log_tail.len(), LOG_TAIL_MAX);
        // Oldest dropped; newest retained.
        assert_eq!(meta.log_tail.last().unwrap(), &format!("line {}", LOG_TAIL_MAX + 24));
        assert_eq!(meta.log_tail.first().unwrap(), &format!("line {}", 25));
    }

    #[test]
    fn model_backed_variants_are_not_recorded_as_transient() {
        let state = SidebarState::default();
        let pid = Uuid::new_v4();
        state.record_transient(pid, &MetadataReport::GitBranch("main".into()));
        state.record_transient(pid, &MetadataReport::Ports(vec![3000]));
        // No transient entry was created for purely model-backed pushes.
        assert!(state.transient_for(pid).is_none());
    }

    #[test]
    fn forget_panel_drops_transient() {
        let state = SidebarState::default();
        let pid = Uuid::new_v4();
        state.record_transient(pid, &MetadataReport::Progress(0.9));
        assert!(state.transient_for(pid).is_some());
        state.forget_panel(pid);
        assert!(state.transient_for(pid).is_none());
    }
}
