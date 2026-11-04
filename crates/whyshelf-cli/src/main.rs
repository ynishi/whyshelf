//! The `whyshelf` command (CLI, and the MCP server as `whyshelf mcp`): parses
//! arguments, wires the `whyshelf-infra` adapters into the `whyshelf-core`
//! use cases, prints the result.
//!
//! The argument parser's definition ([`args`], clap's derive types) is the
//! only description of the commands: help, the command list and validation
//! come from it, not from a second hand-kept table. Structured input is read
//! from a file argument, not pasted into the command line.
//!
//! The crate is an adapter and nothing more; what a command means is decided by
//! the use cases of `whyshelf-core`.
//!
//! | module | job |
//! |---|---|
//! | [`args`] | the commands as clap types, parsed into core types |
//! | [`command`] | a command as a value, and the one function that runs it |
//! | [`actor`] | the task that owns the workspace for the tool server |
//! | [`mcp`] | the tools, one per command |
//! | [`render`] | an outcome as a table, JSON or Markdown |
//! | [`init`] | `whyshelf init` (the only command that writes files itself) |
//! | [`run`] | opening and closing the workspace, printing, the exit status |
//! | [`error`] | the errors and how they are shown |

mod actor;
mod args;
mod command;
mod error;
mod init;
mod mcp;
mod render;
mod run;

#[tokio::main]
async fn main() -> std::process::ExitCode {
    run::main().await
}
