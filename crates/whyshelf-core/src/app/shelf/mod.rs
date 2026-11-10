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
//! | [`judge`](ShelfApp::judge) | **yes** | [`Judged`]: the record that was appended |
//!
//! # Reading and writing
//!
//! A use case that reads takes `&self` and is generic over [`ShelfRead`], which has no
//! way to write. [`judge`](ShelfApp::judge) takes `&mut self` and [`ShelfWrite`]: the
//! writes of one program run one at a time, by the type. Two processes can still
//! write at once; that is safe because a record is a file of its own named by its own
//! identity and a write never replaces a file that is there (see the adapter).
//!
//! # Every answer carries what the model noticed
//!
//! A [`Row`] has its [`Notice`](crate::domain::shelf::freshness::Notice)s (a
//! document it informs of that does not hold, a link that cannot be followed, a
//! pinned upstream whose approval lapsed), and every report has a [`Health`]: the
//! files that are no document, the cycles, the records that contradict one another
//! and the files of the ledger that are no record. None is a failure of the use case,
//! and none is left out.

mod error;
mod judge;
mod load;
pub mod ports;
mod read;
mod report;

#[cfg(test)]
mod fake_tests;
#[cfg(test)]
mod tests;

pub use error::{ErrorClass, ShelfError};
pub use judge::{JudgeRequest, Judgement};
pub use ports::{
    ActorSource, BookSchemaSource, Clock, DocSource, IdSource, LedgerAppend, LedgerContents,
    LedgerRead, ShelfPort, ShelfRead, ShelfWrite, UnreadableRecord, UnreadableWhy,
};
pub use report::{
    BookCount, BooksCheck, DownstreamEdge, GetReport, Health, Judged, Names, Presence, Row,
    StaleEntry, StaleReport, StatusReport, TraceReport, UpstreamEdge, UpstreamEnd,
};

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
