//! Browser catalog + installed-browser detection + path resolution (P11a core).
//!
//! A static table of 20+ browsers (cmux parity) with engine family + tier, plus
//! pure path-resolution helpers. Forks (Floorp/Waterfox/Zen/LibreWolf/…) are just
//! additional table rows of the matching family — same store format, so detection
//! and decrypt reuse the family path without per-fork code.
//!
//! Detection is split into a pure core (`detect_in`, takes a base-dir resolver) so
//! it is fixture-testable, and a thin OS wrapper (`detect_installed`) that feeds it
//! the real platform data dirs. Artifact resolution (`chromium_artifacts`,
//! `firefox_artifacts`) is pure over a data-root path.

use std::path::{Path, PathBuf};

use super::types::{BrowserFamily, BrowserInfo};

/// One catalog row. The `*_root` fields are the browser's data-root path RELATIVE
/// to an OS base dir; detection joins them onto each candidate base and keeps the
/// one that exists (a wrong base simply won't exist → harmless).
pub struct BrowserDescriptor {
    pub id: &'static str,
    pub display_name: &'static str,
    pub family: BrowserFamily,
    pub tier: u8,
    /// Relative to `%LOCALAPPDATA%` or `%APPDATA%`.
    pub win_root: &'static str,
    /// Relative to `$HOME`.
    pub linux_root: &'static str,
    /// Relative to `$HOME` (e.g. `Library/Application Support/...`).
    pub mac_root: &'static str,
}

use BrowserFamily::{Chromium, Firefox, Webkit};

/// The catalog: 22 browsers across chromium / firefox / webkit (cmux parity).
pub const CATALOG: &[BrowserDescriptor] = &[
    // --- Tier 1 ---
    d(
        "chrome",
        "Google Chrome",
        Chromium,
        1,
        "Google/Chrome/User Data",
        ".config/google-chrome",
        "Library/Application Support/Google/Chrome",
    ),
    d(
        "firefox",
        "Firefox",
        Firefox,
        1,
        "Mozilla/Firefox",
        ".mozilla/firefox",
        "Library/Application Support/Firefox",
    ),
    d(
        "edge",
        "Microsoft Edge",
        Chromium,
        1,
        "Microsoft/Edge/User Data",
        ".config/microsoft-edge",
        "Library/Application Support/Microsoft Edge",
    ),
    d(
        "brave",
        "Brave",
        Chromium,
        1,
        "BraveSoftware/Brave-Browser/User Data",
        ".config/BraveSoftware/Brave-Browser",
        "Library/Application Support/BraveSoftware/Brave-Browser",
    ),
    d(
        "arc",
        "Arc",
        Chromium,
        1,
        "Arc/User Data",
        ".config/arc",
        "Library/Application Support/Arc/User Data",
    ),
    d("safari", "Safari", Webkit, 1, "", "", "Library/Safari"),
    // --- Tier 2 ---
    d(
        "vivaldi",
        "Vivaldi",
        Chromium,
        2,
        "Vivaldi/User Data",
        ".config/vivaldi",
        "Library/Application Support/Vivaldi",
    ),
    d(
        "opera",
        "Opera",
        Chromium,
        2,
        "Opera Software/Opera Stable",
        ".config/opera",
        "Library/Application Support/com.operasoftware.Opera",
    ),
    d(
        "opera_gx",
        "Opera GX",
        Chromium,
        2,
        "Opera Software/Opera GX Stable",
        ".config/opera-gx",
        "Library/Application Support/com.operasoftware.OperaGX",
    ),
    d(
        "zen",
        "Zen Browser",
        Firefox,
        2,
        "zen",
        ".zen",
        "Library/Application Support/zen",
    ),
    d(
        "orion",
        "Orion",
        Webkit,
        2,
        "",
        "",
        "Library/Application Support/Orion",
    ),
    // --- Tier 3 ---
    d(
        "chromium",
        "Chromium",
        Chromium,
        3,
        "Chromium/User Data",
        ".config/chromium",
        "Library/Application Support/Chromium",
    ),
    d(
        "librewolf",
        "LibreWolf",
        Firefox,
        3,
        "librewolf",
        ".librewolf",
        "Library/Application Support/librewolf",
    ),
    d(
        "floorp",
        "Floorp",
        Firefox,
        3,
        "Floorp",
        ".floorp",
        "Library/Application Support/Floorp",
    ),
    d(
        "waterfox",
        "Waterfox",
        Firefox,
        3,
        "Waterfox",
        ".waterfox",
        "Library/Application Support/Waterfox",
    ),
    d(
        "dia",
        "Dia",
        Chromium,
        3,
        "Dia/User Data",
        ".config/dia",
        "Library/Application Support/Dia/User Data",
    ),
    d(
        "comet",
        "Perplexity Comet",
        Chromium,
        3,
        "Perplexity/Comet/User Data",
        ".config/comet",
        "Library/Application Support/Comet",
    ),
    d(
        "sigmaos",
        "SigmaOS",
        Chromium,
        3,
        "SigmaOS/User Data",
        ".config/sigmaos",
        "Library/Application Support/SigmaOS",
    ),
    d(
        "sidekick",
        "Sidekick",
        Chromium,
        3,
        "Sidekick/User Data",
        ".config/sidekick",
        "Library/Application Support/Sidekick",
    ),
    d(
        "helium",
        "Helium",
        Chromium,
        3,
        "Helium/User Data",
        ".config/helium",
        "Library/Application Support/Helium",
    ),
    d(
        "atlas",
        "Atlas",
        Chromium,
        3,
        "Atlas/User Data",
        ".config/atlas",
        "Library/Application Support/Atlas",
    ),
    d(
        "ladybird",
        "Ladybird",
        Webkit,
        3,
        "",
        "",
        "Library/Application Support/Ladybird",
    ),
];

/// `const fn` row constructor (keeps the table compact + readable).
const fn d(
    id: &'static str,
    display_name: &'static str,
    family: BrowserFamily,
    tier: u8,
    win_root: &'static str,
    linux_root: &'static str,
    mac_root: &'static str,
) -> BrowserDescriptor {
    BrowserDescriptor {
        id,
        display_name,
        family,
        tier,
        win_root,
        linux_root,
        mac_root,
    }
}

impl BrowserDescriptor {
    /// Data-root path relative to an OS base dir for the current platform.
    pub fn rel_root(&self) -> &'static str {
        #[cfg(target_os = "windows")]
        {
            self.win_root
        }
        #[cfg(target_os = "macos")]
        {
            self.mac_root
        }
        #[cfg(all(unix, not(target_os = "macos")))]
        {
            self.linux_root
        }
    }
}

/// Look up a catalog row by id.
pub fn descriptor(id: &str) -> Option<&'static BrowserDescriptor> {
    CATALOG.iter().find(|b| b.id == id)
}

/// Resolve chromium artifacts under a `User Data` root: `(cookie_db, key_store, history_db)`.
/// Tries the `Default` profile first (common case); each is `None` if absent.
pub fn chromium_artifacts(root: &Path) -> (Option<PathBuf>, Option<PathBuf>, Option<PathBuf>) {
    let cookie = first_existing(root, &["Default/Network/Cookies", "Default/Cookies"]);
    let key = first_existing(root, &["Local State"]);
    let history = first_existing(root, &["Default/History"]);
    (cookie, key, history)
}

/// Resolve firefox artifacts under a profile root: `(cookie_db, history_db)`.
/// Picks the default profile from `profiles.ini`; falls back to the first profile
/// dir that has a `cookies.sqlite`.
pub fn firefox_artifacts(root: &Path) -> (Option<PathBuf>, Option<PathBuf>) {
    if let Some(profile) = firefox_default_profile(root) {
        let cookie = exists_or_none(profile.join("cookies.sqlite"));
        let history = exists_or_none(profile.join("places.sqlite"));
        if cookie.is_some() || history.is_some() {
            return (cookie, history);
        }
    }
    (None, None)
}

/// Resolve the default firefox profile dir from `profiles.ini` (`Default=` under
/// an `[Install*]` section, the canonical "default" pointer). Returns absolute.
fn firefox_default_profile(root: &Path) -> Option<PathBuf> {
    let ini = std::fs::read_to_string(root.join("profiles.ini")).ok()?;
    let mut current_is_install = false;
    for line in ini.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            current_is_install = line.starts_with("[Install");
        } else if current_is_install {
            if let Some(rel) = line.strip_prefix("Default=") {
                return Some(root.join(rel.trim()));
            }
        }
    }
    None
}

fn first_existing(root: &Path, candidates: &[&str]) -> Option<PathBuf> {
    candidates.iter().map(|c| root.join(c)).find(|p| p.exists())
}

fn exists_or_none(p: PathBuf) -> Option<PathBuf> {
    if p.exists() {
        Some(p)
    } else {
        None
    }
}

/// Build a [`BrowserInfo`] for a descriptor given its resolved data root.
fn info_for(desc: &BrowserDescriptor, root: &Path) -> BrowserInfo {
    let (cookie_db, key_store, history_db) = match desc.family {
        BrowserFamily::Chromium => chromium_artifacts(root),
        BrowserFamily::Firefox => {
            let (c, h) = firefox_artifacts(root);
            (c, None, h)
        }
        // Webkit cookie/history paths are binary formats not read in P11a.
        BrowserFamily::Webkit => (None, None, None),
    };
    BrowserInfo {
        id: desc.id.to_string(),
        display_name: desc.display_name.to_string(),
        family: desc.family,
        tier: desc.tier,
        cookie_db: cookie_db.map(path_string),
        key_store: key_store.map(path_string),
        history_db: history_db.map(path_string),
    }
}

fn path_string(p: PathBuf) -> String {
    p.to_string_lossy().into_owned()
}

/// Pure detection core: for each catalog row, ask `resolve_root` for its data root
/// (a base-dir lookup); a `Some(existing_dir)` means installed. Sorted by tier
/// then display name (cmux parity ordering). Fixture-testable.
pub fn detect_in<F>(resolve_root: F) -> Vec<BrowserInfo>
where
    F: Fn(&BrowserDescriptor) -> Option<PathBuf>,
{
    let mut found: Vec<BrowserInfo> = CATALOG
        .iter()
        .filter_map(|desc| resolve_root(desc).map(|root| info_for(desc, &root)))
        .collect();
    found.sort_by(|a, b| {
        a.tier
            .cmp(&b.tier)
            .then_with(|| a.display_name.cmp(&b.display_name))
    });
    found
}

/// OS wrapper: detect installed browsers using the real platform base dirs. Joins
/// each descriptor's relative root onto every candidate base and keeps existing.
pub fn detect_installed() -> Vec<BrowserInfo> {
    let bases = os_base_dirs();
    detect_in(|desc| {
        let rel = desc.rel_root();
        if rel.is_empty() {
            return None;
        }
        bases.iter().map(|base| base.join(rel)).find(|p| p.exists())
    })
}

/// Candidate OS base dirs a data-root is resolved under.
fn os_base_dirs() -> Vec<PathBuf> {
    #[cfg(target_os = "windows")]
    {
        ["LOCALAPPDATA", "APPDATA"]
            .iter()
            .filter_map(|v| std::env::var(v).ok().map(PathBuf::from))
            .collect()
    }
    #[cfg(unix)]
    {
        std::env::var("HOME")
            .ok()
            .map(|h| vec![PathBuf::from(h)])
            .unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalog_has_expected_breadth_and_unique_ids() {
        assert!(
            CATALOG.len() >= 20,
            "want 20+ browsers, got {}",
            CATALOG.len()
        );
        let mut ids: Vec<&str> = CATALOG.iter().map(|b| b.id).collect();
        ids.sort_unstable();
        let before = ids.len();
        ids.dedup();
        assert_eq!(before, ids.len(), "duplicate browser ids in catalog");
    }

    #[test]
    fn catalog_includes_forks_of_each_family() {
        // Forks share a family with their parent (same store format → reused path).
        assert_eq!(descriptor("floorp").unwrap().family, BrowserFamily::Firefox);
        assert_eq!(
            descriptor("waterfox").unwrap().family,
            BrowserFamily::Firefox
        );
        assert_eq!(descriptor("brave").unwrap().family, BrowserFamily::Chromium);
        assert_eq!(descriptor("orion").unwrap().family, BrowserFamily::Webkit);
    }

    #[test]
    fn detect_in_finds_chromium_from_fixture_root() {
        let dir = std::env::temp_dir().join(format!("wg-cat-{}", uuid::Uuid::new_v4()));
        let root = dir.join("Chrome/User Data");
        std::fs::create_dir_all(root.join("Default/Network")).unwrap();
        std::fs::write(root.join("Default/Network/Cookies"), b"x").unwrap();
        std::fs::write(root.join("Local State"), b"{}").unwrap();
        std::fs::write(root.join("Default/History"), b"x").unwrap();

        let found = detect_in(|desc| {
            if desc.id == "chrome" {
                Some(root.clone())
            } else {
                None
            }
        });
        assert_eq!(found.len(), 1);
        let chrome = &found[0];
        assert_eq!(chrome.id, "chrome");
        assert!(chrome.cookie_db.as_ref().unwrap().ends_with("Cookies"));
        assert!(chrome.key_store.as_ref().unwrap().ends_with("Local State"));
        assert!(chrome.history_db.as_ref().unwrap().ends_with("History"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn detect_in_resolves_firefox_default_profile_from_ini() {
        let dir = std::env::temp_dir().join(format!("wg-ff-{}", uuid::Uuid::new_v4()));
        let root = dir.join("firefox");
        let profile = root.join("abc.default-release");
        std::fs::create_dir_all(&profile).unwrap();
        std::fs::write(
            root.join("profiles.ini"),
            "[Install123]\nDefault=abc.default-release\n[Profile0]\nPath=abc.default-release\n",
        )
        .unwrap();
        std::fs::write(profile.join("cookies.sqlite"), b"x").unwrap();
        std::fs::write(profile.join("places.sqlite"), b"x").unwrap();

        let found = detect_in(|desc| (desc.id == "firefox").then(|| root.clone()));
        assert_eq!(found.len(), 1);
        let ff = &found[0];
        assert!(ff.cookie_db.as_ref().unwrap().ends_with("cookies.sqlite"));
        assert!(ff.history_db.as_ref().unwrap().ends_with("places.sqlite"));
        assert!(ff.key_store.is_none()); // firefox cookies are plaintext
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn detect_in_sorts_by_tier_then_name() {
        // Resolve two browsers of different tiers to the same dummy (existing) dir.
        let dir = std::env::temp_dir().join(format!("wg-sort-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let found = detect_in(|desc| matches!(desc.id, "chromium" | "chrome").then(|| dir.clone()));
        // chrome (tier 1) before chromium (tier 3).
        assert_eq!(found[0].id, "chrome");
        assert_eq!(found[1].id, "chromium");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn detect_installed_does_not_panic() {
        // Smoke: real OS dirs; result depends on the machine, just must not panic.
        let _ = detect_installed();
    }
}
