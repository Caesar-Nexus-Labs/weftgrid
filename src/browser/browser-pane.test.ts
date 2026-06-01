import { describe, it, expect, vi } from "vitest";
import {
  BrowserPaneController,
  buildSyncPayload,
  type AnchorRect,
  type WindowFrame,
  type SyncBoundsPayload,
} from "./browser-pane";

const FRAME: WindowFrame = {
  outerX: 100,
  outerY: 100,
  insetX: 8,
  insetY: 31,
  scale: 1.5,
};

function rect(overrides: Partial<AnchorRect> = {}): AnchorRect {
  return { x: 300, y: 80, width: 900, height: 600, ...overrides };
}

describe("buildSyncPayload", () => {
  it("maps gathered inputs to the wire shape (frame physical, anchor CSS)", () => {
    const payload = buildSyncPayload({
      paneId: "pane-1",
      rect: rect(),
      scrollX: 0,
      scrollY: 0,
      frame: FRAME,
      visible: true,
    });
    expect(payload).toEqual<SyncBoundsPayload>({
      paneId: "pane-1",
      mainOuterX: 100,
      mainOuterY: 100,
      clientInsetX: 8,
      clientInsetY: 31,
      anchorX: 300,
      anchorY: 80,
      anchorWidth: 900,
      anchorHeight: 600,
      scrollX: 0,
      scrollY: 0,
      mainScale: 1.5,
      visible: true,
    });
  });
});

function mockController(initialRect: AnchorRect) {
  let current = initialRect;
  const invoke = vi.fn().mockResolvedValue(undefined);
  const getRect = vi.fn(() => current);
  const getFrame = vi.fn(() => FRAME);
  const controller = new BrowserPaneController({
    paneId: "pane-1",
    invoke: invoke as never,
    getRect,
    getFrame,
  });
  return {
    controller,
    invoke,
    setRect(r: AnchorRect) {
      current = r;
    },
  };
}

describe("BrowserPaneController.report", () => {
  it("reports the current rect+scroll on a layout change (mocked getBoundingClientRect)", async () => {
    const h = mockController(rect());
    await h.controller.report(0, 0);
    expect(h.invoke).toHaveBeenCalledWith("browser_sync_bounds", {
      params: expect.objectContaining({ anchorX: 300, anchorY: 80, anchorWidth: 900, anchorHeight: 600 }),
    });

    // Layout shifts (e.g. divider drag): the next report reflects the new rect.
    h.invoke.mockClear();
    h.setRect(rect({ x: 420, width: 700 }));
    await h.controller.report(0, 0);
    const [, args] = h.invoke.mock.calls[0];
    expect((args as { params: SyncBoundsPayload }).params.anchorX).toBe(420);
    expect((args as { params: SyncBoundsPayload }).params.anchorWidth).toBe(700);
  });

  it("forwards a document-relative scroll offset", async () => {
    const h = mockController(rect());
    await h.controller.report(-40, -250);
    const [, args] = h.invoke.mock.calls[0];
    const p = (args as { params: SyncBoundsPayload }).params;
    expect(p.scrollX).toBe(-40);
    expect(p.scrollY).toBe(-250);
  });
});

describe("BrowserPaneController visibility", () => {
  it("is visible only when tab active + on-screen + not occluded", () => {
    const h = mockController(rect());
    expect(h.controller.isVisible).toBe(true);
  });

  it("hides the overlay when the tab goes inactive", async () => {
    const h = mockController(rect());
    await h.controller.setTabActive(false);
    expect(h.controller.isVisible).toBe(false);
    const [, args] = h.invoke.mock.calls[0];
    expect((args as { params: SyncBoundsPayload }).params.visible).toBe(false);
  });

  it("hides when scrolled off-screen (IntersectionObserver false)", async () => {
    const h = mockController(rect());
    await h.controller.setOnScreen(false);
    expect(h.controller.isVisible).toBe(false);
    const [, args] = h.invoke.mock.calls[0];
    expect((args as { params: SyncBoundsPayload }).params.visible).toBe(false);
  });

  it("reports occlusion when app UI covers the anchor (breakage mode #2)", async () => {
    const h = mockController(rect());
    await h.controller.setOccluded(true);
    expect(h.controller.isVisible).toBe(false);
    const [, args] = h.invoke.mock.calls[0];
    expect((args as { params: SyncBoundsPayload }).params.visible).toBe(false);

    // Palette closes → occlusion clears → overlay shows again.
    h.invoke.mockClear();
    await h.controller.setOccluded(false);
    expect(h.controller.isVisible).toBe(true);
    const [, args2] = h.invoke.mock.calls[0];
    expect((args2 as { params: SyncBoundsPayload }).params.visible).toBe(true);
  });

  it("stays hidden while any one condition is false", async () => {
    const h = mockController(rect());
    await h.controller.setTabActive(true);
    await h.controller.setOnScreen(false); // off-screen
    await h.controller.setOccluded(false);
    expect(h.controller.isVisible).toBe(false);
  });
});

describe("BrowserPaneController commands", () => {
  it("navigate invokes browser_navigate with the pane id + url", async () => {
    const h = mockController(rect());
    await h.controller.navigate("https://example.com");
    expect(h.invoke).toHaveBeenCalledWith("browser_navigate", {
      paneId: "pane-1",
      url: "https://example.com",
    });
  });

  it("close invokes browser_close", async () => {
    const h = mockController(rect());
    await h.controller.close();
    expect(h.invoke).toHaveBeenCalledWith("browser_close", { paneId: "pane-1" });
  });
});
