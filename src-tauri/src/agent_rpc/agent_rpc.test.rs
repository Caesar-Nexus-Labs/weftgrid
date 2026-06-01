//! P13 integration tests: token issuance → CLI-style load → framed request →
//! server auth + dispatch → framed response. Complements the per-module unit
//! tests in `auth`/`protocol`/`dispatch`/`server`.

use std::sync::Arc;

use super::auth::{self, SessionToken};
use super::dispatch::{test_support::MockAutomation, Dispatcher, StubHandlers};
use super::protocol::{
    read_frame, write_frame, BrowserAction, Command, ErrorModel, GetKind, PaneTarget, RpcRequest,
    RpcResponse,
};
use super::server::{handle_connection, ServerContext};

fn temp_dir(tag: &str) -> std::path::PathBuf {
    let mut p = std::env::temp_dir();
    p.push(format!("weft-rpc-it-{tag}-{}", uuid::Uuid::new_v4()));
    p
}

fn ctx_with_token(secret: &str) -> ServerContext {
    let dispatcher = Dispatcher::new(Box::new(MockAutomation::default()), Box::new(StubHandlers));
    ServerContext {
        token: Arc::new(SessionToken::from_secret(secret)),
        dispatcher: Arc::new(dispatcher),
    }
}

async fn round_trip(ctx: ServerContext, body: Vec<u8>) -> RpcResponse {
    let (mut client, mut server) = tokio::io::duplex(8192);
    let server_task = tokio::spawn(async move {
        handle_connection(&ctx, &mut server).await.unwrap();
    });
    write_frame(&mut client, &body).await.unwrap();
    let resp_bytes = read_frame(&mut client).await.unwrap();
    server_task.await.unwrap();
    serde_json::from_slice(&resp_bytes).unwrap()
}

#[tokio::test]
async fn issued_token_authenticates_and_dispatches() {
    // App side: issue + persist a token; CLI side: load it back from the file.
    let dir = temp_dir("issue");
    let issued = SessionToken::generate();
    let path = issued.persist(&dir).unwrap();
    let loaded = auth::load_token(&path).unwrap();

    let req = RpcRequest {
        token: loaded,
        command: Command::Browser {
            target: PaneTarget::Pane { id: "pane-7".into() },
            action: BrowserAction::Snapshot,
        },
    };
    let body = serde_json::to_vec(&req).unwrap();
    let ctx = ctx_with_token(issued.as_str());

    match round_trip(ctx, body).await {
        RpcResponse::Ok { data } => assert_eq!(data["snapshot"], "e1 button \"OK\""),
        other => panic!("expected ok, got {other:?}"),
    }
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn wrong_token_is_rejected_unauthorized() {
    let req = RpcRequest {
        token: "attacker-guess".into(),
        command: Command::Browser {
            target: PaneTarget::Focused,
            action: BrowserAction::Snapshot,
        },
    };
    let body = serde_json::to_vec(&req).unwrap();
    let ctx = ctx_with_token("real-session-token");
    match round_trip(ctx, body).await {
        RpcResponse::Error { error } => assert_eq!(error.code, "unauthorized"),
        other => panic!("expected unauthorized, got {other:?}"),
    }
}

#[tokio::test]
async fn missing_token_field_is_rejected() {
    // A body with no token field at all is a bad_request (deserialization fails on
    // the required `token`), never an accidental accept.
    let body = serde_json::to_vec(&serde_json::json!({
        "domain": "browser",
        "target": "focused",
        "action": { "op": "snapshot" }
    }))
    .unwrap();
    let ctx = ctx_with_token("real-session-token");
    match round_trip(ctx, body).await {
        RpcResponse::Error { error } => assert_eq!(error.code, "bad_request"),
        other => panic!("expected bad_request, got {other:?}"),
    }
}

#[test]
fn protocol_request_serde_round_trips() {
    // The wire shape the CLI builds must deserialize byte-identical on the server.
    let req = RpcRequest {
        token: "t".into(),
        command: Command::Browser {
            target: PaneTarget::Pane { id: "p1".into() },
            action: BrowserAction::Fill {
                reference: "e3".into(),
                text: "value".into(),
            },
        },
    };
    let json = serde_json::to_vec(&req).unwrap();
    let back: RpcRequest = serde_json::from_slice(&json).unwrap();
    assert_eq!(req, back);
}

#[test]
fn cli_get_wire_json_deserializes_into_protocol_get() {
    // Guards the CLI↔server seam the type system can't (weft-cli is a standalone
    // crate that cannot link this one). This is the EXACT JSON `cmd_browser.rs`
    // emits for `weft browser get e3 --kind attr --attr href`; it must land as
    // BrowserAction::Get with the ref-rename + GetKind token resolved.
    let wire = serde_json::json!({
        "token": "t",
        "domain": "browser",
        "target": "focused",
        "action": { "op": "get", "ref": "e3", "kind": "attr", "attr": "href" }
    });
    let req: RpcRequest = serde_json::from_value(wire).unwrap();
    match req.command {
        Command::Browser {
            action:
                BrowserAction::Get {
                    reference,
                    kind,
                    attr,
                },
            ..
        } => {
            assert_eq!(reference, "e3");
            assert_eq!(kind, GetKind::Attr);
            assert_eq!(attr.as_deref(), Some("href"));
        }
        other => panic!("expected browser get, got {other:?}"),
    }

    // attr omitted (the `weft browser get e1` / `--kind text` case) must parse too.
    let wire_no_attr = serde_json::json!({
        "token": "t",
        "domain": "browser",
        "target": "focused",
        "action": { "op": "get", "ref": "e1", "kind": "text" }
    });
    let req: RpcRequest = serde_json::from_value(wire_no_attr).unwrap();
    match req.command {
        Command::Browser {
            action: BrowserAction::Get { kind, attr, .. },
            ..
        } => {
            assert_eq!(kind, GetKind::Text);
            assert!(attr.is_none());
        }
        other => panic!("expected browser get, got {other:?}"),
    }
}

#[test]
fn error_response_serde_shape_is_stable() {
    let resp = RpcResponse::error(ErrorModel::new("unauthorized", "authentication failed"));
    let json = serde_json::to_string(&resp).unwrap();
    assert!(json.contains("\"status\":\"error\""));
    assert!(json.contains("\"code\":\"unauthorized\""));
}
