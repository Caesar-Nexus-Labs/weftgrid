// Workspace data-model contract — TS mirror of src-tauri/src/model (P2 Keystone 4).
//
// Wire format matches serde: LayoutNode is internally-tagged on `type`;
// snapshot DTOs are camelCase. UI tracks (P4 panes, P15 sidebar) import from here.
//
// Vocabulary: Workspace / Pane / Surface (no "Tab"). Invariants:
// 1. Tree-of-ids + flat panel registry (layout holds ids; content in `panels`).
// 2. Binary split tree (exactly 2 children; divider 0.1..0.9).
// 3. `remoteConfiguration?` optional on every workspace.
// 4. Snapshot DTOs are flat + immutable (UI rows take these as props, never a store ref).

export type WorkspaceId = string;
export type PaneId = string;
export type PanelId = string;
export type GroupId = string;

export type SplitOrientation = "horizontal" | "vertical";

/** Binary split-tree node (invariant #1, #2). */
export type LayoutNode =
  | { type: "pane"; panel_ids: PanelId[]; selected_panel_id?: PanelId }
  | {
      type: "split";
      orientation: SplitOrientation;
      divider_position: number; // clamped 0.1..0.9
      first: LayoutNode;
      second: LayoutNode;
    };

export const DIVIDER_MIN = 0.1;
export const DIVIDER_MAX = 0.9;

/** Clamp a raw divider fraction into the legal range (invariant #2). */
export function clampDivider(position: number): number {
  return Math.min(DIVIDER_MAX, Math.max(DIVIDER_MIN, position));
}

export type PanelType = "terminal" | "browser";

export type PanelPayload =
  | { kind: "terminal"; working_directory?: string; shell?: string; scrollback?: string; tty_name?: string }
  | { kind: "browser"; url?: string };

/** A single surface's content + presentation state (flat registry value). */
export interface Panel {
  id: PanelId;
  panel_type: PanelType;
  title?: string;
  custom_title?: string;
  directory?: string;
  is_pinned: boolean;
  is_manually_unread: boolean;
  has_unread_indicator: boolean;
  payload: PanelPayload;
}

export type RemoteTransport = "ssh" | "websocket";

export interface RemoteConfiguration {
  transport: RemoteTransport;
  destination: string;
  port?: number;
  identity_file?: string;
  ssh_options: string[];
  local_proxy_port?: number;
  terminal_startup_command?: string;
}

export interface WorkspaceGroup {
  id: GroupId;
  name: string;
  is_collapsed: boolean;
  is_pinned: boolean;
  anchor_workspace_id: WorkspaceId;
  custom_color?: string;
}

/** One sidebar row = one split-tree of panes. */
export interface Workspace {
  id: WorkspaceId;
  title: string;
  custom_title?: string;
  custom_description?: string;
  is_pinned: boolean;
  group_id?: GroupId;
  custom_color?: string;
  current_directory: string;
  layout: LayoutNode;
  panels: Record<PanelId, Panel>;
  focused_panel_id?: PanelId;
  // Per-panel metadata maps (shape frozen at P2; P15b fills). Keyed by PanelId.
  panel_git_branches: Record<PanelId, string>;
  panel_pull_requests: Record<PanelId, string>;
  panel_listening_ports: Record<PanelId, number[]>;
  remote_configuration?: RemoteConfiguration;
}

export interface WorkspaceStore {
  workspaces: Workspace[];
  groups: WorkspaceGroup[];
  selected_index?: number;
}

// --- Snapshot DTOs (camelCase wire format; immutable UI props, invariant #4) ---

/** Immutable per-workspace snapshot for the sidebar. */
export interface WorkspaceSnapshot {
  id: WorkspaceId;
  title: string;
  customDescription?: string;
  isPinned: boolean;
  customColorHex?: string;
  currentDirectory: string;
  unreadCount: number;
  // metadata enrichment (P5 / P10 / P15b fill; shape frozen now)
  latestNotificationText?: string;
  remoteConnectionStatusText?: string;
  listeningPorts: number[];
  gitBranchSummary?: string;
  pullRequestRows: string[];
  progress?: number;
  latestConversationMessage?: string;
}

/** Immutable per-panel snapshot for pane rendering. */
export interface PanelSnapshot {
  id: PanelId;
  panelType: PanelType;
  displayTitle?: string;
  isPinned: boolean;
  hasUnreadIndicator: boolean;
}
