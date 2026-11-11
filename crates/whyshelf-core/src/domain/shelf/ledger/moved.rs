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
//! The names a move may be declared of are one rule, [`PinnedNames::of`]: the names
//! pins of something there, of approvals that stand, are read as now. `moved_space`
//! reads the space of an old name from it, and `moves` (which writes nothing) asks
//! the same way which of them have nothing behind them now, and what each space's
//! adapter suggests they moved to:
//!
//! ```text
//!   PinnedNames::of(ledger) --asks--> ArtifactSource::resolve (one reading)
//!        |                                  | readings
//!        +--gone (nothing there, as frame_moved reads an old name)
//!                  |  GoneNames, by space
//!                  +--> each space's adapter, once ---> SpaceSuggestions
//!                  +--suggest--> MoveSuggestions (every gone name, its candidates in
//!                                one order; the spaces that cannot suggest)
//! ```
//!
//! [`Ledger::moved_to`]: super::Ledger::moved_to

use std::cmp::Reverse;
use std::collections::{BTreeMap, BTreeSet};

use thiserror::Error;

use crate::domain::actor::Actor;
use crate::domain::id::DocId;
use crate::domain::nonempty::{NonBlank, NonEmptyVec};

use super::super::artifact::{
    ArtifactAsks, ArtifactHash, ArtifactKey, ArtifactName, ArtifactReadings, ArtifactState,
    Comparison, ContentAddress, Revision, Space, Untold,
};
use super::{ArtifactPin, DecisionVerb, JudgedAt, JudgementId, Ledger, PinnedArtifact};

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
    let spaces: BTreeSet<Space> = PinnedNames::of(ledger)
        .0
        .into_keys()
        .filter(|artifact| artifact.name() == from)
        .map(|artifact| artifact.space().clone())
        .collect();
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
    match now_at(readings, &old) {
        Now::Nothing => {}
        Now::There => return Err(MovedRefusal::FromPresent { artifact: old }),
        Now::Untold(why) => return Err(MovedRefusal::FromUntold { artifact: old, why }),
    }
    match now_at(readings, &new) {
        Now::There => {}
        Now::Nothing => return Err(MovedRefusal::ToAbsent { artifact: new }),
        Now::Untold(why) => return Err(MovedRefusal::ToUntold { artifact: new, why }),
    }
    Ok(of)
}

/// What the adapter answered is at one name now, for a move: something, nothing, or
/// it cannot be told.
enum Now {
    There,
    Nothing,
    Untold(Untold),
}

/// The one reading of an answer for a move ([`frame_moved`], [`PinnedNames::gone`]):
/// a reading that is there is there, whatever its comparisons (one with no hash
/// under `bytes` is there too); else what the answer is under `bytes`, and a name
/// with no answer cannot be told. Nothing is read as nothing that the adapter did
/// not say is nothing.
fn now_at(readings: &ArtifactReadings, artifact: &ArtifactKey) -> Now {
    match readings.of(artifact) {
        Some(reading) if reading.is_present() => Now::There,
        _ => match readings.state_of(artifact, &Comparison::Bytes) {
            ArtifactState::Absent => Now::Nothing,
            ArtifactState::Present(_) => Now::There,
            ArtifactState::Unknown(why) => Now::Untold(why),
        },
    }
}

/// What one approval that stands pinned of something there, at a name a pin is read
/// as now: what an adapter may look for the thing by, when the name has nothing
/// behind it any more.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PinnedThere {
    pub(super) subject: DocId,
    /// The name the pin wrote (the name read as is the key of its [`PinnedNames`]).
    pub(super) written: ArtifactKey,
    pub(super) comparison: Comparison,
    pub(super) hash: ArtifactHash,
    pub(super) address: Option<ContentAddress>,
    pub(super) revision: Option<Revision>,
}

impl PinnedThere {
    /// The document the approval is about.
    pub fn subject(&self) -> DocId {
        self.subject
    }

    /// The name the pin wrote, before the moves declared since: the name the thing
    /// had at [`PinnedThere::revision`].
    pub fn written(&self) -> &ArtifactKey {
        &self.written
    }

    /// How the hash was made.
    pub fn comparison(&self) -> &Comparison {
        &self.comparison
    }

    /// The hash of what was there, under [`PinnedThere::comparison`].
    pub fn hash(&self) -> &ArtifactHash {
        &self.hash
    }

    /// Where the content, as it was pinned, can be read back, when that was said.
    pub fn address(&self) -> Option<&ContentAddress> {
        self.address.as_ref()
    }

    /// The point of the project's history the approval was made at, when it kept
    /// one (rule A5).
    pub fn revision(&self) -> Option<&Revision> {
        self.revision.as_ref()
    }

    /// The hash a thing of the same content would have now, when the comparison
    /// counts any content: none under [`Comparison::Presence`], whose one hash every
    /// thing that is there shares.
    pub fn content(&self) -> Option<(&Comparison, &ArtifactHash)> {
        match self.comparison {
            Comparison::Presence => None,
            Comparison::Bytes | Comparison::Lines { .. } => Some((&self.comparison, &self.hash)),
        }
    }
}

/// The names a move may be declared of, each with what pinned it: every name a pin
/// of an approval that stands, one that pinned something there, is read as now,
/// through the moves already declared.
///
/// The one rule of "a name a move may be declared of": [`moved_space`] reads the
/// space of an old name from it, and `moves` asks which of its names have nothing
/// behind them now ([`PinnedNames::gone`]). A pin of a name as absent held no content
/// to move, and is in none of them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PinnedNames(BTreeMap<ArtifactKey, NonEmptyVec<PinnedThere>>);

impl PinnedNames {
    /// The names of `ledger`'s approvals that stand, each with its pins in the
    /// order of the documents and then of the record.
    pub fn of(ledger: &Ledger) -> Self {
        let mut names: BTreeMap<ArtifactKey, NonEmptyVec<PinnedThere>> = BTreeMap::new();
        for subject in ledger.subjects() {
            let revision = match ledger.standing_decision(*subject).map(|d| &d.verb) {
                Some(DecisionVerb::Approve { revision, .. }) => revision.clone(),
                _ => None,
            };
            for read in ledger.read_pins(*subject) {
                let PinnedArtifact::Present { hash, address } = read.pin().seen() else {
                    continue;
                };
                let there = PinnedThere {
                    subject: *subject,
                    written: read.pin().artifact().clone(),
                    comparison: read.pin().comparison().clone(),
                    hash: *hash,
                    address: address.clone(),
                    revision: revision.clone(),
                };
                match names.get_mut(read.read_as()) {
                    Some(pins) => pins.push(there),
                    None => {
                        names.insert(read.read_as().clone(), NonEmptyVec::new(there, Vec::new()));
                    }
                }
            }
        }
        Self(names)
    }

    /// What to ask the adapters about: each name, whether it has anything behind it
    /// now.
    pub fn asks(&self) -> ArtifactAsks {
        let mut asks = ArtifactAsks::new();
        for artifact in self.0.keys() {
            asks.ask_name(artifact.space().clone(), artifact.name().clone());
        }
        asks
    }

    /// The names, by their space.
    pub fn names(&self) -> impl Iterator<Item = (&ArtifactKey, &NonEmptyVec<PinnedThere>)> {
        self.0.iter()
    }

    /// Of these names, those the adapter answered (in `readings`, asked with
    /// [`PinnedNames::asks`]) have nothing behind them now, as [`frame_moved`] reads
    /// an old name, by space and name. A name that cannot be told now is not among
    /// them. A name pins are read as in more than one space is among them, with
    /// those spaces ([`GoneName::in_spaces`]): [`moved_space`] refuses a move of it,
    /// so no adapter is asked about it.
    pub fn gone(self, readings: &ArtifactReadings) -> GoneNames {
        let mut spaces_of: BTreeMap<&ArtifactName, BTreeSet<&Space>> = BTreeMap::new();
        for artifact in self.0.keys() {
            spaces_of
                .entry(artifact.name())
                .or_default()
                .insert(artifact.space());
        }
        let in_spaces: BTreeMap<ArtifactName, BTreeSet<Space>> = spaces_of
            .into_iter()
            .filter(|(_, spaces)| spaces.len() > 1)
            .map(|(name, spaces)| (name.clone(), spaces.into_iter().cloned().collect()))
            .collect();
        let mut gone: BTreeMap<Space, Vec<GoneName>> = BTreeMap::new();
        for (artifact, pins) in self.0 {
            if matches!(now_at(readings, &artifact), Now::Nothing) {
                let in_spaces = in_spaces.get(artifact.name()).cloned().unwrap_or_default();
                gone.entry(artifact.space().clone())
                    .or_default()
                    .push(GoneName {
                        artifact,
                        pins,
                        in_spaces,
                    });
            }
        }
        GoneNames(gone)
    }
}

/// A name a move may be declared of, with nothing behind it now, and what pinned it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GoneName {
    artifact: ArtifactKey,
    pins: NonEmptyVec<PinnedThere>,
    /// The spaces pins are read as the name in, when they are more than one; empty
    /// otherwise.
    in_spaces: BTreeSet<Space>,
}

impl GoneName {
    /// The spaces pins are read as its name in, when they are more than one (a move
    /// of it is refused as [`MovedRefusal::InSpaces`]); empty when it is one.
    pub fn in_spaces(&self) -> &BTreeSet<Space> {
        &self.in_spaces
    }

    /// The name, in its space.
    pub fn artifact(&self) -> &ArtifactKey {
        &self.artifact
    }

    /// The name.
    pub fn name(&self) -> &ArtifactName {
        self.artifact.name()
    }

    /// What each approval that stands pinned there, in the order of the documents.
    pub fn pins(&self) -> &NonEmptyVec<PinnedThere> {
        &self.pins
    }

    /// The documents whose approvals pin it, each once, in the order of identity.
    pub fn pinned_by(&self) -> BTreeSet<DocId> {
        self.pins.iter().map(PinnedThere::subject).collect()
    }
}

/// The names of [`PinnedNames::gone`], by space, each space's in the order of the
/// names.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct GoneNames(BTreeMap<Space, Vec<GoneName>>);

impl GoneNames {
    /// Each space with the gone names its adapter is asked to suggest new names for,
    /// once, in the order of the spaces: all but those pinned in more than one space
    /// ([`GoneName::in_spaces`]). A space with none such is not among them.
    pub fn spaces(&self) -> impl Iterator<Item = (&Space, Vec<GoneName>)> {
        self.0.iter().filter_map(|(space, names)| {
            let asked: Vec<GoneName> = names
                .iter()
                .filter(|n| n.in_spaces.is_empty())
                .cloned()
                .collect();
            (!asked.is_empty()).then_some((space, asked))
        })
    }

    /// Whether no name is gone.
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// The suggestions, from what each space's adapter answered (`answers`; a space
    /// asked that has no answer cannot suggest). Every gone name is in the answer,
    /// with the candidates its space's adapter gave and none for a space that cannot
    /// suggest; a candidate that is the name itself or another gone name of its
    /// space is left out, two of one name are one ([`FoundBy::SameContent`] over
    /// [`FoundBy::Renamed`], the larger similarity over the smaller), and they are in
    /// the order [`MoveCandidate`]'s page gives. A name pinned in more than one space
    /// has no candidate and the note [`NoteKind::InTwoSpaces`], so nothing is
    /// suggested that `judge moved` refuses. The notes of a name are in the order of
    /// their kinds, each kind once.
    pub fn suggest(self, mut answers: BTreeMap<Space, SpaceSuggestions>) -> MoveSuggestions {
        let mut absent = Vec::new();
        let mut cannot_suggest = BTreeSet::new();
        for (space, names) in self.0 {
            let gone: BTreeSet<ArtifactName> = names.iter().map(|n| n.name().clone()).collect();
            let asked = names.iter().any(|n| n.in_spaces.is_empty());
            let mut answered = match answers.remove(&space) {
                Some(SpaceSuggestions::Suggested(answered)) => answered,
                Some(SpaceSuggestions::CannotSuggest) | None => {
                    if asked {
                        cannot_suggest.insert(space.clone());
                    }
                    BTreeMap::new()
                }
            };
            for name in names {
                if !name.in_spaces.is_empty() {
                    let notes = vec![MoveNote::InTwoSpaces {
                        spaces: name.in_spaces.clone(),
                    }];
                    absent.push(MoveSuggestion {
                        gone: name,
                        candidates: Vec::new(),
                        notes,
                    });
                    continue;
                }
                let suggested = answered.remove(name.name()).unwrap_or_default();
                let mut kept: BTreeMap<ArtifactName, MoveCandidate> = BTreeMap::new();
                for candidate in suggested.candidates {
                    if gone.contains(&candidate.name) {
                        continue;
                    }
                    match kept.get(&candidate.name) {
                        Some(earlier) if earlier.rank() <= candidate.rank() => {}
                        _ => {
                            kept.insert(candidate.name.clone(), candidate);
                        }
                    }
                }
                let mut candidates: Vec<MoveCandidate> = kept.into_values().collect();
                candidates.sort_by(|a, b| (a.rank(), &a.name).cmp(&(b.rank(), &b.name)));
                let mut notes = suggested.notes;
                notes.sort_by_key(MoveNote::kind);
                notes.dedup_by_key(|n| n.kind());
                absent.push(MoveSuggestion {
                    gone: name,
                    candidates,
                    notes,
                });
            }
        }
        MoveSuggestions {
            absent,
            cannot_suggest,
        }
    }
}

/// How alike two contents are, as a whole percent: what a rename detection gives.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Similarity(u8);

impl Similarity {
    /// A percent, from 0 to 100; none above.
    pub fn new(percent: u8) -> Option<Self> {
        (percent <= 100).then_some(Self(percent))
    }

    /// The percent.
    pub fn percent(self) -> u8 {
        self.0
    }
}

/// How an adapter found a name a gone artifact may have moved to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum FoundBy {
    /// A thing there now has the content that was pinned: its hash under the pin's
    /// comparison is the hash pinned.
    SameContent,
    /// The space's history pairs the old name with this one, as a rename since the
    /// approval, with how alike the two are when it says.
    Renamed {
        /// How alike, when the history says.
        similarity: Option<Similarity>,
    },
}

/// A name a gone artifact may have moved to, as its space's adapter found it.
///
/// Candidates are in one order: [`FoundBy::SameContent`] first, then
/// [`FoundBy::Renamed`] from the most alike to the least (one with no similarity
/// last); of one rank, by name. A candidate is a suggestion only: `judge moved`
/// holds the move against what is there when it is declared.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct MoveCandidate {
    name: ArtifactName,
    found: FoundBy,
    present: bool,
}

impl MoveCandidate {
    /// A candidate at `name`, found so, with whether something is there now.
    pub fn new(name: ArtifactName, found: FoundBy, present: bool) -> Self {
        Self {
            name,
            found,
            present,
        }
    }

    /// The new name.
    pub fn name(&self) -> &ArtifactName {
        &self.name
    }

    /// How it was found.
    pub fn found(&self) -> FoundBy {
        self.found
    }

    /// Whether something is at the new name now, as the adapter says.
    pub fn present(&self) -> bool {
        self.present
    }

    /// Its place in the order: smaller first.
    fn rank(&self) -> (u8, Reverse<Option<Similarity>>) {
        match self.found {
            FoundBy::SameContent => (0, Reverse(None)),
            FoundBy::Renamed { similarity } => (1, Reverse(similarity)),
        }
    }
}

/// What could not be looked at for a gone name, or why nothing is suggested for it:
/// a closed set of kinds, each with what it needs to be told.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MoveNote {
    /// The space keeps no history the adapter can read here (for files, the project
    /// is in no repository): renames were not looked for.
    NoRepository,
    /// No approval that pins the name kept a revision: renames were not looked for.
    NoRevision,
    /// These revisions, kept by approvals that pin it, are not in the history: no
    /// rename since them was looked for.
    UnknownRevision {
        /// The revisions.
        revisions: BTreeSet<Revision>,
    },
    /// The history could not be read, for this reason: renames were not looked for.
    HistoryUnreadable {
        /// What the adapter said.
        detail: NonBlank,
    },
    /// The search for the same content stopped at its bound, after reading this
    /// many things.
    ScanTruncated {
        /// How many were read.
        read: usize,
    },
    /// This many things could not be read, and were passed over by the search for
    /// the same content.
    UnreadableSkipped {
        /// How many.
        count: usize,
    },
    /// This many things were larger than the bound of one reading, in bytes, and were
    /// not read.
    TooLargeSkipped {
        /// How many.
        count: usize,
        /// The bound.
        limit: u64,
    },
    /// Pins are read as the name in these spaces, more than one: `judge moved`
    /// refuses a move of it, so nothing is suggested.
    InTwoSpaces {
        /// The spaces.
        spaces: BTreeSet<Space>,
    },
}

/// The kind of a [`MoveNote`], its place in the order of a name's notes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum NoteKind {
    /// [`MoveNote::InTwoSpaces`].
    InTwoSpaces,
    /// [`MoveNote::NoRepository`].
    NoRepository,
    /// [`MoveNote::NoRevision`].
    NoRevision,
    /// [`MoveNote::UnknownRevision`].
    UnknownRevision,
    /// [`MoveNote::HistoryUnreadable`].
    HistoryUnreadable,
    /// [`MoveNote::ScanTruncated`].
    ScanTruncated,
    /// [`MoveNote::UnreadableSkipped`].
    UnreadableSkipped,
    /// [`MoveNote::TooLargeSkipped`].
    TooLargeSkipped,
}

impl MoveNote {
    /// Its kind.
    pub fn kind(&self) -> NoteKind {
        match self {
            Self::NoRepository => NoteKind::NoRepository,
            Self::NoRevision => NoteKind::NoRevision,
            Self::UnknownRevision { .. } => NoteKind::UnknownRevision,
            Self::HistoryUnreadable { .. } => NoteKind::HistoryUnreadable,
            Self::ScanTruncated { .. } => NoteKind::ScanTruncated,
            Self::UnreadableSkipped { .. } => NoteKind::UnreadableSkipped,
            Self::TooLargeSkipped { .. } => NoteKind::TooLargeSkipped,
            Self::InTwoSpaces { .. } => NoteKind::InTwoSpaces,
        }
    }
}

/// What a space's adapter suggests for one gone name: the candidates, and notes of
/// what it could not look at.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct NameSuggestions {
    /// The candidates, in any order.
    pub candidates: Vec<MoveCandidate>,
    /// What could not be looked at, each kind once.
    pub notes: Vec<MoveNote>,
}

/// What a space's adapter answers when asked to suggest new names for its gone
/// names.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SpaceSuggestions {
    /// The suggestions, by the gone name; a name with no entry has no candidate.
    Suggested(BTreeMap<ArtifactName, NameSuggestions>),
    /// The space has no way to suggest.
    CannotSuggest,
}

/// One gone name with what its space's adapter suggests.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MoveSuggestion {
    gone: GoneName,
    candidates: Vec<MoveCandidate>,
    notes: Vec<MoveNote>,
}

impl MoveSuggestion {
    /// The gone name and what pinned it.
    pub fn gone(&self) -> &GoneName {
        &self.gone
    }

    /// The candidates, in their order.
    pub fn candidates(&self) -> &[MoveCandidate] {
        &self.candidates
    }

    /// What could not be looked at, or why nothing is suggested, in the order of
    /// their kinds, each kind once.
    pub fn notes(&self) -> &[MoveNote] {
        &self.notes
    }
}

/// The answer of `moves`: every gone name, by space and then by name, and the spaces
/// that cannot suggest.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct MoveSuggestions {
    /// The gone names, each with its candidates.
    pub absent: Vec<MoveSuggestion>,
    /// The spaces of gone names whose adapter has no way to suggest.
    pub cannot_suggest: BTreeSet<Space>,
}
