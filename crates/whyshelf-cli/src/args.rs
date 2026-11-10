//! The command line: the one description of the commands (help, the list of
//! commands and the checks of the arguments come from here).

use std::path::PathBuf;

use clap::{Parser, Subcommand};

/// whyshelf: documents whose kinds a project declares, linked to one another and to
/// the files they stand on, judged, and checked for whether each judgement still
/// holds.
#[derive(Debug, Parser)]
#[command(name = "whyshelf", version, about)]
pub struct Cli {
    /// The project root (default: the current directory).
    #[arg(long, global = true, value_name = "DIR")]
    pub root: Option<PathBuf>,
    /// What to do.
    #[command(subcommand)]
    pub command: Sub,
}

/// Every command.
#[derive(Debug, Subcommand)]
pub enum Sub {
    /// Documents whose kinds the project declares in `.whyshelf/books.toml`: check,
    /// status, get, trace, stale, show, diff, new, judge, schema. Answers in JSON.
    #[command(subcommand)]
    Books(crate::books::BooksCommand),
}
