//! Tauri commands for the VMClient contract (P8).
//!
//! Thin bridge so the frontend can probe cloud availability today. The MVP binds
//! the transport to [`LocalStub`], so these always resolve to
//! `VmTransportError::Unsupported` — letting the UI render a "cloud not
//! configured" state without a real service. When a real remote transport ships
//! (separate repo), only the transport construction here changes.

use super::wire_types::{StatusParams, VmRequest, VmResponse, VmStatusDto, VmTransportError};
use super::{LocalStub, VmCloudTransport};

/// Wire-protocol version the local build speaks. Lets the frontend display/log
/// the contract version without a network round-trip.
#[tauri::command]
pub fn vm_protocol_version() -> u32 {
    LocalStub::new().protocol_version()
}

/// Query a VM's status through the configured transport. On the local-only MVP
/// this returns `Err(VmTransportError::Unsupported)`.
#[tauri::command]
pub async fn vm_status(vm_id: String) -> Result<VmStatusDto, VmTransportError> {
    let transport = LocalStub::new();
    match transport
        .request(VmRequest::Status(StatusParams { vm_id }))
        .await?
    {
        VmResponse::Status(status) => Ok(status),
        other => Err(VmTransportError::Protocol(format!(
            "expected Status response, got {other:?}"
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn protocol_version_matches_contract() {
        assert_eq!(vm_protocol_version(), super::super::WIRE_PROTOCOL_VERSION);
    }

    #[tokio::test]
    async fn vm_status_is_unsupported_locally() {
        let result = vm_status("vm-1".to_string()).await;
        assert!(matches!(result, Err(VmTransportError::Unsupported(_))));
    }
}
