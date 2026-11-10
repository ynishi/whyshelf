//! The command line: the one description of the commands (help, the list of
//! commands and the checks of the arguments come from here).
//!
//! The commands are verbs at the top level, as one kind of thing (the documents of
//! the declared books) is all the tool works on; a group of commands is added only
//! when a second kind of thing needs one.

use std::path::PathBuf;

use clap::Parser;

/// whyshelf: documents whose kinds a project declares, linked to one another and to
/// the files they stand on, judged, and checked for whether each judgement still
/// holds.
#[derive(Debug, Parser)]
// No `help` command: only `--help` and `--version` answer in text, and every other
// argument is a command or a wrong request answered in JSON.
#[command(name = "whyshelf", version, about, disable_help_subcommand = true)]
pub struct Cli {
    /// The project root (default: the current directory).
    #[arg(long, global = true, value_name = "DIR")]
    pub root: Option<PathBuf>,
    /// What to do.
    #[command(subcommand)]
    pub command: crate::books::BooksCommand,
}
