// App-command-registry (P16) — the single catalog of commands, one entry per
// action, surfaced through many entrypoints (palette / keybinding / menu).
//
// An entry is pure metadata + a `run` thunk. Built-in entries delegate `run` to
// a shared `ActionDispatch` so the palette, a keybinding, and a menu item all
// invoke the SAME handler (cmux shared-behavior policy — no duplicated logic).
// Custom commands come from a project/global `weft.json` (parsed by P12); their
// `run` is the trust-gated custom runner.
//
// `when` gates VISIBILITY (does the feature exist on this build/context — e.g.
// browser/SSH actions hide until those panels exist). `enablement` gates whether
// a visible entry can run right now (e.g. "find next" needs an open find bar).

import type { WeftCommand } from "../settings/settings-store";
import type { PaletteCandidate } from "./palette-search";

/** Plain bool/string snapshot of app state read by `when` / `enablement`. */
export interface CommandContext {
  hasBrowser: boolean;
  hasSsh: boolean;
  hasActiveTerminal: boolean;
  hasSelection: boolean;
  findOpen: boolean;
  canCloseSurface: boolean;
}

/** A neutral context (everything off) — tests/hosts override the fields they set. */
export function defaultContext(overrides: Partial<CommandContext> = {}): CommandContext {
  return {
    hasBrowser: false,
    hasSsh: false,
    hasActiveTerminal: false,
    hasSelection: false,
    findOpen: false,
    canCloseSurface: false,
    ...overrides,
  };
}

/** Shared handler table: action-id → side effect. Reused by keybinding + menu. */
export type ActionDispatch = (actionId: string, ctx: CommandContext) => void | Promise<void>;

/** One palette/keybinding/menu command. */
export interface CommandEntry {
  id: string;
  title: string;
  subtitle?: string;
  keywords: string[];
  /** Display-only chord hint (resolved from P12 keybinding registry). */
  shortcutHint?: string;
  /** Visibility gate — the feature exists in this context. Absent = always shown. */
  when?: (ctx: CommandContext) => boolean;
  /** Run gate — the command can fire now. Absent = always enabled. */
  enablement?: (ctx: CommandContext) => boolean;
  /** Close the overlay after a successful run (default true). */
  dismissOnRun: boolean;
  run: (ctx: CommandContext) => void | Promise<void>;
}

/** weft.json palette `actions[]` overlay (presentation/override/hide layer).
 * P12 parses `commands`; the `actions` array — which can hide or re-skin a
 * built-in, or bind a custom command into the palette — is a TS-layer concern. */
export interface PaletteAction {
  /** Target/own action id (a built-in id to override/hide, or a new id). */
  id?: string;
  /** Name of a `commands[]` entry this action runs (type "command"). */
  command?: string;
  type?: "builtin" | "command" | "agent" | "workspaceCommand";
  title?: string;
  keywords?: string[];
  shortcut?: string;
  /** `false` hides the targeted entry from the palette. */
  palette?: boolean;
}

/** What a host hands the registry: P12-parsed commands + the actions overlay. */
export interface WeftContribution {
  commands: WeftCommand[];
  actions?: PaletteAction[];
}

/** Slug a weft.json command name into its palette id. MUST match the Rust
 * `WeftCommand::id()` (`percent_alnum`) so palette ids + trust keys line up. */
export function customCommandId(name: string): string {
  const bytes = new TextEncoder().encode(name);
  let out = "weft.config.command.";
  for (const b of bytes) {
    const alnum = (b >= 48 && b <= 57) || (b >= 65 && b <= 90) || (b >= 97 && b <= 122);
    out += alnum
      ? String.fromCharCode(b)
      : "%" + b.toString(16).toUpperCase().padStart(2, "0");
  }
  return out;
}

/** Built-in catalog: ONLY actions mapping to features weftgrid actually ships.
 * Browser/SSH entries are gated behind `when` so they hide until those panels
 * exist. `shortcuts` maps action-id → chord (from P12 `keybinding_resolve`). */
export function createBuiltinCatalog(
  dispatch: ActionDispatch,
  shortcuts: Record<string, string> = {},
): CommandEntry[] {
  const make = (
    id: string,
    title: string,
    keywords: string[],
    extra: Partial<CommandEntry> = {},
  ): CommandEntry => ({
    id,
    title,
    keywords,
    shortcutHint: shortcuts[id],
    dismissOnRun: true,
    run: (ctx) => dispatch(id, ctx),
    ...extra,
  });

  return [
    make("palette.switcher", "Go to Workspace", ["switch", "jump", "workspace"]),
    make("surface.newTerminal", "New Terminal", ["tab", "shell", "open"]),
    make("surface.close", "Close Surface", ["tab", "close", "kill"], {
      enablement: (ctx) => ctx.canCloseSurface,
    }),
    make("split.right", "Split Right", ["pane", "vertical", "split"]),
    make("split.down", "Split Down", ["pane", "horizontal", "split"]),
    make("focus.left", "Focus Pane Left", ["pane", "navigate"]),
    make("focus.right", "Focus Pane Right", ["pane", "navigate"]),
    make("focus.up", "Focus Pane Up", ["pane", "navigate"]),
    make("focus.down", "Focus Pane Down", ["pane", "navigate"]),
    make("find.open", "Find in Terminal", ["search", "grep"], {
      enablement: (ctx) => ctx.hasActiveTerminal,
    }),
    make("find.next", "Find Next", ["search", "next"], {
      enablement: (ctx) => ctx.findOpen,
    }),
    make("find.previous", "Find Previous", ["search", "previous"], {
      enablement: (ctx) => ctx.findOpen,
    }),
    make("app.openSettings", "Open Settings", ["preferences", "config"]),
    // Feature-gated: hidden until the panel/transport exists in this context.
    make("browser.newTab", "New Browser Tab", ["web", "url", "browser"], {
      when: (ctx) => ctx.hasBrowser,
    }),
    make("ssh.connect", "Connect via SSH", ["remote", "ssh", "host"], {
      when: (ctx) => ctx.hasSsh,
    }),
  ];
}

/** Merge built-ins with a weft.json contribution: apply `actions` overrides/hides
 * to built-ins, then append visible custom commands. Later wins on title/keywords/
 * shortcut; `palette:false` removes the targeted entry. */
export function mergeRegistry(
  builtins: CommandEntry[],
  contribution: WeftContribution,
  customRun: (cmd: WeftCommand, ctx: CommandContext) => void | Promise<void> = () => {},
): CommandEntry[] {
  const byId = new Map<string, CommandEntry>();
  for (const entry of builtins) {
    byId.set(entry.id, entry);
  }

  // Custom commands → entries (id matches Rust slug so trust keys align).
  const customByName = new Map<string, WeftCommand>();
  for (const cmd of contribution.commands) {
    customByName.set(cmd.name, cmd);
    const id = customCommandId(cmd.name);
    byId.set(id, {
      id,
      title: cmd.name,
      subtitle: cmd.description,
      keywords: cmd.keywords ?? [],
      dismissOnRun: true,
      run: (ctx) => customRun(cmd, ctx),
    });
  }

  // Apply the actions overlay: hide / override built-ins, bind custom commands.
  for (const action of contribution.actions ?? []) {
    const targetId =
      action.id ??
      (action.command ? customCommandId(action.command) : undefined);
    if (!targetId) {
      continue;
    }
    if (action.palette === false) {
      byId.delete(targetId);
      continue;
    }
    const existing = byId.get(targetId);
    if (existing) {
      byId.set(targetId, {
        ...existing,
        title: action.title ?? existing.title,
        keywords: action.keywords ?? existing.keywords,
        shortcutHint: action.shortcut ?? existing.shortcutHint,
      });
    } else if (action.command && customByName.has(action.command)) {
      const cmd = customByName.get(action.command)!;
      byId.set(targetId, {
        id: targetId,
        title: action.title ?? cmd.name,
        keywords: action.keywords ?? cmd.keywords ?? [],
        shortcutHint: action.shortcut,
        dismissOnRun: true,
        run: (ctx) => customRun(cmd, ctx),
      });
    }
  }

  return [...byId.values()];
}

/** Project visible entries (passing `when`) to the nucleo corpus, recording
 * which entries are disabled (failing `enablement`) for the UI to dim. */
export function buildCorpus(
  entries: CommandEntry[],
  ctx: CommandContext,
): { candidates: PaletteCandidate[]; byId: Map<string, CommandEntry>; disabled: Set<string> } {
  const candidates: PaletteCandidate[] = [];
  const byId = new Map<string, CommandEntry>();
  const disabled = new Set<string>();
  entries.forEach((entry, index) => {
    if (entry.when && !entry.when(ctx)) {
      return;
    }
    byId.set(entry.id, entry);
    if (entry.enablement && !entry.enablement(ctx)) {
      disabled.add(entry.id);
    }
    candidates.push({
      id: entry.id,
      text: entry.title,
      keywords: [entry.subtitle ?? "", ...entry.keywords].join(" ").trim(),
      rank: index,
    });
  });
  return { candidates, byId, disabled };
}
