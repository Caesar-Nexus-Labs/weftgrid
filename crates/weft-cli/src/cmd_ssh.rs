//! `weft ssh` subcommand skeleton (P13 owns the binary; P10 contributes the full
//! spec).
//!
//! Sends a `Command::Ssh { destination }` over RPC. The wire shape is reserved here
//! so the single binary speaks one protocol; the server-side handler currently
//! returns `unimplemented` until P10 wires it (initializing an SSH workspace via
//! the `ssh` transport track).

use clap::Args;

use crate::rpc_client;

#[derive(Debug, Args)]
pub struct SshArgs {
    /// Destination in `user@host` form.
    destination: String,
}

pub fn run(args: SshArgs) -> i32 {
    let command = serde_json::json!({
        "domain": "ssh",
        "destination": args.destination,
    });
    rpc_client::run_command(command)
}
