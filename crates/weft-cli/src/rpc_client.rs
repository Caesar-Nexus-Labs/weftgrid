//! RPC client (P13 CLI side): discover the endpoint + token from the app-data
//! well-known location, connect the local socket/pipe, frame the request, and
//! parse the framed response.
//!
//! The CLI does NOT compile against `src-tauri`, so the wire types are mirrored
//! here as the protocol's stable JSON contract. Keep these in sync with
//! `src-tauri/src/agent_rpc/protocol.rs`.
//!
//! Token handling (Security): the token is read from the user-only app-data file,
//! NOT taken from argv (a process list must not leak it), and placed inside the
//! framed JSON body.

use std::io;
use std::path::{Path, PathBuf};

use directories::ProjectDirs;
use serde::{Deserialize, Serialize};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

/// Matches `agent_rpc::auth` file-name convention.
const TOKEN_FILE_NAME: &str = "weft-rpc-token";
const ENDPOINT_FILE_NAME: &str = "weft-rpc-endpoint";
/// Matches `agent_rpc::protocol::MAX_FRAME_BYTES`.
const MAX_FRAME_BYTES: usize = 64 * 1024 * 1024;

/// The request envelope (mirrors `agent_rpc::protocol::RpcRequest`): token plus a
/// flattened command. Commands are built by the `cmd_*` modules as raw
/// `serde_json::Value` so this client stays decoupled from each subcommand's shape.
#[derive(Debug, Serialize)]
struct RpcRequest {
    token: String,
    #[serde(flatten)]
    command: serde_json::Value,
}

/// The reply (mirrors `agent_rpc::protocol::RpcResponse`): tagged `status` with
/// either `data` or `error`.
#[derive(Debug, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum RpcResponse {
    Ok { data: serde_json::Value },
    Error { error: ErrorModel },
}

#[derive(Debug, Deserialize)]
pub struct ErrorModel {
    pub code: String,
    pub message: String,
}

/// Resolve the weftgrid app-data dir (same `ProjectDirs` qualifier the app uses).
fn app_data_dir() -> io::Result<PathBuf> {
    let proj = ProjectDirs::from("", "", "weftgrid")
        .ok_or_else(|| io::Error::other("could not resolve OS app-data dir"))?;
    Ok(proj.data_dir().to_path_buf())
}

/// Read the current session token from the user-only file. Re-read every
/// invocation (never cache) so an app restart that re-issues the token is picked
/// up — a stale token would just fail auth.
fn read_token(dir: &Path) -> io::Result<String> {
    let raw = std::fs::read_to_string(dir.join(TOKEN_FILE_NAME)).map_err(|e| {
        io::Error::new(
            e.kind(),
            format!(
                "cannot read RPC token (is weftgrid running?): {}",
                dir.join(TOKEN_FILE_NAME).display()
            ),
        )
    })?;
    Ok(raw.trim_end_matches(['\n', '\r']).to_string())
}

/// Read the endpoint address (unix socket path / Windows pipe name) the app wrote.
fn read_endpoint(dir: &Path) -> io::Result<String> {
    let raw = std::fs::read_to_string(dir.join(ENDPOINT_FILE_NAME)).map_err(|e| {
        io::Error::new(
            e.kind(),
            format!("cannot read RPC endpoint (is weftgrid running?): {e}"),
        )
    })?;
    Ok(raw.trim_end_matches(['\n', '\r']).to_string())
}

/// Send `command` (a flattened command value) to the running app and return the
/// parsed response. Synchronous wrapper around the async transport so the
/// `cmd_*` modules stay simple.
pub fn send(command: serde_json::Value) -> io::Result<RpcResponse> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    runtime.block_on(async move {
        let dir = app_data_dir()?;
        let token = read_token(&dir)?;
        let endpoint = read_endpoint(&dir)?;
        let request = RpcRequest { token, command };
        let body = serde_json::to_vec(&request)?;
        let reply = transact(&endpoint, &body).await?;
        let response: RpcResponse = serde_json::from_slice(&reply)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
        Ok(response)
    })
}

/// Connect the OS transport, write one framed request, read one framed response.
#[cfg(unix)]
async fn transact(endpoint: &str, body: &[u8]) -> io::Result<Vec<u8>> {
    use tokio::net::UnixStream;
    let mut stream = UnixStream::connect(endpoint).await?;
    write_frame(&mut stream, body).await?;
    read_frame(&mut stream).await
}

#[cfg(windows)]
async fn transact(endpoint: &str, body: &[u8]) -> io::Result<Vec<u8>> {
    use tokio::net::windows::named_pipe::ClientOptions;
    // The server may be between accept instances; a brief retry covers that race.
    let mut stream = loop {
        match ClientOptions::new().open(endpoint) {
            Ok(s) => break s,
            Err(e)
                if e.raw_os_error()
                    == Some(windows_pipe_busy_code()) =>
            {
                tokio::time::sleep(std::time::Duration::from_millis(20)).await;
            }
            Err(e) => return Err(e),
        }
    };
    write_frame(&mut stream, body).await?;
    read_frame(&mut stream).await
}

/// ERROR_PIPE_BUSY (231): all pipe instances are busy; caller retries.
#[cfg(windows)]
fn windows_pipe_busy_code() -> i32 {
    231
}

/// Length-prefixed write: 4-byte big-endian length header then the payload.
async fn write_frame<W>(writer: &mut W, payload: &[u8]) -> io::Result<()>
where
    W: tokio::io::AsyncWrite + Unpin,
{
    if payload.len() > MAX_FRAME_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "request exceeds maximum frame size",
        ));
    }
    let len = payload.len() as u32;
    writer.write_all(&len.to_be_bytes()).await?;
    writer.write_all(payload).await?;
    writer.flush().await?;
    Ok(())
}

/// Length-prefixed read matching the server's framing.
async fn read_frame<R>(reader: &mut R) -> io::Result<Vec<u8>>
where
    R: tokio::io::AsyncRead + Unpin,
{
    let mut len_buf = [0u8; 4];
    reader.read_exact(&mut len_buf).await?;
    let len = u32::from_be_bytes(len_buf) as usize;
    if len > MAX_FRAME_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "response frame exceeds maximum size",
        ));
    }
    let mut buf = vec![0u8; len];
    reader.read_exact(&mut buf).await?;
    Ok(buf)
}

/// Send `command`, print the result to stdout (pretty JSON on success, the error
/// to stderr on failure / unreachable app), and return the process exit code:
/// `0` ok, `1` RPC error, `2` transport/connection failure.
pub fn run_command(command: serde_json::Value) -> i32 {
    match send(command) {
        Ok(RpcResponse::Ok { data }) => {
            print_value(&data);
            0
        }
        Ok(RpcResponse::Error { error }) => {
            eprintln!("error[{}]: {}", error.code, error.message);
            1
        }
        Err(e) => {
            eprintln!("weft: {e}");
            2
        }
    }
}

/// Print a JSON value: a bare string prints unquoted (snapshot text reads
/// cleanly); anything else prints pretty JSON.
fn print_value(value: &serde_json::Value) {
    match value {
        serde_json::Value::String(s) => println!("{s}"),
        other => println!(
            "{}",
            serde_json::to_string_pretty(other).unwrap_or_else(|_| other.to_string())
        ),
    }
}
