// Settings store — TS client + typed mirror of the Rust config schema (P12).
//
// Thin wrappers over the Tauri command surface owned by src-tauri/src/config.
// Types mirror serde wire shapes: `Config` fields are snake_case (serde default),
// snapshot DTOs are camelCase (see $lib/model). No hand-written JS — this is the
// single typed entry the Settings UI + sidebar/palette tracks call.

import { invoke } from "@tauri-apps/api/core";
import type { WorkspaceId, WorkspaceSnapshot } from "$lib/model";

export type TabLayout = "horizontal" | "vertical";
export type RespawnPolicy = "fresh-shell" | "rerun-last-command";

/** Terminal + UI theme selection (names resolved by the UI layer). */
export interface ThemeConfig {
  ui: string;
  terminal_colors: string;
}

/** Typed settings document — mirror of Rust `config::schema::Config`. */
export interface Config {
  schema_version: number;
  tab_layout: TabLayout;
  theme: ThemeConfig;
  default_shell?: string;
  default_respawn_policy: RespawnPolicy;
  import_consent: boolean;
  /** action id -> chord (e.g. "palette.commands" -> "ctrl+shift+p"). */
  keybinding_overrides: Record<string, string>;
}

/** One (action, chord) row for the keybinding editor. */
export interface KeybindingRow {
  action: string;
  chord: string;
}

/** A pair of actions colliding on one chord. */
export interface Conflict {
  chord: string;
  action_a: string;
  action_b: string;
}

/** Result of setting a binding: any conflicts the change introduced. */
export interface KeybindingSetResult {
  conflicts: Conflict[];
}

/** Parsed weft.json command (mirror of Rust `weft_config::WeftCommand`). */
export interface WeftCommand {
  name: string;
  description?: string;
  keywords: string[];
  confirm?: boolean;
  command?: string;
  workspace?: { name?: string; cwd?: string; color?: string };
  restart?: "new" | "recreate" | "ignore" | "confirm";
}

/** Parsed weft.json document. */
export interface WeftConfig {
  commands: WeftCommand[];
  newWorkspaceCommand?: string;
}

// --- config ---

export const configGet = (): Promise<Config> => invoke("config_get");

export const configSet = (config: Config): Promise<void> =>
  invoke("config_set", { config });

// --- workspace store (P15) ---

export const workspaceSnapshot = (): Promise<WorkspaceSnapshot[]> =>
  invoke("workspace_snapshot");

export const workspaceAdd = (title: string, cwd: string): Promise<WorkspaceId> =>
  invoke("workspace_add", { title, cwd });

export const workspaceRemove = (id: WorkspaceId): Promise<boolean> =>
  invoke("workspace_remove", { id });

export const workspaceSelect = (id: WorkspaceId): Promise<boolean> =>
  invoke("workspace_select", { id });

export const workspaceReorder = (from: number, to: number): Promise<boolean> =>
  invoke("workspace_reorder", { from, to });

// --- weft.json (P16) ---

export const weftDefsGet = (content: string): Promise<WeftConfig> =>
  invoke("weft_defs_get", { content });

/** True when the command must be confirmed before running (trust gate). */
export const weftTrustCheck = (
  content: string,
  commandName: string,
  projectLocal: boolean,
  sourcePath: string,
): Promise<boolean> =>
  invoke("weft_trust_check", {
    content,
    commandName,
    projectLocal,
    sourcePath,
  });

export const weftTrustGrant = (
  content: string,
  commandName: string,
  sourcePath: string,
): Promise<boolean> =>
  invoke("weft_trust_grant", { content, commandName, sourcePath });

// --- keybindings (P3/P4/P15/P16) ---

export const keybindingResolve = (action: string): Promise<string | null> =>
  invoke("keybinding_resolve", { action });

export const keybindingList = (): Promise<KeybindingRow[]> =>
  invoke("keybinding_list");

export const keybindingSet = (
  action: string,
  chord: string,
): Promise<KeybindingSetResult> =>
  invoke("keybinding_set", { action, chord });
