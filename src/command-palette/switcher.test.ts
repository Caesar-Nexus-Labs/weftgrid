// Switcher tests (P16) — workspace/surface entries → P12 workspace_select.

import { describe, it, expect, vi } from "vitest";
import {
  buildSwitcherEntries,
  switcherCorpus,
  Switcher,
  type SwitcherWorkspace,
  type SwitcherSurface,
} from "./switcher";

const workspaces: SwitcherWorkspace[] = [
  { id: "ws-1", title: "weftgrid", currentDirectory: "/repos/weftgrid" },
  { id: "ws-2", title: "notes", currentDirectory: "/notes" },
];

const surfaces: SwitcherSurface[] = [
  { workspaceId: "ws-1", surfaceId: "s-1", title: "shell" },
];

describe("buildSwitcherEntries", () => {
  it("maps workspaces to entries (cwd as subtitle), surfaces excluded by default", () => {
    const entries = buildSwitcherEntries(workspaces, surfaces);
    expect(entries).toHaveLength(2);
    expect(entries[0]).toMatchObject({ id: "switcher.ws.ws-1", title: "weftgrid", workspaceId: "ws-1" });
    expect(entries[0].subtitle).toBe("/repos/weftgrid");
  });

  it("includes surface entries when allSurfaces is on (owning workspace as subtitle)", () => {
    const entries = buildSwitcherEntries(workspaces, surfaces, true);
    const surface = entries.find((e) => e.id === "switcher.surface.s-1")!;
    expect(surface.workspaceId).toBe("ws-1");
    expect(surface.surfaceId).toBe("s-1");
    expect(surface.subtitle).toBe("weftgrid");
  });
});

describe("switcherCorpus", () => {
  it("projects entries to matcher candidates (title + cwd keywords, stable rank)", () => {
    const corpus = switcherCorpus(buildSwitcherEntries(workspaces));
    expect(corpus[0]).toEqual({
      id: "switcher.ws.ws-1",
      text: "weftgrid",
      keywords: "/repos/weftgrid",
      rank: 0,
    });
  });
});

describe("Switcher.select", () => {
  it("selecting a workspace entry calls workspace_select with its id", async () => {
    const select = vi.fn(async () => true);
    const s = new Switcher(select);
    s.setEntries(buildSwitcherEntries(workspaces));

    const ok = await s.select("switcher.ws.ws-2");
    expect(select).toHaveBeenCalledWith("ws-2");
    expect(ok).toBe(true);
  });

  it("selecting a surface entry selects its owning workspace", async () => {
    const select = vi.fn(async () => true);
    const s = new Switcher(select);
    s.setEntries(buildSwitcherEntries(workspaces, surfaces, true));

    await s.select("switcher.surface.s-1");
    expect(select).toHaveBeenCalledWith("ws-1");
  });

  it("returns false for an unknown entry id without calling select", async () => {
    const select = vi.fn(async () => true);
    const s = new Switcher(select);
    s.setEntries(buildSwitcherEntries(workspaces));

    const ok = await s.select("switcher.ws.nope");
    expect(ok).toBe(false);
    expect(select).not.toHaveBeenCalled();
  });
});
