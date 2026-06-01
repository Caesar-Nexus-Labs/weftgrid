// @vitest-environment jsdom
import { describe, it, expect, vi } from "vitest";
import {
  createTabBar,
  selectedIndex,
  nextSelection,
  setTabHighlight,
  type TabModel,
} from "./tabs";

const model: TabModel = { panelIds: ["a", "b", "c"], selectedPanelId: "b" };

describe("tabs selection math", () => {
  it("selectedIndex resolves the selected panel, defaulting to 0", () => {
    expect(selectedIndex(model)).toBe(1);
    expect(selectedIndex({ panelIds: ["a", "b"] })).toBe(0);
    // Stale selection falls back to 0.
    expect(selectedIndex({ panelIds: ["a"], selectedPanelId: "gone" })).toBe(0);
  });

  it("nextSelection steps and wraps around the surfaces", () => {
    expect(nextSelection(model, +1)).toBe("c");
    expect(nextSelection({ ...model, selectedPanelId: "c" }, +1)).toBe("a"); // wrap
    expect(nextSelection({ ...model, selectedPanelId: "a" }, -1)).toBe("c"); // wrap back
    expect(nextSelection({ panelIds: [] }, +1)).toBeUndefined();
  });
});

describe("tab bar element", () => {
  it("renders one tab per surface with tab roles + selected state", () => {
    const bar = createTabBar(model);
    expect(bar.getAttribute("role")).toBe("tablist");
    const tabs = bar.querySelectorAll('[role="tab"]');
    expect(tabs).toHaveLength(3);
    expect(tabs[1].getAttribute("aria-selected")).toBe("true");
    expect(tabs[0].getAttribute("aria-selected")).toBe("false");
  });

  it("orientation reflects in aria-orientation + class", () => {
    const v = createTabBar(model, { orientation: "vertical" });
    expect(v.getAttribute("aria-orientation")).toBe("vertical");
    expect(v.className).toContain("pane-tabbar--vertical");
  });

  it("clicking a tab fires onSelect with its panel id", () => {
    const onSelect = vi.fn();
    const bar = createTabBar(model, { onSelect });
    (bar.querySelectorAll('[role="tab"]')[2] as HTMLElement).click();
    expect(onSelect).toHaveBeenCalledWith("c");
  });

  it("horizontal arrow keys move selection (keybind switch)", () => {
    const onSelect = vi.fn();
    const bar = createTabBar(model, { onSelect, orientation: "horizontal" });
    bar.dispatchEvent(new window.KeyboardEvent("keydown", { key: "ArrowRight" }));
    expect(onSelect).toHaveBeenLastCalledWith("c"); // b → c
    bar.dispatchEvent(new window.KeyboardEvent("keydown", { key: "ArrowLeft" }));
    expect(onSelect).toHaveBeenLastCalledWith("a"); // b → a
  });

  it("vertical arrows drive a vertical strip; Home/End jump to ends", () => {
    const onSelect = vi.fn();
    const bar = createTabBar(model, { onSelect, orientation: "vertical" });
    bar.dispatchEvent(new window.KeyboardEvent("keydown", { key: "ArrowDown" }));
    expect(onSelect).toHaveBeenLastCalledWith("c");
    bar.dispatchEvent(new window.KeyboardEvent("keydown", { key: "Home" }));
    expect(onSelect).toHaveBeenLastCalledWith("a");
    bar.dispatchEvent(new window.KeyboardEvent("keydown", { key: "End" }));
    expect(onSelect).toHaveBeenLastCalledWith("c");
  });

  it("custom label is used for tab text", () => {
    const bar = createTabBar(model, { label: (id) => `tab:${id}` });
    expect((bar.querySelectorAll('[role="tab"]')[0] as HTMLElement).textContent).toBe("tab:a");
  });
});

describe("tab highlight (P5b additive)", () => {
  it("setTabHighlight marks the matching tab unread with an a11y description", () => {
    const bar = createTabBar(model);
    expect(setTabHighlight(bar, "b", true)).toBe(true);

    const tab = bar.querySelectorAll('[role="tab"]')[1] as HTMLElement;
    expect(tab.classList.contains("pane-tab--unread")).toBe(true);
    expect(tab.dataset.unread).toBe("true");
    // Not color-only: assistive tech can announce the unread tab.
    expect(tab.getAttribute("aria-description")).toBe("Unread notification");
  });

  it("setTabHighlight(false) clears the unread marking", () => {
    const bar = createTabBar(model);
    setTabHighlight(bar, "b", true);

    setTabHighlight(bar, "b", false);
    const tab = bar.querySelectorAll('[role="tab"]')[1] as HTMLElement;
    expect(tab.classList.contains("pane-tab--unread")).toBe(false);
    expect(tab.dataset.unread).toBeUndefined();
    expect(tab.hasAttribute("aria-description")).toBe(false);
  });

  it("setTabHighlight returns false for an unknown panel id", () => {
    const bar = createTabBar(model);
    expect(setTabHighlight(bar, "missing", true)).toBe(false);
  });
});

