//! Wire-protocol DTOs for the cloud-VM client contract (P8).
//!
//! These types are the CLIENT side of a wire protocol spoken to a SEPARATE,
//! networked cloud service (HTTP/RPC). They carry NO cloud logic — only the
//! request/response/error shapes that cross the network boundary. The future
//! real transport (a closed service in its own repo) serializes/deserializes
//! exactly these DTOs; keeping them GPL-safe via "mere aggregation" (the cloud
//! is a remote program, not in-binary code).
//!
//! Defined from first principles (provision / connect / exec / dispose /
//! status) — NOT derived from any external billing/VM source (clean-room).
//!
//! Wire stability: each polymorphic message is adjacently tagged
//! (`{ "kind": ..., "data": ... }`) so variants can be added without breaking
//! existing decoders, and field names are `camelCase` to match the project's
//! IPC/snapshot DTO convention.

use serde::{Deserialize, Serialize};

/// Wire-protocol version. Bumped on any breaking change to the DTOs below so a
/// client and remote service can negotiate/refuse incompatible payloads. Carried
/// by the transport contract (see `VmCloudTransport::protocol_version`).
pub const WIRE_PROTOCOL_VERSION: u32 = 1;

// --- Requests (client → cloud) ---------------------------------------------

/// A single over-the-wire request. One variant per VM lifecycle operation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "data", rename_all = "camelCase")]
pub enum VmRequest {
    Provision(ProvisionParams),
    Connect(ConnectParams),
    Exec(ExecParams),
    Dispose(DisposeParams),
    Status(StatusParams),
}

/// Request a new VM be provisioned. Sizing fields are optional hints; the remote
/// service applies its own defaults/limits.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProvisionParams {
    /// Image / template identifier the remote service understands.
    pub image: String,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub region: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub cpu_count: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub memory_mb: Option<u32>,
    /// Free-form labels (billing tags, owner, etc.).
    #[serde(default)]
    pub labels: Vec<String>,
}

/// Open an interactive session against an already-provisioned VM.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConnectParams {
    pub vm_id: String,
}

/// Run a one-shot command on a VM and collect its result.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExecParams {
    pub vm_id: String,
    pub command: String,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub working_directory: Option<String>,
    /// Environment overrides as `KEY=VALUE` pairs.
    #[serde(default)]
    pub env: Vec<String>,
}

/// Tear down a VM and release its resources.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DisposeParams {
    pub vm_id: String,
}

/// Query the current lifecycle state of a VM.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StatusParams {
    pub vm_id: String,
}

// --- Responses (cloud → client) --------------------------------------------

/// A single over-the-wire response. Variant corresponds to the request kind;
/// `Disposed` is unit (no payload).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "data", rename_all = "camelCase")]
pub enum VmResponse {
    Provisioned(VmHandleDto),
    Connected(VmSessionDto),
    Executed(ExecResultDto),
    Disposed,
    Status(VmStatusDto),
}

/// Coarse lifecycle state of a VM, reported by the remote service.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum VmState {
    Provisioning,
    Running,
    Stopped,
    Disposed,
    Error,
}

/// Handle returned after a successful provision.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VmHandleDto {
    pub vm_id: String,
    pub state: VmState,
    /// RFC3339 timestamp from the remote service (opaque to the client).
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub created_at: Option<String>,
}

/// An open session against a connected VM.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VmSessionDto {
    pub vm_id: String,
    pub session_id: String,
    /// Connection endpoint (e.g. ws/ssh URL) the client dials. Opaque here.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub endpoint: Option<String>,
}

/// Result of a one-shot `Exec`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExecResultDto {
    pub exit_code: i32,
    pub stdout: String,
    pub stderr: String,
}

/// Reported status of a VM.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VmStatusDto {
    pub vm_id: String,
    pub state: VmState,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub message: Option<String>,
}

// --- Errors ----------------------------------------------------------------

/// Transport-level error surface. `Unsupported` is what the local-only build's
/// `LocalStub` always returns; the rest describe failure modes a real remote
/// transport will produce over HTTP/RPC.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "data", rename_all = "camelCase")]
pub enum VmTransportError {
    /// No cloud service is wired into this build (local-only MVP).
    Unsupported(String),
    /// Could not reach the remote service (connect/timeout/DNS).
    Network(String),
    /// Reply was unintelligible (decode/version mismatch).
    Protocol(String),
    /// Remote service replied with an application-level error.
    Remote { code: u32, message: String },
}

impl std::fmt::Display for VmTransportError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            VmTransportError::Unsupported(m) => write!(f, "unsupported: {m}"),
            VmTransportError::Network(m) => write!(f, "network error: {m}"),
            VmTransportError::Protocol(m) => write!(f, "protocol error: {m}"),
            VmTransportError::Remote { code, message } => {
                write!(f, "remote error {code}: {message}")
            }
        }
    }
}

impl std::error::Error for VmTransportError {}
