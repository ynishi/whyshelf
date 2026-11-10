//! Tests of how an error ends a command: the exit status `exit_of` gives each class
//! of [`ShelfError`], and the JSON `output::error` makes of it.

use serde_json::{Value, json};
use whyshelf_core::app::shelf::ports::ShelfPort;
use whyshelf_core::domain::id::DocId;
use whyshelf_core::domain::nonempty::TwoOrMore;
use whyshelf_core::domain::shelf::build::DocFailure;
use whyshelf_core::domain::shelf::ledger::{DecisionProblem, FrameRefusal};
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
            }),
            "{error:?}"
        );
    }
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
