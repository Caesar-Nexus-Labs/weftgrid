// @vitest-environment jsdom
import { describe, it, expect, vi } from "vitest";
import { buildSidebarTree, type SidebarGroup } from "./sidebar-tree";
import type { WorkspaceSnapshot } from "$lib/model";

/** Minimal snapshot with only the cheap/local P15a fields populated. */
function snap(id: string, title: string, over: Partial<WorkspaceSnapshot> = {}): WorkspaceSnapshot {
  return {
    id,
    title,
    isPinned: false,
    currentDirectory: `/work/${title}`,
    unreadCount: 0,
    listeningPorts: [],
    pullRequestRows: [],
    ...over,
  };
}

describe("buildSidebarTree", () => {
  it("renders N snapshots as N rows in array (sidebar) order", () => {
    const snaps = [snap("a", "alpha"), snap("b", "beta"), snap("c", "gamma")];
    const tree = buildSidebarTree(snaps);
    const rows = [...tree.querySelectorAll(".sidebar-row")];
    expect(rows).toHaveLength(3);
    expect(rows.map((r) => (r as HTMLElement).dataset.workspaceId)).toEqual(["a", "b", "c"]);
  });

  it("marks the selected row aria-selected and tabbable", () => {
    const tree = buildSidebarTree([snap("a", "alpha"), snap("b", "beta")], {}, { selectedId: "b" });
    const selected = tree.querySelector<HTMLElement>('[data-workspace-id="b"]')!;
    expect(selected.getAttribute("aria-selected")).toBe("true");
    expect(selected.tabIndex).toBe(0);
  });

  it("renders a group header followed by its member rows when expanded", () => {
    const snaps = [snap("a", "alpha"), snap("b", "beta"), snap("c", "gamma")];
    const groups: SidebarGroup[] = [
      { id: "g1", name: "Backend", isCollapsed: false, memberIds: ["b", "c"] },
    ];
    const tree = buildSidebarTree(snaps, {}, { groups });
    const header = tree.querySelector<HTMLElement>(".sidebar-group-header")!;
    expect(header.dataset.groupId).toBe("g1");
    // Ungrouped "a" renders, group header + its 2 members render → 3 rows total.
    expect(tree.querySelectorAll(".sidebar-row")).toHaveLength(3);
    expect(header.querySelector(".sidebar-group-header__count")!.textContent).toBe("2");
  });

  it("collapsed group hides its members but keeps the header", () => {
    const snaps = [snap("a", "alpha"), snap("b", "beta"), snap("c", "gamma")];
    const groups: SidebarGroup[] = [
      { id: "g1", name: "Backend", isCollapsed: true, memberIds: ["b", "c"] },
    ];
    const tree = buildSidebarTree(snaps, {}, { groups });
    expect(tree.querySelector(".sidebar-group-header")).not.toBeNull();
    // Only ungrouped "a" is visible; b and c are hidden under the collapsed group.
    const rows = [...tree.querySelectorAll<HTMLElement>(".sidebar-row")];
    expect(rows.map((r) => r.dataset.workspaceId)).toEqual(["a"]);
  });

  it("toggling a group header invokes onToggleGroup with the group id", () => {
    const onToggleGroup = vi.fn();
    const groups: SidebarGroup[] = [
      { id: "g1", name: "Backend", isCollapsed: false, memberIds: ["a"] },
    ];
    const tree = buildSidebarTree([snap("a", "alpha")], { onToggleGroup }, { groups });
    tree.querySelector<HTMLElement>(".sidebar-group-header")!.click();
    expect(onToggleGroup).toHaveBeenCalledWith("g1");
  });
});
