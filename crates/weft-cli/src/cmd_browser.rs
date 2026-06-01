//! `weft browser` subcommand (P13): the agent-facing browser automation verbs.
//!
//! Each verb builds the `Command::Browser` JSON the server expects (mirrors
//! `agent_rpc::protocol`) and ships it via [`crate::rpc_client`]. The `--pane`
//! selector targets a specific browser pane; omitted means the focused pane
//! (the server errors on ambiguity — multiple panes, none focused).

use clap::{Args, Subcommand, ValueEnum};

use crate::rpc_client;

#[derive(Debug, Args)]
pub struct BrowserArgs {
    #[command(subcommand)]
    action: BrowserAction,

    /// Target browser pane id. Omit to target the focused pane.
    #[arg(long, global = true)]
    pane: Option<String>,
}

/// Which property `get` reads off an element. Tokens match the protocol's
/// `GetKind` (snake_case) so the CLI and server share one vocabulary.
#[derive(Debug, Clone, Copy, ValueEnum)]
enum GetKind {
    Text,
    Html,
    Value,
    Attr,
    Box,
    Styles,
    Count,
}

impl GetKind {
    fn as_wire(self) -> &'static str {
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

#[derive(Debug, Subcommand)]
enum BrowserAction {
    /// Capture a DOM snapshot (ephemeral `eN` refs + accessibility text).
    Snapshot,
    /// Click the element bound to a ref (`eN`).
    Click {
        #[arg(value_name = "ref")]
        reference: String,
    },
    /// Type text into the field bound to a ref (`eN`).
    Fill {
        #[arg(value_name = "ref")]
        reference: String,
        text: String,
    },
    /// Evaluate JavaScript in the pane and return its result.
    Eval {
        js: String,
    },
    /// Wait for a selector (optional) up to a timeout (ms).
    Wait {
        #[arg(long)]
        selector: Option<String>,
        #[arg(long)]
        timeout_ms: Option<u64>,
    },
    /// Read a property (text|html|value|attr|box|styles|count) off the element
    /// bound to a ref (`eN`). `--attr` names the attribute when `--kind attr`.
    Get {
        #[arg(value_name = "ref")]
        reference: String,
        #[arg(long, value_enum, default_value_t = GetKind::Text)]
        kind: GetKind,
        #[arg(long)]
        attr: Option<String>,
    },
    /// Find elements matching a query.
    Find {
        query: String,
    },
}

/// Build the pane target JSON: `Pane{id}` when `--pane` is given, else `Focused`.
fn target_json(pane: &Option<String>) -> serde_json::Value {
    match pane {
        Some(id) => serde_json::json!({ "pane": { "id": id } }),
        None => serde_json::Value::String("focused".to_string()),
    }
}

/// Map a parsed action to the `action` object the protocol's `BrowserAction`
/// (tag = "op") deserializes.
fn action_json(action: &BrowserAction) -> serde_json::Value {
    match action {
        BrowserAction::Snapshot => serde_json::json!({ "op": "snapshot" }),
        BrowserAction::Click { reference } => {
            serde_json::json!({ "op": "click", "ref": reference })
        }
        BrowserAction::Fill { reference, text } => {
            serde_json::json!({ "op": "fill", "ref": reference, "text": text })
        }
        BrowserAction::Eval { js } => serde_json::json!({ "op": "eval", "js": js }),
        BrowserAction::Wait {
            selector,
            timeout_ms,
        } => {
            let mut obj = serde_json::Map::new();
            obj.insert("op".into(), "wait".into());
            if let Some(s) = selector {
                obj.insert("selector".into(), s.clone().into());
            }
            if let Some(ms) = timeout_ms {
                obj.insert("timeout_ms".into(), (*ms).into());
            }
            serde_json::Value::Object(obj)
        }
        BrowserAction::Get {
            reference,
            kind,
            attr,
        } => {
            let mut obj = serde_json::Map::new();
            obj.insert("op".into(), "get".into());
            obj.insert("ref".into(), reference.clone().into());
            obj.insert("kind".into(), kind.as_wire().into());
            if let Some(a) = attr {
                obj.insert("attr".into(), a.clone().into());
            }
            serde_json::Value::Object(obj)
        }
        BrowserAction::Find { query } => serde_json::json!({ "op": "find", "query": query }),
    }
}

/// Build the full flattened `Command::Browser` value and send it.
pub fn run(args: BrowserArgs) -> i32 {
    let command = serde_json::json!({
        "domain": "browser",
        "target": target_json(&args.pane),
        "action": action_json(&args.action),
    });
    rpc_client::run_command(command)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn focused_target_when_no_pane() {
        assert_eq!(target_json(&None), serde_json::json!("focused"));
    }

    #[test]
    fn explicit_pane_target() {
        assert_eq!(
            target_json(&Some("p1".to_string())),
            serde_json::json!({ "pane": { "id": "p1" } })
        );
    }

    #[test]
    fn fill_action_carries_ref_and_text() {
        let a = BrowserAction::Fill {
            reference: "e3".into(),
            text: "hi".into(),
        };
        assert_eq!(
            action_json(&a),
            serde_json::json!({ "op": "fill", "ref": "e3", "text": "hi" })
        );
    }

    #[test]
    fn wait_omits_absent_optionals() {
        let a = BrowserAction::Wait {
            selector: None,
            timeout_ms: None,
        };
        assert_eq!(action_json(&a), serde_json::json!({ "op": "wait" }));
    }

    #[test]
    fn get_action_carries_ref_kind_and_attr() {
        let a = BrowserAction::Get {
            reference: "e3".into(),
            kind: GetKind::Attr,
            attr: Some("href".into()),
        };
        assert_eq!(
            action_json(&a),
            serde_json::json!({ "op": "get", "ref": "e3", "kind": "attr", "attr": "href" })
        );
    }

    #[test]
    fn get_action_omits_attr_when_absent() {
        let a = BrowserAction::Get {
            reference: "e1".into(),
            kind: GetKind::Text,
            attr: None,
        };
        assert_eq!(
            action_json(&a),
            serde_json::json!({ "op": "get", "ref": "e1", "kind": "text" })
        );
    }
}
