// Inject DOM action bodies (P7 owner). Pure functions that operate on a resolved
// element: click, fill (React-compatible native setter), get html/value/styles/
// box, visibility/state checks, find-by-selector, and a wait predicate.
//
// Ported from cmux (GPL-3.0) / vercel-labs/agent-browser (Apache-2.0).
// Modified for weftgrid, 2026-05-31.

import { cssPath, isElementVisible, normalize } from "./dom-walk";

/** Discriminated result for action bodies (mirrors cmux `{ok, error, value}`). */
export interface ActionResult {
  ok: boolean;
  error?: string;
  value?: unknown;
}

export type GetKind =
  | "text" | "html" | "value" | "attr" | "box" | "styles" | "count";

/**
 * Set an input's value via the native prototype setter so React/Vue/Angular's
 * value override doesn't swallow the change. Walks the prototype chain (works
 * cross-realm / web components). Caller fires the input/change events after.
 */
export function reactCompatibleSetValue(el: Element, newValue: string): void {
  let nativeSetter: ((v: string) => void) | null = null;
  for (let proto = Object.getPrototypeOf(el); proto; proto = Object.getPrototypeOf(proto)) {
    const desc = Object.getOwnPropertyDescriptor(proto, "value");
    if (desc && desc.set) {
      nativeSetter = desc.set as (v: string) => void;
      break;
    }
  }
  if (nativeSetter) {
    nativeSetter.call(el, newValue);
  } else {
    (el as HTMLInputElement).value = newValue;
  }
}

export function clickElement(el: Element | null): ActionResult {
  if (!el) return { ok: false, error: "not_found" };
  (el as HTMLElement).scrollIntoView?.({ block: "nearest", inline: "nearest" });
  const htmlEl = el as HTMLElement;
  if (typeof htmlEl.click === "function") {
    htmlEl.click();
  } else {
    el.dispatchEvent(new MouseEvent("click", {
      bubbles: true, cancelable: true, view: window, detail: 1,
    }));
  }
  return { ok: true };
}

/** Replace an element's value (empty string clears). Fires input + change. */
export function fillElement(el: Element | null, text: string): ActionResult {
  if (!el) return { ok: false, error: "not_found" };
  (el as HTMLElement).focus?.();
  const newValue = String(text);
  if ("value" in el) {
    reactCompatibleSetValue(el, newValue);
    el.dispatchEvent(new Event("input", { bubbles: true }));
    el.dispatchEvent(new Event("change", { bubbles: true }));
  } else {
    (el as HTMLElement).textContent = newValue;
  }
  return { ok: true };
}

export function getFromElement(
  el: Element | null,
  kind: GetKind,
  win: Window,
  attr?: string,
): ActionResult {
  if (!el) return { ok: false, error: "not_found" };
  const htmlEl = el as HTMLElement;
  switch (kind) {
    case "text":
      return { ok: true, value: String(htmlEl.innerText || el.textContent || "").trim() };
    case "html":
      return { ok: true, value: String(el.outerHTML || "") };
    case "value":
      return { ok: true, value: String((el as HTMLInputElement).value ?? "") };
    case "attr":
      return { ok: true, value: attr ? el.getAttribute(attr) : null };
    case "box": {
      const r = el.getBoundingClientRect();
      return {
        ok: true,
        value: {
          x: r.x, y: r.y, width: r.width, height: r.height,
          top: r.top, left: r.left, right: r.right, bottom: r.bottom,
        },
      };
    }
    case "styles": {
      const style = win.getComputedStyle(el);
      if (attr) return { ok: true, value: style.getPropertyValue(attr) };
      return {
        ok: true,
        value: {
          display: style.display,
          visibility: style.visibility,
          opacity: style.opacity,
          color: style.color,
          background: style.background,
          width: style.width,
          height: style.height,
        },
      };
    }
    default:
      return { ok: false, error: "invalid_params" };
  }
}

export function isVisible(el: Element | null, win: Window): ActionResult {
  if (!el) return { ok: false, error: "not_found" };
  return { ok: true, value: isElementVisible(el, win) };
}

export function isEnabled(el: Element | null): ActionResult {
  if (!el) return { ok: false, error: "not_found" };
  return { ok: true, value: !(el as HTMLInputElement).disabled };
}

export function isChecked(el: Element | null): ActionResult {
  if (!el) return { ok: false, error: "not_found" };
  const checked = "checked" in el ? !!(el as HTMLInputElement).checked : false;
  return { ok: true, value: checked };
}

/** Count matches for a raw selector (does not allocate refs). */
export function countSelector(doc: Document, selector: string): ActionResult {
  try {
    return { ok: true, value: doc.querySelectorAll(selector).length };
  } catch {
    return { ok: false, error: "invalid_selector" };
  }
}

export interface FindResult extends ActionResult {
  selector?: string;
  tag?: string;
  text?: string;
}

/**
 * Resolve a raw CSS selector to its first match and return a fully-qualified
 * `:nth-of-type` path (maxParts = Infinity) so the resulting ref is unambiguous.
 */
export function findBySelector(doc: Document, selector: string): FindResult {
  let el: Element | null;
  try {
    el = doc.querySelector(selector);
  } catch {
    return { ok: false, error: "invalid_selector" };
  }
  if (!el) return { ok: false, error: "not_found" };
  const path = cssPath(el, Number.POSITIVE_INFINITY);
  if (!path) return { ok: false, error: "not_found" };
  return {
    ok: true,
    selector: path,
    tag: String(el.tagName || "").toLowerCase(),
    text: normalize(el.textContent || ""),
  };
}

/** Evaluate a wait predicate against the current page state. */
export type WaitKind =
  | { kind: "selector"; selector: string }
  | { kind: "urlContains"; value: string }
  | { kind: "textContains"; value: string }
  | { kind: "loadState"; value: string }
  | { kind: "function"; expr: string };

export function evalWaitCondition(env: { doc: Document; win: Window }, cond: WaitKind): boolean {
  const { doc, win } = env;
  switch (cond.kind) {
    case "selector":
      try {
        return !!doc.querySelector(cond.selector);
      } catch {
        return false;
      }
    case "urlContains":
      return String(win.location?.href || "").includes(cond.value);
    case "textContains":
      return !!doc.body && String((doc.body as HTMLElement).innerText || doc.body.textContent || "").includes(cond.value);
    case "loadState": {
      const state = String(doc.readyState || "").toLowerCase();
      if (cond.value.toLowerCase() === "interactive") {
        return state === "interactive" || state === "complete";
      }
      return state === cond.value.toLowerCase();
    }
    case "function":
      try {
        // eslint-disable-next-line no-new-func
        return !!new Function(`return (${cond.expr});`)();
      } catch {
        return false;
      }
  }
}
