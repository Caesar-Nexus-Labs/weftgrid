// Pane ring binding (P5b integration) — wires the P5a notification client to the
// P4 pane view (ring class + tab highlight) and the clear-on-focus path.
//
// This is PURE VIEW WIRING. The Rust notification-manager is the single source of
// truth: it parses OSC, owns per-pane ring state, and emits `notification-changed`
// (see notification-client.ts). This module subscribes to that event and forwards
// the new state to injected view callbacks; it does NOT parse or store ring state.
//
// `invoke`/`listen` are injected (matching the notification-client / xterm-wrapper
// pattern) so the binding unit-tests under jsdom without a live Tauri runtime.
//
// Mapping note: the event carries a `paneId`. How a paneId maps to a DOM host
// (pane-renderer keys by first surface id) or a tab is the HOST's concern — the
// host supplies `setPaneRing` / `setTabHighlight` that translate as needed. The
// binding stays framework-agnostic and knows nothing about the DOM.

import type { InvokeFn } from "../terminal/xterm-wrapper";
import type { PaneId } from "$lib/model";
import {
  onNotificationChanged,
  clearPaneNotification,
  fetchPaneState,
  type ListenFn,
  type PaneRingState,
} from "./notification-client";

/**
 * The view surface the binding drives. The host implements these to toggle the
 * P4 pane ring (pane-renderer.setPaneRing) and the surface-tab highlight
 * (tabs.setTabHighlight). Both receive the backend `paneId`; the host maps it to
 * its own DOM keys. A sidebar highlight (P15b) can be added as a third callback
 * later without changing this binding (see SIDEBAR HIGHLIGHT note below).
 */
export interface PaneRingView {
  /** Toggle the unread ring on the pane identified by `paneId`. */
  setPaneRing(paneId: PaneId, hasRing: boolean): void;
  /** Toggle the unread highlight on the surface tab for `paneId`. */
  setTabHighlight(paneId: PaneId, hasRing: boolean): void;
}

export interface PaneRingBindingDeps {
  listen: ListenFn;
  invoke: InvokeFn;
  view: PaneRingView;
}

/**
 * Connects backend ring-state changes to the pane view and handles clear-on-focus.
 *
 * Lifecycle: `start()` subscribes to `notification-changed`; `stop()` unsubscribes.
 * `clearOnFocus(paneId)` is called by the pane focus/click handler — it invokes
 * `notify_clear` and optimistically clears the local view so the ring disappears
 * immediately (the backend then emits a confirming `notification-changed`).
 */
export class PaneRingBinding {
  private unlisten: (() => void) | null = null;

  constructor(private readonly deps: PaneRingBindingDeps) {}

  /** Subscribe to backend ring-state changes. Idempotent. */
  async start(): Promise<void> {
    if (this.unlisten) {
      return;
    }
    this.unlisten = await onNotificationChanged(this.deps.listen, (state) =>
      this.apply(state),
    );
  }

  /** Unsubscribe from backend events (call on teardown). */
  stop(): void {
    this.unlisten?.();
    this.unlisten = null;
  }

  /**
   * Clear-on-focus hook: the pane gained focus (focus-nav or click), so its
   * notification is dismissed. Tells the backend (`notify_clear`) and optimistically
   * drops the ring + highlight for snappy feedback. Returns the backend result.
   */
  async clearOnFocus(paneId: PaneId): Promise<boolean> {
    this.deps.view.setPaneRing(paneId, false);
    this.deps.view.setTabHighlight(paneId, false);
    return clearPaneNotification(this.deps.invoke, paneId);
  }

  /**
   * Mount-time sync: pull a pane's current ring state from the backend and apply
   * it (the pane may have been spawned with an unread notification already set).
   */
  async syncPane(paneId: PaneId): Promise<void> {
    const state = await fetchPaneState(this.deps.invoke, paneId);
    this.apply(state);
  }

  /** Push one pane's backend state into the view (ring + tab highlight). */
  private apply(state: PaneRingState): void {
    this.deps.view.setPaneRing(state.paneId, state.hasRing);
    this.deps.view.setTabHighlight(state.paneId, state.hasRing);
  }
}
