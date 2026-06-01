//! Sidebar + workspace navigation track (P15 owner: `src-tauri/src/sidebar/**`,
//! `src/sidebar/**`).
//!
//! Vertical-tab tree where each row = one Workspace (split-tree). Source of truth
//! is the P12 `WorkspaceStore`; the sidebar renders immutable snapshots (rows hold
//! plain props + closure actions, never a reactive store ref — avoids the cmux
//! re-render-storm class of bug). P15b adds metadata enrichment (git/PR/ports)
//! behind default-off toggles plus a push receiver fed by the P13 socket.
//!
//! `register` is additive-only (no `invoke_handler` — commands are listed once in
//! `command_registry` per the last-wins constraint).

use tauri::{Builder, Runtime};

use crate::model::{PanelId, Workspace};

pub mod commands;
pub mod git_probe;
pub mod port_scanner;
pub mod report_receiver;
pub mod snapshot_enrich;
pub mod state;

/// One metadata update pushed from a producer (P13 `weft` CLI / shell-integration
/// / agent over the local socket). Variant names mirror the producer verbs
/// (`report_git_branch` / `report_pr` / `report_ports` / `set_status` /
/// `set_progress` / `log`) so Wave-3 can decode the wire frame straight into this.
#[derive(Debug, Clone, PartialEq)]
pub enum MetadataReport {
    GitBranch(String),
    PullRequest(String),
    Ports(Vec<u16>),
    Status(String),
    Progress(f64),
    Log(String),
}

/// Outcome of applying a report — distinguishes fields that have a home in the
/// frozen P2 model (the per-panel maps) from ones recorded in the sidebar-owned
/// transient store (status/progress/log have no per-panel model field).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReportOutcome {
    /// Written into a `Workspace` per-panel metadata map (git branch / PR / ports).
    Stored,
    /// Recorded in the sidebar-owned transient store (status / progress / log —
    /// the frozen P2 model has no per-panel field for them).
    StoredTransient,
    /// Accepted but went nowhere: `apply_report` saw a variant with no model field
    /// (the receiver routes those to `StoredTransient` instead), or the receiver
    /// could not resolve the target workspace.
    Unstored,
}

/// Map a pushed report onto the workspace's per-panel metadata maps (P2 invariant:
/// metadata is keyed by `PanelId`). Kept PURE (mutates the borrowed workspace, no
/// IO, no managed state) so it unit-tests in isolation. The `report_receiver`
/// orchestrates the full path: it calls this for the model-backed variants and
/// routes the transient ones (status/progress/log) to the sidebar transient store.
///
/// Git branch / PR / ports have frozen per-panel maps in the model and are
/// `Stored`. Status / progress / log have no per-panel model field, so this pure
/// function reports them `Unstored` — the receiver records them in the transient
/// store (`StoredTransient`) instead of mutating the frozen model.
pub fn apply_report(
    workspace: &mut Workspace,
    panel_id: PanelId,
    report: MetadataReport,
) -> ReportOutcome {
    match report {
        MetadataReport::GitBranch(branch) => {
            workspace.panel_git_branches.insert(panel_id, branch);
            ReportOutcome::Stored
        }
        MetadataReport::PullRequest(pr) => {
            workspace.panel_pull_requests.insert(panel_id, pr);
            ReportOutcome::Stored
        }
        MetadataReport::Ports(ports) => {
            workspace.panel_listening_ports.insert(panel_id, ports);
            ReportOutcome::Stored
        }
        // No per-panel model field for these (frozen P2 shape). The pure mapper
        // leaves the model untouched and reports `Unstored`; the `report_receiver`
        // records them in the sidebar transient store (`StoredTransient`).
        MetadataReport::Status(_) | MetadataReport::Progress(_) | MetadataReport::Log(_) => {
            ReportOutcome::Unstored
        }
    }
}

/// Additive setup: `.manage()` the sidebar's `SidebarState` (the transient
/// status/progress/log store + the default-off scan toggles). P15a renders from
/// the P12 `ConfigState` snapshot; this state holds only the Wave-3 enrichment
/// that has no home in the frozen P2 model. No `invoke_handler` and no
/// `Builder::setup` (both last-wins — see `command_registry`).
pub fn register<R: Runtime>(builder: Builder<R>) -> Builder<R> {
    builder.manage(state::SidebarState::default())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Panel;
    use uuid::Uuid;

    fn workspace() -> (Workspace, PanelId) {
        let pid = Uuid::new_v4();
        let ws = Workspace::new_local(Uuid::new_v4(), "ws", "/cwd", Panel::terminal(pid));
        (ws, pid)
    }

    #[test]
    fn git_branch_report_writes_panel_map() {
        let (mut ws, pid) = workspace();
        let outcome = apply_report(&mut ws, pid, MetadataReport::GitBranch("main".into()));
        assert_eq!(outcome, ReportOutcome::Stored);
        assert_eq!(ws.panel_git_branches.get(&pid).map(String::as_str), Some("main"));
    }

    #[test]
    fn pull_request_report_writes_panel_map() {
        let (mut ws, pid) = workspace();
        apply_report(&mut ws, pid, MetadataReport::PullRequest("#12 fix".into()));
        assert_eq!(
            ws.panel_pull_requests.get(&pid).map(String::as_str),
            Some("#12 fix")
        );
    }

    #[test]
    fn ports_report_writes_panel_map() {
        let (mut ws, pid) = workspace();
        let outcome = apply_report(&mut ws, pid, MetadataReport::Ports(vec![3000, 5173]));
        assert_eq!(outcome, ReportOutcome::Stored);
        assert_eq!(ws.panel_listening_ports.get(&pid), Some(&vec![3000, 5173]));
    }

    #[test]
    fn status_progress_log_are_accepted_but_unstored_until_wave3() {
        let (mut ws, pid) = workspace();
        assert_eq!(
            apply_report(&mut ws, pid, MetadataReport::Status("running".into())),
            ReportOutcome::Unstored
        );
        assert_eq!(
            apply_report(&mut ws, pid, MetadataReport::Progress(0.5)),
            ReportOutcome::Unstored
        );
        assert_eq!(
            apply_report(&mut ws, pid, MetadataReport::Log("line".into())),
            ReportOutcome::Unstored
        );
        // None of the per-panel maps were touched by the unstored variants.
        assert!(ws.panel_git_branches.is_empty());
        assert!(ws.panel_pull_requests.is_empty());
        assert!(ws.panel_listening_ports.is_empty());
    }

    #[test]
    fn latest_git_branch_report_overwrites_previous() {
        let (mut ws, pid) = workspace();
        apply_report(&mut ws, pid, MetadataReport::GitBranch("main".into()));
        apply_report(&mut ws, pid, MetadataReport::GitBranch("dev".into()));
        assert_eq!(ws.panel_git_branches.get(&pid).map(String::as_str), Some("dev"));
    }
}
