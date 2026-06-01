//! Snapshot enrichment with sidebar-owned transient metadata (P15b).
//!
//! P12's `workspace_store::snapshot` surfaces the durable model-backed fields
//! (git branch, PR rows, ports). The transient fields (`set_progress` /
//! `set_status` / `log`) live in the sidebar-owned [`SidebarState`], not the frozen
//! model — so overlaying them onto the snapshot is the sidebar track's job, not
//! P12's (keeps `config` free of a sidebar dependency).
//!
//! Of the transient fields only `progress` has a `WorkspaceSnapshot` slot today;
//! `status` / `log_tail` have no DTO field yet (a future snapshot-shape change owns
//! that). A row's progress mirrors the git-branch summary rule: the FOCUSED panel's
//! value (the surface the user is looking at), `None` when unreported.
//!
//! Kept pure (mutates the borrowed snapshot slice; reads `SidebarState` clones) so
//! it preserves the snapshot boundary and unit-tests without a live app.
//!
//! DEFERRED (live wiring): this overlay is built + tested but NOT yet called from
//! the `workspace_snapshot` Tauri command (`config/commands.rs`) — that command
//! returns the bare P12 snapshot today. Wiring it (lock `ConfigState` for the store
//! + read the managed `SidebarState`, call `overlay_transient`) is the same Wave-3
//! integration step that wires the P13 socket into `report_receiver`; both share
//! the need for a P12 accessor that hands out the store + sidebar state together.

use crate::model::{WorkspaceSnapshot, WorkspaceStore};

use super::state::SidebarState;

/// Overlay transient `progress` onto each snapshot from the focused panel's
/// transient metadata. Snapshots are matched to workspaces by id; the workspace's
/// `focused_panel_id` selects which panel's transient progress the row shows.
pub fn overlay_transient(
    snapshots: &mut [WorkspaceSnapshot],
    store: &WorkspaceStore,
    state: &SidebarState,
) {
    for snap in snapshots.iter_mut() {
        let Some(ws) = store.workspaces.iter().find(|w| w.id == snap.id) else {
            continue;
        };
        let Some(focused) = ws.focused_panel_id else {
            continue;
        };
        if let Some(meta) = state.transient_for(focused) {
            // Only overwrite when a value was actually reported, so enrichment is
            // additive over whatever the base snapshot already carried.
            if meta.progress.is_some() {
                snap.progress = meta.progress;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::command_registry::config::workspace_store::snapshot;
    use crate::command_registry::sidebar::MetadataReport;
    use crate::model::{Panel, Workspace};
    use uuid::Uuid;

    fn store_one() -> (WorkspaceStore, Uuid) {
        let pid = Uuid::new_v4();
        let ws = Workspace::new_local(Uuid::new_v4(), "ws", "/cwd", Panel::terminal(pid));
        let store = WorkspaceStore {
            workspaces: vec![ws],
            groups: vec![],
            selected_index: Some(0),
        };
        (store, pid)
    }

    #[test]
    fn overlay_sets_progress_from_focused_panel() {
        let (store, focused) = store_one();
        let state = SidebarState::default();
        state.record_transient(focused, &MetadataReport::Progress(0.42));

        let mut snaps = snapshot(&store);
        assert!(snaps[0].progress.is_none()); // base snapshot has no progress
        overlay_transient(&mut snaps, &store, &state);
        assert_eq!(snaps[0].progress, Some(0.42));
    }

    #[test]
    fn overlay_leaves_progress_none_when_unreported() {
        let (store, _focused) = store_one();
        let state = SidebarState::default();
        let mut snaps = snapshot(&store);
        overlay_transient(&mut snaps, &store, &state);
        assert!(snaps[0].progress.is_none());
    }

    #[test]
    fn overlay_ignores_non_focused_panel_progress() {
        let (store, _focused) = store_one();
        let state = SidebarState::default();
        // Progress reported for a DIFFERENT panel must not surface on the row.
        state.record_transient(Uuid::new_v4(), &MetadataReport::Progress(0.9));
        let mut snaps = snapshot(&store);
        overlay_transient(&mut snaps, &store, &state);
        assert!(snaps[0].progress.is_none());
    }
}
