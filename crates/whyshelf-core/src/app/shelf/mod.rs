//! The use cases of a shelf: thin, because the judgements are the domain's.
//!
//! Every use case builds the same three things through the ports and asks the entry of
//! [`domain::shelf`](crate::domain::shelf) that the table of its module page names for
//! what it needs. Nothing here decides whether a document is valid, what a link
//! means, whether an approval holds or what a reference names; this layer reads, calls
//! and carries the answer, with the notices the model gave beside it.
//!
//! | use case | writes? | answer |
//! |---|---|---|
//! | [`books_check`](ShelfApp::books_check) | no | [`BooksCheck`]: the problems of the declaration, or the faults around the documents |
//! | [`map`](ShelfApp::map) | no | [`MapReport`]: the problems of the declaration, or its books and links (nothing else is read) |
//! | [`status`](ShelfApp::status) | no | [`StatusReport`]: every document's declared status and standing |
//! | [`status_filtered`](ShelfApp::status_filtered) | no | [`StatusReport`]: the documents that pass a filter by book, standing and field, with the filter as given; a filter that does not fit the declaration is [`ShelfError::Filter`] |
//! | [`get`](ShelfApp::get) | no | [`GetReport`]: one document, its standing, notices and root causes |
//! | [`trace`](ShelfApp::trace) | no | [`TraceReport`]: links up and down, each node with its standing |
//! | [`reach`](ShelfApp::reach) | no | [`ReachReport`]: the named documents and every document that depends on one of them, the artifacts all of them stand on with where each lives, the files of the project all of it is in, and what may depend on them that could not be read |
//! | [`impact`](ShelfApp::impact) | no | [`ImpactReport`]: the two ends of a change, read the one way the run names, and for every document whether its approval held, holds, and whether the change adds a cause that makes it not hold; with [`impact_within`](ShelfApp::impact_within), whether the change stays inside the reach of named documents before it, a verdict per document |
//! | [`unlinked`](ShelfApp::unlinked) | no | [`UnlinkedReport`]: the problems of the declaration, or the documents linked neither way, those no link reaches and those that hold none, those decided against apart |
//! | [`stale`](ShelfApp::stale) | no | [`StaleReport`]: the approvals that no longer hold, with where they start to fail and what can settle each origin, and whether a gate passes ([`StaleReport::gate`]) |
//! | [`units`](ShelfApp::units) | no | [`UnitsReport`]: the approvals that no longer hold (and, asked, the pending documents) divided into units of work that share nothing, each with its documents upstream first, the files its artifacts live in and the upstreams it waits on, and the artifacts whose place cannot be told |
//! | [`stats`](ShelfApp::stats) | no | [`StatsReport`]: how the work went, from the ledger: approvals made again, pending time, runs, and the pins that changed under approvals made again |
//! | [`show_approved`](ShelfApp::show_approved) | no | [`ApprovedReport`]: what the approval that stands saw, each item with its content or why it could not be read back |
//! | [`diff`](ShelfApp::diff) | no | [`DiffReport`]: each item of that approval, as approved and as it is now |
//! | [`judge`](ShelfApp::judge) | **yes** | [`JudgedAll`]: the records that were appended ([`Judged`]), with the addresses of what an approval stood on and the revision, and the approvals left out because they would only repeat a fresh one, each with why |
//! | [`approve_chosen`](ShelfApp::approve_chosen) | **yes** | [`ChosenApproved`]: the approvals made, upstream first and in one run, of every document the rules chosen take (expired or stale, pending, or both) that would hold once made, each with the rule that took it, and those left out, each with why |
//! | [`approve_stale`](ShelfApp::approve_stale) | **yes** | [`ChosenApproved`]: `approve_chosen` with the stale rule alone |
//! | [`same_meaning`](ShelfApp::same_meaning) | **yes** | [`Declared`]: the declaration that an edit keeps the meaning, with the earlier hashes it covers |
//! | [`moves`](ShelfApp::moves) | no | [`MovesReport`]: the names approvals that stand pinned something at that have nothing behind them now, each with the documents that pin it and the names its space's adapter suggests it moved to (by the same content, by a rename in the space's history, or by the end of the name), the spaces that cannot suggest, and the moves of a prefix that several gone names share, each tried as `judge moved --prefix` would take it: those that stand, those refused, and how many were not tried |
//! | [`moved`](ShelfApp::moved) | **yes** | [`Moved`](crate::domain::shelf::ledger::Moved): the declarations that an artifact moved from one name to another, after the space's adapter said the old name has nothing and the new one has something |
//! | [`moved_prefix`](ShelfApp::moved_prefix) | **yes** | [`MovedPrefix`]: the declarations that every name approvals pin under one prefix moved to the same name under another (one declaration per name, in one run), the names each space's adapter built, and the spaces whose adapter takes no prefix |
//! | [`cover`](ShelfApp::cover) | no | [`CoverReport`]: the problems of the declaration, or each covering link to artifacts with its selection, how complete the listing of the names it takes is, and each name reached, exempt or a gap |
//! | [`exempt`](ShelfApp::exempt) | **yes** | [`Exempted`]: the declarations that a document, or an artifact, is not expected to be reached by one covering link, each with the document's key |
//! | [`withdraw`](ShelfApp::withdraw) | **yes** | [`Record`](crate::domain::shelf::ledger::Record): the revocation that withdraws the records named by their identity |
//! | [`new_doc`](ShelfApp::new_doc) | **yes** | [`Created`]: the new document's identity, key and file, with the links of it that cannot be followed yet |
//! | [`new_docs`](ShelfApp::new_docs) | **yes** | [`NewMany`]: the documents of a run written, each as `new_doc` makes one, in the order they were written, and the ones no other stands on |
//! | [`init`](ShelfApp::init) | **yes** | [`Initialized`]: the file of the declaration it wrote, and the documents it made beside it, each as `new_doc` makes one |
//! | [`fmt_check`](ShelfApp::fmt_check) | no | [`FmtReport`]: the documents' files not in the one form, those in it, and those `fmt` would leave as they are, each with why, and the names it would drop from each file it would write (those asked in a [`DropAsk`](crate::domain::shelf::build::DropAsk)); names that do not fit the declaration are [`ShelfError::Drop`] |
//! | [`fmt`](ShelfApp::fmt) | **yes** | [`FmtReport`]: the files written over in the one form, those in it already, and those left as they are, each with why, and the names dropped from each file written; names that do not fit the declaration are [`ShelfError::Drop`], and nothing is written |
//! | [`rekey`](ShelfApp::rekey) | **yes** | [`RekeyReport`]: the document that took a new key, the documents whose links were rewritten, those that stayed with the old key and those that keep both, the documents whose body names the old key, whether each approval holds after the write, and the files written (all or none); a link that is not decided is [`ShelfError::Rekey`], every one listed, and nothing is written |
//!
//! # Reading and writing
//!
//! A use case that reads takes `&self` and is generic over [`ShelfRead`], which has no
//! way to write. [`judge`](ShelfApp::judge),
//! [`approve_chosen`](ShelfApp::approve_chosen), [`approve_stale`](ShelfApp::approve_stale),
//! [`same_meaning`](ShelfApp::same_meaning),
//! [`moved`](ShelfApp::moved), [`moved_prefix`](ShelfApp::moved_prefix), [`exempt`](ShelfApp::exempt), [`withdraw`](ShelfApp::withdraw), [`new_doc`](ShelfApp::new_doc),
//! [`new_docs`](ShelfApp::new_docs), [`init`](ShelfApp::init), [`fmt`](ShelfApp::fmt) and [`rekey`](ShelfApp::rekey) take `&mut self` and [`ShelfWrite`]: the writes of one
//! program run one at a time, by the type. Two processes can still write at once;
//! that is safe because a record is a file of its own named by its own identity, and
//! no write but `fmt`'s and `rekey`'s replaces or removes a file that is there (see the
//! adapter). Two
//! processes that make a new document of one book at once can be given the same next
//! key: the second finds the file there, and writes nothing. `fmt` compares a file
//! with what was read just before it replaces it, so an edit made after the reading
//! is left as it is; one saved in the short time between that comparison and the
//! replacement is overwritten, and a file removed in it is written again
//! ([`DocReplace::replace_read`]); `rekey` does the same for the files it writes over,
//! and for the file it removes (the file is compared with what was read just before, and
//! an edit saved in the short time between is lost with it, [`DocRemove::remove_read`]).
//!
//! # Every answer carries what the model noticed
//!
//! A [`Row`] has its [`Notice`](crate::domain::shelf::freshness::Notice)s (a
//! document it informs of that does not hold, a link that cannot be followed, a
//! pinned upstream whose approval lapsed, an edit declared to keep the meaning), and
//! every report has a [`Health`]: the
//! files that are no document, the cycles, the records that contradict one another
//! and the files of the ledger that are no record. None is a failure of the use case,
//! and none is left out.
//!
//! # Which end of a change is read
//!
//! Every port that reads the state of the project takes a [`Side`], and every space's
//! ask that reads does too: a use case reads one end of a change, [`Side::After`] for
//! every command here, reading the clock once and handing that time to the reading of
//! the side: the declaration and then the documents and the ledger, in one load, or,
//! for a use case that answers a declaration that is not sound or holds its ask
//! against the declaration first, the declaration and then the rest, at the same side
//! and time. The trees those readings are of are read through the ports of
//! [`state`], by one adapter for the run.

mod adapter;
mod approve_chosen;
mod approved;
mod attribution;
mod cover;
mod dispatch;
mod error;
mod exempt;
mod fmt;
mod init;
mod judge;
mod load;
mod moved;
mod moves;
mod new_doc;
pub mod ports;
mod prefix_trial;
mod read;
mod rekey;
mod report;
mod run_writes;
mod same_meaning;
pub mod state;
mod unlinked;
mod withdraw;

#[cfg(test)]
mod actor_tests;
#[cfg(test)]
mod approve_fresh_tests;
#[cfg(test)]
mod approve_pending_tests;
#[cfg(test)]
mod approve_stale_tests;
#[cfg(test)]
mod artifact_cover_tests;
#[cfg(test)]
mod artifact_tests;
#[cfg(test)]
mod comparison_tests;
#[cfg(test)]
mod cover_tests;
#[cfg(test)]
mod exempt_tests;
#[cfg(test)]
mod fake_tests;
#[cfg(test)]
mod fmt_tests;
#[cfg(test)]
mod init_tests;
#[cfg(test)]
mod ledger_run_tests;
#[cfg(test)]
mod moved_prefix_tests;
#[cfg(test)]
mod moved_tests;
#[cfg(test)]
mod moves_tests;
#[cfg(test)]
mod new_doc_tests;
#[cfg(test)]
mod new_many_tests;
#[cfg(test)]
mod prefix_trial_tests;
#[cfg(test)]
mod preset_tests;
#[cfg(test)]
mod reach_tests;
#[cfg(test)]
mod rekey_tests;
#[cfg(test)]
mod run_tests;
#[cfg(test)]
mod run_writes_tests;
#[cfg(test)]
mod same_meaning_tests;
#[cfg(test)]
mod side_tests;
#[cfg(test)]
mod space_tests;
#[cfg(test)]
mod stale_gate_tests;
#[cfg(test)]
mod status_filter_tests;
#[cfg(test)]
mod successor_tests;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod units_tests;
#[cfg(test)]
mod unlinked_tests;
#[cfg(test)]
mod within_tests;

pub use adapter::{
    Answers, AsyncVisit, CannotList, CannotPlace, CannotSuggest, ListWay, NameList, NamePlaces,
    NamePrefixes, NoPrefixes, NoReadBack, PlacesWay, PrefixesWay, ReadBackWay, SpaceAdapter,
    SpaceReadBack, SpaceSuggest, SuggestWay, Visit,
};
pub use approve_chosen::{
    ApproveChosenRequest, ApproveStaleRequest, Choose, ChosenApproved, ChosenJudged, ChosenLeftOut,
    PendingScope,
};
pub use attribution::AttributionAsk;
pub use cover::CoverLinkAsk;
pub use dispatch::{DispatchError, SpaceRegistry, Spaces};
pub use error::{ErrorClass, Obstacle, PersistProblem, ShelfError};
pub use exempt::{ExemptAsk, ExemptRequest, Exempted};
pub use fmt::{Dropped, FmtReport, LeftAlone, LeftAloneWhy};
pub use init::{InitFile, InitRequest, Initialized};
pub use judge::{JudgeRequest, Judgement};
pub use moved::{MovedPrefix, MovedRequest, PrefixMovedRequest};
pub use new_doc::{Created, EntryInput, ItemInput, NewDocRequest, NewMany};
pub use ports::{
    ActorSource, ArtifactSource, BookSchemaSource, Clock, ContentOf, ContentStore, Creation,
    DeclarationText, DeclarationWrite, DocRemove, DocReplace, DocSource, DocWrite, FileRead,
    FileSync, FileWrite, IdSource, LedgerAppend, LedgerContents, LedgerRead, MoveSource, Removal,
    Rendered, Replaceable, Replacement, RevisionSource, ShelfPort, ShelfRead, ShelfWrite,
    UnreadableRecord, UnreadableWhy,
};
pub use rekey::{RekeyReport, RekeyRequest, Rekeyed};
pub use report::{
    Absent, ApprovedItem, ApprovedReport, ArtifactCoverLists, Awaiting, BookCount, BooksCheck,
    CoverReport, DiffItem, DiffReport, DownstreamEdge, ExemptArtifact, ExemptEntry, Gate,
    GetReport, Health, ImpactReport, Judged, JudgedAll, LeftOut, LeftOutWhy, MapReport, MovedAway,
    MovesReport, Names, ReachReport, ReachedArtifact, ReachedDoc, RepeatedArtifact,
    RepeatedExemption, Row, SettledOrigin, StaleEntry, StaleReport, StatsReport, StatusReport,
    TraceReport, Uncovered, UncoveredArtifact, UnitsReport, Unknown, UnlinkedEntry, UnlinkedLists,
    UnlinkedReport, UnneededArtifact, UnneededExemption, UpstreamEdge, UpstreamEnd,
};
pub use run_writes::Edited;
pub use same_meaning::{Declared, DeclaredAbout, SameMeaningAbout, SameMeaningRequest};
pub use state::{
    AtSide, Beyond, ByteBound, DirListing, Entries, EntryKind, Leads, NameAnswers, NotADir,
    RawPart, ReadWay, Side, StateList, StateRead, TreeFiles, Unnamed,
};
pub use unlinked::{UnlinkedAsk, UnlinkedFilter};
pub use withdraw::WithdrawRequest;

/// The use cases of a shelf over a bundle of ports.
///
/// Generic over the ports, so a test runs the same code on an in-memory fake and the
/// command line runs it on real adapters.
#[derive(Debug)]
pub struct ShelfApp<P> {
    ports: P,
}

impl<P> ShelfApp<P> {
    /// The use cases over these ports.
    pub fn new(ports: P) -> Self {
        Self { ports }
    }

    /// The ports.
    pub fn ports(&self) -> &P {
        &self.ports
    }
}
