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
use crate::domain::nonempty::{NonEmptyVec, TwoOrMore};
use crate::domain::shelf::ledger::{DecisionProblem, FrameRefusal};
use crate::domain::shelf::lookup::Reference;
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
    /// The reference names a book that is not declared.
    #[error("`{reference}`: there is no book `{book}`")]
    UnknownBook {
        /// The reference.
        reference: Reference,
        /// The book it names.
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
            | Self::NotFound { .. }
            | Self::Ambiguous { .. }
            | Self::UnknownBook { .. }
            | Self::NotAKeyOf { .. }
            | Self::NotAKeyOfAnyBook { .. }
            | Self::NothingToRevoke { .. }
            | Self::NotASuccessor { .. }
            | Self::NothingRequested => ErrorClass::Request,
            Self::Refused(_) | Self::Decision(_) => ErrorClass::Refused,
            Self::Port { .. } | Self::Value(_) => ErrorClass::Failed,
        }
    }
}
