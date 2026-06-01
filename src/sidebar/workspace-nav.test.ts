import { describe, it, expect, vi, beforeEach } from "vitest";
import { WorkspaceNav, eventToChord, type ChordEvent } from "./workspace-nav";
import type { WorkspaceSnapshot } from "$lib/model";

function snap(id: string, over: Partial<WorkspaceSnapshot> = {}): WorkspaceSnapshot {
  return {
    id,
    title: id,
    isPinned: false,
    currentDirectory: `/work/${id}`,
    unreadCount: 0,
    listeningPorts: [],
    pullRequestRows: [],
    ...over,
  };
}

describe("WorkspaceNav command forwarding (mock invoke)", () => {
  const invoke = vi.fn();
  let nav: WorkspaceNav;

  beforeEach(() => {
    invoke.mockReset();
    nav = new WorkspaceNav(invoke);
  });

  it("select forwards workspace_select with the id", async () => {
    invoke.mockResolvedValueOnce(true);
    await nav.select("ws-1");
    expect(invoke).toHaveBeenCalledWith("workspace_select", { id: "ws-1" });
  });

  it("add forwards workspace_add with title + cwd", async () => {
    invoke.mockResolvedValueOnce("new-id");
    const id = await nav.add("proj", "/proj");
    expect(invoke).toHaveBeenCalledWith("workspace_add", { title: "proj", cwd: "/proj" });
    expect(id).toBe("new-id");
  });

  it("remove forwards workspace_remove with the id", async () => {
    invoke.mockResolvedValueOnce(true);
    await nav.remove("ws-9");
    expect(invoke).toHaveBeenCalledWith("workspace_remove", { id: "ws-9" });
  });

  it("reorder forwards workspace_reorder with from/to indices", async () => {
    invoke.mockResolvedValueOnce(true);
    await nav.reorder(0, 2);
    expect(invoke).toHaveBeenCalledWith("workspace_reorder", { from: 0, to: 2 });
  });

  it("reorderById resolves ids to indices then calls workspace_reorder", async () => {
    invoke
      .mockResolvedValueOnce([snap("a"), snap("b"), snap("c")]) // workspace_snapshot
      .mockResolvedValueOnce(true); // workspace_reorder
    const ok = await nav.reorderById("a", "c");
    expect(ok).toBe(true);
    expect(invoke).toHaveBeenNthCalledWith(1, "workspace_snapshot");
    expect(invoke).toHaveBeenNthCalledWith(2, "workspace_reorder", { from: 0, to: 2 });
  });

  it("reorderById is a no-op when an id is unknown", async () => {
    invoke.mockResolvedValueOnce([snap("a"), snap("b")]);
    const ok = await nav.reorderById("a", "missing");
    expect(ok).toBe(false);
    // Only the snapshot read happened; no reorder command issued.
    expect(invoke).toHaveBeenCalledTimes(1);
  });
});

describe("WorkspaceNav next/prev/jump-unread (snapshot-driven)", () => {
  const invoke = vi.fn();
  let nav: WorkspaceNav;

  beforeEach(() => {
    invoke.mockReset();
    nav = new WorkspaceNav(invoke);
  });

  it("step(+1) selects the next workspace after fromId", async () => {
    invoke
      .mockResolvedValueOnce([snap("a"), snap("b"), snap("c")])
      .mockResolvedValueOnce(true);
    await nav.step(1, "a");
    expect(invoke).toHaveBeenNthCalledWith(2, "workspace_select", { id: "b" });
  });

  it("step(-1) wraps to the last workspace from the first", async () => {
    invoke
      .mockResolvedValueOnce([snap("a"), snap("b"), snap("c")])
      .mockResolvedValueOnce(true);
    await nav.step(-1, "a");
    expect(invoke).toHaveBeenNthCalledWith(2, "workspace_select", { id: "c" });
  });

  it("jumpUnread selects the next workspace with unread", async () => {
    invoke
      .mockResolvedValueOnce([snap("a"), snap("b", { unreadCount: 0 }), snap("c", { unreadCount: 2 })])
      .mockResolvedValueOnce(true);
    await nav.jumpUnread("a");
    expect(invoke).toHaveBeenNthCalledWith(2, "workspace_select", { id: "c" });
  });

  it("jumpUnread returns false when nothing is unread", async () => {
    invoke.mockResolvedValueOnce([snap("a"), snap("b")]);
    const moved = await nav.jumpUnread("a");
    expect(moved).toBe(false);
    expect(invoke).toHaveBeenCalledTimes(1);
  });
});

describe("WorkspaceNav chord routing via keybinding_resolve", () => {
  const invoke = vi.fn();
  let nav: WorkspaceNav;

  beforeEach(() => {
    invoke.mockReset();
    nav = new WorkspaceNav(invoke);
  });

  const ctrl = (key: string): ChordEvent => ({
    key,
    ctrlKey: true,
    metaKey: false,
    altKey: false,
    shiftKey: false,
  });

  it("eventToChord normalizes modifier order", () => {
    expect(eventToChord(ctrl("Tab"))).toBe("ctrl+tab");
  });

  it("a chord bound to workspace.next triggers step(+1) via the registry", async () => {
    // resolveChord(workspace.next) → "ctrl+tab"; the event matches → next.
    invoke
      .mockResolvedValueOnce("ctrl+tab") // keybinding_resolve workspace.next
      .mockResolvedValueOnce([snap("a"), snap("b")]) // workspace_snapshot
      .mockResolvedValueOnce(true); // workspace_select
    const handled = await nav.handleKey(ctrl("Tab"), "a");
    expect(handled).toBe(true);
    expect(invoke).toHaveBeenNthCalledWith(1, "keybinding_resolve", { action: "workspace.next" });
    expect(invoke).toHaveBeenNthCalledWith(3, "workspace_select", { id: "b" });
  });

  it("an unbound chord is ignored (no nav command issued)", async () => {
    // All actions resolve to something other than the pressed chord.
    invoke.mockResolvedValue(null);
    const handled = await nav.handleKey(ctrl("q"), "a");
    expect(handled).toBe(false);
    // Only keybinding_resolve calls happened — never a workspace_* mutation.
    expect(invoke.mock.calls.every((c) => c[0] === "keybinding_resolve")).toBe(true);
  });
});
