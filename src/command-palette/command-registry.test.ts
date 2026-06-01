// Command-registry merge tests (P16).
//
// Verifies: built-in catalog construction + shared-dispatch run; merge of
// weft.json custom commands; `palette:false` hides a built-in; `actions[]`
// override of a built-in's title/keywords/shortcut; feature-gated `when`
// entries are excluded from the corpus until their context flag is on.

import { describe, it, expect, vi } from "vitest";
import {
  createBuiltinCatalog,
  mergeRegistry,
  buildCorpus,
  customCommandId,
  defaultContext,
  type WeftContribution,
} from "./command-registry";
import type { WeftCommand } from "../settings/settings-store";

function shellCmd(name: string, keywords: string[] = []): WeftCommand {
  return { name, keywords, command: `run ${name}` };
}

describe("createBuiltinCatalog", () => {
  it("built-in run() delegates to the shared dispatch with the action id", async () => {
    const dispatch = vi.fn();
    const catalog = createBuiltinCatalog(dispatch, { "split.right": "ctrl+shift+e" });
    const split = catalog.find((c) => c.id === "split.right")!;
    expect(split.shortcutHint).toBe("ctrl+shift+e");
    const ctx = defaultContext();
    await split.run(ctx);
    expect(dispatch).toHaveBeenCalledWith("split.right", ctx);
  });

  it("gates browser/ssh entries behind a `when` predicate", () => {
    const catalog = createBuiltinCatalog(vi.fn());
    const browser = catalog.find((c) => c.id === "browser.newTab")!;
    expect(browser.when!(defaultContext())).toBe(false);
    expect(browser.when!(defaultContext({ hasBrowser: true }))).toBe(true);
  });
});

describe("mergeRegistry", () => {
  it("merges weft.json custom commands as entries with the Rust-aligned id", () => {
    const builtins = createBuiltinCatalog(vi.fn());
    const contribution: WeftContribution = { commands: [shellCmd("Deploy", ["ship"])] };
    const merged = mergeRegistry(builtins, contribution);
    const deploy = merged.find((c) => c.id === customCommandId("Deploy"));
    expect(deploy).toBeDefined();
    expect(deploy!.title).toBe("Deploy");
    expect(deploy!.keywords).toEqual(["ship"]);
  });

  it("palette:false hides a targeted built-in", () => {
    const builtins = createBuiltinCatalog(vi.fn());
    const contribution: WeftContribution = {
      commands: [],
      actions: [{ id: "split.down", palette: false }],
    };
    const merged = mergeRegistry(builtins, contribution);
    expect(merged.find((c) => c.id === "split.down")).toBeUndefined();
    // Other built-ins survive.
    expect(merged.find((c) => c.id === "split.right")).toBeDefined();
  });

  it("overrides a built-in's title/keywords/shortcut without replacing its run", async () => {
    const dispatch = vi.fn();
    const builtins = createBuiltinCatalog(dispatch);
    const contribution: WeftContribution = {
      commands: [],
      actions: [
        {
          id: "find.open",
          title: "Search Buffer",
          keywords: ["lookup"],
          shortcut: "ctrl+k",
        },
      ],
    };
    const merged = mergeRegistry(builtins, contribution);
    const find = merged.find((c) => c.id === "find.open")!;
    expect(find.title).toBe("Search Buffer");
    expect(find.keywords).toEqual(["lookup"]);
    expect(find.shortcutHint).toBe("ctrl+k");
    // run still delegates to the shared dispatch (override is presentation-only).
    await find.run(defaultContext());
    expect(dispatch).toHaveBeenCalledWith("find.open", expect.anything());
  });

  it("custom run() is invoked for a custom command entry", async () => {
    const customRun = vi.fn();
    const builtins = createBuiltinCatalog(vi.fn());
    const cmd = shellCmd("Build");
    const merged = mergeRegistry(builtins, { commands: [cmd] }, customRun);
    const entry = merged.find((c) => c.id === customCommandId("Build"))!;
    const ctx = defaultContext();
    await entry.run(ctx);
    expect(customRun).toHaveBeenCalledWith(cmd, ctx);
  });
});

describe("buildCorpus", () => {
  it("excludes `when`-failing entries and marks `enablement`-failing ones disabled", () => {
    const builtins = createBuiltinCatalog(vi.fn());
    const ctx = defaultContext({ hasActiveTerminal: false }); // find.open disabled, browser hidden
    const { candidates, disabled } = buildCorpus(builtins, ctx);
    const ids = candidates.map((c) => c.id);
    expect(ids).not.toContain("browser.newTab"); // hidden by `when`
    expect(ids).toContain("find.open"); // visible...
    expect(disabled.has("find.open")).toBe(true); // ...but disabled (no terminal)
  });

  it("includes a gated entry once its context flag is on", () => {
    const builtins = createBuiltinCatalog(vi.fn());
    const { candidates } = buildCorpus(builtins, defaultContext({ hasSsh: true }));
    expect(candidates.map((c) => c.id)).toContain("ssh.connect");
  });

  it("folds subtitle + keywords into the matcher keyword field", () => {
    const builtins = createBuiltinCatalog(vi.fn());
    const { candidates } = buildCorpus(builtins, defaultContext());
    const split = candidates.find((c) => c.id === "split.right")!;
    expect(split.text).toBe("Split Right");
    expect(split.keywords).toContain("vertical");
  });
});
