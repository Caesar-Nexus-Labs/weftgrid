import { describe, it, expect, vi } from "vitest";
import {
  onNotificationChanged,
  fetchPaneState,
  fetchUnreadCount,
  clearPaneNotification,
  markPaneRead,
  NOTIFICATION_CHANGED_EVENT,
  type PaneRingState,
  type ListenFn,
} from "./notification-client";

const RING: PaneRingState = {
  paneId: "pane-1",
  hasRing: true,
  latest: {
    id: "n1",
    paneId: "pane-1",
    title: "Claude Code",
    subtitle: "",
    body: "Agent needs input",
    seq: 0,
    isRead: false,
  },
};

describe("onNotificationChanged", () => {
  it("subscribes to the backend event and forwards the payload", async () => {
    let captured: ((e: { payload: PaneRingState }) => void) | undefined;
    const unlisten = vi.fn();
    const listen: ListenFn = vi.fn(async (_event, handler) => {
      captured = handler as (e: { payload: PaneRingState }) => void;
      return unlisten;
    });
    const received: PaneRingState[] = [];

    const off = await onNotificationChanged(listen, (s) => received.push(s));

    expect(listen).toHaveBeenCalledWith(NOTIFICATION_CHANGED_EVENT, expect.any(Function));
    captured!({ payload: RING });
    expect(received).toEqual([RING]);
    expect(off).toBe(unlisten);
  });
});

describe("notification command wrappers", () => {
  it("fetchPaneState invokes notify_pane_state with the pane id", async () => {
    const invoke = vi.fn().mockResolvedValue(RING);
    const got = await fetchPaneState(invoke, "pane-1");
    expect(invoke).toHaveBeenCalledWith("notify_pane_state", { paneId: "pane-1" });
    expect(got).toBe(RING);
  });

  it("fetchUnreadCount invokes notify_unread_count", async () => {
    const invoke = vi.fn().mockResolvedValue(3);
    expect(await fetchUnreadCount(invoke)).toBe(3);
    expect(invoke).toHaveBeenCalledWith("notify_unread_count");
  });

  it("clearPaneNotification invokes notify_clear (the focus hook)", async () => {
    const invoke = vi.fn().mockResolvedValue(true);
    expect(await clearPaneNotification(invoke, "pane-1")).toBe(true);
    expect(invoke).toHaveBeenCalledWith("notify_clear", { paneId: "pane-1" });
  });

  it("markPaneRead invokes notify_mark_read", async () => {
    const invoke = vi.fn().mockResolvedValue(true);
    expect(await markPaneRead(invoke, "pane-1")).toBe(true);
    expect(invoke).toHaveBeenCalledWith("notify_mark_read", { paneId: "pane-1" });
  });
});
