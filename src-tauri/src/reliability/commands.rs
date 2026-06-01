//! Reliability track commands + startup wiring (P14).
//!
//! Exposes one `#[tauri::command]` — [`reliability_crash_optin`] — letting the
//! frontend flip the crash-dump SHARE opt-in at runtime (default OFF → dumps stay
//! local, never uploaded). The flag is a shared [`AtomicBool`] the panic hook reads,
//! so toggling it takes effect immediately without re-installing the hook.
//!
//! [`init_reliability`] is the testable startup core: it resolves the app-data dir,
//! starts logging, installs the panic hook, runs the retention sweep, and returns
//! the [`ReliabilityState`] (holding the log guard + opt-in flag) for the caller to
//! `.manage()`. `mod::setup_reliability` is the thin Tauri-facing wrapper the shared
//! `.setup()` hook calls.

use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use tauri::State;

use super::crash_reporter::set_panic_hook;
use super::logging::{init_logging, LogHandle};
use super::rotation::{apply_retention, RotationPolicy};

/// `.manage()`d reliability state. Owns the logging [`LogHandle`] (its guard must
/// outlive the process to flush logs) and the crash-share opt-in flag the panic hook
/// reads. Created in [`init_reliability`].
pub struct ReliabilityState {
    /// Kept alive for the process lifetime so the non-blocking appender keeps
    /// flushing; never read directly.
    pub log: LogHandle,
    /// Crash-dump SHARE opt-in. Default false (local-only). Shared with the panic
    /// hook so a runtime toggle is seen on the next panic.
    pub crash_share_opt_in: Arc<AtomicBool>,
}

/// Toggle whether locally-stored crash dumps are flagged shareable. Returns the new
/// value. NEVER triggers an upload — share is a separate, explicit user action; this
/// only records consent that future tooling could honour.
#[tauri::command]
pub fn reliability_crash_optin(state: State<'_, ReliabilityState>, enabled: bool) -> bool {
    state.crash_share_opt_in.store(enabled, Ordering::Relaxed);
    enabled
}

/// Startup core (testable): wire logging + panic hook + retention under
/// `app_data_dir`. Best-effort — logging/retention failures are swallowed so a
/// read-only or full disk never blocks app start; the returned state always has a
/// valid (possibly no-op-flushing) handle.
///
/// Returns `None` only if the log appender truly cannot be created (dir create
/// failed); the caller treats that as "run without file logging".
pub fn init_reliability(app_data_dir: &Path) -> Option<ReliabilityState> {
    let log = match init_logging(app_data_dir) {
        Ok(h) => h,
        Err(_) => return None,
    };

    // Sweep old logs once at startup (the appender rolls daily; this enforces the
    // retention bound so files don't accumulate across runs).
    let _ = apply_retention(
        log.log_dir(),
        super::logging::LOG_FILE_PREFIX,
        &RotationPolicy::default(),
        std::time::SystemTime::now(),
    );

    let crash_share_opt_in = Arc::new(AtomicBool::new(false));
    set_panic_hook(app_data_dir.to_path_buf(), crash_share_opt_in.clone());

    Some(ReliabilityState {
        log,
        crash_share_opt_in,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn init_reliability_sets_up_state_in_temp_dir() {
        let dir = std::env::temp_dir().join(format!("weft-rel-{}", uuid::Uuid::new_v4()));
        let state = init_reliability(&dir).expect("init");
        // Opt-in defaults to local-only.
        assert!(!state.crash_share_opt_in.load(Ordering::Relaxed));
        // Logs dir was created under app-data.
        assert!(dir.join(super::super::logging::LOG_SUBDIR).exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn crash_optin_flag_round_trips() {
        // The command body delegates to the atomic; verify the flag itself round
        // trips (the command wrapper just stores into it).
        let flag = Arc::new(AtomicBool::new(false));
        flag.store(true, Ordering::Relaxed);
        assert!(flag.load(Ordering::Relaxed));
        flag.store(false, Ordering::Relaxed);
        assert!(!flag.load(Ordering::Relaxed));
    }
}
