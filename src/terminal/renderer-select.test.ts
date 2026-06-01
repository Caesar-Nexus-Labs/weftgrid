import { describe, it, expect } from "vitest";
import {
  RendererSelector,
  type RendererAddon,
  type RendererTerminal,
} from "./renderer-select";

/** Fake terminal recording loaded addons. */
function fakeTerm(): RendererTerminal & { loaded: number } {
  return {
    loaded: 0,
    loadAddon() {
      this.loaded += 1;
    },
  };
}

/** Fake WebGL addon that captures its context-loss handler so tests can fire it. */
function fakeWebglAddon(): RendererAddon & { fireContextLoss: () => void; disposed: boolean } {
  let lossHandler: (() => void) | null = null;
  return {
    disposed: false,
    onContextLoss(handler: () => void) {
      lossHandler = handler;
      return { dispose() {} };
    },
    dispose() {
      this.disposed = true;
    },
    fireContextLoss() {
      lossHandler?.();
    },
  };
}

describe("RendererSelector", () => {
  it("selects webgl when the addon loads successfully", () => {
    const term = fakeTerm();
    const sel = new RendererSelector(term, () => fakeWebglAddon());
    expect(sel.init()).toBe("webgl");
    expect(sel.renderer).toBe("webgl");
    expect(term.loaded).toBe(1);
  });

  it("falls back to dom when the webgl factory throws (init failure)", () => {
    const term = fakeTerm();
    const sel = new RendererSelector(term, () => {
      throw new Error("no WebGL2");
    });
    expect(sel.init()).toBe("dom");
    expect(sel.renderer).toBe("dom");
  });

  it("falls back to dom when loadAddon throws", () => {
    const term: RendererTerminal = {
      loadAddon() {
        throw new Error("context creation failed");
      },
    };
    const sel = new RendererSelector(term, () => fakeWebglAddon());
    expect(sel.init()).toBe("dom");
  });

  it("swaps webgl → dom at runtime on context loss, disposing the addon", () => {
    const term = fakeTerm();
    const addon = fakeWebglAddon();
    const changes: string[] = [];
    const sel = new RendererSelector(term, () => addon, (k) => changes.push(k));

    expect(sel.init()).toBe("webgl");
    addon.fireContextLoss();

    expect(sel.renderer).toBe("dom");
    expect(addon.disposed).toBe(true);
    expect(changes).toEqual(["webgl", "dom"]);
  });
});
