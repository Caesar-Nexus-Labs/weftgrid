//! SSH remote workspace transport (P10a, owner: `src-tauri/src/ssh/**`).
//!
//! Wave-1 CORE: a russh-backed transport + a local **SOCKS5h** proxy broker that
//! forwards browser/TCP traffic through the SSH connection with **remote DNS
//! resolution** (the S6 hard-gate). No UI dependency — P10b (Wave-3) wires the
//! broker's `proxy_url` into P6 browser overlays and a remote shell into P3.
//!
//! Module map:
//! - [`socks`] — SOCKS5 wire protocol parse/reply (socks5h: domain kept raw, never locally resolved).
//! - [`socks_broker`] — loopback-only listener + `ChannelOpener` boundary.
//! - [`client`] — russh connect/auth + the `direct-tcpip` opener (ring backend).
//! - [`config`] — `RemoteConfiguration` → `SshConnectParams` + auth.
//! - [`host_key`] — accept-new host-key policy (change-detect on reconnect).
//! - [`reconnect`] — deterministic backoff/retry state machine.
//! - [`session`] — `SshManager`: connect + broker + reconnect supervisor.
//! - [`commands`] — Tauri `ssh_connect`/`ssh_disconnect`/`ssh_status`.
//!
//! ## Proxy-endpoint surface for P10b (Wave-3 handoff)
//! After `ssh_connect`, `SshManager` exposes a STABLE loopback endpoint per
//! connection:
//! - `proxy_url(id)` → `socks5h://127.0.0.1:<port>` — set as the P6 overlay
//!   `proxy_url` at creation time (do not reconfigure post-init).
//! - `proxy_port(id)` → the bound port; preserved across reconnects, so overlays
//!   never need their `proxy_url` changed when a session drops/recovers.
//! - `status(id)` → `ConnectionStatus` for the UI to render reconnecting/failed.
//!
//! On reconnect the broker's backing channel opener is swapped in place — same
//! port, new SSH session — so P10b only needs to re-attach the shell channel.
//!
//! ## Auth scope (Windows-first, spec M6)
//! MVP supports **key file + password** on all platforms. ssh-agent auth
//! (Windows named-pipe Pageant/OpenSSH spike) is deferred — `AuthMethod` has no
//! agent variant yet; add it when the spike lands.
//!
//! ## Anti-HOL note (spec M5)
//! One session backs the SOCKS broker here. The interactive shell (P10b) should
//! open a SECOND russh session so page-load traffic can't block keystrokes;
//! `SshManager` is shaped to host that later.

use tauri::{Builder, Runtime};

pub mod client;
pub mod commands;
pub mod config;
pub mod host_key;
pub mod reconnect;
pub mod session;
pub mod socks;
pub mod socks_broker;

#[allow(unused_imports)]
pub use config::{AuthMethod, SshConnectParams};
#[allow(unused_imports)]
pub use reconnect::ConnectionStatus;
#[allow(unused_imports)]
pub use session::SshManager;
#[allow(unused_imports)]
pub use socks_broker::{ChannelOpener, SocksBroker};

/// Additive setup: register the `.manage()`d [`SshManager`]. No `invoke_handler`
/// (last-wins → commands listed centrally in `command_registry`).
pub fn register<R: Runtime>(builder: Builder<R>) -> Builder<R> {
    builder.manage(SshManager::new())
}
