//! The trial of the moves of a prefix: what `judge moved --prefix` does up to the
//! point of writing, for any number of moves on one reading.
//!
//! ```text
//!   each move:  ArtifactSource::renamed (each space's own grammar)
//!               --> plan_prefix_moves (which names move, or NothingUnder)
//!   every plan: ArtifactSource::resolve, once, every old and new name
//!   each plan:  frame_prefix_moves (every refusal of both steps in one answer)
//! ```
//!
//! [`ShelfApp::frame_prefixes`] is the one place that procedure is written: `judge
//! moved --prefix` ([`ShelfApp::moved_prefix`]) runs it for the one move asked, and
//! `moves` for the moves its suggestions carry, to offer only those that stand. It
//! writes nothing and records nothing: a move that stands is held against what is
//! there now, not admitted by who may decide ([`Ledger::accept`] is not tried).
//!
//! The application decides nothing there either: which names are under a prefix is
//! the adapter's, which move is [`plan_prefix_moves`]'s, whether a move stands is
//! [`frame_prefix_moves`]'s.
//!
//! [`Ledger::accept`]: crate::domain::shelf::ledger::Ledger::accept

use crate::domain::nonempty::NonEmptyVec;
use crate::domain::shelf::artifact::{ArtifactAsks, ArtifactReadings, PrefixMove, Space};
use crate::domain::shelf::ledger::{
    MovedOf, PinnedNames, PrefixRefusal, frame_prefix_moves, plan_prefix_moves,
};

use super::ShelfApp;
use super::error::ShelfError;
use super::ports::{ArtifactSource, ShelfPort, ShelfRead, port};

/// What the trial of one move of a prefix came to.
pub(super) struct FramedPrefix {
    /// The move that was tried.
    pub(super) of: PrefixMove,
    /// The moves that stand, one per name, or why the prefix is refused.
    pub(super) framed: Result<NonEmptyVec<MovedOf>, PrefixRefusal>,
    /// The spaces pinned in whose adapter takes no prefix: names of them may be under
    /// it, and none was moved. Empty when the plan was refused.
    pub(super) not_asked: Vec<Space>,
}

impl<P: ShelfRead> ShelfApp<P> {
    /// Try each move of `moves` as `judge moved --prefix` would, against the names
    /// `pinned`: the adapters say which names are under it and what each is under the
    /// new one ([`ArtifactSource::renamed`]), the domain decides which move
    /// ([`plan_prefix_moves`]), every old and new name of every plan is asked of the
    /// adapters once, and each plan is held against what is there
    /// ([`frame_prefix_moves`]). One result per move, in the order of `moves`; a
    /// refusal is in its result, and a port that fails is the error of the use case.
    ///
    /// Nothing is asked of the adapters for the names when every move is refused
    /// before that.
    pub(super) async fn frame_prefixes(
        &self,
        pinned: &PinnedNames,
        moves: &[PrefixMove],
    ) -> Result<Vec<FramedPrefix>, ShelfError> {
        let prefix_asks = pinned.prefix_asks();
        let mut asks = ArtifactAsks::new();
        let mut planned = Vec::with_capacity(moves.len());
        for of in moves {
            let answers = self
                .ports
                .artifacts()
                .renamed(&prefix_asks, of)
                .map_err(port(ShelfPort::Artifacts))?;
            let plan = plan_prefix_moves(pinned, &answers);
            if let Ok(plan) = &plan {
                for one in &plan.moves {
                    asks.ask_name(one.space().clone(), one.from_name().clone());
                    asks.ask_name(one.space().clone(), one.to_name().clone());
                }
            }
            planned.push((of, plan));
        }
        let readings = if planned.iter().any(|(_, plan)| plan.is_ok()) {
            self.ports
                .artifacts()
                .resolve(&asks)
                .await
                .map_err(port(ShelfPort::Artifacts))?
        } else {
            ArtifactReadings::new()
        };
        Ok(planned
            .into_iter()
            .map(|(of, plan)| match plan {
                Ok(plan) => FramedPrefix {
                    of: of.clone(),
                    not_asked: plan.not_asked.clone(),
                    framed: frame_prefix_moves(plan, &readings),
                },
                Err(refusal) => FramedPrefix {
                    of: of.clone(),
                    framed: Err(refusal),
                    not_asked: Vec::new(),
                },
            })
            .collect())
    }
}
