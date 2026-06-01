// Renderer selection — WebGL with runtime DOM fallback (P3 red-team M4).
//
// xterm 6's built-in renderer is the DOM renderer. WebGL is an opt-in addon
// (`@xterm/addon-webgl`) that is much faster but can fail at INIT (no GPU / no
// WebGL2) and — critically — at RUNTIME (driver reset, GPU eviction) which fires
// `WebglAddon.onContextLoss`. Init-time try/catch alone is insufficient: we must
// subscribe to onContextLoss, dispose the addon, and fall back to the DOM
// renderer live. `@xterm/addon-canvas` is NOT used: at xterm 6 it is incompatible
// (its peer dep is xterm ^5) and effectively superseded by the DOM renderer.
//
// This module is dependency-injected (addon factory + a minimal terminal port)
// so it unit-tests without a real WebGL context or DOM.

/** Active renderer after selection. `'dom'` is the always-available fallback. */
export type RendererKind = "webgl" | "dom";

/** Minimal surface of an xterm addon we need (load + dispose). */
export interface RendererAddon {
  /** Subscribe to GPU context loss; returns an unsubscribe disposable. */
  onContextLoss(handler: () => void): { dispose(): void };
  dispose(): void;
}

/** Minimal terminal surface: loading an addon may throw if WebGL is unavailable. */
export interface RendererTerminal {
  loadAddon(addon: RendererAddon): void;
}

/** Factory for a fresh WebGL addon. Throws (or the load throws) when unsupported. */
export type WebglAddonFactory = () => RendererAddon;

/** Notified whenever the active renderer changes (init result or runtime swap). */
export type RendererChangeHandler = (kind: RendererKind) => void;

/**
 * Chooses and manages the terminal renderer.
 *
 * - `init()` tries WebGL; on any failure it reports `'dom'` (built-in, no addon
 *   needed — the terminal already renders via DOM).
 * - If WebGL loads, it watches `onContextLoss` and swaps to DOM at runtime.
 */
export class RendererSelector {
  private current: RendererKind = "dom";
  private webgl: RendererAddon | null = null;

  constructor(
    private readonly term: RendererTerminal,
    private readonly makeWebgl: WebglAddonFactory,
    private readonly onChange: RendererChangeHandler = () => {},
  ) {}

  /** Attempt WebGL; fall back to DOM on any error. Returns the active kind. */
  init(): RendererKind {
    try {
      const addon = this.makeWebgl();
      this.term.loadAddon(addon);
      addon.onContextLoss(() => this.handleContextLoss());
      this.webgl = addon;
      this.setRenderer("webgl");
    } catch {
      this.disposeWebgl();
      this.setRenderer("dom");
    }
    return this.current;
  }

  /** Runtime GPU loss: drop WebGL, render via the built-in DOM renderer. */
  private handleContextLoss(): void {
    this.disposeWebgl();
    this.setRenderer("dom");
  }

  private disposeWebgl(): void {
    if (this.webgl) {
      try {
        this.webgl.dispose();
      } catch {
        // Disposing a lost-context addon may throw; the swap to DOM is what
        // matters, so swallow and continue.
      }
      this.webgl = null;
    }
  }

  private setRenderer(kind: RendererKind): void {
    if (this.current !== kind || kind === "webgl") {
      this.current = kind;
      this.onChange(kind);
    }
  }

  get renderer(): RendererKind {
    return this.current;
  }
}
