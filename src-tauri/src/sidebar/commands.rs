//! Tauri command surface for the sidebar track (P15b).
//!
//! Only the default-off expensive-scan TOGGLES are exposed as commands — the UI
//! flips them, a scan runner checks them before spawning `lsof`/`ss`/`git status`.
//! They mutate the `.manage()`d [`SidebarState`]. The metadata push RECEIVER is
//! deliberately NOT a command: the P13 RPC server runs in-process and calls
//! [`super::report_receiver::receive_report`] directly (see that module's seam
//! docs), so there is nothing for an external IPC caller to invoke.
//!
//! Commands are registered once in `command_registry` (last-wins `invoke_handler`
//! constraint) — this module adds no handler. Names reported to lead.

use tauri::State;

use super::state::SidebarState;

#[tauri::command]
pub fn sidebar_port_scan_enabled(state: State<'_, SidebarState>) -> bool {
    state.port_scan_enabled()
}

#[tauri::command]
pub fn sidebar_set_port_scan(state: State<'_, SidebarState>, enabled: bool) {
    state.set_port_scan(enabled);
}

#[tauri::command]
pub fn sidebar_git_watch_enabled(state: State<'_, SidebarState>) -> bool {
    state.git_watch_enabled()
}

#[tauri::command]
pub fn sidebar_set_git_watch(state: State<'_, SidebarState>, enabled: bool) {
    state.set_git_watch(enabled);
}
