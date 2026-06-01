// Terminal session wrapper — wires xterm to the PTY backend (P3).
//
// One TerminalSession = one pane = one xterm Terminal + one backend PTY. It owns
// the full data path:
//   - OUTPUT: backend coalesced batches arrive over a Tauri `Channel<ArrayBuffer>`
//     → `term.write(bytes, callback)`; the callback drives the BackpressureController
//     (outstanding-bytes → pty_pause/resume). An empty batch = shell EOF.
//   - INPUT: `term.onData` → `invoke('pty_write', {paneId, data})`.
//   - RESIZE: caller drives `fit()` (e.g. from a ResizeObserver) → fit addon
//     proposes rows/cols → `invoke('pty_resize', ...)`.
//   - RENDERER: WebGL with runtime DOM fallback (see renderer-select).
//   - FIND: addon-search controller (see find-bar).
//
// Tauri APIs are injected (invoke + Channel ctor) so the wrapper unit-tests
// without a live Tauri runtime.

import { Terminal, type ITerminalAddon as IXtermAddon } from "@xterm/xterm";
import { FitAddon } from "@xterm/addon-fit";
import { SearchAddon } from "@xterm/addon-search";
import { WebglAddon } from "@xterm/addon-webgl";
import {
  BackpressureController,
  type BackpressureOptions,
} from "./backpressure";
import { RendererSelector, type RendererKind } from "./renderer-select";
import { FindController } from "./find-bar";

/** Subset of `@tauri-apps/api/core`'s `invoke` we depend on. */
export type InvokeFn = <T>(cmd: string, args?: Record<string, unknown>) => Promise<T>;

/** A channel that delivers backend output bytes (Tauri `Channel` binary body). */
export interface OutputChannel {
  onmessage: (message: ArrayBuffer | Uint8Array | number[]) => void;
}

/** Factory producing a fresh output channel to hand to `pty_spawn`. */
export type ChannelFactory = () => OutputChannel;

export interface TerminalSessionDeps {
  invoke: InvokeFn;
  /** Creates the Channel passed to `pty_spawn` as `onOutput`. */
  makeChannel: ChannelFactory;
}

export interface SpawnConfig {
  shell?: string;
  cwd?: string;
  rows?: number;
  cols?: number;
}

export interface TerminalSessionOptions {
  backpressure?: Partial<BackpressureOptions>;
}

/** Normalize whatever the channel hands us into bytes for `term.write`. */
function toBytes(message: ArrayBuffer | Uint8Array | number[]): Uint8Array {
  if (message instanceof Uint8Array) {
    return message;
  }
  if (message instanceof ArrayBuffer) {
    return new Uint8Array(message);
  }
  return Uint8Array.from(message);
}

export class TerminalSession {
  readonly term: Terminal;
  readonly fit: FitAddon;
  readonly search: SearchAddon;
  readonly find: FindController;
  private readonly renderer: RendererSelector;
  private readonly backpressure: BackpressureController;
  private readonly channel: OutputChannel;
  private disposables: Array<{ dispose(): void }> = [];
  private closed = false;
  private onCloseHandler: (() => void) | null = null;

  constructor(
    readonly paneId: string,
    private readonly deps: TerminalSessionDeps,
    options: TerminalSessionOptions = {},
  ) {
    this.term = new Terminal({ allowProposedApi: true, convertEol: false });
    this.fit = new FitAddon();
    this.search = new SearchAddon();
    this.term.loadAddon(this.fit);
    this.term.loadAddon(this.search);

    this.find = new FindController(this.search, this.term);
    // Terminal.loadAddon accepts ITerminalAddon; RendererAddon is a structural
    // subset (WebglAddon satisfies both), so adapt the port explicitly.
    this.renderer = new RendererSelector(
      { loadAddon: (addon) => this.term.loadAddon(addon as unknown as IXtermAddon) },
      () => new WebglAddon(),
    );

    this.backpressure = new BackpressureController(
      {
        pause: () => void this.deps.invoke("pty_pause", { paneId: this.paneId }),
        resume: () => void this.deps.invoke("pty_resume", { paneId: this.paneId }),
      },
      options.backpressure,
    );

    this.channel = this.deps.makeChannel();
    this.channel.onmessage = (message) => this.handleOutput(toBytes(message));

    // INPUT: forward keystrokes to the PTY as raw bytes.
    this.disposables.push(
      this.term.onData((data: string) => {
        void this.deps.invoke("pty_write", {
          paneId: this.paneId,
          data: Array.from(new TextEncoder().encode(data)),
        });
      }),
    );
  }

  /** Attach to a DOM element and pick the renderer. Call before `spawn`. */
  mount(parent: HTMLElement): RendererKind {
    this.term.open(parent);
    return this.renderer.init();
  }

  /** Spawn the backend shell; output then flows over the channel. Returns pid. */
  async spawn(config: SpawnConfig = {}): Promise<number> {
    return this.deps.invoke<number>("pty_spawn", {
      paneId: this.paneId,
      shell: config.shell ?? null,
      cwd: config.cwd ?? null,
      rows: config.rows ?? this.term.rows,
      cols: config.cols ?? this.term.cols,
      onOutput: this.channel,
    });
  }

  /** OUTPUT path: write a coalesced batch with a backpressure-tracking callback. */
  private handleOutput(bytes: Uint8Array): void {
    if (bytes.length === 0) {
      // Empty batch = backend EOF convention (shell exited).
      this.closed = true;
      this.onCloseHandler?.();
      return;
    }
    const n = bytes.length;
    this.term.write(bytes, () => this.backpressure.onFlushed(n));
    this.backpressure.onWritten(n);
  }

  /** Fit to the container and push the new size to the PTY. */
  resize(): void {
    this.fit.fit();
    void this.deps.invoke("pty_resize", {
      paneId: this.paneId,
      rows: this.term.rows,
      cols: this.term.cols,
    });
  }

  /** Register a callback for shell exit (EOF over the channel). */
  onClose(handler: () => void): void {
    this.onCloseHandler = handler;
  }

  get isClosed(): boolean {
    return this.closed;
  }

  get rendererKind(): RendererKind {
    return this.renderer.renderer;
  }

  /** Tear down: kill the backend PTY and dispose the terminal + addons. */
  async dispose(): Promise<void> {
    for (const d of this.disposables) {
      d.dispose();
    }
    this.disposables = [];
    this.find.dispose();
    try {
      await this.deps.invoke("pty_kill", { paneId: this.paneId });
    } finally {
      this.term.dispose();
    }
  }
}
