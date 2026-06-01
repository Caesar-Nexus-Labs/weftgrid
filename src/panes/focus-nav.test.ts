import { describe, it, expect } from "vitest";
import {
  computeRects,
  focusInDirection,
  handleKey,
  samePath,
  firstPanePath,
} from "./focus-nav";
import { pane, split } from "./split-tree";
import {
  DefaultKeybindingSource,
  type KeybindingSource,
  type ChordEvent,
} from "./keybinding-source";

// A 2x2 grid:  [ (a | b) ]   horizontal at root splits left/right columns,
//              [ (c | d) ]   each column is a vertical split top/bottom.
// left column = first (a top, c bottom); right column = second (b top, d bottom).
const grid2x2 = split(
  "horizontal",
  split("vertical", pane(["a"]), pane(["c"])),
  split("vertical", pane(["b"]), pane(["d"])),
);

const PATH = {
  a: ["first", "first"] as const,
  c: ["first", "second"] as const,
  b: ["second", "first"] as const,
  d: ["second", "second"] as const,
};

describe("focus-nav geometry", () => {
  it("computeRects assigns disjoint normalized rects covering the unit square", () => {
    const rects = computeRects(grid2x2);
    expect(rects).toHaveLength(4);
    const a = rects.find((r) => samePath(r.path, [...PATH.a]))!;
    expect(a).toMatchObject({ x: 0, y: 0, width: 0.5, height: 0.5 });
    const d = rects.find((r) => samePath(r.path, [...PATH.d]))!;
    expect(d).toMatchObject({ x: 0.5, y: 0.5, width: 0.5, height: 0.5 });
  });

  it("focus right from a (top-left) lands on b (top-right)", () => {
    expect(focusInDirection(grid2x2, [...PATH.a], "right")).toEqual([...PATH.b]);
  });

  it("focus down from a (top-left) lands on c (bottom-left)", () => {
    expect(focusInDirection(grid2x2, [...PATH.a], "down")).toEqual([...PATH.c]);
  });

  it("focus left from d (bottom-right) lands on c (bottom-left)", () => {
    expect(focusInDirection(grid2x2, [...PATH.d], "left")).toEqual([...PATH.c]);
  });

  it("returns null at the edge (nothing to the left of a)", () => {
    expect(focusInDirection(grid2x2, [...PATH.a], "left")).toBeNull();
  });

  it("single pane has no neighbor in any direction", () => {
    const solo = pane(["x"]);
    expect(focusInDirection(solo, [], "right")).toBeNull();
    expect(focusInDirection(solo, [], "down")).toBeNull();
  });
});

describe("focus-nav binding via KeybindingSource (no hardcoded literals)", () => {
  const ctrlAlt = (key: string): ChordEvent => ({
    key,
    ctrlKey: true,
    altKey: true,
    metaKey: false,
    shiftKey: false,
  });

  it("default source binds Ctrl+Alt+arrow → directional move", () => {
    const bindings = new DefaultKeybindingSource();
    expect(handleKey(grid2x2, [...PATH.a], ctrlAlt("ArrowRight"), bindings)).toEqual([...PATH.b]);
    expect(handleKey(grid2x2, [...PATH.a], ctrlAlt("ArrowDown"), bindings)).toEqual([...PATH.c]);
  });

  it("ignores a chord with no matching binding", () => {
    const bindings = new DefaultKeybindingSource();
    const plainArrow: ChordEvent = {
      key: "ArrowRight",
      ctrlKey: false,
      altKey: false,
      metaKey: false,
      shiftKey: false,
    };
    expect(handleKey(grid2x2, [...PATH.a], plainArrow, bindings)).toBeNull();
  });

  it("honors a custom binding source (P12 registry stand-in)", () => {
    // Mock registry rebinds focus-right to Ctrl+Shift+L.
    const custom: KeybindingSource = {
      resolve: (action) => (action === "pane.focus.right" ? "ctrl+shift+l" : null),
    };
    const chord: ChordEvent = {
      key: "L",
      ctrlKey: true,
      altKey: false,
      metaKey: false,
      shiftKey: true,
    };
    expect(handleKey(grid2x2, [...PATH.a], chord, custom)).toEqual([...PATH.b]);
    // The old default no longer fires under the custom source.
    expect(
      handleKey(grid2x2, [...PATH.a], { key: "ArrowRight", ctrlKey: true, altKey: true, metaKey: false, shiftKey: false }, custom),
    ).toBeNull();
  });
});

describe("focus-nav helpers", () => {
  it("firstPanePath returns the depth-first first leaf", () => {
    expect(firstPanePath(grid2x2)).toEqual([...PATH.a]);
    expect(firstPanePath(pane(["solo"]))).toEqual([]);
  });
});
