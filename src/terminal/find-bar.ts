// Terminal Find — `@xterm/addon-search` controller (P3, cmux SearchState parity).
//
// cmux delegates find to Ghostty native; weftgrid uses `@xterm/addon-search`,
// which searches the in-memory xterm buffer in the webview (pure TS, no Rust).
// cmux's `SearchState` is just {needle, selected, total} — we mirror that via the
// addon's `onDidChangeResults({resultIndex, resultCount})`.
//
// Responsibilities:
//   - needle → findNext / findPrevious with `decorations` (highlight all + active)
//   - `onDidChangeResults` → match counter `selected/total` (selected = index+1)
//   - "use selection for find" (Cmd/Ctrl+E): term.getSelection() → needle
//   - keybindings: default Ctrl+F open, Ctrl+G next, Ctrl+Shift+G prev, Ctrl+E
//     use-selection. P12's keybinding registry is the eventual source of truth;
//     until then these defaults apply (HANDOFF: P12 should override via setKeymap).
//
// The DOM overlay (input field + counter + buttons) is rendered by the pane/UI
// track; this controller owns search behavior + counter state and exposes a
// subscribe() so the overlay re-renders on result changes.

/** Match-counter state — cmux SearchState parity. */
export interface FindState {
  needle: string;
  /** 1-based index of the active match (0 when none). */
  selected: number;
  /** Total matches for the current needle. */
  total: number;
  open: boolean;
}

/** Subset of `ISearchOptions` we expose; all optional toggles default off. */
export interface FindOptions {
  caseSensitive?: boolean;
  wholeWord?: boolean;
  regex?: boolean;
}

/** Result payload from addon-search `onDidChangeResults`. */
export interface SearchResultChange {
  resultIndex: number;
  resultCount: number;
}

/** Minimal `SearchAddon` surface (decorations on by default for highlight-all). */
export interface SearchAddonPort {
  findNext(term: string, options?: Record<string, unknown>): boolean;
  findPrevious(term: string, options?: Record<string, unknown>): boolean;
  clearDecorations(): void;
  onDidChangeResults(handler: (e: SearchResultChange) => void): { dispose(): void };
}

/** Minimal terminal surface needed for find (selection → needle). */
export interface FindTerminal {
  getSelection(): string;
}

/** Highlight colors for decorations; supplied by the theme/UI track. */
export interface FindDecorationColors {
  matchBackground?: string;
  activeMatchBackground?: string;
  matchOverviewRuler?: string;
  activeMatchColorOverviewRuler?: string;
}

const EMPTY_STATE: FindState = { needle: "", selected: 0, total: 0, open: false };

/**
 * Drives `@xterm/addon-search` and maintains the match counter. UI-agnostic: the
 * overlay subscribes for re-render; keyboard handling is exposed via
 * `handleKey` so the host can route events from whatever element has focus.
 */
export class FindController {
  private state: FindState = { ...EMPTY_STATE };
  private options: FindOptions = {};
  private readonly listeners = new Set<(s: FindState) => void>();
  private readonly resultsSub: { dispose(): void };

  constructor(
    private readonly addon: SearchAddonPort,
    private readonly term: FindTerminal,
    private readonly decorations: FindDecorationColors = {},
  ) {
    // Counter is driven by the addon, which reports index/count after each find.
    this.resultsSub = this.addon.onDidChangeResults((e) => {
      this.state = {
        ...this.state,
        // resultIndex is 0-based and -1 when there is no active match.
        selected: e.resultIndex >= 0 ? e.resultIndex + 1 : 0,
        total: e.resultCount,
      };
      this.emit();
    });
  }

  subscribe(listener: (s: FindState) => void): () => void {
    this.listeners.add(listener);
    listener(this.state);
    return () => this.listeners.delete(listener);
  }

  getState(): FindState {
    return this.state;
  }

  open(): void {
    this.state = { ...this.state, open: true };
    this.emit();
  }

  close(): void {
    this.addon.clearDecorations();
    this.state = { ...EMPTY_STATE };
    this.emit();
  }

  setOptions(options: FindOptions): void {
    this.options = { ...this.options, ...options };
    if (this.state.needle) {
      this.findNext();
    }
  }

  /** Set the needle and incrementally search forward (called on each keystroke). */
  setNeedle(needle: string): void {
    this.state = { ...this.state, needle };
    this.emit();
    if (needle) {
      this.findNext(true);
    } else {
      this.addon.clearDecorations();
      this.state = { ...this.state, selected: 0, total: 0 };
      this.emit();
    }
  }

  findNext(incremental = false): boolean {
    return this.addon.findNext(this.state.needle, this.searchOptions(incremental));
  }

  findPrevious(): boolean {
    return this.addon.findPrevious(this.state.needle, this.searchOptions(false));
  }

  /** Cmd/Ctrl+E: seed the needle from the current terminal selection. */
  useSelection(): void {
    const sel = this.term.getSelection();
    if (sel) {
      this.open();
      this.setNeedle(sel);
    }
  }

  /**
   * Route a keyboard event. Returns true if handled (caller should
   * preventDefault). Defaults match the spec; P12 will inject a keymap later.
   */
  handleKey(e: { key: string; ctrlKey: boolean; metaKey: boolean; shiftKey: boolean }): boolean {
    const mod = e.ctrlKey || e.metaKey;
    if (!mod) {
      return false;
    }
    const key = e.key.toLowerCase();
    if (key === "f") {
      this.open();
      return true;
    }
    if (key === "e") {
      this.useSelection();
      return true;
    }
    if (key === "g") {
      if (e.shiftKey) {
        this.findPrevious();
      } else {
        this.findNext();
      }
      return true;
    }
    return false;
  }

  dispose(): void {
    this.resultsSub.dispose();
    this.listeners.clear();
  }

  private searchOptions(incremental: boolean): Record<string, unknown> {
    return {
      incremental,
      caseSensitive: this.options.caseSensitive ?? false,
      wholeWord: this.options.wholeWord ?? false,
      regex: this.options.regex ?? false,
      decorations: {
        matchBackground: this.decorations.matchBackground,
        activeMatchBackground: this.decorations.activeMatchBackground,
        matchOverviewRuler: this.decorations.matchOverviewRuler,
        activeMatchColorOverviewRuler: this.decorations.activeMatchColorOverviewRuler,
      },
    };
  }

  private emit(): void {
    for (const listener of this.listeners) {
      listener(this.state);
    }
  }
}
