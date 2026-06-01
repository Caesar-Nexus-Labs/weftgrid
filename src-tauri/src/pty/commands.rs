//! PTY track Tauri commands (P3).
//!
//! Thin glue between the frontend and [`super::PtyManager`]. Output flows back
//! over a Tauri v2 `Channel<InvokeResponseBody>` as binary `Raw(Vec<u8>)` batches
//! (NOT `emit` per-chunk — see red-team C4): coalesced batches cross the IPC
//! bridge as raw bytes with no per-chunk JSON serialization.
//!
//! Commands to register in `command_registry::register_all`'s `generate_handler!`:
//!   pty::commands::ping (existing stub),
//!   pty::commands::pty_spawn, pty_write, pty_resize, pty_kill,
//!   pty::commands::pty_pause, pty_resume, serialize_pane_state.

use serde::{Deserialize, Serialize};
use tauri::ipc::{Channel, InvokeResponseBody};
use tauri::State;

use super::coalescer::{BatchSink, PaneId};
use super::{PtyManager, SpawnOptions};

/// Demo command proving the registration pattern wires end-to-end (kept from P2).
#[tauri::command]
pub fn ping() -> String {
    "pong".to_string()
}

/// `BatchSink` backed by the frontend's IPC `Channel`. Each coalesced batch is
/// sent as a binary `Raw` body; EOF is signaled with a zero-length batch (the
/// frontend treats an empty payload as "shell closed").
struct ChannelSink {
    channel: Channel<InvokeResponseBody>,
}

impl BatchSink for ChannelSink {
    fn send_batch(&self, data: Vec<u8>) {
        // Empty batches never originate from a real read; reserve them for EOF.
        if data.is_empty() {
            return;
        }
        let _ = self.channel.send(InvokeResponseBody::Raw(data));
    }

    fn on_closed(&self) {
        let _ = self.channel.send(InvokeResponseBody::Raw(Vec::new()));
    }
}

/// Spawn a shell into a new PTY for `pane_id`. Output streams to `on_output`.
/// Returns the child process id.
#[tauri::command]
pub fn pty_spawn(
    manager: State<'_, PtyManager>,
    pane_id: PaneId,
    shell: Option<String>,
    cwd: Option<String>,
    rows: u16,
    cols: u16,
    on_output: Channel<InvokeResponseBody>,
) -> Result<u32, String> {
    let opts = SpawnOptions {
        shell,
        cwd,
        rows,
        cols,
    };
    manager.spawn(pane_id, opts, Box::new(ChannelSink { channel: on_output }))
}

/// Send user input bytes to the shell.
#[tauri::command]
pub fn pty_write(
    manager: State<'_, PtyManager>,
    pane_id: PaneId,
    data: Vec<u8>,
) -> Result<(), String> {
    manager.write(&pane_id, &data)
}

/// Resize the PTY (UI fit-addon → rows/cols).
#[tauri::command]
pub fn pty_resize(
    manager: State<'_, PtyManager>,
    pane_id: PaneId,
    rows: u16,
    cols: u16,
) -> Result<(), String> {
    manager.resize(&pane_id, rows, cols)
}

/// Terminate the shell and drop the pane.
#[tauri::command]
pub fn pty_kill(manager: State<'_, PtyManager>, pane_id: PaneId) -> Result<(), String> {
    manager.kill(&pane_id)
}

/// Backpressure: stop reading from the PTY (UI outstanding bytes over threshold).
#[tauri::command]
pub fn pty_pause(manager: State<'_, PtyManager>, pane_id: PaneId) -> Result<(), String> {
    manager.pause(&pane_id)
}

/// Backpressure released: resume reading.
#[tauri::command]
pub fn pty_resume(manager: State<'_, PtyManager>, pane_id: PaneId) -> Result<(), String> {
    manager.resume(&pane_id)
}

/// Serializable pane state (camelCase wire format). Persistence/restore/respawn
/// is P12's job — P3 only exposes the data. `scrollback` is filled by the caller
/// (the UI passes captured scrollback through; the backend has no buffer copy).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PaneState {
    pub pane_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cwd: Option<String>,
    pub shell: String,
    pub rows: u16,
    pub cols: u16,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scrollback: Option<String>,
}

/// Expose a pane's spawn/size state for persistence (P12).
#[tauri::command]
pub fn serialize_pane_state(
    manager: State<'_, PtyManager>,
    pane_id: String,
    scrollback: Option<String>,
) -> Result<PaneState, String> {
    let info = manager
        .pane_info(&pane_id)
        .ok_or_else(|| format!("unknown pane: {pane_id}"))?;
    Ok(PaneState {
        pane_id,
        cwd: info.cwd,
        shell: info.shell,
        rows: info.rows,
        cols: info.cols,
        scrollback,
    })
}
