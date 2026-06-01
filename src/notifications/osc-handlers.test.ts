import { describe, it, expect, vi } from "vitest";
import {
  registerNotificationOscHandlers,
  NOTIFICATION_OSC_CODES,
  type OscCapableTerminal,
} from "./osc-handlers";

/** Fake xterm parser capturing registered OSC callbacks by code. */
function makeFakeTerm(): {
  term: OscCapableTerminal;
  handlers: Map<number, (data: string) => boolean | Promise<boolean>>;
  disposed: number[];
} {
  const handlers = new Map<number, (data: string) => boolean | Promise<boolean>>();
  const disposed: number[] = [];
  const term: OscCapableTerminal = {
    parser: {
      registerOscHandler(code, callback) {
        handlers.set(code, callback);
        return { dispose: () => disposed.push(code) };
      },
    },
  };
  return { term, handlers, disposed };
}

describe("registerNotificationOscHandlers", () => {
  it("registers a handler for each notification OSC code", () => {
    const { term, handlers } = makeFakeTerm();
    registerNotificationOscHandlers(term, "pane-1", vi.fn());
    expect([...handlers.keys()].sort((a, b) => a - b)).toEqual([9, 99, 777]);
    expect(NOTIFICATION_OSC_CODES).toEqual([9, 99, 777]);
  });

  it("forwards the de-framed payload to notify_ingest_osc with pane + code", () => {
    const { term, handlers } = makeFakeTerm();
    const invoke = vi.fn().mockResolvedValue(null);
    registerNotificationOscHandlers(term, "pane-7", invoke);

    handlers.get(777)!("notify;Title;Body");

    expect(invoke).toHaveBeenCalledWith("notify_ingest_osc", {
      paneId: "pane-7",
      code: 777,
      data: "notify;Title;Body",
    });
  });

  it("returns true from each handler so the sequence is not double-handled", () => {
    const { term, handlers } = makeFakeTerm();
    registerNotificationOscHandlers(term, "pane-1", vi.fn().mockResolvedValue(null));
    for (const code of NOTIFICATION_OSC_CODES) {
      expect(handlers.get(code)!("x")).toBe(true);
    }
  });

  it("swallows backend rejection without throwing from the handler", () => {
    const { term, handlers } = makeFakeTerm();
    const invoke = vi.fn().mockRejectedValue(new Error("ipc down"));
    registerNotificationOscHandlers(term, "pane-1", invoke);
    // Handler must still report handled synchronously despite the rejected promise.
    expect(() => handlers.get(9)!("body")).not.toThrow();
    expect(handlers.get(9)!("body")).toBe(true);
  });

  it("disposer removes every registered handler", () => {
    const { term, disposed } = makeFakeTerm();
    const dispose = registerNotificationOscHandlers(term, "pane-1", vi.fn());
    dispose();
    expect(disposed.sort((a, b) => a - b)).toEqual([9, 99, 777]);
  });
});
