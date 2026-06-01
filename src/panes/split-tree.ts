// Split-tree ops — immutable transforms over the P2 `LayoutNode` contract.
//
// P4 does NOT redefine the tree shape: it imports `LayoutNode` from $lib/model
// (binary split tree, panes carry `panel_ids`, splits carry `divider_position`).
// Every op returns a NEW tree (structural sharing of untouched subtrees) so the
// UI can diff/undo and so re-layout never mutates state in place.
//
// Addressing: panes have no id in the model, so a leaf/branch is located by a
// `PanePath` — the list of "first"/"second" turns from the root. Root = `[]`.
// This keeps ops pure + deterministic and avoids inventing an id the contract
// does not have.
//
// Orientation convention (shared with pane-renderer + focus-nav, locked here):
//   "horizontal" → children laid out left|right (flex row); divider_position is
//                  `first`'s WIDTH fraction; the divider bar is vertical.
//   "vertical"   → children laid out top|bottom (flex column); divider_position
//                  is `first`'s HEIGHT fraction; the divider bar is horizontal.

import {
  type LayoutNode,
  type PanelId,
  type SplitOrientation,
  clampDivider,
} from "$lib/model";

/** A pane (leaf) node — narrowed from the `LayoutNode` union. */
export type PaneNode = Extract<LayoutNode, { type: "pane" }>;
/** A split (branch) node — narrowed from the `LayoutNode` union. */
export type SplitNode = Extract<LayoutNode, { type: "split" }>;

/** One turn down the binary tree. */
export type Branch = "first" | "second";
/** Path from the root to a node (root = empty path). */
export type PanePath = Branch[];

export function isPane(node: LayoutNode): node is PaneNode {
  return node.type === "pane";
}

export function isSplit(node: LayoutNode): node is SplitNode {
  return node.type === "split";
}

/** Build a pane leaf. `selectedPanelId` defaults to the first panel when present. */
export function pane(panelIds: PanelId[], selectedPanelId?: PanelId): PaneNode {
  const node: PaneNode = { type: "pane", panel_ids: [...panelIds] };
  const selected = selectedPanelId ?? panelIds[0];
  if (selected !== undefined) {
    node.selected_panel_id = selected;
  }
  return node;
}

/** Build a split branch with a clamped divider (defaults to an even 0.5). */
export function split(
  orientation: SplitOrientation,
  first: LayoutNode,
  second: LayoutNode,
  dividerPosition = 0.5,
): SplitNode {
  return {
    type: "split",
    orientation,
    divider_position: clampDivider(dividerPosition),
    first,
    second,
  };
}

/** Resolve the node at `path`, or `undefined` when the path runs off a leaf. */
export function getNode(tree: LayoutNode, path: PanePath): LayoutNode | undefined {
  let node: LayoutNode = tree;
  for (const branch of path) {
    if (!isSplit(node)) {
      return undefined;
    }
    node = node[branch];
  }
  return node;
}

/** Rebuild the tree replacing only the node at `path` (rest is shared). */
function mapAt(
  node: LayoutNode,
  path: PanePath,
  fn: (n: LayoutNode) => LayoutNode,
): LayoutNode {
  if (path.length === 0) {
    return fn(node);
  }
  if (!isSplit(node)) {
    throw new Error("split-tree: path descends past a leaf");
  }
  const [head, ...rest] = path;
  return { ...node, [head]: mapAt(node[head], rest, fn) };
}

export interface SplitOptions {
  /** First's fraction of the parent axis (clamped 0.1..0.9). Defaults 0.5. */
  dividerPosition?: number;
  /** Place the NEW leaf in `first` (top/left). Defaults false (new = second). */
  placeNewFirst?: boolean;
}

/**
 * Split the leaf at `path` into a branch: the existing pane plus `newLeaf`.
 * The new leaf carries its own freshly-spawned panel(s) — the caller owns
 * PanelId creation + PTY spawn (P3). Throws if `path` is not a pane leaf.
 */
export function splitLeaf(
  tree: LayoutNode,
  path: PanePath,
  orientation: SplitOrientation,
  newLeaf: LayoutNode,
  opts: SplitOptions = {},
): LayoutNode {
  return mapAt(tree, path, (n) => {
    if (!isPane(n)) {
      throw new Error("split-tree: can only split a pane leaf");
    }
    const first = opts.placeNewFirst ? newLeaf : n;
    const second = opts.placeNewFirst ? n : newLeaf;
    return split(orientation, first, second, opts.dividerPosition ?? 0.5);
  });
}

/**
 * Close the pane at `path`. Its parent split collapses to the surviving sibling
 * (which keeps its own subtree). Returns `null` when the last pane (root) is
 * closed — the caller decides what an empty workspace means.
 */
export function closePane(tree: LayoutNode, path: PanePath): LayoutNode | null {
  if (path.length === 0) {
    return null;
  }
  return removeAt(tree, path);
}

function removeAt(node: LayoutNode, path: PanePath): LayoutNode {
  if (!isSplit(node)) {
    throw new Error("split-tree: path descends past a leaf");
  }
  const [head, ...rest] = path;
  const sibling: Branch = head === "first" ? "second" : "first";
  if (rest.length === 0) {
    // Remove the targeted child → the split collapses to its sibling subtree.
    return node[sibling];
  }
  return { ...node, [head]: removeAt(node[head], rest) };
}

/** Set the divider fraction of the split at `path` (clamped). Throws if not a split. */
export function resizeDivider(
  tree: LayoutNode,
  path: PanePath,
  position: number,
): LayoutNode {
  return mapAt(tree, path, (n) => {
    if (!isSplit(n)) {
      throw new Error("split-tree: resize target is not a split");
    }
    return { ...n, divider_position: clampDivider(position) };
  });
}

/** Reset every divider in the tree back to an even 0.5 split. */
export function equalize(node: LayoutNode): LayoutNode {
  if (isPane(node)) {
    return node;
  }
  return {
    ...node,
    divider_position: 0.5,
    first: equalize(node.first),
    second: equalize(node.second),
  };
}

/** A pane leaf paired with the path that addresses it. */
export interface LocatedPane {
  path: PanePath;
  pane: PaneNode;
}

/** Depth-first list of every pane leaf with its path (first before second). */
export function collectPanes(node: LayoutNode, path: PanePath = []): LocatedPane[] {
  if (isPane(node)) {
    return [{ path, pane: node }];
  }
  return [
    ...collectPanes(node.first, [...path, "first"]),
    ...collectPanes(node.second, [...path, "second"]),
  ];
}
