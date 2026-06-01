//! Git probe (P15b, app-driven exception — behind the `sidebar.watchGitStatus`
//! toggle, default-off).
//!
//! Detects a workspace's git branch by reading `.git/HEAD` directly (no spawned
//! process for the branch — a plain file read is cheap and deterministic). The
//! "dirty" flag DOES need `git status`, which is expensive, so this module keeps
//! the SPAWN out: the caller runs `git status --porcelain` (only when the toggle
//! is on) and hands the captured output to [`parse_dirty`]. That split lets the
//! whole module unit-test against a temp `.git` dir + sample status strings with
//! no subprocess.
//!
//! Std-only.

use std::path::Path;

/// A workspace's git summary: branch name (or short detached SHA) + dirty flag.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitSummary {
    /// Branch name, or a short commit hash when HEAD is detached.
    pub branch: String,
    /// True when the working tree has uncommitted changes (from `git status`).
    pub dirty: bool,
}

impl GitSummary {
    /// Sidebar one-line form: `main` clean, `main*` dirty (cmux convention).
    pub fn summary(&self) -> String {
        if self.dirty {
            format!("{}*", self.branch)
        } else {
            self.branch.clone()
        }
    }
}

/// Read the current branch (or short detached SHA) for `cwd`, or `None` when the
/// directory is not a git repo. Resolves the `.git` dir whether it is a real
/// directory or a `gitdir:` pointer file (worktrees/submodules).
pub fn detect_branch(cwd: &Path) -> Option<String> {
    let git_dir = resolve_git_dir(cwd)?;
    let head = std::fs::read_to_string(git_dir.join("HEAD")).ok()?;
    parse_head(&head)
}

/// Full probe: branch + dirty. `status_porcelain` is the captured output of
/// `git status --porcelain` (the caller spawns it only behind the toggle).
/// Returns `None` when `cwd` is not a git repo.
pub fn probe(cwd: &Path, status_porcelain: &str) -> Option<GitSummary> {
    let branch = detect_branch(cwd)?;
    Some(GitSummary {
        branch,
        dirty: parse_dirty(status_porcelain),
    })
}

/// Gate the full probe behind the default-off `sidebar.watchGitStatus` toggle.
/// Returns `None` when the toggle is off — even the cheap branch read is skipped
/// so the basic sidebar shows no git enrichment until the user opts in. `enabled`
/// is read from [`super::state::SidebarState::git_watch_enabled`]; the caller must
/// also skip spawning `git status` when disabled (this is the structural reminder
/// at the probe boundary).
pub fn probe_gated(enabled: bool, cwd: &Path, status_porcelain: &str) -> Option<GitSummary> {
    if !enabled {
        return None;
    }
    probe(cwd, status_porcelain)
}

/// Parse `.git/HEAD` contents into a branch name or short detached SHA.
fn parse_head(head: &str) -> Option<String> {
    let head = head.trim();
    if let Some(reference) = head.strip_prefix("ref:") {
        // `ref: refs/heads/feature/x` → keep everything after `refs/heads/`.
        let name = reference.trim();
        return Some(
            name.strip_prefix("refs/heads/")
                .unwrap_or(name)
                .to_string(),
        );
    }
    if head.is_empty() {
        return None;
    }
    // Detached HEAD: HEAD holds a raw 40-char SHA → show a short form.
    Some(head.chars().take(7).collect())
}

/// True when `git status --porcelain` output shows any change (any non-blank
/// line means a modified/untracked/staged entry; empty output = clean tree).
pub fn parse_dirty(status_porcelain: &str) -> bool {
    status_porcelain.lines().any(|l| !l.trim().is_empty())
}

/// Resolve the real `.git` directory: a `.git` subdir, or follow a `.git` FILE
/// that contains a `gitdir: <path>` pointer (linked worktrees / submodules).
fn resolve_git_dir(cwd: &Path) -> Option<std::path::PathBuf> {
    let dot_git = cwd.join(".git");
    let meta = std::fs::metadata(&dot_git).ok()?;
    if meta.is_dir() {
        return Some(dot_git);
    }
    // `.git` is a file → `gitdir: /abs/or/rel/path`.
    let content = std::fs::read_to_string(&dot_git).ok()?;
    let target = content.trim().strip_prefix("gitdir:")?.trim();
    let path = Path::new(target);
    if path.is_absolute() {
        Some(path.to_path_buf())
    } else {
        Some(cwd.join(path))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU32, Ordering};

    /// Create a unique temp dir (filename-safe across platforms); caller removes
    /// it. A process-local atomic counter keeps parallel test threads disjoint.
    fn temp_dir(tag: &str) -> PathBuf {
        static COUNTER: AtomicU32 = AtomicU32::new(0);
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let mut dir = std::env::temp_dir();
        dir.push(format!("weftgrid-gitprobe-{tag}-{}-{n}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn write_head(repo: &Path, head: &str) {
        let git = repo.join(".git");
        fs::create_dir_all(&git).unwrap();
        fs::write(git.join("HEAD"), head).unwrap();
    }

    #[test]
    fn detects_branch_from_git_head_ref() {
        let repo = temp_dir("branch");
        write_head(&repo, "ref: refs/heads/main\n");
        assert_eq!(detect_branch(&repo).as_deref(), Some("main"));
        let _ = fs::remove_dir_all(&repo);
    }

    #[test]
    fn keeps_slashes_in_feature_branch_names() {
        let repo = temp_dir("feature");
        write_head(&repo, "ref: refs/heads/feature/login\n");
        assert_eq!(detect_branch(&repo).as_deref(), Some("feature/login"));
        let _ = fs::remove_dir_all(&repo);
    }

    #[test]
    fn detached_head_yields_short_sha() {
        let repo = temp_dir("detached");
        write_head(&repo, "1a2b3c4d5e6f7a8b9c0d1e2f3a4b5c6d7e8f9a0b\n");
        assert_eq!(detect_branch(&repo).as_deref(), Some("1a2b3c4"));
        let _ = fs::remove_dir_all(&repo);
    }

    #[test]
    fn no_git_dir_returns_none() {
        let plain = temp_dir("nogit");
        assert_eq!(detect_branch(&plain), None);
        assert!(probe(&plain, "").is_none());
        let _ = fs::remove_dir_all(&plain);
    }

    #[test]
    fn follows_gitdir_pointer_file_for_worktrees() {
        let repo = temp_dir("worktree-main");
        let real_git = repo.join("real-git");
        fs::create_dir_all(&real_git).unwrap();
        fs::write(real_git.join("HEAD"), "ref: refs/heads/wt\n").unwrap();
        // A linked worktree has `.git` as a FILE pointing at the real gitdir.
        let wt = temp_dir("worktree-linked");
        fs::write(wt.join(".git"), format!("gitdir: {}\n", real_git.display())).unwrap();
        assert_eq!(detect_branch(&wt).as_deref(), Some("wt"));
        let _ = fs::remove_dir_all(&repo);
        let _ = fs::remove_dir_all(&wt);
    }

    #[test]
    fn parse_dirty_true_only_when_status_has_entries() {
        assert!(!parse_dirty(""));
        assert!(!parse_dirty("\n  \n"));
        assert!(parse_dirty(" M src/main.rs\n?? new.txt"));
    }

    #[test]
    fn probe_combines_branch_and_dirty_into_summary() {
        let repo = temp_dir("summary");
        write_head(&repo, "ref: refs/heads/main\n");
        let clean = probe(&repo, "").unwrap();
        assert_eq!(clean.summary(), "main");
        let dirty = probe(&repo, " M a.txt\n").unwrap();
        assert_eq!(dirty.summary(), "main*");
        let _ = fs::remove_dir_all(&repo);
    }

    #[test]
    fn gated_probe_returns_none_when_toggle_off() {
        // Default-off invariant: a real git repo yields no summary while the toggle
        // is off (not even the cheap branch read surfaces), so the basic sidebar
        // shows no git enrichment until the user opts in.
        let repo = temp_dir("gated");
        write_head(&repo, "ref: refs/heads/main\n");
        assert_eq!(probe_gated(false, &repo, ""), None);
        assert_eq!(probe_gated(true, &repo, "").unwrap().summary(), "main");
        let _ = fs::remove_dir_all(&repo);
    }
}
