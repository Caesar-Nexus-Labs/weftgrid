//! Trust gate for `weft.json` shell commands (P12a; security-critical).
//!
//! Running arbitrary shell from a repo's `weft.json` is dangerous. Mirroring cmux
//! `CmuxActionTrust`, the rule is:
//!   - `confirm: true` on a command forces a prompt every time, regardless of
//!     origin or prior trust — an explicit per-command "always ask me" opt-in.
//!   - otherwise, a command from the GLOBAL config (user's own
//!     `~/.config/weftgrid/weft.json`) runs unconfirmed.
//!   - otherwise, a command from a PROJECT-LOCAL config (a repo's `weft.json`)
//!     requires a confirm prompt UNLESS its fingerprint was previously granted
//!     ("Trust and Run").
//!
//! The fingerprint is a SHA-256 over the command's identity (name + command/
//! workspace + source path) so editing the command re-triggers confirmation.
//! Trusted fingerprints persist as a JSON set in the app-config dir.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use super::store::write_synced;
use super::weft_config::WeftCommand;

/// The decision for a given (command, origin) pair.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrustDecision {
    /// Safe to run without prompting.
    AllowUnconfirmed,
    /// Must show a confirm dialog before running.
    NeedsConfirm,
}

/// Where a command originated, used to decide the trust gate.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommandOrigin {
    /// User's own global config — implicitly trusted.
    Global,
    /// A project's `weft.json` — gated.
    ProjectLocal,
}

/// Persistent set of trusted command fingerprints.
#[derive(Debug, Clone, Default)]
pub struct TrustStore {
    fingerprints: BTreeSet<String>,
    path: Option<PathBuf>,
}

const TRUST_FILE_NAME: &str = "trusted-commands.json";

impl TrustStore {
    /// In-memory store with no disk backing (tests, ephemeral).
    pub fn in_memory() -> Self {
        TrustStore::default()
    }

    /// Store backed by `<dir>/trusted-commands.json`, loading any existing set.
    pub fn with_dir(dir: impl Into<PathBuf>) -> Self {
        let dir = dir.into();
        let path = dir.join(TRUST_FILE_NAME);
        let fingerprints = std::fs::read_to_string(&path)
            .ok()
            .and_then(|raw| serde_json::from_str::<Vec<String>>(&raw).ok())
            .map(|v| v.into_iter().collect())
            .unwrap_or_default();
        TrustStore {
            fingerprints,
            path: Some(path),
        }
    }

    /// Decide whether `command` from `origin` may run unconfirmed.
    pub fn decide(
        &self,
        command: &WeftCommand,
        origin: CommandOrigin,
        source_path: &str,
    ) -> TrustDecision {
        // `confirm: true` is an explicit per-command "always ask me" opt-in and
        // wins over trust from EITHER origin — including the user's own global
        // config (a destructive global command can still demand a prompt every
        // run). Checked before the global short-circuit so the opt-in is honored.
        if command.confirm == Some(true) {
            return TrustDecision::NeedsConfirm;
        }
        // Global config is otherwise implicitly trusted.
        if origin == CommandOrigin::Global {
            return TrustDecision::AllowUnconfirmed;
        }
        if self
            .fingerprints
            .contains(&fingerprint(command, source_path))
        {
            TrustDecision::AllowUnconfirmed
        } else {
            TrustDecision::NeedsConfirm
        }
    }

    /// Persist trust for a command (the user picked "Trust and Run").
    pub fn grant(&mut self, command: &WeftCommand, source_path: &str) {
        self.fingerprints.insert(fingerprint(command, source_path));
        self.save();
    }

    /// Whether a command's fingerprint is currently trusted.
    pub fn is_trusted(&self, command: &WeftCommand, source_path: &str) -> bool {
        self.fingerprints
            .contains(&fingerprint(command, source_path))
    }

    fn save(&self) {
        let Some(path) = &self.path else { return };
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let sorted: Vec<&String> = self.fingerprints.iter().collect();
        if let Ok(json) = serde_json::to_vec(&sorted) {
            let _ = write_synced(path, &json);
        }
    }
}

/// Stable SHA-256 fingerprint over the command's executable identity + origin
/// path. Editing the command (or moving the config) invalidates trust.
fn fingerprint(command: &WeftCommand, source_path: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(command.id().as_bytes());
    hasher.update(b"\0");
    hasher.update(command.command.as_deref().unwrap_or("").as_bytes());
    hasher.update(b"\0");
    // Workspace builder identity (serialized) so a workspace command is also keyed.
    if let Some(ws) = &command.workspace {
        if let Ok(s) = serde_json::to_string(ws) {
            hasher.update(s.as_bytes());
        }
    }
    hasher.update(b"\0");
    hasher.update(canonical(source_path).as_bytes());
    hex(&hasher.finalize())
}

/// Best-effort canonical path (falls back to the input when it doesn't exist yet).
fn canonical(path: &str) -> String {
    Path::new(path)
        .canonicalize()
        .ok()
        .and_then(|p| p.to_str().map(str::to_string))
        .unwrap_or_else(|| path.to_string())
}

fn hex(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        s.push_str(&format!("{b:02x}"));
    }
    s
}

// --- minimal dependency-free SHA-256 (avoids adding a crate just for trust keys) ---

struct Sha256 {
    state: [u32; 8],
    buf: Vec<u8>,
    len: u64,
}

impl Sha256 {
    fn new() -> Self {
        Sha256 {
            state: [
                0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab,
                0x5be0cd19,
            ],
            buf: Vec::new(),
            len: 0,
        }
    }

    fn update(&mut self, data: &[u8]) {
        self.len = self.len.wrapping_add(data.len() as u64);
        self.buf.extend_from_slice(data);
        while self.buf.len() >= 64 {
            let block: [u8; 64] = self.buf[..64].try_into().unwrap();
            self.process(&block);
            self.buf.drain(..64);
        }
    }

    fn finalize(mut self) -> [u8; 32] {
        let bit_len = self.len.wrapping_mul(8);
        self.buf.push(0x80);
        while self.buf.len() % 64 != 56 {
            self.buf.push(0);
        }
        self.buf.extend_from_slice(&bit_len.to_be_bytes());
        let chunks = self.buf.len() / 64;
        for i in 0..chunks {
            let block: [u8; 64] = self.buf[i * 64..i * 64 + 64].try_into().unwrap();
            self.process(&block);
        }
        let mut out = [0u8; 32];
        for (i, word) in self.state.iter().enumerate() {
            out[i * 4..i * 4 + 4].copy_from_slice(&word.to_be_bytes());
        }
        out
    }

    fn process(&mut self, block: &[u8; 64]) {
        const K: [u32; 64] = [
            0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4,
            0xab1c5ed5, 0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe,
            0x9bdc06a7, 0xc19bf174, 0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f,
            0x4a7484aa, 0x5cb0a9dc, 0x76f988da, 0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7,
            0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967, 0x27b70a85, 0x2e1b2138, 0x4d2c6dfc,
            0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85, 0xa2bfe8a1, 0xa81a664b,
            0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070, 0x19a4c116,
            0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
            0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7,
            0xc67178f2,
        ];
        let mut w = [0u32; 64];
        for i in 0..16 {
            w[i] = u32::from_be_bytes(block[i * 4..i * 4 + 4].try_into().unwrap());
        }
        for i in 16..64 {
            let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
            let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
            w[i] = w[i - 16]
                .wrapping_add(s0)
                .wrapping_add(w[i - 7])
                .wrapping_add(s1);
        }
        let mut h = self.state;
        for i in 0..64 {
            let s1 = h[4].rotate_right(6) ^ h[4].rotate_right(11) ^ h[4].rotate_right(25);
            let ch = (h[4] & h[5]) ^ ((!h[4]) & h[6]);
            let t1 = h[7]
                .wrapping_add(s1)
                .wrapping_add(ch)
                .wrapping_add(K[i])
                .wrapping_add(w[i]);
            let s0 = h[0].rotate_right(2) ^ h[0].rotate_right(13) ^ h[0].rotate_right(22);
            let maj = (h[0] & h[1]) ^ (h[0] & h[2]) ^ (h[1] & h[2]);
            let t2 = s0.wrapping_add(maj);
            h[7] = h[6];
            h[6] = h[5];
            h[5] = h[4];
            h[4] = h[3].wrapping_add(t1);
            h[3] = h[2];
            h[2] = h[1];
            h[1] = h[0];
            h[0] = t1.wrapping_add(t2);
        }
        for (slot, hv) in self.state.iter_mut().zip(h.iter()) {
            *slot = slot.wrapping_add(*hv);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::weft_config::WeftCommand;
    use super::*;

    fn cmd(name: &str, confirm: Option<bool>) -> WeftCommand {
        WeftCommand {
            name: name.to_string(),
            description: None,
            keywords: vec![],
            confirm,
            command: Some("rm -rf /tmp/x".to_string()),
            workspace: None,
            restart: None,
        }
    }

    #[test]
    fn sha256_matches_known_vector() {
        // SHA-256("abc")
        let mut h = Sha256::new();
        h.update(b"abc");
        assert_eq!(
            hex(&h.finalize()),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn global_command_runs_unconfirmed() {
        let store = TrustStore::in_memory();
        let c = cmd("Deploy", None);
        assert_eq!(
            store.decide(
                &c,
                CommandOrigin::Global,
                "/home/u/.config/weftgrid/weft.json"
            ),
            TrustDecision::AllowUnconfirmed
        );
    }

    #[test]
    fn global_command_with_confirm_true_still_prompts() {
        // `confirm: true` is an explicit per-command opt-in that must win even for
        // the user's own global config (e.g. a destructive deploy they want to
        // re-approve every run).
        let store = TrustStore::in_memory();
        let c = cmd("Deploy", Some(true));
        assert_eq!(
            store.decide(
                &c,
                CommandOrigin::Global,
                "/home/u/.config/weftgrid/weft.json"
            ),
            TrustDecision::NeedsConfirm
        );
    }

    #[test]
    fn project_local_command_needs_confirm_until_granted() {
        let mut store = TrustStore::in_memory();
        let c = cmd("Deploy", None);
        let src = "/proj/weft.json";
        assert_eq!(
            store.decide(&c, CommandOrigin::ProjectLocal, src),
            TrustDecision::NeedsConfirm
        );
        store.grant(&c, src);
        assert_eq!(
            store.decide(&c, CommandOrigin::ProjectLocal, src),
            TrustDecision::AllowUnconfirmed
        );
    }

    #[test]
    fn confirm_true_forces_prompt_even_when_trusted() {
        let mut store = TrustStore::in_memory();
        let c = cmd("Deploy", Some(true));
        let src = "/proj/weft.json";
        store.grant(&c, src);
        assert_eq!(
            store.decide(&c, CommandOrigin::ProjectLocal, src),
            TrustDecision::NeedsConfirm
        );
    }

    #[test]
    fn editing_command_invalidates_trust() {
        let mut store = TrustStore::in_memory();
        let src = "/proj/weft.json";
        let original = cmd("Deploy", None);
        store.grant(&original, src);
        let mut edited = original.clone();
        edited.command = Some("rm -rf /".to_string());
        assert_eq!(
            store.decide(&edited, CommandOrigin::ProjectLocal, src),
            TrustDecision::NeedsConfirm
        );
    }

    #[test]
    fn trust_persists_across_store_reload() {
        let mut dir = std::env::temp_dir();
        dir.push(format!("weftgrid-trust-test-{}", uuid::Uuid::new_v4()));
        let c = cmd("Deploy", None);
        let src = "/proj/weft.json";
        {
            let mut store = TrustStore::with_dir(&dir);
            store.grant(&c, src);
        }
        let reloaded = TrustStore::with_dir(&dir);
        assert!(reloaded.is_trusted(&c, src));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
