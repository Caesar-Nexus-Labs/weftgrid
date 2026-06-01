// Directional focus navigation — "focus the pane to the left/right/up/down".
//
// Geometry, not tree-adjacency: the user expects arrow-style focus to move to
// whatever pane is visually in that direction, even across split boundaries.
// We compute each leaf's normalized rect ([0,1] in both axes) from the divider
// positions, then pick the best neighbor in the requested direction.
//
// Bindings come from a `KeybindingSource` (P12 seam) — NO key literals here.
// `handleKey` maps an event → chord → FocusAction → new focused path.

import { type LayoutNode } from "$lib/model";
import { collectPanes, type PanePath } from "./split-tree";
import {
  type KeybindingSource,
  type FocusAction,
  type ChordEvent,
  FOCUS_ACTIONS,
  eventToChord,
} from "./keybinding-source";

export type Direction = "left" | "right" | "up" | "down";

const ACTION_DIRECTION: Record<FocusAction, Direction> = {
  "pane.focus.left": "left",
  "pane.focus.right": "right",
  "pane.focus.up": "up",
  "pane.focus.down": "down",
};

/** Normalized rect of a leaf in the workspace ([0,1] origin top-left). */
export interface PaneRect {
  path: PanePath;
  x: number;
  y: number;
  width: number;
  height: number;
}

/** Compute every leaf's normalized rect from the split-tree divider positions. */
export function computeRects(
  node: LayoutNode,
  rect: { x: number; y: number; width: number; height: number } = { x: 0, y: 0, width: 1, height: 1 },
  path: PanePath = [],
): PaneRect[] {
  if (node.type === "pane") {
    return [{ path, ...rect }];
  }
  const d = node.divider_position;
  if (node.orientation === "horizontal") {
    // first | second along the x-axis.
    const firstW = rect.width * d;
    return [
      ...computeRects(node.first, { ...rect, width: firstW }, [...path, "first"]),
      ...computeRects(
        node.second,
        { ...rect, x: rect.x + firstW, width: rect.width - firstW },
        [...path, "second"],
      ),
    ];
  }
  // vertical: first / second along the y-axis.
  const firstH = rect.height * d;
  return [
    ...computeRects(node.first, { ...rect, height: firstH }, [...path, "first"]),
    ...computeRects(
      node.second,
      { ...rect, y: rect.y + firstH, height: rect.height - firstH },
      [...path, "second"],
    ),
  ];
}

function center(r: PaneRect): { cx: number; cy: number } {
  return { cx: r.x + r.width / 2, cy: r.y + r.height / 2 };
}

/** True when `cand` lies in `direction` from `from` (with axis overlap). */
function isInDirection(from: PaneRect, cand: PaneRect, direction: Direction): boolean {
  const overlap = (aStart: number, aLen: number, bStart: number, bLen: number) =>
    aStart < bStart + bLen && bStart < aStart + aLen;
  switch (direction) {
    case "left":
      return cand.x + cand.width <= from.x + 1e-9 && overlap(from.y, from.height, cand.y, cand.height);
    case "right":
      return cand.x >= from.x + from.width - 1e-9 && overlap(from.y, from.height, cand.y, cand.height);
    case "up":
      return cand.y + cand.height <= from.y + 1e-9 && overlap(from.x, from.width, cand.x, cand.width);
    case "down":
      return cand.y >= from.y + from.height - 1e-9 && overlap(from.x, from.width, cand.x, cand.width);
  }
}

/**
 * Find the pane path nearest to `from` in `direction`, or `null` at the edge.
 * "Nearest" = smallest center-to-center distance among candidates in-direction.
 */
export function focusInDirection(
  tree: LayoutNode,
  from: PanePath,
  direction: Direction,
): PanePath | null {
  const rects = computeRects(tree);
  const fromRect = rects.find((r) => samePath(r.path, from));
  if (!fromRect) {
    return null;
  }
  const { cx: fx, cy: fy } = center(fromRect);
  let best: PaneRect | null = null;
  let bestDist = Infinity;
  for (const r of rects) {
    if (samePath(r.path, from) || !isInDirection(fromRect, r, direction)) {
      continue;
    }
    const { cx, cy } = center(r);
    const dist = (cx - fx) ** 2 + (cy - fy) ** 2;
    if (dist < bestDist) {
      bestDist = dist;
      best = r;
    }
  }
  return best ? best.path : null;
}

/**
 * Route a keyboard event through the binding source. Returns the new focused
 * path when an action fired and moved focus, else `null` (not a focus chord,
 * unbound, or already at the edge). The host calls preventDefault when non-null.
 */
export function handleKey(
  tree: LayoutNode,
  from: PanePath,
  event: ChordEvent,
  bindings: KeybindingSource,
): PanePath | null {
  const chord = eventToChord(event);
  for (const action of FOCUS_ACTIONS) {
    if (bindings.resolve(action) === chord) {
      return focusInDirection(tree, from, ACTION_DIRECTION[action]);
    }
  }
  return null;
}

/** Structural path equality (paths are short Branch arrays). */
export function samePath(a: PanePath, b: PanePath): boolean {
  return a.length === b.length && a.every((step, i) => step === b[i]);
}

/** First leaf path — a sane default focus target after a layout change. */
export function firstPanePath(tree: LayoutNode): PanePath {
  return collectPanes(tree)[0]?.path ?? [];
}
