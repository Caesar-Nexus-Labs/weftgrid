//! Tests for the recreate-with-state-transfer pathway.
//!
//! A `RecordingSpawner` captures seam calls (incl. `restore_scroll`) so the
//! capture -> create-new -> navigate+restore -> swap+destroy-old ordering is
//! verifiable without a live app.

use super::*;
use std::cell::RefCell;
use std::path::PathBuf;
use uuid::Uuid;

use super::super::overlay_bounds::PhysicalRect;

#[derive(Default)]
struct RecordingSpawner {
    calls: RefCell<Vec<String>>,
}

impl WindowSpawner for RecordingSpawner {
    fn spawn(
        &self,
        params: &OverlayCreateParams,
        initial_url: Option<&str>,
    ) -> Result<String, String> {
        self.calls.borrow_mut().push(format!(
            "spawn:{}:cdp={:?}:proxy={:?}:url={:?}",
            params.window_label(),
            params.cdp_port,
            params.proxy_url,
            initial_url
        ));
        Ok(params.window_label())
    }
    fn set_bounds(&self, label: &str, _rect: PhysicalRect) -> Result<(), String> {
        self.calls.borrow_mut().push(format!("bounds:{label}"));
        Ok(())
    }
    fn navigate(&self, label: &str, url: &str) -> Result<(), String> {
        self.calls.borrow_mut().push(format!("nav:{label}:{url}"));
        Ok(())
    }
    fn set_visible(&self, label: &str, visible: bool) -> Result<(), String> {
        self.calls
            .borrow_mut()
            .push(format!("visible:{label}:{visible}"));
        Ok(())
    }
    fn set_on_top(&self, label: &str, on_top: bool) -> Result<(), String> {
        self.calls
            .borrow_mut()
            .push(format!("ontop:{label}:{on_top}"));
        Ok(())
    }
    fn destroy(&self, label: &str) -> Result<(), String> {
        self.calls.borrow_mut().push(format!("destroy:{label}"));
        Ok(())
    }
    fn restore_scroll(&self, label: &str, x: f64, y: f64) -> Result<(), String> {
        self.calls
            .borrow_mut()
            .push(format!("scroll:{label}:{x},{y}"));
        Ok(())
    }
}

fn params(pane: Uuid, cdp: Option<u16>, proxy: Option<&str>) -> OverlayCreateParams {
    OverlayCreateParams {
        pane_id: pane,
        cdp_port: cdp,
        proxy_url: proxy.map(str::to_string),
        profile_dir: PathBuf::from("/tmp/p"),
    }
}

#[test]
fn plan_captures_current_url_and_scroll() {
    let mgr = OverlayManager::new(RecordingSpawner::default());
    let pane = Uuid::new_v4();
    mgr.create(params(pane, None, None), Some("https://a.test".into()))
        .unwrap();
    mgr.navigate(&pane, "https://b.test").unwrap();

    // Enabling automation = recreate with a CDP port baked in.
    let new = params(pane, Some(9555), None);
    let plan = plan_recreate(&mgr, &pane, new.clone(), Some((0.0, 420.0))).unwrap();

    assert_eq!(plan.old_label, format!("browser-{pane}"));
    assert_eq!(plan.new_params, new);
    assert_eq!(plan.transfer.url.as_deref(), Some("https://b.test"));
    assert_eq!(plan.transfer.scroll, Some((0.0, 420.0)));
}

#[test]
fn plan_errors_when_no_overlay() {
    let mgr = OverlayManager::new(RecordingSpawner::default());
    let pane = Uuid::new_v4();
    let err = plan_recreate(&mgr, &pane, params(pane, None, None), None).unwrap_err();
    assert!(err.contains("no overlay"));
}

#[test]
fn plan_errors_on_pane_id_mismatch() {
    let mgr = OverlayManager::new(RecordingSpawner::default());
    let pane = Uuid::new_v4();
    mgr.create(params(pane, None, None), None).unwrap();
    let other = Uuid::new_v4();
    let err = plan_recreate(&mgr, &pane, params(other, None, None), None).unwrap_err();
    assert!(err.contains("must match"));
}

#[test]
fn execute_transfers_state_and_swaps_windows() {
    let mgr = OverlayManager::new(RecordingSpawner::default());
    let pane = Uuid::new_v4();
    mgr.create(params(pane, None, None), Some("https://start.test".into()))
        .unwrap();
    mgr.navigate(&pane, "https://current.test").unwrap();
    mgr.spawner().calls.borrow_mut().clear();

    // Recreate to attach an SSH proxy (new creation-time proxy_url).
    let new = params(pane, None, Some("socks5h://127.0.0.1:1080"));
    let plan = plan_recreate(&mgr, &pane, new, Some((10.0, 200.0))).unwrap();
    let new_label = execute_recreate(&mgr, plan).unwrap();

    assert_eq!(new_label, format!("browser-{pane}"));
    // New overlay carries the proxy + captured URL; map still has exactly one
    // entry for the pane (old bookkeeping replaced).
    assert_eq!(mgr.len(), 1);
    assert_eq!(
        mgr.entry(&pane).unwrap().params.proxy_url.as_deref(),
        Some("socks5h://127.0.0.1:1080")
    );
    assert_eq!(
        mgr.entry(&pane).unwrap().current_url.as_deref(),
        Some("https://current.test")
    );

    let calls = mgr.spawner().calls.borrow().clone();
    // Old window destroyed BEFORE the new spawn (no duplicate-guard collision),
    // new overlay spawned with captured URL, scroll restored, then raised on top.
    assert_eq!(calls[0], format!("destroy:browser-{pane}"));
    assert!(calls[1].starts_with(&format!("spawn:browser-{pane}")));
    assert!(calls[1].contains("url=Some(\"https://current.test\")"));
    assert_eq!(calls[2], format!("scroll:browser-{pane}:10,200"));
    assert_eq!(calls[3], format!("ontop:browser-{pane}:true"));
}

#[test]
fn execute_without_scroll_skips_restore() {
    let mgr = OverlayManager::new(RecordingSpawner::default());
    let pane = Uuid::new_v4();
    mgr.create(params(pane, None, None), Some("https://x.test".into()))
        .unwrap();
    mgr.spawner().calls.borrow_mut().clear();

    let plan = plan_recreate(&mgr, &pane, params(pane, Some(9001), None), None).unwrap();
    execute_recreate(&mgr, plan).unwrap();

    let calls = mgr.spawner().calls.borrow().clone();
    assert!(calls.iter().all(|c| !c.starts_with("scroll:")));
}
