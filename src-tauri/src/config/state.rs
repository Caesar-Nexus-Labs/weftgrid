//! Shared config-track state + its testable logic (P12a).
//!
//! `ConfigState` is the single `.manage()`d value the Tauri commands lock and
//! delegate to. Logic lives here (testable without a running app); `commands`
//! holds only thin `#[tauri::command]` wrappers that lock the mutex and call
//! these methods. State = the live `Config`, its disk `ConfigStore`, the live
//! `WorkspaceStore`, the `TrustStore`, and the resolved `KeybindingRegistry`.

use std::sync::Mutex;

use crate::model::{Panel, Workspace, WorkspaceId, WorkspaceSnapshot, WorkspaceStore};
use uuid::Uuid;

use super::keybindings::{Conflict, KeybindingRegistry};
use super::schema::Config;
use super::store::{ConfigStore, StoreError};
use super::weft_config::{parse_weft_config, WeftConfig, WeftConfigError};
use super::weft_trust::{CommandOrigin, TrustDecision, TrustStore};

/// Mutable inner state, guarded by one mutex (config writes are infrequent).
pub struct ConfigInner {
    pub config: Config,
    pub store: ConfigStore,
    pub workspaces: WorkspaceStore,
    pub trust: TrustStore,
    pub keybindings: KeybindingRegistry,
}

/// `.manage()`d wrapper exposing locking + delegated logic.
pub struct ConfigState {
    inner: Mutex<ConfigInner>,
}

impl ConfigState {
    /// Build state from a config directory: load+migrate config, derive the
    /// keybinding registry from its overrides, open the disk-backed trust store.
    pub fn from_dir(dir: impl Into<std::path::PathBuf>) -> Result<Self, StoreError> {
        let dir = dir.into();
        let store = ConfigStore::with_dir(&dir);
        let config = store.load()?;
        let keybindings = KeybindingRegistry::with_overrides(&config.keybinding_overrides);
        Ok(ConfigState {
            inner: Mutex::new(ConfigInner {
                config,
                store,
                workspaces: WorkspaceStore::default(),
                trust: TrustStore::with_dir(&dir),
                keybindings,
            }),
        })
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, ConfigInner> {
        self.inner.lock().expect("config state mutex poisoned")
    }

    // --- config ---

    pub fn get_config(&self) -> Config {
        self.lock().config.clone()
    }

    /// Replace + persist the config, rederiving the keybinding registry.
    pub fn set_config(&self, config: Config) -> Result<(), StoreError> {
        let mut inner = self.lock();
        inner.store.save(&config)?;
        inner.keybindings = KeybindingRegistry::with_overrides(&config.keybinding_overrides);
        inner.config = config;
        Ok(())
    }

    // --- workspace store ---

    pub fn workspace_snapshot(&self) -> Vec<WorkspaceSnapshot> {
        super::workspace_store::snapshot(&self.lock().workspaces)
    }

    /// Create a local workspace with one terminal pane; returns its id.
    pub fn workspace_add(&self, title: String, cwd: String) -> WorkspaceId {
        let id = Uuid::new_v4();
        let ws = Workspace::new_local(id, title, cwd, Panel::terminal(Uuid::new_v4()));
        let mut inner = self.lock();
        super::workspace_store::add(&mut inner.workspaces, ws);
        id
    }

    pub fn workspace_remove(&self, id: WorkspaceId) -> bool {
        super::workspace_store::remove(&mut self.lock().workspaces, id).is_some()
    }

    pub fn workspace_select(&self, id: WorkspaceId) -> bool {
        super::workspace_store::select(&mut self.lock().workspaces, id)
    }

    pub fn workspace_reorder(&self, from: usize, to: usize) -> bool {
        super::workspace_store::reorder(&mut self.lock().workspaces, from, to)
    }

    // --- weft.json ---

    pub fn weft_defs(&self, content: &str) -> Result<WeftConfig, WeftConfigError> {
        parse_weft_config(content)
    }

    /// Decide whether a project-local/global command may run unconfirmed.
    pub fn weft_trust_check(
        &self,
        content: &str,
        command_name: &str,
        origin: CommandOrigin,
        source_path: &str,
    ) -> Result<TrustDecision, WeftConfigError> {
        let cfg = parse_weft_config(content)?;
        let cmd = cfg
            .commands
            .into_iter()
            .find(|c| c.name == command_name)
            .ok_or_else(|| WeftConfigError::CommandNotFound(command_name.to_string()))?;
        Ok(self.lock().trust.decide(&cmd, origin, source_path))
    }

    /// Persist trust for a named command from a project config.
    pub fn weft_trust_grant(
        &self,
        content: &str,
        command_name: &str,
        source_path: &str,
    ) -> Result<bool, WeftConfigError> {
        let cfg = parse_weft_config(content)?;
        let cmd = cfg
            .commands
            .into_iter()
            .find(|c| c.name == command_name)
            .ok_or_else(|| WeftConfigError::CommandNotFound(command_name.to_string()))?;
        self.lock().trust.grant(&cmd, source_path);
        Ok(true)
    }

    // --- keybindings ---

    pub fn keybinding_resolve(&self, action: &str) -> Option<String> {
        self.lock().keybindings.resolve(action).map(str::to_string)
    }

    pub fn keybinding_list(&self) -> Vec<(String, String)> {
        self.lock().keybindings.list()
    }

    /// Set a binding, persist it into config overrides, return any conflicts.
    pub fn keybinding_set(&self, action: &str, chord: &str) -> Result<Vec<Conflict>, StoreError> {
        let mut inner = self.lock();
        inner.keybindings.set(action, chord);
        inner
            .config
            .keybinding_overrides
            .insert(action.to_string(), chord.to_string());
        let config = inner.config.clone();
        inner.store.save(&config)?;
        Ok(inner.keybindings.detect_conflicts())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_state() -> (ConfigState, std::path::PathBuf) {
        let mut dir = std::env::temp_dir();
        dir.push(format!("weftgrid-state-{}", Uuid::new_v4()));
        (ConfigState::from_dir(&dir).unwrap(), dir)
    }

    #[test]
    fn config_get_set_persists() {
        let (state, dir) = temp_state();
        let mut cfg = state.get_config();
        cfg.import_consent = true;
        state.set_config(cfg.clone()).unwrap();
        // Re-open from disk to prove persistence.
        let reopened = ConfigState::from_dir(&dir).unwrap();
        assert!(reopened.get_config().import_consent);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn workspace_lifecycle_through_state() {
        let (state, dir) = temp_state();
        let a = state.workspace_add("a".into(), "/a".into());
        let b = state.workspace_add("b".into(), "/b".into());
        assert_eq!(state.workspace_snapshot().len(), 2);
        assert!(state.workspace_select(a));
        assert!(state.workspace_reorder(0, 1));
        assert!(state.workspace_remove(b));
        assert_eq!(state.workspace_snapshot().len(), 1);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn weft_trust_flow_through_state() {
        let (state, dir) = temp_state();
        let content = r#"{ "commands": [ { "name": "Deploy", "command": "./deploy.sh" } ] }"#;
        let src = "/proj/weft.json";
        assert_eq!(
            state
                .weft_trust_check(content, "Deploy", CommandOrigin::ProjectLocal, src)
                .unwrap(),
            TrustDecision::NeedsConfirm
        );
        state.weft_trust_grant(content, "Deploy", src).unwrap();
        assert_eq!(
            state
                .weft_trust_check(content, "Deploy", CommandOrigin::ProjectLocal, src)
                .unwrap(),
            TrustDecision::AllowUnconfirmed
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn keybinding_set_persists_override_and_reports_conflict() {
        let (state, dir) = temp_state();
        // Re-bind switcher onto the commands chord → conflict surfaced.
        let conflicts = state
            .keybinding_set("palette.switcher", "ctrl+shift+p")
            .unwrap();
        assert!(!conflicts.is_empty());
        // Override persisted to config.
        let reopened = ConfigState::from_dir(&dir).unwrap();
        assert_eq!(
            reopened.keybinding_resolve("palette.switcher").as_deref(),
            Some("ctrl+shift+p")
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}
