//! SSH connection manager + reconnect supervisor (P10a).
//!
//! Owns live connections keyed by id. Each connection = one authenticated
//! [`SshSession`] + one loopback [`SocksBroker`] + a background supervisor task
//! that watches for drops and re-establishes (spec H3: reconnect is routine).
//!
//! HOL-latency note (spec M5): P10a establishes ONE session backing the SOCKS
//! broker. The interactive shell (Session A) is wired in P10b and SHOULD use a
//! *separate* russh session so a heavy page-load on the broker session can't
//! block keystrokes. The manager is structured to host that second session
//! later; only the broker session exists now (no shell consumer yet — YAGNI).
//!
//! Credentials: [`SshConnectParams`] (incl. the in-memory password/key) are held
//! for the connection's life so the supervisor can re-auth a reconnect without
//! re-prompting. Never serialized, never logged (see `config::AuthMethod`).

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use tokio::task::JoinHandle;

use super::client::{connect_and_auth, SshSession};
use super::config::SshConnectParams;
use super::host_key::HostKeyStore;
use super::reconnect::{ConnectionStatus, ReconnectPolicy, ReconnectSupervisor};
use super::socks_broker::SocksBroker;

/// How often the supervisor polls the session for liveness.
const LIVENESS_POLL: Duration = Duration::from_millis(500);

/// A live SSH connection: its broker endpoint, current status, and the
/// supervisor task. Dropping it aborts the supervisor and (via `SocksBroker`'s
/// own `Drop`) the broker accept loop.
pub struct SshConnection {
    proxy_url: String,
    proxy_port: u16,
    status: Arc<Mutex<ConnectionStatus>>,
    supervisor: JoinHandle<()>,
    _broker: Arc<SocksBroker>,
}

impl SshConnection {
    pub fn proxy_url(&self) -> String {
        self.proxy_url.clone()
    }
    pub fn proxy_port(&self) -> u16 {
        self.proxy_port
    }
    pub fn status(&self) -> ConnectionStatus {
        self.status.lock().unwrap().clone()
    }
}

impl Drop for SshConnection {
    fn drop(&mut self) {
        self.supervisor.abort();
    }
}

/// `.manage()`d SSH state: id → live connection. One per remote workspace.
#[derive(Default)]
pub struct SshManager {
    connections: Mutex<HashMap<String, SshConnection>>,
    policy: ReconnectPolicy,
}

impl SshManager {
    pub fn new() -> Self {
        Self::default()
    }

    /// Connect + authenticate + bind the SOCKS broker, then spawn the supervisor.
    /// Returns the `socks5h://` proxy URL P6 overlays bind at creation time
    /// (P10b). Replaces any existing connection with the same id.
    pub async fn connect(&self, id: String, params: SshConnectParams) -> Result<String, String> {
        let store = Arc::new(HostKeyStore::new());
        let session = connect_and_auth(&params, store.clone()).await?;

        let broker = Arc::new(SocksBroker::bind(session.channel_opener()).await?);
        let proxy_url = broker.proxy_url();
        let proxy_port = broker.local_addr().port();

        let status = Arc::new(Mutex::new(ConnectionStatus::Connected));
        let supervisor = tokio::spawn(supervise(
            params,
            store,
            broker.clone(),
            status.clone(),
            session,
            self.policy.clone(),
        ));

        let conn = SshConnection {
            proxy_url: proxy_url.clone(),
            proxy_port,
            status,
            supervisor,
            _broker: broker,
        };
        self.connections.lock().unwrap().insert(id, conn);
        Ok(proxy_url)
    }

    /// Tear down a connection (user-initiated). Idempotent.
    pub fn disconnect(&self, id: &str) -> Result<(), String> {
        self.connections
            .lock()
            .unwrap()
            .remove(id)
            .map(|_| ())
            .ok_or_else(|| format!("no ssh connection: {id}"))
    }

    /// Current status for a connection, if it exists.
    pub fn status(&self, id: &str) -> Option<ConnectionStatus> {
        self.connections.lock().unwrap().get(id).map(|c| c.status())
    }

    /// The broker proxy URL for a connection (P10b reads this to bind overlays).
    pub fn proxy_url(&self, id: &str) -> Option<String> {
        self.connections
            .lock()
            .unwrap()
            .get(id)
            .map(|c| c.proxy_url())
    }

    /// The stable broker port (preserved across reconnects).
    pub fn proxy_port(&self, id: &str) -> Option<u16> {
        self.connections
            .lock()
            .unwrap()
            .get(id)
            .map(|c| c.proxy_port())
    }

    pub fn is_connected(&self, id: &str) -> bool {
        matches!(self.status(id), Some(ConnectionStatus::Connected))
    }
}

/// Supervisor loop: watch the session, reconnect on drop with backoff, swap the
/// broker's opener so the **stable** broker port keeps serving (overlays need no
/// `proxy_url` change). Pure decisions come from [`ReconnectSupervisor`].
async fn supervise(
    params: SshConnectParams,
    store: Arc<HostKeyStore>,
    broker: Arc<SocksBroker>,
    status: Arc<Mutex<ConnectionStatus>>,
    mut session: SshSession,
    policy: ReconnectPolicy,
) {
    let mut sup = ReconnectSupervisor::new(policy);
    sup.record_connected();
    publish(&status, &sup);

    loop {
        // Wait until the current session drops.
        while !session.is_closed() {
            tokio::time::sleep(LIVENESS_POLL).await;
        }

        // Dropped — schedule the first retry (or fail if none remain).
        let mut backoff = match sup.record_drop() {
            Some(b) => b,
            None => {
                publish(&status, &sup);
                return;
            }
        };
        publish(&status, &sup);

        // Retry with backoff until reconnected or exhausted.
        loop {
            tokio::time::sleep(backoff).await;
            match connect_and_auth(&params, store.clone()).await {
                Ok(new_session) => {
                    broker.swap_opener(new_session.channel_opener());
                    session = new_session;
                    sup.record_connected();
                    publish(&status, &sup);
                    break;
                }
                Err(_) => match sup.record_attempt_failed() {
                    Some(b) => {
                        backoff = b;
                        publish(&status, &sup);
                    }
                    None => {
                        publish(&status, &sup);
                        return;
                    }
                },
            }
        }
    }
}

fn publish(status: &Arc<Mutex<ConnectionStatus>>, sup: &ReconnectSupervisor) {
    *status.lock().unwrap() = sup.status().clone();
}
