//! Report receiver (P15b): the seam the P13 socket dispatch calls to push a
//! metadata report onto the sidebar's stores.
//!
//! The producer (`weft` CLI / shell-integration / agent) frames a `report_*` push
//! over the P13 local socket. P13 decodes the frame and hands a [`ReportFrame`] to
//! [`receive_report`] — that is the in-process SEAM this module owns (P13 calls a
//! plain Rust fn, NOT a Tauri command: the RPC server already runs in-process and
//! holds its dispatcher). Wiring agent_rpc to call this is Wave-3 (this track does
//! NOT edit agent_rpc); the decode → map → apply path below is built and tested
//! headless here.
//!
//! Routing:
//! - git-branch / PR / ports have frozen per-panel model maps → [`apply_report`]
//!   writes them (`Stored`).
//! - status / progress / log have no model field → recorded in the sidebar-owned
//!   transient store (`StoredTransient`).
//! - an unknown target workspace → `Unstored` (nothing mutated).
//!
//! Kept pure (no locking, no IO — operates on the borrowed `WorkspaceStore` +
//! `SidebarState`) so it unit-tests directly. The live wiring locks P12's
//! `ConfigState` to borrow the `WorkspaceStore` mutably (see DEFERRED note below).

use serde::{Deserialize, Serialize};

use crate::model::{PanelId, WorkspaceId, WorkspaceStore};

use super::state::SidebarState;
use super::{apply_report, MetadataReport, ReportOutcome};

/// Wire body of a `report_*` push: the report verb + its payload. Tagged on
/// `kind` (snake_case) to mirror the P13 `Command` / `BrowserAction` framing
/// style; the variant names match the producer verbs so P13 decodes a frame
/// straight into this. This is the contract P13 adopts (e.g. a future
/// `Command::Report(ReportFrame)`); P13 owns adding that protocol variant.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ReportKind {
    /// `report_git_branch <branch>`.
    GitBranch { branch: String },
    /// `report_pr <pr>`.
    Pr { pr: String },
    /// `report_ports <ports...>`.
    Ports { ports: Vec<u16> },
    /// `set_status <status>`.
    Status { status: String },
    /// `set_progress <0.0..=1.0>`.
    Progress { progress: f64 },
    /// `log <line>`.
    Log { line: String },
}

impl ReportKind {
    /// Lower the wire body into the internal [`MetadataReport`] the stores accept.
    pub fn into_report(self) -> MetadataReport {
        match self {
            ReportKind::GitBranch { branch } => MetadataReport::GitBranch(branch),
            ReportKind::Pr { pr } => MetadataReport::PullRequest(pr),
            ReportKind::Ports { ports } => MetadataReport::Ports(ports),
            ReportKind::Status { status } => MetadataReport::Status(status),
            ReportKind::Progress { progress } => MetadataReport::Progress(progress),
            ReportKind::Log { line } => MetadataReport::Log(line),
        }
    }
}

/// A full report frame: which workspace + panel it targets, plus the report body.
/// camelCase to match the snapshot DTO wire format the rest of the IPC uses.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReportFrame {
    pub workspace_id: WorkspaceId,
    pub panel_id: PanelId,
    #[serde(flatten)]
    pub kind: ReportKind,
}

/// Apply a decoded report frame onto the sidebar's stores. The SEAM P13's socket
/// dispatch calls once it lands (Wave-3).
///
/// Resolves the target workspace by id, routes model-backed variants through
/// [`apply_report`] (per-panel maps) and transient variants through the
/// [`SidebarState`] store, and returns the [`ReportOutcome`].
///
/// DEFERRED (live P13 socket): the call site that locks P12's `ConfigState`,
/// borrows the contained `WorkspaceStore` mutably + the managed `SidebarState`,
/// and invokes this. P12 exposes no metadata-mutating accessor on `ConfigState`
/// today, so wiring needs either a P12 accessor or this fn invoked behind a P12
/// command — a Wave-3 integration decision, out of this track's ownership.
pub fn receive_report(
    store: &mut WorkspaceStore,
    sidebar: &SidebarState,
    frame: ReportFrame,
) -> ReportOutcome {
    let ReportFrame {
        workspace_id,
        panel_id,
        kind,
    } = frame;
    let report = kind.into_report();

    let Some(workspace) = store.workspaces.iter_mut().find(|w| w.id == workspace_id) else {
        // Unknown target — accept the frame but record nothing (the producing
        // panel may have closed between push and delivery).
        return ReportOutcome::Unstored;
    };

    match apply_report(workspace, panel_id, report.clone()) {
        ReportOutcome::Stored => ReportOutcome::Stored,
        // No model field (status/progress/log) — the transient sidebar store is
        // their home. `apply_report` never returns `StoredTransient`; folded here
        // so the match stays exhaustive without an `unreachable!`.
        ReportOutcome::Unstored | ReportOutcome::StoredTransient => {
            sidebar.record_transient(panel_id, &report);
            ReportOutcome::StoredTransient
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Panel, Workspace};
    use uuid::Uuid;

    fn store_with_workspace() -> (WorkspaceStore, WorkspaceId, PanelId) {
        let wid = Uuid::new_v4();
        let pid = Uuid::new_v4();
        let ws = Workspace::new_local(wid, "ws", "/cwd", Panel::terminal(pid));
        let store = WorkspaceStore {
            workspaces: vec![ws],
            groups: vec![],
            selected_index: Some(0),
        };
        (store, wid, pid)
    }

    fn frame(workspace_id: WorkspaceId, panel_id: PanelId, kind: ReportKind) -> ReportFrame {
        ReportFrame {
            workspace_id,
            panel_id,
            kind,
        }
    }

    #[test]
    fn report_kind_lowers_to_matching_metadata_report() {
        assert_eq!(
            ReportKind::GitBranch { branch: "main".into() }.into_report(),
            MetadataReport::GitBranch("main".into())
        );
        assert_eq!(
            ReportKind::Pr { pr: "#7".into() }.into_report(),
            MetadataReport::PullRequest("#7".into())
        );
        assert_eq!(
            ReportKind::Ports { ports: vec![3000] }.into_report(),
            MetadataReport::Ports(vec![3000])
        );
        assert_eq!(
            ReportKind::Status { status: "run".into() }.into_report(),
            MetadataReport::Status("run".into())
        );
        assert_eq!(
            ReportKind::Progress { progress: 0.25 }.into_report(),
            MetadataReport::Progress(0.25)
        );
        assert_eq!(
            ReportKind::Log { line: "l".into() }.into_report(),
            MetadataReport::Log("l".into())
        );
    }

    #[test]
    fn frame_decodes_from_p13_wire_json() {
        let wid = Uuid::new_v4();
        let pid = Uuid::new_v4();
        let json = format!(
            r#"{{"workspaceId":"{wid}","panelId":"{pid}","kind":"git_branch","branch":"dev"}}"#
        );
        let decoded: ReportFrame = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded.workspace_id, wid);
        assert_eq!(decoded.panel_id, pid);
        assert_eq!(decoded.kind, ReportKind::GitBranch { branch: "dev".into() });
    }

    #[test]
    fn ports_frame_decode_apply_reflects_in_workspace_snapshot() {
        // End-to-end: a pushed `report_ports` lands in the model map and surfaces
        // into the snapshot. P12's snapshot sorts + dedups for determinism (the
        // model map is a HashMap — insertion order is not meaningful).
        let (mut store, wid, pid) = store_with_workspace();
        let sidebar = SidebarState::default();
        let json = format!(
            r#"{{"workspaceId":"{wid}","panelId":"{pid}","kind":"ports","ports":[5173,3000]}}"#
        );
        let frame: ReportFrame = serde_json::from_str(&json).unwrap();

        assert_eq!(receive_report(&mut store, &sidebar, frame), ReportOutcome::Stored);

        let snaps = crate::command_registry::config::workspace_store::snapshot(&store);
        assert_eq!(snaps.len(), 1);
        // Sorted ascending (deterministic), not insertion order.
        assert_eq!(snaps[0].listening_ports, vec![3000, 5173]);
    }

    #[test]
    fn git_branch_frame_writes_model_map() {
        let (mut store, wid, pid) = store_with_workspace();
        let sidebar = SidebarState::default();
        let outcome = receive_report(
            &mut store,
            &sidebar,
            frame(wid, pid, ReportKind::GitBranch { branch: "feature/x".into() }),
        );
        assert_eq!(outcome, ReportOutcome::Stored);
        assert_eq!(
            store.workspaces[0].panel_git_branches.get(&pid).map(String::as_str),
            Some("feature/x")
        );
    }

    #[test]
    fn status_progress_log_route_to_transient_store() {
        let (mut store, wid, pid) = store_with_workspace();
        let sidebar = SidebarState::default();

        assert_eq!(
            receive_report(&mut store, &sidebar, frame(wid, pid, ReportKind::Status { status: "building".into() })),
            ReportOutcome::StoredTransient
        );
        assert_eq!(
            receive_report(&mut store, &sidebar, frame(wid, pid, ReportKind::Progress { progress: 0.4 })),
            ReportOutcome::StoredTransient
        );
        assert_eq!(
            receive_report(&mut store, &sidebar, frame(wid, pid, ReportKind::Log { line: "compiling".into() })),
            ReportOutcome::StoredTransient
        );

        let meta = sidebar.transient_for(pid).unwrap();
        assert_eq!(meta.status.as_deref(), Some("building"));
        assert_eq!(meta.progress, Some(0.4));
        assert_eq!(meta.log_tail, vec!["compiling".to_string()]);
        // Transient variants never touched the durable model maps.
        assert!(store.workspaces[0].panel_git_branches.is_empty());
    }

    #[test]
    fn unknown_workspace_is_unstored_and_mutates_nothing() {
        let (mut store, _wid, pid) = store_with_workspace();
        let sidebar = SidebarState::default();
        let stranger = Uuid::new_v4();
        let outcome = receive_report(
            &mut store,
            &sidebar,
            frame(stranger, pid, ReportKind::GitBranch { branch: "main".into() }),
        );
        assert_eq!(outcome, ReportOutcome::Unstored);
        assert!(store.workspaces[0].panel_git_branches.is_empty());
        assert!(sidebar.transient_for(pid).is_none());
    }
}
