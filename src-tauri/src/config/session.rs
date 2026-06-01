//! Session persist/restore (P12b).
//!
//! On change/exit weftgrid persists enough to relaunch the prior layout: the
//! serialized split-tree per workspace (P4 owns the tree *shape*; this stores an
//! opaque blob of it) plus per-pane respawn metadata (cwd/shell/policy). On
//! restore we hand the tree blob back to P4 to deserialize and emit the list of
//! shells P3 must spawn — applying the respawn policy (default `FreshShell`;
//! `RerunLastCommand` only when the pane opted in).
//!
//! Re-running a stored command can be destructive, so `FreshShell` is the default
//! and `RerunLastCommand` must be explicit per pane. Persistence is atomic
//! (temp+rename) via the same path as `ConfigStore`.

use std::fs;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::schema::RespawnPolicy;
use super::store::{write_synced, StoreError};
use crate::model::{PaneId, WorkspaceId};

/// Per-pane respawn metadata captured at persist time.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PaneSessionState {
    pub pane_id: PaneId,
    pub cwd: String,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub shell: Option<String>,
    #[serde(default)]
    pub respawn_policy: RespawnPolicy,
    /// Last command run in the pane; only replayed when policy is
    /// `RerunLastCommand`.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub last_command: Option<String>,
}

/// One workspace's persisted session: its id, the opaque serialized layout tree
/// (P4's `LayoutNode` JSON), and per-pane respawn metadata.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WorkspaceSessionState {
    pub workspace_id: WorkspaceId,
    pub title: String,
    /// Opaque P4 split-tree blob (stored verbatim; not interpreted here).
    pub serialized_tree: Value,
    #[serde(default)]
    pub panes: Vec<PaneSessionState>,
}

/// The full persisted session across all workspaces.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct SessionState {
    #[serde(default)]
    pub workspaces: Vec<WorkspaceSessionState>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub selected_index: Option<usize>,
}

/// A shell to spawn on restore, resolved from a pane's respawn policy. The
/// `initial_command` is `Some` only when the pane opted into rerun.
#[derive(Debug, Clone, PartialEq)]
pub struct ShellSpawn {
    pub workspace_id: WorkspaceId,
    pub pane_id: PaneId,
    pub cwd: String,
    pub shell: Option<String>,
    pub initial_command: Option<String>,
}

impl SessionState {
    /// Resolve every pane into a `ShellSpawn`, honouring respawn policy. P3 spawns
    /// these; P4 restores the tree shape from `serialized_tree` in parallel.
    pub fn shells_to_spawn(&self) -> Vec<ShellSpawn> {
        let mut out = Vec::new();
        for ws in &self.workspaces {
            for pane in &ws.panes {
                let initial_command = match pane.respawn_policy {
                    RespawnPolicy::FreshShell => None,
                    RespawnPolicy::RerunLastCommand => pane.last_command.clone(),
                };
                out.push(ShellSpawn {
                    workspace_id: ws.workspace_id,
                    pane_id: pane.pane_id,
                    cwd: pane.cwd.clone(),
                    shell: pane.shell.clone(),
                    initial_command,
                });
            }
        }
        out
    }
}

const SESSION_FILE_NAME: &str = "session.json";
const SESSION_TEMP_NAME: &str = "session.json.tmp";

/// Persists/restores the `SessionState` document under the app-config dir.
#[derive(Debug, Clone)]
pub struct SessionStore {
    dir: PathBuf,
}

impl SessionStore {
    pub fn with_dir(dir: impl Into<PathBuf>) -> Self {
        SessionStore { dir: dir.into() }
    }

    pub fn session_path(&self) -> PathBuf {
        self.dir.join(SESSION_FILE_NAME)
    }

    /// Restore the session, or an empty one when none exists yet.
    pub fn restore(&self) -> Result<SessionState, StoreError> {
        let path = self.session_path();
        if !path.exists() {
            return Ok(SessionState::default());
        }
        let raw = fs::read_to_string(&path)?;
        serde_json::from_str(&raw).map_err(StoreError::Serialize)
    }

    /// Atomically persist the session (temp+rename, same invariant as config).
    pub fn persist(&self, session: &SessionState) -> Result<(), StoreError> {
        fs::create_dir_all(&self.dir)?;
        let json = serde_json::to_string_pretty(session).map_err(StoreError::Serialize)?;
        let temp = self.dir.join(SESSION_TEMP_NAME);
        write_synced(&temp, json.as_bytes())?;
        fs::rename(&temp, self.session_path())?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use uuid::Uuid;

    fn sample_session() -> SessionState {
        let wid = Uuid::new_v4();
        let p1 = Uuid::new_v4();
        let p2 = Uuid::new_v4();
        SessionState {
            workspaces: vec![WorkspaceSessionState {
                workspace_id: wid,
                title: "proj".to_string(),
                // A nested split tree blob (opaque P4 shape).
                serialized_tree: json!({
                    "type": "split",
                    "orientation": "horizontal",
                    "divider_position": 0.5,
                    "first": { "type": "pane", "panel_ids": [p1.to_string()] },
                    "second": { "type": "pane", "panel_ids": [p2.to_string()] }
                }),
                panes: vec![
                    PaneSessionState {
                        pane_id: p1,
                        cwd: "/proj".to_string(),
                        shell: Some("/bin/zsh".to_string()),
                        respawn_policy: RespawnPolicy::FreshShell,
                        last_command: Some("npm run dev".to_string()),
                    },
                    PaneSessionState {
                        pane_id: p2,
                        cwd: "/proj/api".to_string(),
                        shell: None,
                        respawn_policy: RespawnPolicy::RerunLastCommand,
                        last_command: Some("cargo watch".to_string()),
                    },
                ],
            }],
            selected_index: Some(0),
        }
    }

    #[test]
    fn persist_restore_round_trips_complex_tree() {
        let mut dir = std::env::temp_dir();
        dir.push(format!("weftgrid-session-{}", Uuid::new_v4()));
        let store = SessionStore::with_dir(&dir);
        let session = sample_session();
        store.persist(&session).unwrap();
        let restored = store.restore().unwrap();
        assert_eq!(session, restored);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn restore_missing_returns_empty() {
        let mut dir = std::env::temp_dir();
        dir.push(format!("weftgrid-session-none-{}", Uuid::new_v4()));
        let store = SessionStore::with_dir(&dir);
        assert_eq!(store.restore().unwrap(), SessionState::default());
    }

    #[test]
    fn respawn_policy_drives_initial_command() {
        let session = sample_session();
        let spawns = session.shells_to_spawn();
        assert_eq!(spawns.len(), 2);
        // Fresh shell pane → no replayed command (safe default).
        assert_eq!(spawns[0].initial_command, None);
        // Opt-in rerun pane → replays last command.
        assert_eq!(spawns[1].initial_command.as_deref(), Some("cargo watch"));
    }
}
