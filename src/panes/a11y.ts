// A11y helpers — ARIA roles + keyboard-nav primitives for the pane UI.
//
// [red-team L2] The project a11y rule targets the APP's own UI. P4 is the main
// UI surface, so the pane tree must be navigable by keyboard and expose correct
// roles. This module centralizes the role vocabulary + attribute application so
// pane-renderer/tabs/divider-drag stay consistent (one source of truth, DRY).
//
// MVP acceptance (locked): keyboard nav + ARIA roles. Focus-visual + screen
// reader pronunciation = manual checks, not asserted in CI. jsdom is enough to
// assert roles/attributes/tabindex here.

/** ARIA roles used across the pane UI. `group` is the generic split container. */
export const PaneAriaRole = {
  /** A terminal/browser pane leaf container. */
  pane: "group",
  /** A split container holding two children + a divider. */
  split: "group",
  /** The drag handle between two split children. */
  divider: "separator",
  /** The in-pane tab strip. */
  tablist: "tablist",
  /** A single surface tab. */
  tab: "tab",
  /** The content region a tab controls (one surface). */
  surface: "tabpanel",
} as const;

export type PaneAriaRoleName = (typeof PaneAriaRole)[keyof typeof PaneAriaRole];

/** Apply a role + optional ARIA attributes/data to an element (idempotent). */
export function applyAria(
  el: HTMLElement,
  role: PaneAriaRoleName,
  attrs: Record<string, string | number | boolean | undefined> = {},
): void {
  el.setAttribute("role", role);
  for (const [name, value] of Object.entries(attrs)) {
    if (value === undefined) {
      el.removeAttribute(name);
    } else {
      el.setAttribute(name, String(value));
    }
  }
}

/**
 * Mark a divider as an ARIA separator with orientation + value range. A
 * horizontal SPLIT lays children side-by-side, so its divider is oriented
 * vertically (the bar is vertical) — ARIA orientation refers to the separator
 * bar, hence the inversion.
 */
export function applyDividerAria(
  el: HTMLElement,
  splitOrientation: "horizontal" | "vertical",
  dividerPosition: number,
): void {
  const barOrientation = splitOrientation === "horizontal" ? "vertical" : "horizontal";
  applyAria(el, PaneAriaRole.divider, {
    "aria-orientation": barOrientation,
    "aria-valuemin": 10,
    "aria-valuemax": 90,
    "aria-valuenow": Math.round(dividerPosition * 100),
    tabindex: 0,
  });
}

/** Roving-tabindex: exactly one element in a group is tabbable (tabindex 0). */
export function setRovingTabindex(elements: HTMLElement[], activeIndex: number): void {
  elements.forEach((el, i) => {
    el.tabIndex = i === activeIndex ? 0 : -1;
  });
}

/** Mark a tab selected/unselected (aria-selected + roving tabindex). */
export function setTabSelected(tab: HTMLElement, selected: boolean): void {
  tab.setAttribute("aria-selected", String(selected));
  tab.tabIndex = selected ? 0 : -1;
}

/** Move focus to the element if it is focusable; safe no-op outside the DOM. */
export function focusElement(el: HTMLElement | null | undefined): void {
  if (el && typeof el.focus === "function") {
    el.focus();
  }
}

/**
 * Compute the next index for arrow-key tab navigation, wrapping at the ends.
 * `delta` is +1 (next) or -1 (previous). Empty list returns -1.
 */
export function rovingNextIndex(current: number, length: number, delta: number): number {
  if (length === 0) {
    return -1;
  }
  return (current + delta + length) % length;
}
