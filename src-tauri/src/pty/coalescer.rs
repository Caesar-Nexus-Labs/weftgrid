//! Coalescing layer — flow-control layer 2 (P3 red-team C4).
//!
//! Raw PTY reads are bursty (thousands of tiny chunks/sec under flood). Sending
//! each chunk over the IPC Channel would serialize/cross the bridge far too often
//! and drown the webview. The coalescer buffers per-pane bytes inside an 8–16ms
//! window (or until a size cap) and emits ONE batch per window — collapsing N
//! reads into 1 channel send.
//!
//! Two pieces:
//! - [`Coalescer`]: pure per-pane buffer + flush decision (time/size). Takes `now`
//!   as a parameter so it is fully deterministic and unit-testable (no clock dep).
//! - [`CoalescerHub`]: the runtime registry mapping `pane_id -> (Coalescer, sink)`.
//!   The reader pushes raw bytes here; a flusher thread (see `mod.rs`) periodically
//!   drains due panes into their [`BatchSink`].

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// Pane identity at the PTY layer is the opaque id the frontend assigns to a
/// terminal surface. Kept a `String` (not `model::PaneId`) so this layer stays
/// decoupled from the workspace model and trivially unit-testable.
pub type PaneId = String;

/// Default coalesce window — middle of the spec's 8–16ms band. Tuned manually
/// against real flood throughput (see phase Manual Perf Acceptance).
pub const DEFAULT_WINDOW: Duration = Duration::from_millis(8);
/// Flush early if a single window accumulates this many bytes (flood relief),
/// so latency never spikes waiting for the timer when data is plentiful.
pub const DEFAULT_SIZE_CAP: usize = 64 * 1024;

/// Pure per-pane byte buffer with a time+size flush rule. No clock of its own:
/// callers pass `now`, making every decision deterministic for tests.
pub struct Coalescer {
    buf: Vec<u8>,
    window: Duration,
    size_cap: usize,
    /// When the current (non-empty) batch started accumulating.
    first_push: Option<Instant>,
}

impl Coalescer {
    pub fn new(window: Duration, size_cap: usize) -> Self {
        Coalescer {
            buf: Vec::new(),
            window,
            size_cap,
            first_push: None,
        }
    }

    /// Append a raw read. Records the batch start time on the first byte.
    pub fn push(&mut self, now: Instant, data: &[u8]) {
        if self.buf.is_empty() {
            self.first_push = Some(now);
        }
        self.buf.extend_from_slice(data);
    }

    pub fn is_empty(&self) -> bool {
        self.buf.is_empty()
    }

    /// True when the buffer should be emitted: it is non-empty AND either the
    /// size cap is reached or the time window has elapsed since `first_push`.
    pub fn is_flush_due(&self, now: Instant) -> bool {
        if self.buf.is_empty() {
            return false;
        }
        if self.buf.len() >= self.size_cap {
            return true;
        }
        self.first_push
            .is_some_and(|start| now.duration_since(start) >= self.window)
    }

    /// Drain the accumulated batch, resetting the window timer.
    pub fn take(&mut self) -> Vec<u8> {
        self.first_push = None;
        std::mem::take(&mut self.buf)
    }
}

/// Downstream of coalescing: where a finished batch goes (the IPC Channel in
/// production, a collecting `Vec` in tests). One sink per pane.
pub trait BatchSink: Send + Sync {
    /// Deliver one coalesced batch of PTY output to the UI.
    fn send_batch(&self, data: Vec<u8>);
    /// Signal the shell/PTY closed (EOF). Sent once after the final batch.
    fn on_closed(&self);
}

struct PaneEntry {
    coalescer: Coalescer,
    sink: Box<dyn BatchSink>,
    /// EOF seen by the reader; flush remaining bytes then notify + drop.
    closed: bool,
}

/// Runtime registry: reader threads push raw bytes here; the flusher thread
/// drains due panes. Implements [`super::reader::RawSink`] so the reader pool
/// can target it directly without knowing about coalescing internals.
pub struct CoalescerHub {
    panes: Mutex<HashMap<PaneId, PaneEntry>>,
    window: Duration,
    size_cap: usize,
}

impl CoalescerHub {
    pub fn new(window: Duration, size_cap: usize) -> Self {
        CoalescerHub {
            panes: Mutex::new(HashMap::new()),
            window,
            size_cap,
        }
    }

    /// Register a pane's downstream sink (called from `pty_spawn`).
    pub fn register(&self, pane: PaneId, sink: Box<dyn BatchSink>) {
        let mut panes = self.panes.lock().unwrap();
        panes.insert(
            pane,
            PaneEntry {
                coalescer: Coalescer::new(self.window, self.size_cap),
                sink,
                closed: false,
            },
        );
    }

    /// Drop a pane (called from `pty_kill`).
    pub fn remove(&self, pane: &PaneId) {
        self.panes.lock().unwrap().remove(pane);
    }

    /// Flush every pane whose window/size rule is satisfied, plus finalize any
    /// pane that hit EOF. Returns the number of batches sent (test signal).
    pub fn flush_due(&self, now: Instant) -> usize {
        let mut panes = self.panes.lock().unwrap();
        let mut sent = 0;
        let mut finished: Vec<PaneId> = Vec::new();

        for (id, entry) in panes.iter_mut() {
            if entry.coalescer.is_flush_due(now) {
                entry.sink.send_batch(entry.coalescer.take());
                sent += 1;
            }
            if entry.closed && entry.coalescer.is_empty() {
                entry.sink.on_closed();
                finished.push(id.clone());
            }
        }
        for id in finished {
            panes.remove(&id);
        }
        sent
    }
}

impl super::reader::RawSink for CoalescerHub {
    fn on_data(&self, pane: &PaneId, data: &[u8]) {
        let now = Instant::now();
        if let Some(entry) = self.panes.lock().unwrap().get_mut(pane) {
            entry.coalescer.push(now, data);
        }
    }

    fn on_eof(&self, pane: &PaneId) {
        if let Some(entry) = self.panes.lock().unwrap().get_mut(pane) {
            entry.closed = true;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex as StdMutex};

    fn now() -> Instant {
        Instant::now()
    }

    #[test]
    fn not_flush_due_within_window() {
        let mut c = Coalescer::new(Duration::from_millis(8), 1024);
        let t0 = now();
        c.push(t0, b"hi");
        // Same instant: window not elapsed, under size cap → not due.
        assert!(!c.is_flush_due(t0));
    }

    #[test]
    fn flush_due_after_window_elapses() {
        let mut c = Coalescer::new(Duration::from_millis(8), 1024);
        let t0 = now();
        c.push(t0, b"hi");
        assert!(c.is_flush_due(t0 + Duration::from_millis(8)));
    }

    #[test]
    fn flush_due_when_size_cap_reached() {
        let mut c = Coalescer::new(Duration::from_secs(60), 4);
        let t0 = now();
        c.push(t0, b"abcd"); // == cap, window far away → due by size.
        assert!(c.is_flush_due(t0));
    }

    #[test]
    fn take_resets_and_concatenates() {
        let mut c = Coalescer::new(Duration::from_millis(8), 1024);
        let t0 = now();
        c.push(t0, b"foo");
        c.push(t0, b"bar");
        assert_eq!(c.take(), b"foobar");
        assert!(c.is_empty());
        assert!(!c.is_flush_due(t0 + Duration::from_secs(1)));
    }

    #[test]
    fn many_reads_collapse_into_one_batch() {
        // Flood: 1000 tiny reads within one window → exactly 1 batch on take.
        let mut c = Coalescer::new(Duration::from_millis(8), DEFAULT_SIZE_CAP);
        let t0 = now();
        for _ in 0..1000 {
            c.push(t0, b"x");
        }
        let batch = c.take();
        assert_eq!(batch.len(), 1000);
        // After draining, the single window produced a single batch (1 << 1000).
        assert!(c.is_empty());
    }

    #[derive(Default)]
    struct CollectSink {
        batches: Arc<StdMutex<Vec<Vec<u8>>>>,
        closed: Arc<StdMutex<bool>>,
    }
    impl BatchSink for CollectSink {
        fn send_batch(&self, data: Vec<u8>) {
            self.batches.lock().unwrap().push(data);
        }
        fn on_closed(&self) {
            *self.closed.lock().unwrap() = true;
        }
    }

    #[test]
    fn hub_flushes_due_pane_once_per_window() {
        use super::super::reader::RawSink;
        let hub = CoalescerHub::new(Duration::from_millis(8), DEFAULT_SIZE_CAP);
        let batches = Arc::new(StdMutex::new(Vec::new()));
        let closed = Arc::new(StdMutex::new(false));
        hub.register(
            "p1".into(),
            Box::new(CollectSink {
                batches: batches.clone(),
                closed: closed.clone(),
            }),
        );

        // Many raw pushes, then a flush after the window → one batch.
        for _ in 0..50 {
            hub.on_data(&"p1".into(), b"ab");
        }
        let sent = hub.flush_due(Instant::now() + Duration::from_millis(20));
        assert_eq!(sent, 1);
        let b = batches.lock().unwrap();
        assert_eq!(b.len(), 1);
        assert_eq!(b[0].len(), 100);
    }

    #[test]
    fn hub_finalizes_pane_on_eof() {
        use super::super::reader::RawSink;
        let hub = CoalescerHub::new(Duration::from_millis(8), DEFAULT_SIZE_CAP);
        let closed = Arc::new(StdMutex::new(false));
        hub.register(
            "p1".into(),
            Box::new(CollectSink {
                batches: Arc::new(StdMutex::new(Vec::new())),
                closed: closed.clone(),
            }),
        );
        hub.on_data(&"p1".into(), b"bye");
        hub.on_eof(&"p1".into());
        // Flush after window: emits final batch AND fires on_closed.
        hub.flush_due(Instant::now() + Duration::from_millis(20));
        assert!(*closed.lock().unwrap());
    }
}
