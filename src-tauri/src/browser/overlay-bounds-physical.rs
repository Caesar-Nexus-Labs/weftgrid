//! Overlay bounds math — PHYSICAL coords end-to-end (P6 red-team C5).
//!
//! Pure module (no window handles, no Tauri) so the geometry is unit-testable
//! against real captured dual-DPI fixtures. The browser overlay is a separate OS
//! window that must sit exactly on top of a browser-anchor leaf living inside the
//! MAIN webview. Computing that screen rectangle correctly across monitors with
//! different DPI is the whole point of this module.
//!
//! ## Why physical end-to-end (no single-scale shortcut)
//!
//! The classic bug is `physical = logical_outer_pos * scale + anchor_css * scale`.
//! A window's *logical* outer position is ambiguous when it straddles monitors of
//! different scale, so multiplying it by one scale lands the overlay in the wrong
//! place. The fix:
//!   - take the main window's outer position from the OS already in PHYSICAL px
//!     (`WebviewWindow::outer_position()` returns `PhysicalPosition`),
//!   - add the client-area inset in PHYSICAL px (`inner_position - outer_position`),
//!   - convert ONLY the anchor's CSS-relative offset by the main monitor's scale
//!     (the main webview renders entirely at the main monitor's scale, so its CSS
//!     px map by exactly `main_scale`).
//!
//! Physical screen coords are a single unified space across all monitors (the
//! Windows virtual screen is physical px). So when the resulting overlay rect
//! straddles a 100%/150% boundary the OS renders each half at the right DPI while
//! the rect itself stays correct — we never force one scale onto both monitors.

use serde::{Deserialize, Serialize};

/// A point in PHYSICAL screen pixels (unified multi-monitor space; may be
/// negative for monitors left of / above the primary).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct PhysicalPoint {
    pub x: i32,
    pub y: i32,
}

/// A rectangle in PHYSICAL screen pixels — what the overlay's physical setters
/// (`set_position`/`set_size`) consume directly.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct PhysicalRect {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

/// An anchor rectangle in CSS pixels as reported by `getBoundingClientRect`
/// (viewport-relative — i.e. relative to the main webview's client-area origin).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct CssRect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

/// Everything needed to place the overlay, kept explicit so a fixture can encode a
/// real captured geometry and assert the output without a live window.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct BoundsInput {
    /// Main window outer top-left in PHYSICAL screen px (from `outer_position()`).
    /// Already physical — never derived by scaling a logical position.
    pub main_outer_physical: PhysicalPoint,
    /// Offset from the outer top-left to the client-area top-left, in PHYSICAL px
    /// (window border + titlebar + menu). Computed as `inner_position -
    /// outer_position`; recomputed on fullscreen toggle (client area changes).
    pub client_inset_physical: PhysicalPoint,
    /// Anchor leaf rect from `getBoundingClientRect`, CSS px, viewport-relative.
    pub anchor_rect_css: CssRect,
    /// Signed CSS-px translation added to the anchor before scaling. For standard
    /// viewport-relative `getBoundingClientRect` reporting this is `(0.0, 0.0)`
    /// (scroll is already reflected in the rect). It exists for callers that report
    /// the anchor in document-relative coords, where it carries the negative scroll
    /// position so the result still lands at the viewport position.
    pub scroll_offset_css: (f64, f64),
    /// Scale factor of the monitor hosting the MAIN window (CSS px -> physical px).
    /// Only the anchor offset/size is scaled by this — never the window position.
    pub main_scale: f64,
}

/// Convert a main-webview anchor rect into a PHYSICAL screen rectangle for the
/// overlay window. See module docs for why the position is never re-scaled.
pub fn overlay_physical_rect(input: &BoundsInput) -> PhysicalRect {
    // Client-area origin in physical px: outer position + inset, both physical.
    let client_x = input.main_outer_physical.x + input.client_inset_physical.x;
    let client_y = input.main_outer_physical.y + input.client_inset_physical.y;

    // Anchor offset within the client area, CSS px, then scaled ONCE by the main
    // monitor's scale (the only scale that applies to the main webview's content).
    let css_x = input.anchor_rect_css.x + input.scroll_offset_css.0;
    let css_y = input.anchor_rect_css.y + input.scroll_offset_css.1;

    let phys_x = client_x + (css_x * input.main_scale).round() as i32;
    let phys_y = client_y + (css_y * input.main_scale).round() as i32;

    // Size scales with the same factor; physical footprint is fixed even when the
    // rect straddles two monitors (OS renders each half at its own DPI).
    let width = (input.anchor_rect_css.width * input.main_scale)
        .round()
        .max(0.0) as u32;
    let height = (input.anchor_rect_css.height * input.main_scale)
        .round()
        .max(0.0) as u32;

    PhysicalRect {
        x: phys_x,
        y: phys_y,
        width,
        height,
    }
}

/// Whether the computed physical rect crosses a monitor boundary given the list of
/// monitor rects (physical px). Used by the manager to decide when straddle-aware
/// repositioning matters; pure so it is testable from fixtures.
pub fn straddles_monitors(rect: &PhysicalRect, monitors: &[PhysicalRect]) -> bool {
    let covering = monitors.iter().filter(|m| rect_intersects(rect, m)).count();
    covering > 1
}

fn rect_intersects(a: &PhysicalRect, b: &PhysicalRect) -> bool {
    let ax2 = a.x + a.width as i32;
    let ay2 = a.y + a.height as i32;
    let bx2 = b.x + b.width as i32;
    let by2 = b.y + b.height as i32;
    a.x < bx2 && ax2 > b.x && a.y < by2 && ay2 > b.y
}

#[cfg(test)]
#[path = "overlay-bounds-physical.test.rs"]
mod tests;
