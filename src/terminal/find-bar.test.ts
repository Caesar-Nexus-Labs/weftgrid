import { describe, it, expect, vi } from "vitest";
import {
  FindController,
  type SearchAddonPort,
  type FindTerminal,
  type SearchResultChange,
} from "./find-bar";

/** Mock addon-search capturing calls + letting tests fire onDidChangeResults. */
function mockAddon() {
  let resultsHandler: ((e: SearchResultChange) => void) | null = null;
  const findNext = vi.fn((_term: string, _options?: Record<string, unknown>) => true);
  const findPrevious = vi.fn((_term: string, _options?: Record<string, unknown>) => true);
  const clearDecorations = vi.fn();
  const port: SearchAddonPort = {
    findNext,
    findPrevious,
    clearDecorations,
    onDidChangeResults(handler) {
      resultsHandler = handler;
      return { dispose() {} };
    },
  };
  return {
    ...port,
    findNext,
    findPrevious,
    clearDecorations,
    fireResults(e: SearchResultChange) {
      resultsHandler?.(e);
    },
  };
}

function mockTerm(selection = ""): FindTerminal {
  return { getSelection: () => selection };
}

describe("FindController", () => {
  it("setNeedle triggers an incremental findNext with the needle", () => {
    const addon = mockAddon();
    const fc = new FindController(addon, mockTerm());
    fc.setNeedle("error");
    expect(addon.findNext).toHaveBeenCalledTimes(1);
    const [term, opts] = addon.findNext.mock.calls[0];
    expect(term).toBe("error");
    expect((opts as Record<string, unknown>).incremental).toBe(true);
  });

  it("clears decorations + counter when the needle is emptied", () => {
    const addon = mockAddon();
    const fc = new FindController(addon, mockTerm());
    fc.setNeedle("x");
    fc.setNeedle("");
    expect(addon.clearDecorations).toHaveBeenCalled();
    expect(fc.getState().total).toBe(0);
    expect(fc.getState().selected).toBe(0);
  });

  it("maps onDidChangeResults to selected/total (1-based, cmux SearchState)", () => {
    const addon = mockAddon();
    const fc = new FindController(addon, mockTerm());
    const states: Array<{ selected: number; total: number }> = [];
    fc.subscribe((s) => states.push({ selected: s.selected, total: s.total }));

    addon.fireResults({ resultIndex: 2, resultCount: 7 });
    expect(fc.getState().selected).toBe(3); // 0-based 2 → 1-based 3
    expect(fc.getState().total).toBe(7);

    addon.fireResults({ resultIndex: -1, resultCount: 0 }); // no match
    expect(fc.getState().selected).toBe(0);
    expect(fc.getState().total).toBe(0);
  });

  it("findNext / findPrevious delegate to the addon", () => {
    const addon = mockAddon();
    const fc = new FindController(addon, mockTerm());
    fc.setNeedle("foo");
    addon.findNext.mockClear();
    fc.findNext();
    fc.findPrevious();
    expect(addon.findNext).toHaveBeenCalledWith("foo", expect.any(Object));
    expect(addon.findPrevious).toHaveBeenCalledWith("foo", expect.any(Object));
  });

  it("useSelection (Cmd+E) seeds the needle from terminal selection and opens", () => {
    const addon = mockAddon();
    const fc = new FindController(addon, mockTerm("selected-term"));
    fc.useSelection();
    expect(fc.getState().needle).toBe("selected-term");
    expect(fc.getState().open).toBe(true);
    expect(addon.findNext).toHaveBeenCalledWith("selected-term", expect.any(Object));
  });

  it("ignores empty selection on useSelection", () => {
    const addon = mockAddon();
    const fc = new FindController(addon, mockTerm(""));
    fc.useSelection();
    expect(fc.getState().needle).toBe("");
    expect(addon.findNext).not.toHaveBeenCalled();
  });

  it("handleKey routes default Ctrl shortcuts (F open, G next, Shift+G prev, E selection)", () => {
    const addon = mockAddon();
    const fc = new FindController(addon, mockTerm("sel"));
    fc.setNeedle("q");
    addon.findNext.mockClear();
    addon.findPrevious.mockClear();

    expect(fc.handleKey({ key: "f", ctrlKey: true, metaKey: false, shiftKey: false })).toBe(true);
    expect(fc.getState().open).toBe(true);

    expect(fc.handleKey({ key: "g", ctrlKey: true, metaKey: false, shiftKey: false })).toBe(true);
    expect(addon.findNext).toHaveBeenCalled();

    expect(fc.handleKey({ key: "g", ctrlKey: true, metaKey: false, shiftKey: true })).toBe(true);
    expect(addon.findPrevious).toHaveBeenCalled();

    expect(fc.handleKey({ key: "e", ctrlKey: true, metaKey: false, shiftKey: false })).toBe(true);
    expect(fc.getState().needle).toBe("sel");

    // Non-modified key is not handled.
    expect(fc.handleKey({ key: "f", ctrlKey: false, metaKey: false, shiftKey: false })).toBe(false);
  });

  it("close clears decorations and resets state", () => {
    const addon = mockAddon();
    const fc = new FindController(addon, mockTerm());
    fc.open();
    fc.setNeedle("x");
    fc.close();
    expect(addon.clearDecorations).toHaveBeenCalled();
    expect(fc.getState().open).toBe(false);
    expect(fc.getState().needle).toBe("");
  });
});
