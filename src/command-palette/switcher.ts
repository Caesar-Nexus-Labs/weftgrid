// Switcher (P16) — the Cmd+P scope: jump to a workspace (or surface, when the
// all-surfaces toggle is on). Entries come from the P12 WorkspaceStore snapshot;
// selecting one calls `workspace_select` (the same path P15's sidebar uses).
//
// Surface entries are a thin extension: when `allSurfaces` is on, each workspace
// contributes its panes too, so the user can jump straight to a pane. Selecting a
// surface still selects its owning workspace (pane-level focus is a P4 seam).

import type { PaletteCandidate } from "./palette-search";

/** Minimal workspace shape the switcher needs (subset of `WorkspaceSnapshot`). */
export interface SwitcherWorkspace {
  id: string;
  title: string;
  currentDirectory?: string;
}

/** A surface (pane) within a workspace, surfaced when all-surfaces is on. */
export interface SwitcherSurface {
  workspaceId: string;
  surfaceId: string;
  title: string;
}

/** One switcher row: either a workspace or a surface jump target. */
export interface SwitcherEntry {
  /** Palette id (`switcher.ws.<id>` or `switcher.surface.<id>`). */
  id: string;
  title: string;
  subtitle?: string;
  workspaceId: string;
  surfaceId?: string;
}

/** P12 seam: select a workspace (returns true when it existed). */
export type SelectWorkspaceFn = (id: string) => Promise<boolean>;

/** Build switcher entries from the workspace snapshot (+ surfaces if enabled). */
export function buildSwitcherEntries(
  workspaces: SwitcherWorkspace[],
  surfaces: SwitcherSurface[] = [],
  allSurfaces = false,
): SwitcherEntry[] {
  const entries: SwitcherEntry[] = workspaces.map((ws) => ({
    id: `switcher.ws.${ws.id}`,
    title: ws.title,
    subtitle: ws.currentDirectory,
    workspaceId: ws.id,
  }));

  if (allSurfaces) {
    for (const surface of surfaces) {
      entries.push({
        id: `switcher.surface.${surface.surfaceId}`,
        title: surface.title,
        subtitle: workspaces.find((w) => w.id === surface.workspaceId)?.title,
        workspaceId: surface.workspaceId,
        surfaceId: surface.surfaceId,
      });
    }
  }

  return entries;
}

/** Project switcher entries to the nucleo corpus (title + cwd as keywords). */
export function switcherCorpus(entries: SwitcherEntry[]): PaletteCandidate[] {
  return entries.map((entry, index) => ({
    id: entry.id,
    text: entry.title,
    keywords: entry.subtitle ?? "",
    rank: index,
  }));
}

/** Drives workspace/surface selection through the P12 WorkspaceStore. */
export class Switcher {
  private entries: SwitcherEntry[] = [];

  constructor(private readonly selectWorkspace: SelectWorkspaceFn) {}

  /** Replace the current entry set (call when the snapshot changes). */
  setEntries(entries: SwitcherEntry[]): void {
    this.entries = entries;
  }

  corpus(): PaletteCandidate[] {
    return switcherCorpus(this.entries);
  }

  /** Select the entry with `id`. Selecting a surface selects its workspace
   * (pane focus is deferred to P4). Returns true when the workspace existed. */
  async select(id: string): Promise<boolean> {
    const entry = this.entries.find((e) => e.id === id);
    if (!entry) {
      return false;
    }
    return this.selectWorkspace(entry.workspaceId);
  }
}
