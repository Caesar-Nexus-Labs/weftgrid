// KeybindingSource — the seam between P4 focus-nav and P12's keybinding registry.
//
// [red-team H5] P4 must NOT hardcode key literals in navigation logic, and it
// must NOT own the keybinding registry (that's P12). So focus-nav reads its
// chords from this interface. P4 ships a `DefaultKeybindingSource` returning the
// spec default (`Ctrl+Alt+arrow`); P12 will later supply a registry-backed impl
// (e.g. wrapping `keybinding_resolve`) without touching focus-nav.
//
// Actions are namespaced strings so the registry can key on them; chord syntax
// is the lowercase "+"-joined form ("ctrl+alt+arrowleft") matching what a
// KeyboardEvent yields after normalization (see `eventToChord`).

/** Directional focus actions P4 needs bound. */
export type FocusAction =
  | "pane.focus.left"
  | "pane.focus.right"
  | "pane.focus.up"
  | "pane.focus.down";

export const FOCUS_ACTIONS: readonly FocusAction[] = [
  "pane.focus.left",
  "pane.focus.right",
  "pane.focus.up",
  "pane.focus.down",
];

/**
 * Resolves an action id to its chord, or `null` when unbound. P12 swaps the
 * default impl for a registry-backed one; focus-nav only sees this interface.
 */
export interface KeybindingSource {
  resolve(action: string): string | null;
}

/** Spec default focus-nav bindings (`Ctrl+Alt+arrow`). */
const DEFAULT_FOCUS_BINDINGS: Record<FocusAction, string> = {
  "pane.focus.left": "ctrl+alt+arrowleft",
  "pane.focus.right": "ctrl+alt+arrowright",
  "pane.focus.up": "ctrl+alt+arrowup",
  "pane.focus.down": "ctrl+alt+arrowdown",
};

/** P4's standalone default until P12's registry is wired in. */
export class DefaultKeybindingSource implements KeybindingSource {
  resolve(action: string): string | null {
    return (DEFAULT_FOCUS_BINDINGS as Record<string, string>)[action] ?? null;
  }
}

/** Modifier+key shape we normalize a DOM KeyboardEvent down to. */
export interface ChordEvent {
  key: string;
  ctrlKey: boolean;
  metaKey: boolean;
  altKey: boolean;
  shiftKey: boolean;
}

/**
 * Normalize a keyboard event into the canonical chord string used by bindings.
 * Modifier order is fixed (ctrl, meta, alt, shift) so comparison is exact.
 */
export function eventToChord(e: ChordEvent): string {
  const parts: string[] = [];
  if (e.ctrlKey) parts.push("ctrl");
  if (e.metaKey) parts.push("meta");
  if (e.altKey) parts.push("alt");
  if (e.shiftKey) parts.push("shift");
  parts.push(e.key.toLowerCase());
  return parts.join("+");
}
