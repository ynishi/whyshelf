//! Tests of how `new`'s arguments become a request: the title is an entry like any
//! other, given only when `--title` is.

use clap::Parser;

use super::*;
use crate::args::Cli;

fn entries_of(request: &NewDocRequest) -> Vec<(&str, &str)> {
    request
        .entries
        .iter()
        .map(|(k, v)| (k.as_str(), v.as_str()))
        .collect()
}

#[test]
fn with_no_title_no_title_entry_is_sent() {
    let request = new_request("measure", None, None, vec!["count=3".to_owned()], None).unwrap();
    assert_eq!(entries_of(&request), vec![("count", "3")]);
}

#[test]
fn a_title_is_sent_as_the_entry_title_first() {
    let request = new_request(
        "spec",
        Some("Sign-in".to_owned()),
        None,
        vec!["grounds=F-1".to_owned()],
        None,
    )
    .unwrap();
    assert_eq!(
        entries_of(&request),
        vec![("title", "Sign-in"), ("grounds", "F-1")]
    );
}

#[test]
fn a_title_may_be_given_as_an_entry_too() {
    let request = new_request("spec", None, None, vec!["title=Sign-in".to_owned()], None).unwrap();
    assert_eq!(entries_of(&request), vec![("title", "Sign-in")]);
}

#[test]
fn new_parses_without_title() {
    let cli = Cli::try_parse_from(["whyshelf", "new", "measure", "count=3"]).unwrap();
    let BooksCommand::New { title, entries, .. } = cli.command else {
        panic!("new");
    };
    assert_eq!(title, None);
    assert_eq!(entries, vec!["count=3".to_owned()]);
}
