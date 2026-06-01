//! Reader layer — bounded blocking-read pool (P3 red-team M3 + blocking-read fix).
//!
//! `portable-pty`'s reader is a blocking `Box<dyn Read>` with no timeout, so each
//! pane needs a thread parked in `read()`. The naive "one OS thread per pane
//! forever" approach explodes (100 panes = 100 threads). [`ReaderPool`] enforces
//! a **hard pane cap**: spawning beyond the cap is rejected (the caller surfaces
//! an error) rather than silently allocating unbounded threads.
//!
//! Each admitted reader loop:
//!   1. checks the pane's `pause_flag` (flow-control layer 3 — backpressure from
//!      the UI). While paused it parks briefly instead of reading, so the OS pipe
//!      fills and ConPTY/the shell throttle themselves.
//!   2. blocking-reads a chunk, pushes raw bytes into the [`RawSink`] (the
//!      coalescer). On EOF it signals `on_eof` and exits, freeing its pool slot.

use std::collections::HashSet;
use std::io::Read;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::thread;
use std::time::Duration;

use super::coalescer::PaneId;

/// Where raw (un-coalesced) reads go. Implemented by the `CoalescerHub`.
pub trait RawSink: Send + Sync {
    fn on_data(&self, pane: &PaneId, data: &[u8]);
    fn on_eof(&self, pane: &PaneId);
}

/// Per-pane pause flag toggled by `pty_pause`/`pty_resume`. When set, the reader
/// stops pulling from the PTY so backpressure propagates to the shell.
pub type PauseFlag = Arc<AtomicBool>;

/// Default max concurrent PTY readers. Beyond this, `pty_spawn` errors instead
/// of leaking threads (red-team M3). Generous for real use, bounded for safety.
pub const DEFAULT_PANE_CAP: usize = 64;

/// How long a paused reader parks before re-checking its flag. Short enough to
/// resume promptly, long enough to avoid a busy-spin.
const PAUSE_POLL: Duration = Duration::from_millis(4);
/// Read buffer per blocking read. 64 KiB matches a typical pipe buffer.
const READ_BUF: usize = 64 * 1024;

/// Admission control + live-reader accounting for the bounded reader pool.
///
/// Not a worker pool that multiplexes (portable-pty offers no non-blocking
/// poll), but a capped set of dedicated threads: the cap is the safety
/// property the spec asserts.
pub struct ReaderPool {
    cap: usize,
    /// Pane ids with a live reader thread. Guards both the count and identity
    /// (prevents double-spawn for one pane).
    active: Mutex<HashSet<PaneId>>,
    /// Mirror of `active.len()` for lock-free reads in tests/metrics.
    count: AtomicUsize,
}

impl ReaderPool {
    pub fn new(cap: usize) -> Self {
        ReaderPool {
            cap,
            active: Mutex::new(HashSet::new()),
            count: AtomicUsize::new(0),
        }
    }

    /// Current number of live reader threads.
    pub fn active_count(&self) -> usize {
        self.count.load(Ordering::SeqCst)
    }

    pub fn cap(&self) -> usize {
        self.cap
    }

    /// Try to reserve a slot for `pane`. Returns `false` if the cap is reached
    /// or the pane already has a reader. On success the caller MUST eventually
    /// cause the reader loop to run (which releases the slot on exit).
    fn try_admit(&self, pane: &PaneId) -> bool {
        let mut active = self.active.lock().unwrap();
        if active.len() >= self.cap || active.contains(pane) {
            return false;
        }
        active.insert(pane.clone());
        self.count.store(active.len(), Ordering::SeqCst);
        true
    }

    fn release(&self, pane: &PaneId) {
        let mut active = self.active.lock().unwrap();
        active.remove(pane);
        self.count.store(active.len(), Ordering::SeqCst);
    }

    /// Spawn a bounded reader thread for `pane`. Returns `Err` if the pool is at
    /// capacity (red-team M3: spawn is rejected, never unbounded).
    pub fn spawn_reader(
        self: &Arc<Self>,
        pane: PaneId,
        mut reader: Box<dyn Read + Send>,
        pause: PauseFlag,
        sink: Arc<dyn RawSink>,
    ) -> Result<(), ReaderPoolFull> {
        if !self.try_admit(&pane) {
            return Err(ReaderPoolFull {
                cap: self.cap,
                active: self.active_count(),
            });
        }
        let pool = Arc::clone(self);
        thread::Builder::new()
            .name(format!("pty-reader-{pane}"))
            .spawn(move || {
                read_loop(&pane, &mut reader, &pause, sink.as_ref());
                pool.release(&pane);
            })
            .expect("spawn pty reader thread");
        Ok(())
    }
}

/// Blocking read loop honoring the pause flag. Extracted so tests can drive it
/// with an in-memory reader (no real PTY) and a fake sink.
pub fn read_loop(
    pane: &PaneId,
    reader: &mut (dyn Read + Send),
    pause: &AtomicBool,
    sink: &dyn RawSink,
) {
    let mut buf = [0u8; READ_BUF];
    loop {
        if pause.load(Ordering::SeqCst) {
            thread::sleep(PAUSE_POLL);
            continue;
        }
        match reader.read(&mut buf) {
            Ok(0) => {
                sink.on_eof(pane);
                return;
            }
            Ok(n) => sink.on_data(pane, &buf[..n]),
            Err(ref e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(_) => {
                sink.on_eof(pane);
                return;
            }
        }
    }
}

/// Error returned when the bounded pool refuses a new reader.
#[derive(Debug, Clone)]
pub struct ReaderPoolFull {
    pub cap: usize,
    pub active: usize,
}

impl std::fmt::Display for ReaderPoolFull {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "reader pool full: {}/{} panes have live readers",
            self.active, self.cap
        )
    }
}

impl std::error::Error for ReaderPoolFull {}

/// A reader that blocks until released, used by tests to hold a pool slot open
/// the way a real idle PTY would.
pub struct BlockingReader {
    gate: Arc<(Mutex<bool>, Condvar)>,
}

impl BlockingReader {
    pub fn new() -> (Self, BlockingReaderHandle) {
        let gate = Arc::new((Mutex::new(false), Condvar::new()));
        (
            BlockingReader { gate: gate.clone() },
            BlockingReaderHandle { gate },
        )
    }
}

/// Releases a [`BlockingReader`], making its next `read` return EOF.
pub struct BlockingReaderHandle {
    gate: Arc<(Mutex<bool>, Condvar)>,
}

impl BlockingReaderHandle {
    pub fn release(&self) {
        let (lock, cvar) = &*self.gate;
        *lock.lock().unwrap() = true;
        cvar.notify_all();
    }
}

impl Read for BlockingReader {
    fn read(&mut self, _buf: &mut [u8]) -> std::io::Result<usize> {
        let (lock, cvar) = &*self.gate;
        let mut released = lock.lock().unwrap();
        while !*released {
            released = cvar.wait(released).unwrap();
        }
        Ok(0) // released → EOF
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;
    use std::sync::Mutex as StdMutex;

    #[derive(Default)]
    struct RecordingSink {
        data: StdMutex<Vec<u8>>,
        eof: AtomicBool,
    }
    impl RawSink for RecordingSink {
        fn on_data(&self, _pane: &PaneId, data: &[u8]) {
            self.data.lock().unwrap().extend_from_slice(data);
        }
        fn on_eof(&self, _pane: &PaneId) {
            self.eof.store(true, Ordering::SeqCst);
        }
    }

    #[test]
    fn read_loop_forwards_then_eofs() {
        let sink = RecordingSink::default();
        let mut reader = Cursor::new(b"hello world".to_vec());
        let pause = AtomicBool::new(false);
        read_loop(&"p".into(), &mut reader, &pause, &sink);
        assert_eq!(&*sink.data.lock().unwrap(), b"hello world");
        assert!(sink.eof.load(Ordering::SeqCst));
    }

    #[test]
    fn pause_flag_stops_enqueue_until_resumed() {
        // Reader is paused before any data is available; assert nothing flows,
        // then resume and confirm data + EOF arrive.
        let sink = Arc::new(RecordingSink::default());
        let pause: PauseFlag = Arc::new(AtomicBool::new(true));
        let (blocking, handle) = BlockingReader::new();
        // Compose: paused reader parks on the flag, not on read().
        let pane: PaneId = "p".into();
        let s = sink.clone();
        let p = pause.clone();
        let t = thread::spawn(move || {
            let mut r = blocking;
            read_loop(&pane, &mut r, &p, s.as_ref());
        });

        // Still paused → no EOF yet.
        thread::sleep(Duration::from_millis(20));
        assert!(!sink.eof.load(Ordering::SeqCst));

        // Resume: reader unparks, hits the blocking read.
        pause.store(false, Ordering::SeqCst);
        // Release the blocking read → EOF.
        handle.release();
        t.join().unwrap();
        assert!(sink.eof.load(Ordering::SeqCst));
    }

    #[test]
    fn pool_rejects_spawn_beyond_cap() {
        let pool = Arc::new(ReaderPool::new(2));
        let sink: Arc<dyn RawSink> = Arc::new(RecordingSink::default());
        let mut handles = Vec::new();

        // Fill the cap with readers parked in a blocking read.
        for i in 0..2 {
            let (reader, h) = BlockingReader::new();
            handles.push(h);
            pool.spawn_reader(
                format!("p{i}"),
                Box::new(reader),
                Arc::new(AtomicBool::new(false)),
                sink.clone(),
            )
            .expect("within cap");
        }
        assert_eq!(pool.active_count(), 2);

        // Third spawn must be rejected — cap, not unbounded threads.
        let (reader, h) = BlockingReader::new();
        handles.push(h);
        let res = pool.spawn_reader(
            "p_overflow".into(),
            Box::new(reader),
            Arc::new(AtomicBool::new(false)),
            sink.clone(),
        );
        assert!(res.is_err());
        assert_eq!(pool.active_count(), 2);

        // Release one → its slot frees up; a new spawn now succeeds.
        handles[0].release();
        // Wait for the released reader thread to exit and drop the slot.
        let mut waited = 0;
        while pool.active_count() == 2 && waited < 200 {
            thread::sleep(Duration::from_millis(5));
            waited += 1;
        }
        assert_eq!(pool.active_count(), 1);

        let (reader, h) = BlockingReader::new();
        handles.push(h);
        pool.spawn_reader(
            "p_new".into(),
            Box::new(reader),
            Arc::new(AtomicBool::new(false)),
            sink.clone(),
        )
        .expect("slot freed");

        // Release all so threads exit cleanly.
        for h in &handles {
            h.release();
        }
    }

    #[test]
    fn pool_rejects_duplicate_pane() {
        let pool = Arc::new(ReaderPool::new(8));
        let sink: Arc<dyn RawSink> = Arc::new(RecordingSink::default());
        let (r1, h1) = BlockingReader::new();
        pool.spawn_reader(
            "dup".into(),
            Box::new(r1),
            Arc::new(AtomicBool::new(false)),
            sink.clone(),
        )
        .unwrap();
        let (r2, h2) = BlockingReader::new();
        let res = pool.spawn_reader(
            "dup".into(),
            Box::new(r2),
            Arc::new(AtomicBool::new(false)),
            sink.clone(),
        );
        assert!(res.is_err());
        h1.release();
        h2.release();
    }
}
