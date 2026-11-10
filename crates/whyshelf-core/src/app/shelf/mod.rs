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
//! | [`status`](ShelfApp::status) | no | [`StatusReport`]: every document's declared status and standing |
//! | [`get`](ShelfApp::get) | no | [`GetReport`]: one document, its standing, notices and root causes |
//! | [`trace`](ShelfApp::trace) | no | [`TraceReport`]: links up and down, each node with its standing |
//! | [`stale`](ShelfApp::stale) | no | [`StaleReport`]: the approvals that no longer hold, with where they start to fail |
//! | [`show_approved`](ShelfApp::show_approved) | no | [`ApprovedReport`]: what the approval that stands saw, each item with its content or why it could not be read back |
//! | [`diff`](ShelfApp::diff) | no | [`DiffReport`]: each item of that approval, as approved and as it is now |
//! | [`judge`](ShelfApp::judge) | **yes** | [`Judged`]: the record that was appended, with the addresses of what an approval stood on and the revision |
//! | [`same_meaning`](ShelfApp::same_meaning) | **yes** | [`Declared`]: the declaration that an edit keeps the meaning, with the earlier hashes it covers |
//! | [`withdraw`](ShelfApp::withdraw) | **yes** | [`Record`](crate::domain::shelf::ledger::Record): the revocation that withdraws the records named by their identity |
//! | [`new_doc`](ShelfApp::new_doc) | **yes** | [`Created`]: the new document's identity, key and file, with the links of it that cannot be followed yet |
//!
//! # Reading and writing
//!
//! A use case that reads takes `&self` and is generic over [`ShelfRead`], which has no
//! way to write. [`judge`](ShelfApp::judge), [`same_meaning`](ShelfApp::same_meaning),
//! [`withdraw`](ShelfApp::withdraw) and [`new_doc`](ShelfApp::new_doc) take
//! `&mut self` and [`ShelfWrite`]: the writes of one program run one at a time, by the
//! type. Two processes can still write at once; that is safe because a record is a
//! file of its own named by its own identity, and neither write replaces a file that
//! is there (see the adapter). Two processes that make a new document of one book at
//! once can be given the same next key: the second finds the file there, and writes
//! nothing.
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

mod approved;
mod error;
mod judge;
mod load;
mod new_doc;
pub mod ports;
mod read;
mod report;
mod same_meaning;
mod withdraw;

#[cfg(test)]
mod actor_tests;
#[cfg(test)]
mod artifact_tests;
#[cfg(test)]
mod comparison_tests;
#[cfg(test)]
mod cover_tests;
#[cfg(test)]
mod fake_tests;
#[cfg(test)]
mod new_doc_tests;
#[cfg(test)]
mod same_meaning_tests;
#[cfg(test)]
mod successor_tests;
#[cfg(test)]
mod tests;

pub use error::{ErrorClass, ShelfError};
pub use judge::{JudgeRequest, Judgement};
pub use new_doc::{Created, NewDocRequest};
pub use ports::{
    ActorSource, ArtifactSource, BookSchemaSource, Clock, ContentStore, Creation, DocSource,
    DocWrite, IdSource, LedgerAppend, LedgerContents, LedgerRead, RevisionSource, ShelfPort,
    ShelfRead, ShelfWrite, UnreadableRecord, UnreadableWhy,
};
pub use report::{
    ApprovedItem, ApprovedReport, BookCount, BooksCheck, DiffItem, DiffReport, DownstreamEdge,
    GetReport, Health, Judged, Names, Row, StaleEntry, StaleReport, StatusReport, TraceReport,
    Uncovered, UpstreamEdge, UpstreamEnd,
};
pub use same_meaning::{Declared, DeclaredAbout, SameMeaningAbout, SameMeaningRequest};
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
