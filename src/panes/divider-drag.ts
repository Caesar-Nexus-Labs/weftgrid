// Divider drag — pointer events → new divider_position → rAF-batched re-layout.
//
// The divider is the bar between a split's two children. Dragging it changes the
// parent split's `divider_position` (first child's fraction of the parent axis).
// We measure against the split CONTAINER's rect so the fraction is exact across
// nesting. Updates are coalesced with requestAnimationFrame so a burst of
// pointermove events triggers at most one layout per frame (the "~60fps" target
// is a manual acceptance check, but rAF batching is the mechanism).
//
// This module is interaction-only: it computes the clamped fraction and calls
// back with it. The host applies the new tree (resizeDivider) + re-renders. Pure
// math (`positionFromPointer`) is exported for unit tests without a live pointer.

import { type SplitOrientation, clampDivider } from "$lib/model";

/** Container geometry needed to turn a pointer coord into a fraction. */
export interface DragBounds {
  left: number;
  top: number;
  width: number;
  height: number;
}

/** Compute the clamped divider fraction for a pointer at (clientX, clientY). */
export function positionFromPointer(
  orientation: SplitOrientation,
  bounds: DragBounds,
  clientX: number,
  clientY: number,
): number {
  const raw =
    orientation === "horizontal"
      ? (clientX - bounds.left) / bounds.width
      : (clientY - bounds.top) / bounds.height;
  return clampDivider(raw);
}

export interface DividerDragDeps {
  /** rAF scheduler (injectable for tests). Defaults to window.requestAnimationFrame. */
  requestFrame?: (cb: () => void) => number;
  /** rAF canceller. Defaults to window.cancelAnimationFrame. */
  cancelFrame?: (handle: number) => void;
}

export interface DividerDragConfig {
  orientation: SplitOrientation;
  /** Element whose rect defines the drag axis (the split container). */
  container: HTMLElement;
  /** Called (rAF-batched) with each clamped fraction during the drag. */
  onResize: (position: number) => void;
  /** Optional end-of-drag hook (commit/persist trigger for the host). */
  onCommit?: (position: number) => void;
}

/**
 * Attach pointer-drag behavior to a divider element. Returns a disposer that
 * removes listeners + cancels any pending frame. Uses pointer capture so the
 * drag continues even if the pointer leaves the thin divider.
 */
export function attachDividerDrag(
  divider: HTMLElement,
  config: DividerDragConfig,
  deps: DividerDragDeps = {},
): () => void {
  const requestFrame = deps.requestFrame ?? ((cb) => requestAnimationFrame(cb));
  const cancelFrame = deps.cancelFrame ?? ((h) => cancelAnimationFrame(h));

  let frame: number | null = null;
  let pending = 0;
  let dragging = false;

  const flush = () => {
    frame = null;
    config.onResize(pending);
  };

  const schedule = (position: number) => {
    pending = position;
    if (frame === null) {
      frame = requestFrame(flush);
    }
  };

  const bounds = (): DragBounds => {
    const r = config.container.getBoundingClientRect();
    return { left: r.left, top: r.top, width: r.width, height: r.height };
  };

  const onPointerMove = (e: PointerEvent) => {
    if (!dragging) {
      return;
    }
    schedule(positionFromPointer(config.orientation, bounds(), e.clientX, e.clientY));
  };

  const onPointerUp = (e: PointerEvent) => {
    if (!dragging) {
      return;
    }
    dragging = false;
    if (frame !== null) {
      cancelFrame(frame);
      frame = null;
    }
    const final = positionFromPointer(config.orientation, bounds(), e.clientX, e.clientY);
    config.onResize(final);
    config.onCommit?.(final);
    if (divider.hasPointerCapture?.(e.pointerId)) {
      divider.releasePointerCapture(e.pointerId);
    }
  };

  const onPointerDown = (e: PointerEvent) => {
    dragging = true;
    divider.setPointerCapture?.(e.pointerId);
    e.preventDefault();
  };

  divider.addEventListener("pointerdown", onPointerDown);
  divider.addEventListener("pointermove", onPointerMove);
  divider.addEventListener("pointerup", onPointerUp);
  divider.addEventListener("pointercancel", onPointerUp);

  return () => {
    divider.removeEventListener("pointerdown", onPointerDown);
    divider.removeEventListener("pointermove", onPointerMove);
    divider.removeEventListener("pointerup", onPointerUp);
    divider.removeEventListener("pointercancel", onPointerUp);
    if (frame !== null) {
      cancelFrame(frame);
    }
  };
}
