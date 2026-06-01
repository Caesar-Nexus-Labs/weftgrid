// Automation client surface (P7) — typed consume side for the agent RPC / browser
// pane (P6/P10b/P13). Thin `invoke` wrappers around the Rust automation commands.
//
// Types mirror the serde DTOs in `src-tauri/src/automation/mod.rs` + `commands.rs`
// (camelCase wire) — keep them in sync. No DOM logic lives here: the single
// inject-JS DOM-walk (inject/snapshot.ts) is the one algorithm, embedded by Rust
// and driven over `evaluate_script`. This module just types the command bridge.
//
// `invoke` is injected so this unit-tests without a live Tauri runtime.

import type { InvokeFn } from "../terminal/xterm-wrapper";

/** Per-ref metadata (mirror of Rust `RefInfo`). */
export interface RefInfo {
  role: string;
  name?: string;
}

/** One AX-tree node (mirror of Rust `SnapshotEntry`). */
export interface SnapshotEntry {
  selector: string;
  role: string;
  name: string;
  depth: number;
}

/** Full snapshot payload (mirror of Rust `Snapshot`, camelCase wire). */
export interface Snapshot {
  title: string;
  url: string;
  readyState: string;
  text: string;
  html: string;
  /** Deterministic AX-tree text with inline `[ref=eN]` tokens. */
  snapshotText: string;
  /** `eN` → {role, name}. */
  refs: Record<string, RefInfo>;
  entries: SnapshotEntry[];
}

/** What `get(ref, kind)` reads (mirror of Rust `GetKind`, snake_case wire). */
export type GetKind =
  | "text" | "html" | "value" | "attr" | "box" | "styles" | "count";

/** A `wait` predicate (mirror of Rust `WaitCond`, `{kind, value}` wire). */
export type WaitCond =
  | { kind: "selector"; value: string }
  | { kind: "urlContains"; value: string }
  | { kind: "textContains"; value: string }
  | { kind: "loadState"; value: string }
  | { kind: "function"; value: string };

/** Tauri command names registered for this track (see commands.rs doc header). */
export const AUTOMATION_COMMANDS = {
  snapshot: "browser_snapshot",
  click: "browser_click",
  fill: "browser_fill",
  eval: "browser_eval",
  wait: "browser_wait",
  get: "browser_get",
  find: "browser_find",
} as const;

/**
 * Typed wrapper over the automation commands for one browser surface.
 *
 * Re-snapshot-before-act invariant: refs `eN` are reset on every `snapshot()`,
 * so callers must snapshot, then act on the fresh refs in the same turn.
 */
export class AutomationClient {
  constructor(
    private readonly invoke: InvokeFn,
    private readonly surfaceId: string,
  ) {}

  snapshot(): Promise<Snapshot> {
    return this.invoke(AUTOMATION_COMMANDS.snapshot, { surfaceId: this.surfaceId });
  }

  click(ref: string): Promise<void> {
    return this.invoke(AUTOMATION_COMMANDS.click, { surfaceId: this.surfaceId, ref });
  }

  /** Fill a field (empty `text` clears it). */
  fill(ref: string, text: string): Promise<void> {
    return this.invoke(AUTOMATION_COMMANDS.fill, { surfaceId: this.surfaceId, ref, text });
  }

  eval(js: string): Promise<unknown> {
    return this.invoke(AUTOMATION_COMMANDS.eval, { surfaceId: this.surfaceId, js });
  }

  wait(cond: WaitCond): Promise<boolean> {
    return this.invoke(AUTOMATION_COMMANDS.wait, { surfaceId: this.surfaceId, cond });
  }

  get(ref: string, kind: GetKind, attr?: string): Promise<unknown> {
    return this.invoke(AUTOMATION_COMMANDS.get, {
      surfaceId: this.surfaceId, ref, kind, attr: attr ?? null,
    });
  }

  /** Resolve a raw CSS selector to a fresh ref `eN`. */
  find(selector: string): Promise<string> {
    return this.invoke(AUTOMATION_COMMANDS.find, { surfaceId: this.surfaceId, selector });
  }
}
