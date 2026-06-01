//! Overlay manager — creation-time-param factory + lifecycle (P6 red-team C3).
//!
//! A browser pane is one borderless child `WebviewWindow` owned by the main
//! window, positioned over a browser-anchor leaf. wry bakes three params at webview
//! creation that CANNOT change afterwards: the CDP port (`additional_browser_args`),
//! the proxy (`proxy_url`), and the per-profile data dir (`data_directory`). So the
//! factory takes all three up front; "turn automation/proxy/profile on later" is
//! the recreate-with-state-transfer pathway, never a mutation (see
//! `overlay-recreate-state-transfer.rs`).
//!
//! ## Testable core vs. window seam
//!
//! Building a real `WebviewWindow` needs a running app, so the LOGIC that this
//! phase can verify — param plumbing, the CDP-args string (which must re-include
//! wry's defaults because `additional_browser_args` OVERRIDES them), and the
//! paneId->label bookkeeping map — lives here and is unit-tested directly. The one
//! call that actually touches a window is isolated behind the [`WindowSpawner`]
//! trait so the manager is exercisable with a fake spawner in tests.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Mutex;

use crate::model::PaneId;

use super::overlay_bounds::PhysicalRect;

/// wry's default WebView2 args. `additional_browser_args` OVERRIDES the entire
/// default string, so when we append a CDP port we must re-include these or we
/// silently re-enable the mini-menu / PDF OOUI / SmartScreen surfaces wry removes.
/// Mirrors `wry::webview2` `default_args` exactly.
pub const WRY_DEFAULT_DISABLE_FEATURES: &str =
    "--disable-features=msWebOOUI,msPdfOOUI,msSmartScreenProtection";
/// wry's autoplay flag (added when autoplay is enabled). We re-include it so an
/// overlay with automation behaves like wry's normal webview.
pub const WRY_AUTOPLAY_FLAG: &str = "--autoplay-policy=no-user-gesture-required";

/// The three creation-time params that cannot change after the webview is built.
/// `proxy_url` is kept as a `String` (parsed to `tauri::Url` only at the window
/// seam) so this struct — and its tests — need no `url` crate dependency.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OverlayCreateParams {
    pub pane_id: PaneId,
    /// `Some(port)` enables CDP via `additional_browser_args`. `0` = ephemeral port
    /// (discover the bound port over `/json/version`, loopback-only — security).
    pub cdp_port: Option<u16>,
    /// `Some(url)` sets the webview proxy (socks5h/http). Validated/parsed at the seam.
    pub proxy_url: Option<String>,
    /// Per-profile WebView2 user-data-folder (cookie isolation; P11 seeds it).
    pub profile_dir: PathBuf,
}

impl OverlayCreateParams {
    /// Stable window label for this pane's overlay (`browser-{paneId}`).
    pub fn window_label(&self) -> String {
        format!("browser-{}", self.pane_id)
    }

    /// Build the `additional_browser_args` string for `with_additional_browser_args`.
    /// Returns `None` when CDP is off (then we don't call the override at all, so
    /// wry keeps its own defaults). When CDP is on we MUST re-include the defaults
    /// because the override replaces them wholesale.
    pub fn cdp_args(&self) -> Option<String> {
        let port = self.cdp_port?;
        Some(format!(
            "--remote-debugging-port={port} {WRY_DEFAULT_DISABLE_FEATURES} {WRY_AUTOPLAY_FLAG}"
        ))
    }
}

/// Live-window operations the manager needs. Implemented for real by Tauri at
/// integration; faked in tests so lifecycle bookkeeping is verifiable without a
/// running app. Methods return `Result<(), String>` to cross IPC as rejected
/// promises uniformly.
pub trait WindowSpawner {
    /// Create the borderless overlay window from these params, navigate to
    /// `initial_url` if given. Returns the created window's label on success.
    fn spawn(&self, params: &OverlayCreateParams, initial_url: Option<&str>)
        -> Result<String, String>;
    /// Move/resize an existing overlay to a PHYSICAL rect (physical setters).
    fn set_bounds(&self, label: &str, rect: PhysicalRect) -> Result<(), String>;
    /// Navigate an existing overlay.
    fn navigate(&self, label: &str, url: &str) -> Result<(), String>;
    /// Show / hide (visibility + occlusion handling clips to hide).
    fn set_visible(&self, label: &str, visible: bool) -> Result<(), String>;
    /// Raise/lower relative z-order (overlay on top only while its pane is active).
    fn set_on_top(&self, label: &str, on_top: bool) -> Result<(), String>;
    /// Destroy the overlay window.
    fn destroy(&self, label: &str) -> Result<(), String>;
    /// Restore scroll position after a recreate navigates the fresh overlay
    /// (seam evals `window.scrollTo` once the page loads). No-op default for
    /// spawners that don't support it.
    fn restore_scroll(&self, _label: &str, _x: f64, _y: f64) -> Result<(), String> {
        Ok(())
    }
}

/// Per-overlay bookkeeping the manager tracks alongside the live window.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OverlayEntry {
    pub label: String,
    pub params: OverlayCreateParams,
    /// Last URL we navigated to (seeds state-transfer on recreate).
    pub current_url: Option<String>,
    pub visible: bool,
}

/// paneId -> overlay map + the window seam. `S` is the spawner so production wires
/// a Tauri-backed impl while tests wire a fake.
pub struct OverlayManager<S: WindowSpawner> {
    spawner: S,
    overlays: Mutex<HashMap<PaneId, OverlayEntry>>,
}

impl<S: WindowSpawner> OverlayManager<S> {
    pub fn new(spawner: S) -> Self {
        Self {
            spawner,
            overlays: Mutex::new(HashMap::new()),
        }
    }

    /// Create an overlay for `params.pane_id`, optionally navigating to `url`.
    /// Idempotent-guarded: refuses a duplicate for a pane that already has one
    /// (callers recreate via the state-transfer pathway, not double-create).
    pub fn create(
        &self,
        params: OverlayCreateParams,
        url: Option<String>,
    ) -> Result<String, String> {
        let pane = params.pane_id;
        {
            let map = self.overlays.lock().unwrap();
            if map.contains_key(&pane) {
                return Err(format!("overlay already exists for pane {pane}"));
            }
        }
        let label = self.spawner.spawn(&params, url.as_deref())?;
        let entry = OverlayEntry {
            label: label.clone(),
            params,
            current_url: url,
            visible: true,
        };
        self.overlays.lock().unwrap().insert(pane, entry);
        Ok(label)
    }

    /// Reposition an overlay to a freshly computed physical rect.
    pub fn position(&self, pane: &PaneId, rect: PhysicalRect) -> Result<(), String> {
        let label = self.label_of(pane)?;
        self.spawner.set_bounds(&label, rect)
    }

    /// Navigate an overlay and remember the URL (so recreate can restore it).
    pub fn navigate(&self, pane: &PaneId, url: &str) -> Result<(), String> {
        let label = self.label_of(pane)?;
        self.spawner.navigate(&label, url)?;
        if let Some(entry) = self.overlays.lock().unwrap().get_mut(pane) {
            entry.current_url = Some(url.to_string());
        }
        Ok(())
    }

    /// Show/hide (mode #2/#3: clip-to-hide on occlusion or scroll-out).
    pub fn set_visible(&self, pane: &PaneId, visible: bool) -> Result<(), String> {
        let label = self.label_of(pane)?;
        self.spawner.set_visible(&label, visible)?;
        if let Some(entry) = self.overlays.lock().unwrap().get_mut(pane) {
            entry.visible = visible;
        }
        Ok(())
    }

    /// Raise/lower z-order (mode #1: overlay on top only while pane active).
    pub fn set_on_top(&self, pane: &PaneId, on_top: bool) -> Result<(), String> {
        let label = self.label_of(pane)?;
        self.spawner.set_on_top(&label, on_top)
    }

    /// Destroy and forget an overlay.
    pub fn destroy(&self, pane: &PaneId) -> Result<(), String> {
        let entry = self
            .overlays
            .lock()
            .unwrap()
            .remove(pane)
            .ok_or_else(|| format!("no overlay for pane {pane}"))?;
        self.spawner.destroy(&entry.label)
    }

    /// Forget an overlay's bookkeeping WITHOUT calling the window seam. The
    /// recreate pathway uses this: it destroys the old window via the captured
    /// label itself, then drops the stale entry so a same-pane `create` doesn't
    /// trip the duplicate guard. Returns whether an entry was present.
    pub fn destroy_silent(&self, pane: &PaneId) -> bool {
        self.overlays.lock().unwrap().remove(pane).is_some()
    }

    /// Snapshot an overlay's bookkeeping entry (used by the recreate pathway to
    /// capture creation params + current URL before swapping).
    pub fn entry(&self, pane: &PaneId) -> Option<OverlayEntry> {
        self.overlays.lock().unwrap().get(pane).cloned()
    }

    /// Number of live overlays (mode #9 event-storm / N-overlay reasoning).
    pub fn len(&self) -> usize {
        self.overlays.lock().unwrap().len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    fn label_of(&self, pane: &PaneId) -> Result<String, String> {
        self.overlays
            .lock()
            .unwrap()
            .get(pane)
            .map(|e| e.label.clone())
            .ok_or_else(|| format!("no overlay for pane {pane}"))
    }

    /// Borrow the spawner (recreate pathway drives lifecycle ops through it).
    pub fn spawner(&self) -> &S {
        &self.spawner
    }
}

#[cfg(test)]
#[path = "overlay-manager.test.rs"]
mod tests;
