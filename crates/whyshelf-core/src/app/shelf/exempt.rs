//! The use case that exempts a document, or an artifact, from one covering link:
//! `judge exempt`.
//!
//! ```text
//!   reference         ---- resolve -----------------> the document's identity
//!   or an artifact's  ---- the space of the link ----> the artifact, by space and name
//!   name
//!   + book, link      ---- frame_exempt ------------> what to exempt (the document
//!                         frame_exempt_artifact       valid, or the name listed, the
//!                                                     link covering them, no exemption
//!                                                     standing), or an ExemptRefusal
//!   artifact only     ---- listings_of, ------------> the names the selections of the
//!                          ListedNames::of            links named take, read as the
//!                                                     assessment of coverage reads them
//!   + time, actor (and who ran the command), new id, reason
//!                     ---- Exempt::new -------------> a Record
//!   record            ---- Ledger::accept ---------> admitted by who may decide, or
//!                                                     NotAdmitted
//!   admitted          ---- RunWrites::record -------> the ledger (create-only), every
//!                                                     record put on disk once, at the end
//! ```
//!
//! The application decides nothing: what an exemption may be made of is
//! [`frame_exempt`]'s and [`frame_exempt_artifact`]'s. As for `judge`, several
//! exemptions are decided in order on one reading, each on the ledger the ones before
//! it left (so the same document named twice is refused the second time), and all are
//! written or none (DEC-27). An artifact is exempt only from a link to artifacts that
//! declares a `cover`, and only a name the selection of that link takes now: the names
//! are asked of the adapter of the link's space only when the run exempts one.

use std::collections::BTreeSet;

use crate::domain::id::DocId;
use crate::domain::nonempty::NonBlank;
use crate::domain::shelf::artifact::{
    ArtifactKey, ArtifactName, ListedNames, Listings, Selection, Space,
};
use crate::domain::shelf::doc::DocKey;
use crate::domain::shelf::exemption::{
    ExemptRefusal, artifact_cover_of, artifact_space_of, frame_exempt, frame_exempt_artifact,
};
use crate::domain::shelf::ledger::{Exempt, Record};
use crate::domain::shelf::lookup::Reference;
use crate::domain::shelf::schema::{BookName, EntryName};

use super::ShelfApp;
use super::attribution::AttributionAsk;
use super::error::ShelfError;
use super::load::{Loaded, listings_of, load};
use super::ports::{IdSource, ShelfPort, ShelfWrite, port};
use super::read::identity;

/// What `judge exempt` is asked to exempt from the link: a document, or an artifact.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExemptAsk {
    /// A document, by a reference.
    Doc(Reference),
    /// An artifact, by its name as a link writes it. The space is the link's.
    Artifact(ArtifactName),
}

/// A request to exempt a document or an artifact from one covering link.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExemptRequest {
    /// What is exempt: a document, or an artifact.
    pub subject: ExemptAsk,
    /// The book that declares the link.
    pub book: BookName,
    /// The link, whose cover the subject is exempt from.
    pub link: EntryName,
    /// Why the subject is meant to stay unreached. Required.
    pub reason: NonBlank,
    /// Who declares it, and who runs the command.
    pub by: AttributionAsk,
}

/// One exemption `judge exempt` wrote.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Exempted {
    /// The record that was appended.
    pub record: Exempt,
    /// The key of the document; none for an artifact, or for a document that has
    /// none.
    pub key: Option<DocKey>,
}

impl Exempted {
    /// The document, when the exemption is of one.
    pub fn doc(&self) -> Option<DocId> {
        self.record.of().doc()
    }

    /// The artifact, when the exemption is of one.
    pub fn artifact(&self) -> Option<&ArtifactKey> {
        self.record.of().artifact()
    }
}

impl<P: ShelfWrite> ShelfApp<P> {
    /// Exempt, in order and on one reading, each document or artifact named from the
    /// link named, and append every exemption, or none.
    ///
    /// Refused (nothing written), as [`frame_exempt`] and [`frame_exempt_artifact`]
    /// refuse: a reference that names no document or several, a document that is not
    /// valid or is decided against, a link of the book that is not declared, is
    /// declared with no cover, points into another book than the document's (or is
    /// not a link to artifacts, for an artifact) or reaches the subject now; an
    /// artifact the selection of the link does not take now, or one whose names
    /// cannot all be listed; and a subject an exemption that stands already exempts
    /// from the link, which, when that exemption is an earlier one of the run, the
    /// refusal says ([`ExemptRefusal::AlreadyExemptInRun`]). An empty list is
    /// [`ShelfError::NothingRequested`].
    pub async fn exempt(
        &mut self,
        requests: Vec<ExemptRequest>,
    ) -> Result<Vec<Exempted>, ShelfError> {
        if requests.is_empty() {
            return Err(ShelfError::NothingRequested);
        }
        let mut loaded = load(&self.ports).await?;
        let listed = self.listed_for(&loaded, &requests).await?;
        let mut exempted = Vec::with_capacity(requests.len());
        let mut admitted = Vec::with_capacity(requests.len());
        for request in requests {
            let one = match self.declare_exempt(&loaded, request, &listed).await {
                Ok(one) => one,
                // The exemption that stands is one of this run, not written yet: the
                // subject was named twice, and its id names no record.
                Err(ShelfError::ExemptRefused(ExemptRefusal::AlreadyExempt {
                    book,
                    link,
                    record,
                })) if exempted.iter().any(|e: &Exempted| e.record.id() == record) => {
                    return Err(ExemptRefusal::AlreadyExemptInRun { book, link }.into());
                }
                Err(other) => return Err(other),
            };
            admitted.push(loaded.take(Record::Exempt(one.record.clone()))?);
            exempted.push(one);
        }
        self.write(&admitted).await?;
        Ok(exempted)
    }

    /// The names the selections of the links an artifact is exempted from take, read
    /// as the assessment of coverage reads them ([`ListedNames::of`]); none were asked
    /// for when the run exempts no artifact. A link that is no covering link to
    /// artifacts asks nothing: [`frame_exempt_artifact`] refuses it.
    async fn listed_for(
        &self,
        loaded: &Loaded,
        requests: &[ExemptRequest],
    ) -> Result<ListedNames, ShelfError> {
        let schema = loaded.shelf.schema();
        let asks: BTreeSet<(Space, Selection)> = requests
            .iter()
            .filter(|request| matches!(request.subject, ExemptAsk::Artifact(_)))
            .filter_map(|request| {
                let space = artifact_space_of(schema, &request.book, &request.link).ok()?;
                let cover = artifact_cover_of(schema, &request.book, &request.link, space).ok()?;
                Some((space.clone(), cover.selection().clone()))
            })
            .collect();
        if asks.is_empty() {
            return Ok(ListedNames::of(&Listings::NotAsked));
        }
        let listings = listings_of(self.ports.artifacts(), asks).await?;
        Ok(ListedNames::of(&listings))
    }

    /// One exemption, decided on what is loaded; nothing is written.
    async fn declare_exempt(
        &self,
        loaded: &Loaded,
        request: ExemptRequest,
        listed: &ListedNames,
    ) -> Result<Exempted, ShelfError> {
        let (of, key) = match request.subject {
            ExemptAsk::Doc(reference) => {
                let id = identity(loaded, &reference)?;
                let of = frame_exempt(
                    &loaded.shelf,
                    &loaded.ledger,
                    id,
                    request.book,
                    request.link,
                )?;
                (of, loaded.shelf.doc(id).map(|doc| doc.key().clone()))
            }
            ExemptAsk::Artifact(name) => {
                let space = artifact_space_of(loaded.shelf.schema(), &request.book, &request.link)
                    .map_err(|miss| ExemptRefusal::NotCovering {
                        book: request.book.clone(),
                        link: request.link.clone(),
                        miss,
                    })?
                    .clone();
                let of = frame_exempt_artifact(
                    &loaded.shelf,
                    &loaded.ledger,
                    ArtifactKey::new(space, name),
                    request.book,
                    request.link,
                    listed,
                )?;
                (of, None)
            }
        };
        let by = self.attribution(request.by).await?;
        let record_id = self
            .ports
            .ids()
            .new_judgement_id()
            .await
            .map_err(port(ShelfPort::Ids))?;
        Ok(Exempted {
            key,
            record: Exempt::new(record_id, of, by, loaded.now, request.reason),
        })
    }
}
