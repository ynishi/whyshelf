//! Why a use case of the shelf gives no answer.
//!
//! A use case does not fail because the answer is "something is wrong with the
//! documents": that is the answer (a [`BooksCheck`](super::BooksCheck) with its
//! problems, a stale list). An error is for a request that has no answer (a reference
//! that names nothing), a refusal of a judgement, and a port that failed. One variant
//! for each reason, none a message string, so a surface can decide its exit on the
//! reason ([`ShelfError::class`]).

use thiserror::Error;

use crate::domain::error::ValueError;
use crate::domain::id::DocId;
use crate::domain::location::RelPath;
use crate::domain::nonempty::{NonEmptyVec, TwoOrMore};
use crate::domain::shelf::build::{ClaimKeyProblem, NewDocProblem, NextKeyProblem};
use crate::domain::shelf::doc::DocKey;
use crate::domain::shelf::exemption::ExemptRefusal;
use crate::domain::shelf::filter::FilterProblem;
use crate::domain::shelf::ledger::{
    DecisionProblem, FrameRefusal, MovedRefusal, SameMeaningProblem, SameMeaningRefusal,
    WithdrawRefusal,
};
use crate::domain::shelf::lookup::Reference;
use crate::domain::shelf::order::NotPending;
use crate::domain::shelf::schema::{BookName, KeyProblem, SchemaProblem};

use super::ports::ShelfPort;

/// Why a use case gave no answer.
#[derive(Debug, Error)]
pub enum ShelfError {
    /// A port failed. The adapter's own error is kept as the source.
    #[error("the {port} port failed")]
    Port {
        /// Which port.
        port: ShelfPort,
        /// The adapter's error.
        #[source]
        source: Box<dyn std::error::Error + Send + Sync>,
    },
    /// The declaration of the books is not sound, so nothing can be read through it.
    /// Every problem is listed.
    #[error("the declaration of the books has {} problem(s)", .0.len())]
    Schema(NonEmptyVec<SchemaProblem>),
    /// The filter of `status` does not fit the declaration of the books: a book or a
    /// field that is not declared, or a value no book that declares the field reads.
    /// Every problem is listed.
    #[error("the filter has {} problem(s)", .0.len())]
    Filter(NonEmptyVec<FilterProblem>),
    /// The reference names nothing: no document, and no record of the ledger.
    #[error("`{reference}` names no document")]
    NotFound {
        /// The reference.
        reference: Reference,
    },
    /// The reference names more than one document.
    #[error("`{reference}` names {} documents", candidates.len())]
    Ambiguous {
        /// The reference.
        reference: Reference,
        /// The documents it could be.
        candidates: TwoOrMore<DocId>,
    },
    /// A book that is not declared is named: by a reference, or by its name alone.
    #[error(
        "{}there is no book `{book}`",
        .reference.as_ref().map(|r| format!("`{r}`: ")).unwrap_or_default()
    )]
    UnknownBook {
        /// The reference that names it, when it was named by one.
        reference: Option<Reference>,
        /// The book named.
        book: BookName,
    },
    /// The reference writes a key that the book's grammar refuses.
    #[error("`{reference}` is not a key of the book `{book}`")]
    NotAKeyOf {
        /// The reference.
        reference: Reference,
        /// The book.
        book: BookName,
        /// Why the grammar refuses it.
        #[source]
        problem: KeyProblem,
    },
    /// The reference is a key alone that no book's grammar accepts.
    #[error("`{reference}` is not a key of any book")]
    NotAKeyOfAnyBook {
        /// The reference.
        reference: Reference,
    },
    /// The document cannot be judged as asked: it is not valid, a link it stands on
    /// cannot be followed, or it is in a cycle.
    #[error("the judgement is refused")]
    Refused(#[from] FrameRefusal),
    /// The decision breaks an invariant of a decision.
    #[error("the decision is not sound")]
    Decision(#[from] DecisionProblem),
    /// The edit cannot be declared to keep the meaning: the document is not valid,
    /// the artifact is not linked, not there or cannot be told now, or no approval
    /// pinned another hash of it.
    #[error("the declaration of the same meaning is refused")]
    SameMeaningRefused(#[from] SameMeaningRefusal),
    /// The declaration breaks an invariant of a declaration.
    #[error("the declaration of the same meaning is not sound")]
    SameMeaning(#[from] SameMeaningProblem),
    /// The move cannot be declared: the two names are one, no pin is read as the old
    /// name now or pins are in two spaces, something is still at it, nothing is at
    /// the new one, or either cannot be told now.
    #[error("the declaration of a move is refused")]
    MovedRefused(#[from] MovedRefusal),
    /// The exemption cannot be made: the document is not valid, the link is no
    /// covering link into its book, it reaches the document now, the document is
    /// decided against, or an exemption that stands already exempts it.
    #[error("the exemption is refused")]
    ExemptRefused(#[from] ExemptRefusal),
    /// A record named to be withdrawn is no decision or declaration, or is already
    /// withdrawn.
    #[error("the withdrawal is refused")]
    Withdraw(#[from] WithdrawRefusal),
    /// A document named to `judge approve --pending` is not pending: it is approved
    /// or decided against, or no valid document has it now.
    #[error("`{reference}` is not pending: it is {why}")]
    NotPending {
        /// The reference.
        reference: Reference,
        /// The document it names.
        subject: DocId,
        /// What it is instead.
        why: NotPending,
    },
    /// A decision about a document that no decision stands about cannot be revoked.
    #[error("no decision stands about {subject}, so there is nothing to revoke")]
    NothingToRevoke {
        /// The document.
        subject: DocId,
    },
    /// A value was refused by its rule where the code made it: a mistake of this
    /// program, not of the request.
    #[error("a value the use case made is refused")]
    Value(#[from] ValueError),
    /// The document named as a successor does not take the place of the one deprecated.
    #[error(
        "{successor} is not a successor of {subject}: it has no `Replaces` link to it, or it is not pending or approved"
    )]
    NotASuccessor {
        /// The document deprecated.
        subject: DocId,
        /// The one named.
        successor: DocId,
    },
    /// A run of judgements with none in it.
    #[error("no document to judge was named")]
    NothingRequested,
    /// What an approval saw was asked of a document no approval stands about.
    #[error("no approval stands about {subject}, so there is nothing it saw")]
    NotApproved {
        /// The document.
        subject: DocId,
    },
    /// The book has no next key (a book of slugs, an unknown book, no number left).
    #[error("no key for the new document")]
    NextKey(#[from] NextKeyProblem),
    /// The key given for a new document is malformed or taken, or its book unknown.
    #[error("the key cannot be the new document's")]
    ClaimKey(#[from] ClaimKeyProblem),
    /// The new document would be invalid, or cannot be made of what was given.
    #[error("the new document is refused")]
    NewDoc(#[from] NewDocProblem),
    /// One entry was given twice for a new document.
    #[error("the entry `{name}` is given twice")]
    EntryTwice {
        /// The entry.
        name: String,
    },
    /// An item (`name+=text`) was given for an entry the book does not read as a
    /// list.
    #[error("`{name}` is no list field or link of the book, so it takes no item")]
    ItemOfNoList {
        /// The entry.
        name: String,
    },
    /// An item (`name+=text`) with nothing in it but white space.
    #[error("the item given for `{name}` is empty")]
    EmptyItem {
        /// The entry.
        name: String,
    },
    /// A file is already where the new document would be written (a file whose key
    /// is not its name); nothing was written.
    #[error("{path} is already there; nothing was written")]
    FileTaken {
        /// The file.
        path: RelPath,
    },
    /// A project cannot be started here: something is at the declaration's file or
    /// at a document's, or a file of a book already writes a key a document would
    /// take. Nothing was written.
    #[error("{}; nothing was written", in_the_way(.obstacles))]
    InTheWay {
        /// Each thing in the way: the paths first (the declaration's before the
        /// documents'), then the keys taken.
        obstacles: NonEmptyVec<Obstacle>,
    },
    /// `init` was asked to write two files at one path, or one under the other (the
    /// declaration, another file or a document): a fault of the request, found
    /// before anything is written. Nothing was written.
    #[error(
        "{path} would be written twice, or under {with}, which is written too; nothing was written"
    )]
    Overlapping {
        /// The later of the two, in the order they would be written.
        path: RelPath,
        /// The earlier.
        with: RelPath,
    },
    /// `init` wrote some files and then failed to write one: those it wrote stay
    /// (the declaration first, then documents, each whole), and the rest were not
    /// written. The class is the cause's: a port that failed, or a file that came
    /// to be at a path between the check and the write.
    #[error("{} written, then {failed} was not", and_list(.written.iter(), "was", "were"))]
    PartWay {
        /// The files written, in order.
        written: NonEmptyVec<RelPath>,
        /// The file that was not.
        failed: RelPath,
        /// Why.
        #[source]
        cause: Box<ShelfError>,
    },
}

/// One thing in the way of `init` ([`ShelfError::InTheWay`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Obstacle {
    /// Something is at a path it would write: a file, a directory, a link.
    There(RelPath),
    /// A file of a book already writes a key a document it makes would take.
    KeyTaken {
        /// The key.
        key: DocKey,
        /// The file that writes it.
        file: RelPath,
    },
}

impl std::fmt::Display for Obstacle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::There(path) => write!(f, "{path} is already there"),
            Self::KeyTaken { key, file } => write!(f, "{file} already writes the key {key}"),
        }
    }
}

/// The sentence of what is in the way: `a and b are already there; c already
/// writes the key k`.
fn in_the_way(obstacles: &NonEmptyVec<Obstacle>) -> String {
    let there: Vec<&RelPath> = obstacles
        .iter()
        .filter_map(|o| match o {
            Obstacle::There(path) => Some(path),
            Obstacle::KeyTaken { .. } => None,
        })
        .collect();
    let mut clauses = Vec::new();
    if !there.is_empty() {
        clauses.push(format!(
            "{} already there",
            and_list(there.into_iter(), "is", "are")
        ));
    }
    clauses.extend(obstacles.iter().filter_map(|o| match o {
        Obstacle::KeyTaken { .. } => Some(o.to_string()),
        Obstacle::There(_) => None,
    }));
    clauses.join("; ")
}

/// `a is`, `a and b are`, `a, b and c are`.
fn and_list<T: std::fmt::Display>(items: impl Iterator<Item = T>, one: &str, many: &str) -> String {
    let items: Vec<String> = items.map(|i| i.to_string()).collect();
    match items.as_slice() {
        [] => String::new(),
        [only] => format!("{only} {one}"),
        [rest @ .., last] => format!("{} and {last} {many}", rest.join(", ")),
    }
}

/// What kind of trouble an error is, for a surface to decide how to end.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorClass {
    /// The request is wrong: what it names is not there or is not one thing, or the
    /// project's declaration of its books cannot be used.
    Request,
    /// A rule of the model refused the judgement; a person can set right what it
    /// names (a file, a link, an expiry).
    Refused,
    /// The run failed: a port could not do its work.
    Failed,
}

impl ShelfError {
    /// What kind of trouble this is. The arms name every variant, with no catch-all,
    /// so an error added without a place here does not build.
    pub fn class(&self) -> ErrorClass {
        match self {
            Self::Schema(_)
            | Self::Filter(_)
            | Self::NotFound { .. }
            | Self::Ambiguous { .. }
            | Self::UnknownBook { .. }
            | Self::NotAKeyOf { .. }
            | Self::NotAKeyOfAnyBook { .. }
            | Self::NotPending { .. }
            | Self::NothingToRevoke { .. }
            | Self::NotASuccessor { .. }
            | Self::NothingRequested
            | Self::NotApproved { .. }
            | Self::NextKey(_)
            | Self::ClaimKey(_)
            | Self::NewDoc(_)
            | Self::EntryTwice { .. }
            | Self::ItemOfNoList { .. }
            | Self::EmptyItem { .. }
            | Self::FileTaken { .. }
            | Self::InTheWay { .. }
            | Self::Overlapping { .. } => ErrorClass::Request,
            // As a reference that names nothing and a revocation of nothing: a name of
            // an artifact no document links or that is linked in two spaces, a hash
            // that names nothing pinned, an earlier hash that cannot be told
            // without being named, nothing left to declare or to withdraw.
            Self::SameMeaningRefused(
                SameMeaningRefusal::ArtifactNotLinked { .. }
                | SameMeaningRefusal::ArtifactInSpaces { .. }
                | SameMeaningRefusal::NotPinned { .. }
                | SameMeaningRefusal::SeveralEarlier { .. }
                | SameMeaningRefusal::PinnedUnderOtherComparison { .. }
                | SameMeaningRefusal::NothingToDeclare,
            )
            | Self::MovedRefused(
                MovedRefusal::NotAMove(_)
                | MovedRefusal::NotKnown { .. }
                | MovedRefusal::InSpaces { .. },
            )
            | Self::Withdraw(
                WithdrawRefusal::Unknown { .. } | WithdrawRefusal::AlreadyWithdrawn { .. },
            )
            // As a reference that names nothing and nothing left to declare: a link
            // that is no covering link into the document's book, a document it
            // reaches or that is decided against (an exemption unneeded at once), and
            // an exemption already made.
            | Self::ExemptRefused(
                ExemptRefusal::NotCovering { .. }
                | ExemptRefusal::Reached { .. }
                | ExemptRefusal::DecidedAgainst { .. }
                | ExemptRefusal::AlreadyExempt { .. }
                | ExemptRefusal::AlreadyExemptInRun { .. },
            ) => ErrorClass::Request,
            // As a judgement the model refuses: no valid document, an artifact with
            // nothing behind it now or that cannot be told now.
            Self::Refused(_)
            | Self::Decision(_)
            | Self::SameMeaningRefused(
                SameMeaningRefusal::DocNotValid { .. }
                | SameMeaningRefusal::ArtifactAbsent { .. }
                | SameMeaningRefusal::ArtifactUnknown { .. },
            )
            | Self::SameMeaning(_)
            | Self::ExemptRefused(ExemptRefusal::DocNotValid { .. })
            // As a judgement the model refuses: a copy, a name with nothing behind it,
            // or one that cannot be told now.
            | Self::MovedRefused(
                MovedRefusal::FromPresent { .. }
                | MovedRefusal::FromUntold { .. }
                | MovedRefusal::ToAbsent { .. }
                | MovedRefusal::ToUntold { .. },
            ) => ErrorClass::Refused,
            Self::Port { .. } | Self::Value(_) => ErrorClass::Failed,
            Self::PartWay { cause, .. } => cause.class(),
        }
    }
}
