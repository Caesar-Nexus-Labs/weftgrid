// Pane renderer — recursive LayoutNode → nested CSS-flex DOM.
//
// CRITICAL (risk-assessment): a re-layout MUST reuse the existing DOM host for a
// pane rather than recreate it. The host holds a live xterm canvas/WebGL context
// + scrollback; recreating it destroys scrollback and churns the GPU. So the
// renderer keeps a `Map<paneId, host>` cache and, on each render, re-parents the
// cached hosts into a freshly-built flex skeleton. Only structure (splits +
// dividers) is rebuilt; leaf content survives.
//
// Layout: split → flex container; orientation decides flex-direction.
//   horizontal → row (children left|right), first gets `flex-grow: position`.
//   vertical   → column (children top|bottom), first gets `flex-grow: position`.
// The divider sits between children; divider-drag.ts attaches behavior later.
//
// Pane identity: the model's pane leaf has no id, so we derive a stable pane key
// from its surfaces. A leaf is keyed by its FIRST panel id (a pane always has at
// least one surface). That key addresses the reusable host; the per-surface
// xterm mounts live inside the host (P3 owns mounting — see PaneHostHooks).

import { type LayoutNode, type PanelId } from "$lib/model";
import { applyAria, applyDividerAria, PaneAriaRole } from "./a11y";

/** Stable key for a pane host = its first surface id (panes always have ≥1). */
export function paneKey(node: Extract<LayoutNode, { type: "pane" }>): string {
  return node.panel_ids[0] ?? "empty";
}

/**
 * Host lifecycle hooks. The renderer creates the host element once per pane key
 * and calls `onMount` so P3 can attach the xterm/browser-anchor; `onReuse` fires
 * when an already-mounted host is re-attached during a re-layout (no remount).
 */
export interface PaneHostHooks {
  onMount?: (host: HTMLElement, paneId: PanelId, node: Extract<LayoutNode, { type: "pane" }>) => void;
  onReuse?: (host: HTMLElement, paneId: PanelId, node: Extract<LayoutNode, { type: "pane" }>) => void;
}

/**
 * Renders a split-tree into `container`, reusing pane hosts across calls. Call
 * `render(tree)` again after any tree change; hosts for surviving panes are
 * preserved. Panes that disappear have their hosts removed (caller disposes the
 * session separately — renderer does not own xterm lifecycles).
 */
export class PaneRenderer {
  private readonly hosts = new Map<string, HTMLElement>();
  // P5b notification ring: which pane keys currently hold an unread ring. Kept
  // separate from the host DOM so the ring survives re-layout (host reuse) and is
  // re-applied in decoratePane on every render. The Rust notification-manager is
  // the single source of truth; this map is a pure view cache driven by setPaneRing.
  private readonly ringState = new Map<string, boolean>();

  constructor(
    private readonly container: HTMLElement,
    private readonly hooks: PaneHostHooks = {},
  ) {}

  render(tree: LayoutNode): void {
    const seen = new Set<string>();
    const root = this.build(tree, seen);
    // Swap children in one shot; cached hosts were detached by build() and are
    // re-attached inside `root`, so their xterm contexts persist.
    this.container.replaceChildren(root);
    // Evict hosts for panes that no longer exist.
    for (const [key, host] of this.hosts) {
      if (!seen.has(key)) {
        host.remove();
        this.hosts.delete(key);
      }
    }
  }

  /** The reusable host element for a pane key (for P3 to mount/look up xterm). */
  getHost(key: string): HTMLElement | undefined {
    return this.hosts.get(key);
  }

  /**
   * P5b notification hook: toggle the unread ring on a pane (keyed by paneKey =
   * first surface id). The notification-manager (Rust) is the source of truth;
   * the binding calls this on each `notification-changed`. The flag is cached so
   * a re-layout (host reuse) re-applies it. Returns true if a live host updated.
   */
  setPaneRing(key: string, hasRing: boolean): boolean {
    if (hasRing) {
      this.ringState.set(key, true);
    } else {
      this.ringState.delete(key);
    }
    const host = this.hosts.get(key);
    if (host) {
      this.applyRing(host, hasRing);
      return true;
    }
    return false;
  }

  private build(node: LayoutNode, seen: Set<string>): HTMLElement {
    if (node.type === "pane") {
      return this.buildPane(node, seen);
    }
    return this.buildSplit(node, seen);
  }

  private buildPane(node: Extract<LayoutNode, { type: "pane" }>, seen: Set<string>): HTMLElement {
    const key = paneKey(node);
    seen.add(key);
    let host = this.hosts.get(key);
    const firstPanel = node.panel_ids[0];
    if (host) {
      // Reuse: keep the live xterm; just refresh selection-driven attributes.
      this.decoratePane(host, node);
      if (firstPanel) {
        this.hooks.onReuse?.(host, firstPanel, node);
      }
      return host;
    }
    host = document.createElement("div");
    host.className = "pane-host";
    host.dataset.paneKey = key;
    this.decoratePane(host, node);
    this.hosts.set(key, host);
    if (firstPanel) {
      this.hooks.onMount?.(host, firstPanel, node);
    }
    return host;
  }

  private decoratePane(host: HTMLElement, node: Extract<LayoutNode, { type: "pane" }>): void {
    const selected = node.selected_panel_id ?? node.panel_ids[0];
    applyAria(host, PaneAriaRole.pane, {
      "data-selected-panel-id": selected,
      tabindex: 0,
    });
    // Browser-anchor placeholder: a leaf whose surfaces include a browser panel
    // exposes a rect div P6 reads via getBoundingClientRect(). Real overlay sync
    // is P6's job; here it is just a marked, measurable rect.
    host.dataset.browserAnchor = node.panel_ids.some((id) => id.startsWith("browser-")) ? "true" : "false";
    // Re-apply any unread notification ring (survives host reuse across re-layout).
    this.applyRing(host, this.ringState.get(paneKey(node)) === true);
  }

  /**
   * Mark/unmark the unread ring on a pane host. Not color-only (a11y): sets a
   * `data-has-notification` flag + `pane-host--ring` class for styling AND an
   * `aria-description` so assistive tech announces the unread state.
   */
  private applyRing(host: HTMLElement, hasRing: boolean): void {
    host.classList.toggle("pane-host--ring", hasRing);
    if (hasRing) {
      host.dataset.hasNotification = "true";
      host.setAttribute("aria-description", "Unread notification");
    } else {
      delete host.dataset.hasNotification;
      host.removeAttribute("aria-description");
    }
  }

  private buildSplit(node: Extract<LayoutNode, { type: "split" }>, seen: Set<string>): HTMLElement {
    const box = document.createElement("div");
    box.className = `pane-split pane-split--${node.orientation}`;
    box.style.display = "flex";
    box.style.flexDirection = node.orientation === "horizontal" ? "row" : "column";
    box.style.width = "100%";
    box.style.height = "100%";
    applyAria(box, PaneAriaRole.split, { "aria-orientation": node.orientation });

    const first = this.wrapChild(this.build(node.first, seen), node.divider_position);
    const second = this.wrapChild(this.build(node.second, seen), 1 - node.divider_position);
    const divider = this.buildDivider(node);

    box.append(first, divider, second);
    return box;
  }

  /** Wrap a child so flex-grow encodes its fraction of the parent axis. */
  private wrapChild(child: HTMLElement, grow: number): HTMLElement {
    const cell = document.createElement("div");
    cell.className = "pane-cell";
    cell.style.flexGrow = String(grow);
    cell.style.flexBasis = "0";
    cell.style.overflow = "hidden";
    cell.style.position = "relative";
    cell.append(child);
    return cell;
  }

  private buildDivider(node: Extract<LayoutNode, { type: "split" }>): HTMLElement {
    const divider = document.createElement("div");
    divider.className = `pane-divider pane-divider--${node.orientation}`;
    applyDividerAria(divider, node.orientation, node.divider_position);
    return divider;
  }
}
