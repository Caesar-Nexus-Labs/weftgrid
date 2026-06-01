//! Production window seam — `WindowSpawner` backed by a Tauri `AppHandle`.
//!
//! This is the ONE place that touches real `WebviewWindow`s; all lifecycle LOGIC
//! lives in `overlay-manager.rs` / `overlay-recreate-state-transfer.rs` against the
//! [`WindowSpawner`] trait so it stays unit-testable. Here we translate trait calls
//! into `WebviewWindowBuilder` + window ops.
//!
//! Creation-time params are applied exactly once at build:
//!   - `cdp_args()` -> `additional_browser_args` (re-includes wry defaults),
//!   - `proxy_url` (String) -> parsed to `tauri::Url` -> `proxy_url(..)`,
//!   - `profile_dir` -> `data_directory(..)` (per-profile cookie isolation).
//!
//! The overlay is borderless (`decorations(false)`), off-taskbar
//! (`skip_taskbar(true)`), owned by the main window, and starts hidden so the
//! first bounds sync places it before it is shown (no flash at the origin).

use tauri::{AppHandle, Manager, Runtime, WebviewUrl, WebviewWindowBuilder};

use super::overlay_bounds::PhysicalRect;
use super::overlay_manager::{OverlayCreateParams, WindowSpawner};

/// Label of the main window the overlays are owned by / positioned against.
/// Only referenced on Windows (the `owner` builder call); on other platforms the
/// overlay follows the main window via bounds-sync, so the const would be dead.
#[cfg(windows)]
const MAIN_WINDOW_LABEL: &str = "main";

/// Tauri-backed spawner. Cloneable handle so the manager can live in `.manage()`d
/// state and still reach the app.
pub struct TauriWindowSpawner<R: Runtime> {
    app: AppHandle<R>,
}

impl<R: Runtime> TauriWindowSpawner<R> {
    pub fn new(app: AppHandle<R>) -> Self {
        Self { app }
    }
}

impl<R: Runtime> WindowSpawner for TauriWindowSpawner<R> {
    fn spawn(
        &self,
        params: &OverlayCreateParams,
        initial_url: Option<&str>,
    ) -> Result<String, String> {
        let label = params.window_label();

        // Start at about:blank; the command navigates to the real URL after build
        // so an invalid initial URL can't fail window creation. about:blank is an
        // External URL (App paths resolve against the bundled assets).
        let blank = "about:blank"
            .parse::<tauri::Url>()
            .map_err(|e| format!("about:blank parse: {e}"))?;
        let mut builder = WebviewWindowBuilder::new(&self.app, &label, WebviewUrl::External(blank))
            .decorations(false)
            .skip_taskbar(true)
            .visible(false)
            .data_directory(params.profile_dir.clone());

        // CDP: only override args when automation is on; otherwise wry keeps its
        // own (correct) defaults. The override string re-includes those defaults.
        // `additional_browser_args` is WebView2-only (Windows); on other platforms
        // the CDP-port mechanism does not apply, so the arg is simply not set.
        #[cfg(windows)]
        if let Some(args) = params.cdp_args() {
            builder = builder.additional_browser_args(&args);
        }

        // Proxy: parse String -> tauri::Url only here (avoids a `url` dep elsewhere).
        if let Some(proxy) = &params.proxy_url {
            let url = proxy
                .parse::<tauri::Url>()
                .map_err(|e| format!("invalid proxy_url '{proxy}': {e}"))?;
            builder = builder.proxy_url(url);
        }

        // Own + position relative to the main window so the overlay follows it
        // across virtual desktops / z-order (breakage modes #1, #7). `owner` is a
        // Win32 owner-window concept (Windows-only on the builder); on WebKitGTK the
        // overlay follows the main window via the bounds-sync reposition path instead.
        #[cfg(windows)]
        if let Some(main) = self.app.get_webview_window(MAIN_WINDOW_LABEL) {
            builder = builder
                .owner(&main)
                .map_err(|e| format!("set overlay owner: {e}"))?;
        }

        let window = builder
            .build()
            .map_err(|e| format!("build overlay window: {e}"))?;

        if let Some(url) = initial_url {
            let parsed = url
                .parse::<tauri::Url>()
                .map_err(|e| format!("invalid initial url '{url}': {e}"))?;
            window
                .navigate(parsed)
                .map_err(|e| format!("navigate overlay: {e}"))?;
        }

        Ok(label)
    }

    fn set_bounds(&self, label: &str, rect: PhysicalRect) -> Result<(), String> {
        let window = self.window(label)?;
        window
            .set_position(tauri::PhysicalPosition::new(rect.x, rect.y))
            .map_err(|e| format!("set_position: {e}"))?;
        window
            .set_size(tauri::PhysicalSize::new(rect.width, rect.height))
            .map_err(|e| format!("set_size: {e}"))
    }

    fn navigate(&self, label: &str, url: &str) -> Result<(), String> {
        let parsed = url
            .parse::<tauri::Url>()
            .map_err(|e| format!("invalid url '{url}': {e}"))?;
        self.window(label)?
            .navigate(parsed)
            .map_err(|e| format!("navigate: {e}"))
    }

    fn set_visible(&self, label: &str, visible: bool) -> Result<(), String> {
        let window = self.window(label)?;
        if visible {
            window.show().map_err(|e| format!("show: {e}"))
        } else {
            window.hide().map_err(|e| format!("hide: {e}"))
        }
    }

    fn set_on_top(&self, label: &str, on_top: bool) -> Result<(), String> {
        self.window(label)?
            .set_always_on_top(on_top)
            .map_err(|e| format!("set_always_on_top: {e}"))
    }

    fn destroy(&self, label: &str) -> Result<(), String> {
        self.window(label)?
            .destroy()
            .map_err(|e| format!("destroy: {e}"))
    }

    fn restore_scroll(&self, label: &str, x: f64, y: f64) -> Result<(), String> {
        // Best-effort: eval window.scrollTo on the overlay after navigation. A
        // failed eval must not fail the recreate.
        let window = self.window(label)?;
        let _ = window.eval(format!("window.scrollTo({x}, {y});"));
        Ok(())
    }
}

impl<R: Runtime> TauriWindowSpawner<R> {
    fn window(&self, label: &str) -> Result<tauri::WebviewWindow<R>, String> {
        self.app
            .get_webview_window(label)
            .ok_or_else(|| format!("overlay window '{label}' not found"))
    }
}
