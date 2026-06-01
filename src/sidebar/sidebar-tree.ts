// Sidebar tree — render an ordered list of workspace rows + group headers.
//
// Pure builder over an IMMUTABLE snapshot list (snapshot boundary, see
// sidebar-row.ts): given `WorkspaceSnapshot[]` (sidebar order == array order)
// plus optional group descriptors, produce the row DOM. Grouping is "flatten +
// header": a group renders a header row, then its member rows directly beneath;
// a collapsed group hides its members but keeps the header. cwd auto-grouping is
// NOT done here (deferred per plan) — the host decides group membership and
// passes it in, so this stays a deterministic view function.
//
// The tree holds no store ref and no per-row state; to reflect a change the host
// calls `buildSidebarTree` again with a fresh snapshot list.

import type { WorkspaceSnapshot, WorkspaceId, GroupId } from "$lib/model";
import { buildSidebarRow, type SidebarRowActions } from "./sidebar-row";

/** Minimal group descriptor the host supplies (subset of model WorkspaceGroup). */
export interface SidebarGroup {
  id: GroupId;
  name: string;
  isCollapsed: boolean;
  /** Ordered workspace ids that belong to this group. */
  memberIds: WorkspaceId[];
}

export interface SidebarTreeActions extends SidebarRowActions {
  /** Toggle a group header's collapsed state (host mutates, re-renders). */
  onToggleGroup?: (id: GroupId) => void;
}

export interface SidebarTreeOptions {
  groups?: SidebarGroup[];
  /** Currently-selected workspace (drives aria-selected + row tabindex). */
  selectedId?: WorkspaceId;
}

/**
 * Build the sidebar list container from immutable snapshots. Ungrouped
 * workspaces render in array order; each group renders a header followed by its
 * member rows (hidden when collapsed). Members are matched by id against the
 * snapshot list so a stale group id never crashes the render.
 */
export function buildSidebarTree(
  snapshots: ReadonlyArray<Readonly<WorkspaceSnapshot>>,
  actions: SidebarTreeActions = {},
  options: SidebarTreeOptions = {},
): HTMLElement {
  const list = document.createElement("div");
  list.className = "sidebar-tree";
  list.setAttribute("role", "listbox");

  const byId = new Map<WorkspaceId, Readonly<WorkspaceSnapshot>>();
  for (const snap of snapshots) {
    byId.set(snap.id, snap);
  }

  const groups = options.groups ?? [];
  const grouped = new Set<WorkspaceId>();
  for (const group of groups) {
    for (const memberId of group.memberIds) {
      grouped.add(memberId);
    }
  }

  // Ungrouped rows first, preserving the snapshot (sidebar) order.
  for (const snap of snapshots) {
    if (!grouped.has(snap.id)) {
      list.appendChild(row(snap, actions, options));
    }
  }

  // Then each group: header + (when expanded) its member rows.
  for (const group of groups) {
    list.appendChild(buildGroupHeader(group, actions));
    if (group.isCollapsed) {
      continue;
    }
    for (const memberId of group.memberIds) {
      const snap = byId.get(memberId);
      if (snap) {
        list.appendChild(row(snap, actions, options));
      }
    }
  }

  return list;
}

function row(
  snap: Readonly<WorkspaceSnapshot>,
  actions: SidebarTreeActions,
  options: SidebarTreeOptions,
): HTMLElement {
  return buildSidebarRow(snap, actions, { selected: snap.id === options.selectedId });
}

/** A clickable group header that toggles collapse; shows member count. */
function buildGroupHeader(group: SidebarGroup, actions: SidebarTreeActions): HTMLElement {
  const header = document.createElement("div");
  header.className = "sidebar-group-header";
  header.dataset.groupId = group.id;
  header.dataset.collapsed = String(group.isCollapsed);
  header.setAttribute("role", "group");
  header.setAttribute("aria-expanded", String(!group.isCollapsed));

  const name = document.createElement("span");
  name.className = "sidebar-group-header__name";
  name.textContent = group.name;
  header.appendChild(name);

  const count = document.createElement("span");
  count.className = "sidebar-group-header__count";
  count.textContent = String(group.memberIds.length);
  header.appendChild(count);

  header.addEventListener("click", () => actions.onToggleGroup?.(group.id));
  return header;
}
