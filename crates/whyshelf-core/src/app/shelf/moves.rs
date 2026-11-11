//! The use case that suggests the moves the adapters can see: `moves`. It writes
//! nothing (DEC-29).
//!
//! ```text
//!   load ---------------------------------> the ledger, the keys, the health
//!   PinnedNames::of ----------------------> the names a move may be declared of
//!   ArtifactSource::resolve (one reading) > what is at each now
//!   PinnedNames::gone --------------------> those with nothing behind them, by space
//!   MoveSource::suggest, once per space --> candidates, or "cannot suggest"
//!   GoneNames::suggest -------------------> MoveSuggestions, in one order
//! ```
//!
//! The application decides nothing: which names are asked about, what "nothing
//! there" is, and the order of the answer are the domain's
//! ([`PinnedNames`], [`GoneNames::suggest`]); the adapters look, and this layer
//! carries what they say.

use std::collections::BTreeMap;

use crate::domain::shelf::artifact::ArtifactReadings;
#[cfg(doc)]
use crate::domain::shelf::ledger::GoneNames;
use crate::domain::shelf::ledger::PinnedNames;

use super::ShelfApp;
use super::error::ShelfError;
use super::load::{View, load};
use super::ports::{ArtifactSource, MoveSource, ShelfPort, ShelfRead, port};
use super::report::MovesReport;

impl<P: ShelfRead> ShelfApp<P> {
    /// Every name a pin of something there, of an approval that stands, is read as
    /// now (through the moves declared) that has nothing behind it now, with the
    /// documents whose approvals pin it and the names its space's adapter suggests
    /// it moved to; and the spaces that cannot suggest.
    ///
    /// The artifact source is asked twice: once by the load, about the names the
    /// documents link to, as for every reading, and once more about the pinned names
    /// only (not at all when there are none). Each space's adapter of suggestions is
    /// asked once, about its gone names but those pinned in more than one space, and
    /// not at all when it has none. Nothing is declared: a person reads the answer
    /// and runs `judge moved`. A port that fails is the error of the use case.
    pub async fn moves(&self) -> Result<MovesReport, ShelfError> {
        let loaded = load(&self.ports).await?;
        let pinned = PinnedNames::of(&loaded.ledger);
        let asks = pinned.asks();
        let readings = if asks.is_empty() {
            ArtifactReadings::new()
        } else {
            self.ports
                .artifacts()
                .resolve(&asks)
                .await
                .map_err(port(ShelfPort::Artifacts))?
        };
        let gone = pinned.gone(&readings);
        let mut answers = BTreeMap::new();
        for (space, names) in gone.spaces() {
            let answer = self
                .ports
                .moves()
                .suggest(space, &names)
                .await
                .map_err(port(ShelfPort::Moves))?;
            answers.insert(space.clone(), answer);
        }
        let assessment = loaded.assess();
        let view = View {
            loaded: &loaded,
            assessment: &assessment,
        };
        Ok(MovesReport {
            suggestions: gone.suggest(answers),
            names: loaded.names(),
            health: view.health(),
        })
    }
}
