//! SSH connection params + auth method (P10a).
//!
//! Bridges the persisted [`RemoteConfiguration`] contract (P2 `model::workspace`)
//! into the concrete inputs `client` needs: a host:port, a username, and an auth
//! method. The `user@host` form in `RemoteConfiguration.destination` is parsed
//! here so callers don't re-implement it.
//!
//! Secret hygiene: a password lives only in memory (held for one reconnect
//! re-auth) and is never serialized or logged. `AuthMethod`'s `Debug` redacts it.

use std::fmt;

use crate::model::{RemoteConfiguration, RemoteTransport};

/// Default SSH port when `RemoteConfiguration.port` is unset.
pub const DEFAULT_SSH_PORT: u16 = 22;

/// How to authenticate. Windows MVP = key file + password; agent auth is a
/// later add (named-pipe spike — see mod-level note). Password is redacted in
/// `Debug` so it never leaks into logs.
#[derive(Clone)]
pub enum AuthMethod {
    /// Private key file on disk (optionally encrypted with `passphrase`).
    KeyFile {
        path: String,
        passphrase: Option<String>,
    },
    /// Interactive password (kept in memory only for the session's reconnects).
    Password(String),
}

impl fmt::Debug for AuthMethod {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AuthMethod::KeyFile { path, passphrase } => f
                .debug_struct("KeyFile")
                .field("path", path)
                .field("passphrase", &passphrase.as_ref().map(|_| "<redacted>"))
                .finish(),
            AuthMethod::Password(_) => write!(f, "Password(<redacted>)"),
        }
    }
}

/// Everything `client::connect_and_auth` needs. Built from a
/// [`RemoteConfiguration`] + an [`AuthMethod`] resolved at connect time (the
/// model never stores the password).
#[derive(Debug, Clone)]
pub struct SshConnectParams {
    pub host: String,
    pub port: u16,
    pub username: String,
    pub auth: AuthMethod,
}

impl SshConnectParams {
    /// Derive connect params from the workspace contract. The `auth` is supplied
    /// separately (resolved from UI/key file at connect time). Returns `Err` if
    /// the transport isn't SSH or `destination` lacks a `user@host`.
    pub fn from_remote_config(cfg: &RemoteConfiguration, auth: AuthMethod) -> Result<Self, String> {
        if cfg.transport != RemoteTransport::Ssh {
            return Err(format!("not an SSH transport: {:?}", cfg.transport));
        }
        let (username, host) = parse_destination(&cfg.destination)?;
        // identity_file in the config implies a key file when no explicit auth
        // was given is handled by the caller; here `auth` already encodes it.
        Ok(SshConnectParams {
            host,
            port: cfg.port.unwrap_or(DEFAULT_SSH_PORT),
            username,
            auth,
        })
    }

    /// `host:port` for `russh::client::connect`.
    pub fn socket_addr(&self) -> String {
        format!("{}:{}", self.host, self.port)
    }
}

/// Split `user@host` → `(user, host)`. Requires an explicit user (no OS-login
/// fallback in MVP — keep it explicit). Rejects empty parts and missing `@`.
pub fn parse_destination(destination: &str) -> Result<(String, String), String> {
    let dest = destination.trim();
    let (user, host) = dest
        .rsplit_once('@')
        .ok_or_else(|| format!("destination must be user@host, got {dest:?}"))?;
    if user.is_empty() || host.is_empty() {
        return Err(format!("destination must be user@host, got {dest:?}"));
    }
    Ok((user.to_string(), host.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::RemoteTransport;

    fn ssh_config(dest: &str, port: Option<u16>) -> RemoteConfiguration {
        RemoteConfiguration {
            transport: RemoteTransport::Ssh,
            destination: dest.to_string(),
            port,
            identity_file: None,
            ssh_options: vec![],
            local_proxy_port: None,
            terminal_startup_command: None,
        }
    }

    #[test]
    fn parses_user_at_host() {
        assert_eq!(
            parse_destination("alice@example.com").unwrap(),
            ("alice".to_string(), "example.com".to_string())
        );
    }

    #[test]
    fn rejects_destination_without_user() {
        assert!(parse_destination("example.com").is_err());
        assert!(parse_destination("@example.com").is_err());
        assert!(parse_destination("alice@").is_err());
    }

    #[test]
    fn from_remote_config_uses_default_port() {
        let cfg = ssh_config("bob@host.internal", None);
        let p =
            SshConnectParams::from_remote_config(&cfg, AuthMethod::Password("pw".into())).unwrap();
        assert_eq!(p.username, "bob");
        assert_eq!(p.host, "host.internal");
        assert_eq!(p.port, DEFAULT_SSH_PORT);
        assert_eq!(p.socket_addr(), "host.internal:22");
    }

    #[test]
    fn from_remote_config_honors_explicit_port() {
        let cfg = ssh_config("bob@host", Some(2222));
        let p = SshConnectParams::from_remote_config(
            &cfg,
            AuthMethod::KeyFile {
                path: "/k".into(),
                passphrase: None,
            },
        )
        .unwrap();
        assert_eq!(p.port, 2222);
        assert_eq!(p.socket_addr(), "host:2222");
    }

    #[test]
    fn rejects_non_ssh_transport() {
        let mut cfg = ssh_config("bob@host", None);
        cfg.transport = RemoteTransport::Websocket;
        let res = SshConnectParams::from_remote_config(&cfg, AuthMethod::Password("x".into()));
        assert!(res.is_err());
    }

    #[test]
    fn password_is_redacted_in_debug() {
        let dbg = format!("{:?}", AuthMethod::Password("hunter2".into()));
        assert!(!dbg.contains("hunter2"), "password leaked in Debug: {dbg}");
        assert!(dbg.contains("redacted"));

        let dbg = format!(
            "{:?}",
            AuthMethod::KeyFile {
                path: "/id".into(),
                passphrase: Some("secret".into())
            }
        );
        assert!(!dbg.contains("secret"));
        assert!(dbg.contains("/id"), "key path should be visible: {dbg}");
    }
}
