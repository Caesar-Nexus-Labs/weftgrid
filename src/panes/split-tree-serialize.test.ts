import { describe, it, expect } from "vitest";
import { serialize, deserialize, LayoutDeserializeError } from "./split-tree-serialize";
import { pane, split } from "./split-tree";
import type { LayoutNode } from "$lib/model";

describe("split-tree-serialize round-trip", () => {
  it("deserialize(serialize(tree)) deep-equals a single pane", () => {
    const tree = pane(["a", "b"], "b");
    expect(deserialize(serialize(tree))).toEqual(tree);
  });

  it("deep-equals a nested multi-level tree", () => {
    const tree = split(
      "horizontal",
      pane(["a"]),
      split("vertical", pane(["b", "c"], "c"), pane(["d"]), 0.7),
      0.35,
    );
    expect(deserialize(serialize(tree))).toEqual(tree);
  });

  it("serialize deep-clones — mutating the output never touches the source", () => {
    const tree = split("horizontal", pane(["a"]), pane(["b"]));
    const wire = serialize(tree) as { first: { panel_ids: string[] }; divider_position: number };
    wire.first.panel_ids.push("mutated");
    wire.divider_position = 0.99;
    // Original is untouched.
    if (tree.type !== "split") throw new Error("expected split");
    expect((tree.first as { panel_ids: string[] }).panel_ids).toEqual(["a"]);
    expect(tree.divider_position).toBe(0.5);
  });

  it("clamps an out-of-range divider on deserialize (tampered/old file)", () => {
    const wire = {
      type: "split",
      orientation: "horizontal",
      divider_position: 5,
      first: { type: "pane", panel_ids: ["a"] },
      second: { type: "pane", panel_ids: ["b"] },
    };
    const node = deserialize(wire) as Extract<LayoutNode, { type: "split" }>;
    expect(node.divider_position).toBe(0.9);
  });

  it("drops unknown fields so round-trip stays normalized", () => {
    const wire = {
      type: "pane",
      panel_ids: ["a"],
      selected_panel_id: "a",
      junk: "ignored",
    };
    expect(deserialize(wire)).toEqual(pane(["a"], "a"));
  });

  it("rejects an unknown node type", () => {
    expect(() => deserialize({ type: "frame" })).toThrow(LayoutDeserializeError);
  });

  it("rejects non-string panel_ids", () => {
    expect(() => deserialize({ type: "pane", panel_ids: [1, 2] })).toThrow(LayoutDeserializeError);
  });

  it("rejects a split missing a child", () => {
    expect(() =>
      deserialize({ type: "split", orientation: "horizontal", divider_position: 0.5, first: { type: "pane", panel_ids: ["a"] } }),
    ).toThrow(LayoutDeserializeError);
  });

  it("rejects an invalid orientation", () => {
    expect(() =>
      deserialize({
        type: "split",
        orientation: "diagonal",
        divider_position: 0.5,
        first: { type: "pane", panel_ids: ["a"] },
        second: { type: "pane", panel_ids: ["b"] },
      }),
    ).toThrow(LayoutDeserializeError);
  });
});
