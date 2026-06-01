//! Request dispatch (P13): route a verified [`Command`] to its domain handler and
//! produce an [`RpcResponse`].
//!
//! Browser ops route to the P7 automation command set. P7 is built in parallel and
//! its concrete API is not available at this track's build time, so dispatch calls
//! through the [`AutomationDispatch`] trait — a contract P7 implements at Wave-3
//! integration. `notify`/`ssh` are skeleton handlers P5/P10 flesh out.

use super::protocol::{BrowserAction, Command, ErrorModel, GetKind, PaneTarget, RpcResponse};

/// Result of an automation op: the JSON payload P7 produces (snapshot text, ref
/// list, eval return value, ...). Dispatch wraps it in [`RpcResponse::Ok`].
pub type AutomationResult = Result<serde_json::Value, ErrorModel>;

/// The contract P13's dispatch calls; P7 implements it at integration.
///
/// `target` is resolved by the implementation: `Focused` means the focused
/// browser pane, `Pane{id}` an explicit one. The implementation owns ambiguity
/// errors (e.g. `Focused` with multiple panes and none focused) and returns an
/// [`ErrorModel`] the CLI surfaces — dispatch stays transport-only.
pub trait AutomationDispatch: Send + Sync {
    /// Capture a DOM snapshot (ephemeral `eN` refs + text) for `target`.
    fn snapshot(&self, target: &PaneTarget) -> AutomationResult;
    /// Click the element bound to `reference` (`eN`) in `target`.
    fn click(&self, target: &PaneTarget, reference: &str) -> AutomationResult;
    /// Fill `reference`'s field with `text` in `target`.
    fn fill(&self, target: &PaneTarget, reference: &str, text: &str) -> AutomationResult;
    /// Evaluate `js` in `target`, returning its JSON-serialized result.
    fn eval(&self, target: &PaneTarget, js: &str) -> AutomationResult;
    /// Wait for `selector` (or readiness) up to `timeout_ms` in `target`.
    fn wait(
        &self,
        target: &PaneTarget,
        selector: Option<&str>,
        timeout_ms: Option<u64>,
    ) -> AutomationResult;
    /// Get property `kind` (optionally `attr`) off the element bound to
    /// `reference` (`eN`) in `target` — P7 `browser_get` parity.
    fn get(
        &self,
        target: &PaneTarget,
        reference: &str,
        kind: GetKind,
        attr: Option<&str>,
    ) -> AutomationResult;
    /// Find elements matching `query` in `target`.
    fn find(&self, target: &PaneTarget, query: &str) -> AutomationResult;
}

/// The skeleton notify/ssh sinks P5/P10 implement. Default impls return
/// `Unimplemented` so the single binary's protocol is wired end-to-end before
/// those tracks land.
pub trait DomainHandlers: Send + Sync {
    fn notify(&self, _message: &str) -> AutomationResult {
        Err(ErrorModel::new(
            "unimplemented",
            "notify handler not yet provided (P5)",
        ))
    }
    fn ssh(&self, _destination: &str) -> AutomationResult {
        Err(ErrorModel::new(
            "unimplemented",
            "ssh handler not yet provided (P10)",
        ))
    }
}

/// Everything dispatch needs at runtime: the automation backend plus the
/// notify/ssh handlers. Held behind the app's RPC state.
pub struct Dispatcher {
    automation: Box<dyn AutomationDispatch>,
    handlers: Box<dyn DomainHandlers>,
}

impl Dispatcher {
    pub fn new(
        automation: Box<dyn AutomationDispatch>,
        handlers: Box<dyn DomainHandlers>,
    ) -> Self {
        Dispatcher {
            automation,
            handlers,
        }
    }

    /// Route a verified command. Token verification happens before this in the
    /// server; here we only map the command to a handler call.
    pub fn dispatch(&self, command: &Command) -> RpcResponse {
        let result = match command {
            Command::Browser { target, action } => self.dispatch_browser(target, action),
            Command::Notify { message } => self.handlers.notify(message),
            Command::Ssh { destination } => self.handlers.ssh(destination),
        };
        match result {
            Ok(data) => RpcResponse::ok(data),
            Err(error) => RpcResponse::error(error),
        }
    }

    fn dispatch_browser(&self, target: &PaneTarget, action: &BrowserAction) -> AutomationResult {
        match action {
            BrowserAction::Snapshot => self.automation.snapshot(target),
            BrowserAction::Click { reference } => self.automation.click(target, reference),
            BrowserAction::Fill { reference, text } => {
                self.automation.fill(target, reference, text)
            }
            BrowserAction::Eval { js } => self.automation.eval(target, js),
            BrowserAction::Wait {
                selector,
                timeout_ms,
            } => self
                .automation
                .wait(target, selector.as_deref(), *timeout_ms),
            BrowserAction::Get {
                reference,
                kind,
                attr,
            } => self
                .automation
                .get(target, reference, *kind, attr.as_deref()),
            BrowserAction::Find { query } => self.automation.find(target, query),
        }
    }
}

/// A no-op [`DomainHandlers`] using the trait defaults (returns `Unimplemented`).
/// Used until P5/P10 wire real handlers.
pub struct StubHandlers;
impl DomainHandlers for StubHandlers {}

#[cfg(test)]
pub(crate) mod test_support {
    //! A mock [`AutomationDispatch`] that records calls and returns canned JSON.
    //! Lets dispatch/server tests assert routing without P7's real backend.
    use super::*;
    use std::sync::Mutex;

    #[derive(Default)]
    pub struct MockAutomation {
        pub calls: Mutex<Vec<String>>,
    }

    impl MockAutomation {
        fn record(&self, what: &str) {
            self.calls.lock().unwrap().push(what.to_string());
        }
    }

    impl AutomationDispatch for MockAutomation {
        fn snapshot(&self, target: &PaneTarget) -> AutomationResult {
            self.record(&format!("snapshot:{target:?}"));
            Ok(serde_json::json!({ "snapshot": "e1 button \"OK\"" }))
        }
        fn click(&self, _t: &PaneTarget, reference: &str) -> AutomationResult {
            self.record(&format!("click:{reference}"));
            Ok(serde_json::json!({ "clicked": reference }))
        }
        fn fill(&self, _t: &PaneTarget, reference: &str, text: &str) -> AutomationResult {
            self.record(&format!("fill:{reference}:{text}"));
            Ok(serde_json::json!({ "filled": reference }))
        }
        fn eval(&self, _t: &PaneTarget, js: &str) -> AutomationResult {
            self.record(&format!("eval:{js}"));
            Ok(serde_json::json!({ "result": 42 }))
        }
        fn wait(
            &self,
            _t: &PaneTarget,
            selector: Option<&str>,
            _timeout_ms: Option<u64>,
        ) -> AutomationResult {
            self.record(&format!("wait:{selector:?}"));
            Ok(serde_json::json!({ "ready": true }))
        }
        fn get(
            &self,
            _t: &PaneTarget,
            reference: &str,
            kind: GetKind,
            attr: Option<&str>,
        ) -> AutomationResult {
            self.record(&format!("get:{reference}:{kind:?}:{attr:?}"));
            Ok(serde_json::json!({ "value": "ok" }))
        }
        fn find(&self, _t: &PaneTarget, query: &str) -> AutomationResult {
            self.record(&format!("find:{query}"));
            Ok(serde_json::json!({ "matches": [] }))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::test_support::MockAutomation;
    use super::*;
    use std::sync::Arc;

    fn dispatcher_with(mock: Arc<MockAutomation>) -> Dispatcher {
        struct Wrapper(Arc<MockAutomation>);
        impl AutomationDispatch for Wrapper {
            fn snapshot(&self, t: &PaneTarget) -> AutomationResult {
                self.0.snapshot(t)
            }
            fn click(&self, t: &PaneTarget, r: &str) -> AutomationResult {
                self.0.click(t, r)
            }
            fn fill(&self, t: &PaneTarget, r: &str, x: &str) -> AutomationResult {
                self.0.fill(t, r, x)
            }
            fn eval(&self, t: &PaneTarget, js: &str) -> AutomationResult {
                self.0.eval(t, js)
            }
            fn wait(
                &self,
                t: &PaneTarget,
                s: Option<&str>,
                ms: Option<u64>,
            ) -> AutomationResult {
                self.0.wait(t, s, ms)
            }
            fn get(
                &self,
                t: &PaneTarget,
                r: &str,
                kind: GetKind,
                attr: Option<&str>,
            ) -> AutomationResult {
                self.0.get(t, r, kind, attr)
            }
            fn find(&self, t: &PaneTarget, q: &str) -> AutomationResult {
                self.0.find(t, q)
            }
        }
        Dispatcher::new(Box::new(Wrapper(mock)), Box::new(StubHandlers))
    }

    #[test]
    fn browser_snapshot_routes_to_automation() {
        let mock = Arc::new(MockAutomation::default());
        let d = dispatcher_with(mock.clone());
        let resp = d.dispatch(&Command::Browser {
            target: PaneTarget::Focused,
            action: BrowserAction::Snapshot,
        });
        match resp {
            RpcResponse::Ok { data } => assert_eq!(data["snapshot"], "e1 button \"OK\""),
            other => panic!("expected ok, got {other:?}"),
        }
        assert_eq!(mock.calls.lock().unwrap().len(), 1);
    }

    #[test]
    fn browser_fill_passes_ref_and_text() {
        let mock = Arc::new(MockAutomation::default());
        let d = dispatcher_with(mock.clone());
        let resp = d.dispatch(&Command::Browser {
            target: PaneTarget::Pane { id: "p2".into() },
            action: BrowserAction::Fill {
                reference: "e7".into(),
                text: "hello".into(),
            },
        });
        assert!(matches!(resp, RpcResponse::Ok { .. }));
        assert_eq!(mock.calls.lock().unwrap()[0], "fill:e7:hello");
    }

    #[test]
    fn browser_get_passes_ref_kind_and_attr() {
        let mock = Arc::new(MockAutomation::default());
        let d = dispatcher_with(mock.clone());
        let resp = d.dispatch(&Command::Browser {
            target: PaneTarget::Focused,
            action: BrowserAction::Get {
                reference: "e3".into(),
                kind: GetKind::Attr,
                attr: Some("href".into()),
            },
        });
        assert!(matches!(resp, RpcResponse::Ok { .. }));
        assert_eq!(mock.calls.lock().unwrap()[0], "get:e3:Attr:Some(\"href\")");
    }

    #[test]
    fn notify_and_ssh_are_unimplemented_by_default() {
        let mock = Arc::new(MockAutomation::default());
        let d = dispatcher_with(mock);
        let resp = d.dispatch(&Command::Notify {
            message: "hi".into(),
        });
        match resp {
            RpcResponse::Error { error } => assert_eq!(error.code, "unimplemented"),
            other => panic!("expected error, got {other:?}"),
        }
    }
}
