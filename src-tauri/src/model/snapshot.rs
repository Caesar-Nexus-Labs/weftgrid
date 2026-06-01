//! Flat, immutable snapshot DTOs for IPC (P2 Keystone 4, invariant #4).
//!
//! UI rows render from these value snapshots, NEVER from a live store reference —
//! this is the perf contract that prevents re-render storms (a row holding a
//! reactive store ref re-renders on every unrelated store change). The Rust core
//! produces snapshots; the Svelte UI consumes them as plain `$state` props.
//!
//! Metadata-enrichment fields (git/PR/ports/status/progress/conversation) are
//! declared NOW even though P5/P10/P15b fill them, so the DTO shape is frozen and
//! parallel tracks never need a mid-wave contract change.

use serde::{Deserialize, Serialize};

use super::panel::PanelType;
use super::{PanelId, WorkspaceId};

/// Immutable per-workspace snapshot for the sidebar (cmux `SidebarWorkspaceSnapshot`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceSnapshot {
    pub id: WorkspaceId,
    pub title: String,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub custom_description: Option<String>,
    pub is_pinned: bool,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub custom_color_hex: Option<String>,
    pub current_directory: String,
    pub unread_count: u32,

    // --- metadata enrichment (P5 / P10 / P15b fill; shape frozen now) ---
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub latest_notification_text: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub remote_connection_status_text: Option<String>,
    #[serde(default)]
    pub listening_ports: Vec<u16>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub git_branch_summary: Option<String>,
    #[serde(default)]
    pub pull_request_rows: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub progress: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub latest_conversation_message: Option<String>,
}

/// Immutable per-panel snapshot for pane rendering.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PanelSnapshot {
    pub id: PanelId,
    pub panel_type: PanelType,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub display_title: Option<String>,
    pub is_pinned: bool,
    pub has_unread_indicator: bool,
}
