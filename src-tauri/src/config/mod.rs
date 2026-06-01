//! Config + workspace-store + session + keybinding + weft.json track
//! (P12 owner: `src-tauri/src/config/**`).
//!
//! P12a (Wave-1 blocker): typed config store (atomic write), workspace-store
//! logic, weft.json parse + trust gate, keybinding registry, and the Tauri
//! command surface P15/P16 build against. P12b: session persist/restore.
//!
//! `register` `.manage()`s the single `ConfigState` (config + workspace store +
//! trust + keybindings). Commands are listed once in `command_registry`
//! (last-wins `invoke_handler` constraint) — this module adds no handler.

use tauri::{Builder, Runtime};

pub mod commands;
pub mod keybindings;
pub mod migration;
pub mod schema;
pub mod session;
pub mod state;
pub mod store;
pub mod weft_config;
pub mod weft_trust;
pub mod workspace_store;

use state::ConfigState;

/// Additive setup: resolve the OS app-config dir and manage the live state. On a
/// dir-resolution failure we fall back to an in-process temp dir so the app still
/// boots (config just won't persist) rather than panicking at startup.
pub fn register<R: Runtime>(builder: Builder<R>) -> Builder<R> {
    let dir = directories::ProjectDirs::from("", "", "weftgrid")
        .map(|p| p.config_dir().to_path_buf())
        .unwrap_or_else(std::env::temp_dir);
    let state = ConfigState::from_dir(&dir)
        .unwrap_or_else(|_| ConfigState::from_dir(std::env::temp_dir()).expect("temp dir state"));
    builder.manage(state)
}
