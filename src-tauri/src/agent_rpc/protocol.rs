//! RPC wire protocol (P13): request/response schema, length-prefixed framing, and
//! the stable error model.
//!
//! Framing is a 4-byte big-endian `u32` length header followed by exactly that
//! many JSON bytes. A snapshot can be large, so the header carries the full body
//! length and the reader pulls the whole frame before parsing (no line-delimited
//! assumptions that would break on embedded newlines).
//!
//! The auth token rides INSIDE the framed JSON envelope ([`RpcRequest::token`]),
//! never on the CLI argv — a process list must not leak it.

use serde::{Deserialize, Serialize};
use std::io;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

/// Upper bound on a single frame so a malformed/hostile length header can't make
/// the server pre-allocate unbounded memory. 64 MiB comfortably fits a large DOM
/// snapshot while staying a hard ceiling.
pub const MAX_FRAME_BYTES: usize = 64 * 1024 * 1024;

/// Which browser pane an automation op targets. Defaults to [`PaneTarget::Focused`]
/// so an agent with a single browser pane needs no `--pane`; the dispatcher errors
/// on ambiguity (multiple panes, none focused).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PaneTarget {
    /// Resolve to the currently focused browser pane.
    Focused,
    /// An explicit pane id (from `weft browser ... --pane <id>`).
    Pane { id: String },
}

impl Default for PaneTarget {
    fn default() -> Self {
        PaneTarget::Focused
    }
}

/// Which property `get` reads off the element behind a ref. Mirrors P7's
/// `automation::GetKind` (kept as a protocol-local copy so the standalone CLI,
/// which does not link the app crate, shares one wire vocabulary). Wire tokens
/// are snake_case and must match P7's `GetKind` so Wave-3 dispatch maps 1:1.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GetKind {
    Text,
    Html,
    Value,
    Attr,
    Box,
    Styles,
    Count,
}

/// One browser automation operation. Mirrors the cmux `browser` verb set so agent
/// scripts port over. `ref` is an ephemeral element ref (`eN`) from a prior
/// snapshot.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum BrowserAction {
    Snapshot,
    Click {
        #[serde(rename = "ref")]
        reference: String,
    },
    Fill {
        #[serde(rename = "ref")]
        reference: String,
        text: String,
    },
    Eval {
        js: String,
    },
    Wait {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        selector: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        timeout_ms: Option<u64>,
    },
    /// Read a property off the element behind `ref` (P7 `browser_get` parity:
    /// text|html|value|attr|box|styles|count). `attr` names the attribute when
    /// `kind = attr`.
    Get {
        #[serde(rename = "ref")]
        reference: String,
        kind: GetKind,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        attr: Option<String>,
    },
    Find {
        query: String,
    },
}

/// The domain-routed command carried by an [`RpcRequest`]. `notify`/`ssh` are
/// skeletons P5/P10 flesh out; their wire shape is reserved here so the single
/// `weft` binary speaks one protocol.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "domain", rename_all = "snake_case")]
pub enum Command {
    Browser {
        #[serde(default)]
        target: PaneTarget,
        action: BrowserAction,
    },
    Notify {
        message: String,
    },
    Ssh {
        destination: String,
    },
}

/// A framed request: the session token plus the flattened command. `token` is
/// verified before the command is dispatched.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RpcRequest {
    pub token: String,
    #[serde(flatten)]
    pub command: Command,
}

/// Stable, machine-readable error. `code` is a fixed string an agent script can
/// branch on; `message` is human context (never the token or other secrets).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ErrorModel {
    pub code: String,
    pub message: String,
}

impl ErrorModel {
    pub fn new(code: impl Into<String>, message: impl Into<String>) -> Self {
        ErrorModel {
            code: code.into(),
            message: message.into(),
        }
    }
}

/// The framed reply. `status` discriminates success (`data`) from failure
/// (`error`) so the CLI can map it to an exit code without guessing.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum RpcResponse {
    Ok { data: serde_json::Value },
    Error { error: ErrorModel },
}

impl RpcResponse {
    pub fn ok(data: serde_json::Value) -> Self {
        RpcResponse::Ok { data }
    }

    pub fn error(error: ErrorModel) -> Self {
        RpcResponse::Error { error }
    }
}

/// Write `payload` as one length-prefixed frame and flush. Errors if the payload
/// exceeds [`MAX_FRAME_BYTES`] (would overflow the `u32` contract anyway).
pub async fn write_frame<W: AsyncWrite + Unpin>(writer: &mut W, payload: &[u8]) -> io::Result<()> {
    if payload.len() > MAX_FRAME_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "frame exceeds maximum size",
        ));
    }
    let len = payload.len() as u32;
    writer.write_all(&len.to_be_bytes()).await?;
    writer.write_all(payload).await?;
    writer.flush().await?;
    Ok(())
}

/// Read exactly one length-prefixed frame. Rejects a header claiming more than
/// [`MAX_FRAME_BYTES`] before allocating.
pub async fn read_frame<R: AsyncRead + Unpin>(reader: &mut R) -> io::Result<Vec<u8>> {
    let mut len_buf = [0u8; 4];
    reader.read_exact(&mut len_buf).await?;
    let len = u32::from_be_bytes(len_buf) as usize;
    if len > MAX_FRAME_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "frame length exceeds maximum size",
        ));
    }
    let mut buf = vec![0u8; len];
    reader.read_exact(&mut buf).await?;
    Ok(buf)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The protocol's `GetKind` is a deliberate standalone-CLI-friendly mirror of
    /// `automation::GetKind` (the CLI crate can't link the app crate). The two live
    /// in separate compilation units with no shared type, so nothing but this test
    /// stops them drifting. Lock every variant's wire token to P7's `as_js_kind()`
    /// so Wave-3 dispatch (protocol Get → automation get) maps 1:1 byte-for-byte.
    #[test]
    fn get_kind_tokens_match_automation_get_kind() {
        use crate::command_registry::automation::GetKind as AutoKind;

        // (protocol variant, automation variant) — must serialize to the same token.
        let pairs = [
            (GetKind::Text, AutoKind::Text),
            (GetKind::Html, AutoKind::Html),
            (GetKind::Value, AutoKind::Value),
            (GetKind::Attr, AutoKind::Attr),
            (GetKind::Box, AutoKind::Box),
            (GetKind::Styles, AutoKind::Styles),
            (GetKind::Count, AutoKind::Count),
        ];
        for (proto, auto) in pairs {
            let proto_token = serde_json::to_value(proto).unwrap();
            let proto_token = proto_token.as_str().unwrap();
            // automation::GetKind serde token and its as_js_kind() are the inject
            // wire token P7 switches on; both must equal the protocol token.
            assert_eq!(
                proto_token,
                auto.as_js_kind(),
                "protocol GetKind token diverged from automation::GetKind"
            );
            assert_eq!(
                serde_json::to_value(auto).unwrap().as_str().unwrap(),
                proto_token,
                "automation::GetKind serde token diverged from protocol GetKind"
            );
        }
    }
}
