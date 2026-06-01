//! Deterministic config migration chain (P12a).
//!
//! Old on-disk configs carry an older `schema_version`. `migrate_to_current`
//! walks the registry of single-step migrators in order until the document is at
//! `CURRENT_SCHEMA_VERSION`, so no upgrade path is ever skipped. Each bump MUST
//! add one migrator + a round-trip test (see tests below).
//!
//! Migrators operate on `serde_json::Value` (not the typed `Config`) because an
//! old document predates fields the typed struct now requires — parsing it as
//! `Config` first would be lossy/ambiguous.

use serde_json::{json, Value};

use super::schema::{Config, CURRENT_SCHEMA_VERSION};

/// Error from a failed migration step.
#[derive(Debug, PartialEq, Eq)]
pub enum MigrationError {
    /// The document's version is newer than this build understands.
    FutureVersion(u32),
    /// No migrator registered for a version we still need to advance past.
    MissingMigrator(u32),
    /// Stored document is not a JSON object.
    NotAnObject,
}

impl std::fmt::Display for MigrationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            MigrationError::FutureVersion(v) => {
                write!(
                    f,
                    "config schema_version {v} is newer than supported {CURRENT_SCHEMA_VERSION}"
                )
            }
            MigrationError::MissingMigrator(v) => write!(f, "no migrator from schema_version {v}"),
            MigrationError::NotAnObject => write!(f, "config document is not a JSON object"),
        }
    }
}

impl std::error::Error for MigrationError {}

/// Read the `schema_version` of a raw document, defaulting to 1 when absent
/// (the very first releases had no version field).
fn read_version(doc: &Value) -> Result<u32, MigrationError> {
    let obj = doc.as_object().ok_or(MigrationError::NotAnObject)?;
    Ok(obj
        .get("schema_version")
        .and_then(Value::as_u64)
        .map(|v| v as u32)
        .unwrap_or(1))
}

/// Apply the single-step migrator that advances `from` → `from + 1`.
fn step(from: u32, doc: Value) -> Result<Value, MigrationError> {
    match from {
        1 => Ok(migrate_v1_to_v2(doc)),
        other => Err(MigrationError::MissingMigrator(other)),
    }
}

/// v1 → v2: introduced `default_respawn_policy` (session-restore) and
/// `keybinding_overrides`. Old configs lacked both — fill safe defaults.
fn migrate_v1_to_v2(mut doc: Value) -> Value {
    if let Some(obj) = doc.as_object_mut() {
        obj.entry("default_respawn_policy")
            .or_insert_with(|| json!("fresh-shell"));
        obj.entry("keybinding_overrides")
            .or_insert_with(|| json!({}));
        obj.insert("schema_version".to_string(), json!(2));
    }
    doc
}

/// Migrate a raw stored document up to the current schema, then parse it into the
/// typed `Config`. Walks one version at a time so every step's migrator runs.
pub fn migrate_to_current(mut doc: Value) -> Result<Config, MigrationError> {
    let mut version = read_version(&doc)?;
    if version > CURRENT_SCHEMA_VERSION {
        return Err(MigrationError::FutureVersion(version));
    }
    while version < CURRENT_SCHEMA_VERSION {
        doc = step(version, doc)?;
        version = read_version(&doc)?;
    }
    let cfg: Config = serde_json::from_value(doc).map_err(|_| MigrationError::NotAnObject)?;
    Ok(cfg)
}

#[cfg(test)]
mod tests {
    use super::super::schema::{RespawnPolicy, TabLayout};
    use super::*;

    #[test]
    fn v1_config_migrates_to_v2_without_losing_fields() {
        // v1 fixture: no version field, no respawn policy / overrides.
        let v1 = json!({
            "tab_layout": "vertical",
            "theme": { "ui": "dark", "terminal_colors": "solarized" },
            "default_shell": "/bin/zsh",
            "import_consent": true
        });
        let cfg = migrate_to_current(v1).unwrap();
        assert_eq!(cfg.schema_version, CURRENT_SCHEMA_VERSION);
        // Preserved v1 data:
        assert_eq!(cfg.tab_layout, TabLayout::Vertical);
        assert_eq!(cfg.theme.ui, "dark");
        assert_eq!(cfg.default_shell.as_deref(), Some("/bin/zsh"));
        assert!(cfg.import_consent);
        // Filled v2 defaults:
        assert_eq!(cfg.default_respawn_policy, RespawnPolicy::FreshShell);
        assert!(cfg.keybinding_overrides.is_empty());
    }

    #[test]
    fn explicit_v1_version_field_also_migrates() {
        let v1 = json!({ "schema_version": 1, "tab_layout": "horizontal" });
        let cfg = migrate_to_current(v1).unwrap();
        assert_eq!(cfg.schema_version, 2);
    }

    #[test]
    fn current_version_is_passthrough() {
        let doc = serde_json::to_value(Config::default()).unwrap();
        let cfg = migrate_to_current(doc).unwrap();
        assert_eq!(cfg, Config::default());
    }

    #[test]
    fn future_version_is_rejected() {
        let doc = json!({ "schema_version": 99 });
        assert_eq!(
            migrate_to_current(doc),
            Err(MigrationError::FutureVersion(99))
        );
    }
}
