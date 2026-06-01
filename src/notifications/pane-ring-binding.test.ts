// @vitest-environment jsdom
import { describe, it, expect, vi } from "vitest";
import { PaneRingBinding, type PaneRingView } from "./pane-ring-binding";
import {
  NOTIFICATION_CHANGED_EVENT,
  type ListenFn,
  type PaneRingState,
} from "./notification-client";

function ringState(paneId: string, hasRing: boolean): PaneRingState {
  return {
    paneId,
    hasRing,
    latest: hasRing
      ? {
          id: "n1",
          paneId,
          title: "Claude Code",
          subtitle: "",
          body: "Agent needs input",
          seq: 1,
          isRead: false,
        }
      : null,
  };
}

/** A spy view + a controllable `listen` that captures the backend handler. */
function harness() {
  const view: PaneRingView = {
    setPaneRing: vi.fn(),
    setTabHighlight: vi.fn(),
  };
  let emit: ((s: PaneRingState) => void) | undefined;
  const unlisten = vi.fn();
  const listen: ListenFn = vi.fn(async (_event, handler) => {
    emit = (s) => handler({ payload: s as unknown });
    return unlisten;
  }) as ListenFn;
  return { view, listen, unlisten, emit: () => emit! };
}

describe("PaneRingBinding event wiring", () => {
  it("subscribes to notification-changed on start", async () => {
    const { view, listen } = harness();
    const invoke = vi.fn();
    const binding = new PaneRingBinding({ listen, invoke, view });
    await binding.start();
    expect(listen).toHaveBeenCalledWith(NOTIFICATION_CHANGED_EVENT, expect.any(Function));
  });

  it("hasRing=true → drives pane ring + tab highlight on", async () => {
    const { view, listen, emit } = harness();
    const invoke = vi.fn();
    const binding = new PaneRingBinding({ listen, invoke, view });
    await binding.start();

    emit()(ringState("pane-1", true));

    expect(view.setPaneRing).toHaveBeenCalledWith("pane-1", true);
    expect(view.setTabHighlight).toHaveBeenCalledWith("pane-1", true);
  });

  it("hasRing=false → clears pane ring + tab highlight", async () => {
    const { view, listen, emit } = harness();
    const invoke = vi.fn();
    const binding = new PaneRingBinding({ listen, invoke, view });
    await binding.start();

    emit()(ringState("pane-2", false));

    expect(view.setPaneRing).toHaveBeenCalledWith("pane-2", false);
    expect(view.setTabHighlight).toHaveBeenCalledWith("pane-2", false);
  });

  it("start is idempotent (no double subscribe)", async () => {
    const { view, listen } = harness();
    const invoke = vi.fn();
    const binding = new PaneRingBinding({ listen, invoke, view });
    await binding.start();
    await binding.start();
    expect(listen).toHaveBeenCalledTimes(1);
  });

  it("stop unsubscribes", async () => {
    const { view, listen, unlisten } = harness();
    const invoke = vi.fn();
    const binding = new PaneRingBinding({ listen, invoke, view });
    await binding.start();
    binding.stop();
    expect(unlisten).toHaveBeenCalledTimes(1);
  });
});

describe("PaneRingBinding clear-on-focus", () => {
  it("invokes notify_clear and optimistically clears the view", async () => {
    const { view, listen } = harness();
    const invoke = vi.fn().mockResolvedValue(true);
    const binding = new PaneRingBinding({ listen, invoke, view });
    await binding.start();

    const result = await binding.clearOnFocus("pane-1");

    expect(invoke).toHaveBeenCalledWith("notify_clear", { paneId: "pane-1" });
    expect(view.setPaneRing).toHaveBeenCalledWith("pane-1", false);
    expect(view.setTabHighlight).toHaveBeenCalledWith("pane-1", false);
    expect(result).toBe(true);
  });
});

describe("PaneRingBinding mount sync", () => {
  it("syncPane fetches notify_pane_state and applies it", async () => {
    const { view, listen } = harness();
    const invoke = vi.fn().mockResolvedValue(ringState("pane-3", true));
    const binding = new PaneRingBinding({ listen, invoke, view });

    await binding.syncPane("pane-3");

    expect(invoke).toHaveBeenCalledWith("notify_pane_state", { paneId: "pane-3" });
    expect(view.setPaneRing).toHaveBeenCalledWith("pane-3", true);
    expect(view.setTabHighlight).toHaveBeenCalledWith("pane-3", true);
  });
});
