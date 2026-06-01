// Inject runtime entry (P7 owner). Authored in TS, bundled to a self-contained
// IIFE JS string and embedded by Rust via include_str! — the ONE user-approved
// exception to "no hand-written JS" (this runs in page-context, like any website
// script, not as app logic).
//
// THE single DOM-walk automation backend. Identical script runs on BOTH WebView2
// (Windows) and WebKitGTK (Linux) so ephemeral refs `eN` and snapshot text are
// byte-identical cross-platform → agent scripts are portable.
//
// Algorithm ported from cmux (GPL-3.0), itself ported from
// vercel-labs/agent-browser (Apache-2.0). Modified for weftgrid, 2026-05-31.
//
// SPDX-License-Identifier: GPL-3.0-or-later
// Portions Copyright (c) Vercel, Inc. (agent-browser, Apache-2.0).
// Portions Copyright (c) the cmux authors (GPL-3.0).

import { buildSnapshot, makeRefTable, resetRefs, type RefTable, type SnapshotOptions } from "./dom-walk";
import {
  clickElement, countSelector, evalWaitCondition, fillElement, findBySelector,
  getFromElement, isChecked, isEnabled, isVisible,
  type ActionResult, type GetKind, type WaitKind,
} from "./dom-actions";

// Build/version marker. KEEP the literal "weftgrid-inject-stub" substring: the
// Rust embed test (inject_asset.rs) asserts the bundle contains it as a cheap
// "bundle is really embedded" probe. The rest of this file is the real runtime.
const WEFT_INJECT_MARKER = "weftgrid-inject-stub";
const WEFT_INJECT_VERSION = "p7-domwalk-1";

/** Command envelope sent from Rust via evaluate_script(`window.__weft.dispatch(...)`). */
interface WeftCommand {
  id: number;
  action: string;
  ref?: string;
  selector?: string;
  text?: string;
  attr?: string;
  kind?: string;
  value?: string;
  options?: SnapshotOptions;
  wait?: WaitKind;
}

/** IPC bridge Rust installs (`window.__weft.postMessage`). Stubbed for tests. */
interface WeftBridge {
  postMessage?: (payload: string) => void;
}

(() => {
  const w = window as unknown as {
    __weft?: WeftBridge & {
      version?: string;
      marker?: string;
      refs?: RefTable;
      dispatch?: (cmd: WeftCommand) => string;
    };
  };

  const bridge: WeftBridge = w.__weft || {};
  const refs: RefTable = makeRefTable();

  /** Resolve a `eN` ref OR a raw selector to a live element. */
  const resolve = (cmd: WeftCommand): Element | null => {
    const raw = cmd.ref ?? cmd.selector ?? "";
    const token = raw.startsWith("@") ? raw.slice(1) : raw;
    const mapped = refs.map[token];
    const selector = mapped ?? raw;
    if (!selector) return null;
    try {
      return document.querySelector(selector);
    } catch {
      return null;
    }
  };

  /** Run one command synchronously, returning a JSON-serializable result. */
  const run = (cmd: WeftCommand): unknown => {
    switch (cmd.action) {
      case "snapshot": {
        resetRefs(refs); // re-snapshot-before-act invariant: refs are fresh
        return buildSnapshot({ doc: document, win: window }, refs, cmd.options || {});
      }
      case "click":
        return clickElement(resolve(cmd));
      case "fill":
        return fillElement(resolve(cmd), cmd.text ?? cmd.value ?? "");
      case "get":
        return getFromElement(resolve(cmd), (cmd.kind as GetKind) || "text", window, cmd.attr);
      case "count":
        return countSelector(document, cmd.selector ?? "");
      case "isVisible":
        return isVisible(resolve(cmd), window);
      case "isEnabled":
        return isEnabled(resolve(cmd));
      case "isChecked":
        return isChecked(resolve(cmd));
      case "find": {
        const res = findBySelector(document, cmd.selector ?? "");
        if (res.ok && res.selector) {
          refs.counter += 1;
          const ref = `e${refs.counter}`;
          refs.map[ref] = res.selector;
          return { ...res, ref };
        }
        return res;
      }
      case "wait":
        return cmd.wait
          ? { ok: true, value: evalWaitCondition({ doc: document, win: window }, cmd.wait) }
          : ({ ok: false, error: "invalid_params" } as ActionResult);
      default:
        return { ok: false, error: "unknown_action" } as ActionResult;
    }
  };

  /**
   * Dispatch a command and round-trip the result to Rust. Linux's evaluate_script
   * is historically fire-and-forget, so we POST the JSON over the IPC bridge
   * keyed by command id; the synchronous return is the same JSON (Windows path).
   */
  const dispatch = (cmd: WeftCommand): string => {
    let payload: string;
    try {
      payload = JSON.stringify({ id: cmd.id, ok: true, result: run(cmd) });
    } catch (err) {
      payload = JSON.stringify({
        id: cmd.id,
        ok: false,
        error: String((err as Error)?.message || err),
      });
    }
    try {
      bridge.postMessage?.(payload);
    } catch {
      // Bridge not installed (e.g. unit test) — synchronous return still works.
    }
    return payload;
  };

  w.__weft = Object.assign(bridge, {
    version: WEFT_INJECT_VERSION,
    marker: WEFT_INJECT_MARKER,
    refs,
    dispatch,
  });
})();
