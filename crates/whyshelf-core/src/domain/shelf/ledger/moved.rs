//! Declarations that an artifact moved: [`Moved`].
//!
//! A person says that what was at one name of a space is now at another (a file
//! moved, a Rust item renamed), with a reason. Freshness then reads a pin of the old
//! name, made at or before the declaration, as a pin of the new name, with the hash
//! it pinned ([`Ledger::moved_to`]). A move is of something that was there: a pin of
//! the name as absent held no content to move and is read as written, and only a pin
//! of something there makes a name one a move may be declared of. An unchanged move leaves the
//! approval as it was. Nothing is asked of the content: a move with an edit is a move
//! and a change, an `artifact_changed` at the new name, so the approval is stale and
//! not broken. A pin made after the declaration is read as written.
//!
//! Moves are followed in the order of time, one after another, so a chain of moves
//! is read to its end, and a way back ends at the name it started at. Of two moves of
//! one name at one time, the one of the larger identity is followed, as the larger
//! identity is the decision that stands at one time:
//!
//! ```text
//!   pinned a.rs (t0) -- moved a.rs -> b.rs (t1) -- moved b.rs -> c.rs (t2)   read as c.rs
//!   pinned b.rs (t1) ------------------------------ moved b.rs -> c.rs (t2)   read as c.rs
//!   pinned a.rs (t3)                                                          read as a.rs
//!   pinned a.rs (t0) -- moved a.rs -> b.rs (t1) -- moved b.rs -> a.rs (t2)   read as a.rs
//! ```
//!
//! The names a way left ([`ReadPin::left_behind`]: the name pinned and each name a
//! chain passed, but the one it ends at) are names a link is not to write any more.
//!
//! A declaration is made in two steps, so that the domain asks no adapter:
//! [`moved_space`] says which space the old name is of, from the pins of the
//! approvals that stand as they are read now; the application asks that space's
//! adapter what both names are now; [`frame_moved`] holds the answer against the
//! rule (the old name has nothing there now, the new one has: a copy is no move). The
//! one rule of what a move is, two names that differ, is [`MovedOf::new`]'s. A
//! declaration is withdrawn by a [`Revocation`](super::Revocation), as a decision is.
//!
//! [`Ledger::moved_to`]: super::Ledger::moved_to

use std::cmp::Reverse;
use std::collections::BTreeSet;

use thiserror::Error;

use crate::domain::actor::Actor;
use crate::domain::nonempty::{NonBlank, NonEmptyVec};

use super::super::artifact::{
    ArtifactKey, ArtifactName, ArtifactReadings, ArtifactState, Comparison, Space, Untold,
};
use super::{ArtifactPin, JudgedAt, JudgementId, Ledger, PinnedArtifact};

/// What a declaration says moved: in one space, from one name to another.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MovedOf {
    space: Space,
    from: ArtifactName,
    to: ArtifactName,
}

impl MovedOf {
    /// A move in `space` from `from` to `to`. Refuses a move to the same name: the
    /// one place that rule is held.
    pub fn new(space: Space, from: ArtifactName, to: ArtifactName) -> Result<Self, MovedProblem> {
        if from == to {
            return Err(MovedProblem::SameName { name: from });
        }
        Ok(Self { space, from, to })
    }

    /// The space both names are of.
    pub fn space(&self) -> &Space {
        &self.space
    }

    /// The old name, in its space.
    pub fn from(&self) -> ArtifactKey {
        ArtifactKey::new(self.space.clone(), self.from.clone())
    }

    /// The new name, in its space.
    pub fn to(&self) -> ArtifactKey {
        ArtifactKey::new(self.space.clone(), self.to.clone())
    }

    /// The old name.
    pub fn from_name(&self) -> &ArtifactName {
        &self.from
    }

    /// The new name.
    pub fn to_name(&self) -> &ArtifactName {
        &self.to
    }
}

/// Why a declaration of a move cannot be made as given.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum MovedProblem {
    /// The two names are one: a move to where it is.
    #[error("`{name}` is moved to itself")]
    SameName {
        /// The name.
        name: ArtifactName,
    },
}

/// A person's declaration that an artifact moved from one name to another.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Moved {
    pub(super) id: JudgementId,
    pub(super) of: MovedOf,
    pub(super) actor: Actor,
    pub(super) at: JudgedAt,
    pub(super) reason: NonBlank,
}

impl Moved {
    /// A declaration of a move ([`MovedOf::new`], or what [`frame_moved`] framed). The
    /// reason is required: a declaration is a claim a reader must be able to check.
    pub fn new(id: JudgementId, of: MovedOf, actor: Actor, at: JudgedAt, reason: NonBlank) -> Self {
        Self {
            id,
            of,
            actor,
            at,
            reason,
        }
    }

    /// The identity of the record.
    pub fn id(&self) -> JudgementId {
        self.id
    }

    /// What moved, and where to.
    pub fn of(&self) -> &MovedOf {
        &self.of
    }

    /// Who declared it.
    pub fn actor(&self) -> &Actor {
        &self.actor
    }

    /// When.
    pub fn at(&self) -> JudgedAt {
        self.at
    }

    /// Why.
    pub fn reason(&self) -> &NonBlank {
        &self.reason
    }
}

/// The way a pin is read through the moves declared after it was made: the name it
/// ends at, the declarations it went through, and the name each of them moved from.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct MovedTo {
    artifact: ArtifactKey,
    /// Each step of the way, in order: the declaration and the name it moved from
    /// (the first is the name pinned).
    steps: NonEmptyVec<(JudgementId, ArtifactKey)>,
}

impl MovedTo {
    /// The name it is read as, in its space: where the way ends.
    pub fn artifact(&self) -> &ArtifactKey {
        &self.artifact
    }

    /// The declarations it was read through, in order.
    pub fn declared(&self) -> NonEmptyVec<JudgementId> {
        self.steps.map(|(id, _)| *id)
    }

    /// The names the way moved from, in order: the name pinned first, then each name
    /// a chain passed.
    pub fn moved_from(&self) -> impl Iterator<Item = &ArtifactKey> {
        self.steps.iter().map(|(_, from)| from)
    }
}

/// A pin of an artifact as the ledger reads it now: the pin as the record keeps it,
/// and the moves it is read through, when people declared some after it was made.
///
/// Two values, kept apart: the [`ArtifactPin`] is what was pinned (what a record
/// writes and what [`repeated`](super::repeated) compares), and the reading is
/// derived from the ledger on every assessment, never stored. Made by
/// [`Ledger::read_pins`] alone.
///
/// [`Ledger::read_pins`]: super::Ledger::read_pins
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ReadPin {
    pub(super) pin: ArtifactPin,
    pub(super) moved: Option<MovedTo>,
}

impl ReadPin {
    /// The pin, as the record keeps it: the name it wrote, what was there, the
    /// comparison.
    pub fn pin(&self) -> &ArtifactPin {
        &self.pin
    }

    /// The space and the name the pin is read as now: where the way of declared moves
    /// ends, else the one it wrote. What freshness compares with what is there now.
    pub fn read_as(&self) -> &ArtifactKey {
        self.moved
            .as_ref()
            .map_or(self.pin.artifact(), MovedTo::artifact)
    }

    /// The moves it is read through; none when no move was declared of it since.
    pub fn moved(&self) -> Option<&MovedTo> {
        self.moved.as_ref()
    }

    /// The name it is read as, when that is not the name it wrote: the name a link
    /// is to write now. None when it is read as written (no move, or a way back).
    pub fn moved_away(&self) -> Option<&ArtifactKey> {
        Some(self.read_as()).filter(|name| *name != self.pin.artifact())
    }

    /// The names its way left: each name a move took it from that is not the name it
    /// is read as now. A link that writes one is behind the move.
    pub fn left_behind(&self) -> impl Iterator<Item = &ArtifactKey> {
        let now = self.read_as();
        let mut seen = BTreeSet::new();
        self.moved
            .iter()
            .flat_map(MovedTo::moved_from)
            .filter(move |name| *name != now && seen.insert(*name))
    }
}

/// The way the name `artifact`, pinned at `made`, is read through `moves`: from the
/// name it has come to, the next move of its space declared at or after the step
/// before (at or after `made` for the first), the earliest, and of those at one time
/// the one of the larger identity; each move is taken once. None when no move
/// applies.
///
/// The rule of [`Ledger::moved_to`], kept apart so it is the one place a move is
/// followed.
pub(super) fn moved_way<'a>(
    moves: impl IntoIterator<Item = &'a Moved>,
    artifact: &ArtifactKey,
    made: JudgedAt,
) -> Option<MovedTo> {
    let space = artifact.space();
    let mut open: Vec<&Moved> = moves
        .into_iter()
        .filter(|m| m.of.space == *space && m.at >= made)
        .collect();
    let mut name = artifact.name().clone();
    let mut since = made;
    let mut steps: Option<NonEmptyVec<(JudgementId, ArtifactKey)>> = None;
    while let Some(next) = open
        .iter()
        .enumerate()
        .filter(|(_, m)| m.of.from == name && m.at >= since)
        .min_by_key(|(_, m)| (m.at, Reverse(m.id)))
        .map(|(i, _)| i)
    {
        let m = open.swap_remove(next);
        let step = (m.id, ArtifactKey::new(space.clone(), name));
        match &mut steps {
            Some(steps) => steps.push(step),
            None => steps = Some(NonEmptyVec::new(step, Vec::new())),
        }
        name = m.of.to.clone();
        since = m.at;
    }
    steps.map(|steps| MovedTo {
        artifact: ArtifactKey::new(space.clone(), name),
        steps,
    })
}

/// Why a declaration of a move is refused.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum MovedRefusal {
    /// What is declared is no move ([`MovedOf::new`]).
    #[error("the declaration is no move")]
    NotAMove(#[from] MovedProblem),
    /// No pin of an approval that stands, that pinned something there, is read as
    /// the old name now (through the moves already declared), so which space it is of
    /// cannot be told, and no approval would read the move. A name only a document's
    /// link writes, or only pinned as absent, is not known: nothing there was pinned.
    #[error("no approval that stands has a pin of something there read as `{name}` now")]
    NotKnown {
        /// The old name.
        name: ArtifactName,
    },
    /// Pins are read as the old name in more than one space, so which artifact is
    /// meant cannot be told.
    #[error("pins are read as `{name}` in more than one space: {spaces:?}")]
    InSpaces {
        /// The old name.
        name: ArtifactName,
        /// The spaces.
        spaces: Vec<Space>,
    },
    /// Something is still at the old name: a copy, or no move yet.
    #[error("{artifact} is still there: a copy is no move")]
    FromPresent {
        /// The old name.
        artifact: ArtifactKey,
    },
    /// The adapter cannot tell about the old name now.
    #[error("{artifact} cannot be told about now: {why}")]
    FromUntold {
        /// The old name.
        artifact: ArtifactKey,
        /// Why.
        why: Untold,
    },
    /// Nothing is at the new name.
    #[error("{artifact} has nothing there now")]
    ToAbsent {
        /// The new name.
        artifact: ArtifactKey,
    },
    /// The adapter cannot tell about the new name now.
    #[error("{artifact} cannot be told about now: {why}")]
    ToUntold {
        /// The new name.
        artifact: ArtifactKey,
        /// Why.
        why: Untold,
    },
}

/// The space a move of `from` is declared in: the one space in which a pin of an
/// approval that stands, that pinned something there, is read as `from` now,
/// through the moves already declared. A move is of something that was there: a pin
/// of a name with nothing behind it is read as written and makes no name known.
///
/// So a name is moved on from where its pins are now: once `a` moved to `b`, no pin
/// is read as `a`, and a second move of `a` (a copy in two steps) is refused, while a
/// move of `b` back to `a` is not. A name only a document's link writes is
/// [`MovedRefusal::NotKnown`]: nothing pinned it, and no approval would read the move.
/// Refused too: pins read as `from` in more than one space.
pub fn moved_space(ledger: &Ledger, from: &ArtifactName) -> Result<Space, MovedRefusal> {
    let mut spaces: BTreeSet<Space> = BTreeSet::new();
    for subject in ledger.subjects() {
        spaces.extend(
            ledger
                .read_pins(*subject)
                .iter()
                .filter(|pin| matches!(pin.pin().seen(), PinnedArtifact::Present { .. }))
                .filter(|pin| pin.read_as().name() == from)
                .map(|pin| pin.read_as().space().clone()),
        );
    }
    let mut spaces = spaces.into_iter();
    match (spaces.next(), spaces.next()) {
        (None, _) => Err(MovedRefusal::NotKnown { name: from.clone() }),
        (Some(space), None) => Ok(space),
        (Some(first), Some(second)) => Err(MovedRefusal::InSpaces {
            name: from.clone(),
            spaces: [first, second].into_iter().chain(spaces).collect(),
        }),
    }
}

/// Whether the move `of` stands, given what the space's adapter answered for both
/// names now (`readings`, asked by the application).
///
/// The rule: nothing is at the old name and something is at the new one. Refused: a
/// thing still at the old name (a copy is no move), nothing at the new one, and either
/// that the adapter cannot tell about now (or did not answer for, or says is no name
/// of its space). The content is not compared: a move with an edit is a move, and
/// freshness tells the edit as a change at the new name.
pub fn frame_moved(of: MovedOf, readings: &ArtifactReadings) -> Result<MovedOf, MovedRefusal> {
    let old = of.from();
    let new = of.to();
    match readings.of(&old) {
        Some(reading) if reading.is_present() => {
            return Err(MovedRefusal::FromPresent { artifact: old });
        }
        _ => match readings.state_of(&old, &Comparison::Bytes) {
            ArtifactState::Absent => {}
            // A reading that is there is answered above; one with no hash under
            // `bytes` is there too.
            ArtifactState::Present(_) => return Err(MovedRefusal::FromPresent { artifact: old }),
            ArtifactState::Unknown(why) => {
                return Err(MovedRefusal::FromUntold { artifact: old, why });
            }
        },
    }
    match readings.of(&new) {
        Some(reading) if reading.is_present() => {}
        _ => match readings.state_of(&new, &Comparison::Bytes) {
            ArtifactState::Absent => return Err(MovedRefusal::ToAbsent { artifact: new }),
            ArtifactState::Present(_) => {}
            ArtifactState::Unknown(why) => {
                return Err(MovedRefusal::ToUntold { artifact: new, why });
            }
        },
    }
    Ok(of)
}
