//! Workspace data-model contract (P2 Keystone 4).
//!
//! Shared types every UI/persistence/SSH track builds against. Pinned EARLY so
//! P4 (panes), P10 (SSH), P12 (persistence/store), P15 (sidebar) consume one
//! contract instead of refactoring later.
//!
//! Vocabulary (drop cmux's overloaded "Tab"): **Workspace / Pane / Surface**.
//! - Workspace = one sidebar row = one whole split-tree.
//! - Pane = a leaf of the split-tree; stacks one or more Surfaces (in-pane tab bar).
//! - Surface = a single terminal/browser content instance (= a Panel).
//!
//! Invariants (must hold everywhere):
//! 1. Tree-of-ids + flat panel-registry: `LayoutNode` stores only PanelIds; content
//!    `Panel` objects live in `Workspace.panels` (flat map). Layout mutation is
//!    decoupled from content lifecycle.
//! 2. Binary split tree: a split node has EXACTLY two children; divider in 0.1..=0.9.
//! 3. `remote_configuration` is optional on every workspace (SSH = an attribute,
//!    not a separate type) — present even if unused until P10.
//! 4. Snapshot DTOs (see `snapshot`) are flat + immutable, separate from the live
//!    model, so UI rows never hold a reactive store reference (perf: avoids the
//!    re-render storm class of bug).

pub mod layout;
pub mod panel;
pub mod snapshot;
pub mod workspace;

// P2 contract surface — consumers arrive in Wave-1/2 (P3/P4/P12/P15). Re-exported
// now so tracks import from one place; allow dead_code until then.
#[allow(unused_imports)]
pub use layout::{LayoutNode, SplitOrientation};
#[allow(unused_imports)]
pub use panel::{BrowserPayload, Panel, PanelPayload, PanelType, TerminalPayload};
#[allow(unused_imports)]
pub use snapshot::{PanelSnapshot, WorkspaceSnapshot};
#[allow(unused_imports)]
pub use workspace::{
    RemoteConfiguration, RemoteTransport, Workspace, WorkspaceGroup, WorkspaceStore,
};

/// Stable identifier for a workspace (sidebar row / split-tree).
pub type WorkspaceId = uuid::Uuid;
/// Stable identifier for a pane (leaf of the split-tree).
pub type PaneId = uuid::Uuid;
/// Stable identifier for a panel (a single terminal/browser surface).
pub type PanelId = uuid::Uuid;
/// Stable identifier for a workspace group (sidebar grouping).
pub type GroupId = uuid::Uuid;
