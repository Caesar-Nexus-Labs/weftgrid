//! weftgrid Rust core entry (P2 Keystone 1).
//!
//! `lib.rs` is frozen after P2: it only builds the app via `command_registry`.
//! Track modules self-register their commands — agents never edit this file.

// P2 lays down the workspace-model contract + inject embed before any consumer
// exists (Wave-1/2 tracks). Allow dead_code crate-wide until those land.
#![allow(dead_code)]

mod command_registry;
mod inject_asset;
mod model;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let builder = tauri::Builder::default().plugin(tauri_plugin_opener::init());
    let builder = command_registry::register_all(builder);
    builder
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
