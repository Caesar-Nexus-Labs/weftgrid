//! Panel = a single surface's content (P2 Keystone 4, invariant #1).
//!
//! Panels live in the flat `Workspace.panels` registry, keyed by `PanelId`. The
//! layout tree references them by id only. Per-type payload is a discriminated
//! union on `panel_type`.

use serde::{Deserialize, Serialize};

use super::PanelId;

/// Kind of surface a panel hosts. Open set — terminal/browser are MVP; others
/// (markdown, file-preview, project) can be added without breaking the contract.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PanelType {
    Terminal,
    Browser,
}

/// Terminal-surface payload (P3 produces, P12 persists).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct TerminalPayload {
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub working_directory: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub shell: Option<String>,
    /// Captured scrollback for session restore (best-effort).
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub scrollback: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub tty_name: Option<String>,
}

/// Browser-surface payload (P6 produces, P12 persists).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct BrowserPayload {
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub url: Option<String>,
}

/// Per-type payload (discriminated by the panel's `panel_type`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum PanelPayload {
    Terminal(TerminalPayload),
    Browser(BrowserPayload),
}

/// A single surface's content + presentation state, stored in the flat registry.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Panel {
    pub id: PanelId,
    pub panel_type: PanelType,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub custom_title: Option<String>,
    /// Live cwd shown in the UI (OSC7 / shell-integration driven).
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub directory: Option<String>,
    #[serde(default)]
    pub is_pinned: bool,
    /// User toggled unread (sticks until viewed).
    #[serde(default)]
    pub is_manually_unread: bool,
    /// Derived unread indicator (e.g. from a notification while unfocused).
    #[serde(default)]
    pub has_unread_indicator: bool,
    pub payload: PanelPayload,
}

impl Panel {
    /// A fresh terminal panel with default payload.
    pub fn terminal(id: PanelId) -> Self {
        Panel {
            id,
            panel_type: PanelType::Terminal,
            title: None,
            custom_title: None,
            directory: None,
            is_pinned: false,
            is_manually_unread: false,
            has_unread_indicator: false,
            payload: PanelPayload::Terminal(TerminalPayload::default()),
        }
    }
}
