//! SSH track Tauri commands (P10a).
//!
//! Thin bridge between the frontend and [`SshManager`]. Commands:
//!   - `ssh_connect`   — open a remote workspace's transport; returns the
//!     `socks5h://` proxy URL P6 overlays bind at creation time (P10b).
//!   - `ssh_disconnect`— tear down a connection.
//!   - `ssh_status`    — poll connection status (Connected/Reconnecting/Failed)
//!     for the UI.
//!
//! Register in `command_registry::register_all`'s `generate_handler!`:
//!   ssh::commands::ssh_connect, ssh_disconnect, ssh_status.
//!
//! Secret hygiene: the password (when used) arrives over IPC, is moved into the
//! manager's in-memory credential store for reconnect re-auth, and is never
//! echoed back or persisted.

use serde::{Deserialize, Serialize};
use tauri::State;

use super::config::{AuthMethod, SshConnectParams, DEFAULT_SSH_PORT};
use super::reconnect::ConnectionStatus;
use super::session::SshManager;

/// Wire form of the auth choice (camelCase). `password` is write-only — it is
/// consumed into the manager and never returned by any command.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", tag = "kind")]
pub enum AuthInput {
    Password {
        password: String,
    },
    KeyFile {
        path: String,
        passphrase: Option<String>,
    },
}

impl From<AuthInput> for AuthMethod {
    fn from(input: AuthInput) -> Self {
        match input {
            AuthInput::Password { password } => AuthMethod::Password(password),
            AuthInput::KeyFile { path, passphrase } => AuthMethod::KeyFile { path, passphrase },
        }
    }
}

/// Result of a successful connect: the broker endpoint the UI hands to P6.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SshConnectResult {
    /// `socks5h://127.0.0.1:<port>` — set as the overlay `proxy_url` (P10b).
    pub proxy_url: String,
    pub proxy_port: u16,
}

/// Connect to a remote and start its SOCKS5h broker.
///
/// `id` is the caller's handle (typically the workspace id). `destination` is
/// `user@host`. `port` defaults to 22.
#[tauri::command]
pub async fn ssh_connect(
    manager: State<'_, SshManager>,
    id: String,
    destination: String,
    port: Option<u16>,
    auth: AuthInput,
) -> Result<SshConnectResult, String> {
    let (username, host) = super::config::parse_destination(&destination)?;
    let params = SshConnectParams {
        host,
        port: port.unwrap_or(DEFAULT_SSH_PORT),
        username,
        auth: auth.into(),
    };
    let proxy_url = manager.connect(id.clone(), params).await?;
    let proxy_port = manager
        .proxy_port(&id)
        .ok_or_else(|| "broker port missing after connect".to_string())?;
    Ok(SshConnectResult {
        proxy_url,
        proxy_port,
    })
}

/// Tear down a remote connection (stops broker + supervisor).
#[tauri::command]
pub fn ssh_disconnect(manager: State<'_, SshManager>, id: String) -> Result<(), String> {
    manager.disconnect(&id)
}

/// Poll a connection's status for the UI (Connecting/Connected/Reconnecting/
/// Failed/Disconnected). Returns `Disconnected` for an unknown id.
#[tauri::command]
pub fn ssh_status(manager: State<'_, SshManager>, id: String) -> ConnectionStatus {
    manager
        .status(&id)
        .unwrap_or(ConnectionStatus::Disconnected)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn auth_input_password_maps_and_redacts() {
        let json = r#"{"kind":"password","password":"hunter2"}"#;
        let input: AuthInput = serde_json::from_str(json).unwrap();
        let method: AuthMethod = input.into();
        // Debug must never leak the password (config::AuthMethod redacts).
        assert!(!format!("{method:?}").contains("hunter2"));
    }

    #[test]
    fn auth_input_keyfile_maps() {
        let json = r#"{"kind":"keyFile","path":"/home/u/.ssh/id_ed25519","passphrase":null}"#;
        let input: AuthInput = serde_json::from_str(json).unwrap();
        match input.into() {
            AuthMethod::KeyFile { path, passphrase } => {
                assert_eq!(path, "/home/u/.ssh/id_ed25519");
                assert!(passphrase.is_none());
            }
            _ => panic!("expected KeyFile"),
        }
    }

    #[test]
    fn connect_result_serializes_camel_case() {
        let r = SshConnectResult {
            proxy_url: "socks5h://127.0.0.1:5000".into(),
            proxy_port: 5000,
        };
        let v = serde_json::to_value(&r).unwrap();
        assert_eq!(v["proxyUrl"], "socks5h://127.0.0.1:5000");
        assert_eq!(v["proxyPort"], 5000);
    }
}
