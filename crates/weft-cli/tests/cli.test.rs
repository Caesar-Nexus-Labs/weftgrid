//! CLI parsing/exit-code tests (P13). Drives the built `weft` binary as a
//! black box: good subcommands parse, bad args exit non-zero, and `--help` /
//! `--version` short-circuit. These do NOT need a running app — they exercise the
//! clap layer and (for bad args) never reach the RPC transport.
//!
//! Commands that DO reach the transport (a valid `browser snapshot` with no app
//! running) exit `2` (connection failure), which we also assert: parse succeeds,
//! transport fails cleanly.

use std::process::Command;

/// Path to the compiled `weft` binary for this test profile (cargo sets
/// `CARGO_BIN_EXE_<name>`).
fn weft() -> Command {
    Command::new(env!("CARGO_BIN_EXE_weft"))
}

fn code(cmd: &mut Command) -> i32 {
    let out = cmd.output().expect("spawn weft");
    out.status.code().unwrap_or(-1)
}

#[test]
fn no_subcommand_is_usage_error() {
    // clap exits non-zero on a missing required subcommand.
    let bare = weft().output().expect("spawn");
    assert_ne!(bare.status.code(), Some(0));
}

#[test]
fn help_exits_zero() {
    assert_eq!(code(weft().arg("--help")), 0);
}

#[test]
fn version_exits_zero() {
    assert_eq!(code(weft().arg("--version")), 0);
}

#[test]
fn unknown_subcommand_is_error() {
    assert_eq!(code(weft().arg("teleport")), 2);
}

#[test]
fn browser_requires_action() {
    // `weft browser` with no action is a parse error (2), not a transport attempt.
    assert_eq!(code(weft().arg("browser")), 2);
}

#[test]
fn browser_click_requires_ref() {
    assert_eq!(code(weft().args(["browser", "click"])), 2);
}

#[test]
fn browser_fill_requires_ref_and_text() {
    // missing text
    assert_eq!(code(weft().args(["browser", "fill", "e1"])), 2);
}

#[test]
fn notify_requires_message() {
    assert_eq!(code(weft().arg("notify")), 2);
}

#[test]
fn ssh_requires_destination() {
    assert_eq!(code(weft().arg("ssh")), 2);
}

#[test]
fn valid_browser_snapshot_parses_then_fails_transport() {
    // Well-formed command, but no app is running in the test env, so the RPC
    // transport fails → exit 2 (connection failure), proving parse succeeded and
    // the error is a clean transport error, not a panic.
    let out = weft()
        .args(["browser", "snapshot", "--pane", "p1"])
        .output()
        .expect("spawn");
    // 2 = transport/connection failure (token/endpoint file absent or no server).
    assert_eq!(out.status.code(), Some(2));
}
