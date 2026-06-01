// In-pane tab bar — surface stacking within one pane (multi-surface IN-SCOPE).
//
// A pane can stack multiple surfaces (terminals/browsers) and show one at a time
// via a tab strip, just like cmux Bonsplit. The strip reads the pane's
// `panel_ids[]` + `selected_panel_id` (P2 contract) and emits a selection when a
// tab is clicked or arrow-navigated. The strip orientation (horizontal across
// the top, or vertical down the side) is a UI setting, not a tree property.
//
// Pure selection math (`nextSelection`) is split out so it unit-tests without a
// DOM; the builder wires ARIA (via a11y) + click/keyboard to that math.

import { type PanelId } from "$lib/model";
import {
  PaneAriaRole,
  applyAria,
  setTabSelected,
  rovingNextIndex,
} from "./a11y";

export type TabBarOrientation = "horizontal" | "vertical";

/** The slice of a pane the tab bar needs (mirrors the P2 pane leaf). */
export interface TabModel {
  panelIds: PanelId[];
  selectedPanelId?: PanelId;
}

export interface TabBarOptions {
  orientation?: TabBarOrientation;
  /** Human label for a surface tab (P5/title track supplies real titles). */
  label?: (panelId: PanelId) => string;
  /** Fired when the user picks a surface (click or keyboard). */
  onSelect?: (panelId: PanelId) => void;
}

/** Index of the currently-selected panel (0 when none/selection is stale). */
export function selectedIndex(model: TabModel): number {
  if (!model.selectedPanelId) {
    return 0;
  }
  const i = model.panelIds.indexOf(model.selectedPanelId);
  return i >= 0 ? i : 0;
}

/** Pure: the panel id `delta` steps from the current selection (wraps). */
export function nextSelection(model: TabModel, delta: number): PanelId | undefined {
  const i = rovingNextIndex(selectedIndex(model), model.panelIds.length, delta);
  return i >= 0 ? model.panelIds[i] : undefined;
}

/**
 * Build a tab strip element for a pane. Rebuilt on each re-layout (cheap, holds
 * no precious state — the surface hosts are what get reused, see pane-renderer).
 */
export function createTabBar(model: TabModel, opts: TabBarOptions = {}): HTMLElement {
  const orientation = opts.orientation ?? "horizontal";
  const label = opts.label ?? ((id) => id);

  const bar = document.createElement("div");
  bar.className = `pane-tabbar pane-tabbar--${orientation}`;
  applyAria(bar, PaneAriaRole.tablist, { "aria-orientation": orientation });

  const active = selectedIndex(model);
  const tabs: HTMLElement[] = model.panelIds.map((panelId, i) => {
    const tab = document.createElement("button");
    tab.type = "button";
    tab.className = "pane-tab";
    tab.dataset.panelId = panelId;
    tab.textContent = label(panelId);
    applyAria(tab, PaneAriaRole.tab, { "aria-controls": `surface-${panelId}` });
    setTabSelected(tab, i === active);
    tab.addEventListener("click", () => opts.onSelect?.(panelId));
    bar.appendChild(tab);
    return tab;
  });

  // Arrow keys move selection within the strip (roving). Orientation decides
  // which arrow pair applies; Home/End jump to ends.
  bar.addEventListener("keydown", (e) => {
    const delta = arrowDelta(e.key, orientation);
    if (delta !== 0) {
      e.preventDefault();
      const target = nextSelection(model, delta);
      if (target) {
        opts.onSelect?.(target);
      }
      return;
    }
    if (e.key === "Home" && model.panelIds.length > 0) {
      e.preventDefault();
      opts.onSelect?.(model.panelIds[0]);
    } else if (e.key === "End" && model.panelIds.length > 0) {
      e.preventDefault();
      opts.onSelect?.(model.panelIds[model.panelIds.length - 1]);
    }
  });

  // Expose the tab elements for roving-focus management by the host.
  (bar as unknown as { _tabs: HTMLElement[] })._tabs = tabs;
  return bar;
}

/**
 * P5b notification hook: highlight (or clear) a surface tab when its pane holds
 * an unread ring. Additive view glue — the notification-manager (Rust) is the
 * source of truth; the binding calls this on each `notification-changed`.
 *
 * Not color-only (a11y): toggles a `pane-tab--unread` class for styling AND sets
 * `data-unread` + `aria-description` so assistive tech announces the unread tab.
 * Returns true if a matching tab was found in `bar`.
 */
export function setTabHighlight(bar: HTMLElement, panelId: PanelId, hasRing: boolean): boolean {
  const tab = tabElements(bar).find((t) => t.dataset.panelId === panelId);
  if (!tab) {
    return false;
  }
  tab.classList.toggle("pane-tab--unread", hasRing);
  if (hasRing) {
    tab.dataset.unread = "true";
    tab.setAttribute("aria-description", "Unread notification");
  } else {
    delete tab.dataset.unread;
    tab.removeAttribute("aria-description");
  }
  return true;
}

/** The tab buttons created by `createTabBar` (stashed on the bar for reuse). */
function tabElements(bar: HTMLElement): HTMLElement[] {
  return (bar as unknown as { _tabs?: HTMLElement[] })._tabs ?? [];
}

/** Map an arrow key to a step (+1/-1) for the strip's orientation, else 0. */
function arrowDelta(key: string, orientation: TabBarOrientation): number {
  if (orientation === "horizontal") {
    if (key === "ArrowRight") return +1;
    if (key === "ArrowLeft") return -1;
  } else {
    if (key === "ArrowDown") return +1;
    if (key === "ArrowUp") return -1;
  }
  return 0;
}
