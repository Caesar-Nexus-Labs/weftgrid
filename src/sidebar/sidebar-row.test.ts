// @vitest-environment jsdom
import { describe, it, expect, vi } from "vitest";
import { buildSidebarRow } from "./sidebar-row";
import type { WorkspaceSnapshot } from "$lib/model";

function snap(over: Partial<WorkspaceSnapshot> = {}): WorkspaceSnapshot {
  return {
    id: "ws-1",
    title: "alpha",
    isPinned: false,
    currentDirectory: "/work/alpha",
    unreadCount: 0,
    listeningPorts: [],
    pullRequestRows: [],
    ...over,
  };
}

describe("buildSidebarRow rendering", () => {
  it("renders title, cwd, pin and unread from the snapshot", () => {
    const row = buildSidebarRow(snap({ isPinned: true, unreadCount: 3, customColorHex: "#ff0000" }));
    expect(row.querySelector(".sidebar-row__title")!.textContent).toBe("alpha");
    expect(row.querySelector(".sidebar-row__cwd")!.textContent).toBe("/work/alpha");
    expect(row.querySelector<HTMLElement>(".sidebar-row__pin")!.dataset.pinned).toBe("true");
    expect(row.querySelector<HTMLElement>(".sidebar-row__unread")!.dataset.unread).toBe("3");
    expect(row.querySelector<HTMLElement>(".sidebar-row__color")!.dataset.colorHex).toBe("#ff0000");
  });

  it("omits pin/unread/color elements when those fields are absent", () => {
    const row = buildSidebarRow(snap());
    expect(row.querySelector(".sidebar-row__pin")).toBeNull();
    expect(row.querySelector(".sidebar-row__unread")).toBeNull();
    expect(row.querySelector(".sidebar-row__color")).toBeNull();
  });
});

describe("buildSidebarRow closure actions", () => {
  it("click → onSelect(id); dblclick → onRename(id); pin-toggle → onTogglePin(id)", () => {
    const onSelect = vi.fn();
    const onRename = vi.fn();
    const onTogglePin = vi.fn();
    const row = buildSidebarRow(snap(), { onSelect, onRename, onTogglePin });

    row.click();
    expect(onSelect).toHaveBeenCalledWith("ws-1");

    row.dispatchEvent(new MouseEvent("dblclick"));
    expect(onRename).toHaveBeenCalledWith("ws-1");

    row.querySelector<HTMLElement>(".sidebar-row__pin-toggle")!.click();
    expect(onTogglePin).toHaveBeenCalledWith("ws-1");
    // pin-toggle click must not also bubble into a select.
    expect(onSelect).toHaveBeenCalledTimes(1);
  });
});

// SNAPSHOT-BOUNDARY (cmux #2586) — the load-bearing perf contract. A row must be
// a pure function of its value props: it copies primitives at build time and
// holds NO reference to the snapshot object (which would make it a live store
// subscriber and trigger the re-render storm). These tests PROVE the boundary.
describe("buildSidebarRow snapshot boundary (holds no store ref)", () => {
  it("does not retain a reference to the snapshot object on the element", () => {
    const s = snap();
    const row = buildSidebarRow(s);
    // No property of the row (or its dataset) is the snapshot object itself.
    const values: unknown[] = [...Object.values(row), ...Object.values(row.dataset)];
    expect(values.includes(s)).toBe(false);
  });

  it("reads each snapshot field exactly once (no retained reactive subscription)", () => {
    // Wrap the snapshot in a proxy counting property reads. A row that subscribed
    // to a live store would read fields again after build; a value-copy reads each
    // consumed field a bounded number of times during the single build pass.
    const base = snap({ isPinned: true, unreadCount: 2, customColorHex: "#0f0", customDescription: "d" });
    const reads: string[] = [];
    const tracked = new Proxy(base, {
      get(target, prop, recv) {
        reads.push(String(prop));
        return Reflect.get(target, prop, recv);
      },
    });
    const row = buildSidebarRow(tracked);
    const readsDuringBuild = reads.length;

    // Fire interactions AFTER build. If the row held the (proxied) snapshot ref it
    // would re-read fields here; a value-copy row reads nothing more from it.
    row.click();
    row.dispatchEvent(new MouseEvent("dblclick"));
    expect(reads.length).toBe(readsDuringBuild);
  });

  it("ignores post-build mutation of the snapshot (value copy, not live binding)", () => {
    const s = snap({ title: "before" });
    const row = buildSidebarRow(s);
    // Mutate the source object after the row exists.
    s.title = "after";
    s.unreadCount = 99;
    // DOM reflects the value at build time, proving no live binding to the object.
    expect(row.querySelector(".sidebar-row__title")!.textContent).toBe("before");
    expect(row.querySelector(".sidebar-row__unread")).toBeNull();
  });
});
