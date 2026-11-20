//! The answer when a command gives none: the error, its causes and its list of
//! problems, and the answers for a request the command line itself refuses.

use schemars::JsonSchema;
use serde::Serialize;
use whyshelf_core::app::shelf::{ErrorClass, ShelfError};
use whyshelf_core::domain::shelf::build::NewDocProblem;
use whyshelf_core::domain::shelf::ledger::PrefixRefusal;

use super::words::schema_problem;

/// The answer when a command gives none.
#[derive(Debug, Serialize, JsonSchema)]
pub struct ErrorOut {
    /// `request`, `refused` or `failed` (the exit status is 2, 1 or 3).
    pub error: &'static str,
    /// The error and its causes, outermost first.
    pub messages: Vec<String>,
    /// The documents a reference could be, when it names more than one.
    pub candidates: Vec<String>,
    /// Each problem, one per item, when the error has a list of them: the problems
    /// of a declaration that cannot be used, or of a new document that would be
    /// invalid; for `new --from`, each row refused (`row <n>: ...`, the line of the
    /// file) or each line that cannot be read; for `init`, each thing in the way, or
    /// the files written before a failure part way and the one that was not.
    pub problems: Vec<String>,
    /// For `judge moved --prefix`, each name under the prefix that cannot be moved,
    /// with why (every one is listed, and none is moved); empty for any other error.
    pub refused: Vec<RefusedNameOut>,
}

/// A name under the prefix of `judge moved --prefix` that cannot be moved.
#[derive(Debug, Serialize, JsonSchema)]
pub struct RefusedNameOut {
    /// The old name.
    pub name: String,
    /// The space the refusal is about, when it is of one (none when it is of the name
    /// as such: pins read as it in two spaces, or no move).
    pub space: Option<String>,
    /// Why, in words.
    pub why: String,
}

/// The answer of a command that gave none.
pub fn error(error: &ShelfError, messages: Vec<String>) -> ErrorOut {
    ErrorOut {
        error: match error.class() {
            ErrorClass::Request => "request",
            ErrorClass::Refused => "refused",
            ErrorClass::Failed => "failed",
        },
        messages,
        candidates: match error {
            ShelfError::Ambiguous { candidates, .. } => {
                candidates.iter().map(ToString::to_string).collect()
            }
            _ => Vec::new(),
        },
        problems: match error {
            ShelfError::Schema(problems) => problems.iter().map(schema_problem).collect(),
            ShelfError::Filter(problems) => problems.iter().map(ToString::to_string).collect(),
            ShelfError::Drop(problems) => problems.iter().map(ToString::to_string).collect(),
            ShelfError::NewDoc(NewDocProblem::Invalid(problems)) => {
                problems.iter().map(ToString::to_string).collect()
            }
            ShelfError::Plan(problems) => problems.iter().map(ToString::to_string).collect(),
            ShelfError::Rows(faults) => faults.iter().map(ToString::to_string).collect(),
            ShelfError::InTheWay { obstacles } => {
                obstacles.iter().map(ToString::to_string).collect()
            }
            ShelfError::PartWay {
                written,
                failed,
                cause,
                not_durable,
            } => written
                .iter()
                .map(|path| format!("{path} was written"))
                .chain(std::iter::once(format!(
                    "{failed} was not written: {cause}"
                )))
                .chain(not_durable.iter().map(|problem| {
                    format!(
                        "putting what was written on disk failed too: {}",
                        problem.reason()
                    )
                }))
                .collect(),
            ShelfError::NotDurable { files, cause } => files
                .iter()
                .map(|path| format!("{path} was written whole, and could not be put on disk"))
                .chain(std::iter::once(format!(
                    "putting what was written on disk failed: {}",
                    cause.reason()
                )))
                .collect(),
            _ => Vec::new(),
        },
        refused: match error {
            ShelfError::MovedPrefixRefused(PrefixRefusal::Names(names)) => names
                .iter()
                .map(|named| RefusedNameOut {
                    name: named.name.to_string(),
                    space: named.space().map(ToString::to_string),
                    why: named.why.to_string(),
                })
                .collect(),
            _ => Vec::new(),
        },
    }
}

/// The answer when the file of `new --from` gives no run: what is wrong, and each
/// line that cannot be read, with its number.
pub fn lines_refused(messages: Vec<String>, problems: Vec<String>) -> ErrorOut {
    ErrorOut {
        error: "request",
        messages,
        candidates: Vec::new(),
        problems,
        refused: Vec::new(),
    }
}

/// The answer to arguments that are no request.
pub fn usage(message: String) -> ErrorOut {
    ErrorOut {
        error: "request",
        messages: vec![message],
        candidates: Vec::new(),
        problems: Vec::new(),
        refused: Vec::new(),
    }
}

/// The answer when the program cannot make its own request (a template it ships is
/// not one): a run that failed.
pub fn failure(messages: Vec<String>) -> ErrorOut {
    ErrorOut {
        error: "failed",
        messages,
        candidates: Vec::new(),
        problems: Vec::new(),
        refused: Vec::new(),
    }
}
