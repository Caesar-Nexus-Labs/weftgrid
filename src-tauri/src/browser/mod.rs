//! Browser pane (overlay window) track (P6 owner: `src-tauri/src/browser/**`).
//!
//! In-app browser pane = overlay child `WebviewWindow` positioned over a
//! browser-anchor leaf in the split-tree. Factory takes creation-time params
//! `{cdp_port, proxy_url, profile_dir}` (wry can't change these post-init), with a
//! recreate-with-state-transfer pathway when they must change. Bounds math is
//! physical-coords end-to-end (multi-monitor dual-DPI correct).
//!
//! ## Module map
//! - `overlay_bounds` (`overlay-bounds-physical.rs`) — pure physical-coords math.
//! - `overlay_manager` (`overlay-manager.rs`) — creation-time-param factory +
//!   lifecycle + paneId->window map, lifecycle logic behind a [`WindowSpawner`] seam.
//! - `overlay_recreate` (`overlay-recreate-state-transfer.rs`) — recreate pathway.
//! - `tauri_spawner` (`tauri-window-spawner.rs`) — production `WindowSpawner` seam.
//! - `commands` — Tauri commands (open/navigate/close/sync_bounds/recreate).
//!
//! `register` is additive-only. Startup work that needs the concrete runtime +
//! `AppHandle` (building the overlay manager and `.manage()`ing it) lives in
//! [`setup_overlay`], which `command_registry` calls from the SINGLE shared
//! `.setup()` hook — `Builder::setup` is last-wins (like `invoke_handler`), so
//! tracks must NOT each call it. Commands are listed once in `command_registry`.

use tauri::{App, Builder, Manager, Runtime};

#[path = "overlay-bounds-physical.rs"]
pub mod overlay_bounds;
#[path = "overlay-manager.rs"]
pub mod overlay_manager;
#[path = "overlay-recreate-state-transfer.rs"]
pub mod overlay_recreate;
#[path = "tauri-window-spawner.rs"]
pub mod tauri_spawner;

pub mod commands;

use commands::BrowserState;
use overlay_manager::OverlayManager;
use tauri_spawner::TauriWindowSpawner;

/// Production overlay manager: lifecycle logic over the Tauri-backed window seam,
/// parameterized by the app's runtime.
pub type ProdOverlayManager<R> = OverlayManager<TauriWindowSpawner<R>>;

/// Additive-only fold. The overlay manager is built in [`setup_overlay`] instead,
/// because `Builder::setup` is last-wins — `command_registry` owns the one shared
/// setup hook and calls each track's setup function from it.
pub fn register<R: Runtime>(builder: Builder<R>) -> Builder<R> {
    builder
}

/// Build the overlay manager (production spawner) and `.manage()` it. Called from
/// the shared `.setup()` hook where the concrete runtime + `AppHandle` exist, so
/// the managed state is a concrete (monomorphized) type per runtime.
pub fn setup_overlay<R: Runtime>(app: &mut App<R>) {
    let spawner = TauriWindowSpawner::new(app.handle().clone());
    let manager: ProdOverlayManager<R> = OverlayManager::new(spawner);
    app.manage(BrowserState::new(manager));
}
