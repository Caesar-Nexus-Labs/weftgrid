//! russh client: connect + auth + the production `direct-tcpip` channel opener
//! (P10a).
//!
//! Uses russh's **ring** crypto backend (configured in Cargo.toml; the aws-lc-rs
//! backend needs a C/NASM toolchain that fails to build on the Windows target).
//!
//! Two responsibilities:
//!   1. [`connect_and_auth`] — open a TCP/SSH transport, verify the host key
//!      (accept-new via [`super::host_key`]), authenticate (password or key
//!      file). Returns a cloneable session [`SshSession`].
//!   2. [`SshChannelOpener`] — the production [`ChannelOpener`] for the SOCKS
//!      broker. Each CONNECT opens a `direct-tcpip` channel with the **raw
//!      hostname** so the *remote* sshd resolves it (socks5h, S6 gate).
//!
//! Live connection tests need a real sshd, so they are gated behind
//! `WEFTGRID_SSH_LIVE=1` (mirrors P3's `WEFTGRID_PTY_LIVE`). The protocol/policy
//! logic (host-key, auth-method derivation, socks parsing, reconnect) is covered
//! deterministically by the unit tests in the sibling modules.

use std::sync::Arc;

use russh::client::{self, Handle};
use russh::keys::{load_secret_key, HashAlg, PrivateKeyWithHashAlg, PublicKey};

use super::config::{AuthMethod, SshConnectParams};
use super::host_key::{HostKeyDecision, HostKeyStore};
use super::socks_broker::{ChannelOpener, OpenFuture, TunnelStream};

/// Origin reported to the remote for `direct-tcpip` channels. The values are
/// informational (logged by sshd); loopback keeps it honest.
const ORIGIN_HOST: &str = "127.0.0.1";
const ORIGIN_PORT: u32 = 0;

/// russh client handler. Holds the host string + shared [`HostKeyStore`] so
/// `check_server_key` can apply accept-new and detect key changes across
/// reconnects.
pub struct ClientHandler {
    host: String,
    store: Arc<HostKeyStore>,
}

impl client::Handler for ClientHandler {
    type Error = russh::Error;

    async fn check_server_key(&mut self, key: &PublicKey) -> Result<bool, Self::Error> {
        let fingerprint = key.fingerprint(HashAlg::Sha256).to_string();
        match self.store.check(&self.host, &fingerprint) {
            HostKeyDecision::AcceptNew | HostKeyDecision::Match => Ok(true),
            // Reject a changed key — never auto-trust on reconnect.
            HostKeyDecision::Changed { .. } => Ok(false),
        }
    }
}

/// A live, authenticated SSH session. Cheaply cloneable (shares the underlying
/// russh `Handle` via `Arc`) so the SOCKS broker and a future shell channel can
/// each open channels off one transport.
#[derive(Clone)]
pub struct SshSession {
    handle: Arc<Handle<ClientHandler>>,
}

impl SshSession {
    /// `true` once the transport is gone (drop / keepalive timeout) — the
    /// reconnect supervisor polls this to detect a dropped session.
    pub fn is_closed(&self) -> bool {
        self.handle.is_closed()
    }

    /// Build the production SOCKS channel opener bound to this session.
    pub fn channel_opener(&self) -> Arc<dyn ChannelOpener> {
        Arc::new(SshChannelOpener {
            handle: self.handle.clone(),
        })
    }
}

/// Connect + verify host key + authenticate. `store` is shared with the session
/// supervisor so reconnects reuse the accept-new memory.
pub async fn connect_and_auth(
    params: &SshConnectParams,
    store: Arc<HostKeyStore>,
) -> Result<SshSession, String> {
    let config = Arc::new(client::Config::default());
    let handler = ClientHandler {
        host: params.host.clone(),
        store,
    };

    let mut handle = client::connect(config, params.socket_addr(), handler)
        .await
        .map_err(|e| format!("ssh connect to {} failed: {e}", params.socket_addr()))?;

    authenticate(&mut handle, params).await?;

    Ok(SshSession {
        handle: Arc::new(handle),
    })
}

async fn authenticate(
    handle: &mut Handle<ClientHandler>,
    params: &SshConnectParams,
) -> Result<(), String> {
    let result = match &params.auth {
        AuthMethod::Password(password) => handle
            .authenticate_password(&params.username, password.clone())
            .await
            .map_err(|e| format!("password auth error: {e}"))?,
        AuthMethod::KeyFile { path, passphrase } => {
            let key = load_secret_key(path, passphrase.as_deref())
                .map_err(|e| format!("load key {path}: {e}"))?;
            let key = PrivateKeyWithHashAlg::new(Arc::new(key), None);
            handle
                .authenticate_publickey(&params.username, key)
                .await
                .map_err(|e| format!("publickey auth error: {e}"))?
        }
    };

    if result.success() {
        Ok(())
    } else {
        // No secret in the message — just the method/user.
        Err(format!(
            "authentication rejected for {} (method {})",
            params.username,
            match &params.auth {
                AuthMethod::Password(_) => "password",
                AuthMethod::KeyFile { .. } => "publickey",
            }
        ))
    }
}

/// Production [`ChannelOpener`]: opens a `direct-tcpip` channel per CONNECT with
/// the raw hostname so the remote resolves DNS (socks5h).
struct SshChannelOpener {
    handle: Arc<Handle<ClientHandler>>,
}

impl ChannelOpener for SshChannelOpener {
    fn open(&self, host: String, port: u16) -> OpenFuture<'_> {
        let handle = self.handle.clone();
        Box::pin(async move {
            let channel = handle
                .channel_open_direct_tcpip(host, port as u32, ORIGIN_HOST, ORIGIN_PORT)
                .await
                .map_err(|e| format!("direct-tcpip open failed: {e}"))?;
            Ok(Box::new(channel.into_stream()) as TunnelStream)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::super::config::AuthMethod;
    use super::*;
    use crate::model::{RemoteConfiguration, RemoteTransport};

    fn live_params() -> Option<SshConnectParams> {
        // Gated: needs a real sshd. Set WEFTGRID_SSH_LIVE=1 plus the connection
        // env vars to exercise the russh connect/auth path end-to-end.
        if std::env::var("WEFTGRID_SSH_LIVE").is_err() {
            return None;
        }
        let dest = std::env::var("WEFTGRID_SSH_DEST").expect("WEFTGRID_SSH_DEST=user@host");
        let cfg = RemoteConfiguration {
            transport: RemoteTransport::Ssh,
            destination: dest,
            port: std::env::var("WEFTGRID_SSH_PORT")
                .ok()
                .and_then(|p| p.parse().ok()),
            identity_file: None,
            ssh_options: vec![],
            local_proxy_port: None,
            terminal_startup_command: None,
        };
        let auth = if let Ok(key) = std::env::var("WEFTGRID_SSH_KEY") {
            AuthMethod::KeyFile {
                path: key,
                passphrase: std::env::var("WEFTGRID_SSH_KEY_PASS").ok(),
            }
        } else {
            AuthMethod::Password(std::env::var("WEFTGRID_SSH_PASS").expect("WEFTGRID_SSH_PASS"))
        };
        Some(SshConnectParams::from_remote_config(&cfg, auth).unwrap())
    }

    #[tokio::test]
    async fn live_connect_and_open_channel() {
        let Some(params) = live_params() else {
            eprintln!("skipping live SSH test (set WEFTGRID_SSH_LIVE=1)");
            return;
        };
        let store = Arc::new(HostKeyStore::new());
        let session = connect_and_auth(&params, store)
            .await
            .expect("connect + auth");
        assert!(!session.is_closed());

        // S6 live form: resolve a remote target by hostname through the tunnel.
        let host = std::env::var("WEFTGRID_SSH_TARGET_HOST").unwrap_or_else(|_| "localhost".into());
        let port: u16 = std::env::var("WEFTGRID_SSH_TARGET_PORT")
            .ok()
            .and_then(|p| p.parse().ok())
            .unwrap_or(22);
        let opener = session.channel_opener();
        let result = opener.open(host, port).await.map(|_| ());
        assert!(
            result.is_ok(),
            "direct-tcpip open should succeed: {:?}",
            result.err()
        );
    }
}
