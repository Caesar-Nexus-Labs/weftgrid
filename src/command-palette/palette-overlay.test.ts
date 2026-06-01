// Scope detection + overlay navigation tests (P16).
//
// Scope: `>foo` → commands (prefix stripped), `foo`/empty → switcher.
// Nav: Ctrl+N/ArrowDown next, Ctrl+P/ArrowUp prev (wrap), Enter runs, Esc
// dismisses + restores focus. Pure logic, jsdom-free.

import { describe, it, expect, vi } from "vitest";
import {
  detectScope,
  PaletteOverlay,
  type OverlayHandlers,
  type ResultRow,
} from "./palette-overlay";

function rows(...ids: string[]): ResultRow[] {
  return ids.map((id) => ({ id, text: id, indices: [], disabled: false }));
}

function handlers(): OverlayHandlers & { run: ReturnType<typeof vi.fn>; restoreFocus: ReturnType<typeof vi.fn> } {
  const run = vi.fn(async () => true);
  const restoreFocus = vi.fn();
  return { run, restoreFocus };
}

describe("detectScope", () => {
  it("treats a leading > as commands scope and strips it", () => {
    expect(detectScope(">split")).toEqual({ scope: "commands", term: "split" });
  });

  it("trims whitespace after the > prefix", () => {
    expect(detectScope(">  find")).toEqual({ scope: "commands", term: "find" });
  });

  it("treats a non-> query as switcher scope, term unchanged", () => {
    expect(detectScope("proj")).toEqual({ scope: "switcher", term: "proj" });
  });

  it("treats an empty query as switcher scope", () => {
    expect(detectScope("")).toEqual({ scope: "switcher", term: "" });
  });
});

describe("PaletteOverlay", () => {
  it("openCommands seeds > and commands scope; openSwitcher is empty/switcher", () => {
    const o = new PaletteOverlay();
    o.openCommands();
    expect(o.getState().open).toBe(true);
    expect(o.getState().query).toBe(">");
    expect(o.getState().scope).toBe("commands");
    expect(o.searchTerm()).toBe("");

    o.openSwitcher();
    expect(o.getState().query).toBe("");
    expect(o.getState().scope).toBe("switcher");
  });

  it("setResults selects the first row; move wraps both directions", () => {
    const o = new PaletteOverlay();
    o.openCommands();
    o.setResults(rows("a", "b", "c"));
    expect(o.getState().selectedIndex).toBe(0);

    o.move(1);
    expect(o.selected()?.id).toBe("b");
    o.move(-1);
    o.move(-1); // wrap past the top → last
    expect(o.selected()?.id).toBe("c");
    o.move(1); // wrap past the bottom → first
    expect(o.selected()?.id).toBe("a");
  });

  it("Ctrl+N/Ctrl+P and arrows move selection", async () => {
    const o = new PaletteOverlay();
    const h = handlers();
    o.openCommands();
    o.setResults(rows("a", "b"));

    await o.handleKey({ key: "n", ctrlKey: true, metaKey: false, shiftKey: false }, h);
    expect(o.selected()?.id).toBe("b");
    await o.handleKey({ key: "p", ctrlKey: true, metaKey: false, shiftKey: false }, h);
    expect(o.selected()?.id).toBe("a");
    await o.handleKey({ key: "ArrowDown", ctrlKey: false, metaKey: false, shiftKey: false }, h);
    expect(o.selected()?.id).toBe("b");
    await o.handleKey({ key: "ArrowUp", ctrlKey: false, metaKey: false, shiftKey: false }, h);
    expect(o.selected()?.id).toBe("a");
  });

  it("Enter runs the selected id with the active scope, then dismisses", async () => {
    const o = new PaletteOverlay();
    const h = handlers();
    o.openCommands();
    o.setResults(rows("split.right", "split.down"));
    o.move(1);

    await o.handleKey({ key: "Enter", ctrlKey: false, metaKey: false, shiftKey: false }, h);
    expect(h.run).toHaveBeenCalledWith("split.down", "commands");
    expect(o.getState().open).toBe(false);
    expect(h.restoreFocus).toHaveBeenCalled();
  });

  it("keeps the overlay open when run() returns false (e.g. confirm cancelled)", async () => {
    const o = new PaletteOverlay();
    const h = handlers();
    h.run.mockResolvedValueOnce(false);
    o.openCommands();
    o.setResults(rows("deploy"));

    await o.handleKey({ key: "Enter", ctrlKey: false, metaKey: false, shiftKey: false }, h);
    expect(h.run).toHaveBeenCalledWith("deploy", "commands");
    expect(o.getState().open).toBe(true);
    expect(h.restoreFocus).not.toHaveBeenCalled();
  });

  it("does not run a disabled row", async () => {
    const o = new PaletteOverlay();
    const h = handlers();
    o.openCommands();
    o.setResults([{ id: "find.next", text: "Find Next", indices: [], disabled: true }]);

    await o.handleKey({ key: "Enter", ctrlKey: false, metaKey: false, shiftKey: false }, h);
    expect(h.run).not.toHaveBeenCalled();
    expect(o.getState().open).toBe(true);
  });

  it("Escape dismisses and restores prior focus", async () => {
    const o = new PaletteOverlay();
    const h = handlers();
    o.openCommands();
    o.setResults(rows("a"));

    await o.handleKey({ key: "Escape", ctrlKey: false, metaKey: false, shiftKey: false }, h);
    expect(o.getState().open).toBe(false);
    expect(h.restoreFocus).toHaveBeenCalled();
  });

  it("ignores keys when closed", async () => {
    const o = new PaletteOverlay();
    const h = handlers();
    const handled = await o.handleKey(
      { key: "Enter", ctrlKey: false, metaKey: false, shiftKey: false },
      h,
    );
    expect(handled).toBe(false);
  });
});
