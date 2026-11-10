//! The use case that withdraws records by their identity: `judge withdraw`.
//!
//! `judge revoke <ref>` withdraws the decision that stands about a document. A
//! declaration that an edit keeps the meaning stands about no document's status, and
//! an older decision may be the one to take back, so a record can also be withdrawn
//! by its identity (the `id` of the record, which every answer that names a record
//! shows). Each identity is checked by [`Ledger::check_withdraw`]; one revocation
//! withdraws them all, or nothing is written.
//!
//! [`Ledger::check_withdraw`]: crate::domain::shelf::ledger::Ledger::check_withdraw

use std::collections::BTreeSet;

use crate::domain::actor::Actor;
use crate::domain::nonempty::{NonBlank, NonEmptySet};
use crate::domain::shelf::ledger::{JudgementId, Record, Revocation};

use super::ShelfApp;
use super::error::ShelfError;
use super::load::load;
use super::ports::{ActorSource, IdSource, LedgerAppend, ShelfPort, ShelfWrite, port};

/// A request to withdraw records by their identity.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WithdrawRequest {
    /// The records to withdraw.
    pub records: Vec<JudgementId>,
    /// Why. Required.
    pub reason: NonBlank,
    /// Who withdraws them. `None` is the person the actor port names.
    pub actor: Option<Actor>,
}

impl<P: ShelfWrite> ShelfApp<P> {
    /// Withdraw the records named, with one revocation, and append it.
    ///
    /// Refused (nothing written): a record that is no decision or declaration of the
    /// ledger, or one already withdrawn. An empty list is
    /// [`ShelfError::NothingRequested`].
    pub async fn withdraw(&mut self, request: WithdrawRequest) -> Result<Record, ShelfError> {
        let loaded = load(&self.ports).await?;
        for id in &request.records {
            loaded.ledger.check_withdraw(*id)?;
        }
        let targets = NonEmptySet::from_set(request.records.into_iter().collect::<BTreeSet<_>>())
            .map_err(|_| ShelfError::NothingRequested)?;
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
        let record = Record::Revocation(Revocation::new(
            id,
            targets,
            actor,
            loaded.now,
            request.reason,
        ));
        self.ports
            .append()
            .append(&record)
            .await
            .map_err(port(ShelfPort::LedgerAppend))?;
        Ok(record)
    }
}
