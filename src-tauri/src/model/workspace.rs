//! Workspace + store (P2 Keystone 4).
//!
//! A `Workspace` is one sidebar row = one whole split-tree. `WorkspaceStore` is
//! the ordered container (cmux's `TabManager`, renamed). Types only — mutation
//! logic (add/remove/reorder/select, persistence) lives in P12.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use super::layout::LayoutNode;
use super::panel::Panel;
use super::{GroupId, PanelId, WorkspaceId};

/// SSH/remote transport kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RemoteTransport {
    Ssh,
    Websocket,
}

/// Remote attributes for a workspace (invariant #3). Present only on remote
/// workspaces; `None` for local. Defined now even though P10 fills it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RemoteConfiguration {
    pub transport: RemoteTransport,
    /// e.g. `user@host`.
    pub destination: String,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub port: Option<u16>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub identity_file: Option<String>,
    #[serde(default)]
    pub ssh_options: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub local_proxy_port: Option<u16>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub terminal_startup_command: Option<String>,
}

/// Sidebar grouping (invariant: only the field is pinned here; grouping LOGIC —
/// cwd auto-group, render flattening — is P15).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WorkspaceGroup {
    pub id: GroupId,
    pub name: String,
    #[serde(default)]
    pub is_collapsed: bool,
    #[serde(default)]
    pub is_pinned: bool,
    /// Closing this workspace dissolves the group; it renders AS the header.
    pub anchor_workspace_id: WorkspaceId,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub custom_color: Option<String>,
}

/// One sidebar row = one split-tree of panes.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Workspace {
    pub id: WorkspaceId,
    /// Process-derived title (auto).
    pub title: String,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub custom_title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub custom_description: Option<String>,
    #[serde(default)]
    pub is_pinned: bool,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub group_id: Option<GroupId>,
    /// Hex `#RRGGBB`.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub custom_color: Option<String>,
    /// Live cwd (OSC7 / shell-integration driven).
    pub current_directory: String,

    /// Layout: split-tree of ids (invariant #1).
    pub layout: LayoutNode,
    /// Flat content registry (invariant #1).
    pub panels: HashMap<PanelId, Panel>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub focused_panel_id: Option<PanelId>,

    /// Per-panel metadata maps (shape frozen at P2; P15b fills via CLI/socket
    /// push). Keyed by PanelId so sidebar can show per-surface git/PR/ports
    /// without a mid-wave contract change. Empty until enrichment lands.
    #[serde(default)]
    pub panel_git_branches: HashMap<PanelId, String>,
    #[serde(default)]
    pub panel_pull_requests: HashMap<PanelId, String>,
    #[serde(default)]
    pub panel_listening_ports: HashMap<PanelId, Vec<u16>>,

    /// Remote attributes (invariant #3); `None` = local workspace.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub remote_configuration: Option<RemoteConfiguration>,
}

impl Workspace {
    /// A new local workspace with one terminal pane.
    pub fn new_local(
        id: WorkspaceId,
        title: impl Into<String>,
        cwd: impl Into<String>,
        first_panel: Panel,
    ) -> Self {
        let panel_id = first_panel.id;
        let mut panels = HashMap::new();
        panels.insert(panel_id, first_panel);
        Workspace {
            id,
            title: title.into(),
            custom_title: None,
            custom_description: None,
            is_pinned: false,
            group_id: None,
            custom_color: None,
            current_directory: cwd.into(),
            layout: LayoutNode::leaf(panel_id),
            panels,
            focused_panel_id: Some(panel_id),
            panel_git_branches: HashMap::new(),
            panel_pull_requests: HashMap::new(),
            panel_listening_ports: HashMap::new(),
            remote_configuration: None,
        }
    }
}

/// Ordered container of workspaces + groups (cmux `TabManager`, renamed).
/// Mutation logic lives in P12; this is the shape only.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct WorkspaceStore {
    /// Ordered; array order == sidebar order.
    pub workspaces: Vec<Workspace>,
    #[serde(default)]
    pub groups: Vec<WorkspaceGroup>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub selected_index: Option<usize>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::panel::Panel;
    use uuid::Uuid;

    #[test]
    fn new_local_workspace_has_one_terminal_pane() {
        let wid = Uuid::new_v4();
        let pid = Uuid::new_v4();
        let ws = Workspace::new_local(wid, "title", "/cwd", Panel::terminal(pid));
        assert_eq!(ws.id, wid);
        assert_eq!(ws.focused_panel_id, Some(pid));
        assert_eq!(ws.layout.panel_ids(), vec![pid]);
        assert!(ws.panels.contains_key(&pid));
        assert!(ws.remote_configuration.is_none());
    }

    #[test]
    fn workspace_store_serde_round_trip() {
        let wid = Uuid::new_v4();
        let pid = Uuid::new_v4();
        let store = WorkspaceStore {
            workspaces: vec![Workspace::new_local(wid, "w", "/c", Panel::terminal(pid))],
            groups: vec![],
            selected_index: Some(0),
        };
        let json = serde_json::to_string(&store).unwrap();
        let back: WorkspaceStore = serde_json::from_str(&json).unwrap();
        assert_eq!(store, back);
    }
}
