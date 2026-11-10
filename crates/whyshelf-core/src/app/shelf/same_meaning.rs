//! The use case that declares an edit keeps the meaning: `judge same`.
//!
//! ```text
//!   reference or path ---- resolve -----------------> a SameMeaningTarget
//!   target            ---- frame_same_meaning ------> what to declare (every earlier
//!                                                     hash an approval pinned), or a
//!                                                     SameMeaningRefusal
//!   + time, actor, new id, reason
//!                     ---- SameMeaning::new --------> a Record
//!   record            ---- LedgerAppend::append ----> the ledger (create-only)
//! ```
//!
//! The application chooses none of the hashes: [`frame_same_meaning`] does. As for
//! `judge`, several declarations are decided in order on one reading, each on the
//! ledger the ones before it left, and all are written or none.

use crate::domain::actor::Actor;
use crate::domain::hash::ContentHash;
use crate::domain::id::DocId;
use crate::domain::location::RelPath;
use crate::domain::nonempty::NonBlank;
use crate::domain::shelf::doc::DocKey;
use crate::domain::shelf::ledger::{
    Ledger, Record, SameMeaning, SameMeaningOf, SameMeaningTarget, frame_same_meaning,
};
use crate::domain::shelf::lookup::Reference;

use super::ShelfApp;
use super::error::ShelfError;
use super::load::{Loaded, load};
use super::ports::{ActorSource, IdSource, LedgerAppend, ShelfPort, ShelfWrite, port};
use super::read::identity;

/// What a declaration is to be about, as the person names it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SameMeaningAbout {
    /// A document, by a reference.
    Doc(Reference),
    /// A file that is no document, by its path under the root.
    Artifact(RelPath),
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
    /// Who declares it. `None` is the person the actor port names.
    pub actor: Option<Actor>,
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
    /// A file.
    Artifact {
        /// Its path.
        path: RelPath,
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
    /// Refused (nothing written): a document that is not valid, a path no document
    /// links or with no file, and an edit no approval that stands pinned another hash
    /// of (or one already declared).
    pub async fn same_meaning(
        &mut self,
        requests: Vec<SameMeaningRequest>,
    ) -> Result<Vec<Declared>, ShelfError> {
        if requests.is_empty() {
            return Err(ShelfError::NothingRequested);
        }
        let mut loaded = load(&self.ports).await?;
        let mut declared = Vec::with_capacity(requests.len());
        for request in requests {
            let one = self.declare(&loaded, request).await?;
            loaded.records.push(one.record.clone());
            loaded.ledger = Ledger::build(loaded.records.clone());
            declared.push(one);
        }
        for one in &declared {
            self.ports
                .append()
                .append(&one.record)
                .await
                .map_err(port(ShelfPort::LedgerAppend))?;
        }
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
            SameMeaningAbout::Artifact(path) => SameMeaningTarget::Artifact(path.clone()),
        };
        let of = frame_same_meaning(&loaded.shelf, &loaded.ledger, target, request.from)?;
        let about = match &of {
            SameMeaningOf::Doc { id, .. } => DeclaredAbout::Doc {
                id: *id,
                key: loaded.shelf.doc(*id).map(|doc| doc.key().clone()),
            },
            SameMeaningOf::Artifact { path, .. } => DeclaredAbout::Artifact { path: path.clone() },
        };

        let actor = match request.actor {
            Some(actor) => actor,
            None => self
                .ports
                .actor_source()
                .actor()
                .await
                .map_err(port(ShelfPort::Actor))?,
        };
        let id = self
            .ports
            .ids()
            .new_judgement_id()
            .await
            .map_err(port(ShelfPort::Ids))?;
        let record =
            Record::SameMeaning(SameMeaning::new(id, of, actor, loaded.now, request.reason)?);
        Ok(Declared { record, about })
    }
}
