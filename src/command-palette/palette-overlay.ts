// Palette overlay logic (P16) — framework-agnostic controller for the single
// command-palette overlay. Owns scope detection, the result list + selection,
// keyboard navigation, and run/dismiss. The DOM (input, list rendering) is the
// UI track's job; this exposes state + handlers so it is testable in jsdom.
//
// Scope is chosen by prefix (cmux ContentView parity):
//   - query starting with `>`  → COMMANDS scope (the `>` is stripped before match)
//   - any other query (incl. empty) → SWITCHER scope (workspace/surface jump)
// Cmd+Shift+P opens pre-seeded with `>`; Cmd+P opens empty (switcher).
//
// Nav: Ctrl+N / ArrowDown = next, Ctrl+P / ArrowUp = previous (wrapping). Return
// runs the selected entry; Esc dismisses and the host restores prior focus.

/** Which command source the current query targets. */
export type PaletteScope = "commands" | "switcher";

/** A row ready to render: id, display text, and the matched char offsets. */
export interface ResultRow {
  id: string;
  text: string;
  /** Char offsets into `text` to highlight (from the Rust matcher). */
  indices: number[];
  /** Dimmed + non-runnable (failed its `enablement`). */
  disabled: boolean;
}

/** Immutable snapshot the UI renders from. */
export interface OverlayState {
  open: boolean;
  /** Full input text including any `>` prefix. */
  query: string;
  scope: PaletteScope;
  results: ResultRow[];
  /** Index into `results` of the highlighted row (-1 when empty). */
  selectedIndex: number;
}

const COMMANDS_PREFIX = ">";

/** Detect scope + strip the `>` so the matcher never sees the prefix. */
export function detectScope(query: string): { scope: PaletteScope; term: string } {
  if (query.startsWith(COMMANDS_PREFIX)) {
    return { scope: "commands", term: query.slice(COMMANDS_PREFIX.length).trimStart() };
  }
  return { scope: "switcher", term: query };
}

const CLOSED: OverlayState = {
  open: false,
  query: "",
  scope: "switcher",
  results: [],
  selectedIndex: -1,
};

/** A normalized keyboard event (host adapts the DOM event to this shape). */
export interface OverlayKey {
  key: string;
  ctrlKey: boolean;
  metaKey: boolean;
  shiftKey: boolean;
}

/** Host callbacks the overlay drives. `restoreFocus` returns focus on dismiss. */
export interface OverlayHandlers {
  /** Run the chosen entry. Return false to keep the overlay open (e.g. the
   * custom-command confirm was cancelled). Default-treated as run → dismiss. */
  run(id: string, scope: PaletteScope): boolean | Promise<boolean>;
  restoreFocus(): void;
}

/**
 * Stateful overlay controller. The host feeds it query changes + key events and
 * supplies fresh results (already ranked by `PaletteSearch`). Subscribe for
 * re-render. Pure logic — no DOM, no timers.
 */
export class PaletteOverlay {
  private state: OverlayState = { ...CLOSED };
  private readonly listeners = new Set<(s: OverlayState) => void>();

  getState(): OverlayState {
    return this.state;
  }

  subscribe(listener: (s: OverlayState) => void): () => void {
    this.listeners.add(listener);
    listener(this.state);
    return () => this.listeners.delete(listener);
  }

  /** Open in commands scope (Cmd+Shift+P) — input seeded with `>`. */
  openCommands(): void {
    this.state = { ...CLOSED, open: true, query: COMMANDS_PREFIX, scope: "commands" };
    this.emit();
  }

  /** Open in switcher scope (Cmd+P) — empty input. */
  openSwitcher(): void {
    this.state = { ...CLOSED, open: true, query: "", scope: "switcher" };
    this.emit();
  }

  /** Update the raw query (re-detects scope; results arrive via `setResults`). */
  setQuery(query: string): void {
    const { scope } = detectScope(query);
    this.state = { ...this.state, query, scope };
    this.emit();
  }

  /** The scope-stripped term to feed the matcher for the current query. */
  searchTerm(): string {
    return detectScope(this.state.query).term;
  }

  /** Install freshly-ranked results, keeping selection on the first row. */
  setResults(results: ResultRow[]): void {
    this.state = {
      ...this.state,
      results,
      selectedIndex: results.length > 0 ? 0 : -1,
    };
    this.emit();
  }

  /** Move selection (wraps). `delta` is +1 (next) or -1 (previous). */
  move(delta: number): void {
    const n = this.state.results.length;
    if (n === 0) {
      return;
    }
    const next = (this.state.selectedIndex + delta + n) % n;
    this.state = { ...this.state, selectedIndex: next };
    this.emit();
  }

  /** The currently-highlighted row, if any. */
  selected(): ResultRow | undefined {
    return this.state.results[this.state.selectedIndex];
  }

  /** Run the selected row (skips disabled rows). Dismisses unless the handler
   * returns false (e.g. a cancelled confirm). */
  async run(handlers: OverlayHandlers): Promise<void> {
    const row = this.selected();
    if (!row || row.disabled) {
      return;
    }
    const ran = await handlers.run(row.id, this.state.scope);
    if (ran !== false) {
      this.dismiss(handlers);
    }
  }

  /** Close the overlay and restore the host's prior focus. */
  dismiss(handlers: OverlayHandlers): void {
    this.state = { ...CLOSED };
    this.emit();
    handlers.restoreFocus();
  }

  /**
   * Route a key event. Returns true when handled (host should preventDefault).
   * Ctrl+N/ArrowDown next, Ctrl+P/ArrowUp prev, Enter runs, Escape dismisses.
   */
  async handleKey(e: OverlayKey, handlers: OverlayHandlers): Promise<boolean> {
    if (!this.state.open) {
      return false;
    }
    const key = e.key.toLowerCase();

    if (key === "escape") {
      this.dismiss(handlers);
      return true;
    }
    if (key === "enter") {
      await this.run(handlers);
      return true;
    }
    if (key === "arrowdown" || (e.ctrlKey && key === "n")) {
      this.move(1);
      return true;
    }
    if (key === "arrowup" || (e.ctrlKey && key === "p")) {
      this.move(-1);
      return true;
    }
    return false;
  }

  private emit(): void {
    for (const listener of this.listeners) {
      listener(this.state);
    }
  }
}
