//! Who a record is attributed to: the one place the names of a request and of the
//! actor port are put together ([`ShelfApp::attribution`]).
//!
//! ```text
//!   request: --actor? --ran-by?        actor port: actor(), ran_by()
//!         \                                /
//!          `---- the request's name, else the port's ----> Attribution::new
//! ```
//!
//! A name in the request wins, and the port is not asked for it. The port is asked
//! for a name only when the request names none, and only at the point where a use case
//! settles who it writes as, which each use case's page says.

use crate::domain::actor::{Actor, Attribution};

use super::ShelfApp;
use super::error::ShelfError;
use super::ports::{ActorSource, ShelfPort, ShelfWrite, port};

/// The names a request gives of who judges and who runs the command; a name it leaves
/// out is the actor port's.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AttributionAsk {
    /// Who answers for the records. `None` is the person the actor port names.
    pub actor: Option<Actor>,
    /// Who ran the command, when it is not the actor. `None` is the one the actor
    /// port names, if it names one: nobody is none.
    pub ran_by: Option<Actor>,
}

/// What the actor port answered in one run, so that a run of several requests asks it
/// at most once for each name, however the requests ask.
#[derive(Debug, Default)]
pub(super) struct PortNames {
    actor: Option<Actor>,
    ran_by: Option<Option<Actor>>,
}

impl<P: ShelfWrite> ShelfApp<P> {
    /// The attribution of the records of a run: each name `ask` gives, else the one the
    /// actor port gives; no actor fails the run. The one place a request's names meet
    /// the port's, so every use case that writes a record asks the same way. A
    /// `ran_by` that is the actor is one name ([`Attribution::new`]).
    pub(super) async fn attribution(&self, ask: AttributionAsk) -> Result<Attribution, ShelfError> {
        self.attribution_in(ask, &mut PortNames::default()).await
    }

    /// As [`attribution`](Self::attribution), for one request of a run: a name the port
    /// gave an earlier request of the run, kept in `asked`, is not asked again.
    pub(super) async fn attribution_in(
        &self,
        ask: AttributionAsk,
        asked: &mut PortNames,
    ) -> Result<Attribution, ShelfError> {
        let actor = match (ask.actor, &asked.actor) {
            (Some(actor), _) => actor,
            (None, Some(actor)) => actor.clone(),
            (None, None) => {
                let actor = self
                    .ports
                    .actor_source()
                    .actor()
                    .await
                    .map_err(port(ShelfPort::Actor))?;
                asked.actor = Some(actor.clone());
                actor
            }
        };
        let ran_by = match (ask.ran_by, &asked.ran_by) {
            (Some(ran_by), _) => Some(ran_by),
            (None, Some(ran_by)) => ran_by.clone(),
            (None, None) => {
                let ran_by = self
                    .ports
                    .actor_source()
                    .ran_by()
                    .await
                    .map_err(port(ShelfPort::Actor))?;
                asked.ran_by = Some(ran_by.clone());
                ran_by
            }
        };
        Ok(Attribution::new(actor, ran_by))
    }
}
