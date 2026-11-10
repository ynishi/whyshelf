//! The answers of the use cases, as values of the domain.
//!
//! A report holds domain types and no text: how a key or a cause is written, and in
//! what format, is the surface's business. Every report that speaks of a document
//! carries what the model said about it that changes no freshness (its
//! [`Notice`]s), and every report carries a [`Health`], the faults around the
//! documents that no one document owns: files that could not be read as documents,
//! cycles, records of the ledger that contradict one another or could not be read. A
//! decision that cannot be read is a decision the answer does not know of, so no
//! answer leaves it out.

use std::collections::BTreeMap;

use crate::domain::id::DocId;
use crate::domain::location::RelPath;
use crate::domain::nonempty::NonEmptyVec;
use crate::domain::shelf::build::{Cycle, DocProblem, InvalidDoc};
use crate::domain::shelf::doc::{Doc, DocHash, DocKey, UnresolvedLink};
use crate::domain::shelf::freshness::{Notice, RootCause, Severity, Standing, Successors};
use crate::domain::shelf::ledger::{DeclaredStatus, LedgerProblem, Record};
use crate::domain::shelf::schema::{BookDir, BookName, EntryName, Propagation, SchemaProblem};

use super::ports::UnreadableRecord;

/// The keys of the valid documents of the shelf, so that a surface can write a
/// document by its key and not only by its identity.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Names(pub(super) BTreeMap<DocId, DocKey>);

impl Names {
    /// The key of a valid document; none for one that is not valid or not there.
    pub fn key_of(&self, id: DocId) -> Option<&DocKey> {
        self.0.get(&id)
    }
}

/// What is wrong around the documents, as the shelf and the ledger set it aside.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Health {
    /// Every file that could not be made a valid document, with why.
    pub invalid: Vec<InvalidDoc>,
    /// The cycles among the links of the documents and among the frames of the
    /// approvals.
    pub cycles: Vec<Cycle>,
    /// The records of the ledger that contradict one another, set aside.
    pub ledger_problems: Vec<LedgerProblem>,
    /// The files of the ledger that are not a record.
    pub unreadable_records: Vec<UnreadableRecord>,
}

impl Health {
    /// Whether there is nothing wrong.
    pub fn is_clean(&self) -> bool {
        self.invalid.is_empty()
            && self.cycles.is_empty()
            && self.ledger_problems.is_empty()
            && self.unreadable_records.is_empty()
    }
}

/// What the shelf has for an identity now.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Presence {
    /// A valid document.
    Valid {
        /// Its key.
        key: DocKey,
        /// The file.
        origin: RelPath,
        /// Its hash now.
        hash: DocHash,
    },
    /// One or more files carry the identity, and they are invalid.
    Invalid {
        /// The files.
        files: Vec<RelPath>,
        /// Every problem of every file.
        problems: NonEmptyVec<DocProblem>,
    },
    /// No file carries it.
    Gone,
    /// No file is known to carry it, but some file's identity could not be read.
    Unknown {
        /// The files whose identity could not be read.
        unreadable: NonEmptyVec<RelPath>,
    },
}

/// A document as the answers show it: what it is, what was decided about it, where
/// it stands, and what is worth knowing that changes nothing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row {
    /// The identity.
    pub id: DocId,
    /// What the shelf has for it.
    pub presence: Presence,
    /// What the ledger says people decided.
    pub declared: DeclaredStatus,
    /// Where it stands now, with the causes when an approval does not hold.
    pub standing: Standing,
    /// What is worth knowing that changes no freshness.
    pub notices: Vec<Notice>,
}

/// One book of a checked shelf, counted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BookCount {
    /// The book.
    pub name: BookName,
    /// Its directory.
    pub dir: BookDir,
    /// How many valid documents it holds.
    pub valid: usize,
    /// How many files under it are not valid documents.
    pub invalid: usize,
}

/// The result of `books check`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BooksCheck {
    /// The declaration of the books is not sound. Nothing was read through it.
    SchemaInvalid(NonEmptyVec<SchemaProblem>),
    /// The declaration is sound; the documents and the ledger were read.
    Checked {
        /// Each book, counted.
        books: Vec<BookCount>,
        /// What is wrong around them.
        health: Health,
    },
}

impl BooksCheck {
    /// Whether the check found nothing.
    pub fn is_clean(&self) -> bool {
        match self {
            Self::SchemaInvalid(_) => false,
            Self::Checked { health, .. } => health.is_clean(),
        }
    }
}

/// The result of `status`: every document with what was decided and where it stands.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StatusReport {
    /// Every document the shelf or the ledger knows, by identity.
    pub rows: Vec<Row>,
    /// The keys of the valid documents.
    pub names: Names,
    /// What is wrong around them.
    pub health: Health,
}

/// The result of `get`: one document in full.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GetReport {
    /// The document as the answers show it.
    pub row: Row,
    /// The document itself, when it is valid.
    pub doc: Option<Doc>,
    /// Where the degradation of an approval starts.
    pub roots: Vec<RootCause>,
    /// The keys of the valid documents.
    pub names: Names,
    /// What is wrong around them.
    pub health: Health,
}

/// What a link of a document, followed upstream, comes to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UpstreamEnd {
    /// A document it names.
    Doc(Box<Row>),
    /// A key that cannot be followed.
    Unresolved(UnresolvedLink),
}

/// One link followed upstream: `from` holds `link`, and it comes to `to`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UpstreamEdge {
    /// The document that holds the link.
    pub from: DocId,
    /// The link.
    pub link: EntryName,
    /// What the link means for freshness.
    pub propagation: Propagation,
    /// How many links from the document asked about (1 is a link of its own).
    pub depth: usize,
    /// What it comes to.
    pub to: UpstreamEnd,
}

/// One link followed downstream: `holder` has a link `link` to `to`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DownstreamEdge {
    /// The document the link names.
    pub to: DocId,
    /// The link.
    pub link: EntryName,
    /// What the link means for freshness.
    pub propagation: Propagation,
    /// How many links from the document asked about (1 is a link to it).
    pub depth: usize,
    /// The document that holds the link.
    pub holder: Box<Row>,
}

/// The result of `trace`: what a document is linked to, up and down.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TraceReport {
    /// The document asked about.
    pub root: Row,
    /// What it links to, and what those link to, each link once.
    pub upstream: Vec<UpstreamEdge>,
    /// What links to it, and what links to those, each link once.
    pub downstream: Vec<DownstreamEdge>,
    /// The keys of the valid documents.
    pub names: Names,
    /// What is wrong around them.
    pub health: Health,
}

/// An approval that does not hold, with where it starts to fail.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StaleEntry {
    /// The document, with all its causes in its standing.
    pub row: Row,
    /// How strongly its approval is degraded.
    pub level: Severity,
    /// Where the degradation starts.
    pub roots: Vec<RootCause>,
}

/// The result of `stale`: the approvals that no longer hold.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StaleReport {
    /// The documents, the strongest degradation first.
    pub entries: Vec<StaleEntry>,
    /// The keys of the valid documents.
    pub names: Names,
    /// What is wrong around them.
    pub health: Health,
}

/// What `judge` wrote.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Judged {
    /// The record that was appended.
    pub record: Record,
    /// The document it is about; for a revocation, the document whose standing
    /// decision was withdrawn.
    pub subject: DocId,
    /// The key of the document, when it is valid.
    pub key: Option<DocKey>,
    /// For a deprecation, the documents that take the place of the one deprecated.
    pub successors: Option<Successors>,
}
