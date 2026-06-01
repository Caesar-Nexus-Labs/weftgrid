//! Automation Tauri commands (P7). The agent-facing browser command surface:
//! snapshot / click / fill / eval / wait / get / find.
//!
//! ## Registration (REPORT to lead for `command_registry::register_all`)
//! Add these to the single `generate_handler!`:
//! ```text
//! automation::commands::browser_snapshot,
//! automation::commands::browser_click,
//! automation::commands::browser_fill,
//! automation::commands::browser_eval,
//! automation::commands::browser_wait,
//! automation::commands::browser_get,
//! automation::commands::browser_find,
//! ```
//!
//! ## Pane wiring (deferred to P6 / P10b)
//! Driving a real webview (`evaluate_script` + IPC round-trip) needs the P6
//! browser pane + P10b remote routing. This track owns the algorithm and the
//! command contract; until the pane manager is `.manage()`d, commands map params
//! to an [`InjectCommand`] (validated here) and return
//! [`AutomationError::PaneWiringPending`]. This is the same honest-stub pattern as
//! `vmclient` (returns `Unsupported` locally) — no fake success.

use serde::{Deserialize, Serialize};

use super::inject::InjectCommand;
use super::{GetKind, Snapshot, WaitCond};

/// Errors surfaced to the frontend. `serde` tag lets the UI switch on `kind`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "message", rename_all = "snake_case")]
pub enum AutomationError {
    /// Webview pane manager not yet wired (P6/P10b). Carries a human note.
    PaneWiringPending(String),
    /// Invalid command parameters (e.g. empty selector).
    InvalidParams(String),
    /// Inject runtime / round-trip error.
    JsError(String),
}

impl std::fmt::Display for AutomationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AutomationError::PaneWiringPending(m) => write!(f, "pane wiring pending: {m}"),
            AutomationError::InvalidParams(m) => write!(f, "invalid params: {m}"),
            AutomationError::JsError(m) => write!(f, "js error: {m}"),
        }
    }
}

const PANE_PENDING: &str = "browser pane webview wiring lands in P6/P10b";

/// Monotonic command-id source. Real correlation happens once the pane manager
/// drives `evaluate_script`; for now it makes each built [`InjectCommand`] unique.
fn next_id() -> u64 {
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(1);
    COUNTER.fetch_add(1, Ordering::Relaxed)
}

fn require_nonempty(label: &str, value: &str) -> Result<(), AutomationError> {
    if value.trim().is_empty() {
        return Err(AutomationError::InvalidParams(format!("empty {label}")));
    }
    Ok(())
}

/// Capture the AX-relevant snapshot (text + ephemeral refs `eN`) for `surface_id`.
/// Resets the ref table (re-snapshot-before-act invariant).
#[tauri::command]
pub fn browser_snapshot(surface_id: String) -> Result<Snapshot, AutomationError> {
    require_nonempty("surface_id", &surface_id)?;
    let _cmd = InjectCommand::snapshot(next_id());
    Err(AutomationError::PaneWiringPending(PANE_PENDING.into()))
}

/// Click the element behind a ref `eN` (re-snapshot first to refresh refs).
#[tauri::command]
pub fn browser_click(surface_id: String, r#ref: String) -> Result<(), AutomationError> {
    require_nonempty("surface_id", &surface_id)?;
    require_nonempty("ref", &r#ref)?;
    let _cmd = InjectCommand::click(next_id(), r#ref);
    Err(AutomationError::PaneWiringPending(PANE_PENDING.into()))
}

/// Fill a field behind a ref via the React-compatible native setter. Empty text
/// clears the field.
#[tauri::command]
pub fn browser_fill(
    surface_id: String,
    r#ref: String,
    text: String,
) -> Result<(), AutomationError> {
    require_nonempty("surface_id", &surface_id)?;
    require_nonempty("ref", &r#ref)?;
    let _cmd = InjectCommand::fill(next_id(), r#ref, text);
    Err(AutomationError::PaneWiringPending(PANE_PENDING.into()))
}

/// Evaluate JS. Linux uses `evaluate_script`; Windows may use the optional CDP
/// superset. Result is normalized by [`super::normalize_js_value`].
#[tauri::command]
pub fn browser_eval(surface_id: String, js: String) -> Result<serde_json::Value, AutomationError> {
    require_nonempty("surface_id", &surface_id)?;
    require_nonempty("js", &js)?;
    Err(AutomationError::PaneWiringPending(PANE_PENDING.into()))
}

/// Wait until a predicate holds (selector / url / text / load-state / function).
#[tauri::command]
pub fn browser_wait(surface_id: String, cond: WaitCond) -> Result<bool, AutomationError> {
    require_nonempty("surface_id", &surface_id)?;
    let _cmd = InjectCommand::wait(next_id(), cond);
    Err(AutomationError::PaneWiringPending(PANE_PENDING.into()))
}

/// Read a property off the element behind a ref (`text|html|value|attr|box|styles`).
#[tauri::command]
pub fn browser_get(
    surface_id: String,
    r#ref: String,
    kind: GetKind,
    attr: Option<String>,
) -> Result<serde_json::Value, AutomationError> {
    require_nonempty("surface_id", &surface_id)?;
    require_nonempty("ref", &r#ref)?;
    let _cmd = InjectCommand::get(next_id(), r#ref, kind, attr);
    Err(AutomationError::PaneWiringPending(PANE_PENDING.into()))
}

/// Resolve a raw CSS selector to a fresh ref `eN` (fully-qualified `:nth-of-type`
/// path). Extends the live ref table without resetting it.
#[tauri::command]
pub fn browser_find(surface_id: String, selector: String) -> Result<String, AutomationError> {
    require_nonempty("surface_id", &surface_id)?;
    require_nonempty("selector", &selector)?;
    let _cmd = InjectCommand::find(next_id(), selector);
    Err(AutomationError::PaneWiringPending(PANE_PENDING.into()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_surface_id_is_invalid_params() {
        let err = browser_snapshot("  ".to_string()).unwrap_err();
        assert!(matches!(err, AutomationError::InvalidParams(_)));
    }

    #[test]
    fn valid_params_report_pane_pending() {
        let err = browser_click("surface:1".into(), "e1".into()).unwrap_err();
        assert!(matches!(err, AutomationError::PaneWiringPending(_)));
    }

    #[test]
    fn fill_validates_ref_before_pending() {
        let err = browser_fill("surface:1".into(), "".into(), "x".into()).unwrap_err();
        assert!(matches!(err, AutomationError::InvalidParams(_)));
    }

    #[test]
    fn get_accepts_box_kind() {
        let err = browser_get("surface:1".into(), "e1".into(), GetKind::Box, None).unwrap_err();
        assert!(matches!(err, AutomationError::PaneWiringPending(_)));
    }

    #[test]
    fn wait_accepts_selector_cond() {
        let err = browser_wait("surface:1".into(), WaitCond::Selector("#x".into())).unwrap_err();
        assert!(matches!(err, AutomationError::PaneWiringPending(_)));
    }

    #[test]
    fn error_serializes_with_kind_tag() {
        let v = serde_json::to_value(AutomationError::InvalidParams("empty ref".into())).unwrap();
        assert_eq!(v["kind"], "invalid_params");
        assert_eq!(v["message"], "empty ref");
    }

    #[test]
    fn ids_are_monotonic() {
        let a = next_id();
        let b = next_id();
        assert!(b > a);
    }
}
