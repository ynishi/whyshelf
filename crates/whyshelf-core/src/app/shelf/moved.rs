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
//!   record             ---- RunWrites::record ----------> the ledger (create-only), every
//!                                                         record put on disk once, at the end
//! ```
//!
//! The application decides nothing: the space and the rule of a move are the
//! domain's ([`moved_space`], [`frame_moved`]); this layer asks the space's adapter
//! what is at both names, through the artifact port, and carries the answer. As for
//! `judge`, several declarations are decided in order, each on the ledger the ones
//! before it left, on one reading of the documents and the ledger; the adapter is
//! asked again for each declaration, about its two names only. All are written or
//! none (DEC-27).
//!
//! `judge moved --prefix` ([`ShelfApp::moved_prefix`]) declares every name under a
//! prefix moved at once, on one reading:
//!
//! ```text
//!   load --> PinnedNames::prefix_asks --> ArtifactSource::renamed (each space's own grammar)
//!        --> plan_prefix_moves (which names move, or NothingUnder)
//!        --> ArtifactSource::resolve, once, every old and new name
//!        --> frame_prefix_moves (every refusal of both steps in one answer)
//!        --> actor, once; id for each; the time of the load
//!        --> RunWrites::run: every record, put on disk once
//! ```
//!
//! The application decides nothing there either: which names are under the prefix and
//! what each is under the new one is the adapter's, which names move is
//! [`plan_prefix_moves`]'s, whether a move stands is [`frame_moved`]'s.

use crate::domain::actor::Actor;
use crate::domain::nonempty::NonBlank;
use crate::domain::shelf::artifact::{ArtifactAsks, ArtifactName, PrefixMove, Space};
use crate::domain::shelf::ledger::{
    Ledger, Moved, MovedOf, MovedRefusal, PinnedNames, Record, frame_moved, frame_prefix_moves,
    moved_space, plan_prefix_moves,
};

use super::ShelfApp;
use super::error::ShelfError;
use super::load::{Loaded, load};
use super::ports::{ActorSource, ArtifactSource, IdSource, ShelfPort, ShelfWrite, port};
use super::run_writes::RunWrites;

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

/// A request to declare that every name under one prefix moved to the same name under
/// another.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrefixMovedRequest {
    /// The prefix the names are under now, and the one they are moved under.
    pub of: PrefixMove,
    /// Why they are the same things. Required.
    pub reason: NonBlank,
    /// Who declares it. `None` is the person the actor port names.
    pub actor: Option<Actor>,
}

/// What a declaration of the move of a prefix wrote.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MovedPrefix {
    /// The declarations, one per name moved, in the order of their spaces and names.
    pub moved: Vec<Moved>,
    /// The spaces names are pinned in whose adapter takes no prefix: names of them
    /// may be under the prefix, and none was moved.
    pub not_asked: Vec<Space>,
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
        let records = declared
            .iter()
            .cloned()
            .map(Record::Moved)
            .collect::<Vec<_>>();
        RunWrites::run(&self.ports, |writes| {
            Box::pin(async move {
                for record in &records {
                    writes.record(record).await?;
                }
                Ok(())
            })
        })
        .await?;
        Ok(declared)
    }

    /// Declare, once and on one reading, that every name an approval pins under one
    /// prefix moved to the same name under another (`judge moved --prefix`), and
    /// append every declaration, or none.
    ///
    /// The names asked about are the ones approvals that stand pin, in every space; each
    /// space's adapter says which are under the prefix and what each is under the new
    /// one ([`ArtifactSource::renamed`]), and the domain decides which move
    /// ([`plan_prefix_moves`]). Every old and new name is then asked of the adapters
    /// once, and each move is held against what is there ([`frame_prefix_moves`]).
    /// One declaration is made per name, so that `judge withdraw` takes one back, each
    /// with its own identity and the time of the load, by one actor.
    ///
    /// Refused (nothing written): nothing under the prefix in any space
    /// ([`PrefixRefusal::NothingUnder`](crate::domain::shelf::ledger::PrefixRefusal::NothingUnder)),
    /// and any name under it that cannot be moved, every one listed
    /// ([`PrefixRefusal::Names`](crate::domain::shelf::ledger::PrefixRefusal::Names)). The spaces whose adapter takes no
    /// prefix are told with the answer ([`MovedPrefix::not_asked`]).
    pub async fn moved_prefix(
        &mut self,
        request: PrefixMovedRequest,
    ) -> Result<MovedPrefix, ShelfError> {
        let loaded = load(&self.ports).await?;
        let pinned = PinnedNames::of(&loaded.ledger);
        let answers = self
            .ports
            .artifacts()
            .renamed(&pinned.prefix_asks(), &request.of)
            .map_err(port(ShelfPort::Artifacts))?;
        let plan = plan_prefix_moves(&pinned, &answers)?;
        let not_asked = plan.not_asked.clone();
        let mut asks = ArtifactAsks::new();
        for of in &plan.moves {
            asks.ask_name(of.space().clone(), of.from_name().clone());
            asks.ask_name(of.space().clone(), of.to_name().clone());
        }
        let readings = self
            .ports
            .artifacts()
            .resolve(&asks)
            .await
            .map_err(port(ShelfPort::Artifacts))?;
        let framed = frame_prefix_moves(plan, &readings)?;
        let actor = match request.actor {
            Some(actor) => actor,
            None => self
                .ports
                .actor_source()
                .actor()
                .await
                .map_err(port(ShelfPort::Actor))?,
        };
        let mut moved = Vec::with_capacity(framed.len());
        for of in framed.into_vec() {
            let id = self
                .ports
                .ids()
                .new_judgement_id()
                .await
                .map_err(port(ShelfPort::Ids))?;
            moved.push(Moved::new(
                id,
                of,
                actor.clone(),
                loaded.now,
                request.reason.clone(),
            ));
        }
        let records = moved.iter().cloned().map(Record::Moved).collect::<Vec<_>>();
        RunWrites::run(&self.ports, |writes| {
            Box::pin(async move {
                for record in &records {
                    writes.record(record).await?;
                }
                Ok(())
            })
        })
        .await?;
        Ok(MovedPrefix { moved, not_asked })
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
