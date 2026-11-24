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

use std::collections::{BTreeMap, BTreeSet};

use crate::domain::actor::Actor;
use crate::domain::id::DocId;
use crate::domain::location::RelPath;
use crate::domain::nonempty::{NonBlank, NonEmptySet, NonEmptyVec, TwoOrMore};
use crate::domain::shelf::artifact::{Artifact, ArtifactKey, Untold};
use crate::domain::shelf::build::{BookTally, DocPresence, InvalidDoc, NowNamed, SkippedFile};
use crate::domain::shelf::coverage::{CoverAssessment, LinkCover};
use crate::domain::shelf::doc::{Doc, DocKey, KeyCollision, UnresolvedLink};
use crate::domain::shelf::exemption::Unneeded;
use crate::domain::shelf::filter::StatusFilterAsk;
use crate::domain::shelf::freshness::{
    Degradation, Holding, Notice, OriginGroup, RootCause, Severity, Standing, StandingKind,
    Successors,
};
use crate::domain::shelf::graph::Cycle;
use crate::domain::shelf::impact::ImpactEntry;
use crate::domain::shelf::ledger::{
    Decision, DeclaredStatus, Exempt, FramePin, FrameRefusal, JudgementId, LeaveOut, LedgerProblem,
    MoveSuggestions, NotAdmitted, PinName, Record, ShortSameMeaning,
};
use crate::domain::shelf::map::ShelfMap;
use crate::domain::shelf::order::HeldBack;
use crate::domain::shelf::placement::Placement;
use crate::domain::shelf::reach::Unwalked;
use crate::domain::shelf::schema::{
    Approvals, BookDir, BookName, CoverSeverity, EntryName, Propagation, SchemaProblem,
};
use crate::domain::shelf::settle::Settlement;
use crate::domain::shelf::snapshot::{ItemCompared, Retrieval, Snapshot, SnapshotItem};
use crate::domain::shelf::stats::Stats;
use crate::domain::shelf::units::Units;
use crate::domain::shelf::untold::UntoldName;
use crate::domain::shelf::within::Within;

use super::ports::UnreadableRecord;
use super::state::ReadWay;

/// The keys of the valid documents of the shelf, so that a surface can write a
/// document by its key and not only by its identity, and what each upstream a record
/// of the ledger pinned comes to now, so that it can name a pin by the key its
/// upstream has.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Names {
    pub(super) keys: BTreeMap<DocId, DocKey>,
    pub(super) pinned: BTreeMap<DocId, NowNamed>,
}

impl Names {
    /// The key of a valid document; none for one that is not valid or not there.
    pub fn key_of(&self, id: DocId) -> Option<&DocKey> {
        self.keys.get(&id)
    }

    /// How an answer names the upstream a pin stands on: what that upstream comes to
    /// now ([`Shelf::now_named`](crate::domain::shelf::build::Shelf::now_named)), read
    /// by [`FramePin::named`]. An upstream these names hold nothing of (names made
    /// for no ledger) is named as it was pinned.
    pub fn pin_name(&self, pin: &FramePin) -> PinName {
        match self.pinned.get(&pin.target()) {
            Some(now) => pin.named(now),
            None => PinName::Same(pin.key().clone()),
        }
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
    /// The files in its range that are not its documents, each with why, in the
    /// order they were read ([`Shelf::skipped`]): never counted as valid or invalid,
    /// and never left unsaid. Empty unless the book says
    /// `without_front_matter = "skip"`.
    ///
    /// [`Shelf::skipped`]: crate::domain::shelf::build::Shelf::skipped
    pub skipped: Vec<SkippedFile>,
}

/// The result of `check`.
// `Checked` holds a list of each kind inline (the enum is as large as that), and a
// `check` makes one answer: boxing the lists would add an allocation and a deref to
// each read of it and save nothing.
#[allow(clippy::large_enum_variant)]
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
        /// The names links write that a pin of an approval was moved away from since
        /// (the
        /// [`Cause::ArtifactMoved`](crate::domain::shelf::freshness::Cause::ArtifactMoved)
        /// causes of every approval): the approval is stale, and the link is to
        /// write the name the pin is read as. They leave the check clean too.
        moved: Vec<MovedAway>,
        /// The exemptions that stand and take the place of a gap (the
        /// [`Notice::Exempt`](crate::domain::shelf::freshness::Notice::Exempt)
        /// notices of every document), each with who made it, when and why. An
        /// exempt document is not in `uncovered`, and leaves the check clean
        /// whatever the strength of the link.
        exempt: Vec<ExemptEntry>,
        /// The exemptions that stand and are no longer needed (the
        /// [`Notice::ExemptionUnneeded`](crate::domain::shelf::freshness::Notice::ExemptionUnneeded)
        /// notices), each with why. They leave the check clean too.
        unneeded: Vec<UnneededExemption>,
        /// The (document, link)s two or more exemptions that stand are of (the
        /// [`Notice::ExemptionRepeated`](crate::domain::shelf::freshness::Notice::ExemptionRepeated)
        /// notices), as merged branches can leave them. They leave the check clean
        /// too.
        repeated: Vec<RepeatedExemption>,
        /// The approvals whose content fewer different names read than the rule of
        /// who may decide asks for, at any freshness (the
        /// [`Quorum::Short`](crate::domain::shelf::ledger::Quorum::Short) of every
        /// approval), by identity. They leave the check clean: another name is to
        /// approve them.
        awaiting: Vec<Awaiting>,
        /// The edits declared to keep the meaning by fewer different names than
        /// they need, which join nothing yet, that a pin that stale reads now meets
        /// ([`Ledger::same_meaning_short`](crate::domain::shelf::ledger::Ledger::same_meaning_short)).
        /// They leave the check clean too.
        same_meaning_short: Vec<ShortSameMeaning>,
        /// The keys that two or more files write ([`Shelf::collisions`]), one for each
        /// key, with every file and the identity of each that could be read. Each of
        /// the files is invalid (it is in `health`), so they leave the check as it
        /// was.
        ///
        /// [`Shelf::collisions`]: crate::domain::shelf::build::Shelf::collisions
        collisions: Vec<KeyCollision>,
        /// The assessment of coverage the lists above and below are made from, for
        /// documents and for names of artifacts alike: what
        /// [`is_clean`](BooksCheck::is_clean) reads, and how complete the listing of
        /// each covering link to artifacts is.
        cover: CoverAssessment,
        /// What the covering links to artifacts expect to reach and do not, and
        /// the exemptions of artifacts, each read as for documents.
        artifacts: ArtifactCoverLists,
        /// The keys of the valid documents, for the documents the entries name
        /// (such as those an unneeded exemption's link reaches it from).
        names: Names,
    },
}

/// A name of an artifact a covering link expects to reach and nobody does.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UncoveredArtifact {
    /// The book that declares the link.
    pub book: BookName,
    /// The link.
    pub link: EntryName,
    /// The space and the name.
    pub artifact: ArtifactKey,
    /// How strongly the link expects to reach it.
    pub severity: CoverSeverity,
}

/// An exemption of an artifact that stands and takes the place of a gap.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExemptArtifact {
    /// The space and the name.
    pub artifact: ArtifactKey,
    /// How strongly the link expects to reach the names of its selection.
    pub severity: CoverSeverity,
    /// The exemption, with the book and the link, who, when and why.
    pub record: Exempt,
}

/// An exemption of an artifact that stands and is no longer needed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnneededArtifact {
    /// The space and the name.
    pub artifact: ArtifactKey,
    /// The exemption, with the book and the link, who, when and why.
    pub record: Exempt,
    /// Why it is not needed.
    pub why: Unneeded,
}

/// Two or more exemptions that stand of one artifact from one link.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RepeatedArtifact {
    /// The space and the name.
    pub artifact: ArtifactKey,
    /// The book the exemptions name.
    pub book: BookName,
    /// The link they name.
    pub link: EntryName,
    /// The exemptions, in the order of identity.
    pub exemptions: TwoOrMore<JudgementId>,
}

/// What the covering links to artifacts and the exemptions of artifacts come to,
/// as the assessment of coverage read them: for `check` and for `cover`.
///
/// An exemption that cannot be told (its link's listing is not the whole) is in none
/// of the lists: the completeness of the link says it.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ArtifactCoverLists {
    /// The names a covering link expects and nobody reaches, of either strength,
    /// in the order of the links, then of the names.
    pub uncovered: Vec<UncoveredArtifact>,
    /// The exemptions that stand and take the place of a gap.
    pub exempt: Vec<ExemptArtifact>,
    /// The exemptions that stand and are no longer needed.
    pub unneeded: Vec<UnneededArtifact>,
    /// The (artifact, link)s two or more exemptions that stand are of.
    pub repeated: Vec<RepeatedArtifact>,
}

/// An approval whose content awaits the approvals of other names.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Awaiting {
    /// The document.
    pub doc: DocId,
    /// Its key, when it is valid.
    pub key: Option<DocKey>,
    /// Where it stands, as shown ([`Standing::kind`]): awaiting when it is fresh,
    /// else the level of its freshness.
    pub standing: StandingKind,
    /// The names whose reading counts now.
    pub readers: NonEmptySet<Actor>,
    /// How many different names the rule asks for.
    pub need: Approvals,
}

/// Two or more exemptions that stand of one document from one link.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RepeatedExemption {
    /// The document.
    pub doc: DocId,
    /// Its key, when it is valid.
    pub key: Option<DocKey>,
    /// The book the exemptions name.
    pub book: BookName,
    /// The link they name.
    pub link: EntryName,
    /// The exemptions, in the order of identity.
    pub exemptions: TwoOrMore<JudgementId>,
}

/// An exemption that stands and takes the place of a gap.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExemptEntry {
    /// The document.
    pub doc: DocId,
    /// Its key.
    pub key: Option<DocKey>,
    /// How strongly the link expects to reach the documents of its book.
    pub severity: CoverSeverity,
    /// The exemption, with the book and the link, who, when and why.
    pub record: Exempt,
}

/// An exemption that stands and is no longer needed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnneededExemption {
    /// The document.
    pub doc: DocId,
    /// Its key, when it is valid.
    pub key: Option<DocKey>,
    /// The exemption, with the book and the link, who, when and why.
    pub record: Exempt,
    /// Why it is not needed.
    pub why: Unneeded,
}

/// A name a link writes that an approval pinned, declared moved since.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MovedAway {
    /// The document.
    pub doc: DocId,
    /// Its key.
    pub key: Option<DocKey>,
    /// The link.
    pub link: EntryName,
    /// The space and the name the link writes, which the pin's way left (the name
    /// pinned, or one a chain of moves passed).
    pub artifact: ArtifactKey,
    /// The name the pin is read as now.
    pub moved_to: ArtifactKey,
    /// The declarations of the moves, in order.
    pub declared: NonEmptyVec<JudgementId>,
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

impl UntoldName for Unknown {
    fn doc(&self) -> DocId {
        self.doc
    }

    fn artifact(&self) -> &ArtifactKey {
        &self.artifact
    }

    fn why(&self) -> &Untold {
        &self.why
    }
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
    /// and nothing the covering links of strength [`Error`](CoverSeverity::Error)
    /// fail at. It reads the assessment of coverage only, not what the adapters
    /// listed:
    ///
    /// | an `error` cover | is clean when |
    /// |---|---|
    /// | of documents, or of names of artifacts | no document and no name is a gap of it |
    /// | of names of artifacts | the listing of the names its selection takes is the whole ([`Completeness::All`](crate::domain::shelf::artifact::Completeness::All)): a listing that is partial, cannot be made or was not asked proves no absence of a gap, so it fails |
    /// | of names of artifacts | its selection takes at least one name: one that takes none expects nothing, which is a mistake of the declaration more than a fulfilled one |
    ///
    /// A document or a name a link of strength [`Warn`](CoverSeverity::Warn) fails
    /// to reach is listed in `uncovered` (`artifacts.uncovered` for a name) with its
    /// strength and leaves the check clean, as `absent` and `unknown` do, and so do
    /// a `warn` cover that selects nothing or whose listing is not the whole. A
    /// document or a name exempt from a link is not a gap of it, whatever the
    /// strength, and an exemption no longer needed leaves the check clean too.
    pub fn is_clean(&self) -> bool {
        match self {
            Self::SchemaInvalid(_) => false,
            Self::Checked { health, cover, .. } => {
                health.is_clean()
                    && cover
                        .gaps()
                        .all(|gap| gap.severity() != CoverSeverity::Error)
                    && cover
                        .links()
                        .iter()
                        .filter(|link| link.severity() == CoverSeverity::Error)
                        .all(|link| link.completeness().is_all() && !link.selects_nothing())
            }
        }
    }
}

/// The result of `cover`: what the covering links to artifacts expect, and how each
/// stands.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CoverReport {
    /// The declaration of the books is not sound, as `check` answers it. Nothing was
    /// read through it.
    SchemaInvalid(NonEmptyVec<SchemaProblem>),
    /// The declaration is sound; the documents, the ledger and the listings were read.
    Covered {
        /// Each covering link to artifacts (the one asked for, when one was), in the
        /// order the declaration writes the books and their links, with its
        /// selection, how complete the listing is and, when it is the whole, each
        /// name with how it stands.
        links: Vec<LinkCover>,
        /// The gaps and the exemptions of those links.
        lists: ArtifactCoverLists,
        /// The keys of the valid documents.
        names: Names,
        /// What is wrong around them.
        health: Health,
    },
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
        /// those no decision went against.
        standing: UnlinkedLists,
        /// The documents decided against
        /// ([`NotStanding`](crate::domain::shelf::ledger::NotStanding)), apart, so
        /// that they do not crowd out the ones that stand.
        decided_against: UnlinkedLists,
        /// The keys of the valid documents.
        names: Names,
        /// What is wrong around them.
        health: Health,
    },
}

/// The result of `status`: the documents that pass the filter, each with what was
/// decided and where it stands.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StatusReport {
    /// Every document the shelf or the ledger knows that passes the filter, by
    /// identity: all of them when the filter is empty.
    pub rows: Vec<Row>,
    /// The filter as it was given, so that the rows are read with what they were
    /// chosen by.
    pub filters: StatusFilterAsk,
    /// The keys of the valid documents, of the whole project.
    pub names: Names,
    /// What is wrong around the documents, of the whole project and not of the rows.
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

/// One document a reach reached ([`ReachReport`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReachedDoc {
    /// The document as the answers show it.
    pub row: Row,
    /// Whether it was named; one not named depends on one that was, transitively.
    pub named: bool,
    /// The files it may be in: its file, the files of an invalid one, or every file
    /// whose identity cannot be read when it may be in any of them; none when it is
    /// gone.
    pub files: BTreeSet<RelPath>,
}

/// One artifact the documents a reach reached stand on ([`ReachReport`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReachedArtifact {
    /// Its space and name.
    pub artifact: ArtifactKey,
    /// The documents reached that stand on it: their `Depends` links to artifacts name
    /// it now, or the approval that stands about them pinned it.
    pub by: BTreeSet<DocId>,
    /// Where it lives in the files of the project, as the adapter of its space says.
    pub placement: Placement,
}

/// The result of `reach`: what depends on the named documents, what all of them stand
/// on, and the files of the project that holds it, read at the side after.
///
/// The rules are on the page of [`reach`](crate::domain::shelf::reach): the documents
/// reached are the named and every document whose `Depends` links name one reached;
/// the artifacts are those each of them stands on; the files are every file a document
/// reached may be in and every file of the placement of an artifact.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReachReport {
    /// Every document reached, by identity.
    pub docs: Vec<ReachedDoc>,
    /// Every artifact they stand on, by space and name.
    pub artifacts: Vec<ReachedArtifact>,
    /// The files: those of the documents and those of the placements of the
    /// artifacts, whole or in part.
    pub files: BTreeSet<RelPath>,
    /// The documents reached that are gone: no file carries them.
    pub gone: Vec<DocId>,
    /// What may depend on the named and could not be read, when anything may: the
    /// reach may be more than these documents.
    pub untold: Option<Unwalked>,
    /// The keys of the valid documents.
    pub names: Names,
    /// What is wrong around them.
    pub health: Health,
}

impl ReachReport {
    /// The artifacts whose place the adapter of their space cannot say, each with
    /// why.
    pub fn unplaced(&self) -> impl Iterator<Item = (&ArtifactKey, &NonBlank)> {
        self.artifacts
            .iter()
            .filter_map(|reached| match &reached.placement {
                Placement::Unplaced(why) => Some((&reached.artifact, why)),
                Placement::Only(_) | Placement::Partly(_) => None,
            })
    }
}

/// The result of `units`: what does not hold, divided into units of work that share
/// nothing, read at the side after. The rules are on the page of
/// [`units`](crate::domain::shelf::units).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnitsReport {
    /// The units, and the artifacts whose place cannot be told.
    pub units: Units,
    /// The keys of the valid documents.
    pub names: Names,
    /// What is wrong around them.
    pub health: Health,
}

/// The result of `impact`: the two ends of a change, read the one way the run names,
/// and for every document whether its approval held, holds, and whether the change
/// adds a cause that makes it not hold; and, asked of named documents, whether the
/// change stays inside their reach. The rules are on the pages of
/// [`impact`](crate::domain::shelf::impact) and
/// [`within`](crate::domain::shelf::within).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImpactReport {
    /// How the two ends were read, as the composition root names it.
    pub way: ReadWay,
    /// Every document either end lists, by identity.
    pub entries: Vec<ImpactEntry>,
    /// The keys of the valid documents before the change.
    pub names_before: Names,
    /// The keys of the valid documents after it.
    pub names_after: Names,
    /// What is wrong around them before the change.
    pub health_before: Health,
    /// What is wrong around them after it.
    pub health_after: Health,
    /// Whether the change stays inside the reach of the named documents before it,
    /// when documents were named ([`ShelfApp::impact_within`]); none otherwise.
    ///
    /// [`ShelfApp::impact_within`]: super::ShelfApp::impact_within
    pub within: Option<Within>,
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

/// An origin of stale approvals with what can settle it.
///
/// Why they come together: what can settle an origin is read once, in the domain
/// ([`settle_all`]), from the origin and from what the approving commands are
/// foreseen to approve, and a surface writes it in its own words; the application
/// only puts the two side by side.
///
/// [`settle_all`]: crate::domain::shelf::settle::settle_all
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SettledOrigin {
    /// Where the degradations start, and what they degrade.
    pub group: OriginGroup,
    /// What can settle it, in the order a person would try it.
    pub next: Vec<Settlement>,
}

/// The result of `stale`: the approvals that no longer hold.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StaleReport {
    /// The documents, the strongest degradation first.
    pub entries: Vec<StaleEntry>,
    /// Where their degradations start ([`Assessment::origins`]), each with what can
    /// settle it.
    ///
    /// [`Assessment::origins`]: crate::domain::shelf::freshness::Assessment::origins
    pub origins: Vec<SettledOrigin>,
    /// The names that approved documents stand on through a `depends` link and
    /// that their adapter cannot tell about now: their freshness is unchanged, and
    /// whether their approvals hold cannot be said.
    pub unknown: Vec<Unknown>,
    /// The fold of every document's holding ([`Gate::fold`] of each
    /// [`Gate::from`]), made as the use case asks each document: what
    /// [`StaleReport::gate`] answers.
    pub held: Gate,
    /// The keys of the valid documents.
    pub names: Names,
    /// What is wrong around them.
    pub health: Health,
}

impl StaleReport {
    /// Whether a gate can say the approvals hold, as DEC-26 decides: the one place
    /// the rule is read, so that a surface maps the answer to its own signal (the
    /// command line to an exit status) instead of reading the fields again.
    ///
    /// An approval that does not hold, or that only expired, is
    /// [`Gate::DoesNotHold`]; with none, one that pins a name its adapter cannot
    /// tell about now is [`Gate::CannotTell`], since whether it holds cannot be
    /// said while its freshness is unchanged; otherwise [`Gate::Holds`]. A document
    /// still pending has no approval and is no reason to fail, and neither is a
    /// fresh approval still awaiting names (DEC-89): another name settles it.
    ///
    /// It is the fold of every document's [`Holding`] ([`Gate::fold`] of
    /// [`Gate::from`] each), which the use case made as it asked each document.
    pub fn gate(&self) -> Gate {
        self.held
    }
}

impl From<&Holding> for Gate {
    /// What one document's holding says to a gate: the one mapping. A document with
    /// no approval (none there, pending, decided against) is no reason to fail.
    fn from(holding: &Holding) -> Self {
        match holding {
            Holding::DoesNotHold { .. } => Self::DoesNotHold,
            Holding::CannotTell { .. } => Self::CannotTell,
            Holding::NotThere | Holding::Pending | Holding::Against(_) | Holding::Holds(_) => {
                Self::Holds
            }
        }
    }
}

/// What a gate can say about the approvals of a shelf ([`StaleReport::gate`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Gate {
    /// No approval is degraded, and none pins a name that cannot be told about
    /// now; pending documents and fresh approvals still awaiting names pass too.
    Holds,
    /// An approval does not hold, or only expired.
    DoesNotHold,
    /// No approval is known not to hold, but one pins a name that cannot be told
    /// about now.
    CannotTell,
}

impl Gate {
    /// The gate of many: the one place they are folded (DEC-26). One that does not
    /// hold makes it [`DoesNotHold`](Self::DoesNotHold); else one that cannot be told
    /// makes it [`CannotTell`](Self::CannotTell); else it [`Holds`](Self::Holds),
    /// none at all too.
    pub fn fold(gates: impl IntoIterator<Item = Gate>) -> Gate {
        gates
            .into_iter()
            .fold(Self::Holds, |folded, gate| match (folded, gate) {
                (Self::DoesNotHold, _) | (_, Self::DoesNotHold) => Self::DoesNotHold,
                (Self::CannotTell, _) | (_, Self::CannotTell) => Self::CannotTell,
                (Self::Holds, Self::Holds) => Self::Holds,
            })
    }

    /// Whether the gate passes: only when it [`Holds`](Self::Holds).
    pub fn passes(self) -> bool {
        matches!(self, Self::Holds)
    }
}

/// The result of `stats`: how the work went, as the ledger tells it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StatsReport {
    /// The measures ([`stats::of`](crate::domain::shelf::stats::of)).
    pub stats: Stats,
    /// The keys of the valid documents, for the documents `health` names.
    pub names: Names,
    /// What is wrong around the documents.
    pub health: Health,
}

/// The result of `moves`: the names approvals pinned something at that have nothing
/// behind them now, each with where its space's adapter suggests it moved.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MovesReport {
    /// The gone names with their candidates, and the spaces that cannot suggest
    /// ([`GoneNames::suggest`](crate::domain::shelf::ledger::GoneNames::suggest)).
    pub suggestions: MoveSuggestions,
    /// The keys of the valid documents, for the documents that pin a gone name and
    /// those `health` names.
    pub names: Names,
    /// What is wrong around the documents.
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
    /// The item as it was approved, beside what it is now: its hash now, whether it
    /// changed, and for a document the file that holds it now
    /// ([`SnapshotItem::compared`]).
    pub compared: ItemCompared,
    /// The content as it was approved, or why it could not be read back.
    pub approved: Retrieval,
    /// The bytes of the document's file, or of the artifact, now; none when nothing
    /// is there.
    pub current: Option<Vec<u8>>,
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

/// What a run of `judge` wrote, and the approvals it left out.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JudgedAll {
    /// The records written, in the order the documents were named.
    pub judged: Vec<Judged>,
    /// The approvals not made, in the order the documents were named: each would
    /// have repeated the reading of the one who approves ([`LeftOutWhy::Repeats`]).
    /// Only an approval is left out; every other verb is judged or refused.
    pub left_out: Vec<LeftOut>,
}

/// Why an approval asked for was not made.
// A `Degradation` holds every cause inline (the enum is as large as that), and a run
// leaves out few approvals: boxing it would add an allocation and a deref to each read
// of it, to save copying a few words. (A link that cannot be followed shrank when its
// collision of keys was boxed, which narrowed the other variants and so the lint.)
#[allow(clippy::large_enum_variant)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LeftOutWhy {
    /// Of `judge approve --stale`: its approval is broken now.
    Broken(Degradation),
    /// Of `judge approve --stale` or `--pending`: the model refuses its frame now (an
    /// upstream decided against, an artifact that cannot be told, a link that cannot
    /// be followed, a cycle it would close).
    Refused(FrameRefusal),
    /// Of `judge approve --stale` or `--pending`: a `Depends` link names an upstream
    /// that is broken, pending or stale and not in the run, or left out.
    StandsOn {
        /// The link, the upstream and why.
        held: HeldBack,
        /// The upstream's key, when it is valid.
        key: Option<DocKey>,
    },
    /// Of `judge approve`, and of `--stale` for one stale only through what an
    /// earlier approval of the run approved again: an approval made now would be a
    /// [`Repeat`], by one who already read the content that stands and with the
    /// expiry of that reading, so it would add nothing. That one may be fresh or not
    /// (stale over a pending upstream, say): it is the repeat that is told, not a
    /// freshness. It stands, and holds back nothing that stands on it.
    ///
    /// [`Repeat`]: crate::domain::shelf::ledger::ApprovalKind::Repeat
    Repeats {
        /// The approval that stands.
        approval: JudgementId,
    },
    /// Of `judge approve --stale`: an approval made now would be a
    /// [`SecondReading`], of the content that stands by one who has not read it,
    /// which a run that chose the document by a rule does not write; it is said by
    /// naming the document. It stands, and holds back nothing that stands on it.
    ///
    /// [`SecondReading`]: crate::domain::shelf::ledger::ApprovalKind::SecondReading
    SecondReading {
        /// The approval that stands.
        approval: JudgementId,
    },
    /// Of `judge approve --stale` or `--pending`: the rules of who may decide do not
    /// admit the actor to approve in the document's book. It is not approved, and
    /// holds back what stands on it; `judge approve <ref>` refuses such an approval
    /// instead ([`ShelfError::NotAdmitted`](super::ShelfError::NotAdmitted)).
    NotAdmitted(NotAdmitted),
}

impl From<LeaveOut> for LeftOutWhy {
    fn from(leave: LeaveOut) -> Self {
        match leave {
            LeaveOut::Repeats { approval } => Self::Repeats { approval },
            LeaveOut::SecondReading { approval } => Self::SecondReading { approval },
        }
    }
}

/// An approval asked for that was not made.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LeftOut {
    /// The document.
    pub doc: DocId,
    /// Its key, when it is valid.
    pub key: Option<DocKey>,
    /// Why it was left out.
    pub why: LeftOutWhy,
}
