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
use crate::domain::shelf::build::{
    ClaimKeyProblem, DropProblem, NewDocProblem, NextKeyProblem, NoFreeKey, PlanProblem,
};
use crate::domain::shelf::doc::DocKey;
use crate::domain::shelf::exemption::ExemptRefusal;
use crate::domain::shelf::filter::FilterProblem;
use crate::domain::shelf::ledger::{
    DecisionProblem, FrameRefusal, MovedRefusal, NamedWhy, NotAdmitted, PrefixRefusal,
    SameMeaningProblem, SameMeaningRefusal, WithdrawRefusal,
};
use crate::domain::shelf::lookup::Reference;
use crate::domain::shelf::order::NotPending;
use crate::domain::shelf::schema::{BookName, KeyProblem, SchemaProblem};
use crate::domain::words::{Join, Mark, listed};

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
    /// The names `fmt --drop` was asked to drop do not fit the declaration of the
    /// books: `id` or `key`, or a name every book declares. Every problem is listed,
    /// and nothing is written.
    #[error("the names to drop have {} problem(s)", .0.len())]
    Drop(NonEmptyVec<DropProblem>),
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
    /// The move of a prefix cannot be declared: no name an approval pins is under it
    /// in any space, or a name under it cannot be moved (every one is listed, and
    /// none is moved).
    #[error("the declaration of the move of a prefix is refused")]
    MovedPrefixRefused(#[from] PrefixRefusal),
    /// The exemption cannot be made: the document is not valid, the link is no
    /// covering link into its book, it reaches the document now, the document is
    /// decided against, or an exemption that stands already exempts it.
    #[error("the exemption is refused")]
    ExemptRefused(#[from] ExemptRefusal),
    /// A record named to be withdrawn is no decision or declaration, or is already
    /// withdrawn.
    #[error("the withdrawal is refused")]
    Withdraw(#[from] WithdrawRefusal),
    /// The rules of who may decide do not admit the name the record would write for
    /// it (there is no authentication: the rule is about the name written); nothing
    /// was written.
    #[error("the record is not admitted by who may decide")]
    NotAdmitted(#[from] NotAdmitted),
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
    /// Rows of a run of new documents are refused, or cannot be planned together:
    /// every one is listed, each with the number of its row. Nothing is written for
    /// a run that is refused when it is planned.
    #[error("{}", plan_problems(.0))]
    Plan(NonEmptyVec<PlanProblem>),
    /// A row of a run of new documents is refused for a fault of the request that is
    /// told with no row of its own (an entry given twice, an item for no list, an empty
    /// item or name): the row it is of, and what is wrong with it.
    #[error("row {row}: {cause}")]
    InRow {
        /// The row.
        row: usize,
        /// What is wrong with it.
        cause: Box<ShelfError>,
    },
    /// Two or more rows of a run of new documents are each refused for a fault of the
    /// request (as [`ShelfError::InRow`] and [`ShelfError::NameOutsideLink`] tell it
    /// for one row), every one listed, in the order of the rows. A run with a fault of
    /// the request is not planned, so no other problem of its rows is told with them.
    #[error("{}", row_faults(.0))]
    Rows(Box<TwoOrMore<ShelfError>>),
    /// An item of an entry that is no link to documents names a row of the run (an
    /// entry of a text list, or a link to artifacts, takes texts only).
    #[error("row {row}: `{entry}` is no link, so it cannot name a row")]
    NameOutsideLink {
        /// The row.
        row: usize,
        /// The entry.
        entry: String,
    },
    /// A key of the run was taken while the run wrote, and a row already written links
    /// to that row by name: its file holds a key that no longer stands for it.
    /// Nothing more was written.
    #[error(
        "the key of row {row} was taken while the run wrote, and row {written_row}, already written, links to it by name"
    )]
    Stuck {
        /// The row whose key was taken.
        row: usize,
        /// The row already written that links to it.
        written_row: usize,
    },
    /// One entry was given twice for a new document.
    #[error("the entry `{name}` is given twice")]
    EntryTwice {
        /// The entry.
        name: String,
    },
    /// The name given for a row of a run is empty.
    #[error("the name of the document is empty")]
    EmptyName,
    /// An item (`name+=text`) was given for an entry the book does not read as a
    /// list.
    #[error("`{name}` is no list field or link of the book, so it takes no item")]
    ItemOfNoList {
        /// The entry.
        name: String,
    },
    /// A name of a row of the run, given as an item of a link, with nothing in it
    /// but white space. (An item that is a text of nothing is not this: it is a
    /// [`DocProblem::ItemBlank`](crate::domain::shelf::build::DocProblem::ItemBlank)
    /// of the document, which the domain tells.)
    #[error("the name given as an item of `{name}` is empty")]
    EmptyItem {
        /// The entry.
        name: String,
    },
    /// A file is already where the new document would be written (a file whose key
    /// is not its name), for a key that was asked for, or for `init`'s files and
    /// documents; nothing was written.
    #[error("{path} is already there; nothing was written")]
    FileTaken {
        /// The file.
        path: RelPath,
    },
    /// A new document's key was chosen, and a file was there at every key tried;
    /// nothing was written. The files were written by other runs at the same time,
    /// or are files the shelf did not count (one whose front matter cannot be read,
    /// say), which `check` lists as no valid document.
    #[error(
        "{0}; nothing was written: other runs may be writing at the same time, or files that are no valid document are in the way (`check` lists them); run `new` again once they are done or put right"
    )]
    NoFreeKey(NoFreeKey),
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
    /// A run of writes (`new`, `init`) wrote some files and then failed to write one:
    /// those it wrote stay, each whole, in the order they were written (for `init`:
    /// the declaration, its other files, then the documents), and
    /// the rest were not written. The files written were put on disk before this was
    /// answered; when that failed too, `not_durable` says why, and they may not
    /// survive a power loss. The class is the cause's (a port that failed, or a file
    /// that came to be at a path between the check and the write), except that when
    /// `not_durable` is there the run failed, whatever stopped it.
    #[error(
        "{} written, then {failed} was not{}",
        and_list(.written.iter(), "was", "were"),
        not_durable_note(.not_durable)
    )]
    PartWay {
        /// The files written, in order.
        written: NonEmptyVec<RelPath>,
        /// The file that was not.
        failed: RelPath,
        /// Why.
        #[source]
        cause: Box<ShelfError>,
        /// Why putting the files written on disk failed, when it did.
        not_durable: Option<PersistProblem>,
    },
    /// A run of writes wrote every file, and then putting them on disk failed. The
    /// files are there, each whole, and are not removed; a power loss may lose them.
    #[error(
        "{} written whole and left in place, but could not be put on disk: a power loss may lose what was written",
        and_list(.files.iter(), "was", "were")
    )]
    NotDurable {
        /// The files written, in order.
        files: NonEmptyVec<RelPath>,
        /// Why they could not be put on disk.
        #[source]
        cause: PersistProblem,
    },
}

/// Why the files a run wrote could not be put on disk ([`FileSync::persist`]
/// failed): the adapter's own error is kept as the source, as
/// [`ShelfError::Port`] keeps one.
///
/// [`FileSync::persist`]: super::ports::FileSync::persist
#[derive(Debug, Error)]
#[error("the {} port failed", ShelfPort::FileSync)]
pub struct PersistProblem {
    /// The adapter's error.
    #[source]
    source: Box<dyn std::error::Error + Send + Sync>,
}

impl PersistProblem {
    /// The problem a failure of the port is.
    pub(super) fn of(source: impl std::error::Error + Send + Sync + 'static) -> Self {
        Self {
            source: Box::new(source),
        }
    }

    /// The adapter's error with the reasons under it, as one line (`a: b: c`).
    pub fn reason(&self) -> String {
        let mut text = self.source.to_string();
        let mut source = self.source.source();
        while let Some(cause) = source {
            text.push_str(": ");
            text.push_str(&cause.to_string());
            source = cause.source();
        }
        text
    }
}

/// What a message adds when putting the files written on disk failed too, or
/// nothing.
fn not_durable_note(problem: &Option<PersistProblem>) -> String {
    match problem {
        Some(problem) => format!(
            "; putting what was written on disk failed too: {}",
            problem.reason()
        ),
        None => String::new(),
    }
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

/// Each problem of a run, as the answer says them: `row 2: a; row 3: b`.
fn plan_problems(problems: &NonEmptyVec<PlanProblem>) -> String {
    listed(problems.iter(), Mark::Plain, Join::Semicolon)
}

/// Each fault of the rows of a run, as the answer says them: `row 2: a; row 5: b`.
fn row_faults(faults: &TwoOrMore<ShelfError>) -> String {
    listed(faults.iter(), Mark::Plain, Join::Semicolon)
}

/// `a is`, `a and b are`, `a, b and c are`.
fn and_list<T: std::fmt::Display>(items: impl Iterator<Item = T>, one: &str, many: &str) -> String {
    let items: Vec<T> = items.collect();
    let verb = match items.len() {
        0 => return String::new(),
        1 => one,
        _ => many,
    };
    format!("{} {verb}", listed(items, Mark::Plain, Join::And))
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
            | Self::Drop(_)
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
            | Self::Plan(_)
            | Self::Rows(_)
            | Self::NameOutsideLink { .. }
            | Self::Stuck { .. }
            | Self::EmptyName
            | Self::EntryTwice { .. }
            | Self::ItemOfNoList { .. }
            | Self::EmptyItem { .. }
            | Self::FileTaken { .. }
            | Self::NoFreeKey(_)
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
                | SameMeaningRefusal::NothingToDeclare
                | SameMeaningRefusal::AlreadyDeclaredBy { .. },
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
            | Self::NotAdmitted(_)
            | Self::SameMeaningRefused(
                SameMeaningRefusal::DocNotValid { .. }
                | SameMeaningRefusal::ArtifactAbsent { .. }
                | SameMeaningRefusal::ArtifactUnknown { .. },
            )
            | Self::SameMeaning(_)
            | Self::ExemptRefused(ExemptRefusal::DocNotValid { .. }) => ErrorClass::Refused,
            Self::MovedRefused(refusal) => moved_class(refusal),
            Self::MovedPrefixRefused(refusal) => prefix_class(refusal),
            Self::Port { .. } | Self::Value(_) | Self::NotDurable { .. } => ErrorClass::Failed,
            // What was written could not be put on disk: the run failed, whatever stopped
            // it, so a caller does not take it for a request to mend and run again.
            Self::PartWay {
                not_durable: Some(_),
                ..
            } => ErrorClass::Failed,
            Self::PartWay { cause, .. } | Self::InRow { cause, .. } => cause.class(),
        }
    }
}

/// The class of the refusal of a move: the one place it is given, for a move of one
/// name and for each name of a move of a prefix ([`named_class`]).
///
/// As a reference that names nothing: two names that are one, an old name no pin is
/// read as now, and one pins read as in two spaces ([`ErrorClass::Request`]). As a
/// judgement the model refuses: a copy, a name with nothing behind it, or one that
/// cannot be told now ([`ErrorClass::Refused`]).
fn moved_class(refusal: &MovedRefusal) -> ErrorClass {
    match refusal {
        MovedRefusal::NotAMove(_)
        | MovedRefusal::NotKnown { .. }
        | MovedRefusal::InSpaces { .. } => ErrorClass::Request,
        MovedRefusal::FromPresent { .. }
        | MovedRefusal::FromUntold { .. }
        | MovedRefusal::ToAbsent { .. }
        | MovedRefusal::ToUntold { .. } => ErrorClass::Refused,
    }
}

/// The class of one name of a move of a prefix that cannot be moved: the class of
/// the rule's refusal, a request for a name no space has a name of the same form
/// under the new prefix ([`NamedWhy::Unbuildable`], the new prefix is to mend), and
/// a refusal for a name its adapter gave no answer for (it cannot be told now, as for
/// [`MovedRefusal::FromUntold`]).
fn named_class(why: &NamedWhy) -> ErrorClass {
    match why {
        NamedWhy::Move(refusal) => moved_class(refusal),
        NamedWhy::Unbuildable { .. } => ErrorClass::Request,
        NamedWhy::NoAnswer { .. } => ErrorClass::Refused,
    }
}

/// The class of the refusal of a move of a prefix: nothing under it is a request (the
/// prefix is wrong); names that cannot be moved are refused when any one of them is
/// ([`named_class`]), else a request.
fn prefix_class(refusal: &PrefixRefusal) -> ErrorClass {
    match refusal {
        PrefixRefusal::NothingUnder { .. } => ErrorClass::Request,
        PrefixRefusal::Names(names) => {
            if names
                .iter()
                .any(|named| named_class(&named.why) == ErrorClass::Refused)
            {
                ErrorClass::Refused
            } else {
                ErrorClass::Request
            }
        }
    }
}
