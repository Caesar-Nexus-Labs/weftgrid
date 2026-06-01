//! Inject-script wiring (P7). Builds the page-context command call that the inject
//! bundle exposes as `window.__weft.dispatch(...)`, and parses the JSON the inject
//! script round-trips back to Rust.
//!
//! Works on BOTH OS. The inject bundle is identical (`crate::inject_asset`); the
//! only OS difference is value return: WebView2 can read the synchronous return of
//! `evaluate_script`, while WebKitGTK's `evaluate_script` is historically
//! fire-and-forget, so the bundle ALSO posts the same JSON over the IPC bridge
//! (`window.__weft.postMessage`). Both paths carry the same command `id`.
//!
//! This module is intentionally decoupled from the P6 browser pane: it produces a
//! script string + parses a reply, so the core is testable against fixtures
//! without a live webview.

use serde::Serialize;
use serde_json::Value;

use super::{normalize_js_value, GetKind, Snapshot, WaitCond};

/// The embedded inject bundle (single source, both OS).
pub use crate::inject_asset::INJECT_SNAPSHOT_JS;

/// A command sent to the inject runtime. Serialized as the argument object to
/// `window.__weft.dispatch(<json>)`. `id` correlates the IPC round-trip reply.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InjectCommand {
    pub id: u64,
    pub action: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub r#ref: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub selector: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub attr: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub wait: Option<WaitCond>,
}

impl InjectCommand {
    fn base(id: u64, action: &str) -> Self {
        InjectCommand {
            id,
            action: action.to_string(),
            r#ref: None,
            selector: None,
            text: None,
            attr: None,
            kind: None,
            wait: None,
        }
    }

    pub fn snapshot(id: u64) -> Self {
        Self::base(id, "snapshot")
    }

    pub fn click(id: u64, r: impl Into<String>) -> Self {
        Self {
            r#ref: Some(r.into()),
            ..Self::base(id, "click")
        }
    }

    pub fn fill(id: u64, r: impl Into<String>, text: impl Into<String>) -> Self {
        Self {
            r#ref: Some(r.into()),
            text: Some(text.into()),
            ..Self::base(id, "fill")
        }
    }

    pub fn get(id: u64, r: impl Into<String>, kind: GetKind, attr: Option<String>) -> Self {
        Self {
            r#ref: Some(r.into()),
            kind: Some(kind.as_js_kind().to_string()),
            attr,
            ..Self::base(id, "get")
        }
    }

    pub fn find(id: u64, selector: impl Into<String>) -> Self {
        Self {
            selector: Some(selector.into()),
            ..Self::base(id, "find")
        }
    }

    pub fn wait(id: u64, cond: WaitCond) -> Self {
        Self {
            wait: Some(cond),
            ..Self::base(id, "wait")
        }
    }
}

/// Render the `evaluate_script` body that invokes the inject runtime for one
/// command. Assumes the bundle has already installed `window.__weft` (inject it
/// once per page load via [`INJECT_SNAPSHOT_JS`]).
pub fn render_dispatch_script(cmd: &InjectCommand) -> Result<String, String> {
    let json = serde_json::to_string(cmd).map_err(|e| e.to_string())?;
    // The bundle returns the JSON string synchronously AND posts it over IPC.
    Ok(format!("window.__weft && window.__weft.dispatch({json})"))
}

/// One-time bootstrap script: the embedded bundle plus a no-op tail so the
/// caller can `evaluate_script` it on page load to install `window.__weft`.
pub fn bootstrap_script() -> &'static str {
    INJECT_SNAPSHOT_JS
}

/// The reply envelope the inject runtime produces (`{id, ok, result|error}`).
#[derive(Debug, Clone, PartialEq)]
pub struct InjectReply {
    pub id: u64,
    pub ok: bool,
    pub result: Value,
    pub error: Option<String>,
}

/// Parse the JSON the inject runtime returns (either the synchronous
/// `evaluate_script` return on Windows or the `postMessage` payload on Linux).
pub fn parse_reply(raw: &str) -> Result<InjectReply, String> {
    let v: Value = serde_json::from_str(raw).map_err(|e| format!("invalid inject reply: {e}"))?;
    let id = v.get("id").and_then(|x| x.as_u64()).unwrap_or(0);
    let ok = v.get("ok").and_then(|x| x.as_bool()).unwrap_or(false);
    if !ok {
        let error = v
            .get("error")
            .and_then(|x| x.as_str())
            .unwrap_or("unknown inject error")
            .to_string();
        return Ok(InjectReply {
            id,
            ok,
            result: Value::Null,
            error: Some(error),
        });
    }
    let result = normalize_js_value(v.get("result").cloned().unwrap_or(Value::Null));
    Ok(InjectReply {
        id,
        ok,
        result,
        error: None,
    })
}

/// Parse a `snapshot` reply into the strongly-typed [`Snapshot`].
pub fn parse_snapshot_reply(raw: &str) -> Result<Snapshot, String> {
    let reply = parse_reply(raw)?;
    if !reply.ok {
        return Err(reply.error.unwrap_or_else(|| "snapshot failed".into()));
    }
    serde_json::from_value(reply.result).map_err(|e| format!("bad snapshot payload: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn dispatch_script_embeds_command_json() {
        let cmd = InjectCommand::click(7, "e3");
        let script = render_dispatch_script(&cmd).unwrap();
        assert!(script.contains("window.__weft.dispatch("));
        assert!(script.contains("\"action\":\"click\""));
        assert!(script.contains("\"ref\":\"e3\""));
        assert!(script.contains("\"id\":7"));
    }

    #[test]
    fn fill_command_carries_text() {
        let cmd = InjectCommand::fill(1, "e1", "hello");
        let json = serde_json::to_value(&cmd).unwrap();
        assert_eq!(json["action"], "fill");
        assert_eq!(json["text"], "hello");
    }

    #[test]
    fn get_command_maps_kind_token() {
        let cmd = InjectCommand::get(2, "e1", GetKind::Box, None);
        let json = serde_json::to_value(&cmd).unwrap();
        assert_eq!(json["kind"], "box");
        assert!(json.get("attr").is_none());
    }

    #[test]
    fn parse_reply_ok_normalizes_result() {
        let raw = json!({ "id": 4, "ok": true, "result": { "value": 42 } }).to_string();
        let reply = parse_reply(&raw).unwrap();
        assert_eq!(reply.id, 4);
        assert!(reply.ok);
        assert_eq!(reply.result["value"], 42);
    }

    #[test]
    fn parse_reply_error_surfaces_message() {
        let raw = json!({ "id": 5, "ok": false, "error": "not_found" }).to_string();
        let reply = parse_reply(&raw).unwrap();
        assert!(!reply.ok);
        assert_eq!(reply.error.as_deref(), Some("not_found"));
    }

    #[test]
    fn parse_snapshot_reply_builds_typed_snapshot() {
        let raw = json!({
            "id": 1,
            "ok": true,
            "result": {
                "title": "Demo",
                "url": "https://x/",
                "readyState": "complete",
                "text": "Go",
                "html": "<html></html>",
                "snapshotText": "- document \"Demo\"\n- button \"Go\" [ref=e1]",
                "refs": { "e1": { "role": "button", "name": "Go" } },
                "entries": []
            }
        })
        .to_string();
        let snap = parse_snapshot_reply(&raw).unwrap();
        assert_eq!(snap.title, "Demo");
        assert!(snap.snapshot_text.contains("[ref=e1]"));
        assert_eq!(snap.refs.get("e1").unwrap().role, "button");
    }

    #[test]
    fn bootstrap_script_contains_marker() {
        assert!(bootstrap_script().contains("weftgrid-inject-stub"));
    }
}
