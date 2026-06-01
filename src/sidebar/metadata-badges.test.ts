// @vitest-environment jsdom
import { describe, it, expect } from "vitest";
import { buildMetadataBadges, updateMetadataBadges } from "./metadata-badges";
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

function badgeKinds(el: HTMLElement): string[] {
  return [...el.querySelectorAll<HTMLElement>(".sidebar-badge")].map((b) => b.dataset.badge!);
}

describe("buildMetadataBadges shows a badge ONLY when its field is present", () => {
  it("renders no badges for a bare P15a snapshot (no enrichment yet)", () => {
    const el = buildMetadataBadges(snap());
    expect(el.querySelectorAll(".sidebar-badge")).toHaveLength(0);
  });

  it("renders the git badge only when gitBranchSummary is set", () => {
    expect(badgeKinds(buildMetadataBadges(snap()))).not.toContain("git");
    const el = buildMetadataBadges(snap({ gitBranchSummary: "main*" }));
    expect(badgeKinds(el)).toContain("git");
    expect(el.querySelector<HTMLElement>('[data-badge="git"]')!.textContent).toContain("main*");
  });

  it("renders the PR badge only when pullRequestRows is non-empty", () => {
    expect(badgeKinds(buildMetadataBadges(snap({ pullRequestRows: [] })))).not.toContain("pr");
    const el = buildMetadataBadges(snap({ pullRequestRows: ["#1 fix", "#2 feat"] }));
    expect(badgeKinds(el)).toContain("pr");
    expect(el.querySelector<HTMLElement>('[data-badge="pr"]')!.textContent).toContain("2");
  });

  it("renders the ports badge only when listeningPorts is non-empty", () => {
    expect(badgeKinds(buildMetadataBadges(snap({ listeningPorts: [] })))).not.toContain("ports");
    const el = buildMetadataBadges(snap({ listeningPorts: [3000, 5173] }));
    expect(badgeKinds(el)).toContain("ports");
    expect(el.querySelector<HTMLElement>('[data-badge="ports"]')!.title).toBe("3000, 5173");
  });

  it("renders the progress badge only when progress is defined (0 still shows)", () => {
    expect(badgeKinds(buildMetadataBadges(snap()))).not.toContain("progress");
    const el = buildMetadataBadges(snap({ progress: 0.42 }));
    expect(el.querySelector<HTMLElement>('[data-badge="progress"]')!.dataset.progress).toBe("42");
    // progress 0 is a real value → badge present.
    expect(badgeKinds(buildMetadataBadges(snap({ progress: 0 })))).toContain("progress");
  });

  it("renders status + notif badges only when their text is present", () => {
    const none = buildMetadataBadges(snap());
    expect(badgeKinds(none)).not.toContain("status");
    expect(badgeKinds(none)).not.toContain("notif");
    const el = buildMetadataBadges(
      snap({ remoteConnectionStatusText: "connected", latestNotificationText: "build done" }),
    );
    expect(badgeKinds(el)).toEqual(expect.arrayContaining(["status", "notif"]));
  });

  it("renders the conversation badge only when latestConversationMessage is present", () => {
    expect(badgeKinds(buildMetadataBadges(snap()))).not.toContain("conversation");
    const el = buildMetadataBadges(snap({ latestConversationMessage: "agent: done" }));
    expect(badgeKinds(el)).toContain("conversation");
    expect(el.querySelector<HTMLElement>('[data-badge="conversation"]')!.title).toBe("agent: done");
  });

  it("renders every badge when all fields are present", () => {
    const el = buildMetadataBadges(
      snap({
        gitBranchSummary: "main",
        pullRequestRows: ["#1"],
        listeningPorts: [8080],
        progress: 0.5,
        remoteConnectionStatusText: "ssh ok",
        latestNotificationText: "tests passed",
        latestConversationMessage: "agent: shipped",
      }),
    );
    expect(badgeKinds(el).sort()).toEqual([
      "conversation",
      "git",
      "notif",
      "ports",
      "pr",
      "progress",
      "status",
    ]);
  });
});

// The receiver-driven update path (P13 push → P12 store → fresh snapshot) replaces
// the row's badges with a NEW container built from the immutable value. These tests
// prove the swap reflects new data and never holds a live snapshot reference.
describe("updateMetadataBadges re-renders from a fresh snapshot (snapshot boundary)", () => {
  it("replaces the prior badge container with one built from the new snapshot", () => {
    const host = document.createElement("div");
    updateMetadataBadges(host, snap()); // bare → no badges
    expect(host.querySelectorAll(".sidebar-badges")).toHaveLength(1);
    expect(host.querySelectorAll(".sidebar-badge")).toHaveLength(0);

    // A push arrives: git branch + progress now present.
    updateMetadataBadges(host, snap({ gitBranchSummary: "main*", progress: 0.3 }));
    // Still exactly one container (old one removed, not stacked).
    expect(host.querySelectorAll(".sidebar-badges")).toHaveLength(1);
    expect(badgeKinds(host)).toEqual(expect.arrayContaining(["git", "progress"]));
  });

  it("clears a badge when the field disappears from a later snapshot", () => {
    const host = document.createElement("div");
    updateMetadataBadges(host, snap({ gitBranchSummary: "main" }));
    expect(badgeKinds(host)).toContain("git");
    // Later snapshot no longer carries the branch → badge gone.
    updateMetadataBadges(host, snap());
    expect(badgeKinds(host)).not.toContain("git");
  });

  it("ignores post-render mutation of the snapshot object (value copy, not live binding)", () => {
    const host = document.createElement("div");
    const s = snap({ gitBranchSummary: "before" });
    updateMetadataBadges(host, s);
    // Mutate the source after render — the DOM must NOT change (no store ref held).
    s.gitBranchSummary = "after";
    s.progress = 0.99;
    expect(host.querySelector<HTMLElement>('[data-badge="git"]')!.textContent).toContain("before");
    expect(badgeKinds(host)).not.toContain("progress");
  });
});
