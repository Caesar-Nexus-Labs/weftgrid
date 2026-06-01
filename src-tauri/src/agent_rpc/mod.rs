//! Agent automation RPC track (P13 owner: `src-tauri/src/agent_rpc/**`,
//! `crates/weft-cli/**`).
//!
//! Local socket / named-pipe RPC server (NEVER TCP) that the unified `weft` CLI
//! connects to, so an agent in a shell pane can drive browser automation
//! (`weft browser snapshot|click|fill|...`). Per-session token auth (user-only
//! file, constant-time verify); length-prefixed JSON framing; dispatch routes to
//! the P7 automation command set.
//!
//! Module map:
//!   - [`protocol`] — request/response schema, length-prefixed framing, error model.
//!   - [`auth`] — per-session token: generate, persist user-only, constant-time verify.
//!   - [`dispatch`] — route a verified command → `AutomationDispatch` (P7) / handlers.
//!   - [`server`] — bind local transport, accept loop, auth, dispatch, respond.
//!   - [`commands`] — Tauri startup hook: issue token + bind server.
//!
//! `register` is additive-only (no `invoke_handler` — this track exposes no
//! `#[tauri::command]`; its surface is the local socket the CLI connects to).
//! Server startup lives in [`setup_rpc_server`], called from the SINGLE shared
//! `.setup()` hook in `command_registry` — `Builder::setup` is last-wins, so
//! tracks must NOT each call it (doing so silently dropped sibling setups).

use tauri::{Builder, Runtime};

pub mod auth;
pub mod commands;
pub mod dispatch;
pub mod protocol;
pub mod server;

#[cfg(test)]
#[path = "agent_rpc.test.rs"]
mod integration_tests;

/// Additive-only fold. Server startup happens in [`setup_rpc_server`] instead,
/// because `Builder::setup` is last-wins — `command_registry` owns the one shared
/// setup hook and calls each track's setup function from it.
pub fn register<R: Runtime>(builder: Builder<R>) -> Builder<R> {
    builder
}

/// Spawn the RPC server (token issuance + transport bind). Best-effort — a bind
/// failure logs and the terminal keeps working without agent RPC, so we never
/// abort app startup. Called from the shared `.setup()` hook.
pub fn setup_rpc_server() {
    match commands::start_rpc_server() {
        Ok(endpoint) => {
            // Endpoint (socket path / pipe name) is not a secret; the token is
            // and is never logged.
            eprintln!("[agent_rpc] RPC server listening at {endpoint}");
        }
        Err(e) => {
            eprintln!("[agent_rpc] RPC server failed to start: {e}");
        }
    }
}
