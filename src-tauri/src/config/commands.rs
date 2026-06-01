//! Tauri command surface for the config track (P12a).
//!
//! Thin wrappers: each locks `ConfigState` and delegates to its logic (see
//! `state`). This is the IPC contract Wave-2 builds against — P15 (sidebar) calls
//! `workspace_*`, P16 (palette) calls `weft_*`, P3/P4/P15/P16 call
//! `keybinding_resolve`. Commands are registered once in `command_registry`
//! (last-wins `invoke_handler` constraint). Error type is `String` so failures
//! cross IPC as a rejected promise the TS client can `catch`.

use serde::Serialize;
use tauri::State;

use super::keybindings::Conflict;
use super::schema::Config;
use super::state::ConfigState;
use super::weft_config::WeftConfig;
use super::weft_trust::{CommandOrigin, TrustDecision};
use crate::model::{WorkspaceId, WorkspaceSnapshot};

/// One (action, chord) row for the keybinding editor UI.
#[derive(Debug, Serialize)]
pub struct KeybindingRow {
    pub action: String,
    pub chord: String,
}

/// Result of setting a binding: the (possibly empty) conflict list.
#[derive(Debug, Serialize)]
pub struct KeybindingSetResult {
    pub conflicts: Vec<ConflictDto>,
}

/// Serializable conflict pair for the UI.
#[derive(Debug, Serialize)]
pub struct ConflictDto {
    pub chord: String,
    pub action_a: String,
    pub action_b: String,
}

impl From<Conflict> for ConflictDto {
    fn from(c: Conflict) -> Self {
        ConflictDto {
            chord: c.chord,
            action_a: c.action_a,
            action_b: c.action_b,
        }
    }
}

// --- config ---

#[tauri::command]
pub fn config_get(state: State<'_, ConfigState>) -> Config {
    state.get_config()
}

#[tauri::command]
pub fn config_set(state: State<'_, ConfigState>, config: Config) -> Result<(), String> {
    state.set_config(config).map_err(|e| e.to_string())
}

// --- workspace store (P15) ---

#[tauri::command]
pub fn workspace_snapshot(state: State<'_, ConfigState>) -> Vec<WorkspaceSnapshot> {
    state.workspace_snapshot()
}

#[tauri::command]
pub fn workspace_add(state: State<'_, ConfigState>, title: String, cwd: String) -> WorkspaceId {
    state.workspace_add(title, cwd)
}

#[tauri::command]
pub fn workspace_remove(state: State<'_, ConfigState>, id: WorkspaceId) -> bool {
    state.workspace_remove(id)
}

#[tauri::command]
pub fn workspace_select(state: State<'_, ConfigState>, id: WorkspaceId) -> bool {
    state.workspace_select(id)
}

#[tauri::command]
pub fn workspace_reorder(state: State<'_, ConfigState>, from: usize, to: usize) -> bool {
    state.workspace_reorder(from, to)
}

// --- weft.json (P16) ---

#[tauri::command]
pub fn weft_defs_get(state: State<'_, ConfigState>, content: String) -> Result<WeftConfig, String> {
    state.weft_defs(&content).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn weft_trust_check(
    state: State<'_, ConfigState>,
    content: String,
    command_name: String,
    project_local: bool,
    source_path: String,
) -> Result<bool, String> {
    let origin = if project_local {
        CommandOrigin::ProjectLocal
    } else {
        CommandOrigin::Global
    };
    state
        .weft_trust_check(&content, &command_name, origin, &source_path)
        .map(|d| d == TrustDecision::NeedsConfirm)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn weft_trust_grant(
    state: State<'_, ConfigState>,
    content: String,
    command_name: String,
    source_path: String,
) -> Result<bool, String> {
    state
        .weft_trust_grant(&content, &command_name, &source_path)
        .map_err(|e| e.to_string())
}

// --- keybindings (P3/P4/P15/P16) ---

#[tauri::command]
pub fn keybinding_resolve(state: State<'_, ConfigState>, action: String) -> Option<String> {
    state.keybinding_resolve(&action)
}

#[tauri::command]
pub fn keybinding_list(state: State<'_, ConfigState>) -> Vec<KeybindingRow> {
    state
        .keybinding_list()
        .into_iter()
        .map(|(action, chord)| KeybindingRow { action, chord })
        .collect()
}

#[tauri::command]
pub fn keybinding_set(
    state: State<'_, ConfigState>,
    action: String,
    chord: String,
) -> Result<KeybindingSetResult, String> {
    state
        .keybinding_set(&action, &chord)
        .map(|conflicts| KeybindingSetResult {
            conflicts: conflicts.into_iter().map(ConflictDto::from).collect(),
        })
        .map_err(|e| e.to_string())
}
