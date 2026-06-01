//! Browser overlay Tauri commands (P6).
//!
//! Thin glue between the frontend and the overlay manager. The managed state is
//! [`BrowserState`] holding an [`OverlayManager`] backed by the production
//! [`TauriWindowSpawner`]; it is created in `register()`'s `setup` hook where the
//! concrete runtime is known. Commands are generic over the runtime `R` so they
//! resolve the same concrete `BrowserState<R>` Tauri monomorphizes in
//! `generate_handler!`.
//!
//! Commands to register in `command_registry::register_all`'s `generate_handler!`:
//!   browser::commands::browser_open,
//!   browser::commands::browser_navigate,
//!   browser::commands::browser_close,
//!   browser::commands::browser_sync_bounds,
//!   browser::commands::browser_recreate.
//!
//! Bounds sync is physical-coords end-to-end: the frontend reports the anchor rect
//! (CSS px) + scroll + the main window's outer/inner positions + scale (it reads
//! those via the Tauri window API), and [`overlay_bounds::overlay_physical_rect`]
//! turns them into the physical rect the overlay setters consume. Keeping the math
//! server-side means one tested implementation, not a JS reimplementation.

use std::path::PathBuf;

use serde::Deserialize;
use tauri::{Runtime, State};

use super::overlay_bounds::{overlay_physical_rect, BoundsInput, CssRect, PhysicalPoint};
use super::overlay_manager::OverlayCreateParams;
use super::overlay_recreate::{execute_recreate, plan_recreate};
use super::ProdOverlayManager;
use crate::model::PaneId;

/// `.manage()`d browser state: the overlay manager over the production spawner.
/// Generic over the runtime so the managed value is a concrete monomorphization.
pub struct BrowserState<R: Runtime> {
    pub manager: ProdOverlayManager<R>,
}

impl<R: Runtime> BrowserState<R> {
    pub fn new(manager: ProdOverlayManager<R>) -> Self {
        Self { manager }
    }
}

/// Creation-time params as they cross IPC (camelCase). `proxy_url` stays a String;
/// the spawner parses it. `profile_dir` is an absolute path the profile track
/// (P11) provisions.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OpenParams {
    pub pane_id: PaneId,
    pub url: Option<String>,
    pub cdp_port: Option<u16>,
    pub proxy_url: Option<String>,
    pub profile_dir: String,
}

impl OpenParams {
    fn into_create_params(self) -> (OverlayCreateParams, Option<String>) {
        (
            OverlayCreateParams {
                pane_id: self.pane_id,
                cdp_port: self.cdp_port,
                proxy_url: self.proxy_url,
                profile_dir: PathBuf::from(self.profile_dir),
            },
            self.url,
        )
    }
}

/// Open a browser pane: create the overlay with creation-time params and navigate.
/// Returns the overlay window label.
///
/// App commands pin the concrete `Wry` runtime (generic `<R: Runtime>` is the
/// plugin pattern — `generate_handler!` can't infer `R` for an app handler). The
/// managed `BrowserState<R>` resolves to `BrowserState<Wry>` at runtime.
#[tauri::command]
pub fn browser_open(
    state: State<'_, BrowserState<tauri::Wry>>,
    params: OpenParams,
) -> Result<String, String> {
    let (create, url) = params.into_create_params();
    state.manager.create(create, url)
}

/// Navigate an existing browser pane's overlay.
#[tauri::command]
pub fn browser_navigate(
    state: State<'_, BrowserState<tauri::Wry>>,
    pane_id: PaneId,
    url: String,
) -> Result<(), String> {
    state.manager.navigate(&pane_id, &url)
}

/// Close (destroy) a browser pane's overlay.
#[tauri::command]
pub fn browser_close(
    state: State<'_, BrowserState<tauri::Wry>>,
    pane_id: PaneId,
) -> Result<(), String> {
    state.manager.destroy(&pane_id)
}

/// Anchor geometry reported by the frontend each layout change (camelCase wire).
/// All window-frame values are PHYSICAL px (frontend reads them from the Tauri
/// window API); the anchor rect + scroll are CSS px. `visible` drives clip-to-hide
/// for occlusion / scroll-out (breakage modes #2/#3).
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncBoundsParams {
    pub pane_id: PaneId,
    pub main_outer_x: i32,
    pub main_outer_y: i32,
    pub client_inset_x: i32,
    pub client_inset_y: i32,
    pub anchor_x: f64,
    pub anchor_y: f64,
    pub anchor_width: f64,
    pub anchor_height: f64,
    #[serde(default)]
    pub scroll_x: f64,
    #[serde(default)]
    pub scroll_y: f64,
    pub main_scale: f64,
    pub visible: bool,
}

/// Sync an overlay to the anchor leaf: compute the physical rect and reposition,
/// then apply visibility (hide when occluded / scrolled out of view).
#[tauri::command]
pub fn browser_sync_bounds(
    state: State<'_, BrowserState<tauri::Wry>>,
    params: SyncBoundsParams,
) -> Result<(), String> {
    let input = BoundsInput {
        main_outer_physical: PhysicalPoint {
            x: params.main_outer_x,
            y: params.main_outer_y,
        },
        client_inset_physical: PhysicalPoint {
            x: params.client_inset_x,
            y: params.client_inset_y,
        },
        anchor_rect_css: CssRect {
            x: params.anchor_x,
            y: params.anchor_y,
            width: params.anchor_width,
            height: params.anchor_height,
        },
        scroll_offset_css: (params.scroll_x, params.scroll_y),
        main_scale: params.main_scale,
    };
    let rect = overlay_physical_rect(&input);
    state.manager.position(&params.pane_id, rect)?;
    state.manager.set_visible(&params.pane_id, params.visible)
}

/// Recreate an overlay with new creation-time params (enable CDP / attach proxy /
/// switch profile). Captures URL + scroll, builds a fresh overlay, restores state,
/// swaps z-order, destroys the old window. Returns the new label.
#[tauri::command]
pub fn browser_recreate(
    state: State<'_, BrowserState<tauri::Wry>>,
    params: OpenParams,
    scroll_x: Option<f64>,
    scroll_y: Option<f64>,
) -> Result<String, String> {
    let pane = params.pane_id;
    let (create, _url) = params.into_create_params();
    let scroll = match (scroll_x, scroll_y) {
        (Some(x), Some(y)) => Some((x, y)),
        _ => None,
    };
    let plan = plan_recreate(&state.manager, &pane, create, scroll)?;
    execute_recreate(&state.manager, plan)
}
