//! `weft notify` subcommand skeleton (P13 owns the binary; P5 contributes the full
//! spec).
//!
//! Sends a `Command::Notify { message }` over RPC. The wire shape is reserved here
//! so the single binary speaks one protocol; the server-side handler currently
//! returns `unimplemented` until P5 wires it (emitting the OSC sequence built by
//! `notify::commands::notify_build_osc`).

use clap::Args;

use crate::rpc_client;

#[derive(Debug, Args)]
pub struct NotifyArgs {
    /// Notification message body.
    message: String,
}

pub fn run(args: NotifyArgs) -> i32 {
    let command = serde_json::json!({
        "domain": "notify",
        "message": args.message,
    });
    rpc_client::run_command(command)
}
