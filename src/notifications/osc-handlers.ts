// Notification OSC handlers (P5a core) — bind OSC 9 / 99 / 777 on a P3 xterm.
//
// cmux delegates OSC parsing to Ghostty's native VT parser; weftgrid is a hard
// fork (cmux SHA c4911439e3e99784bd5d6379096f315034a5259c) on xterm.js + Rust.
// xterm exposes `parser.registerOscHandler(code, cb)`, which fires once with the
// already-de-framed payload after `ESC ] <code> ;`. We forward that payload to
// the Rust core (`notify_ingest_osc`), which owns the single parse + per-pane
// notification manager (so parsing logic is not duplicated in JS).
//
// Each handler returns `true`: the sequence is consumed by us so no
// earlier-registered handler double-handles it. Normal terminal text still
// renders — OSC handlers are a passive tap, not a text gate, so no output is
// swallowed (phase non-functional requirement).
//
// `invoke` is injected so this unit-tests without a live Tauri runtime, matching
// the xterm-wrapper / find-bar pattern in the terminal track.

import type { InvokeFn } from "../terminal/xterm-wrapper";

/** OSC codes weftgrid treats as notifications (iTerm2 / kitty / rxvt). */
export const NOTIFICATION_OSC_CODES = [9, 99, 777] as const;

/** Minimal xterm parser surface we depend on (the de-framed-payload callback). */
export interface OscParser {
  registerOscHandler(
    code: number,
    callback: (data: string) => boolean | Promise<boolean>,
  ): { dispose(): void };
}

/** Minimal xterm surface: just the `parser`. */
export interface OscCapableTerminal {
  readonly parser: OscParser;
}

/**
 * Register OSC 9 / 99 / 777 handlers on `term` for `paneId`. Each completed
 * sequence is forwarded to the Rust core via `invoke('notify_ingest_osc', ...)`;
 * the backend parses + records it and emits `notification-changed` (the event
 * P5b subscribes to for the pane ring + sidebar highlight).
 *
 * Returns a disposer that removes all three handlers (call on pane teardown).
 */
export function registerNotificationOscHandlers(
  term: OscCapableTerminal,
  paneId: string,
  invoke: InvokeFn,
): () => void {
  const disposables = NOTIFICATION_OSC_CODES.map((code) =>
    term.parser.registerOscHandler(code, (data) => {
      // Fire-and-forget: the backend records + emits. Errors must not reject the
      // parser callback (that would surface as an unhandled rejection), so the
      // promise is swallowed. The handler still reports "handled" synchronously.
      void invoke("notify_ingest_osc", { paneId, code, data }).catch(() => {});
      return true;
    }),
  );
  return () => {
    for (const d of disposables) {
      d.dispose();
    }
  };
}
