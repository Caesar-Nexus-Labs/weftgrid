//! `weft.json` parse + validation (P12a; cmux `cmux.json` compat — research §4).
//!
//! A project or global `weft.json` contributes custom commands to the palette
//! (P16). Each command is a shell `command` XOR a `workspace` builder (mutually
//! exclusive). Validation mirrors cmux `CmuxCommandDefinition`: name non-blank,
//! not both, not neither, command not blank. Security boundary (running shell
//! from a repo's config) lives in `weft_trust`.

use serde::{Deserialize, Serialize};

/// Workspace-collision behavior when a command builds a same-named workspace.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RestartBehavior {
    New,
    Recreate,
    Ignore,
    Confirm,
}

/// `workspace` builder payload (subset of cmux `CmuxWorkspaceDefinition`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WorkspaceSpec {
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub cwd: Option<String>,
    /// `#RRGGBB` or a named color.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub color: Option<String>,
}

/// A custom command contributed by a `weft.json` (palette entry for P16).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WeftCommand {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub description: Option<String>,
    #[serde(default)]
    pub keywords: Vec<String>,
    /// Force a confirm prompt before running, every time (overrides trust cache).
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub confirm: Option<bool>,
    /// Shell text sent to a terminal (mutually exclusive with `workspace`).
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub command: Option<String>,
    /// Workspace builder (mutually exclusive with `command`).
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub workspace: Option<WorkspaceSpec>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub restart: Option<RestartBehavior>,
}

impl WeftCommand {
    /// Stable id used as the palette command id + trust key seed.
    pub fn id(&self) -> String {
        format!("weft.config.command.{}", percent_alnum(&self.name))
    }

    fn validate(&self) -> Result<(), WeftConfigError> {
        if self.name.trim().is_empty() {
            return Err(WeftConfigError::BlankName);
        }
        match (&self.command, &self.workspace) {
            (Some(_), Some(_)) => Err(WeftConfigError::BothCommandAndWorkspace(self.name.clone())),
            (None, None) => Err(WeftConfigError::NeitherCommandNorWorkspace(
                self.name.clone(),
            )),
            (Some(cmd), None) if cmd.trim().is_empty() => {
                Err(WeftConfigError::BlankCommand(self.name.clone()))
            }
            _ => Ok(()),
        }
    }
}

/// Parsed `weft.json` (subset relevant to the palette/commands surface).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct WeftConfig {
    #[serde(default)]
    pub commands: Vec<WeftCommand>,
    /// Name of a `commands[]` entry that defines a workspace (new-workspace btn).
    #[serde(
        skip_serializing_if = "Option::is_none",
        default,
        rename = "newWorkspaceCommand"
    )]
    pub new_workspace_command: Option<String>,
}

/// Validation/parse errors (each names the offending command).
#[derive(Debug, PartialEq, Eq)]
pub enum WeftConfigError {
    Parse(String),
    BlankName,
    BlankCommand(String),
    BothCommandAndWorkspace(String),
    NeitherCommandNorWorkspace(String),
    /// A trust/lookup referenced a command name not present in the config.
    CommandNotFound(String),
}

impl std::fmt::Display for WeftConfigError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            WeftConfigError::Parse(m) => write!(f, "weft.json parse error: {m}"),
            WeftConfigError::BlankName => write!(f, "command name must not be blank"),
            WeftConfigError::BlankCommand(n) => {
                write!(f, "command '{n}' must not define a blank 'command'")
            }
            WeftConfigError::BothCommandAndWorkspace(n) => {
                write!(
                    f,
                    "command '{n}' must not define both 'command' and 'workspace'"
                )
            }
            WeftConfigError::NeitherCommandNorWorkspace(n) => {
                write!(
                    f,
                    "command '{n}' must define either 'command' or 'workspace'"
                )
            }
            WeftConfigError::CommandNotFound(n) => write!(f, "command '{n}' not found in config"),
        }
    }
}

impl std::error::Error for WeftConfigError {}

/// Parse + validate a `weft.json` string. Returns the typed config or the first
/// validation failure.
pub fn parse_weft_config(json: &str) -> Result<WeftConfig, WeftConfigError> {
    let config: WeftConfig =
        serde_json::from_str(json).map_err(|e| WeftConfigError::Parse(e.to_string()))?;
    for cmd in &config.commands {
        cmd.validate()?;
    }
    Ok(config)
}

/// Percent-encode non-alphanumerics so a command name becomes a stable id slug.
fn percent_alnum(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_command_and_workspace_fixtures() {
        let json = r#"{
            "commands": [
                { "name": "Build", "command": "cargo build", "keywords": ["b"] },
                { "name": "New WS", "workspace": { "name": "scratch", "cwd": "/tmp" } }
            ],
            "newWorkspaceCommand": "New WS"
        }"#;
        let cfg = parse_weft_config(json).unwrap();
        assert_eq!(cfg.commands.len(), 2);
        assert_eq!(cfg.commands[0].command.as_deref(), Some("cargo build"));
        assert!(cfg.commands[1].workspace.is_some());
        assert_eq!(cfg.new_workspace_command.as_deref(), Some("New WS"));
    }

    #[test]
    fn rejects_command_with_both_command_and_workspace() {
        let json = r#"{ "commands": [
            { "name": "X", "command": "ls", "workspace": { "name": "w" } }
        ] }"#;
        assert_eq!(
            parse_weft_config(json),
            Err(WeftConfigError::BothCommandAndWorkspace("X".to_string()))
        );
    }

    #[test]
    fn rejects_command_with_neither() {
        let json = r#"{ "commands": [ { "name": "X" } ] }"#;
        assert_eq!(
            parse_weft_config(json),
            Err(WeftConfigError::NeitherCommandNorWorkspace("X".to_string()))
        );
    }

    #[test]
    fn rejects_blank_name() {
        let json = r#"{ "commands": [ { "name": "   ", "command": "ls" } ] }"#;
        assert_eq!(parse_weft_config(json), Err(WeftConfigError::BlankName));
    }

    #[test]
    fn rejects_blank_command() {
        let json = r#"{ "commands": [ { "name": "X", "command": "  " } ] }"#;
        assert_eq!(
            parse_weft_config(json),
            Err(WeftConfigError::BlankCommand("X".to_string()))
        );
    }

    #[test]
    fn command_id_is_stable_slug() {
        let cmd = WeftCommand {
            name: "New WS".to_string(),
            description: None,
            keywords: vec![],
            confirm: None,
            command: None,
            workspace: Some(WorkspaceSpec {
                name: None,
                cwd: None,
                color: None,
            }),
            restart: None,
        };
        assert_eq!(cmd.id(), "weft.config.command.New%20WS");
    }

    #[test]
    fn empty_config_is_valid() {
        assert_eq!(parse_weft_config("{}").unwrap(), WeftConfig::default());
    }
}
