//! Tests of how the arguments of `status` become a filter: each shape that is no
//! request is told, and the words of the standings are one table read both ways.

use std::collections::BTreeSet;

use clap::Parser;
use whyshelf_core::domain::shelf::freshness::{FreshnessLevel, StandingKind};
use whyshelf_core::domain::shelf::ledger::ReaderCount;

use super::*;
use crate::args::Cli;

fn strings(items: &[&str]) -> Vec<String> {
    items.iter().map(|s| (*s).to_owned()).collect()
}

#[test]
fn nothing_given_asks_nothing() {
    assert_eq!(
        status_ask(&[], &[], &[], &[], &[], &[]),
        Ok(StatusFilterAsk::default())
    );
}

#[test]
fn books_standings_and_conditions_are_read_as_given() {
    let ask = status_ask(
        &strings(&["spec", "finding", "spec"]),
        &strings(&["stale", "pending"]),
        &strings(&["one", "nobody", "one"]),
        &strings(&["issues=#41", "title=a=b", "note="]),
        &[],
        &[],
    )
    .unwrap();
    assert_eq!(
        ask.readers,
        BTreeSet::from([ReaderCount::Nobody, ReaderCount::One])
    );
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
        .map(|f| match &f.value {
            FieldWant::Value(value) => (f.field.as_str(), value.as_str()),
            FieldWant::NoValue | FieldWant::AnyValue => {
                panic!("no `--without` or `--with` was given")
            }
        })
        .collect();
    // In the order given; the value is what follows the first `=`.
    assert_eq!(
        fields,
        vec![("issues", "#41"), ("title", "a=b"), ("note", "")]
    );
}

#[test]
fn a_book_that_is_no_name_is_told() {
    let error = status_ask(&strings(&["Not A Book"]), &[], &[], &[], &[], &[]).unwrap_err();
    assert!(error.starts_with("--book `Not A Book`: "), "{error}");
}

#[test]
fn a_word_that_is_no_standing_is_told_with_the_words_there_are() {
    let error = status_ask(&[], &strings(&["soon"]), &[], &[], &[], &[]).unwrap_err();
    assert!(error.starts_with("--standing `soon`: "), "{error}");
    for word in StandingKind::ALL.map(output::standing) {
        assert!(error.contains(word), "{word}: {error}");
    }
    // A word is exact: no case, no part.
    for word in ["Fresh", "fre", ""] {
        assert!(
            status_ask(&[], &strings(&[word]), &[], &[], &[], &[]).is_err(),
            "{word:?}"
        );
    }
}

#[test]
fn a_word_that_is_no_count_of_readers_is_told_with_the_words_there_are() {
    let error = status_ask(&[], &[], &strings(&["two"]), &[], &[], &[]).unwrap_err();
    assert!(error.starts_with("--readers `two`: "), "{error}");
    for word in ReaderCount::ALL.map(output::readers) {
        assert!(error.contains(word), "{word}: {error}");
    }
    for word in ["One", "on", "", "none"] {
        assert!(
            status_ask(&[], &[], &strings(&[word]), &[], &[], &[]).is_err(),
            "{word:?}"
        );
    }
}

#[test]
fn every_count_of_readers_has_one_word_that_reads_back_as_it() {
    let words: BTreeSet<&str> = ReaderCount::ALL.into_iter().map(output::readers).collect();
    assert_eq!(words.len(), ReaderCount::ALL.len(), "a word once");
    for count in ReaderCount::ALL {
        let word = output::readers(count);
        let ask = status_ask(&[], &[], &strings(&[word]), &[], &[], &[]).unwrap();
        assert_eq!(ask.readers, BTreeSet::from([count]), "{word}");
    }
}

#[test]
fn a_condition_with_no_equals_or_a_field_that_is_no_name_is_told() {
    let error = status_ask(&[], &[], &[], &strings(&["issues"]), &[], &[]).unwrap_err();
    assert!(error.starts_with("--where `issues`: "), "{error}");
    for given in ["Issues=1", "=1", "id=1", "two words=1"] {
        let error = status_ask(&[], &[], &[], &strings(&[given]), &[], &[]).unwrap_err();
        assert!(
            error.starts_with(&format!("--where `{given}`: ")),
            "{error}"
        );
    }
}

#[test]
fn the_first_error_is_the_one_told() {
    let error = status_ask(
        &strings(&["Bad"]),
        &strings(&["soon"]),
        &strings(&["two"]),
        &strings(&["x"]),
        &strings(&["Bad Field"]),
        &strings(&["Bad Field"]),
    )
    .unwrap_err();
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
        "--readers",
        "one",
        "--readers",
        "several",
        "--where",
        "x=1",
        "--where",
        "y=2",
        "--without",
        "z",
        "--without",
        "w",
        "--with",
        "u",
        "--with",
        "v",
    ])
    .unwrap();
    let BooksCommand::Status {
        books,
        standings,
        readers,
        wheres,
        withouts,
        withs,
    } = cli.command
    else {
        panic!("not a status command");
    };
    assert_eq!(books, strings(&["a", "b"]));
    assert_eq!(standings, strings(&["stale"]));
    assert_eq!(readers, strings(&["one", "several"]));
    assert_eq!(wheres, strings(&["x=1", "y=2"]));
    assert_eq!(withouts, strings(&["z", "w"]));
    assert_eq!(withs, strings(&["u", "v"]));
}

#[test]
fn each_with_is_a_condition_of_any_value_after_the_withouts() {
    let ask = status_ask(
        &[],
        &[],
        &[],
        &strings(&["issues=#41"]),
        &strings(&["source"]),
        &strings(&["tags", "note"]),
    )
    .unwrap();
    let fields: Vec<(&str, FieldWant)> = ask
        .fields
        .iter()
        .map(|f| (f.field.as_str(), f.value.clone()))
        .collect();
    assert_eq!(
        fields,
        vec![
            ("issues", FieldWant::Value("#41".to_owned())),
            ("source", FieldWant::NoValue),
            ("tags", FieldWant::AnyValue),
            ("note", FieldWant::AnyValue),
        ]
    );
}

#[test]
fn a_with_that_is_no_field_name_is_told() {
    for given in ["Source", "", "two words", "id"] {
        let error = status_ask(&[], &[], &[], &[], &[], &strings(&[given])).unwrap_err();
        assert!(error.starts_with(&format!("--with `{given}`: ")), "{error}");
    }
}

#[test]
fn each_drop_is_a_name_that_says_something() {
    let ask = drop_ask(&strings(&["source", "tags", "source"])).unwrap();
    let names: Vec<&str> = ask.names.iter().map(NonBlank::as_str).collect();
    assert_eq!(names, vec!["source", "tags"]);
    assert_eq!(drop_ask(&[]), Ok(DropAsk::default()));
    for given in ["", " ", "\t"] {
        let error = drop_ask(&strings(&["source", given])).unwrap_err();
        assert!(error.starts_with(&format!("--drop `{given}`: ")), "{error}");
    }
}

#[test]
fn the_arguments_of_fmt_are_repeatable() {
    let cli =
        Cli::try_parse_from(["whyshelf", "fmt", "--check", "--drop", "a", "--drop", "b"]).unwrap();
    let BooksCommand::Fmt { check, drops } = cli.command else {
        panic!("not an fmt command");
    };
    assert!(check);
    assert_eq!(drops, strings(&["a", "b"]));
}

#[test]
fn each_without_is_a_condition_of_no_value_after_the_wheres() {
    let ask = status_ask(
        &[],
        &[],
        &[],
        &strings(&["issues=#41", "note="]),
        &strings(&["source", "tags"]),
        &[],
    )
    .unwrap();
    let fields: Vec<(&str, FieldWant)> = ask
        .fields
        .iter()
        .map(|f| (f.field.as_str(), f.value.clone()))
        .collect();
    // `--where note=` stays a condition on the value `` (the filter refuses it);
    // `--without` is the way to ask for no value.
    assert_eq!(
        fields,
        vec![
            ("issues", FieldWant::Value("#41".to_owned())),
            ("note", FieldWant::Value(String::new())),
            ("source", FieldWant::NoValue),
            ("tags", FieldWant::NoValue),
        ]
    );
}

#[test]
fn a_without_that_is_no_field_name_is_told() {
    for given in ["Source", "", "two words", "id"] {
        let error = status_ask(&[], &[], &[], &[], &strings(&[given]), &[]).unwrap_err();
        assert!(
            error.starts_with(&format!("--without `{given}`: ")),
            "{error}"
        );
    }
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
        let ask = status_ask(&[], &strings(&[word]), &[], &[], &[], &[]).unwrap();
        assert_eq!(ask.standings, BTreeSet::from([kind]), "{word}");
    }
}
