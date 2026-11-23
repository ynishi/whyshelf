//! The use case that declares an edit keeps the meaning: `judge same`.
//!
//! ```text
//!   reference or name ---- resolve -----------------> a SameMeaningTarget
//!   target            ---- frame_same_meaning ------> what to declare (the edits stale
//!                                                     tells as changes), or a
//!                                                     SameMeaningRefusal
//!   + actor           ---- Framed::not_declared_by -> without what the actor declared
//!                                                     already, or AlreadyDeclaredBy
//!   + time, new id, reason
//!                     ---- SameMeaning::new,
//!                          in_book ----------------> a Record, naming its document's book
//!   record            ---- Ledger::accept ---------> admitted by who may decide, or
//!                                                     NotAdmitted
//!   admitted          ---- RunWrites::record -------> the ledger (create-only), every
//!                                                     record put on disk once, at the end
//! ```
//!
//! The application chooses none of the hashes: [`frame_same_meaning`] does. As for
//! `judge`, several declarations are decided in order on one reading, each on the
//! ledger the ones before it left, and all are written or none.

use crate::domain::hash::ContentHash;
use crate::domain::id::DocId;
use crate::domain::nonempty::NonBlank;
use crate::domain::shelf::artifact::{ArtifactKey, ArtifactName};
use crate::domain::shelf::doc::DocKey;
use crate::domain::shelf::ledger::{
    Record, SameMeaning, SameMeaningOf, SameMeaningTarget, frame_same_meaning,
};
use crate::domain::shelf::lookup::Reference;

use super::ShelfApp;
use super::attribution::AttributionAsk;
use super::error::ShelfError;
use super::load::{Loaded, load};
use super::ports::{IdSource, ShelfPort, ShelfWrite, port};
use super::read::identity;

/// What a declaration is to be about, as the person names it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SameMeaningAbout {
    /// A document, by a reference.
    Doc(Reference),
    /// An artifact, by its name, in the one space the documents link it in.
    Artifact(ArtifactName),
}

/// A request to declare that an edit keeps the meaning.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SameMeaningRequest {
    /// What it is about.
    pub about: SameMeaningAbout,
    /// The earlier hash the edit was read from, when it cannot be told otherwise
    /// (see [`frame_same_meaning`]).
    pub from: Option<ContentHash>,
    /// Why the meaning is kept. Required.
    pub reason: NonBlank,
    /// Who declares it, and who runs the command.
    pub by: AttributionAsk,
}

/// What a declaration was about, as the answer names it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DeclaredAbout {
    /// A document, with its key.
    Doc {
        /// The document.
        id: DocId,
        /// Its key.
        key: Option<DocKey>,
    },
    /// An artifact.
    Artifact {
        /// Its space and its name.
        artifact: ArtifactKey,
    },
}

/// What `judge same` wrote.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Declared {
    /// The record that was appended.
    pub record: Record,
    /// What it is about.
    pub about: DeclaredAbout,
}

impl<P: ShelfWrite> ShelfApp<P> {
    /// Declare, in order and on one reading, that each edit named keeps the meaning,
    /// and append every declaration, or none.
    ///
    /// Refused (nothing written): a document that is not valid, an edit of a pinned
    /// hash with nothing behind it now or that cannot be told now, and a pin whose
    /// content stale does not compare now (its link no longer writes the name, or its
    /// document is not valid). Refused as a request to put right (nothing written
    /// either): a name of an artifact no document links or that is linked in two
    /// spaces, nothing to declare (no pin of it holds a hash that changed, whatever is
    /// there now, or the edit is joined by the ledger already), a pin whose comparison
    /// changed, several earlier hashes, a hash named that no pin holds as changed (one
    /// nothing pins, the hash now, or one that already means the same), and one already
    /// declared by the same name.
    pub async fn same_meaning(
        &mut self,
        requests: Vec<SameMeaningRequest>,
    ) -> Result<Vec<Declared>, ShelfError> {
        if requests.is_empty() {
            return Err(ShelfError::NothingRequested);
        }
        let mut loaded = load(&self.ports).await?;
        let mut declared = Vec::with_capacity(requests.len());
        let mut admitted = Vec::with_capacity(requests.len());
        for request in requests {
            let one = self.declare(&loaded, request).await?;
            admitted.push(loaded.take(one.record.clone())?);
            declared.push(one);
        }
        self.write(&admitted).await?;
        Ok(declared)
    }

    /// One declaration, decided on what is loaded; nothing is written.
    async fn declare(
        &self,
        loaded: &Loaded,
        request: SameMeaningRequest,
    ) -> Result<Declared, ShelfError> {
        let target = match &request.about {
            SameMeaningAbout::Doc(reference) => {
                SameMeaningTarget::Doc(identity(loaded, reference)?)
            }
            SameMeaningAbout::Artifact(name) => SameMeaningTarget::Artifact(name.clone()),
        };
        let framed = frame_same_meaning(&loaded.shelf, &loaded.ledger, target, request.from)?;

        let by = self.attribution(request.by).await?;
        // A name that declared an edge already adds nothing by declaring it again; who
        // ran the command is not a name that declared it.
        let of = framed.not_declared_by(by.actor())?;
        let about = match &of {
            SameMeaningOf::Doc { id, .. } => DeclaredAbout::Doc {
                id: *id,
                key: loaded.shelf.doc(*id).map(|doc| doc.key().clone()),
            },
            SameMeaningOf::Artifact { artifact, .. } => DeclaredAbout::Artifact {
                artifact: artifact.clone(),
            },
        };
        let id = self
            .ports
            .ids()
            .new_judgement_id()
            .await
            .map_err(port(ShelfPort::Ids))?;
        let record = Record::SameMeaning(
            SameMeaning::new(id, of, by, loaded.now, request.reason)?.in_book(&loaded.shelf),
        );
        Ok(Declared { record, about })
    }
}
