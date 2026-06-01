// Notification client surface (P5a core) — typed consume side for P5b (Wave-3).
//
// The Rust core owns parsing + the per-pane notification manager and emits a
// `notification-changed` event (payload = a pane's ring state) on every change.
// This module is the clean TS contract P5b subscribes to in order to draw the
// pane ring + sidebar highlight, plus thin `invoke` wrappers for the query/clear
// commands. Types mirror the serde `camelCase` DTOs in
// `src-tauri/src/notify/manager.rs` — keep them in sync.
//
// No parsing/state lives here (single source of truth is Rust). `invoke`/`listen`
// are injected so this unit-tests without a live Tauri runtime.

import type { InvokeFn } from "../terminal/xterm-wrapper";
import type { PaneId } from "$lib/model";

/** Tauri event name the backend emits on every notification state change. */
export const NOTIFICATION_CHANGED_EVENT = "notification-changed";

/** A stored notification (mirror of Rust `manager::Notification`). */
export interface Notification {
  id: string;
  paneId: PaneId;
  title: string;
  subtitle: string;
  body: string;
  /** Monotonic arrival order across panes (newer = larger). */
  seq: number;
  isRead: boolean;
}

/** Per-pane ring/unread snapshot (mirror of Rust `manager::PaneRingState`). */
export interface PaneRingState {
  paneId: PaneId;
  /** True while the pane holds an unread notification (P5b draws the ring). */
  hasRing: boolean;
  latest: Notification | null;
}

/** Subset of `@tauri-apps/api/event`'s `listen` we depend on. */
export type ListenFn = <T>(
  event: string,
  handler: (event: { payload: T }) => void,
) => Promise<() => void>;

/**
 * Subscribe to backend ring-state changes. `handler` fires with each pane's new
 * `PaneRingState` (P5b updates the ring + sidebar highlight from this). Resolves
 * to an unlisten function.
 */
export function onNotificationChanged(
  listen: ListenFn,
  handler: (state: PaneRingState) => void,
): Promise<() => void> {
  return listen<PaneRingState>(NOTIFICATION_CHANGED_EVENT, (e) => handler(e.payload));
}

/** Read a pane's current ring state (P5b polls this on mount). */
export function fetchPaneState(invoke: InvokeFn, paneId: PaneId): Promise<PaneRingState> {
  return invoke<PaneRingState>("notify_pane_state", { paneId });
}

/** Global unread count (panes with an unread notification) for the app badge. */
export function fetchUnreadCount(invoke: InvokeFn): Promise<number> {
  return invoke<number>("notify_unread_count");
}

/** Clear a pane's notification — the focus/click hook that turns the ring off. */
export function clearPaneNotification(invoke: InvokeFn, paneId: PaneId): Promise<boolean> {
  return invoke<boolean>("notify_clear", { paneId });
}

/** Mark a pane's notification read (ring off, keep in history). */
export function markPaneRead(invoke: InvokeFn, paneId: PaneId): Promise<boolean> {
  return invoke<boolean>("notify_mark_read", { paneId });
}
