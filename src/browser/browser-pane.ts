// Browser pane — anchor leaf + nav bar + bounds/scroll/visibility/occlusion
// reporting (P6).
//
// A browser pane is an OS overlay window (owned by the Rust backend) painted on
// top of a placeholder "anchor" element living in the main webview's split-tree.
// This module owns the MAIN-webview side: it watches the anchor's geometry
// (ResizeObserver + scroll listener), its on-screen visibility (Intersection
// Observer), whether app UI occludes it, and reports all of that to the backend
// via `browser_sync_bounds` so the overlay follows the anchor. It also renders the
// nav bar (URL input + back/forward/reload) which drives `browser_navigate`.
//
// The geometry math lives in Rust (one tested implementation). This side only
// gathers inputs: the anchor rect (CSS px via getBoundingClientRect), page scroll,
// and the main window frame metrics (physical px + scale, read from the Tauri
// window API). `getRect`, the frame source, and `invoke` are all injected so the
// reporting logic unit-tests under jsdom without a live Tauri runtime.

import type { InvokeFn } from "../terminal/xterm-wrapper";
import type { PaneId } from "$lib/model";

/** A rect as returned by `getBoundingClientRect` (the subset we use). */
export interface AnchorRect {
  x: number;
  y: number;
  width: number;
  height: number;
}

/** Main-window frame metrics (PHYSICAL px) read from the Tauri window API. */
export interface WindowFrame {
  /** Outer top-left in physical screen px (`outerPosition`). */
  outerX: number;
  outerY: number;
  /** Client-area inset = innerPosition - outerPosition, physical px. */
  insetX: number;
  insetY: number;
  /** Scale factor of the monitor hosting the main window. */
  scale: number;
}

/** Wire payload for `browser_sync_bounds` (mirrors Rust `SyncBoundsParams`). */
export interface SyncBoundsPayload {
  paneId: PaneId;
  mainOuterX: number;
  mainOuterY: number;
  clientInsetX: number;
  clientInsetY: number;
  anchorX: number;
  anchorY: number;
  anchorWidth: number;
  anchorHeight: number;
  scrollX: number;
  scrollY: number;
  mainScale: number;
  visible: boolean;
}

/** Inputs that fully determine a sync payload (pure, testable). */
export interface SyncInputs {
  paneId: PaneId;
  rect: AnchorRect;
  scrollX: number;
  scrollY: number;
  frame: WindowFrame;
  /** False when the pane's tab is inactive, scrolled out, or occluded. */
  visible: boolean;
}

/**
 * Assemble the `browser_sync_bounds` payload from gathered inputs. Pure: no DOM,
 * no IPC — the single place the wire shape is defined, so tests assert it
 * directly. Standard `getBoundingClientRect` is already viewport-relative, so
 * scroll is reported as 0 here; it exists for callers that report document-
 * relative rects (the Rust math folds scroll into the anchor offset).
 */
export function buildSyncPayload(inputs: SyncInputs): SyncBoundsPayload {
  return {
    paneId: inputs.paneId,
    mainOuterX: inputs.frame.outerX,
    mainOuterY: inputs.frame.outerY,
    clientInsetX: inputs.frame.insetX,
    clientInsetY: inputs.frame.insetY,
    anchorX: inputs.rect.x,
    anchorY: inputs.rect.y,
    anchorWidth: inputs.rect.width,
    anchorHeight: inputs.rect.height,
    scrollX: inputs.scrollX,
    scrollY: inputs.scrollY,
    mainScale: inputs.frame.scale,
    visible: inputs.visible,
  };
}

/** Reads the current anchor rect (injected → mockable in jsdom). */
export type RectSource = () => AnchorRect;
/** Reads the live main-window frame metrics (injected → mockable). */
export type FrameSource = () => WindowFrame;

export interface BrowserPaneDeps {
  paneId: PaneId;
  invoke: InvokeFn;
  /** How to read the anchor rect (defaults to `el.getBoundingClientRect()`). */
  getRect: RectSource;
  /** How to read main-window frame metrics. */
  getFrame: FrameSource;
}

/**
 * Drives one browser pane's main-webview side. Owns the visibility flags (tab
 * active / on-screen / not occluded) and pushes a fresh sync payload whenever any
 * input changes. Observer wiring (`observe`) is separate so the controller is
 * testable headlessly: tests call `report()` / the flag setters directly.
 */
export class BrowserPaneController {
  private tabActive = true;
  private onScreen = true;
  private occluded = false;

  constructor(private readonly deps: BrowserPaneDeps) {}

  /** True only when the overlay should be shown (active + on-screen + clear). */
  get isVisible(): boolean {
    return this.tabActive && this.onScreen && !this.occluded;
  }

  /** Gather inputs and report the current bounds + visibility to the backend. */
  report(scrollX = 0, scrollY = 0): Promise<void> {
    const payload = buildSyncPayload({
      paneId: this.deps.paneId,
      rect: this.deps.getRect(),
      scrollX,
      scrollY,
      frame: this.deps.getFrame(),
      visible: this.isVisible,
    });
    return this.deps.invoke<void>("browser_sync_bounds", { params: payload });
  }

  /** Tab (in-pane surface) became active/inactive — hide the overlay when off. */
  setTabActive(active: boolean): Promise<void> {
    this.tabActive = active;
    return this.report();
  }

  /** IntersectionObserver result: anchor scrolled in/out of the viewport. */
  setOnScreen(onScreen: boolean): Promise<void> {
    this.onScreen = onScreen;
    return this.report();
  }

  /** App UI (command palette / dropdown / modal) covers the anchor — clip-hide. */
  setOccluded(occluded: boolean): Promise<void> {
    this.occluded = occluded;
    return this.report();
  }

  /** Navigate the overlay to `url` (nav bar URL input / link). */
  navigate(url: string): Promise<void> {
    return this.deps.invoke<void>("browser_navigate", {
      paneId: this.deps.paneId,
      url,
    });
  }

  /** Destroy the overlay (pane closed). */
  close(): Promise<void> {
    return this.deps.invoke<void>("browser_close", { paneId: this.deps.paneId });
  }
}

/** Subset of `ResizeObserver`/`IntersectionObserver` ctors we depend on. */
export interface ObserverFactories {
  resize: (cb: () => void) => { observe(el: Element): void; disconnect(): void };
  intersection: (
    cb: (onScreen: boolean) => void,
  ) => { observe(el: Element): void; disconnect(): void };
}

/**
 * Wire DOM observers on `anchor` so layout changes / scroll / visibility push a
 * sync automatically. Returns a teardown. Kept out of the controller so the
 * controller stays DOM-free and unit-testable; this thin function is exercised at
 * integration (it needs real observers).
 */
export function observeAnchor(
  anchor: Element,
  controller: BrowserPaneController,
  scrollContainer: { addEventListener: typeof window.addEventListener; removeEventListener: typeof window.removeEventListener },
  factories: ObserverFactories,
): () => void {
  const onScroll = () => void controller.report(0, 0);
  const ro = factories.resize(() => void controller.report());
  const io = factories.intersection((onScreen) => void controller.setOnScreen(onScreen));
  ro.observe(anchor);
  io.observe(anchor);
  scrollContainer.addEventListener("scroll", onScroll, { passive: true } as AddEventListenerOptions);
  return () => {
    ro.disconnect();
    io.disconnect();
    scrollContainer.removeEventListener("scroll", onScroll);
  };
}
