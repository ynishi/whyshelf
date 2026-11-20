//! Who may decide, read against the records: [`Admit`], what the admission reads of
//! the shelf; the book a record is read with ([`RecordBook`]); why a record is not
//! admitted ([`NotAdmitted`]); and [`AdmittedRecord`], the one thing a write of the
//! ledger takes, which only [`Ledger::accept`] makes.
//!
//! The admission is one judgement, `Admit::admission`, asked of every record by
//! [`Ledger::build`] as it reads the ledger, and of every record a run would write by
//! [`Ledger::accept`], on the ledger the earlier records of the run left and before the
//! record is added to it. What a rule says is the schema's
//! ([`Authority::permission`]); which rule reads a record is [`RecordBook`]'s:
//!
//! | record | the book it is read with |
//! |---|---|
//! | a decision, a declaration of the same meaning of a document, naming a book declared now (key `book`) | that book |
//! | the same, naming a book not declared now | `[decide]` |
//! | the same, naming none (made before records named one, or of a document whose book could not be told) | the book the shelf tells its document is in now, else `[decide]` |
//! | an exemption | the book of its link, else (not declared now) `[decide]` |
//! | a declaration of a move, of the same meaning of an artifact | `[decide]` |
//! | a revocation | for each record it names, that record's book, with the kind `revoke` |
//!
//! A record the rules do not admit is not used: it makes no decision stand, reads no
//! one as a reader, exempts and declares nothing, and no count reads it. It stays in
//! the ledger as a [`LedgerProblem::NotAdmitted`](super::LedgerProblem::NotAdmitted)
//! (so `check` fails), until a revocation by a name admitted to revoke in its book
//! withdraws it. A revocation not admitted for one record it names withdraws nothing
//! of that one, and is a problem for it while no revocation the rules admit
//! withdraws that record (a revocation is never withdrawn, so nothing else could put
//! it right); it withdraws the others.
//!
//! A record that names no book is read with the book its document is in now, so a
//! file moved to another book after a rule is declared reads its earlier records with
//! the rule of the new one: a rule declared over an earlier ledger is best given a
//! `since`. Only a project that declares a rule names the book in a new record
//! ([`Shelf::book_to_record`]); a build of whyshelf from before records named a book
//! cannot read one that does: it is a key it does not know.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::sync::Arc;

use thiserror::Error;

use crate::domain::actor::Actor;
use crate::domain::id::DocId;
use crate::domain::nonempty::NonEmptySet;
use crate::domain::words::{Join, Mark, listed};

use super::super::build::Shelf;
use super::super::schema::{Authority, BookName, DecideKind, GroupName, Permission, RuleTime};
use super::{
    DecisionVerb, JudgedAt, JudgementId, Ledger, Record, Revocation, SameMeaningOf, target_book,
};

/// What the admission reads of the shelf: who may decide, the books declared now,
/// and the book each document is in now. Shared, not copied, by every ledger built
/// with it (a run builds one per record it takes).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Admit(Arc<Rules>);

/// What an [`Admit`] holds.
#[derive(Debug, Default, PartialEq, Eq)]
struct Rules {
    authority: Authority,
    declared: BTreeSet<BookName>,
    now: BTreeMap<DocId, BookName>,
}

impl Admit {
    /// The admission of `shelf`: its schema's [`Authority`], read with the books it
    /// declares and the book of each of its documents ([`Shelf::book_of`]).
    pub fn of(shelf: &Shelf) -> Self {
        let authority = shelf.schema().authority().clone();
        if authority.is_unruled() {
            return Self::unruled();
        }
        Self(Arc::new(Rules {
            declared: shelf.schema().books().keys().cloned().collect(),
            now: shelf
                .ids()
                .into_iter()
                .filter_map(|id| shelf.book_of(id).map(|book| (id, book.clone())))
                .collect(),
            authority,
        }))
    }

    /// The admission of a project that declares no rule: every record is admitted,
    /// as before rules existed.
    pub fn unruled() -> Self {
        Self::default()
    }

    /// Whether no rule is declared: every record is admitted, whatever its book.
    pub(super) fn is_unruled(&self) -> bool {
        self.0.authority.is_unruled()
    }

    /// The book `record` is read with (the table is on the module page); for a
    /// revocation, of no record: each record it names is read with its own book.
    pub(super) fn book_of(&self, record: &Record) -> RecordBook {
        match record {
            Record::Decision(d) => self.named(d.book(), Some(d.subject())),
            Record::SameMeaning(s) => match s.of() {
                SameMeaningOf::Doc { id, .. } => self.named(s.book(), Some(*id)),
                SameMeaningOf::Artifact { .. } => RecordBook::Shelf,
            },
            Record::Exempt(e) => self.named(Some(e.of().book()), None),
            Record::Moved(_) | Record::Revocation(_) => RecordBook::Shelf,
        }
    }

    fn named(&self, named: Option<&BookName>, subject: Option<DocId>) -> RecordBook {
        match named {
            Some(book) if self.0.declared.contains(book) => RecordBook::Named(book.clone()),
            Some(_) => RecordBook::Shelf,
            None => subject
                .and_then(|id| self.0.now.get(&id))
                .map_or(RecordBook::Shelf, |book| RecordBook::Now(book.clone())),
        }
    }

    /// Whether `record`, read with `book`, is admitted; for a revocation, to withdraw
    /// the record `target`, read with that record's `book`. The one admission: a
    /// record a rule reads and refuses is not, any other is.
    // The refusal is the answer, made once per record not admitted; it is not a hot
    // path.
    #[allow(clippy::result_large_err)]
    pub(super) fn admission(
        &self,
        record: &Record,
        target: Option<JudgementId>,
        book: &RecordBook,
    ) -> Result<(), NotAdmitted> {
        let (kind, actor, at) = match record {
            Record::Decision(d) => (
                match d.verb() {
                    DecisionVerb::Approve { .. } => DecideKind::Approve,
                    DecisionVerb::Deny { .. } => DecideKind::Deny,
                    DecisionVerb::Deprecate { .. } => DecideKind::Deprecate,
                    DecisionVerb::Retire { .. } => DecideKind::Retire,
                },
                d.actor(),
                d.at(),
            ),
            Record::Revocation(r) => (DecideKind::Revoke, &r.actor, r.at),
            Record::SameMeaning(s) => (DecideKind::Same, s.actor(), s.at()),
            Record::Moved(m) => (DecideKind::Moved, &m.actor, m.at),
            Record::Exempt(e) => (DecideKind::Exempt, e.actor(), e.at()),
        };
        self.judged(record.id(), kind, actor, at, target, book)
    }

    /// Whether `revocation` is admitted to withdraw the record `target`, read with
    /// that record's `book`: [`admission`](Self::admission) of a revocation, without
    /// making it a record.
    #[allow(clippy::result_large_err)]
    pub(super) fn revocation(
        &self,
        revocation: &Revocation,
        target: JudgementId,
        book: &RecordBook,
    ) -> Result<(), NotAdmitted> {
        self.judged(
            revocation.id,
            DecideKind::Revoke,
            &revocation.actor,
            revocation.at,
            Some(target),
            book,
        )
    }

    #[allow(clippy::result_large_err)]
    fn judged(
        &self,
        record: JudgementId,
        kind: DecideKind,
        actor: &Actor,
        at: JudgedAt,
        target: Option<JudgementId>,
        book: &RecordBook,
    ) -> Result<(), NotAdmitted> {
        let at = RuleTime::from_millis(at.millis());
        match self.0.authority.permission(book.book(), kind, actor, at) {
            Permission::Unruled | Permission::Open | Permission::Admitted => Ok(()),
            Permission::Refused { groups, since } => Err(NotAdmitted {
                record,
                target,
                actor: actor.clone(),
                kind,
                book: book.clone(),
                groups,
                since,
            }),
        }
    }
}

/// The book a record is read with by who may decide.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RecordBook {
    /// The book the record names (its key `book`; an exemption, the book of its link),
    /// declared now.
    Named(BookName),
    /// The book the shelf tells its document is in now, for a record that names none.
    Now(BookName),
    /// None: the records of no book (`[decide]`), and those whose book cannot be told
    /// or is not declared now.
    Shelf,
}

impl RecordBook {
    /// The book, or none for `[decide]`.
    pub fn book(&self) -> Option<&BookName> {
        match self {
            Self::Named(book) | Self::Now(book) => Some(book),
            Self::Shelf => None,
        }
    }
}

impl fmt::Display for RecordBook {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Named(book) => write!(f, "the rule of book `{book}`, which the record names"),
            Self::Now(book) => write!(f, "the rule of book `{book}`, which its document is in now"),
            Self::Shelf => f.write_str("the rule of `[decide]`"),
        }
    }
}

/// A record the rules of who may decide do not admit: its name is in none of the
/// groups the rule names for its kind, at its time. There is no authentication: the
/// rule is about the name the record writes, as it is written.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[error("{}", said(self))]
pub struct NotAdmitted {
    /// The record.
    pub record: JudgementId,
    /// For a revocation, the record it is not admitted to withdraw.
    pub target: Option<JudgementId>,
    /// The name it writes.
    pub actor: Actor,
    /// Its kind.
    pub kind: DecideKind,
    /// The book it is read with.
    pub book: RecordBook,
    /// The groups whose members may write it.
    pub groups: NonEmptySet<GroupName>,
    /// From when the rule reads records, if not always.
    pub since: Option<RuleTime>,
}

/// The sentence of a [`NotAdmitted`], which names the record.
fn said(not: &NotAdmitted) -> String {
    match not.target {
        Some(target) => format!("{} (a revocation of {target}): {}", not.record, not.why()),
        None => format!("{}: {}", not.record, not.why()),
    }
}

impl NotAdmitted {
    /// Why, without the record: `` `ana` may not approve under the rule of book
    /// `requirement`, which the record names: only a member of `maintainers` at the
    /// time of the record may ``, for an answer about a record that was not made.
    pub fn why(&self) -> String {
        let since = self
            .since
            .map_or_else(String::new, |since| format!(", from {since}"));
        format!(
            "`{}` may not {} under {}{since}: only a member of {} at the time of the record may",
            self.actor,
            self.kind.deed(),
            self.book,
            listed(self.groups.iter(), Mark::Backtick, Join::Or)
        )
    }
}

/// A record the rules of who may decide admit, on the ledger it was accepted by: what
/// a run of writes takes ([`Ledger::accept`] is the one maker), so that no record the
/// rules refuse can be written.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AdmittedRecord(Record);

impl AdmittedRecord {
    /// The record.
    pub fn record(&self) -> &Record {
        &self.0
    }
}

impl Ledger {
    /// The record, admitted to be written after the records of this ledger: the one
    /// way to an [`AdmittedRecord`]. A record a rule refuses is [`NotAdmitted`] (the
    /// first record a revocation is refused for), and nothing is written for it; a
    /// run accepts each of its records on the ledger the ones before it left.
    // The refusal is the answer and is asked once per record; it is not a hot path.
    #[allow(clippy::result_large_err)]
    pub fn accept(&self, record: Record) -> Result<AdmittedRecord, NotAdmitted> {
        match &record {
            Record::Revocation(revocation) => {
                for target in revocation.targets.iter().copied() {
                    self.admit
                        .revocation(revocation, target, &target_book(&self.books, target))?;
                }
            }
            Record::Decision(_) | Record::SameMeaning(_) | Record::Moved(_) | Record::Exempt(_) => {
                self.admit
                    .admission(&record, None, &self.admit.book_of(&record))?;
            }
        }
        Ok(AdmittedRecord(record))
    }
}
