//! Tests of `stale`'s arguments. Its exit is tested on the binary, in
//! `tests/stale_exit_code.rs`.

use clap::Parser;

use super::BooksCommand;
use crate::args::Cli;

#[test]
fn stale_parses_with_and_without_the_flag() {
    let cli = Cli::try_parse_from(["whyshelf", "stale", "--exit-code"]).unwrap();
    assert!(matches!(
        cli.command,
        BooksCommand::Stale { exit_code: true }
    ));
    let cli = Cli::try_parse_from(["whyshelf", "stale"]).unwrap();
    assert!(matches!(
        cli.command,
        BooksCommand::Stale { exit_code: false }
    ));
}
