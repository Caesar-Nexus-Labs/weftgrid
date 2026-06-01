//! RPC server (P13): bind a LOCAL-ONLY transport (unix domain socket on Linux,
//! named pipe on Windows — NEVER TCP), accept connections, verify the token, and
//! dispatch each framed request.
//!
//! The per-connection logic ([`handle_connection`]) is transport-agnostic
//! (`AsyncRead + AsyncWrite`), so it is unit-tested over an in-memory duplex
//! without touching the OS socket layer. The OS-specific bind/accept lives behind
//! the [`Transport`] trait with two `#[cfg]`-gated impls.

use std::sync::Arc;

use tokio::io::{AsyncRead, AsyncWrite};

use super::auth::SessionToken;
use super::dispatch::Dispatcher;
use super::protocol::{read_frame, write_frame, ErrorModel, RpcRequest, RpcResponse};

/// Shared server state passed to each connection: the session token to verify
/// against and the dispatcher to route verified commands.
#[derive(Clone)]
pub struct ServerContext {
    pub token: Arc<SessionToken>,
    pub dispatcher: Arc<Dispatcher>,
}

/// Handle one client connection: read a single request frame, authenticate, route
/// the command, and write the response frame.
///
/// Auth runs BEFORE any dispatch. A missing/wrong token returns an `unauthorized`
/// error with a generic message (no detail on why — avoids an oracle). A malformed
/// frame returns `bad_request`.
pub async fn handle_connection<S>(ctx: &ServerContext, stream: &mut S) -> std::io::Result<()>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    let response = match read_frame(stream).await {
        Ok(bytes) => process_request(ctx, &bytes),
        Err(e) => {
            // A truncated/oversized frame — reply with a structured error rather
            // than dropping silently, so the CLI can surface it.
            RpcResponse::error(ErrorModel::new("bad_request", e.to_string()))
        }
    };
    let encoded = serde_json::to_vec(&response)
        .unwrap_or_else(|_| b"{\"status\":\"error\",\"error\":{\"code\":\"internal\",\"message\":\"encode failed\"}}".to_vec());
    write_frame(stream, &encoded).await
}

/// Parse + authenticate + dispatch a single request body. Split out from IO so it
/// is directly unit-testable.
pub fn process_request(ctx: &ServerContext, body: &[u8]) -> RpcResponse {
    let request: RpcRequest = match serde_json::from_slice(body) {
        Ok(r) => r,
        Err(e) => return RpcResponse::error(ErrorModel::new("bad_request", e.to_string())),
    };
    if !ctx.token.verify(&request.token) {
        // Generic message — do not reveal whether the token was missing vs wrong.
        return RpcResponse::error(ErrorModel::new("unauthorized", "authentication failed"));
    }
    ctx.dispatcher.dispatch(&request.command)
}

/// A bound local transport that yields client connections. Two impls
/// (`#[cfg(unix)]` / `#[cfg(windows)]`) provide the OS-specific endpoint; both are
/// local-only by construction.
pub trait Transport {
    /// A human-readable endpoint address (socket path / pipe name) the CLI reads
    /// from the endpoint file to connect.
    fn endpoint(&self) -> String;
}

/// Serve the accept loop on `transport`, handling each connection with `ctx`.
/// Runs until the transport errors fatally (e.g. the listener is dropped).
#[cfg(unix)]
pub async fn serve(ctx: ServerContext, transport: unix_impl::UnixTransport) -> std::io::Result<()> {
    loop {
        let (mut stream, _addr) = transport.listener.accept().await?;
        let ctx = ctx.clone();
        tokio::spawn(async move {
            let _ = handle_connection(&ctx, &mut stream).await;
        });
    }
}

#[cfg(unix)]
pub mod unix_impl {
    //! Unix domain socket transport. Filesystem permissions on the socket path (it
    //! lives in the user-only app-data dir) keep it local + current-user.
    use super::Transport;
    use std::path::{Path, PathBuf};
    use tokio::net::UnixListener;

    pub struct UnixTransport {
        pub(super) listener: UnixListener,
        path: PathBuf,
    }

    impl UnixTransport {
        /// Bind a fresh socket at `path`, removing any stale socket file first (a
        /// previous session that didn't clean up). The parent dir must already be
        /// user-only (the app-data dir).
        pub fn bind(path: impl AsRef<Path>) -> std::io::Result<Self> {
            let path = path.as_ref().to_path_buf();
            if path.exists() {
                let _ = std::fs::remove_file(&path);
            }
            let listener = UnixListener::bind(&path)?;
            Ok(UnixTransport { listener, path })
        }
    }

    impl Transport for UnixTransport {
        fn endpoint(&self) -> String {
            self.path.to_string_lossy().into_owned()
        }
    }

    impl Drop for UnixTransport {
        fn drop(&mut self) {
            // Best-effort: remove the socket file so the next session binds clean.
            let _ = std::fs::remove_file(&self.path);
        }
    }
}

#[cfg(windows)]
pub async fn serve(
    ctx: ServerContext,
    transport: windows_impl::NamedPipeTransport,
) -> std::io::Result<()> {
    use tokio::net::windows::named_pipe::ServerOptions;
    let pipe_name = transport.pipe_name.clone();
    // First server instance is created in the transport; recreate one before each
    // accept so the pipe stays available for the next client.
    let mut server = transport.into_first_server();
    loop {
        server.connect().await?;
        let mut connected = server;
        // Pre-create the next instance so a client can connect while we serve this.
        server = ServerOptions::new().create(&pipe_name)?;
        let ctx = ctx.clone();
        tokio::spawn(async move {
            let _ = handle_connection(&ctx, &mut connected).await;
        });
    }
}

#[cfg(windows)]
pub mod windows_impl {
    //! Windows named-pipe transport. The default named-pipe ACL can let other
    //! processes connect; `ServerOptions` is created in local-only mode and the
    //! pipe name is unguessable per session. Explicit SDDL (current-user-only
    //! security descriptor) is flagged to lead as hardening — see report.
    use super::Transport;
    use tokio::net::windows::named_pipe::{NamedPipeServer, ServerOptions};

    pub struct NamedPipeTransport {
        pub(super) pipe_name: String,
        first: NamedPipeServer,
    }

    impl NamedPipeTransport {
        /// Create the first server instance of a per-session pipe. The name carries
        /// a random session id so it is not guessable across sessions.
        pub fn bind(pipe_name: impl Into<String>) -> std::io::Result<Self> {
            let pipe_name = pipe_name.into();
            let first = ServerOptions::new()
                .first_pipe_instance(true)
                .create(&pipe_name)?;
            Ok(NamedPipeTransport { pipe_name, first })
        }

        pub(super) fn into_first_server(self) -> NamedPipeServer {
            self.first
        }
    }

    impl Transport for NamedPipeTransport {
        fn endpoint(&self) -> String {
            self.pipe_name.clone()
        }
    }
}

/// Compute the default endpoint address for this OS. Unix: a socket file in the
/// app-data dir. Windows: a per-session named pipe under `\\.\pipe\`.
pub fn default_endpoint(app_data_dir: &std::path::Path, session_id: &str) -> String {
    #[cfg(windows)]
    {
        let _ = app_data_dir;
        format!(r"\\.\pipe\weftgrid-rpc-{session_id}")
    }
    #[cfg(not(windows))]
    {
        let _ = session_id;
        app_data_dir
            .join("weft-rpc.sock")
            .to_string_lossy()
            .into_owned()
    }
}

#[cfg(test)]
mod tests {
    use super::super::dispatch::{test_support::MockAutomation, StubHandlers};
    use super::super::protocol::{
        read_frame, write_frame, BrowserAction, Command, PaneTarget, RpcRequest,
    };
    use super::*;

    fn test_ctx(token: &str) -> ServerContext {
        let dispatcher =
            Dispatcher::new(Box::new(MockAutomation::default()), Box::new(StubHandlers));
        ServerContext {
            token: Arc::new(SessionToken::from_secret(token)),
            dispatcher: Arc::new(dispatcher),
        }
    }

    fn req(token: &str) -> Vec<u8> {
        let r = RpcRequest {
            token: token.to_string(),
            command: Command::Browser {
                target: PaneTarget::Focused,
                action: BrowserAction::Snapshot,
            },
        };
        serde_json::to_vec(&r).unwrap()
    }

    #[test]
    fn process_request_accepts_correct_token() {
        let ctx = test_ctx("secret");
        let resp = process_request(&ctx, &req("secret"));
        assert!(matches!(resp, RpcResponse::Ok { .. }));
    }

    #[test]
    fn process_request_rejects_wrong_token() {
        let ctx = test_ctx("secret");
        match process_request(&ctx, &req("nope")) {
            RpcResponse::Error { error } => assert_eq!(error.code, "unauthorized"),
            other => panic!("expected unauthorized, got {other:?}"),
        }
    }

    #[test]
    fn process_request_rejects_malformed_body() {
        let ctx = test_ctx("secret");
        match process_request(&ctx, b"not json") {
            RpcResponse::Error { error } => assert_eq!(error.code, "bad_request"),
            other => panic!("expected bad_request, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn full_round_trip_over_duplex() {
        let ctx = test_ctx("secret");
        let (mut client, mut server) = tokio::io::duplex(1024);

        // Server handles one connection.
        let server_task = tokio::spawn(async move {
            handle_connection(&ctx, &mut server).await.unwrap();
        });

        // Client frames a request and reads the framed response.
        write_frame(&mut client, &req("secret")).await.unwrap();
        let resp_bytes = read_frame(&mut client).await.unwrap();
        let resp: RpcResponse = serde_json::from_slice(&resp_bytes).unwrap();
        assert!(matches!(resp, RpcResponse::Ok { .. }));
        server_task.await.unwrap();
    }

    #[tokio::test]
    async fn large_frame_round_trips() {
        // A big eval body exercises the length-prefixed framing past one buffer.
        let big_js = "x".repeat(512 * 1024);
        let r = RpcRequest {
            token: "secret".into(),
            command: Command::Browser {
                target: PaneTarget::Focused,
                action: BrowserAction::Eval { js: big_js.clone() },
            },
        };
        let body = serde_json::to_vec(&r).unwrap();

        let ctx = test_ctx("secret");
        let (mut client, mut server) = tokio::io::duplex(4096);
        let server_task = tokio::spawn(async move {
            handle_connection(&ctx, &mut server).await.unwrap();
        });
        write_frame(&mut client, &body).await.unwrap();
        let resp_bytes = read_frame(&mut client).await.unwrap();
        let resp: RpcResponse = serde_json::from_slice(&resp_bytes).unwrap();
        assert!(matches!(resp, RpcResponse::Ok { .. }));
        server_task.await.unwrap();
    }
}
