// @vitest-environment jsdom
import { describe, it, expect, vi, beforeEach } from "vitest";
import { TerminalSession, type OutputChannel, type InvokeFn } from "./xterm-wrapper";

// jsdom does not implement matchMedia; xterm's `open()` calls it via
// CoreBrowserService. Provide a minimal stub so the terminal can mount.
beforeEach(() => {
  if (!window.matchMedia) {
    window.matchMedia = (query: string) =>
      ({
        matches: false,
        media: query,
        onchange: null,
        addEventListener: () => {},
        removeEventListener: () => {},
        addListener: () => {},
        removeListener: () => {},
        dispatchEvent: () => false,
      }) as unknown as MediaQueryList;
  }
});

/** A controllable fake of the Tauri binary Channel. */
class FakeChannel implements OutputChannel {
  onmessage: (m: ArrayBuffer | Uint8Array | number[]) => void = () => {};
  emit(bytes: Uint8Array): void {
    this.onmessage(bytes);
  }
}

function makeDeps() {
  const channel = new FakeChannel();
  const invoke = vi.fn<InvokeFn>(async (cmd: string) => {
    if (cmd === "pty_spawn") return 4242 as never;
    return undefined as never;
  });
  return {
    channel,
    invoke,
    deps: { invoke: invoke as InvokeFn, makeChannel: () => channel },
  };
}

describe("TerminalSession", () => {
  let parent: HTMLElement;

  beforeEach(() => {
    parent = document.createElement("div");
    document.body.appendChild(parent);
  });

  it("forwards onData keystrokes to pty_write as UTF-8 byte array", () => {
    const { invoke, deps } = makeDeps();
    const session = new TerminalSession("pane-1", deps);
    session.mount(parent);

    // Simulate the user typing "hi" — xterm emits onData("hi").
    session.term.input("hi");

    const writeCall = invoke.mock.calls.find((c) => c[0] === "pty_write");
    expect(writeCall).toBeDefined();
    const args = writeCall![1] as { paneId: string; data: number[] };
    expect(args.paneId).toBe("pane-1");
    expect(args.data).toEqual([104, 105]); // "hi"
  });

  it("spawn passes paneId + channel and returns the pid", async () => {
    const { invoke, deps } = makeDeps();
    const session = new TerminalSession("pane-2", deps);
    session.mount(parent);
    const pid = await session.spawn({ shell: "cmd.exe", rows: 30, cols: 100 });
    expect(pid).toBe(4242);
    const spawnCall = invoke.mock.calls.find((c) => c[0] === "pty_spawn");
    const args = spawnCall![1] as Record<string, unknown>;
    expect(args.paneId).toBe("pane-2");
    expect(args.shell).toBe("cmd.exe");
    expect(args.onOutput).toBeDefined();
  });

  it("writes channel batches to the terminal and tracks backpressure", async () => {
    const { channel, invoke, deps } = makeDeps();
    const session = new TerminalSession("pane-3", deps, {
      backpressure: { highWaterMark: 4, lowWaterMark: 1 },
    });
    session.mount(parent);
    const writeSpy = vi.spyOn(session.term, "write");

    channel.emit(new Uint8Array([65, 66, 67, 68, 69])); // 5 bytes >= high (4)
    expect(writeSpy).toHaveBeenCalledOnce();
    // pause requested because outstanding (5) crossed the high-water mark.
    const pauseCall = invoke.mock.calls.find((c) => c[0] === "pty_pause");
    expect(pauseCall).toBeDefined();
  });

  it("treats an empty batch as shell EOF and fires onClose", () => {
    const { channel, deps } = makeDeps();
    const session = new TerminalSession("pane-4", deps);
    session.mount(parent);
    const onClose = vi.fn();
    session.onClose(onClose);

    channel.emit(new Uint8Array([]));
    expect(onClose).toHaveBeenCalledOnce();
    expect(session.isClosed).toBe(true);
  });

  it("resize invokes pty_resize with current dimensions", () => {
    const { invoke, deps } = makeDeps();
    const session = new TerminalSession("pane-5", deps);
    session.mount(parent);
    session.resize();
    const resizeCall = invoke.mock.calls.find((c) => c[0] === "pty_resize");
    expect(resizeCall).toBeDefined();
    const args = resizeCall![1] as { paneId: string; rows: number; cols: number };
    expect(args.paneId).toBe("pane-5");
    expect(typeof args.rows).toBe("number");
    expect(typeof args.cols).toBe("number");
  });

  it("dispose kills the backend PTY", async () => {
    const { invoke, deps } = makeDeps();
    const session = new TerminalSession("pane-6", deps);
    session.mount(parent);
    await session.dispose();
    const killCall = invoke.mock.calls.find((c) => c[0] === "pty_kill");
    expect(killCall).toBeDefined();
    expect((killCall![1] as { paneId: string }).paneId).toBe("pane-6");
  });
});
