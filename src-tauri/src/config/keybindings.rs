//! Keybinding registry: defaults + overrides + conflict detection (P12a).
//!
//! A single registry is the source of truth for action→chord bindings. Phases
//! that previously hardcoded chords (P4 focus-nav `Ctrl+Alt+Arrow`) resolve from
//! here instead, so users can customise and the app can detect conflicts (two
//! actions bound to the same chord). Chords are stored as normalised lowercase
//! strings (`"ctrl+shift+p"`); the UI/keyboard layer parses key events to the
//! same normal form before calling `action_for_chord`.

use std::collections::BTreeMap;

/// A pair of actions that resolved to the same chord (a conflict to surface).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Conflict {
    pub chord: String,
    pub action_a: String,
    pub action_b: String,
}

/// Built-in action ids + their default chords. Kept as a function (not a static)
/// so callers always get a fresh owned map. `cmd` is mapped to `ctrl` on the
/// Win/Linux targets weftgrid ships (cmux's macOS `Cmd` → `Ctrl`).
pub fn default_bindings() -> BTreeMap<String, String> {
    let pairs = [
        // Command palette + switcher (research §1.1).
        ("palette.commands", "ctrl+shift+p"),
        ("palette.switcher", "ctrl+p"),
        // Surfaces / tabs.
        ("surface.newTerminal", "ctrl+t"),
        ("surface.close", "ctrl+w"),
        // Splits.
        ("split.right", "ctrl+shift+e"),
        ("split.down", "ctrl+shift+o"),
        // Focus navigation (P4 previously hardcoded these).
        ("focus.left", "ctrl+alt+left"),
        ("focus.right", "ctrl+alt+right"),
        ("focus.up", "ctrl+alt+up"),
        ("focus.down", "ctrl+alt+down"),
        // Terminal find.
        ("find.open", "ctrl+f"),
        ("find.next", "ctrl+g"),
        ("find.previous", "ctrl+alt+g"),
        // App.
        ("app.openSettings", "ctrl+comma"),
    ];
    pairs
        .iter()
        .map(|(a, c)| (a.to_string(), normalize_chord(c)))
        .collect()
}

/// Normalise a chord string: lowercase, trim, sort modifiers so `shift+ctrl+p`
/// and `ctrl+shift+p` compare equal. The final (non-modifier) token is the key.
pub fn normalize_chord(chord: &str) -> String {
    const MODS: [&str; 4] = ["ctrl", "alt", "shift", "meta"];
    let mut mods: Vec<&'static str> = Vec::new();
    let mut keys: Vec<String> = Vec::new();
    for raw in chord.split('+') {
        let tok = raw.trim().to_lowercase();
        if tok.is_empty() {
            continue;
        }
        // `cmd`/`super`/`win` all fold to `meta`; weftgrid maps meta→ctrl in the
        // default map, but a user override may still type `meta`.
        let tok = match tok.as_str() {
            "cmd" | "command" | "super" | "win" => "meta".to_string(),
            "control" => "ctrl".to_string(),
            "option" => "alt".to_string(),
            other => other.to_string(),
        };
        match MODS.iter().find(|m| **m == tok) {
            Some(canonical) if !mods.contains(canonical) => mods.push(canonical),
            Some(_) => {} // duplicate modifier, ignore
            None => keys.push(tok),
        }
    }
    // Canonical modifier order matches the MODS declaration order.
    mods.sort_by_key(|m| MODS.iter().position(|x| x == m).unwrap());
    let mut parts: Vec<String> = mods.iter().map(|m| m.to_string()).collect();
    parts.extend(keys);
    parts.join("+")
}

/// Resolved action→chord registry (defaults overlaid with user overrides).
#[derive(Debug, Clone)]
pub struct KeybindingRegistry {
    bindings: BTreeMap<String, String>,
}

impl Default for KeybindingRegistry {
    fn default() -> Self {
        KeybindingRegistry {
            bindings: default_bindings(),
        }
    }
}

impl KeybindingRegistry {
    /// Build defaults then apply user overrides (each value re-normalised).
    /// An override with an empty chord unbinds the action.
    pub fn with_overrides(overrides: &BTreeMap<String, String>) -> Self {
        let mut bindings = default_bindings();
        for (action, chord) in overrides {
            let norm = normalize_chord(chord);
            if norm.is_empty() {
                bindings.remove(action);
            } else {
                bindings.insert(action.clone(), norm);
            }
        }
        KeybindingRegistry { bindings }
    }

    /// The chord bound to an action, if any.
    pub fn resolve(&self, action: &str) -> Option<&str> {
        self.bindings.get(action).map(String::as_str)
    }

    /// The action a (already-normalised) chord triggers. When multiple actions
    /// share the chord (a conflict) the first by action-id order wins.
    pub fn action_for_chord(&self, chord: &str) -> Option<&str> {
        let norm = normalize_chord(chord);
        self.bindings
            .iter()
            .find(|(_, c)| **c == norm)
            .map(|(a, _)| a.as_str())
    }

    /// All (action, chord) pairs in stable id order (for the editor UI).
    pub fn list(&self) -> Vec<(String, String)> {
        self.bindings
            .iter()
            .map(|(a, c)| (a.clone(), c.clone()))
            .collect()
    }

    /// Set or change a single binding (chord re-normalised; empty unbinds).
    pub fn set(&mut self, action: &str, chord: &str) {
        let norm = normalize_chord(chord);
        if norm.is_empty() {
            self.bindings.remove(action);
        } else {
            self.bindings.insert(action.to_string(), norm);
        }
    }

    /// Every pair of actions sharing a chord. Empty when conflict-free.
    pub fn detect_conflicts(&self) -> Vec<Conflict> {
        let mut by_chord: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
        for (action, chord) in &self.bindings {
            by_chord.entry(chord).or_default().push(action);
        }
        let mut conflicts = Vec::new();
        for (chord, actions) in by_chord {
            if actions.len() < 2 {
                continue;
            }
            for i in 0..actions.len() {
                for j in (i + 1)..actions.len() {
                    conflicts.push(Conflict {
                        chord: chord.to_string(),
                        action_a: actions[i].to_string(),
                        action_b: actions[j].to_string(),
                    });
                }
            }
        }
        conflicts
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_is_modifier_order_insensitive() {
        assert_eq!(normalize_chord("Shift+Ctrl+P"), "ctrl+shift+p");
        assert_eq!(normalize_chord("ctrl+shift+p"), "ctrl+shift+p");
        assert_eq!(normalize_chord("Cmd+P"), "meta+p");
    }

    #[test]
    fn defaults_resolve_focus_nav_chords() {
        let reg = KeybindingRegistry::default();
        assert_eq!(reg.resolve("focus.left"), Some("ctrl+alt+left"));
        assert_eq!(reg.resolve("palette.commands"), Some("ctrl+shift+p"));
    }

    #[test]
    fn defaults_have_no_conflicts() {
        let reg = KeybindingRegistry::default();
        assert!(reg.detect_conflicts().is_empty());
    }

    #[test]
    fn override_creating_duplicate_chord_is_detected() {
        let mut overrides = BTreeMap::new();
        // Bind palette.switcher onto the palette.commands chord → conflict.
        overrides.insert("palette.switcher".to_string(), "ctrl+shift+p".to_string());
        let reg = KeybindingRegistry::with_overrides(&overrides);
        let conflicts = reg.detect_conflicts();
        assert_eq!(conflicts.len(), 1);
        let c = &conflicts[0];
        assert_eq!(c.chord, "ctrl+shift+p");
        assert!(
            (c.action_a == "palette.commands" && c.action_b == "palette.switcher")
                || (c.action_a == "palette.switcher" && c.action_b == "palette.commands")
        );
    }

    #[test]
    fn set_override_updates_registry_and_resolves_back() {
        let mut reg = KeybindingRegistry::default();
        reg.set("palette.commands", "ctrl+shift+k"); // arbitrary new chord
        let resolved = reg.resolve("palette.commands").unwrap();
        assert_eq!(resolved, "ctrl+shift+k");
        assert_eq!(reg.action_for_chord(resolved), Some("palette.commands"));
    }

    #[test]
    fn empty_chord_unbinds() {
        let mut reg = KeybindingRegistry::default();
        reg.set("find.open", "");
        assert_eq!(reg.resolve("find.open"), None);
    }

    #[test]
    fn action_for_chord_is_modifier_order_insensitive() {
        let reg = KeybindingRegistry::default();
        assert_eq!(
            reg.action_for_chord("shift+ctrl+p"),
            Some("palette.commands")
        );
    }
}
