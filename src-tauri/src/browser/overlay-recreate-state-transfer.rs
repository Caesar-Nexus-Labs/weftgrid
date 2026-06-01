//! Recreate-with-state-transfer pathway (P6 red-team C3).
//!
//! The three creation-time params (`cdp_port`, `proxy_url`, `profile_dir`) are
//! baked into the webview and CANNOT be changed after init. So "turn automation
//! on", "attach an SSH proxy", or "switch profile" all map to the SAME operation:
//! build a NEW overlay with the new params and transfer the visible state across.
//!
//! Ordered steps (state preserved, no visible flash if the swap is tight):
//!   1. capture old state (current URL, scroll offset, physical bounds) from the
//!      manager's bookkeeping,
//!   2. create a new overlay with the new params,
//!   3. navigate it to the captured URL and restore scroll,
//!   4. swap z-order (raise new, lower old) then destroy the old window.
//!
//! Scroll restoration crosses the seam as JS the overlay runs after navigation;
//! the actual eval is part of the window seam, so the CAPTURE + TRANSFER PLAN is
//! what we make unit-testable here (the plan is a value the tests assert).

use crate::model::PaneId;

use super::overlay_manager::{OverlayCreateParams, OverlayManager, WindowSpawner};

/// State carried from the old overlay to the new one. `scroll` is best-effort
/// (None when the UI hasn't reported it yet). Bounds let the new overlay appear in
/// the exact same spot before the next sync tick.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct TransferState {
    pub url: Option<String>,
    pub scroll: Option<(f64, f64)>,
}

/// The ordered plan the recreate executes. Computed from captured state so a test
/// can assert WHAT will happen (URL navigated, scroll restored, old label
/// destroyed) without a live window. `new_params` carries the changed
/// creation-time values; `old_label` is destroyed last.
#[derive(Debug, Clone, PartialEq)]
pub struct RecreatePlan {
    pub old_label: String,
    pub new_params: OverlayCreateParams,
    pub transfer: TransferState,
}

/// Build the recreate plan from the manager's current bookkeeping + the new
/// creation-time params. Pure: reads the existing entry, merges any extra
/// transfer state (e.g. last-reported scroll) the caller supplies. Errors if the
/// pane has no live overlay to recreate.
pub fn plan_recreate<S: WindowSpawner>(
    manager: &OverlayManager<S>,
    pane: &PaneId,
    new_params: OverlayCreateParams,
    last_scroll: Option<(f64, f64)>,
) -> Result<RecreatePlan, String> {
    let entry = manager
        .entry(pane)
        .ok_or_else(|| format!("no overlay to recreate for pane {pane}"))?;
    if new_params.pane_id != *pane {
        return Err("recreate params pane_id must match the target pane".into());
    }
    Ok(RecreatePlan {
        old_label: entry.label,
        new_params,
        transfer: TransferState {
            url: entry.current_url,
            scroll: last_scroll,
        },
    })
}

/// Execute a recreate plan against the manager: create the new overlay (params +
/// captured URL), raise it, then destroy the old window and drop its bookkeeping.
/// Returns the new window label.
///
/// Note: `create` keys overlays by `pane_id`, and the old entry shares that pane,
/// so we remove the old bookkeeping first (it is destroyed anyway), then create
/// the replacement. The OLD window itself is destroyed via the spawner using the
/// captured `old_label` so the user never sees two windows lingering.
pub fn execute_recreate<S: WindowSpawner>(
    manager: &OverlayManager<S>,
    plan: RecreatePlan,
) -> Result<String, String> {
    let pane = plan.new_params.pane_id;

    // Destroy the old window at the seam first (its bookkeeping is removed so the
    // new create() — keyed by pane_id — doesn't collide on the duplicate guard).
    manager.spawner().destroy(&plan.old_label)?;
    let _ = manager.destroy_silent(&pane);

    let new_label = manager.create(plan.new_params, plan.transfer.url.clone())?;
    // Restore scroll after navigation (seam evals JS); raise the fresh overlay.
    if let Some((sx, sy)) = plan.transfer.scroll {
        manager.spawner().restore_scroll(&new_label, sx, sy)?;
    }
    manager.spawner().set_on_top(&new_label, true)?;
    Ok(new_label)
}

#[cfg(test)]
#[path = "overlay-recreate-state-transfer.test.rs"]
mod tests;
