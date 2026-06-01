//! Browser automation (single inject-JS) track (P7 owner:
//! `src-tauri/src/automation/**`, `src/automation/**`).
//!
//! ONE DOM-walk algorithm (authored TS → IIFE JS, embedded via `include_str!`)
//! runs on BOTH WebView2 (Windows) and WebKitGTK (Linux) so ephemeral refs `eN`
//! and snapshot text are byte-identical cross-platform (agent scripts portable).
//! CDP (Windows, optional) is superset-only — never participates in snapshot/ref.
//!
//! `register` is additive-only (no `invoke_handler` — commands are listed once in
//! `command_registry` per the last-wins constraint).
//!
//! ## Ref model (defined ONCE)
//! A ref `eN` maps to a `:nth-of-type` CSS path captured at snapshot time. The
//! same path scheme is used on both OS — never a backend DOM node id. Refs are
//! ephemeral and session-local: each `snapshot()` resets the table, so a stale
//! ref from a prior snapshot is invalid. Callers MUST re-snapshot before acting
//! (the re-snapshot-before-act invariant) — `find()` extends the live table
//! without resetting it, keeping refs unique between snapshots.

use serde::{Deserialize, Serialize};
use tauri::{Builder, Runtime};

pub mod cdp_extras;
pub mod commands;
pub mod inject;

/// Ephemeral element handle (`eN`). Bare token, no leading `@`.
pub type Ref = String;

/// One node in the AX-relevant snapshot tree (mirrors the inject-JS entry shape).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SnapshotEntry {
    /// `:nth-of-type` CSS path resolved at snapshot time.
    pub selector: String,
    pub role: String,
    pub name: String,
    pub depth: u32,
}

/// Per-ref metadata surfaced alongside the snapshot text.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RefInfo {
    pub role: String,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub name: Option<String>,
}

/// Full snapshot payload round-tripped from the inject script (camelCase wire).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Snapshot {
    pub title: String,
    pub url: String,
    pub ready_state: String,
    pub text: String,
    pub html: String,
    /// Deterministic AX-tree text with inline `[ref=eN]` tokens.
    pub snapshot_text: String,
    /// `eN` → {role, name}. Map preserves no order; the text carries order.
    #[serde(default)]
    pub refs: std::collections::BTreeMap<Ref, RefInfo>,
    #[serde(default)]
    pub entries: Vec<SnapshotEntry>,
}

/// What `get(ref, kind)` should read off the element.
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

impl GetKind {
    /// Lowercase token the inject script's `get` action switches on.
    pub fn as_js_kind(self) -> &'static str {
        match self {
            GetKind::Text => "text",
            GetKind::Html => "html",
            GetKind::Value => "value",
            GetKind::Attr => "attr",
            GetKind::Box => "box",
            GetKind::Styles => "styles",
            GetKind::Count => "count",
        }
    }
}

/// A `wait` predicate. Exactly one variant is evaluated per call by the inject
/// script (`evalWaitCondition`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", tag = "kind", content = "value")]
pub enum WaitCond {
    /// Resolve when `document.querySelector(selector)` matches.
    Selector(String),
    /// Resolve when `location.href` contains the substring.
    UrlContains(String),
    /// Resolve when `body.innerText` contains the substring.
    TextContains(String),
    /// Resolve when `document.readyState` reaches the state.
    LoadState(String),
    /// Resolve when the JS expression is truthy.
    Function(String),
}

/// Coerce an arbitrary `serde_json::Value` returned by the inject bridge into a
/// stable shape (mirrors cmux `v2NormalizeJSValue`): `undefined` is encoded as a
/// tagged envelope, everything else passes through structurally. Keeps eval/get
/// results JSON-round-trippable without losing the undefined/null distinction.
pub fn normalize_js_value(value: serde_json::Value) -> serde_json::Value {
    use serde_json::Value;
    match value {
        // The bridge can't emit a literal `undefined`; an explicit tagged object
        // is the agreed encoding (see inject runtime / agent-browser parity).
        Value::Object(map) if is_undefined_envelope(&map) => Value::Object(
            [
                ("__weftType".to_string(), Value::String("undefined".into())),
                ("__weftValue".to_string(), Value::Null),
            ]
            .into_iter()
            .collect(),
        ),
        Value::Object(map) => Value::Object(
            map.into_iter()
                .map(|(k, v)| (k, normalize_js_value(v)))
                .collect(),
        ),
        Value::Array(arr) => Value::Array(arr.into_iter().map(normalize_js_value).collect()),
        other => other,
    }
}

fn is_undefined_envelope(map: &serde_json::Map<String, serde_json::Value>) -> bool {
    map.get("__weftType")
        .and_then(|v| v.as_str())
        .map(|s| s == "undefined")
        .unwrap_or(false)
}

/// Additive setup placeholder until the automation state/manager lands. Commands
/// live in [`commands`]; the inject bundle is embedded by `crate::inject_asset`.
pub fn register<R: Runtime>(builder: Builder<R>) -> Builder<R> {
    builder
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn get_kind_js_tokens_match_inject_runtime() {
        assert_eq!(GetKind::Box.as_js_kind(), "box");
        assert_eq!(GetKind::Styles.as_js_kind(), "styles");
        assert_eq!(GetKind::Html.as_js_kind(), "html");
    }

    #[test]
    fn wait_cond_serializes_with_kind_tag() {
        let v = serde_json::to_value(WaitCond::Selector("#go".into())).unwrap();
        assert_eq!(v, json!({ "kind": "selector", "value": "#go" }));
        let v = serde_json::to_value(WaitCond::Function("x>1".into())).unwrap();
        assert_eq!(v, json!({ "kind": "function", "value": "x>1" }));
    }

    #[test]
    fn snapshot_round_trips_camelcase_wire() {
        let raw = json!({
            "title": "T",
            "url": "https://e/",
            "readyState": "complete",
            "text": "hi",
            "html": "<html></html>",
            "snapshotText": "- document \"T\"\n- button \"Go\" [ref=e1]",
            "refs": { "e1": { "role": "button", "name": "Go" } },
            "entries": [{ "selector": "button", "role": "button", "name": "Go", "depth": 0 }]
        });
        let snap: Snapshot = serde_json::from_value(raw).unwrap();
        assert_eq!(snap.ready_state, "complete");
        assert_eq!(snap.refs.get("e1").unwrap().role, "button");
        assert_eq!(snap.entries.len(), 1);
    }

    #[test]
    fn normalize_js_value_preserves_undefined_envelope() {
        let v = json!({ "__weftType": "undefined", "__weftValue": null });
        let out = normalize_js_value(v);
        assert_eq!(out["__weftType"], json!("undefined"));
    }

    #[test]
    fn normalize_js_value_recurses_structures() {
        let v = json!({ "a": [1, { "b": 2 }], "c": "s" });
        let out = normalize_js_value(v.clone());
        assert_eq!(out, v); // structural pass-through is identity for plain JSON
    }
}
