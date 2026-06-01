// Workspace navigation — the command layer between sidebar UI gestures and the
// P12 WorkspaceStore (the single source of truth).
//
// Every navigation/mutation goes OUT to Rust over Tauri commands; this module
// holds NO workspace state of its own (no local list, no selected index that
// could drift from the store). `invoke` is injected so this unit-tests without a
// live Tauri runtime, mirroring the settings-store / notification-client style.
//
// Keyboard chords (next/prev workspace, jump-unread) are NOT hardcoded: they are
// resolved from the P12 keybinding registry via `keybinding_resolve`, matching
// the P4 KeybindingSource pattern (no key literals in navigation logic).

import type { InvokeFn } from "../terminal/xterm-wrapper";
import type { WorkspaceId, WorkspaceSnapshot } from "$lib/model";

/** Namespaced workspace nav actions the P12 registry binds to chords. */
export type WorkspaceNavAction =
  | "workspace.next"
  | "workspace.prev"
  | "workspace.jumpUnread";

export const WORKSPACE_NAV_ACTIONS: readonly WorkspaceNavAction[] = [
  "workspace.next",
  "workspace.prev",
  "workspace.jumpUnread",
];

/** Modifier+key shape normalized from a DOM KeyboardEvent (mirrors P4). */
export interface ChordEvent {
  key: string;
  ctrlKey: boolean;
  metaKey: boolean;
  altKey: boolean;
  shiftKey: boolean;
}

/** Canonical chord string (fixed modifier order) used to compare bindings. */
export function eventToChord(e: ChordEvent): string {
  const parts: string[] = [];
  if (e.ctrlKey) parts.push("ctrl");
  if (e.metaKey) parts.push("meta");
  if (e.altKey) parts.push("alt");
  if (e.shiftKey) parts.push("shift");
  parts.push(e.key.toLowerCase());
  return parts.join("+");
}

/**
 * Thin client over the P12 WorkspaceStore commands. Each method forwards to its
 * Tauri command with the exact arg shape the Rust side expects (see
 * config/commands.rs) and returns the typed result.
 */
export class WorkspaceNav {
  constructor(private readonly invoke: InvokeFn) {}

  /** Current sidebar snapshot list (drives next/prev index math). */
  snapshot(): Promise<WorkspaceSnapshot[]> {
    return this.invoke<WorkspaceSnapshot[]>("workspace_snapshot");
  }

  select(id: WorkspaceId): Promise<boolean> {
    return this.invoke<boolean>("workspace_select", { id });
  }

  add(title: string, cwd: string): Promise<WorkspaceId> {
    return this.invoke<WorkspaceId>("workspace_add", { title, cwd });
  }

  remove(id: WorkspaceId): Promise<boolean> {
    return this.invoke<boolean>("workspace_remove", { id });
  }

  /** Drag-drop reorder by index (P12 reorders the store + follows selection). */
  reorder(from: number, to: number): Promise<boolean> {
    return this.invoke<boolean>("workspace_reorder", { from, to });
  }

  /** Resolve a nav chord from the P12 registry (null when unbound). */
  resolveChord(action: WorkspaceNavAction): Promise<string | null> {
    return this.invoke<string | null>("keybinding_resolve", { action });
  }

  /**
   * Reorder by id: resolves both ids to their current snapshot indices, then
   * forwards index-based `workspace_reorder`. Keeps the sidebar drag API in id
   * space while the store mutates in index space. No-op when an id is unknown.
   */
  async reorderById(fromId: WorkspaceId, toId: WorkspaceId): Promise<boolean> {
    const snaps = await this.snapshot();
    const from = snaps.findIndex((s) => s.id === fromId);
    const to = snaps.findIndex((s) => s.id === toId);
    if (from < 0 || to < 0 || from === to) {
      return false;
    }
    return this.reorder(from, to);
  }

  /**
   * Select the workspace `delta` steps from `fromId` (wraps). The host passes the
   * currently-selected id (the snapshot DTO carries no selected flag — the store
   * owns the canonical index). Falls back to the first row when `fromId` is
   * unknown so an early key-press before any selection still moves.
   */
  async step(delta: number, fromId?: WorkspaceId): Promise<boolean> {
    const snaps = await this.snapshot();
    if (snaps.length === 0) {
      return false;
    }
    const current = indexOf(snaps, fromId);
    const next = (((current + delta) % snaps.length) + snaps.length) % snaps.length;
    return this.select(snaps[next].id);
  }

  /** Select the next workspace with unread, scanning forward from `fromId`. */
  async jumpUnread(fromId?: WorkspaceId): Promise<boolean> {
    const snaps = await this.snapshot();
    if (snaps.length === 0) {
      return false;
    }
    const start = indexOf(snaps, fromId);
    for (let offset = 1; offset <= snaps.length; offset++) {
      const candidate = snaps[(start + offset) % snaps.length];
      if (candidate.unreadCount > 0) {
        return this.select(candidate.id);
      }
    }
    return false;
  }

  /**
   * Route a keyboard event through the P12 registry. Returns true when a nav
   * action fired (the host calls preventDefault), false when the chord is unbound
   * or did not move selection. Resolves chords lazily so registry rebinds apply.
   * `fromId` is the host's currently-selected workspace.
   */
  async handleKey(event: ChordEvent, fromId?: WorkspaceId): Promise<boolean> {
    const chord = eventToChord(event);
    for (const action of WORKSPACE_NAV_ACTIONS) {
      if ((await this.resolveChord(action)) === chord) {
        return this.runAction(action, fromId);
      }
    }
    return false;
  }

  private runAction(action: WorkspaceNavAction, fromId?: WorkspaceId): Promise<boolean> {
    switch (action) {
      case "workspace.next":
        return this.step(1, fromId);
      case "workspace.prev":
        return this.step(-1, fromId);
      case "workspace.jumpUnread":
        return this.jumpUnread(fromId);
    }
  }
}

/** Index of `id` in the snapshot list, or 0 when absent/unspecified. */
function indexOf(snaps: ReadonlyArray<WorkspaceSnapshot>, id?: WorkspaceId): number {
  if (id === undefined) {
    return 0;
  }
  const i = snaps.findIndex((s) => s.id === id);
  return i >= 0 ? i : 0;
}
