// @vitest-environment jsdom
import { describe, it, expect } from "vitest";
import {
  PaneAriaRole,
  applyAria,
  applyDividerAria,
  setRovingTabindex,
  setTabSelected,
  rovingNextIndex,
  focusElement,
} from "./a11y";

function el(): HTMLElement {
  return document.createElement("div");
}

describe("a11y ARIA roles", () => {
  it("applyAria sets role + attributes and removes undefined ones", () => {
    const node = el();
    applyAria(node, PaneAriaRole.pane, { "aria-label": "terminal", tabindex: 0 });
    expect(node.getAttribute("role")).toBe("group");
    expect(node.getAttribute("aria-label")).toBe("terminal");
    expect(node.getAttribute("tabindex")).toBe("0");

    applyAria(node, PaneAriaRole.pane, { "aria-label": undefined });
    expect(node.hasAttribute("aria-label")).toBe(false);
  });

  it("divider gets separator role with inverted bar orientation + value range", () => {
    const horiz = el();
    // A horizontal split lays children side-by-side → the divider BAR is vertical.
    applyDividerAria(horiz, "horizontal", 0.5);
    expect(horiz.getAttribute("role")).toBe("separator");
    expect(horiz.getAttribute("aria-orientation")).toBe("vertical");
    expect(horiz.getAttribute("aria-valuemin")).toBe("10");
    expect(horiz.getAttribute("aria-valuemax")).toBe("90");
    expect(horiz.getAttribute("aria-valuenow")).toBe("50");
    expect(horiz.getAttribute("tabindex")).toBe("0");

    const vert = el();
    applyDividerAria(vert, "vertical", 0.3);
    expect(vert.getAttribute("aria-orientation")).toBe("horizontal");
    expect(vert.getAttribute("aria-valuenow")).toBe("30");
  });

  it("role vocabulary covers pane/tab/divider/surface", () => {
    expect(PaneAriaRole.divider).toBe("separator");
    expect(PaneAriaRole.tablist).toBe("tablist");
    expect(PaneAriaRole.tab).toBe("tab");
    expect(PaneAriaRole.surface).toBe("tabpanel");
  });
});

describe("a11y keyboard nav (roving tabindex)", () => {
  it("setRovingTabindex makes exactly one element tabbable", () => {
    const items = [el(), el(), el()];
    setRovingTabindex(items, 1);
    expect(items.map((i) => i.tabIndex)).toEqual([-1, 0, -1]);
  });

  it("setTabSelected toggles aria-selected + tabindex together", () => {
    const tab = el();
    setTabSelected(tab, true);
    expect(tab.getAttribute("aria-selected")).toBe("true");
    expect(tab.tabIndex).toBe(0);
    setTabSelected(tab, false);
    expect(tab.getAttribute("aria-selected")).toBe("false");
    expect(tab.tabIndex).toBe(-1);
  });

  it("rovingNextIndex wraps around both ends", () => {
    expect(rovingNextIndex(0, 3, +1)).toBe(1);
    expect(rovingNextIndex(2, 3, +1)).toBe(0); // wrap forward
    expect(rovingNextIndex(0, 3, -1)).toBe(2); // wrap backward
    expect(rovingNextIndex(0, 0, +1)).toBe(-1); // empty
  });

  it("focusElement moves focus and tolerates null", () => {
    const node = el();
    document.body.appendChild(node);
    node.tabIndex = 0;
    focusElement(node);
    expect(document.activeElement).toBe(node);
    expect(() => focusElement(null)).not.toThrow();
  });
});
