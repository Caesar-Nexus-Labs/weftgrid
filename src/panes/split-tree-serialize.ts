// Split-tree serialize/deserialize — pure round-trip for P12 persistence.
//
// `LayoutNode` is already serde-shaped (P2 internally-tagged union), so this is
// NOT about inventing a wire format. P4 only PRODUCES a validated, deep-cloned
// tree that P12 can hand to JSON.stringify / persist to disk. The point of going
// through serialize→deserialize is:
//   1. Deep clone — break aliasing so a persisted snapshot never shares refs with
//      the live UI tree (mutating one must not corrupt the other).
//   2. Structural validation — reject malformed input early (P12 reads untrusted
//      on-disk JSON; an invalid layout should fail loud, not render garbage).
//   3. Normalize — clamp dividers + drop unknown fields so the round-trip is
//      idempotent and deep-equal to a clean original.
//
// P4 does NO disk I/O. Persist (write) + restore-on-launch + shell respawn = P12.

import {
  type LayoutNode,
  type PanelId,
  type SplitOrientation,
  clampDivider,
} from "$lib/model";

/** Plain JSON value as produced by JSON.parse (the P12 input boundary). */
type Json = unknown;

export class LayoutDeserializeError extends Error {
  constructor(message: string) {
    super(`split-tree-serialize: ${message}`);
    this.name = "LayoutDeserializeError";
  }
}

/**
 * Serialize a `LayoutNode` to a plain, deep-cloned JSON-safe value. The result
 * is a fresh object graph sharing no references with `tree`. P12 persists this.
 */
export function serialize(tree: LayoutNode): Json {
  return cloneNode(tree);
}

/**
 * Validate + deep-clone an untrusted JSON value back into a `LayoutNode`.
 * Throws `LayoutDeserializeError` on any structural problem. Dividers are
 * clamped so a tampered/old file still yields a legal tree.
 */
export function deserialize(value: Json): LayoutNode {
  return parseNode(value, "$");
}

function cloneNode(node: LayoutNode): LayoutNode {
  if (node.type === "pane") {
    const out: LayoutNode = { type: "pane", panel_ids: [...node.panel_ids] };
    if (node.selected_panel_id !== undefined) {
      out.selected_panel_id = node.selected_panel_id;
    }
    return out;
  }
  return {
    type: "split",
    orientation: node.orientation,
    divider_position: clampDivider(node.divider_position),
    first: cloneNode(node.first),
    second: cloneNode(node.second),
  };
}

function isRecord(value: Json): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

function parseNode(value: Json, path: string): LayoutNode {
  if (!isRecord(value)) {
    throw new LayoutDeserializeError(`${path} is not an object`);
  }
  const type = value.type;
  if (type === "pane") {
    return parsePane(value, path);
  }
  if (type === "split") {
    return parseSplit(value, path);
  }
  throw new LayoutDeserializeError(`${path}.type must be "pane" | "split", got ${JSON.stringify(type)}`);
}

function parsePane(value: Record<string, unknown>, path: string): LayoutNode {
  const rawIds = value.panel_ids;
  if (!Array.isArray(rawIds) || !rawIds.every((id): id is PanelId => typeof id === "string")) {
    throw new LayoutDeserializeError(`${path}.panel_ids must be an array of strings`);
  }
  const out: LayoutNode = { type: "pane", panel_ids: [...rawIds] };
  const selected = value.selected_panel_id;
  if (selected !== undefined) {
    if (typeof selected !== "string") {
      throw new LayoutDeserializeError(`${path}.selected_panel_id must be a string`);
    }
    out.selected_panel_id = selected;
  }
  return out;
}

function parseOrientation(value: unknown, path: string): SplitOrientation {
  if (value === "horizontal" || value === "vertical") {
    return value;
  }
  throw new LayoutDeserializeError(`${path}.orientation must be "horizontal" | "vertical"`);
}

function parseSplit(value: Record<string, unknown>, path: string): LayoutNode {
  const orientation = parseOrientation(value.orientation, path);
  const divider = value.divider_position;
  if (typeof divider !== "number" || Number.isNaN(divider)) {
    throw new LayoutDeserializeError(`${path}.divider_position must be a number`);
  }
  return {
    type: "split",
    orientation,
    divider_position: clampDivider(divider),
    first: parseNode(value.first, `${path}.first`),
    second: parseNode(value.second, `${path}.second`),
  };
}
