//! Tests of how `new`'s arguments become a request: the title is an entry like any
//! other, given only when `--title` is.

use clap::Parser;

use super::*;
use crate::args::Cli;

fn entries_of(request: &NewDocRequest) -> Vec<(&str, &str)> {
    request
        .entries
        .iter()
        .map(|(k, v)| match v {
            EntryInput::Text(text) => (k.as_str(), text.as_str()),
            other => panic!("a command line gives only texts: {other:?}"),
        })
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
fn an_entry_with_plus_equals_is_one_item_as_written() {
    let request = new_request(
        "decision",
        None,
        None,
        vec![
            "rejected+=Keep it: because X, and Y".to_owned(),
            "rejected+=Drop it".to_owned(),
            "serves=REQ-1,REQ-2".to_owned(),
        ],
        None,
    )
    .unwrap();
    assert_eq!(entries_of(&request), vec![("serves", "REQ-1,REQ-2")]);
    let items: Vec<(&str, &str)> = request
        .items
        .iter()
        .map(|(k, v)| (k.as_str(), v.as_str()))
        .collect();
    assert_eq!(
        items,
        vec![
            ("rejected", "Keep it: because X, and Y"),
            ("rejected", "Drop it")
        ]
    );
}

#[test]
fn plus_equals_with_no_name_is_refused_and_a_value_may_hold_equals() {
    assert!(new_request("spec", None, None, vec!["+=x".to_owned()], None).is_err());
    let request = new_request("spec", None, None, vec!["a+=b=c".to_owned()], None).unwrap();
    assert_eq!(request.items, vec![("a".to_owned(), "b=c".to_owned())]);
}

#[test]
fn lines_are_requests_numbered_by_their_line_with_blank_lines_counted() {
    let text = "{\"book\":\"note\",\"entries\":{\"title\":\"a\"}}\n\n  \n\
                {\"book\":\"note\",\"key\":\"N-5\",\"as\":\"x\",\"entries\":{},\"body\":\"B\"}\n";
    let requests = new_lines::requests_of(text).unwrap();
    let rows: Vec<usize> = requests.iter().map(|r| r.row).collect();
    assert_eq!(rows, [1, 4]);
    assert_eq!(requests[1].key.as_deref(), Some("N-5"));
    assert_eq!(requests[1].name.as_deref(), Some("x"));
    assert_eq!(requests[1].body, "B");
    assert_eq!(requests[0].body, "");
}

#[test]
fn a_line_with_no_entries_is_a_line_with_none_given() {
    let requests =
        new_lines::requests_of("{\"book\":\"note\",\"key\":\"N-1\"}\n{\"book\":\"note\"}").unwrap();
    assert_eq!(requests.len(), 2);
    assert!(requests.iter().all(|request| request.entries.is_empty()));
    assert_eq!(requests[0].key.as_deref(), Some("N-1"));
}

#[test]
fn every_line_that_cannot_be_read_is_told_with_its_line() {
    let text = "{\"book\":\"note\",\"entries\":{}}\nnot json\n\n{\"entries\":{}}\n\
                {\"book\":\"no te\",\"entries\":{}}\n";
    let problems = new_lines::requests_of(text).unwrap_err();
    assert_eq!(problems.len(), 3, "{problems:?}");
    assert!(problems[0].starts_with("row 2: "), "{problems:?}");
    assert!(problems[1].starts_with("row 4: "), "{problems:?}");
    assert!(
        problems[2].starts_with("row 5: book `no te`"),
        "{problems:?}"
    );
}

#[test]
fn the_values_of_a_line_are_given_as_typed_and_a_name_twice_is_kept_twice() {
    use whyshelf_core::app::shelf::{EntryInput, ItemInput};
    let text = "{\"book\":\"m\",\"entries\":{\"n\":3,\"f\":true,\"t\":\"a,b\",\"l\":[\"K-1\",{\"as\":\"x\"}],\"t\":\"c\"}}";
    let requests = new_lines::requests_of(text).unwrap();
    assert_eq!(
        requests[0].entries,
        vec![
            ("n".to_owned(), EntryInput::Integer(3)),
            ("f".to_owned(), EntryInput::Bool(true)),
            ("t".to_owned(), EntryInput::Text("a,b".to_owned())),
            (
                "l".to_owned(),
                EntryInput::Items(vec![
                    ItemInput::Text("K-1".to_owned()),
                    ItemInput::Named("x".to_owned())
                ])
            ),
            ("t".to_owned(), EntryInput::Text("c".to_owned())),
        ]
    );
}

#[test]
fn a_line_has_no_field_the_shape_does_not_name() {
    for text in [
        "{\"book\":\"n\",\"entries\":{},\"colour\":1}",
        "{\"book\":\"n\",\"entries\":{\"l\":[{\"as\":\"x\",\"more\":1}]}}",
        "{\"book\":\"n\",\"entries\":[]}",
        "{\"book\":\"n\",\"entries\":{\"x\":1.5}}",
        "{\"book\":\"n\",\"entries\":{\"x\":null}}",
    ] {
        assert!(new_lines::requests_of(text).is_err(), "{text}");
    }
}

#[test]
fn new_parses_from_alone_and_refuses_it_with_a_book() {
    let cli = Cli::try_parse_from(["whyshelf", "new", "--from", "-"]).unwrap();
    let BooksCommand::New { book, from, .. } = cli.command else {
        panic!("new");
    };
    assert_eq!(book, None);
    assert_eq!(from.as_deref(), Some("-"));
    assert!(Cli::try_parse_from(["whyshelf", "new", "note", "--from", "f"]).is_err());
    assert!(Cli::try_parse_from(["whyshelf", "new"]).is_err());
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
