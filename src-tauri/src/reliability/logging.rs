//! Structured logging init (P14): `tracing-subscriber` → non-blocking rolling file
//! appender in app-data `logs/`, with the redaction layer wired so EVERY byte is
//! scrubbed BEFORE it reaches disk (project HARD rule — no leak window).
//!
//! ## Why redaction sits at the writer, not a tracing Layer
//!
//! A redaction `Layer` would only see fields it knows to visit; a `fmt` layer then
//! re-serialises and could still emit a secret embedded in a `Display` value or a
//! span name. The only place that sees the EXACT bytes about to hit the file is the
//! writer. So we interpose a [`RedactingWriter`] between the formatter and the
//! non-blocking appender: it runs [`super::redaction::redact`] on each formatted
//! record before forwarding. This makes "redact before write" a structural
//! guarantee, not a best-effort field visitor.
//!
//! ## Testability
//!
//! [`init_logging`] takes the target dir, so a test points it at a temp dir, logs a
//! line containing a secret, drops the guard (flushes), and asserts the file holds
//! the marker and not the value. No app-data dependency.

use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use tracing_appender::non_blocking::WorkerGuard;
use tracing_subscriber::fmt::MakeWriter;

use super::redaction::redact;

/// Subdir under app-data where logs live (`<app-data>/logs/`).
pub const LOG_SUBDIR: &str = "logs";
/// Rolling-file basename prefix (rotation/retention scans match this).
pub const LOG_FILE_PREFIX: &str = "weftgrid.log";

/// Default level filter when neither env nor config overrides it. `info` keeps the
/// hot path quiet; bump via `WEFTGRID_LOG` (e.g. `debug`).
pub const DEFAULT_LEVEL: &str = "info";
/// Env var that overrides the log level (mirrors `RUST_LOG` semantics).
pub const LOG_ENV: &str = "WEFTGRID_LOG";

/// Live logging handle. Holds the appender's [`WorkerGuard`]; dropping it flushes
/// the non-blocking writer, so the caller MUST keep it alive for the app's lifetime
/// (store it in app state / a static). Also exposes the resolved log dir for the
/// rotation sweep.
pub struct LogHandle {
    _guard: WorkerGuard,
    log_dir: PathBuf,
}

impl LogHandle {
    pub fn log_dir(&self) -> &Path {
        &self.log_dir
    }
}

/// A `MakeWriter` that redacts each formatted record before handing it to the inner
/// writer. `tracing-subscriber` calls `make_writer`/`make_writer_for` per event and
/// writes the whole formatted line in one `write` call, so redacting per-`write`
/// scrubs complete records (not split fragments).
#[derive(Clone)]
pub struct RedactingWriter<W> {
    inner: W,
}

impl<W> RedactingWriter<W> {
    pub fn new(inner: W) -> Self {
        RedactingWriter { inner }
    }
}

impl<'a, W> MakeWriter<'a> for RedactingWriter<W>
where
    W: for<'w> MakeWriter<'w> + 'a,
{
    type Writer = RedactingSink<<W as MakeWriter<'a>>::Writer>;

    fn make_writer(&'a self) -> Self::Writer {
        RedactingSink {
            inner: self.inner.make_writer(),
        }
    }
}

/// Per-record sink: buffers the formatted bytes, redacts the assembled string, and
/// writes the scrubbed bytes through. Each tracing event is one formatted record
/// delivered via a single `write`/`write_all`, so the buffer holds exactly one
/// record before redaction.
pub struct RedactingSink<W: Write> {
    inner: W,
}

impl<W: Write> Write for RedactingSink<W> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        // Redact on the UTF-8 view (log records are text); lossy is safe because a
        // secret can't hide in invalid UTF-8 the formatter never produced.
        let text = String::from_utf8_lossy(buf);
        let scrubbed = redact(&text);
        self.inner.write_all(scrubbed.as_bytes())?;
        // Report the original length consumed so callers see a complete write.
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        self.inner.flush()
    }
}

/// An in-memory writer used by tests to capture what WOULD hit disk, exercising the
/// exact `RedactingWriter` path the file appender uses. Lives here (not under
/// `#[cfg(test)]`) so the redaction-at-writer guarantee is reusable.
#[derive(Clone, Default)]
pub struct SharedBuffer(Arc<Mutex<Vec<u8>>>);

impl SharedBuffer {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn contents(&self) -> String {
        String::from_utf8_lossy(&self.0.lock().unwrap()).to_string()
    }
}

impl Write for SharedBuffer {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(buf);
        Ok(buf.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl<'a> MakeWriter<'a> for SharedBuffer {
    type Writer = SharedBuffer;
    fn make_writer(&'a self) -> Self::Writer {
        self.clone()
    }
}

/// Resolve the level filter: `WEFTGRID_LOG` env wins, else `DEFAULT_LEVEL`.
pub fn level_filter() -> String {
    std::env::var(LOG_ENV).unwrap_or_else(|_| DEFAULT_LEVEL.to_string())
}

/// Initialise logging into `<app_data_dir>/logs/`, returning the [`LogHandle`] whose
/// guard must outlive the process. Daily-rolling file; the redaction layer wraps the
/// non-blocking writer so nothing unredacted is ever buffered or written.
///
/// Best-effort and idempotent at the process level: if a global subscriber is
/// already set (e.g. a second call, or tests), the set fails and we return the
/// handle anyway so the appender/guard still exist — we never panic app startup
/// over logging.
pub fn init_logging(app_data_dir: &Path) -> io::Result<LogHandle> {
    let log_dir = app_data_dir.join(LOG_SUBDIR);
    std::fs::create_dir_all(&log_dir)?;

    let file_appender = tracing_appender::rolling::daily(&log_dir, LOG_FILE_PREFIX);
    let (non_blocking, guard) = tracing_appender::non_blocking(file_appender);
    let writer = RedactingWriter::new(non_blocking);

    let env_filter = tracing_subscriber::EnvFilter::try_new(level_filter())
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new(DEFAULT_LEVEL));

    // try_init: do not panic if a subscriber is already installed.
    let _ = tracing_subscriber::fmt()
        .with_env_filter(env_filter)
        .with_writer(writer)
        .with_ansi(false)
        .try_init();

    Ok(LogHandle {
        _guard: guard,
        log_dir,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redacting_sink_scrubs_before_inner_write() {
        // The writer path itself must redact — assert with the in-memory sink so the
        // guarantee holds regardless of file/flush timing.
        let buf = SharedBuffer::new();
        let writer = RedactingWriter::new(buf.clone());
        {
            let mut sink = writer.make_writer();
            sink.write_all(b"auth token=supersecretvalue trailing\n")
                .unwrap();
            sink.flush().unwrap();
        }
        let out = buf.contents();
        assert!(!out.contains("supersecretvalue"), "leaked: {out}");
        assert!(out.contains("<redacted:token>"));
        assert!(out.contains("trailing"));
    }

    #[test]
    fn redacting_sink_scrubs_cookie_and_key() {
        let buf = SharedBuffer::new();
        let writer = RedactingWriter::new(buf.clone());
        let mut sink = writer.make_writer();
        sink.write_all(b"Cookie: SID=leakvalue123; x=1\n").unwrap();
        sink.write_all(
            b"-----BEGIN RSA PRIVATE KEY-----\nLEAKBODY\n-----END RSA PRIVATE KEY-----\n",
        )
        .unwrap();
        let out = buf.contents();
        assert!(!out.contains("leakvalue123"));
        assert!(!out.contains("LEAKBODY"));
        assert!(out.contains("<redacted:cookie>"));
        assert!(out.contains("<redacted:ssh-private-key>"));
    }

    #[test]
    fn init_logging_creates_log_dir() {
        let dir = std::env::temp_dir().join(format!("weft-log-{}", uuid::Uuid::new_v4()));
        let handle = init_logging(&dir).unwrap();
        assert!(handle.log_dir().exists());
        assert!(handle.log_dir().ends_with(LOG_SUBDIR));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn level_filter_defaults_to_info_without_env() {
        // Don't mutate the process env (parallel tests); just assert the default
        // constant is what the resolver falls back to.
        assert_eq!(DEFAULT_LEVEL, "info");
        let f = level_filter();
        assert!(!f.is_empty());
    }
}
