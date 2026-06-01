//! Tauri startup wiring (P13): on app launch, generate the session token, bind the
//! local transport, persist the token + endpoint to the user-only app-data dir, and
//! spawn the accept loop.
//!
//! Registration is ADDITIVE (a `.setup()` hook) per the P2 pattern — no
//! `invoke_handler` call here. This track exposes NO `#[tauri::command]`s: the only
//! surface is the local socket/pipe the `weft` CLI connects to, so there is nothing
//! to add to `command_registry`'s `generate_handler!`.
//!
//! P7 integration (Wave-3): replace [`PendingAutomation`] with P7's real
//! `AutomationDispatch` impl. Until then browser ops return a clear `unavailable`
//! error so the transport + auth path is exercisable without P7.

use std::sync::Arc;

use super::auth::{self, SessionToken};
use super::dispatch::{AutomationDispatch, AutomationResult, Dispatcher, StubHandlers};
use super::protocol::{ErrorModel, GetKind, PaneTarget};
use super::server::{self, ServerContext};

/// Placeholder automation backend used until P7 provides its real impl. Every op
/// returns `unavailable` so an agent gets a clear signal rather than a hang.
struct PendingAutomation;

impl PendingAutomation {
    fn pending() -> AutomationResult {
        Err(ErrorModel::new(
            "unavailable",
            "browser automation backend not yet wired (P7 integration pending)",
        ))
    }
}

impl AutomationDispatch for PendingAutomation {
    fn snapshot(&self, _t: &PaneTarget) -> AutomationResult {
        Self::pending()
    }
    fn click(&self, _t: &PaneTarget, _r: &str) -> AutomationResult {
        Self::pending()
    }
    fn fill(&self, _t: &PaneTarget, _r: &str, _x: &str) -> AutomationResult {
        Self::pending()
    }
    fn eval(&self, _t: &PaneTarget, _js: &str) -> AutomationResult {
        Self::pending()
    }
    fn wait(&self, _t: &PaneTarget, _s: Option<&str>, _ms: Option<u64>) -> AutomationResult {
        Self::pending()
    }
    fn get(
        &self,
        _t: &PaneTarget,
        _r: &str,
        _kind: GetKind,
        _attr: Option<&str>,
    ) -> AutomationResult {
        Self::pending()
    }
    fn find(&self, _t: &PaneTarget, _q: &str) -> AutomationResult {
        Self::pending()
    }
}

/// Build the dispatcher with the current automation backend. Wave-3 swaps
/// `PendingAutomation` for P7's impl here (single call site).
fn build_dispatcher() -> Dispatcher {
    Dispatcher::new(Box::new(PendingAutomation), Box::new(StubHandlers))
}

/// Start the RPC server: issue a fresh token, bind the OS transport, write the
/// token + endpoint files (user-only), and spawn the accept loop on the Tauri
/// async runtime. Returns the endpoint string for logging/diagnostics.
///
/// Failure here is non-fatal to the app (the terminal still works without agent
/// RPC), so the caller logs and continues rather than aborting startup.
pub fn start_rpc_server() -> std::io::Result<String> {
    let dir = auth::app_data_dir()?;
    std::fs::create_dir_all(&dir)?;

    let token = SessionToken::generate();
    token.persist(&dir)?;

    let session_id = uuid::Uuid::new_v4().simple().to_string();
    let endpoint = server::default_endpoint(&dir, &session_id);

    let ctx = ServerContext {
        token: Arc::new(token),
        dispatcher: Arc::new(build_dispatcher()),
    };

    spawn_transport(&dir, &endpoint, ctx)?;
    Ok(endpoint)
}

/// Persist the endpoint file then bind + spawn the OS-specific transport.
#[cfg(unix)]
fn spawn_transport(
    dir: &std::path::Path,
    endpoint: &str,
    ctx: ServerContext,
) -> std::io::Result<()> {
    use super::server::unix_impl::UnixTransport;
    let transport = UnixTransport::bind(endpoint)?;
    auth::write_user_only(&auth::endpoint_path(dir), endpoint.as_bytes())?;
    tauri::async_runtime::spawn(async move {
        let _ = server::serve(ctx, transport).await;
    });
    Ok(())
}

#[cfg(windows)]
fn spawn_transport(
    dir: &std::path::Path,
    endpoint: &str,
    ctx: ServerContext,
) -> std::io::Result<()> {
    use super::server::windows_impl::NamedPipeTransport;
    let transport = NamedPipeTransport::bind(endpoint)?;
    auth::write_user_only(&auth::endpoint_path(dir), endpoint.as_bytes())?;
    tauri::async_runtime::spawn(async move {
        let _ = server::serve(ctx, transport).await;
    });
    Ok(())
}

#[cfg(not(any(unix, windows)))]
fn spawn_transport(
    _dir: &std::path::Path,
    _endpoint: &str,
    _ctx: ServerContext,
) -> std::io::Result<()> {
    Err(std::io::Error::other(
        "agent RPC transport unsupported on this platform",
    ))
}
