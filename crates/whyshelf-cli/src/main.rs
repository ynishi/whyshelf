//! The `whyshelf` command: parses the arguments, wires the adapters of
//! `whyshelf-infra` into the use cases of `whyshelf-core`, prints the answer.
//!
//! The crate is an adapter and nothing more; what a command means is decided by the
//! use cases ([`whyshelf_core::app::shelf`]). The commands, their output and the exit
//! status are documented in the "Commands" part of the `whyshelf` crate's page.
//!
//! | module | job |
//! |---|---|
//! | [`args`] | the commands as clap types |
//! | [`books`] | the `books` commands: run one, print its JSON, give its exit status |

mod args;
mod books;

use std::path::PathBuf;
use std::process::ExitCode;

use clap::Parser;

use args::{Cli, Sub};

#[tokio::main(flavor = "current_thread")]
async fn main() -> ExitCode {
    let cli = Cli::parse();
    let root = cli.root.unwrap_or_else(|| PathBuf::from("."));
    match cli.command {
        Sub::Books(command) => books::run(root, command).await,
    }
}
