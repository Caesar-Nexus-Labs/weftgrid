//! Config persistence with atomic writes (P12a).
//!
//! `ConfigStore` owns the on-disk `config.json` under the OS app-config dir
//! (`%APPDATA%\weftgrid\` on Windows, `$XDG_CONFIG_HOME/weftgrid/` or
//! `~/.config/weftgrid/` on Linux). Invariant: a crash mid-write must NEVER
//! corrupt the live config — we write a sibling `*.tmp` then atomically rename
//! over the target (`fs::rename` maps to MoveFileEx/REPLACE_EXISTING on Windows,
//! rename(2) on POSIX). Loads run the migration chain so old documents survive.
//!
//! The store takes its directory by parameter (no hardcoded global path) so it is
//! testable against a temp dir without polluting the user's real settings.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use directories::ProjectDirs;

use super::migration::migrate_to_current;
use super::schema::Config;

const CONFIG_FILE_NAME: &str = "config.json";
const TEMP_FILE_NAME: &str = "config.json.tmp";

/// Owns the config directory + atomic load/save of `config.json`.
#[derive(Debug, Clone)]
pub struct ConfigStore {
    dir: PathBuf,
}

/// Errors surfaced by the store.
#[derive(Debug)]
pub enum StoreError {
    Io(io::Error),
    Serialize(serde_json::Error),
    /// Could not resolve the OS app-config dir.
    NoConfigDir,
    /// Stored document could not be migrated/parsed.
    Migration(String),
}

impl std::fmt::Display for StoreError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            StoreError::Io(e) => write!(f, "config io error: {e}"),
            StoreError::Serialize(e) => write!(f, "config serialize error: {e}"),
            StoreError::NoConfigDir => write!(f, "could not resolve OS app-config dir"),
            StoreError::Migration(m) => write!(f, "config migration error: {m}"),
        }
    }
}

impl std::error::Error for StoreError {}

impl From<io::Error> for StoreError {
    fn from(e: io::Error) -> Self {
        StoreError::Io(e)
    }
}

impl ConfigStore {
    /// Store rooted at an explicit directory (used in tests + when the caller
    /// already resolved the path).
    pub fn with_dir(dir: impl Into<PathBuf>) -> Self {
        ConfigStore { dir: dir.into() }
    }

    /// Store rooted at the OS app-config dir for weftgrid.
    pub fn from_os_dirs() -> Result<Self, StoreError> {
        let proj = ProjectDirs::from("", "", "weftgrid").ok_or(StoreError::NoConfigDir)?;
        Ok(ConfigStore::with_dir(proj.config_dir()))
    }

    /// Path to the live `config.json`.
    pub fn config_path(&self) -> PathBuf {
        self.dir.join(CONFIG_FILE_NAME)
    }

    fn temp_path(&self) -> PathBuf {
        self.dir.join(TEMP_FILE_NAME)
    }

    /// Load + migrate the config. Returns `Config::default()` (without writing)
    /// when no file exists yet, so first launch needs no special-casing.
    pub fn load(&self) -> Result<Config, StoreError> {
        let path = self.config_path();
        if !path.exists() {
            return Ok(Config::default());
        }
        let raw = fs::read_to_string(&path)?;
        let value: serde_json::Value = serde_json::from_str(&raw).map_err(StoreError::Serialize)?;
        migrate_to_current(value).map_err(|e| StoreError::Migration(e.to_string()))
    }

    /// Atomically persist the config: serialize → write `*.tmp` (flushed) →
    /// rename over `config.json`. The rename is the commit point; if the process
    /// dies before it, the previous `config.json` is untouched.
    pub fn save(&self, config: &Config) -> Result<(), StoreError> {
        fs::create_dir_all(&self.dir)?;
        let json = serde_json::to_string_pretty(config).map_err(StoreError::Serialize)?;
        let temp = self.temp_path();
        write_synced(&temp, json.as_bytes())?;
        atomic_rename(&temp, &self.config_path())?;
        Ok(())
    }
}

/// Write bytes to `path` and fsync before returning so the temp file's contents
/// are durable prior to the rename commit. Shared with `weft_trust`/`session`
/// for their own atomic-ish persistence.
pub(crate) fn write_synced(path: &Path, bytes: &[u8]) -> io::Result<()> {
    use std::io::Write;
    let mut file = fs::File::create(path)?;
    file.write_all(bytes)?;
    file.flush()?;
    file.sync_all()?;
    Ok(())
}

/// Rename `from` over `to`. `fs::rename` replaces an existing target on both
/// Windows (MoveFileEx + MOVEFILE_REPLACE_EXISTING) and POSIX (rename(2)). On
/// Windows a transient lock (AV/indexer) can fail it, so retry a few times.
fn atomic_rename(from: &Path, to: &Path) -> io::Result<()> {
    let mut last_err = None;
    for _ in 0..5 {
        match fs::rename(from, to) {
            Ok(()) => return Ok(()),
            Err(e) => last_err = Some(e),
        }
    }
    Err(last_err.unwrap_or_else(|| io::Error::other("rename failed")))
}

#[cfg(test)]
mod tests {
    use super::super::schema::TabLayout;
    use super::*;

    fn temp_dir(tag: &str) -> PathBuf {
        let mut p = std::env::temp_dir();
        p.push(format!("weftgrid-cfg-test-{tag}-{}", uuid::Uuid::new_v4()));
        p
    }

    #[test]
    fn load_returns_default_when_no_file() {
        let dir = temp_dir("nofile");
        let store = ConfigStore::with_dir(&dir);
        assert_eq!(store.load().unwrap(), Config::default());
    }

    #[test]
    fn save_then_load_round_trips() {
        let dir = temp_dir("roundtrip");
        let store = ConfigStore::with_dir(&dir);
        let cfg = Config {
            tab_layout: TabLayout::Vertical,
            import_consent: true,
            ..Default::default()
        };
        store.save(&cfg).unwrap();
        assert_eq!(store.load().unwrap(), cfg);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn crash_mid_write_leaves_previous_config_intact() {
        // Persist a good config, then simulate a crash DURING a second write by
        // writing only the temp file (no rename). The live config must be the
        // first (intact) one, not corrupt/empty.
        let dir = temp_dir("crash");
        let store = ConfigStore::with_dir(&dir);
        let original = Config::default();
        store.save(&original).unwrap();

        // Simulate interrupted write: temp written but rename never happened.
        let half = Config {
            import_consent: true,
            ..Default::default()
        };
        let json = serde_json::to_string_pretty(&half).unwrap();
        write_synced(&store.temp_path(), json.as_bytes()).unwrap();

        // Live config still parses and equals the original (no corruption).
        assert_eq!(store.load().unwrap(), original);

        // Completing the rename commits the new config.
        atomic_rename(&store.temp_path(), &store.config_path()).unwrap();
        assert_eq!(store.load().unwrap(), half);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn save_overwrites_existing_config() {
        let dir = temp_dir("overwrite");
        let store = ConfigStore::with_dir(&dir);
        store.save(&Config::default()).unwrap();
        let updated = Config {
            default_shell: Some("/bin/bash".into()),
            ..Config::default()
        };
        store.save(&updated).unwrap();
        assert_eq!(store.load().unwrap(), updated);
        let _ = fs::remove_dir_all(&dir);
    }
}
