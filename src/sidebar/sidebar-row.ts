// Sidebar row — one workspace row built from an IMMUTABLE WorkspaceSnapshot.
//
// SNAPSHOT-BOUNDARY (load-bearing perf rule, cmux issue #2586): a row MUST be a
// pure function of its value props. It copies primitive snapshot fields into the
// DOM at build time and retains NO reference to the snapshot object or any
// reactive store. Holding a store ref makes every row re-render on any unrelated
// store change → the 100% CPU spin cmux hit. To reflect a change, the host
// rebuilds the row from a fresh snapshot; the row never self-updates.
//
// User intent flows OUT through closure actions (select/rename/pin/reorder) the
// host injects — the row owns no mutation state of its own (source of truth is
// the Rust WorkspaceStore via P12).

import type { WorkspaceSnapshot, WorkspaceId } from "$lib/model";

/** Closure actions the host wires; the row only calls them, never mutates state. */
export interface SidebarRowActions {
  onSelect?: (id: WorkspaceId) => void;
  onRename?: (id: WorkspaceId) => void;
  onTogglePin?: (id: WorkspaceId) => void;
  /** Drag-drop reorder: the dragged row id dropped onto this row's id. */
  onReorder?: (fromId: WorkspaceId, toId: WorkspaceId) => void;
}

export interface SidebarRowOptions {
  /** Marks the row aria-selected + adds the selected class (host-tracked). */
  selected?: boolean;
}

/** MIME-ish drag key so a sidebar drag is distinguishable from other drags. */
const DRAG_TYPE = "application/x-weftgrid-workspace-id";

/**
 * Build a single workspace row element from an immutable snapshot value.
 * Reads only `snapshot`; attaches no reference to it (snapshot boundary).
 */
export function buildSidebarRow(
  snapshot: Readonly<WorkspaceSnapshot>,
  actions: SidebarRowActions = {},
  options: SidebarRowOptions = {},
): HTMLElement {
  // Copy out every value the DOM needs up front so nothing closes over the
  // snapshot object beyond this builder call (boundary guarantee).
  const id = snapshot.id;
  const title = snapshot.title;
  const cwd = snapshot.currentDirectory;
  const isPinned = snapshot.isPinned;
  const unread = snapshot.unreadCount;
  const colorHex = snapshot.customColorHex;
  const description = snapshot.customDescription;

  const row = document.createElement("div");
  row.className = "sidebar-row";
  row.dataset.workspaceId = id;
  row.setAttribute("role", "option");
  row.tabIndex = options.selected ? 0 : -1;
  row.setAttribute("aria-selected", String(options.selected ?? false));
  if (options.selected) {
    row.classList.add("sidebar-row--selected");
  }

  if (colorHex) {
    const dot = document.createElement("span");
    dot.className = "sidebar-row__color";
    dot.style.backgroundColor = colorHex;
    dot.dataset.colorHex = colorHex;
    row.appendChild(dot);
  }

  const titleEl = document.createElement("span");
  titleEl.className = "sidebar-row__title";
  titleEl.textContent = title;
  if (description) {
    titleEl.title = description;
  }
  row.appendChild(titleEl);

  const cwdEl = document.createElement("span");
  cwdEl.className = "sidebar-row__cwd";
  cwdEl.textContent = cwd;
  row.appendChild(cwdEl);

  if (isPinned) {
    const pin = document.createElement("span");
    pin.className = "sidebar-row__pin";
    pin.dataset.pinned = "true";
    pin.setAttribute("aria-label", "pinned");
    row.appendChild(pin);
  }

  if (unread > 0) {
    const badge = document.createElement("span");
    badge.className = "sidebar-row__unread";
    badge.dataset.unread = String(unread);
    badge.textContent = String(unread);
    row.appendChild(badge);
  }

  // Closure actions — the only path that mutates state (out to P12, not local).
  row.addEventListener("click", () => actions.onSelect?.(id));
  row.addEventListener("dblclick", () => actions.onRename?.(id));

  const pinButton = document.createElement("button");
  pinButton.type = "button";
  pinButton.className = "sidebar-row__pin-toggle";
  pinButton.setAttribute("aria-label", isPinned ? "unpin workspace" : "pin workspace");
  pinButton.addEventListener("click", (e) => {
    e.stopPropagation();
    actions.onTogglePin?.(id);
  });
  row.appendChild(pinButton);

  wireReorderDrag(row, id, actions);
  return row;
}

/** Attach native drag-drop so a row can be reordered onto another row. */
function wireReorderDrag(row: HTMLElement, id: WorkspaceId, actions: SidebarRowActions): void {
  row.draggable = true;
  row.addEventListener("dragstart", (e) => {
    e.dataTransfer?.setData(DRAG_TYPE, id);
  });
  row.addEventListener("dragover", (e) => {
    if (e.dataTransfer?.types.includes(DRAG_TYPE)) {
      e.preventDefault(); // allow drop
    }
  });
  row.addEventListener("drop", (e) => {
    const fromId = e.dataTransfer?.getData(DRAG_TYPE);
    if (fromId && fromId !== id) {
      e.preventDefault();
      actions.onReorder?.(fromId, id);
    }
  });
}
