//! Lifecycle + param-plumbing tests for the overlay manager.
//!
//! Uses a `FakeSpawner` recording every window-seam call so the bookkeeping map,
//! creation-time param plumbing, and the CDP-args string (which must re-include
//! wry's defaults) are verified without a running app.

use super::*;
use std::cell::RefCell;
use uuid::Uuid;

/// Records every seam call so tests can assert lifecycle ordering and args.
#[derive(Default)]
struct FakeSpawner {
    calls: RefCell<Vec<String>>,
    /// Captured params from the last `spawn` (for asserting plumbing).
    last_spawn_args: RefCell<Option<(OverlayCreateParams, Option<String>)>>,
}

impl WindowSpawner for FakeSpawner {
    fn spawn(
        &self,
        params: &OverlayCreateParams,
        initial_url: Option<&str>,
    ) -> Result<String, String> {
        self.calls
            .borrow_mut()
            .push(format!("spawn:{}", params.window_label()));
        *self.last_spawn_args.borrow_mut() =
            Some((params.clone(), initial_url.map(str::to_string)));
        Ok(params.window_label())
    }
    fn set_bounds(&self, label: &str, rect: PhysicalRect) -> Result<(), String> {
        self.calls
            .borrow_mut()
            .push(format!("bounds:{label}:{},{}", rect.x, rect.y));
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
}

fn params(cdp: Option<u16>, proxy: Option<&str>) -> OverlayCreateParams {
    OverlayCreateParams {
        pane_id: Uuid::new_v4(),
        cdp_port: cdp,
        proxy_url: proxy.map(str::to_string),
        profile_dir: PathBuf::from("/tmp/weft-profile"),
    }
}

#[test]
fn cdp_args_reinclude_wry_defaults_when_enabled() {
    let p = params(Some(9333), None);
    let args = p.cdp_args().expect("cdp enabled → Some");
    assert!(args.contains("--remote-debugging-port=9333"));
    // Override replaces wry's defaults, so they MUST be re-included.
    assert!(args.contains(WRY_DEFAULT_DISABLE_FEATURES));
    assert!(args.contains(WRY_AUTOPLAY_FLAG));
}

#[test]
fn cdp_args_none_when_disabled_so_wry_keeps_its_defaults() {
    // No CDP → don't override at all → wry keeps msWebOOUI/SmartScreen disabled.
    assert!(params(None, None).cdp_args().is_none());
}

#[test]
fn ephemeral_port_zero_is_still_emitted() {
    // Port 0 = "bind ephemeral, discover via /json/version" — must appear, not skip.
    let args = params(Some(0), None).cdp_args().unwrap();
    assert!(args.contains("--remote-debugging-port=0"));
}

#[test]
fn window_label_is_browser_paneid() {
    let p = params(None, None);
    assert_eq!(p.window_label(), format!("browser-{}", p.pane_id));
}

#[test]
fn create_plumbs_params_and_initial_url() {
    let mgr = OverlayManager::new(FakeSpawner::default());
    let p = params(Some(9444), Some("socks5h://127.0.0.1:1080"));
    let pane = p.pane_id;
    let label = mgr
        .create(p.clone(), Some("https://example.com".into()))
        .unwrap();
    assert_eq!(label, format!("browser-{pane}"));

    let (got_params, got_url) = mgr.spawner().last_spawn_args.borrow().clone().unwrap();
    assert_eq!(got_params, p, "all 3 creation-time params plumbed verbatim");
    assert_eq!(got_url.as_deref(), Some("https://example.com"));
    assert_eq!(mgr.len(), 1);
}

#[test]
fn duplicate_create_for_same_pane_is_rejected() {
    let mgr = OverlayManager::new(FakeSpawner::default());
    let p = params(None, None);
    mgr.create(p.clone(), None).unwrap();
    let err = mgr.create(p, None).unwrap_err();
    assert!(err.contains("already exists"));
    assert_eq!(mgr.len(), 1);
}

#[test]
fn lifecycle_create_position_navigate_hide_destroy() {
    let mgr = OverlayManager::new(FakeSpawner::default());
    let p = params(None, None);
    let pane = p.pane_id;
    let label = mgr.create(p, None).unwrap();

    mgr.position(
        &pane,
        PhysicalRect {
            x: 10,
            y: 20,
            width: 300,
            height: 200,
        },
    )
    .unwrap();
    mgr.navigate(&pane, "https://docs.rs").unwrap();
    mgr.set_visible(&pane, false).unwrap();
    mgr.set_on_top(&pane, true).unwrap();

    assert_eq!(
        mgr.entry(&pane).unwrap().current_url.as_deref(),
        Some("https://docs.rs")
    );
    assert!(!mgr.entry(&pane).unwrap().visible);

    mgr.destroy(&pane).unwrap();
    assert!(mgr.is_empty());
    assert!(mgr.entry(&pane).is_none());

    let calls = mgr.spawner().calls.borrow().clone();
    assert_eq!(
        calls,
        vec![
            format!("spawn:{label}"),
            format!("bounds:{label}:10,20"),
            format!("nav:{label}:https://docs.rs"),
            format!("visible:{label}:false"),
            format!("ontop:{label}:true"),
            format!("destroy:{label}"),
        ]
    );
}

#[test]
fn ops_on_unknown_pane_error() {
    let mgr = OverlayManager::new(FakeSpawner::default());
    let ghost = Uuid::new_v4();
    assert!(mgr
        .position(
            &ghost,
            PhysicalRect {
                x: 0,
                y: 0,
                width: 1,
                height: 1
            }
        )
        .is_err());
    assert!(mgr.navigate(&ghost, "https://x").is_err());
    assert!(mgr.destroy(&ghost).is_err());
}

#[test]
fn proxy_url_kept_as_string_in_params() {
    // proxy_url stays a String through the param struct (no `url` crate dep);
    // parsing happens only at the window seam.
    let p = params(None, Some("http://proxy.local:3128"));
    assert_eq!(p.proxy_url.as_deref(), Some("http://proxy.local:3128"));
}
