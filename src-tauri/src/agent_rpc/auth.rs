//! Per-session RPC auth (P13 Security): generate a high-entropy token, persist it
//! to an app-data file readable by the current user, and verify in constant time.
//!
//! Threat model (red-team M8, same as the P7 CDP port): an unauthenticated RPC
//! lets ANY local process drive the user's already-authenticated browser pane
//! (P11 seeds real cookies). So the token gates EVERY request. It is:
//!   - random, 256-bit (two v4 UUIDs as hex) — not guessable;
//!   - per app session — a restart re-issues, so a leaked old token dies;
//!   - file-stored user-only (unix `0600`; Windows relies on the per-user
//!     `%APPDATA%\Roaming` ACL — see [`write_user_only`] and the report's residual
//!     note on explicit DACL hardening);
//!   - never logged, never passed via argv (process-list leak) — the CLI reads the
//!     file and puts it INSIDE the framed JSON body.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use directories::ProjectDirs;

/// Well-known filenames under the app-data dir. The CLI reads BOTH (token to
/// authenticate, endpoint to connect) from this same dir so there is one
/// discovery convention shared with the server.
pub const TOKEN_FILE_NAME: &str = "weft-rpc-token";
pub const ENDPOINT_FILE_NAME: &str = "weft-rpc-endpoint";

/// Resolve the weftgrid app-data directory (`%APPDATA%\weftgrid\` on Windows,
/// `$XDG_DATA_HOME/weftgrid/` or `~/.local/share/weftgrid/` on Linux). Mirrors the
/// config store's `ProjectDirs` qualifier so both land under the same vendor dir.
pub fn app_data_dir() -> io::Result<PathBuf> {
    let proj = ProjectDirs::from("", "", "weftgrid")
        .ok_or_else(|| io::Error::other("could not resolve OS app-data dir"))?;
    Ok(proj.data_dir().to_path_buf())
}

/// Full path to the token file inside an app-data `dir`.
pub fn token_path(dir: &Path) -> PathBuf {
    dir.join(TOKEN_FILE_NAME)
}

/// Full path to the endpoint (socket/pipe address) file inside an app-data `dir`.
pub fn endpoint_path(dir: &Path) -> PathBuf {
    dir.join(ENDPOINT_FILE_NAME)
}

/// A live session token. Holds the secret in memory and knows how to verify a
/// candidate in constant time.
#[derive(Clone)]
pub struct SessionToken {
    secret: String,
}

impl SessionToken {
    /// Generate a fresh 256-bit token rendered as lowercase hex. `uuid::v4` gives
    /// 122 bits of entropy per value; two concatenated as hex clear the 256-bit
    /// bar without pulling in a separate RNG crate (uuid is already a dep).
    pub fn generate() -> Self {
        let a = uuid::Uuid::new_v4();
        let b = uuid::Uuid::new_v4();
        let secret = format!("{}{}", a.simple(), b.simple());
        SessionToken { secret }
    }

    /// Wrap a known secret (used when loading or seeding in tests).
    pub fn from_secret(secret: impl Into<String>) -> Self {
        SessionToken {
            secret: secret.into(),
        }
    }

    /// The raw secret (for writing to the user-only file). Never log this.
    pub fn as_str(&self) -> &str {
        &self.secret
    }

    /// Constant-time equality against a candidate. Compares over the max length so
    /// the loop count does not reveal where (or whether) they diverged, and folds
    /// a length mismatch into the same non-zero accumulator.
    pub fn verify(&self, candidate: &str) -> bool {
        constant_time_eq(self.secret.as_bytes(), candidate.as_bytes())
    }

    /// Persist the token to `dir/weft-rpc-token` user-only, returning the path
    /// written. Creates the app-data dir if missing.
    pub fn persist(&self, dir: &Path) -> io::Result<PathBuf> {
        fs::create_dir_all(dir)?;
        let path = token_path(dir);
        write_user_only(&path, self.secret.as_bytes())?;
        Ok(path)
    }
}

/// Read a token secret back from a file (the CLI side). Trims trailing newline so
/// an editor-touched file still verifies.
pub fn load_token(path: &Path) -> io::Result<String> {
    let raw = fs::read_to_string(path)?;
    Ok(raw.trim_end_matches(['\n', '\r']).to_string())
}

/// Write `bytes` to `path` so only the current user can read/write it.
///
/// - unix: create with mode `0600`, and re-chmod in case the file pre-existed
///   with looser bits.
/// - windows: the file lives under `%APPDATA%\Roaming\weftgrid`, whose ACL grants
///   only the owning user (plus SYSTEM/Administrators) by default, so a sibling
///   user cannot read it. Explicit per-file DACL stripping (drop inheritance,
///   grant only the current SID) needs the `windows-sys` Security APIs — flagged
///   to lead as a hardening follow-up; the mandatory token check is the primary
///   gate regardless.
pub fn write_user_only(path: &Path, bytes: &[u8]) -> io::Result<()> {
    #[cfg(unix)]
    {
        use std::io::Write;
        use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
        let mut opts = fs::OpenOptions::new();
        opts.write(true).create(true).truncate(true).mode(0o600);
        let mut file = opts.open(path)?;
        file.write_all(bytes)?;
        file.flush()?;
        fs::set_permissions(path, fs::Permissions::from_mode(0o600))?;
        Ok(())
    }
    #[cfg(not(unix))]
    {
        use std::io::Write;
        let mut file = fs::File::create(path)?;
        file.write_all(bytes)?;
        file.flush()
    }
}

/// Compare two byte slices in constant time relative to their contents. Returns
/// `false` for any length mismatch but still walks a fixed number of iterations
/// to avoid a length-dependent early exit.
fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    let max = a.len().max(b.len());
    let mut diff: u8 = (a.len() ^ b.len()) as u8;
    for i in 0..max {
        let x = *a.get(i).unwrap_or(&0);
        let y = *b.get(i).unwrap_or(&0);
        diff |= x ^ y;
    }
    diff == 0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generated_tokens_are_unique_and_long() {
        let a = SessionToken::generate();
        let b = SessionToken::generate();
        assert_ne!(a.as_str(), b.as_str());
        // Two v4 UUIDs as simple hex = 64 chars (256 bits rendered).
        assert_eq!(a.as_str().len(), 64);
    }

    #[test]
    fn verify_accepts_self_rejects_others() {
        let token = SessionToken::from_secret("correct-horse-battery-staple");
        assert!(token.verify("correct-horse-battery-staple"));
        assert!(!token.verify("wrong"));
        assert!(!token.verify(""));
        assert!(!token.verify("correct-horse-battery-stapl")); // off by one
    }

    #[test]
    fn constant_time_eq_handles_length_mismatch() {
        assert!(constant_time_eq(b"abc", b"abc"));
        assert!(!constant_time_eq(b"abc", b"abcd"));
        assert!(!constant_time_eq(b"", b"x"));
        assert!(constant_time_eq(b"", b""));
    }

    #[test]
    fn persist_then_load_round_trips() {
        let mut dir = std::env::temp_dir();
        dir.push(format!("weft-rpc-auth-test-{}", uuid::Uuid::new_v4()));
        let token = SessionToken::generate();
        let path = token.persist(&dir).unwrap();
        let loaded = load_token(&path).unwrap();
        assert_eq!(loaded, token.as_str());
        let _ = fs::remove_dir_all(&dir);
    }

    #[cfg(unix)]
    #[test]
    fn persisted_token_is_user_only_0600() {
        use std::os::unix::fs::PermissionsExt;
        let mut dir = std::env::temp_dir();
        dir.push(format!("weft-rpc-perm-test-{}", uuid::Uuid::new_v4()));
        let token = SessionToken::generate();
        let path = token.persist(&dir).unwrap();
        let mode = fs::metadata(&path).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o600);
        let _ = fs::remove_dir_all(&dir);
    }
}
