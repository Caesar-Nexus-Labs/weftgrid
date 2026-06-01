//! Log rotation + retention policy (P14).
//!
//! `tracing-appender`'s `RollingFileAppender` already rolls files by time, but it
//! never DELETES old files and has no size trigger — so an unattended terminal
//! grows logs forever. This module owns the policy: decide when to roll (size or
//! age) and which historical files to delete (keep at most N files / N days).
//!
//! The decision functions are PURE — they take a clock value / file metadata and
//! return what to do — so they unit-test against a temp dir of fake files without
//! waiting real time. The one filesystem-touching helper, [`apply_retention`],
//! deletes the files the pure planner selected.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

/// Rotation + retention policy. `max_bytes`/`max_age` decide a roll; `keep_files`/
/// `max_age` (reused) bound retention. Conservative defaults: 10 MiB, keep 7 files,
/// 14 days — enough to debug a field issue without unbounded growth.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RotationPolicy {
    pub max_bytes: u64,
    pub max_age: Duration,
    pub keep_files: usize,
}

impl Default for RotationPolicy {
    fn default() -> Self {
        RotationPolicy {
            max_bytes: 10 * 1024 * 1024,
            max_age: Duration::from_secs(14 * 24 * 60 * 60),
            keep_files: 7,
        }
    }
}

impl RotationPolicy {
    /// Whether the active log should roll now: it has hit the size cap OR is older
    /// than the age cap. Pure over the two inputs.
    pub fn should_rotate(&self, current_size: u64, age: Duration) -> bool {
        current_size >= self.max_bytes || age >= self.max_age
    }
}

/// A rotated log file: its path and how old it is relative to `now` (filled by
/// [`scan_rotated`]). Sorted newest-first by the retention planner.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RotatedFile {
    pub path: PathBuf,
    pub age: Duration,
}

/// Decide which rotated files to delete: anything beyond `keep_files` (oldest go
/// first) OR older than `max_age`. Pure — `files` need not be pre-sorted. Returns
/// the paths to delete so a caller can dry-run / test without touching disk.
pub fn plan_retention(policy: &RotationPolicy, mut files: Vec<RotatedFile>) -> Vec<PathBuf> {
    // Newest first so index >= keep_files is the tail we drop.
    files.sort_by_key(|f| f.age);
    let mut to_delete = Vec::new();
    for (idx, f) in files.into_iter().enumerate() {
        let over_count = idx >= policy.keep_files;
        let too_old = f.age >= policy.max_age;
        if over_count || too_old {
            to_delete.push(f.path);
        }
    }
    to_delete
}

/// List rotated log files in `dir` whose name starts with `prefix`, pairing each
/// with its age relative to `now` (from mtime). Skips the directory itself and any
/// unreadable entry. Used to feed [`plan_retention`].
pub fn scan_rotated(dir: &Path, prefix: &str, now: SystemTime) -> io::Result<Vec<RotatedFile>> {
    let mut out = Vec::new();
    if !dir.exists() {
        return Ok(out);
    }
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let name = match path.file_name().and_then(|n| n.to_str()) {
            Some(n) if n.starts_with(prefix) => n.to_string(),
            _ => continue,
        };
        let _ = name;
        let modified = entry.metadata().and_then(|m| m.modified())?;
        let age = now.duration_since(modified).unwrap_or(Duration::ZERO);
        out.push(RotatedFile { path, age });
    }
    Ok(out)
}

/// Scan `dir`, plan retention, and delete the selected files. Returns how many were
/// removed. Best-effort per file: a failed delete is skipped (logged by the caller)
/// rather than aborting the sweep.
pub fn apply_retention(
    dir: &Path,
    prefix: &str,
    policy: &RotationPolicy,
    now: SystemTime,
) -> io::Result<usize> {
    let files = scan_rotated(dir, prefix, now)?;
    let doomed = plan_retention(policy, files);
    let mut removed = 0;
    for path in doomed {
        if fs::remove_file(&path).is_ok() {
            removed += 1;
        }
    }
    Ok(removed)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn secs(n: u64) -> Duration {
        Duration::from_secs(n)
    }

    #[test]
    fn rotates_on_size_cap() {
        let p = RotationPolicy {
            max_bytes: 1000,
            ..RotationPolicy::default()
        };
        assert!(!p.should_rotate(999, secs(0)));
        assert!(p.should_rotate(1000, secs(0)));
        assert!(p.should_rotate(5000, secs(0)));
    }

    #[test]
    fn rotates_on_age_cap() {
        let p = RotationPolicy {
            max_age: secs(60),
            ..RotationPolicy::default()
        };
        assert!(!p.should_rotate(0, secs(59)));
        assert!(p.should_rotate(0, secs(60)));
    }

    #[test]
    fn retention_keeps_newest_n() {
        let p = RotationPolicy {
            keep_files: 2,
            max_age: secs(10_000),
            ..RotationPolicy::default()
        };
        let files = vec![
            RotatedFile {
                path: "a".into(),
                age: secs(30),
            },
            RotatedFile {
                path: "b".into(),
                age: secs(10),
            },
            RotatedFile {
                path: "c".into(),
                age: secs(20),
            },
            RotatedFile {
                path: "d".into(),
                age: secs(40),
            },
        ];
        let del = plan_retention(&p, files);
        // Keep the 2 newest (b@10, c@20); delete the 2 oldest (a@30, d@40).
        assert_eq!(del.len(), 2);
        assert!(del.contains(&PathBuf::from("a")));
        assert!(del.contains(&PathBuf::from("d")));
    }

    #[test]
    fn retention_deletes_beyond_max_age() {
        let p = RotationPolicy {
            keep_files: 100,
            max_age: secs(100),
            ..RotationPolicy::default()
        };
        let files = vec![
            RotatedFile {
                path: "fresh".into(),
                age: secs(50),
            },
            RotatedFile {
                path: "stale".into(),
                age: secs(200),
            },
        ];
        let del = plan_retention(&p, files);
        assert_eq!(del, vec![PathBuf::from("stale")]);
    }

    #[test]
    fn retention_age_and_count_combine() {
        let p = RotationPolicy {
            keep_files: 1,
            max_age: secs(100),
            ..RotationPolicy::default()
        };
        let files = vec![
            RotatedFile {
                path: "newest".into(),
                age: secs(10),
            },
            RotatedFile {
                path: "mid".into(),
                age: secs(50),
            },
            RotatedFile {
                path: "old".into(),
                age: secs(500),
            },
        ];
        let del = plan_retention(&p, files);
        // keep only newest; mid dropped by count, old dropped by count+age.
        assert_eq!(del.len(), 2);
        assert!(del.contains(&PathBuf::from("mid")));
        assert!(del.contains(&PathBuf::from("old")));
        assert!(!del.contains(&PathBuf::from("newest")));
    }

    #[test]
    fn scan_and_apply_on_temp_dir() {
        let dir = std::env::temp_dir().join(format!("weft-rot-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&dir).unwrap();
        // Three matching log files + one unrelated file.
        for name in ["weftgrid.log.1", "weftgrid.log.2", "weftgrid.log.3"] {
            fs::write(dir.join(name), b"x").unwrap();
        }
        fs::write(dir.join("unrelated.txt"), b"x").unwrap();

        let now = SystemTime::now();
        let scanned = scan_rotated(&dir, "weftgrid.log", now).unwrap();
        assert_eq!(scanned.len(), 3, "only prefix-matching files counted");

        let policy = RotationPolicy {
            keep_files: 1,
            max_age: secs(10_000_000),
            ..RotationPolicy::default()
        };
        let removed = apply_retention(&dir, "weftgrid.log", &policy, now).unwrap();
        assert_eq!(removed, 2, "kept 1 newest, deleted 2");
        let left = scan_rotated(&dir, "weftgrid.log", now).unwrap();
        assert_eq!(left.len(), 1);
        // Unrelated file untouched.
        assert!(dir.join("unrelated.txt").exists());

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn scan_missing_dir_is_empty_not_error() {
        let dir = std::env::temp_dir().join(format!("weft-rot-none-{}", uuid::Uuid::new_v4()));
        let got = scan_rotated(&dir, "weftgrid.log", SystemTime::now()).unwrap();
        assert!(got.is_empty());
    }
}
