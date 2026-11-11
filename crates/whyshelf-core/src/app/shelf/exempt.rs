//! The use case that exempts a document from one covering link: `judge exempt`.
//!
//! ```text
//!   reference         ---- resolve -----------------> the document's identity
//!   + book, link      ---- frame_exempt ------------> what to exempt (the document
//!                                                     valid, the link covering its
//!                                                     book, no exemption standing),
//!                                                     or an ExemptRefusal
//!   + time, actor, new id, reason
//!                     ---- Exempt::new -------------> a Record
//!   record            ---- LedgerAppend::append ----> the ledger (create-only)
//! ```
//!
//! The application decides nothing: what an exemption may be made of is
//! [`frame_exempt`]'s. As for `judge`, several exemptions are decided in order on one
//! reading, each on the ledger the ones before it left (so the same document named
//! twice is refused the second time), and all are written or none (DEC-27).

use crate::domain::actor::Actor;
use crate::domain::id::DocId;
use crate::domain::nonempty::NonBlank;
use crate::domain::shelf::doc::DocKey;
use crate::domain::shelf::exemption::{ExemptRefusal, frame_exempt};
use crate::domain::shelf::ledger::{Exempt, Ledger, Record};
use crate::domain::shelf::lookup::Reference;
use crate::domain::shelf::schema::{BookName, EntryName};

use super::ShelfApp;
use super::error::ShelfError;
use super::load::{Loaded, load};
use super::ports::{ActorSource, IdSource, LedgerAppend, ShelfPort, ShelfWrite, port};
use super::read::identity;

/// A request to exempt a document from one covering link.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExemptRequest {
    /// The document, by a reference.
    pub reference: Reference,
    /// The book that declares the link.
    pub book: BookName,
    /// The link, whose cover the document is exempt from.
    pub link: EntryName,
    /// Why the document is meant to stay unreached. Required.
    pub reason: NonBlank,
    /// Who declares it. `None` is the person the actor port names.
    pub actor: Option<Actor>,
}

/// One exemption `judge exempt` wrote.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Exempted {
    /// The record that was appended.
    pub record: Exempt,
    /// The key of the document.
    pub key: Option<DocKey>,
}

impl Exempted {
    /// The document.
    pub fn doc(&self) -> DocId {
        self.record.of().doc()
    }
}

impl<P: ShelfWrite> ShelfApp<P> {
    /// Exempt, in order and on one reading, each document named from the link named,
    /// and append every exemption, or none.
    ///
    /// Refused (nothing written), as [`frame_exempt`] refuses: a reference that names
    /// no document or several, a document that is not valid or is decided against,
    /// a link of the book that is not declared, is declared with no cover, points
    /// into another book than the document's or reaches the document now, and a
    /// document an exemption that stands already exempts from the link; when that
    /// exemption is an earlier one of the run, the refusal says so
    /// ([`ExemptRefusal::AlreadyExemptInRun`]). An empty list is
    /// [`ShelfError::NothingRequested`].
    pub async fn exempt(
        &mut self,
        requests: Vec<ExemptRequest>,
    ) -> Result<Vec<Exempted>, ShelfError> {
        if requests.is_empty() {
            return Err(ShelfError::NothingRequested);
        }
        let mut loaded = load(&self.ports).await?;
        let mut exempted = Vec::with_capacity(requests.len());
        for request in requests {
            let one = match self.declare_exempt(&loaded, request).await {
                Ok(one) => one,
                // The exemption that stands is one of this run, not written yet: the
                // document was named twice, and its id names no record.
                Err(ShelfError::ExemptRefused(ExemptRefusal::AlreadyExempt {
                    book,
                    link,
                    record,
                })) if exempted.iter().any(|e: &Exempted| e.record.id() == record) => {
                    return Err(ExemptRefusal::AlreadyExemptInRun { book, link }.into());
                }
                Err(other) => return Err(other),
            };
            loaded.records.push(Record::Exempt(one.record.clone()));
            loaded.ledger = Ledger::build(loaded.records.clone());
            exempted.push(one);
        }
        for one in &exempted {
            self.ports
                .append()
                .append(&Record::Exempt(one.record.clone()))
                .await
                .map_err(port(ShelfPort::LedgerAppend))?;
        }
        Ok(exempted)
    }

    /// One exemption, decided on what is loaded; nothing is written.
    async fn declare_exempt(
        &self,
        loaded: &Loaded,
        request: ExemptRequest,
    ) -> Result<Exempted, ShelfError> {
        let id = identity(loaded, &request.reference)?;
        let of = frame_exempt(
            &loaded.shelf,
            &loaded.ledger,
            id,
            request.book,
            request.link,
        )?;
        let actor = match request.actor {
            Some(actor) => actor,
            None => self
                .ports
                .actor_source()
                .actor()
                .await
                .map_err(port(ShelfPort::Actor))?,
        };
        let record_id = self
            .ports
            .ids()
            .new_judgement_id()
            .await
            .map_err(port(ShelfPort::Ids))?;
        Ok(Exempted {
            key: loaded.shelf.doc(id).map(|doc| doc.key().clone()),
            record: Exempt::new(record_id, of, actor, loaded.now, request.reason),
        })
    }
}
