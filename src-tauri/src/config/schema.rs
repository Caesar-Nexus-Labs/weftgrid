//! Typed config schema + version (P12a).
//!
//! `Config` is the single typed settings document persisted in the OS app-config
//! dir. Every field is strongly typed (no stringly-typed key bag). `schema_version`
//! drives `migration` so old configs load without data loss.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// Schema version of the on-disk config. Bump + add a migrator (see `migration`)
/// whenever the shape changes.
pub const CURRENT_SCHEMA_VERSION: u32 = 2;

/// Orientation of the in-pane surface tab bar (cmux's tab-layout setting).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TabLayout {
    #[default]
    Horizontal,
    Vertical,
}

/// Shell re-spawn policy on session restore (security-relevant — see `session`).
/// `FreshShell` is the safe default; `RerunLastCommand` is opt-in because a
/// restored command could be destructive.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum RespawnPolicy {
    #[default]
    FreshShell,
    RerunLastCommand,
}

/// UI + terminal color theme selection (names resolved by the UI layer).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ThemeConfig {
    /// UI chrome theme name.
    pub ui: String,
    /// Terminal color-scheme name.
    pub terminal_colors: String,
}

impl Default for ThemeConfig {
    fn default() -> Self {
        ThemeConfig {
            ui: "system".to_string(),
            terminal_colors: "default".to_string(),
        }
    }
}

/// The full typed settings document.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Config {
    /// On-disk schema version (drives migration).
    #[serde(default = "default_schema_version")]
    pub schema_version: u32,
    #[serde(default)]
    pub tab_layout: TabLayout,
    #[serde(default)]
    pub theme: ThemeConfig,
    /// Absolute path to the preferred shell; `None` = OS default.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub default_shell: Option<String>,
    /// Default shell re-spawn policy for session restore.
    #[serde(default)]
    pub default_respawn_policy: RespawnPolicy,
    /// Browser-import consent flag (P11 reads/writes). NEVER a credential.
    #[serde(default)]
    pub import_consent: bool,
    /// User keybinding overrides: action id -> chord (see `keybindings`).
    #[serde(default)]
    pub keybinding_overrides: BTreeMap<String, String>,
}

fn default_schema_version() -> u32 {
    CURRENT_SCHEMA_VERSION
}

impl Default for Config {
    fn default() -> Self {
        Config {
            schema_version: CURRENT_SCHEMA_VERSION,
            tab_layout: TabLayout::default(),
            theme: ThemeConfig::default(),
            default_shell: None,
            default_respawn_policy: RespawnPolicy::default(),
            import_consent: false,
            keybinding_overrides: BTreeMap::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_round_trip_preserves_every_field() {
        let mut overrides = BTreeMap::new();
        overrides.insert("palette.commands".to_string(), "ctrl+shift+k".to_string());
        let cfg = Config {
            schema_version: CURRENT_SCHEMA_VERSION,
            tab_layout: TabLayout::Vertical,
            theme: ThemeConfig {
                ui: "dark".to_string(),
                terminal_colors: "solarized".to_string(),
            },
            default_shell: Some("/bin/zsh".to_string()),
            default_respawn_policy: RespawnPolicy::RerunLastCommand,
            import_consent: true,
            keybinding_overrides: overrides,
        };
        let json = serde_json::to_string(&cfg).unwrap();
        let back: Config = serde_json::from_str(&json).unwrap();
        assert_eq!(cfg, back);
    }

    #[test]
    fn default_config_uses_current_schema_and_safe_defaults() {
        let cfg = Config::default();
        assert_eq!(cfg.schema_version, CURRENT_SCHEMA_VERSION);
        assert_eq!(cfg.default_respawn_policy, RespawnPolicy::FreshShell);
        assert!(!cfg.import_consent);
        assert!(cfg.default_shell.is_none());
    }

    #[test]
    fn missing_optional_fields_deserialize_to_defaults() {
        // A minimal document (only version) must fill defaults, not error.
        let back: Config = serde_json::from_str(r#"{"schema_version":2}"#).unwrap();
        assert_eq!(back.tab_layout, TabLayout::Horizontal);
        assert_eq!(back.theme, ThemeConfig::default());
    }
}
