//! The use case that declares an artifact moved: `judge moved`.
//!
//! ```text
//!   old name           ---- moved_space ----------------> the space its pins are read in
//!                                                         now (or a MovedRefusal)
//!   + new name         ---- MovedOf::new ---------------> a move (two names that differ)
//!   both names, in it  ---- ArtifactSource::resolve ----> what is at each now
//!   the answers        ---- frame_moved ----------------> what moved (or a MovedRefusal)
//!   + time, actor, new id, reason
//!                      ---- Moved::new -----------------> a Record
//!   record             ---- LedgerAppend::append -------> the ledger (create-only)
//! ```
//!
//! The application decides nothing: the space and the rule of a move are the
//! domain's ([`moved_space`], [`frame_moved`]); this layer asks the space's adapter
//! what is at both names, through the artifact port, and carries the answer. As for
//! `judge`, several declarations are decided in order, each on the ledger the ones
//! before it left, on one reading of the documents and the ledger; the adapter is
//! asked again for each declaration, about its two names only. All are written or
//! none (DEC-27).

use crate::domain::actor::Actor;
use crate::domain::nonempty::NonBlank;
use crate::domain::shelf::artifact::{ArtifactAsks, ArtifactName};
use crate::domain::shelf::ledger::{
    Ledger, Moved, MovedOf, MovedRefusal, Record, frame_moved, moved_space,
};

use super::ShelfApp;
use super::error::ShelfError;
use super::load::{Loaded, load};
use super::ports::{
    ActorSource, ArtifactSource, IdSource, LedgerAppend, ShelfPort, ShelfWrite, port,
};

/// A request to declare that an artifact moved.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MovedRequest {
    /// The old name, as a link or a pin writes it.
    pub from: ArtifactName,
    /// The new name, in the same space.
    pub to: ArtifactName,
    /// Why it is the same thing. Required.
    pub reason: NonBlank,
    /// Who declares it. `None` is the person the actor port names.
    pub actor: Option<Actor>,
}

impl<P: ShelfWrite> ShelfApp<P> {
    /// Declare, in order and on one reading, that each artifact named moved, and
    /// append every declaration, or none.
    ///
    /// Refused (nothing written): two names that are one, an old name no pin of an
    /// approval that stands is read as now (a name only a link writes, or one already
    /// moved away), or pins read as it in two spaces;
    /// something still at the old name (a copy is no move), nothing at the new one,
    /// and a name the adapter cannot tell about now. An empty list is
    /// [`ShelfError::NothingRequested`].
    pub async fn moved(&mut self, requests: Vec<MovedRequest>) -> Result<Vec<Moved>, ShelfError> {
        if requests.is_empty() {
            return Err(ShelfError::NothingRequested);
        }
        let mut loaded = load(&self.ports).await?;
        let mut declared = Vec::with_capacity(requests.len());
        for request in requests {
            let one = self.declare_moved(&loaded, request).await?;
            loaded.records.push(Record::Moved(one.clone()));
            loaded.ledger = Ledger::build(loaded.records.clone());
            declared.push(one);
        }
        for one in &declared {
            self.ports
                .append()
                .append(&Record::Moved(one.clone()))
                .await
                .map_err(port(ShelfPort::LedgerAppend))?;
        }
        Ok(declared)
    }

    /// One declaration, decided on what is loaded; nothing is written.
    async fn declare_moved(
        &self,
        loaded: &Loaded,
        request: MovedRequest,
    ) -> Result<Moved, ShelfError> {
        let space = moved_space(&loaded.ledger, &request.from)?;
        let of = MovedOf::new(space, request.from, request.to).map_err(MovedRefusal::from)?;
        let mut asks = ArtifactAsks::new();
        asks.ask_name(of.space().clone(), of.from_name().clone());
        asks.ask_name(of.space().clone(), of.to_name().clone());
        let readings = self
            .ports
            .artifacts()
            .resolve(&asks)
            .await
            .map_err(port(ShelfPort::Artifacts))?;
        let of = frame_moved(of, &readings)?;
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
        Ok(Moved::new(id, of, actor, loaded.now, request.reason))
    }
}
