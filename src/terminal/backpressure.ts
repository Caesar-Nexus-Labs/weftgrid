// Backpressure — flow-control layer 3 (P3 red-team C4).
//
// xterm's `write(data, callback)` queues data on the main thread; the callback
// fires once that chunk is parsed/rendered. Under flood the queue grows
// unbounded and the UI freezes. We count OUTSTANDING bytes (written but not yet
// flushed) and, once they cross a high-water mark, ask the backend to stop
// reading the PTY (`pty_pause`). When the callbacks drain the count back below a
// low-water mark we `pty_resume`. This pushes backpressure all the way down to
// ConPTY/the shell, which throttle themselves.
//
// Hysteresis (high != low) prevents pause/resume thrashing at the threshold.
// This module is pure (no Tauri, no DOM): it takes pause/resume callbacks, so it
// is trivially unit-testable.

/** Tuning for the outstanding-bytes backpressure window. */
export interface BackpressureOptions {
  /** Pause reading once outstanding bytes reach this (default 1 MiB). */
  highWaterMark: number;
  /** Resume reading once outstanding drops to/below this (default 256 KiB). */
  lowWaterMark: number;
}

export const DEFAULT_BACKPRESSURE: BackpressureOptions = {
  highWaterMark: 1024 * 1024,
  lowWaterMark: 256 * 1024,
};

/** Called when the high/low marks are crossed. Async (IPC) but fire-and-forget. */
export interface BackpressureHooks {
  pause(): void;
  resume(): void;
}

/**
 * Tracks bytes written to xterm but not yet flushed (callback pending) and
 * drives pause/resume with hysteresis.
 *
 * Usage per pane:
 *   const bp = new BackpressureController(hooks);
 *   term.write(batch, () => bp.onFlushed(batch.length));
 *   bp.onWritten(batch.length);   // before/after write — order doesn't matter
 */
export class BackpressureController {
  private outstanding = 0;
  private paused = false;
  private readonly high: number;
  private readonly low: number;

  constructor(
    private readonly hooks: BackpressureHooks,
    options: Partial<BackpressureOptions> = {},
  ) {
    this.high = options.highWaterMark ?? DEFAULT_BACKPRESSURE.highWaterMark;
    this.low = options.lowWaterMark ?? DEFAULT_BACKPRESSURE.lowWaterMark;
  }

  /** Record `n` bytes handed to `term.write`; may trip the high-water pause. */
  onWritten(n: number): void {
    this.outstanding += n;
    if (!this.paused && this.outstanding >= this.high) {
      this.paused = true;
      this.hooks.pause();
    }
  }

  /** Record `n` bytes whose write callback fired; may trip the low-water resume. */
  onFlushed(n: number): void {
    this.outstanding = Math.max(0, this.outstanding - n);
    if (this.paused && this.outstanding <= this.low) {
      this.paused = false;
      this.hooks.resume();
    }
  }

  get outstandingBytes(): number {
    return this.outstanding;
  }

  get isPaused(): boolean {
    return this.paused;
  }
}
