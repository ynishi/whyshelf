//! Tests of how an error ends a command: the exit status `exit_of` gives each class
//! of [`ShelfError`], and the JSON `output::error` makes of it.

use serde_json::{Value, json};
use whyshelf_core::app::shelf::ports::ShelfPort;
use whyshelf_core::domain::id::DocId;
use whyshelf_core::domain::nonempty::TwoOrMore;
use whyshelf_core::domain::shelf::artifact::{ArtifactKey, Space};
use whyshelf_core::domain::shelf::build::DocFailure;
use whyshelf_core::domain::shelf::ledger::PrefixRefusal;
use whyshelf_core::domain::shelf::ledger::{
    DecisionProblem, FrameRefusal, MovedRefusal, NamedRefusal, NamedWhy,
};
use whyshelf_core::domain::shelf::lookup::Reference;

use super::*;

fn docid(n: u8) -> DocId {
    let text = format!("0190a000-0000-7000-8000-0000000000{n:02x}");
    DocId::new(text.parse::<EntityId>().expect("a UUID v7"))
}

fn reference(text: &str) -> Reference {
    text.parse().expect("a reference")
}

fn not_found() -> ShelfError {
    ShelfError::NotFound {
        reference: reference("FR-1"),
    }
}

fn ambiguous() -> ShelfError {
    ShelfError::Ambiguous {
        reference: reference("FR-1"),
        candidates: TwoOrMore::new(docid(1), docid(2), vec![]),
    }
}

fn refused() -> ShelfError {
    ShelfError::Refused(FrameRefusal::NotValid(DocFailure::Gone))
}

fn not_sound() -> ShelfError {
    ShelfError::Decision(DecisionProblem::FrameNamesSubject)
}

fn failed() -> ShelfError {
    ShelfError::Port {
        port: ShelfPort::Docs,
        source: Box::new(std::io::Error::other("the disk is gone")),
    }
}

fn json_of(error: &ShelfError, messages: &[&str]) -> Value {
    serde_json::to_value(output::error(
        error,
        messages.iter().map(|m| (*m).to_owned()).collect(),
    ))
    .expect("an error is JSON")
}

// The mapping of a class to an exit is `exit_of`, which `report` uses after printing
// the error; the test asks it directly, so nothing is printed with the test run.
#[test]
fn each_class_of_error_has_its_exit_status() {
    for (error, class, exit) in [
        (not_found(), ErrorClass::Request, 2),
        (ambiguous(), ErrorClass::Request, 2),
        (refused(), ErrorClass::Refused, 1),
        (not_sound(), ErrorClass::Refused, 1),
        (failed(), ErrorClass::Failed, 3),
    ] {
        assert_eq!(error.class(), class, "{error:?}");
        assert_eq!(exit_of(error.class()) as u8, exit, "{error:?}");
    }
}

#[test]
fn the_error_json_names_the_class_and_keeps_the_messages() {
    for (error, name) in [
        (not_found(), "request"),
        (refused(), "refused"),
        (not_sound(), "refused"),
        (failed(), "failed"),
    ] {
        assert_eq!(
            json_of(&error, &["outer", "inner"]),
            json!({
                "error": name,
                "messages": ["outer", "inner"],
                "candidates": [],
                "problems": [],
                "refused": [],
            }),
            "{error:?}"
        );
    }
}

/// A move of a prefix refused for these names: each is of a space, or of none.
fn prefix_refused(names: Vec<NamedRefusal>) -> ShelfError {
    ShelfError::MovedPrefixRefused(PrefixRefusal::Names(
        NonEmptyVec::from_vec(names).expect("a name"),
    ))
}

fn named(name: &str, why: NamedWhy) -> NamedRefusal {
    NamedRefusal {
        name: ArtifactName::new(name).expect("a name"),
        why,
    }
}

fn file() -> Space {
    Space::new("file").expect("a space")
}

#[test]
fn a_move_of_a_prefix_ends_by_the_names_it_could_not_move() {
    let still_there = NamedWhy::Move(MovedRefusal::FromPresent {
        artifact: ArtifactKey::new(file(), ArtifactName::new("src/a/x.rs").expect("a name")),
    });
    let unbuildable = NamedWhy::Unbuildable {
        space: file(),
        why: NonBlank::new("too long").expect("a reason"),
    };
    let in_two = NamedWhy::Move(MovedRefusal::InSpaces {
        name: ArtifactName::new("dup").expect("a name"),
        spaces: vec![file(), Space::new("other").expect("a space")],
    });
    // Any name a rule refused is a refusal (exit 1); only names no one can build, or
    // that are the request's own fault, are a request (exit 2).
    let some = prefix_refused(vec![
        named("src/a/x.rs", still_there),
        named("src/a/y.rs", unbuildable.clone()),
    ]);
    assert_eq!(some.class(), ErrorClass::Refused);
    assert_eq!(exit_of(some.class()) as u8, 1);
    let all = prefix_refused(vec![named("src/a/y.rs", unbuildable), named("dup", in_two)]);
    assert_eq!(all.class(), ErrorClass::Request);
    assert_eq!(exit_of(all.class()) as u8, 2);
    let nothing = ShelfError::MovedPrefixRefused(PrefixRefusal::NothingUnder {
        prefix: NamePrefix::new("lib").expect("a prefix"),
        spaces: Vec::new(),
    });
    assert_eq!(nothing.class(), ErrorClass::Request);

    // Every name is listed, with its space when it is of one.
    let value = json_of(
        &some,
        &["the declaration of the move of a prefix is refused"],
    );
    assert_eq!(
        value["refused"],
        json!([
            {
                "name": "src/a/x.rs",
                "space": "file",
                "why": "`src/a/x.rs` in `file` is still there: a copy is no move",
            },
            {
                "name": "src/a/y.rs",
                "space": "file",
                "why": "no name of `file` is the same name under the new prefix: too long",
            },
        ])
    );
    let value = json_of(&all, &[]);
    assert_eq!(value["refused"][1]["space"], Value::Null);
    assert_eq!(value["error"], "request");
    assert_eq!(json_of(&nothing, &[])["refused"], json!([]));
}

#[test]
fn the_error_json_of_an_ambiguous_reference_lists_the_candidates() {
    let value = json_of(&ambiguous(), &["`FR-1` names 2 documents"]);
    assert_eq!(value["error"], "request");
    assert_eq!(value["messages"], json!(["`FR-1` names 2 documents"]));
    assert_eq!(
        value["candidates"],
        json!([docid(1).to_string(), docid(2).to_string()])
    );
}
