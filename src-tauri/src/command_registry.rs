//! Command registration pattern (P2 Keystone 1).
//!
//! Problem: 6 Wave-1 + 6 Wave-2 agents each add Tauri commands. If every agent
//! edits `lib.rs` `.invoke_handler(...)`, every merge conflicts.
//!
//! KEY CONSTRAINT (verified): `Builder::invoke_handler` is **last-wins, not
//! additive** — calling it twice drops the first set. So modules CANNOT each call
//! `.invoke_handler()`. Instead:
//!
//! - Commands are listed in ONE `generate_handler!` here (this file is the single
//!   designated conflict-point; adding a command = one line, lead-serialized).
//! - Each track's `register(builder)` does only ADDITIVE setup — `.manage()` for
//!   state, `.plugin()`, `.setup()` — which compose without conflict.
//! - `lib.rs` stays frozen; agents never touch the entry point.
//!
//! Frontend still invokes commands by plain name (`invoke('ping')`), unlike the
//! `plugin:name|cmd` convention a plugin-per-track design would force.

use tauri::{Builder, Runtime};

// Track modules are declared HERE (not in lib.rs) so a new track adds its
// `mod <track>;` line to this single file — the one designated conflict-point —
// keeping `lib.rs` truly frozen. Files stay at the conventional `src/<track>/`
// location (matches plan owner-globs) via `#[path]`. Add a track = 3 one-line
// edits in THIS file: (1) `#[path] mod` decl below, (2) `register()` call,
// (3) command(s) in generate_handler!.
#[path = "agent_rpc/mod.rs"]
pub mod agent_rpc;
#[path = "automation/mod.rs"]
pub mod automation;
#[path = "browser/mod.rs"]
pub mod browser;
#[path = "config/mod.rs"]
pub mod config;
#[path = "import/mod.rs"]
pub mod import;
#[path = "notify/mod.rs"]
pub mod notify;
#[path = "palette/mod.rs"]
pub mod palette;
#[path = "pty/mod.rs"]
pub mod pty;
#[path = "reliability/mod.rs"]
pub mod reliability;
#[path = "sidebar/mod.rs"]
pub mod sidebar;
#[path = "ssh/mod.rs"]
pub mod ssh;
#[path = "vmclient/mod.rs"]
pub mod vmclient;

/// Fold every track's ADDITIVE setup (state/plugins/setup hooks) onto the builder,
/// then install the single merged command handler.
///
/// Add a track: (1) `mod <track>;` above, (2) call its `register(builder)` below
/// for state/setup, (3) add its commands to the `generate_handler!` list. All
/// three are one-line edits in this file only — `lib.rs` never changes.
pub fn register_all<R: Runtime>(builder: Builder<R>) -> Builder<R> {
    // --- additive per-track setup (state/plugins/setup) ---
    let builder = pty::register(builder);
    let builder = config::register(builder);
    let builder = ssh::register(builder);
    let builder = import::register(builder);
    let builder = vmclient::register(builder);
    let builder = notify::register(builder);
    // Wave-2 tracks (additive-only setup; commands added to generate_handler! at
    // integration once each track reports its surface).
    let builder = browser::register(builder);
    let builder = automation::register(builder);
    let builder = agent_rpc::register(builder);
    let builder = sidebar::register(builder);
    let builder = palette::register(builder);
    let builder = reliability::register(builder);
    // ... one line per track ...

    // --- single shared `.setup()` hook (last-wins constraint → exactly one call) ---
    // Tracks needing the concrete runtime + AppHandle at startup expose a
    // `setup_*` fn called here, instead of each calling `Builder::setup` (which is
    // last-wins and would silently drop sibling setups — same trap as
    // invoke_handler). Add a track's startup work = one line in this closure.
    let builder = builder.setup(|app| {
        browser::setup_overlay(app);
        agent_rpc::setup_rpc_server();
        reliability::setup_reliability(app);
        Ok(())
    });

    // --- single merged command handler (last-wins constraint → exactly one call) ---
    builder.invoke_handler(tauri::generate_handler![
        pty::commands::ping,
        // P3 terminal core
        pty::commands::pty_spawn,
        pty::commands::pty_write,
        pty::commands::pty_resize,
        pty::commands::pty_kill,
        pty::commands::pty_pause,
        pty::commands::pty_resume,
        pty::commands::serialize_pane_state,
        // P12 config + workspace-store + weft.json + keybindings
        config::commands::config_get,
        config::commands::config_set,
        config::commands::workspace_snapshot,
        config::commands::workspace_add,
        config::commands::workspace_remove,
        config::commands::workspace_select,
        config::commands::workspace_reorder,
        config::commands::weft_defs_get,
        config::commands::weft_trust_check,
        config::commands::weft_trust_grant,
        config::commands::keybinding_resolve,
        config::commands::keybinding_list,
        config::commands::keybinding_set,
        // P5a notification core (OSC parse + per-pane manager)
        notify::commands::notify_ingest_osc,
        notify::commands::notify_ingest_bytes,
        notify::commands::notify_pane_state,
        notify::commands::notify_unread_count,
        notify::commands::notify_list,
        notify::commands::notify_mark_read,
        notify::commands::notify_clear,
        notify::commands::notify_build_osc,
        // P8 vmclient contract stub (local returns Unsupported)
        vmclient::commands::vm_protocol_version,
        vmclient::commands::vm_status,
        // P10a SSH transport core (russh + socks5h broker)
        ssh::commands::ssh_connect,
        ssh::commands::ssh_disconnect,
        ssh::commands::ssh_status,
        // P11a browser import core (rookie decrypt + rusqlite history, consent-gated)
        import::commands::import_list_browsers,
        import::commands::import_cookies,
        import::commands::import_history,
        // P7 browser automation (single inject-JS DOM-walk; live-pane wiring Wave-3)
        automation::commands::browser_snapshot,
        automation::commands::browser_click,
        automation::commands::browser_fill,
        automation::commands::browser_eval,
        automation::commands::browser_wait,
        automation::commands::browser_get,
        automation::commands::browser_find,
        // P6 browser pane overlay window (creation-time params + recreate pathway)
        browser::commands::browser_open,
        browser::commands::browser_navigate,
        browser::commands::browser_close,
        browser::commands::browser_sync_bounds,
        browser::commands::browser_recreate,
        // P16 command palette fuzzy search (in-process nucleo)
        palette::nucleo_search::palette_search,
        // P15b sidebar metadata scan toggles (expensive scans default-off)
        sidebar::commands::sidebar_port_scan_enabled,
        sidebar::commands::sidebar_set_port_scan,
        sidebar::commands::sidebar_git_watch_enabled,
        sidebar::commands::sidebar_set_git_watch,
        // P14 reliability (crash-report opt-in toggle)
        reliability::commands::reliability_crash_optin,
        // ... one line per command, grouped by track ...
    ])
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::Path;

    /// Poka-yoke for the last-wins `Builder::setup` trap. `Builder::setup` stores a
    /// single closure (last call wins, like `invoke_handler`), so if two tracks
    /// each call `builder.setup(...)` the later silently drops the earlier — which
    /// once left `BrowserState` unmanaged and made the overlay commands panic on
    /// first invoke. The fix: tracks expose a `setup_*` fn and the ONE shared
    /// `.setup()` hook lives in `register_all` (this file). A full-builder boot test
    /// would catch this too, but `tauri`'s `test` feature can't link headless on
    /// Windows (WebView2Loader entrypoint), so we guard at the source level instead:
    /// no track `mod.rs` may call `builder.setup(` — only this file may.
    #[test]
    fn only_command_registry_calls_builder_setup() {
        let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        let tracks = [
            "browser", "automation", "agent_rpc", "sidebar", "palette", "pty",
            "config", "ssh", "import", "vmclient", "notify", "reliability",
        ];
        for track in tracks {
            let mod_rs = src.join(track).join("mod.rs");
            let body = fs::read_to_string(&mod_rs)
                .unwrap_or_else(|e| panic!("read {}: {e}", mod_rs.display()));
            // Match a real call, not the words in a doc-comment.
            assert!(
                !body.contains("builder.setup(") && !body.contains(".setup(|"),
                "{track}/mod.rs calls Builder::setup directly — that drops sibling \
                 setups (last-wins). Expose a `setup_*` fn and call it from the \
                 shared hook in register_all instead.",
            );
        }
    }
}
