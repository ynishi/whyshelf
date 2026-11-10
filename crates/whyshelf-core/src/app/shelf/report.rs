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
use crate::domain::nonempty::NonEmptyVec;
use crate::domain::shelf::artifact::{Artifact, ArtifactKey, Untold};
use crate::domain::shelf::build::{BookTally, DocPresence, InvalidDoc};
use crate::domain::shelf::doc::{Doc, DocKey, UnresolvedLink};
use crate::domain::shelf::freshness::{
    Notice, OriginGroup, RootCause, Severity, Standing, Successors,
};
use crate::domain::shelf::graph::Cycle;
use crate::domain::shelf::ledger::{Decision, DeclaredStatus, LedgerProblem, Record};
use crate::domain::shelf::map::ShelfMap;
use crate::domain::shelf::schema::{
    BookDir, BookName, CoverSeverity, EntryName, Propagation, SchemaProblem,
};
use crate::domain::shelf::snapshot::{ItemNow, Retrieval, Snapshot, SnapshotItem};

use crate::domain::location::RelPath;

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
    /// Every file that could not be made a valid document, with why. Those whose
    /// front matter or identity could not be read follow the order the books'
    /// directories are read in, which is the order the declaration writes the
    /// books.
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

/// A document as the answers show it: what it is, what was decided about it, where
/// it stands, and what is worth knowing that changes nothing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row {
    /// The identity.
    pub id: DocId,
    /// What the shelf has for it ([`DocState::presence`]).
    ///
    /// [`DocState::presence`]: crate::domain::shelf::build::DocState::presence
    pub presence: DocPresence,
    /// What the ledger says people decided.
    pub declared: DeclaredStatus,
    /// The decision that stands about it ([`Ledger::standing_decision`]), the one
    /// `declared` is read from, with who made it, when and why; none while it is
    /// pending.
    ///
    /// [`Ledger::standing_decision`]: crate::domain::shelf::ledger::Ledger::standing_decision
    pub decision: Option<Decision>,
    /// Where it stands now, with the causes when an approval does not hold.
    pub standing: Standing,
    /// What is worth knowing that changes no freshness.
    pub notices: Vec<Notice>,
}

/// One book of a checked shelf, counted ([`Shelf::tally_by_book`]).
///
/// [`Shelf::tally_by_book`]: crate::domain::shelf::build::Shelf::tally_by_book
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BookCount {
    /// The book.
    pub name: BookName,
    /// Its directory.
    pub dir: BookDir,
    /// What it holds.
    pub tally: BookTally,
}

/// The result of `check`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BooksCheck {
    /// The declaration of the books is not sound. Nothing was read through it.
    SchemaInvalid(NonEmptyVec<SchemaProblem>),
    /// The declaration is sound; the documents and the ledger were read.
    Checked {
        /// Each book, counted, in the order the declaration writes them.
        books: Vec<BookCount>,
        /// What is wrong around them.
        health: Health,
        /// The documents a covering link is expected to reach and does not (the
        /// [`Notice::NotCovered`](crate::domain::shelf::freshness::Notice::NotCovered)
        /// notices of every document), of either strength;
        /// [`is_clean`](BooksCheck::is_clean) says which of them fail the check.
        uncovered: Vec<Uncovered>,
        /// The names of pending and approved documents with nothing there (the
        /// [`Notice::ArtifactAbsent`](crate::domain::shelf::freshness::Notice::ArtifactAbsent)
        /// notices of every document). They leave the check clean: a name may be of
        /// evidence not written yet.
        absent: Vec<Absent>,
        /// The names of pending and approved documents their adapter cannot tell
        /// about now (the
        /// [`Notice::ArtifactUnknown`](crate::domain::shelf::freshness::Notice::ArtifactUnknown)
        /// notices of every document). They leave the check clean too.
        unknown: Vec<Unknown>,
    },
}

/// A name a link to artifacts writes that has nothing there.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Absent {
    /// The document.
    pub doc: DocId,
    /// Its key.
    pub key: Option<DocKey>,
    /// The link.
    pub link: EntryName,
    /// The space and the name as written.
    pub artifact: ArtifactKey,
}

/// A name a link to artifacts writes that its adapter cannot tell about now.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Unknown {
    /// The document.
    pub doc: DocId,
    /// Its key.
    pub key: Option<DocKey>,
    /// The link.
    pub link: EntryName,
    /// The space and the name as written.
    pub artifact: ArtifactKey,
    /// Why it cannot be told.
    pub why: Untold,
}

/// A document a covering link is expected to reach and does not.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Uncovered {
    /// The document.
    pub doc: DocId,
    /// Its key.
    pub key: Option<DocKey>,
    /// The book whose link is expected to reach it.
    pub book: BookName,
    /// The link.
    pub link: EntryName,
    /// How strongly the link expects to reach it.
    pub severity: CoverSeverity,
}

impl BooksCheck {
    /// Whether the check found nothing wrong: a sound declaration, a clean health,
    /// and no document a covering link of strength
    /// [`Error`](CoverSeverity::Error) fails to reach. A document a link of
    /// strength [`Warn`](CoverSeverity::Warn) fails to reach is listed in
    /// `uncovered` with its strength and leaves the check clean, as `absent` and
    /// `unknown` do.
    pub fn is_clean(&self) -> bool {
        match self {
            Self::SchemaInvalid(_) => false,
            Self::Checked {
                health, uncovered, ..
            } => {
                health.is_clean()
                    && uncovered
                        .iter()
                        .all(|gap| gap.severity != CoverSeverity::Error)
            }
        }
    }
}

/// The result of `map`: what the declaration says, or why it cannot be used.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MapReport {
    /// The declaration of the books is not sound, as `check` answers it.
    SchemaInvalid(NonEmptyVec<SchemaProblem>),
    /// The declaration is sound: its books and links, in its order.
    Mapped(ShelfMap),
}

/// A document `unlinked` lists: the book it is in, and the document as the answers
/// show it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnlinkedEntry {
    /// The book.
    pub book: BookName,
    /// The document.
    pub row: Row,
}

/// The three disjoint lists of `unlinked`, each in the order the declaration writes
/// the books, then by identity. Which document goes in which is the rule of
/// [`Linkage`](crate::domain::shelf::trace::Linkage).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct UnlinkedLists {
    /// Linked neither way ([`Linkage::isolated`]); always answered.
    ///
    /// [`Linkage::isolated`]: crate::domain::shelf::trace::Linkage::isolated
    pub isolated: Vec<UnlinkedEntry>,
    /// Reached by no link ([`Linkage::unreached`]); none when not asked.
    ///
    /// [`Linkage::unreached`]: crate::domain::shelf::trace::Linkage::unreached
    pub no_incoming: Option<Vec<UnlinkedEntry>>,
    /// Holding no link ([`Linkage::holds_none`]); none when not asked.
    ///
    /// [`Linkage::holds_none`]: crate::domain::shelf::trace::Linkage::holds_none
    pub no_outgoing: Option<Vec<UnlinkedEntry>>,
}

/// The result of `unlinked`: the documents that are not linked, each way.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UnlinkedReport {
    /// The declaration of the books is not sound, as `check` answers it. Nothing was
    /// read through it.
    SchemaInvalid {
        /// Its problems.
        problems: NonEmptyVec<SchemaProblem>,
        /// The lists asked for, empty, for the documents that stand and those
        /// decided against alike.
        lists: UnlinkedLists,
    },
    /// The declaration is sound; the documents and the ledger were read.
    Answered {
        /// The documents that [stand](crate::domain::shelf::freshness::Standing::stands):
        /// pending or approved.
        standing: UnlinkedLists,
        /// The documents decided against (denied, deprecated, retired), apart, so
        /// that they do not crowd out the ones that stand.
        decided_against: UnlinkedLists,
        /// The keys of the valid documents.
        names: Names,
        /// What is wrong around them.
        health: Health,
    },
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
    /// An artifact, there, not there, or not told now.
    Artifact(Artifact),
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
    /// Where their degradations start ([`Assessment::origins`]).
    ///
    /// [`Assessment::origins`]: crate::domain::shelf::freshness::Assessment::origins
    pub origins: Vec<OriginGroup>,
    /// The keys of the valid documents.
    pub names: Names,
    /// What is wrong around them.
    pub health: Health,
}

/// One item of what an approval saw, with what reading its content back came to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApprovedItem {
    /// The item: what it is in the approval, its hash and its address.
    pub item: SnapshotItem,
    /// The content as it was approved, or why it could not be read back.
    pub retrieval: Retrieval,
}

/// The result of `get --approved`: what the approval that stands saw.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApprovedReport {
    /// The document as the answers show it now.
    pub row: Row,
    /// What the approval saw.
    pub snapshot: Snapshot,
    /// Each item of it, in the order of the snapshot, with its content.
    pub items: Vec<ApprovedItem>,
    /// The keys of the valid documents.
    pub names: Names,
    /// What is wrong around them.
    pub health: Health,
}

/// One item of an approval set beside what it is now: values, not a text (the text
/// of a difference is the surface's work).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiffItem {
    /// The item as it was approved.
    pub item: SnapshotItem,
    /// The content as it was approved, or why it could not be read back.
    pub approved: Retrieval,
    /// What it is now, by hash.
    pub now: ItemNow,
    /// For a document, the file that holds it now, if one does; none for an
    /// artifact (its space and name are in the item).
    pub file: Option<RelPath>,
    /// The bytes of the document's file, or of the artifact, now; none when nothing
    /// is there.
    pub current: Option<Vec<u8>>,
    /// Whether it is not what was approved ([`SnapshotItem::changed_from`]).
    pub changed: bool,
}

/// The result of `diff`: each item of the approval that stands, then and now.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiffReport {
    /// The document as the answers show it now.
    pub row: Row,
    /// What the approval saw.
    pub snapshot: Snapshot,
    /// Each item, then and now, in the order of the snapshot.
    pub items: Vec<DiffItem>,
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
    /// For an approval, the names of the document's links to artifacts with nothing
    /// there ([`Doc::absent_artifacts`]); a `depends` link pins each as absent.
    /// Empty otherwise.
    pub absent: Vec<(EntryName, ArtifactKey)>,
}
