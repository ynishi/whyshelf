//! Tests of how the arguments of `status` become a filter: each shape that is no
//! request is told, and the words of the standings are one table read both ways.

use std::collections::BTreeSet;

use clap::Parser;
use whyshelf_core::domain::shelf::freshness::{FreshnessLevel, StandingKind};

use super::*;
use crate::args::Cli;

fn strings(items: &[&str]) -> Vec<String> {
    items.iter().map(|s| (*s).to_owned()).collect()
}

#[test]
fn nothing_given_asks_nothing() {
    assert_eq!(status_ask(&[], &[], &[]), Ok(StatusFilterAsk::default()));
}

#[test]
fn books_standings_and_conditions_are_read_as_given() {
    let ask = status_ask(
        &strings(&["spec", "finding", "spec"]),
        &strings(&["stale", "pending"]),
        &strings(&["issues=#41", "title=a=b", "note="]),
    )
    .unwrap();
    let books: Vec<&str> = ask.books.iter().map(BookName::as_str).collect();
    assert_eq!(books, vec!["finding", "spec"]);
    assert_eq!(
        ask.standings,
        BTreeSet::from([
            StandingKind::Pending,
            StandingKind::Approved(FreshnessLevel::Stale)
        ])
    );
    let fields: Vec<(&str, &str)> = ask
        .fields
        .iter()
        .map(|f| (f.field.as_str(), f.value.as_str()))
        .collect();
    // In the order given; the value is what follows the first `=`.
    assert_eq!(
        fields,
        vec![("issues", "#41"), ("title", "a=b"), ("note", "")]
    );
}

#[test]
fn a_book_that_is_no_name_is_told() {
    let error = status_ask(&strings(&["Not A Book"]), &[], &[]).unwrap_err();
    assert!(error.starts_with("--book `Not A Book`: "), "{error}");
}

#[test]
fn a_word_that_is_no_standing_is_told_with_the_words_there_are() {
    let error = status_ask(&[], &strings(&["soon"]), &[]).unwrap_err();
    assert!(error.starts_with("--standing `soon`: "), "{error}");
    for word in StandingKind::ALL.map(output::standing) {
        assert!(error.contains(word), "{word}: {error}");
    }
    // A word is exact: no case, no part.
    for word in ["Fresh", "fre", ""] {
        assert!(status_ask(&[], &strings(&[word]), &[]).is_err(), "{word:?}");
    }
}

#[test]
fn a_condition_with_no_equals_or_a_field_that_is_no_name_is_told() {
    let error = status_ask(&[], &[], &strings(&["issues"])).unwrap_err();
    assert!(error.starts_with("--where `issues`: "), "{error}");
    for given in ["Issues=1", "=1", "id=1", "two words=1"] {
        let error = status_ask(&[], &[], &strings(&[given])).unwrap_err();
        assert!(
            error.starts_with(&format!("--where `{given}`: ")),
            "{error}"
        );
    }
}

#[test]
fn the_first_error_is_the_one_told() {
    let error = status_ask(&strings(&["Bad"]), &strings(&["soon"]), &strings(&["x"])).unwrap_err();
    assert!(error.starts_with("--book "), "{error}");
}

#[test]
fn the_arguments_of_status_are_repeatable() {
    let cli = Cli::try_parse_from([
        "whyshelf",
        "status",
        "--book",
        "a",
        "--book",
        "b",
        "--standing",
        "stale",
        "--where",
        "x=1",
        "--where",
        "y=2",
    ])
    .unwrap();
    let BooksCommand::Status {
        books,
        standings,
        wheres,
    } = cli.command
    else {
        panic!("not a status command");
    };
    assert_eq!(books, strings(&["a", "b"]));
    assert_eq!(standings, strings(&["stale"]));
    assert_eq!(wheres, strings(&["x=1", "y=2"]));
}

#[test]
fn every_standing_has_one_word_that_reads_back_as_it() {
    // That `StandingKind::ALL` holds each kind once is the core's test; here, each
    // kind has its own word, and the word reads back as it.
    let words: BTreeSet<&str> = StandingKind::ALL
        .into_iter()
        .map(output::standing)
        .collect();
    assert_eq!(words.len(), StandingKind::ALL.len(), "a word once");
    for kind in StandingKind::ALL {
        // What an answer prints is what `--standing` reads back.
        let word = output::standing(kind);
        let ask = status_ask(&[], &strings(&[word]), &[]).unwrap();
        assert_eq!(ask.standings, BTreeSet::from([kind]), "{word}");
    }
}
