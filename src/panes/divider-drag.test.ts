// @vitest-environment jsdom
import { describe, it, expect, vi } from "vitest";
import { positionFromPointer, attachDividerDrag } from "./divider-drag";

describe("positionFromPointer", () => {
  const bounds = { left: 100, top: 50, width: 400, height: 200 };

  it("maps x against width for a horizontal split, clamped", () => {
    expect(positionFromPointer("horizontal", bounds, 300, 0)).toBeCloseTo(0.5); // (300-100)/400
    expect(positionFromPointer("horizontal", bounds, 100, 0)).toBe(0.1); // <0 → clamp min
    expect(positionFromPointer("horizontal", bounds, 999, 0)).toBe(0.9); // >1 → clamp max
  });

  it("maps y against height for a vertical split", () => {
    expect(positionFromPointer("vertical", bounds, 0, 150)).toBeCloseTo(0.5); // (150-50)/200
  });
});

/** Build a pointer-like event jsdom will dispatch (no native PointerEvent dep). */
function pointer(type: string, props: Record<string, number>): Event {
  const e = new Event(type, { bubbles: true });
  Object.assign(e, { pointerId: 1, ...props });
  return e;
}

describe("attachDividerDrag", () => {
  function setup() {
    const divider = document.createElement("div");
    const container = document.createElement("div");
    // jsdom getBoundingClientRect returns zeros; stub a real rect.
    container.getBoundingClientRect = () =>
      ({ left: 0, top: 0, width: 1000, height: 500, right: 1000, bottom: 500, x: 0, y: 0, toJSON() {} }) as DOMRect;
    document.body.append(container, divider);
    return { divider, container };
  }

  it("batches pointermove into one rAF call carrying the last position", () => {
    const { divider, container } = setup();
    const frames: Array<() => void> = [];
    const onResize = vi.fn();
    attachDividerDrag(
      divider,
      { orientation: "horizontal", container, onResize },
      { requestFrame: (cb) => (frames.push(cb), frames.length), cancelFrame: () => {} },
    );

    divider.dispatchEvent(pointer("pointerdown", { clientX: 500, clientY: 0 }));
    divider.dispatchEvent(pointer("pointermove", { clientX: 300, clientY: 0 }));
    divider.dispatchEvent(pointer("pointermove", { clientX: 400, clientY: 0 }));
    // No layout yet — coalesced until the frame fires.
    expect(onResize).not.toHaveBeenCalled();
    expect(frames).toHaveLength(1);

    frames[0](); // flush the frame
    expect(onResize).toHaveBeenCalledTimes(1);
    expect(onResize).toHaveBeenCalledWith(0.4); // last move = 400/1000
  });

  it("ignores moves before pointerdown", () => {
    const { divider, container } = setup();
    const onResize = vi.fn();
    const frames: Array<() => void> = [];
    attachDividerDrag(
      divider,
      { orientation: "horizontal", container, onResize },
      { requestFrame: (cb) => (frames.push(cb), frames.length), cancelFrame: () => {} },
    );
    divider.dispatchEvent(pointer("pointermove", { clientX: 300, clientY: 0 }));
    expect(frames).toHaveLength(0);
    expect(onResize).not.toHaveBeenCalled();
  });

  it("commits the final position on pointerup", () => {
    const { divider, container } = setup();
    const onResize = vi.fn();
    const onCommit = vi.fn();
    attachDividerDrag(
      divider,
      { orientation: "vertical", container, onResize, onCommit },
      { requestFrame: (cb) => 0, cancelFrame: () => {} },
    );
    divider.dispatchEvent(pointer("pointerdown", { clientX: 0, clientY: 100 }));
    divider.dispatchEvent(pointer("pointerup", { clientX: 0, clientY: 250 }));
    expect(onResize).toHaveBeenLastCalledWith(0.5); // 250/500
    expect(onCommit).toHaveBeenCalledWith(0.5);
  });

  it("dispose removes listeners so later moves do nothing", () => {
    const { divider, container } = setup();
    const onResize = vi.fn();
    const frames: Array<() => void> = [];
    const dispose = attachDividerDrag(
      divider,
      { orientation: "horizontal", container, onResize },
      { requestFrame: (cb) => (frames.push(cb), frames.length), cancelFrame: () => {} },
    );
    dispose();
    divider.dispatchEvent(pointer("pointerdown", { clientX: 500, clientY: 0 }));
    divider.dispatchEvent(pointer("pointermove", { clientX: 300, clientY: 0 }));
    expect(frames).toHaveLength(0);
    expect(onResize).not.toHaveBeenCalled();
  });
});
