// @vitest-environment jsdom
import { describe, it, expect, vi } from "vitest";
import { PaneRenderer, paneKey } from "./pane-renderer";
import { pane, split, splitLeaf, resizeDivider } from "./split-tree";

function container(): HTMLElement {
  return document.createElement("div");
}

describe("PaneRenderer DOM reuse (critical: do not recreate xterm hosts)", () => {
  it("reuses the same host element for a pane across re-layouts", () => {
    const root = container();
    const renderer = new PaneRenderer(root);

    renderer.render(pane(["a"]));
    const hostBefore = renderer.getHost("a");
    expect(hostBefore).toBeDefined();
    // Tag the live host so we can prove it is the SAME node afterwards.
    (hostBefore as HTMLElement).dataset.scrollback = "precious";

    // Split: pane "a" stays, new pane "b" appears.
    renderer.render(splitLeaf(pane(["a"]), [], "horizontal", pane(["b"])));
    const hostAfter = renderer.getHost("a");
    expect(hostAfter).toBe(hostBefore); // identity preserved
    expect((hostAfter as HTMLElement).dataset.scrollback).toBe("precious");
    expect(renderer.getHost("b")).toBeDefined();
  });

  it("calls onMount once per new pane and onReuse on re-layout", () => {
    const onMount = vi.fn();
    const onReuse = vi.fn();
    const renderer = new PaneRenderer(container(), { onMount, onReuse });

    renderer.render(pane(["a"]));
    expect(onMount).toHaveBeenCalledTimes(1);
    expect(onMount.mock.calls[0][1]).toBe("a");

    renderer.render(splitLeaf(pane(["a"]), [], "vertical", pane(["b"])));
    // "a" reused, "b" mounted fresh.
    expect(onMount).toHaveBeenCalledTimes(2);
    expect(onReuse).toHaveBeenCalledWith(expect.any(HTMLElement), "a", expect.anything());
  });

  it("evicts the host for a pane that no longer exists", () => {
    const renderer = new PaneRenderer(container());
    renderer.render(split("horizontal", pane(["a"]), pane(["b"])));
    expect(renderer.getHost("b")).toBeDefined();

    // Close "b" → tree collapses to pane "a".
    renderer.render(pane(["a"]));
    expect(renderer.getHost("a")).toBeDefined();
    expect(renderer.getHost("b")).toBeUndefined();
  });

  it("encodes orientation as flex-direction and divider as flex-grow", () => {
    const root = container();
    const renderer = new PaneRenderer(root);
    renderer.render(split("horizontal", pane(["a"]), pane(["b"]), 0.3));

    const box = root.querySelector(".pane-split") as HTMLElement;
    expect(box.style.flexDirection).toBe("row");
    const cells = root.querySelectorAll(".pane-cell");
    expect((cells[0] as HTMLElement).style.flexGrow).toBe("0.3");
    expect((cells[1] as HTMLElement).style.flexGrow).toBe("0.7");

    renderer.render(resizeDivider(split("horizontal", pane(["a"]), pane(["b"]), 0.3), [], 0.6));
    const cellsAfter = root.querySelectorAll(".pane-cell");
    expect((cellsAfter[0] as HTMLElement).style.flexGrow).toBe("0.6");
  });

  it("vertical split uses column flex-direction", () => {
    const root = container();
    new PaneRenderer(root).render(split("vertical", pane(["a"]), pane(["b"])));
    expect((root.querySelector(".pane-split") as HTMLElement).style.flexDirection).toBe("column");
  });

  it("marks a browser-anchor leaf as a measurable rect for P6", () => {
    const root = container();
    const renderer = new PaneRenderer(root);
    renderer.render(pane(["browser-1"]));
    expect((renderer.getHost("browser-1") as HTMLElement).dataset.browserAnchor).toBe("true");
    renderer.render(pane(["browser-1"])); // re-render keeps it true
    expect((renderer.getHost("browser-1") as HTMLElement).dataset.browserAnchor).toBe("true");
  });

  it("paneKey derives a stable key from the first surface id", () => {
    expect(paneKey(pane(["a", "b"]))).toBe("a");
  });
});

describe("PaneRenderer notification ring (P5b additive)", () => {
  it("setPaneRing adds an a11y ring marker to a live host", () => {
    const renderer = new PaneRenderer(container());
    renderer.render(pane(["a"]));

    expect(renderer.setPaneRing("a", true)).toBe(true);
    const host = renderer.getHost("a") as HTMLElement;
    expect(host.classList.contains("pane-host--ring")).toBe(true);
    expect(host.dataset.hasNotification).toBe("true");
    // Not color-only: a textual description is exposed for assistive tech.
    expect(host.getAttribute("aria-description")).toBe("Unread notification");
  });

  it("setPaneRing(false) removes the ring marker", () => {
    const renderer = new PaneRenderer(container());
    renderer.render(pane(["a"]));
    renderer.setPaneRing("a", true);

    renderer.setPaneRing("a", false);
    const host = renderer.getHost("a") as HTMLElement;
    expect(host.classList.contains("pane-host--ring")).toBe(false);
    expect(host.dataset.hasNotification).toBeUndefined();
    expect(host.hasAttribute("aria-description")).toBe(false);
  });

  it("ring survives a re-layout (host reuse re-applies it)", () => {
    const renderer = new PaneRenderer(container());
    renderer.render(pane(["a"]));
    renderer.setPaneRing("a", true);

    // Split: pane "a" host is reused; its ring must persist.
    renderer.render(splitLeaf(pane(["a"]), [], "horizontal", pane(["b"])));
    const host = renderer.getHost("a") as HTMLElement;
    expect(host.classList.contains("pane-host--ring")).toBe(true);
    expect(renderer.getHost("b")?.classList.contains("pane-host--ring")).toBe(false);
  });

  it("setPaneRing on an unmounted key returns false but is applied on render", () => {
    const renderer = new PaneRenderer(container());
    // No host yet for "a".
    expect(renderer.setPaneRing("a", true)).toBe(false);

    renderer.render(pane(["a"]));
    expect((renderer.getHost("a") as HTMLElement).dataset.hasNotification).toBe("true");
  });
});

