//! Local-only transport stub for the cloud-VM client contract (P8).
//!
//! The MVP ships with NO cloud service wired in, so `LocalStub` is the default
//! transport: every request resolves to `VmTransportError::Unsupported`. It
//! performs no networking and holds no state — it exists so the rest of the app
//! can depend on `dyn VmCloudTransport` today and swap in a real remote
//! transport (separate networked program, own repo) later without changing call
//! sites.

use std::future::Future;
use std::pin::Pin;

use super::wire_types::{VmRequest, VmResponse, VmTransportError, WIRE_PROTOCOL_VERSION};
use super::VmCloudTransport;

/// Message returned by every `LocalStub` request. Explains WHY the op fails so
/// the UI can surface a meaningful "cloud not configured" state.
const UNSUPPORTED_MESSAGE: &str = "local-only build: no cloud service configured";

/// The no-cloud transport. Zero-sized; clone/construct freely.
#[derive(Debug, Clone, Copy, Default)]
pub struct LocalStub;

impl LocalStub {
    pub fn new() -> Self {
        LocalStub
    }
}

impl VmCloudTransport for LocalStub {
    fn protocol_version(&self) -> u32 {
        WIRE_PROTOCOL_VERSION
    }

    fn request(
        &self,
        _req: VmRequest,
    ) -> Pin<Box<dyn Future<Output = Result<VmResponse, VmTransportError>> + Send + '_>> {
        Box::pin(async move {
            Err(VmTransportError::Unsupported(
                UNSUPPORTED_MESSAGE.to_string(),
            ))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::super::wire_types::*;
    use super::*;

    /// Every request variant the stub can receive, for exhaustive coverage.
    fn all_requests() -> Vec<VmRequest> {
        vec![
            VmRequest::Provision(ProvisionParams {
                image: "img".into(),
                ..Default::default()
            }),
            VmRequest::Connect(ConnectParams { vm_id: "vm".into() }),
            VmRequest::Exec(ExecParams {
                vm_id: "vm".into(),
                command: "ls".into(),
                ..Default::default()
            }),
            VmRequest::Dispose(DisposeParams { vm_id: "vm".into() }),
            VmRequest::Status(StatusParams { vm_id: "vm".into() }),
        ]
    }

    #[tokio::test]
    async fn stub_returns_unsupported_for_every_request() {
        let stub = LocalStub::new();
        for req in all_requests() {
            let result = stub.request(req.clone()).await;
            match result {
                Err(VmTransportError::Unsupported(msg)) => {
                    assert!(
                        msg.contains("local-only"),
                        "message should explain the local-only build: {msg}"
                    );
                }
                other => panic!("expected Unsupported for {req:?}, got {other:?}"),
            }
        }
    }

    #[tokio::test]
    async fn usable_as_trait_object() {
        // Compile-time dyn-safety + runtime use through the network-boundary trait.
        let transport: Box<dyn VmCloudTransport> = Box::new(LocalStub::new());
        assert_eq!(transport.protocol_version(), WIRE_PROTOCOL_VERSION);
        let result = transport
            .request(VmRequest::Status(StatusParams { vm_id: "vm".into() }))
            .await;
        assert!(matches!(result, Err(VmTransportError::Unsupported(_))));
    }
}
