//! Crash reporter (P14): a panic hook that writes a LOCAL, PII/secret-scrubbed dump
//! to app-data `crashes/`. Opt-in share only — there is NO auto-upload (respect the
//! user's/project's anonymity). The dump-building + scrub is pure and unit-tested;
//! [`set_panic_hook`] is the thin seam that installs it.
//!
//! Scrub passes, layered on top of the log redactor:
//!   - secret values → [`super::redaction::redact`] (cookie/token/key/password).
//!   - the OS username inside absolute paths → `<user>` (so `C:\Users\alice\...` or
//!     `/home/alice/...` shared in a bug report doesn't dox the user).
//!
//! The dump is plain text (panic message + location + scrubbed thread name). No
//! backtrace symbol capture here — symbol resolution can itself surface paths and
//! is platform-fiddly; the message + location is the minimum viable, anonymity-safe
//! dump for MVP.

use std::io;
use std::panic::PanicHookInfo;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use super::redaction::redact;

/// Subdir under app-data for crash dumps (`<app-data>/crashes/`).
pub const CRASH_SUBDIR: &str = "crashes";

/// A scrubbed crash dump ready to persist or (opt-in) share. `body` is fully
/// scrubbed text; `share_opt_in` mirrors the user's current choice (default false
/// → local-only).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CrashDump {
    pub body: String,
    pub share_opt_in: bool,
}

/// Build a scrubbed dump from raw panic fields. Pure: no I/O, no global state, so a
/// test feeds a payload full of secrets/PII and asserts none survive. `location` is
/// `file:line` (already non-secret source paths) and is passed through verbatim;
/// the `message` and `thread` are scrubbed because they can contain runtime values.
pub fn build_dump(message: &str, location: &str, thread: &str, share_opt_in: bool) -> CrashDump {
    let scrubbed_msg = scrub(message);
    let scrubbed_thread = scrub(thread);
    let body = format!(
        "weftgrid crash dump\nthread: {scrubbed_thread}\nlocation: {location}\nmessage: {scrubbed_msg}\n"
    );
    CrashDump {
        body,
        share_opt_in,
    }
}

/// Full scrub for crash text: redact secrets, then anonymise usernames in paths.
pub fn scrub(input: &str) -> String {
    scrub_user_paths(&redact(input))
}

/// Replace the username segment of common home-dir paths with `<user>`. Handles
/// `C:\Users\NAME\...`, `/home/NAME/...`, and `/Users/NAME/...` (macOS) without a
/// regex dep — a small left-to-right scan for each known prefix.
fn scrub_user_paths(input: &str) -> String {
    let mut out = input.to_string();
    out = replace_segment_after(&out, "\\Users\\");
    out = replace_segment_after(&out, "/Users/");
    out = replace_segment_after(&out, "/home/");
    out
}

/// For every occurrence of `prefix`, replace the path SEGMENT immediately following
/// it (up to the next separator or whitespace) with `<user>`. Case-insensitive on
/// the prefix so `\users\` / `\Users\` both match on Windows.
fn replace_segment_after(input: &str, prefix: &str) -> String {
    let lower_in = input.to_ascii_lowercase();
    let lower_prefix = prefix.to_ascii_lowercase();
    let mut out = String::with_capacity(input.len());
    let mut search_from = 0;
    let mut copied = 0;
    while let Some(rel) = lower_in[search_from..].find(&lower_prefix) {
        let prefix_start = search_from + rel;
        let seg_start = prefix_start + prefix.len();
        // Copy everything up to and including the prefix.
        out.push_str(&input[copied..seg_start]);
        // Find the end of the username segment.
        let bytes = input.as_bytes();
        let mut j = seg_start;
        while j < bytes.len() {
            match bytes[j] {
                b'\\' | b'/' | b' ' | b'\t' | b'\r' | b'\n' | b'"' | b'\'' => break,
                _ => j += 1,
            }
        }
        if j > seg_start {
            out.push_str("<user>");
        }
        copied = j;
        search_from = j.max(prefix_start + prefix.len());
    }
    out.push_str(&input[copied..]);
    out
}

/// Full path of the crash-dumps dir under `app_data_dir`.
pub fn crash_dir(app_data_dir: &Path) -> PathBuf {
    app_data_dir.join(CRASH_SUBDIR)
}

/// Persist a dump to `<crash_dir>/crash-<id>.txt`. Best-effort; returns the path
/// written. Creates the dir if missing. NEVER uploads — share is a separate,
/// explicit user action elsewhere.
pub fn write_dump(crash_dir: &Path, id: &str, dump: &CrashDump) -> io::Result<PathBuf> {
    std::fs::create_dir_all(crash_dir)?;
    let path = crash_dir.join(format!("crash-{id}.txt"));
    std::fs::write(&path, dump.body.as_bytes())?;
    Ok(path)
}

/// Extract the panic message + location into the scrubbed-dump inputs. Pulled out
/// of the hook so the field-extraction is testable with a synthesised `PanicHookInfo`
/// is awkward — instead the hook calls this with already-extracted strings.
pub fn dump_from_parts(payload: &str, location: Option<String>, share_opt_in: bool) -> CrashDump {
    let thread = std::thread::current()
        .name()
        .unwrap_or("unnamed")
        .to_string();
    let loc = location.unwrap_or_else(|| "unknown".to_string());
    build_dump(payload, &loc, &thread, share_opt_in)
}

/// LIVE SEAM (real-desktop): install the process panic hook. It builds a scrubbed
/// dump and writes it under `app_data_dir/crashes/`, then chains to the previous
/// hook so the default abort/print behaviour still runs. `share_opt_in` is a shared
/// flag the [`commands`](super::commands) toggle flips at runtime — the persisted
/// dump records the current choice; nothing is uploaded here.
///
/// The body is intentionally tiny — all logic it needs is the pure
/// [`dump_from_parts`]/[`write_dump`] above — so installing it is a thin, low-risk
/// wiring step. Unit-testing a real `std::panic` is process-global and flaky, so the
/// hook is verified indirectly via the pure functions; the actual install is
/// accepted at the real-desktop session.
pub fn set_panic_hook(app_data_dir: PathBuf, share_opt_in: Arc<AtomicBool>) {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info: &PanicHookInfo| {
        let payload = panic_payload_str(info);
        let location = info.location().map(|l| format!("{}:{}", l.file(), l.line()));
        let dump = dump_from_parts(&payload, location, share_opt_in.load(Ordering::Relaxed));
        let id = uuid::Uuid::new_v4().simple().to_string();
        let _ = write_dump(&crash_dir(&app_data_dir), &id, &dump);
        previous(info);
    }));
}

/// Best-effort extraction of a panic payload as a string (`&str` or `String`).
fn panic_payload_str(info: &PanicHookInfo) -> String {
    if let Some(s) = info.payload().downcast_ref::<&str>() {
        (*s).to_string()
    } else if let Some(s) = info.payload().downcast_ref::<String>() {
        s.clone()
    } else {
        "non-string panic payload".to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dump_redacts_secret_in_panic_message() {
        let dump = build_dump(
            "connection failed with token=deadbeefsecret while dialing",
            "src/net.rs:42",
            "main",
            false,
        );
        assert!(!dump.body.contains("deadbeefsecret"), "leaked: {}", dump.body);
        assert!(dump.body.contains("<redacted:token>"));
        assert!(dump.body.contains("src/net.rs:42"));
    }

    #[test]
    fn dump_anonymises_windows_user_path() {
        let dump = build_dump(
            r"failed to read C:\Users\alice\AppData\weftgrid\session.json",
            "src/store.rs:10",
            "worker",
            false,
        );
        assert!(!dump.body.contains("alice"), "leaked username: {}", dump.body);
        assert!(dump.body.contains(r"C:\Users\<user>\AppData"));
    }

    #[test]
    fn dump_anonymises_unix_home_path() {
        let s = scrub("panic at /home/bob/.config/weftgrid and /Users/carol/Library/x");
        assert!(!s.contains("bob"));
        assert!(!s.contains("carol"));
        assert!(s.contains("/home/<user>/.config"));
        assert!(s.contains("/Users/<user>/Library"));
    }

    #[test]
    fn share_defaults_to_local_only() {
        let dump = build_dump("boom", "src/x.rs:1", "main", false);
        assert!(!dump.share_opt_in, "must be opt-in (no auto-share)");
    }

    #[test]
    fn write_dump_persists_to_crashes_dir() {
        let app_data = std::env::temp_dir().join(format!("weft-crash-{}", uuid::Uuid::new_v4()));
        let dir = crash_dir(&app_data);
        let dump = build_dump("panic with password=hunter2", "src/x.rs:5", "main", false);
        let path = write_dump(&dir, "abc123", &dump).unwrap();
        assert!(path.exists());
        let written = std::fs::read_to_string(&path).unwrap();
        assert!(!written.contains("hunter2"), "leaked to disk: {written}");
        assert!(written.contains("<redacted:password>"));
        let _ = std::fs::remove_dir_all(&app_data);
    }

    #[test]
    fn scrub_handles_path_with_no_trailing_segment() {
        // A bare prefix at end of string must not panic / over-replace.
        let s = scrub(r"tail C:\Users\");
        assert!(s.contains(r"C:\Users\"));
    }

    #[test]
    fn dump_from_parts_uses_current_thread_and_unknown_location() {
        let dump = dump_from_parts("kaboom token=xyz", None, false);
        assert!(dump.body.contains("location: unknown"));
        assert!(!dump.body.contains("xyz"));
    }
}
