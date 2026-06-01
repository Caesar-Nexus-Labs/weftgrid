//! Unified `weft` CLI entry (P13).
//!
//! `weft` is the single agent-in-shell surface: an agent running inside a shell
//! pane calls `weft browser snapshot|click|fill|...` (or `weft notify`/`weft ssh`),
//! which connects to the running weftgrid app over a LOCAL authenticated
//! socket/pipe and prints the result to stdout. Exit code reflects the outcome
//! (`0` success, non-zero error) so agent scripts can branch.
//!
//! Subcommand modules: P13 owns the binary + `browser`; P5 contributes `notify`,
//! P10 contributes `ssh` (skeletons here until their specs land). Each lives in its
//! own `cmd_*.rs` so the three tracks don't collide on this file.

mod cmd_browser;
mod cmd_notify;
mod cmd_ssh;
mod rpc_client;

use clap::{Parser, Subcommand};

/// Top-level `weft` command.
#[derive(Debug, Parser)]
#[command(
    name = "weft",
    about = "Drive weftgrid from an agent shell pane over a local authenticated RPC",
    version
)]
struct Cli {
    #[command(subcommand)]
    command: TopCommand,
}

#[derive(Debug, Subcommand)]
enum TopCommand {
    /// Browser automation: snapshot/click/fill/eval/wait/get/find on a pane.
    Browser(cmd_browser::BrowserArgs),
    /// Emit a notification (P5 contributes the full spec).
    Notify(cmd_notify::NotifyArgs),
    /// Open an SSH workspace (P10 contributes the full spec).
    Ssh(cmd_ssh::SshArgs),
}

fn main() {
    let cli = Cli::parse();
    // Each subcommand returns an exit code; clap already exits non-zero on a parse
    // error before we get here.
    let code = match cli.command {
        TopCommand::Browser(args) => cmd_browser::run(args),
        TopCommand::Notify(args) => cmd_notify::run(args),
        TopCommand::Ssh(args) => cmd_ssh::run(args),
    };
    std::process::exit(code);
}
