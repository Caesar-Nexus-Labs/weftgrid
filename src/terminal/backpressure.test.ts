import { describe, it, expect } from "vitest";
import { BackpressureController } from "./backpressure";

function makeHooks() {
  const calls: string[] = [];
  return {
    calls,
    hooks: {
      pause: () => calls.push("pause"),
      resume: () => calls.push("resume"),
    },
  };
}

describe("BackpressureController", () => {
  it("pauses once outstanding bytes cross the high-water mark", () => {
    const { calls, hooks } = makeHooks();
    const bp = new BackpressureController(hooks, { highWaterMark: 100, lowWaterMark: 40 });

    bp.onWritten(50);
    expect(bp.isPaused).toBe(false);
    expect(calls).toEqual([]);

    bp.onWritten(60); // 110 >= 100 → pause
    expect(bp.isPaused).toBe(true);
    expect(calls).toEqual(["pause"]);
  });

  it("does not re-pause while already paused (single pause call)", () => {
    const { calls, hooks } = makeHooks();
    const bp = new BackpressureController(hooks, { highWaterMark: 100, lowWaterMark: 40 });
    bp.onWritten(200); // pause
    bp.onWritten(200); // still paused, no second pause
    expect(calls).toEqual(["pause"]);
  });

  it("resumes when flush drains below the low-water mark", () => {
    const { calls, hooks } = makeHooks();
    const bp = new BackpressureController(hooks, { highWaterMark: 100, lowWaterMark: 40 });

    bp.onWritten(120); // pause (outstanding 120)
    expect(calls).toEqual(["pause"]);

    bp.onFlushed(50); // 70 > 40 → still paused
    expect(bp.isPaused).toBe(true);
    expect(calls).toEqual(["pause"]);

    bp.onFlushed(40); // 30 <= 40 → resume
    expect(bp.isPaused).toBe(false);
    expect(calls).toEqual(["pause", "resume"]);
  });

  it("hysteresis prevents thrash at the boundary", () => {
    const { calls, hooks } = makeHooks();
    const bp = new BackpressureController(hooks, { highWaterMark: 100, lowWaterMark: 40 });
    bp.onWritten(100); // pause
    bp.onFlushed(59); // 41 > 40 → still paused (no resume yet)
    expect(calls).toEqual(["pause"]);
    bp.onWritten(10); // 51, already paused → no extra pause
    expect(calls).toEqual(["pause"]);
    bp.onFlushed(20); // 31 <= 40 → resume
    expect(calls).toEqual(["pause", "resume"]);
  });

  it("never lets outstanding go negative", () => {
    const { hooks } = makeHooks();
    const bp = new BackpressureController(hooks);
    bp.onFlushed(1000);
    expect(bp.outstandingBytes).toBe(0);
  });
});
