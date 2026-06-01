//! WorkspaceStore mutation logic (P12a).
//!
//! The `WorkspaceStore` *shape* is defined in `model/` (P2); this module owns the
//! *behavior* the sidebar (P15) and panes (P4) drive over Tauri commands:
//! add/remove/reorder/select workspace + group membership, plus the immutable
//! `WorkspaceSnapshot` DTOs the UI renders from (invariant #4 — UI never holds a
//! live store ref). Pure transformations on the borrowed store; persistence is the
//! caller's concern (`store`/`session`).

use crate::model::{Workspace, WorkspaceId, WorkspaceSnapshot, WorkspaceStore};

/// Append a workspace to the end and select it. Returns its index.
pub fn add(store: &mut WorkspaceStore, workspace: Workspace) -> usize {
    store.workspaces.push(workspace);
    let idx = store.workspaces.len() - 1;
    store.selected_index = Some(idx);
    idx
}

/// Remove the workspace with `id`. Selection is clamped to a valid neighbour
/// (or cleared when the store empties). Returns the removed workspace.
pub fn remove(store: &mut WorkspaceStore, id: WorkspaceId) -> Option<Workspace> {
    let pos = store.workspaces.iter().position(|w| w.id == id)?;
    let removed = store.workspaces.remove(pos);
    store.selected_index = reselect_after_remove(store.workspaces.len(), store.selected_index, pos);
    Some(removed)
}

/// Move the workspace at `from` to `to`, shifting the rest. Selection follows the
/// moved workspace so the user keeps looking at the same row. No-op on bad index.
pub fn reorder(store: &mut WorkspaceStore, from: usize, to: usize) -> bool {
    let len = store.workspaces.len();
    if from >= len || to >= len || from == to {
        return false;
    }
    let ws = store.workspaces.remove(from);
    store.workspaces.insert(to, ws);
    if store.selected_index == Some(from) {
        store.selected_index = Some(to);
    }
    true
}

/// Select the workspace with `id`. Returns false (selection unchanged) if absent.
pub fn select(store: &mut WorkspaceStore, id: WorkspaceId) -> bool {
    match store.workspaces.iter().position(|w| w.id == id) {
        Some(idx) => {
            store.selected_index = Some(idx);
            true
        }
        None => false,
    }
}

/// Set (or clear with `None`) a workspace's group membership.
pub fn set_group(
    store: &mut WorkspaceStore,
    id: WorkspaceId,
    group_id: Option<crate::model::GroupId>,
) -> bool {
    match store.workspaces.iter_mut().find(|w| w.id == id) {
        Some(ws) => {
            ws.group_id = group_id;
            true
        }
        None => false,
    }
}

/// Build the flat snapshot list (sidebar order) the UI renders from.
pub fn snapshot(store: &WorkspaceStore) -> Vec<WorkspaceSnapshot> {
    store.workspaces.iter().map(snapshot_one).collect()
}

fn snapshot_one(ws: &Workspace) -> WorkspaceSnapshot {
    let unread_count = ws
        .panels
        .values()
        .filter(|p| p.has_unread_indicator || p.is_manually_unread)
        .count() as u32;
    // Maps are HashMaps (non-deterministic iteration); sort + dedup so the snapshot
    // is stable across builds/runs (UI diffing + tests must not flake on map order).
    let mut listening_ports: Vec<u16> = ws
        .panel_listening_ports
        .values()
        .flatten()
        .copied()
        .collect();
    listening_ports.sort_unstable();
    listening_ports.dedup();
    // PR rows: distinct across panels, sorted for determinism.
    let mut pull_request_rows: Vec<String> = ws.panel_pull_requests.values().cloned().collect();
    pull_request_rows.sort();
    pull_request_rows.dedup();
    WorkspaceSnapshot {
        id: ws.id,
        title: ws.custom_title.clone().unwrap_or_else(|| ws.title.clone()),
        custom_description: ws.custom_description.clone(),
        is_pinned: ws.is_pinned,
        custom_color_hex: ws.custom_color.clone(),
        current_directory: ws.current_directory.clone(),
        unread_count,
        // Producers below fill these; ones with no producer in scope stay None:
        //   - latest_notification_text  ← P5 notification manager (separate path)
        //   - remote_connection_status_text ← P10 SSH status (separate path)
        //   - progress ← sidebar transient store overlay (sidebar track owns it)
        //   - latest_conversation_message ← agent conversation (separate path)
        latest_notification_text: None,
        remote_connection_status_text: None,
        listening_ports,
        git_branch_summary: git_branch_summary(ws),
        pull_request_rows,
        progress: None,
        latest_conversation_message: None,
    }
}

/// The branch shown on the sidebar row: the focused panel's branch (the surface
/// the user is looking at). Multi-panel aggregation beyond the focused surface is
/// deferred — a row reflects its focused pane. `None` when the focused panel has no
/// reported branch (or there is no focused panel).
fn git_branch_summary(ws: &Workspace) -> Option<String> {
    ws.focused_panel_id
        .and_then(|pid| ws.panel_git_branches.get(&pid))
        .cloned()
}

/// New selection after removing the workspace at `removed_pos` from a store that
/// now has `new_len` workspaces. Keeps the cursor near where it was.
fn reselect_after_remove(
    new_len: usize,
    selected: Option<usize>,
    removed_pos: usize,
) -> Option<usize> {
    if new_len == 0 {
        return None;
    }
    match selected {
        Some(sel) if sel == removed_pos => Some(removed_pos.min(new_len - 1)),
        Some(sel) if sel > removed_pos => Some(sel - 1),
        other => other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Panel;
    use uuid::Uuid;

    fn ws(title: &str) -> Workspace {
        Workspace::new_local(
            Uuid::new_v4(),
            title,
            "/cwd",
            Panel::terminal(Uuid::new_v4()),
        )
    }

    #[test]
    fn add_appends_and_selects() {
        let mut store = WorkspaceStore::default();
        let a = add(&mut store, ws("a"));
        assert_eq!(a, 0);
        let b = add(&mut store, ws("b"));
        assert_eq!(b, 1);
        assert_eq!(store.selected_index, Some(1));
        assert_eq!(store.workspaces.len(), 2);
    }

    #[test]
    fn remove_clamps_selection() {
        let mut store = WorkspaceStore::default();
        let w0 = ws("a");
        let id0 = w0.id;
        add(&mut store, w0);
        add(&mut store, ws("b"));
        add(&mut store, ws("c"));
        store.selected_index = Some(2);
        // Remove first → selection shifts down to keep pointing at same row set.
        remove(&mut store, id0);
        assert_eq!(store.workspaces.len(), 2);
        assert_eq!(store.selected_index, Some(1));
    }

    #[test]
    fn remove_last_remaining_clears_selection() {
        let mut store = WorkspaceStore::default();
        let w = ws("only");
        let id = w.id;
        add(&mut store, w);
        remove(&mut store, id);
        assert!(store.workspaces.is_empty());
        assert_eq!(store.selected_index, None);
    }

    #[test]
    fn reorder_moves_and_follows_selection() {
        let mut store = WorkspaceStore::default();
        let first = ws("a");
        let first_id = first.id;
        add(&mut store, first);
        add(&mut store, ws("b"));
        add(&mut store, ws("c"));
        store.selected_index = Some(0);
        assert!(reorder(&mut store, 0, 2));
        // Order is now b, c, a; selection follows the moved workspace.
        assert_eq!(store.workspaces[2].id, first_id);
        assert_eq!(store.selected_index, Some(2));
    }

    #[test]
    fn reorder_rejects_bad_index() {
        let mut store = WorkspaceStore::default();
        add(&mut store, ws("a"));
        assert!(!reorder(&mut store, 0, 5));
        assert!(!reorder(&mut store, 0, 0));
    }

    #[test]
    fn select_by_id_updates_index() {
        let mut store = WorkspaceStore::default();
        add(&mut store, ws("a"));
        let target = ws("b");
        let target_id = target.id;
        add(&mut store, target);
        assert!(select(&mut store, target_id));
        assert_eq!(store.selected_index, Some(1));
        assert!(!select(&mut store, Uuid::new_v4()));
    }

    #[test]
    fn set_group_assigns_and_clears() {
        let mut store = WorkspaceStore::default();
        let w = ws("a");
        let id = w.id;
        add(&mut store, w);
        let gid = Uuid::new_v4();
        assert!(set_group(&mut store, id, Some(gid)));
        assert_eq!(store.workspaces[0].group_id, Some(gid));
        assert!(set_group(&mut store, id, None));
        assert_eq!(store.workspaces[0].group_id, None);
    }

    #[test]
    fn snapshot_matches_order_and_uses_custom_title() {
        let mut store = WorkspaceStore::default();
        let mut w = ws("auto-title");
        w.custom_title = Some("My WS".to_string());
        add(&mut store, w);
        add(&mut store, ws("second"));
        let snaps = snapshot(&store);
        assert_eq!(snaps.len(), 2);
        assert_eq!(snaps[0].title, "My WS");
        assert_eq!(snaps[1].title, "second");
    }

    #[test]
    fn snapshot_surfaces_focused_panel_git_branch() {
        let mut store = WorkspaceStore::default();
        let mut w = ws("repo");
        let focused = w.focused_panel_id.expect("new_local sets focus");
        w.panel_git_branches.insert(focused, "feature/x".into());
        // A non-focused panel's branch must NOT win the row summary.
        w.panel_git_branches.insert(Uuid::new_v4(), "other".into());
        add(&mut store, w);
        let snaps = snapshot(&store);
        assert_eq!(snaps[0].git_branch_summary.as_deref(), Some("feature/x"));
    }

    #[test]
    fn snapshot_git_branch_none_when_focused_panel_unreported() {
        let mut store = WorkspaceStore::default();
        let w = ws("repo"); // focused panel has no reported branch
        add(&mut store, w);
        assert!(snapshot(&store)[0].git_branch_summary.is_none());
    }

    #[test]
    fn snapshot_ports_sorted_deduped_deterministic() {
        let mut store = WorkspaceStore::default();
        let mut w = ws("ports");
        let p1 = w.focused_panel_id.unwrap();
        let p2 = Uuid::new_v4();
        w.panel_listening_ports.insert(p1, vec![5173, 3000]);
        w.panel_listening_ports.insert(p2, vec![3000, 8080]); // 3000 duplicated across panels
        add(&mut store, w);
        // Sorted + deduped regardless of HashMap iteration order.
        assert_eq!(snapshot(&store)[0].listening_ports, vec![3000, 5173, 8080]);
    }

    #[test]
    fn snapshot_pull_request_rows_sorted_deduped() {
        let mut store = WorkspaceStore::default();
        let mut w = ws("prs");
        let p1 = w.focused_panel_id.unwrap();
        let p2 = Uuid::new_v4();
        let p3 = Uuid::new_v4();
        w.panel_pull_requests.insert(p1, "#12 fix".into());
        w.panel_pull_requests.insert(p2, "#3 feat".into());
        // Two panels referencing the SAME PR must collapse to one row (dedup).
        w.panel_pull_requests.insert(p3, "#3 feat".into());
        add(&mut store, w);
        // Distinct rows, deterministic order, the duplicate "#3 feat" appears once.
        assert_eq!(
            snapshot(&store)[0].pull_request_rows,
            vec!["#12 fix", "#3 feat"]
        );
    }
}
