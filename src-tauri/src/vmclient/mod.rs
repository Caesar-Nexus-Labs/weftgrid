//! VMClient contract-stub track (P8 owner: `src-tauri/src/vmclient/**`).
//!
//! Defines the CLIENT side of a wire protocol to a SEPARATE, networked cloud-VM
//! service — request/response/error DTOs (`wire_types`) + one transport trait
//! (`VmCloudTransport`) + a no-cloud `LocalStub`. There is deliberately NO cloud
//! implementation and NO `cloud` feature-flag here: the real transport is a
//! closed remote program in its own repo that the GPL client merely talks to
//! over the network ("mere aggregation"), keeping this binary GPL-clean.
//!
//! Transport boundary is async + dyn-safe. To avoid pulling in `async-trait`,
//! the one method returns a boxed future by hand (`Pin<Box<dyn Future + Send>>`),
//! which is exactly what `async-trait` would generate.

use std::future::Future;
use std::pin::Pin;

use tauri::{Builder, Runtime};

pub mod commands;
pub mod local_stub;
pub mod wire_types;

pub use local_stub::LocalStub;
#[allow(unused_imports)]
pub use wire_types::{
    ConnectParams, DisposeParams, ExecParams, ExecResultDto, ProvisionParams, StatusParams,
    VmHandleDto, VmRequest, VmResponse, VmSessionDto, VmState, VmStatusDto, VmTransportError,
    WIRE_PROTOCOL_VERSION,
};

/// Boxed, `Send` future returned by the transport's single wire method.
pub type VmResponseFuture<'a> =
    Pin<Box<dyn Future<Output = Result<VmResponse, VmTransportError>> + Send + 'a>>;

/// The network boundary: a future cloud service implements this over HTTP/RPC by
/// serializing [`VmRequest`] and deserializing [`VmResponse`]. The MVP binds it
/// to [`LocalStub`], which always answers `Unsupported`.
///
/// Dyn-safe (usable as `dyn VmCloudTransport`) and `Send + Sync` so it can live
/// in shared state across async tasks.
pub trait VmCloudTransport: Send + Sync {
    /// Wire-protocol version this transport speaks (see [`WIRE_PROTOCOL_VERSION`]).
    fn protocol_version(&self) -> u32;

    /// Send one request across the network boundary and await its response.
    fn request(&self, req: VmRequest) -> VmResponseFuture<'_>;
}

/// Additive setup (state/plugins/setup). No `invoke_handler` (last-wins → central).
pub fn register<R: Runtime>(builder: Builder<R>) -> Builder<R> {
    builder
}

#[cfg(test)]
mod tests {
    use super::wire_types::*;

    /// serialize → deserialize must round-trip every wire message so the DTOs are
    /// safe to send over HTTP/RPC.
    fn round_trip<T>(value: &T)
    where
        T: serde::Serialize + serde::de::DeserializeOwned + PartialEq + std::fmt::Debug,
    {
        let json = serde_json::to_string(value).expect("serialize");
        let back: T = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(value, &back, "round-trip mismatch via {json}");
    }

    #[test]
    fn requests_round_trip() {
        round_trip(&VmRequest::Provision(ProvisionParams {
            image: "ubuntu:24.04".into(),
            region: Some("eu-west".into()),
            cpu_count: Some(4),
            memory_mb: Some(8192),
            labels: vec!["owner=ci".into()],
        }));
        round_trip(&VmRequest::Connect(ConnectParams {
            vm_id: "vm-1".into(),
        }));
        round_trip(&VmRequest::Exec(ExecParams {
            vm_id: "vm-1".into(),
            command: "echo".into(),
            args: vec!["hi".into()],
            working_directory: Some("/root".into()),
            env: vec!["A=1".into()],
        }));
        round_trip(&VmRequest::Dispose(DisposeParams {
            vm_id: "vm-1".into(),
        }));
        round_trip(&VmRequest::Status(StatusParams {
            vm_id: "vm-1".into(),
        }));
    }

    #[test]
    fn responses_round_trip() {
        round_trip(&VmResponse::Provisioned(VmHandleDto {
            vm_id: "vm-1".into(),
            state: VmState::Provisioning,
            created_at: Some("2026-05-31T00:00:00Z".into()),
        }));
        round_trip(&VmResponse::Connected(VmSessionDto {
            vm_id: "vm-1".into(),
            session_id: "sess-1".into(),
            endpoint: Some("wss://example/vm-1".into()),
        }));
        round_trip(&VmResponse::Executed(ExecResultDto {
            exit_code: 0,
            stdout: "ok".into(),
            stderr: String::new(),
        }));
        round_trip(&VmResponse::Disposed);
        round_trip(&VmResponse::Status(VmStatusDto {
            vm_id: "vm-1".into(),
            state: VmState::Running,
            message: None,
        }));
    }

    #[test]
    fn errors_round_trip() {
        round_trip(&VmTransportError::Unsupported("local-only".into()));
        round_trip(&VmTransportError::Network("timeout".into()));
        round_trip(&VmTransportError::Protocol("bad frame".into()));
        round_trip(&VmTransportError::Remote {
            code: 402,
            message: "payment required".into(),
        });
    }

    #[test]
    fn adjacent_tag_shape_is_stable() {
        // Wire shape contract: adjacently-tagged `{ "kind", "data" }`.
        let json = serde_json::to_value(VmRequest::Connect(ConnectParams {
            vm_id: "vm-1".into(),
        }))
        .unwrap();
        assert_eq!(json["kind"], "connect");
        assert_eq!(json["data"]["vmId"], "vm-1");
    }
}
