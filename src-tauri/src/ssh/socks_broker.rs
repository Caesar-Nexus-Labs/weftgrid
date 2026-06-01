//! Local SOCKS5h proxy broker (P10a).
//!
//! Binds a loopback TCP listener and, for each accepted connection, runs the
//! SOCKS5 negotiation ([`super::socks`]) then forwards the byte stream through a
//! [`ChannelOpener`] — in production an SSH `direct-tcpip` channel, so the
//! **remote** host resolves the hostname (socks5h). The broker itself never
//! resolves DNS.
//!
//! The opener lives behind a swap so a reconnect can replace the dead SSH
//! session **without rebinding the port** — P6 browser overlays keep their
//! `proxy_url=socks5h://127.0.0.1:<port>` across reconnects (spec H3).

use std::future::Future;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::pin::Pin;
use std::sync::{Arc, Mutex};

use tokio::io::{copy_bidirectional, AsyncRead, AsyncWrite};
use tokio::net::{TcpListener, TcpStream};
use tokio::task::JoinHandle;

use super::socks::{self, ReplyCode, TargetAddr};

/// Combined async byte stream (read+write) used for a tunneled connection. Blanket
/// impl covers SSH `ChannelStream`, `TcpStream`, and `tokio::io::DuplexStream`.
pub trait TunnelIo: AsyncRead + AsyncWrite + Unpin + Send {}
impl<T: AsyncRead + AsyncWrite + Unpin + Send> TunnelIo for T {}

/// A tunneled connection to `(host, port)` on the remote side.
pub type TunnelStream = Box<dyn TunnelIo>;

/// Boxed `Send` future returned by [`ChannelOpener::open`] (hand-rolled to avoid
/// an `async-trait` dependency — same pattern as `vmclient`).
pub type OpenFuture<'a> = Pin<Box<dyn Future<Output = Result<TunnelStream, String>> + Send + 'a>>;

/// Opens a remote TCP tunnel for a SOCKS CONNECT target. The production impl
/// (`client::SshChannelOpener`) calls `channel_open_direct_tcpip(host, port)`,
/// handing the **raw hostname** to the remote sshd for resolution. Tests supply
/// an in-memory double.
pub trait ChannelOpener: Send + Sync {
    fn open(&self, host: String, port: u16) -> OpenFuture<'_>;
}

/// A running SOCKS5h broker. Dropping it stops the accept loop. The bound port is
/// stable for the broker's lifetime even if the backing SSH session is swapped.
pub struct SocksBroker {
    local_addr: SocketAddr,
    opener: Arc<Mutex<Arc<dyn ChannelOpener>>>,
    accept_task: JoinHandle<()>,
}

impl SocksBroker {
    /// Bind an ephemeral loopback port (`127.0.0.1:0`) and start accepting.
    pub async fn bind(opener: Arc<dyn ChannelOpener>) -> Result<Self, String> {
        Self::bind_on(SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 0), opener).await
    }

    /// Bind a specific address. **Rejects any non-loopback address** so the proxy
    /// is never reachable off-box (spec: broker loopback-only).
    pub async fn bind_on(addr: SocketAddr, opener: Arc<dyn ChannelOpener>) -> Result<Self, String> {
        if !addr.ip().is_loopback() {
            return Err(format!(
                "refusing non-loopback bind: {addr} (broker is loopback-only)"
            ));
        }
        let listener = TcpListener::bind(addr)
            .await
            .map_err(|e| format!("socks broker bind failed: {e}"))?;
        let local_addr = listener
            .local_addr()
            .map_err(|e| format!("local_addr failed: {e}"))?;

        let opener = Arc::new(Mutex::new(opener));
        let task_opener = opener.clone();
        let accept_task = tokio::spawn(async move {
            accept_loop(listener, task_opener).await;
        });

        Ok(SocksBroker {
            local_addr,
            opener,
            accept_task,
        })
    }

    /// The loopback endpoint browser panes point `proxy_url` at (P10b handoff).
    pub fn local_addr(&self) -> SocketAddr {
        self.local_addr
    }

    /// `socks5h://127.0.0.1:<port>` — the value P6 sets as a creation-time
    /// `proxy_url` (P10b). `socks5h` (not `socks5`) so the webview forwards the
    /// hostname for remote resolution.
    pub fn proxy_url(&self) -> String {
        format!("socks5h://{}", self.local_addr)
    }

    /// Replace the backing channel opener after a reconnect — the port is
    /// preserved so overlays need no `proxy_url` change.
    pub fn swap_opener(&self, opener: Arc<dyn ChannelOpener>) {
        *self.opener.lock().unwrap() = opener;
    }
}

impl Drop for SocksBroker {
    fn drop(&mut self) {
        self.accept_task.abort();
    }
}

async fn accept_loop(listener: TcpListener, opener: Arc<Mutex<Arc<dyn ChannelOpener>>>) {
    loop {
        match listener.accept().await {
            Ok((stream, _peer)) => {
                let opener = opener.lock().unwrap().clone();
                tokio::spawn(async move {
                    // Per-connection failures are expected (client aborts, remote
                    // refused) and must never crash the accept loop; discard.
                    let _ = handle_connection(stream, opener).await;
                });
            }
            // A transient accept error must not kill the broker; yield and retry.
            Err(_) => tokio::task::yield_now().await,
        }
    }
}

/// One client connection: negotiate, parse CONNECT, open the remote tunnel, then
/// pipe bytes both ways. The hostname from a DOMAINNAME request is passed to the
/// opener verbatim — the remote resolves it (socks5h).
async fn handle_connection(
    mut client: TcpStream,
    opener: Arc<dyn ChannelOpener>,
) -> Result<(), String> {
    socks::negotiate_auth(&mut client)
        .await
        .map_err(|e| e.to_string())?;

    let target = match socks::read_connect_request(&mut client).await {
        Ok(t) => t,
        Err(e) => {
            let _ = socks::write_reply(&mut client, socks::reply_code_for(&e)).await;
            return Err(e.to_string());
        }
    };

    pipe_target(client, target, opener).await
}

/// Split out so the open→reply→copy path is testable over any stream.
pub(crate) async fn pipe_target<S>(
    mut client: S,
    target: TargetAddr,
    opener: Arc<dyn ChannelOpener>,
) -> Result<(), String>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    let mut tunnel = match opener.open(target.host(), target.port()).await {
        Ok(t) => t,
        Err(e) => {
            let _ = socks::write_reply(&mut client, ReplyCode::ConnectionRefused).await;
            return Err(format!("open tunnel to {target} failed: {e}"));
        }
    };

    socks::write_reply(&mut client, ReplyCode::Succeeded)
        .await
        .map_err(|e| e.to_string())?;

    copy_bidirectional(&mut client, &mut tunnel)
        .await
        .map(|_| ())
        .map_err(|e| format!("pipe to {target} closed: {e}"))
}

#[cfg(test)]
#[path = "socks_broker_tests.rs"]
mod tests;
