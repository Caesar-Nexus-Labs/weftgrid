//! Reliability track (P14 owner: `src-tauri/src/reliability/**`).
//!
//! Cross-cutting recovery + diagnosability layer: structured leveled logging with
//! secret redaction + rotation, a panic/crash reporter (opt-in, PII-scrubbed), and
//! recovery logic for the fragile subsystems the plan enumerates — WebView2
//! `ProcessFailed`, overlay-window crash, CDP socket loss (backoff reconnect), and
//! PTY unexpected death.
//!
//! ## Testable core vs. live seam
//!
//! This is a HEADLESS build env (no webview, `tauri`'s `test` feature can't link),
//! so the phase splits each subsystem into PURE logic (unit-tested here) and a thin
//! LIVE seam (accepted at a real-desktop session):
//!   - [`redaction`] — secret-pattern strip (pure, broad test coverage).
//!   - [`logging`] — `tracing-subscriber` + non-blocking file appender, redaction
//!     wired at the WRITER so nothing unredacted is ever buffered/written.
//!   - [`rotation`] — size/age rotation + N-file/N-day retention policy (pure).
//!   - [`recovery`] — idempotency guard + exponential backoff + max-retry/cooldown +
//!     PTY-death classification (pure state machines).
//!   - [`crash_reporter`] — PII/secret-scrubbed local dump, opt-in share, no upload
//!     (pure scrub; `set_panic_hook` is the thin seam).
//!   - [`webview_recovery`] / [`overlay_recovery`] / [`cdp_reconnect`] /
//!     [`pty_watchdog`] — DECISION logic is pure + tested; each exposes a trait seam
//!     the live `AppHandle`/webview wiring (P6/P7/P3/P12) plugs into. The OS-event
//!     registration itself is LIVE-WIRED WAVE-DEFERRED.
//!
//! `register` is additive-only. Startup work that needs the concrete runtime
//! (install the tracing subscriber + panic hook, `.manage()` the state) lives in
//! [`setup_reliability`], called from the SINGLE shared `.setup()` hook in
//! `command_registry` — tracks must NOT each call `Builder::setup` (last-wins drops
//! siblings). Commands are listed once in `command_registry`.

use tauri::{App, Builder, Manager, Runtime};

pub mod cdp_reconnect;
pub mod commands;
pub mod crash_reporter;
pub mod logging;
pub mod overlay_recovery;
pub mod pty_watchdog;
pub mod recovery;
pub mod redaction;
pub mod rotation;
pub mod webview_recovery;

/// Additive-only fold. Logging/panic-hook init + state management happen in
/// [`setup_reliability`] instead, because `Builder::setup` is last-wins —
/// `command_registry` owns the one shared setup hook and calls each track's setup
/// function from it.
pub fn register<R: Runtime>(builder: Builder<R>) -> Builder<R> {
    builder
}

/// Initialise the reliability layer at startup and `.manage()` its state. Called
/// from the shared `.setup()` hook (where the concrete runtime + `AppHandle` exist),
/// alongside `browser::setup_overlay(app)` and `agent_rpc::setup_rpc_server()`.
///
/// Best-effort: resolves the OS app-data dir (same vendor dir as the config/RPC
/// stores), starts file logging, installs the panic hook, and sweeps stale logs.
/// Any failure is swallowed (logged to stderr) so logging never blocks app start;
/// when state is created it is managed so [`commands::reliability_crash_optin`] can
/// resolve it.
pub fn setup_reliability<R: Runtime>(app: &mut App<R>) {
    let dir = match app_data_dir() {
        Some(d) => d,
        None => {
            eprintln!("[reliability] could not resolve app-data dir; running without file logging");
            return;
        }
    };
    match commands::init_reliability(&dir) {
        Some(state) => {
            app.manage(state);
        }
        None => eprintln!("[reliability] file logging unavailable; continuing without it"),
    }
}

/// Resolve the weftgrid app-data dir (`%APPDATA%\weftgrid\` on Windows,
/// `~/.local/share/weftgrid/` on Linux). Mirrors the config/RPC stores' qualifier so
/// logs + crashes land under the same vendor dir P9 packages.
fn app_data_dir() -> Option<std::path::PathBuf> {
    directories::ProjectDirs::from("", "", "weftgrid").map(|p| p.data_dir().to_path_buf())
}
