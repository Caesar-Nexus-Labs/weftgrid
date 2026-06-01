//! Fixture-driven tests for the physical bounds math.
//!
//! Loaded as `#[path]` test submodule of `overlay-bounds-physical.rs`. Each case
//! reads a REAL captured-geometry fixture from `tests/fixtures/dual-dpi-bounds/`
//! and asserts the computed physical rect. The fixtures encode plausible captured
//! values (outer/inner positions in physical px, `getBoundingClientRect` in CSS
//! px) — NOT a synthesized single-scale formula — so a buggy single-scale
//! implementation would fail the straddle case instead of silently passing.

use super::*;
use serde::Deserialize;
use std::path::PathBuf;

#[derive(Debug, Deserialize)]
struct BoundsFixture {
    description: String,
    input: BoundsInput,
    expected_physical_rect: PhysicalRect,
    monitors: Vec<PhysicalRect>,
    expected_straddles: bool,
}

fn fixtures_dir() -> PathBuf {
    // CARGO_MANIFEST_DIR = src-tauri/; fixtures live at repo-root tests/.
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("tests")
        .join("fixtures")
        .join("dual-dpi-bounds")
}

fn load(name: &str) -> BoundsFixture {
    let path = fixtures_dir().join(name);
    let raw = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("read fixture {}: {e}", path.display()));
    serde_json::from_str(&raw).unwrap_or_else(|e| panic!("parse fixture {}: {e}", path.display()))
}

fn assert_fixture(name: &str) {
    let fx = load(name);
    let got = overlay_physical_rect(&fx.input);
    assert_eq!(
        got, fx.expected_physical_rect,
        "physical rect mismatch for fixture '{}' ({name})",
        fx.description
    );
    let straddles = straddles_monitors(&got, &fx.monitors);
    assert_eq!(
        straddles, fx.expected_straddles,
        "straddle detection mismatch for fixture '{}' ({name})",
        fx.description
    );
}

#[test]
fn single_monitor_100_percent() {
    assert_fixture("single-monitor-100.json");
}

#[test]
fn single_monitor_150_percent() {
    assert_fixture("single-monitor-150.json");
}

#[test]
fn dual_monitor_straddle_150_100() {
    // The case a single-scale implementation gets wrong: overlay crosses a
    // 150%/100% DPI boundary. Position comes from the OS already physical; only
    // the anchor offset is scaled (by the MAIN monitor's 1.5), so the rect stays
    // correct across the boundary.
    assert_fixture("dual-monitor-straddle-150-100.json");
}

#[test]
fn left_monitor_negative_origin_with_scroll() {
    // Negative physical X (monitor left of primary) + document-relative scroll
    // offset folded into the anchor.
    assert_fixture("left-monitor-negative-origin-scroll.json");
}

#[test]
fn straddle_is_false_when_rect_fits_one_monitor() {
    let monitors = [
        PhysicalRect {
            x: 0,
            y: 0,
            width: 1920,
            height: 1080,
        },
        PhysicalRect {
            x: 1920,
            y: 0,
            width: 1920,
            height: 1080,
        },
    ];
    let inside = PhysicalRect {
        x: 100,
        y: 100,
        width: 200,
        height: 200,
    };
    assert!(!straddles_monitors(&inside, &monitors));
    let crossing = PhysicalRect {
        x: 1820,
        y: 100,
        width: 200,
        height: 200,
    };
    assert!(straddles_monitors(&crossing, &monitors));
}

#[test]
fn negative_size_clamps_to_zero() {
    // A collapsed/negative anchor must not produce a negative cast-to-u32 size.
    let input = BoundsInput {
        main_outer_physical: PhysicalPoint { x: 0, y: 0 },
        client_inset_physical: PhysicalPoint { x: 0, y: 0 },
        anchor_rect_css: CssRect {
            x: 0.0,
            y: 0.0,
            width: -10.0,
            height: -5.0,
        },
        scroll_offset_css: (0.0, 0.0),
        main_scale: 1.0,
    };
    let rect = overlay_physical_rect(&input);
    assert_eq!(rect.width, 0);
    assert_eq!(rect.height, 0);
}
