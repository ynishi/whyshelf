//! Tests of how a document that is not valid is told: the clause of each of the three
//! ([`FailureTag::said_of`]) and the sentence a failure makes of it with what it
//! carries ([`DocFailure::said_of`]).

use crate::domain::location::RelPath;
use crate::domain::nonempty::NonEmptyVec;

use super::{Detail, DocFailure, DocProblem, FailureTag};

fn path(text: &str) -> RelPath {
    text.parse().expect("a path")
}

fn invalid() -> DocFailure {
    DocFailure::Invalid(NonEmptyVec::new(
        DocProblem::IdMissing,
        vec![DocProblem::KeyMissing],
    ))
}

fn unknown() -> DocFailure {
    DocFailure::Unknown(NonEmptyVec::new(path("a.md"), vec![path("b/c.md")]))
}

#[test]
fn a_tag_says_its_clause_of_the_subject_it_is_given() {
    for (tag, subject, want) in [
        (FailureTag::Invalid, "it", "it is invalid"),
        (
            FailureTag::Invalid,
            "the document",
            "the document is invalid",
        ),
        (
            FailureTag::Gone,
            "it",
            "no file in the books' ranges carries it",
        ),
        (
            FailureTag::Gone,
            "spec:SP-1",
            "no file in the books' ranges carries spec:SP-1",
        ),
        (
            FailureTag::Unknown,
            "it",
            "whether a file carries it cannot be told",
        ),
        (
            FailureTag::Unknown,
            "the document",
            "whether a file carries the document cannot be told",
        ),
    ] {
        assert_eq!(tag.said_of(&subject), want, "{tag:?} of {subject}");
    }
}

#[test]
fn the_display_of_a_tag_is_its_clause_of_it() {
    for tag in [FailureTag::Invalid, FailureTag::Gone, FailureTag::Unknown] {
        assert_eq!(tag.to_string(), tag.said_of(&"it"), "{tag:?}");
    }
    assert_eq!(FailureTag::Invalid.to_string(), "it is invalid");
}

#[test]
fn a_failure_in_full_tells_what_it_carries() {
    let subject = "the document";
    assert_eq!(
        invalid().said_of(&subject, Detail::Full),
        "the document is invalid: there is no `id:`; there is no `key:`"
    );
    assert_eq!(
        DocFailure::Gone.said_of(&subject, Detail::Full),
        "no file in the books' ranges carries the document"
    );
    assert_eq!(
        unknown().said_of(&subject, Detail::Full),
        "whether a file carries the document cannot be told; unreadable: a.md; b/c.md"
    );
}

#[test]
fn a_failure_in_short_drops_only_the_problems_of_an_invalid_document() {
    let subject = "spec:SP-1";
    assert_eq!(
        invalid().said_of(&subject, Detail::Short),
        "spec:SP-1 is invalid"
    );
    assert_eq!(
        DocFailure::Gone.said_of(&subject, Detail::Short),
        "no file in the books' ranges carries spec:SP-1"
    );
    // The files that could not be read are the whole of what is known, so they stay.
    assert_eq!(
        unknown().said_of(&subject, Detail::Short),
        "whether a file carries spec:SP-1 cannot be told; unreadable: a.md; b/c.md"
    );
}

#[test]
fn a_failure_in_short_is_its_clause_unless_it_has_files_to_name() {
    for failure in [invalid(), DocFailure::Gone] {
        assert_eq!(
            failure.said_of(&"it", Detail::Short),
            failure.tag().said_of(&"it")
        );
    }
}
