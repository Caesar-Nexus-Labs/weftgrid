import { describe, it, expect } from "vitest";
import {
  pane,
  split,
  splitLeaf,
  closePane,
  resizeDivider,
  equalize,
  getNode,
  collectPanes,
  isPane,
  isSplit,
} from "./split-tree";
import { DIVIDER_MIN, DIVIDER_MAX } from "$lib/model";

describe("split-tree ops", () => {
  it("splits a leaf into a branch holding the original + new leaf", () => {
    const tree = pane(["a"]);
    const next = splitLeaf(tree, [], "horizontal", pane(["b"]));

    expect(isSplit(next)).toBe(true);
    if (!isSplit(next)) return;
    expect(next.orientation).toBe("horizontal");
    expect(next.divider_position).toBe(0.5);
    // Default places the new leaf second (right/bottom), original stays first.
    expect(next.first).toEqual(pane(["a"]));
    expect(next.second).toEqual(pane(["b"]));
    // Original tree is untouched (immutability).
    expect(tree).toEqual(pane(["a"]));
  });

  it("places the new leaf first when placeNewFirst is set", () => {
    const next = splitLeaf(pane(["a"]), [], "vertical", pane(["b"]), {
      placeNewFirst: true,
    });
    if (!isSplit(next)) throw new Error("expected split");
    expect(next.first).toEqual(pane(["b"]));
    expect(next.second).toEqual(pane(["a"]));
  });

  it("splits a nested leaf addressed by path", () => {
    const tree = split("horizontal", pane(["a"]), pane(["b"]));
    const next = splitLeaf(tree, ["second"], "vertical", pane(["c"]));

    const target = getNode(next, ["second"]);
    expect(target && isSplit(target)).toBe(true);
    if (!target || !isSplit(target)) return;
    expect(target.orientation).toBe("vertical");
    expect(target.first).toEqual(pane(["b"]));
    expect(target.second).toEqual(pane(["c"]));
    // Sibling subtree is shared/untouched.
    expect(getNode(next, ["first"])).toEqual(pane(["a"]));
  });

  it("rejects splitting where the path is not a pane leaf", () => {
    const tree = split("horizontal", pane(["a"]), pane(["b"]));
    expect(() => splitLeaf(tree, [], "vertical", pane(["c"]))).toThrow();
  });

  it("collapses the parent split to the surviving sibling on close", () => {
    const tree = split("horizontal", pane(["a"]), pane(["b"]));
    const next = closePane(tree, ["first"]);
    // Branch collapses to the surviving child leaf.
    expect(next).toEqual(pane(["b"]));
  });

  it("preserves the surviving sibling's whole subtree on close", () => {
    const surviving = split("vertical", pane(["b"]), pane(["c"]));
    const tree = split("horizontal", pane(["a"]), surviving);
    const next = closePane(tree, ["first"]);
    expect(next).toEqual(surviving);
  });

  it("closing a nested pane only rebalances its parent split", () => {
    // root: [ a | (b / c) ]   close c → root: [ a | b ]
    const tree = split("horizontal", pane(["a"]), split("vertical", pane(["b"]), pane(["c"])));
    const next = closePane(tree, ["second", "second"]);
    expect(next).toEqual(split("horizontal", pane(["a"]), pane(["b"])));
  });

  it("returns null when the root (last) pane is closed", () => {
    expect(closePane(pane(["a"]), [])).toBeNull();
  });

  it("resizes the divider and clamps into the legal range", () => {
    const tree = split("horizontal", pane(["a"]), pane(["b"]));

    const widened = resizeDivider(tree, [], 0.72);
    if (!isSplit(widened)) throw new Error("expected split");
    expect(widened.divider_position).toBe(0.72);

    // Below the floor clamps to DIVIDER_MIN.
    const tooSmall = resizeDivider(tree, [], 0.02);
    if (!isSplit(tooSmall)) throw new Error("expected split");
    expect(tooSmall.divider_position).toBe(DIVIDER_MIN);

    // Above the ceiling clamps to DIVIDER_MAX.
    const tooBig = resizeDivider(tree, [], 0.99);
    if (!isSplit(tooBig)) throw new Error("expected split");
    expect(tooBig.divider_position).toBe(DIVIDER_MAX);
  });

  it("rejects resizing a node that is not a split", () => {
    expect(() => resizeDivider(pane(["a"]), [], 0.5)).toThrow();
  });

  it("equalize resets every divider in the tree to 0.5", () => {
    const tree = split(
      "horizontal",
      split("vertical", pane(["a"]), pane(["b"]), 0.8),
      pane(["c"]),
      0.3,
    );
    const next = equalize(tree);
    if (!isSplit(next)) throw new Error("expected split");
    expect(next.divider_position).toBe(0.5);
    const firstChild = next.first;
    if (!isSplit(firstChild)) throw new Error("expected nested split");
    expect(firstChild.divider_position).toBe(0.5);
    // Leaves are unchanged.
    expect(isPane(next.second)).toBe(true);
  });

  it("collectPanes lists every leaf with its path depth-first", () => {
    const tree = split("horizontal", pane(["a"]), split("vertical", pane(["b"]), pane(["c"])));
    const located = collectPanes(tree);
    expect(located.map((l) => l.path)).toEqual([
      ["first"],
      ["second", "first"],
      ["second", "second"],
    ]);
    expect(located.map((l) => l.pane.panel_ids[0])).toEqual(["a", "b", "c"]);
  });
});
