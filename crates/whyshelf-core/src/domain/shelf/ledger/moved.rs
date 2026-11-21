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
//! pins of something there, of approvals that stand, are read as now.
//! [`PinnedNames::space_of`] reads the space of an old name from it (and
//! `moved_space`, [`PinnedNames::gone`] and [`plan_prefix_moves`] ask it, so the
//! rule of "which spaces a name is known in" is held once), and `moves` (which writes
//! nothing) asks the same way which of them have nothing behind them now, and what
//! each space's adapter suggests they moved to:
//!
//! ```text
//!   PinnedNames::of(ledger) --asks--> ArtifactSource::resolve (one reading)
//!        |                                  | readings
//!        +--gone (nothing there, as frame_moved reads an old name)
//!                  |  GoneNames, by space
//!                  +--> each space's adapter, once ---> SpaceSuggestions
//!                  +--suggest--> MoveSuggestions (every gone name, its candidates in
//!                                one order; the spaces that cannot suggest)
//!                                   |  prefix_trials: the prefix moves candidates
//!                                   |  carry that several gone names share
//!                                   +--> plan_prefix_moves, resolve, frame_prefix_moves
//!                                   |    (what `judge moved --prefix` does, one move
//!                                   |    at a time) ---> offer: a PrefixSuggestion if
//!                                   |    it stands, else a RefusedPrefix
//! ```
//!
//! A person who moved a directory or renamed a module declares it once, for every name
//! under a prefix ([`PrefixMove`](super::super::artifact::PrefixMove)). The domain
//! knows no boundary of a name and builds no new one: each space's adapter says which
//! names are under the prefix and what each is under the new one
//! ([`Renamed`]); the domain decides which of them move, and holds every move
//! against what is there, collecting the refusals of both steps:
//!
//! ```text
//!   PinnedNames::prefix_asks --> ArtifactSource::renamed (per space, its own grammar)
//!        |                                | PrefixAnswers (Renamed per name, SpacePrefixes per space)
//!        +--plan_prefix_moves-------------+--> PrefixPlan (moves, refused, not_asked)
//!                                  |            or PrefixRefusal::NothingUnder
//!                                  +--ArtifactSource::resolve (one reading, every old
//!                                  |  and new name)
//!                                  +--frame_prefix_moves (frame_moved for each) -->
//!                                       every move, or PrefixRefusal::Names (every name
//!                                       that cannot be moved)
//! ```
//!
//! [`Ledger::moved_to`]: super::Ledger::moved_to

use std::cmp::{Ordering, Reverse};
use std::collections::{BTreeMap, BTreeSet};

use thiserror::Error;

use crate::domain::actor::{Actor, Attribution};
use crate::domain::id::DocId;
use crate::domain::nonempty::{NonBlank, NonEmptyVec};
use crate::domain::words::{Join, Mark, listed};

use super::super::artifact::{
    ArtifactAsks, ArtifactHash, ArtifactKey, ArtifactName, ArtifactReadings, ArtifactState,
    Comparison, ContentAddress, NamePrefix, PrefixAnswers, PrefixAsks, PrefixMove, Renamed,
    Revision, Space, SpacePrefixes, Untold,
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
    pub(super) by: Attribution,
    pub(super) at: JudgedAt,
    pub(super) reason: NonBlank,
}

impl Moved {
    /// A declaration of a move ([`MovedOf::new`], or what [`frame_moved`] framed). The
    /// reason is required: a declaration is a claim a reader must be able to check.
    pub fn new(
        id: JudgementId,
        of: MovedOf,
        by: Attribution,
        at: JudgedAt,
        reason: NonBlank,
    ) -> Self {
        Self {
            id,
            of,
            by,
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
        self.by.actor()
    }

    /// Who declared it, and who ran the command when it was another.
    pub fn attribution(&self) -> &Attribution {
        &self.by
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
/// writes and what [`Readers`](super::Readers) compares), and the reading is
/// derived from the ledger on every assessment, never stored. Made by
/// [`Ledger::pins_read_now`] alone.
///
/// [`Ledger::pins_read_now`]: super::Ledger::pins_read_now
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
    #[error("pins are read as `{name}` in more than one space: {}", listed(.spaces.iter(), Mark::Backtick, Join::And))]
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
    PinnedNames::of(ledger).space_of(from)
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
            let revision = match ledger.standing_decision(*subject).map(|d| d.verb()) {
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

    /// The spaces a pin is read as each name in, by name: the one rule of "which
    /// spaces a name is known in", which [`PinnedNames::space_of`],
    /// [`PinnedNames::gone`] and [`plan_prefix_moves`] all ask. Made once for a
    /// question about many names, so that none of them scans every name again.
    fn spaces_by_name(&self) -> BTreeMap<&ArtifactName, BTreeSet<&Space>> {
        let mut spaces: BTreeMap<&ArtifactName, BTreeSet<&Space>> = BTreeMap::new();
        for artifact in self.0.keys() {
            spaces
                .entry(artifact.name())
                .or_default()
                .insert(artifact.space());
        }
        spaces
    }

    /// The space a move of `name` is declared in: the one space in which a pin is
    /// read as `name` now ([`moved_space`] is this over the names of a ledger). None
    /// is [`MovedRefusal::NotKnown`], and more than one [`MovedRefusal::InSpaces`]
    /// (which artifact is meant cannot be told).
    pub fn space_of(&self, name: &ArtifactName) -> Result<Space, MovedRefusal> {
        space_among(name, self.spaces_by_name().get(name))
    }

    /// What each space's adapter is asked about a
    /// [`PrefixMove`](super::super::artifact::PrefixMove): every name, by its
    /// space, and the adapter keeps only those under the prefix
    /// ([`Renamed::Outside`] for the others).
    pub fn prefix_asks(&self) -> PrefixAsks {
        let mut asks = PrefixAsks::new();
        for artifact in self.0.keys() {
            asks.ask(artifact.space().clone(), artifact.name().clone());
        }
        asks
    }

    /// Of these names, those the adapter answered (in `readings`, asked with
    /// [`PinnedNames::asks`]) have nothing behind them now, as [`frame_moved`] reads
    /// an old name, by space and name. A name that cannot be told now is not among
    /// them. A name pins are read as in more than one space is among them, with
    /// those spaces ([`GoneName::in_spaces`]): [`moved_space`] refuses a move of it,
    /// so no adapter is asked about it. The names stay: a move of a prefix is held
    /// against them after the suggestions are made.
    pub fn gone(&self, readings: &ArtifactReadings) -> GoneNames {
        let in_spaces: BTreeMap<ArtifactName, BTreeSet<Space>> = self
            .spaces_by_name()
            .into_iter()
            .filter(|(_, spaces)| spaces.len() > 1)
            .map(|(name, spaces)| (name.clone(), spaces.into_iter().cloned().collect()))
            .collect();
        let mut gone: BTreeMap<Space, Vec<GoneName>> = BTreeMap::new();
        for (artifact, pins) in &self.0 {
            if matches!(now_at(readings, artifact), Now::Nothing) {
                let in_spaces = in_spaces.get(artifact.name()).cloned().unwrap_or_default();
                gone.entry(artifact.space().clone())
                    .or_default()
                    .push(GoneName {
                        artifact: artifact.clone(),
                        pins: pins.clone(),
                        in_spaces,
                    });
            }
        }
        GoneNames(gone)
    }
}

/// The one space of `name` among the spaces a pin is read as it in
/// ([`PinnedNames::spaces_by_name`]): none is [`MovedRefusal::NotKnown`], more than
/// one [`MovedRefusal::InSpaces`].
fn space_among(
    name: &ArtifactName,
    spaces: Option<&BTreeSet<&Space>>,
) -> Result<Space, MovedRefusal> {
    let mut spaces = spaces.into_iter().flatten().map(|space| (*space).clone());
    match (spaces.next(), spaces.next()) {
        (None, _) => Err(MovedRefusal::NotKnown { name: name.clone() }),
        (Some(space), None) => Ok(space),
        (Some(first), Some(second)) => Err(MovedRefusal::InSpaces {
            name: name.clone(),
            spaces: [first, second].into_iter().chain(spaces).collect(),
        }),
    }
}

/// Why nothing of a space is under a prefix, for one space.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NothingWhy {
    /// The adapter read the prefix as one of its names, and no name pinned is under
    /// it.
    NoneUnder,
    /// The prefix is no prefix of the space's names, with why.
    NotUnder(NonBlank),
    /// No adapter of the space takes prefixes, so it cannot be told.
    NoPrefixes,
}

/// A name under a prefix that cannot be moved, with why.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[error("`{name}`: {why}")]
pub struct NamedRefusal {
    /// The old name.
    pub name: ArtifactName,
    /// Why.
    pub why: NamedWhy,
}

impl NamedRefusal {
    /// The space the refusal is about, when it is of one: the adapter's, or the one
    /// an artifact of the rule's refusal is in. None when it is of the name as such
    /// (`NotAMove`, `NotKnown`, pins in more than one space).
    pub fn space(&self) -> Option<&Space> {
        match &self.why {
            NamedWhy::Unbuildable { space, .. } | NamedWhy::NoAnswer { space } => Some(space),
            NamedWhy::Move(
                MovedRefusal::FromPresent { artifact }
                | MovedRefusal::FromUntold { artifact, .. }
                | MovedRefusal::ToAbsent { artifact }
                | MovedRefusal::ToUntold { artifact, .. },
            ) => Some(artifact.space()),
            NamedWhy::Move(
                MovedRefusal::NotAMove(_)
                | MovedRefusal::NotKnown { .. }
                | MovedRefusal::InSpaces { .. },
            ) => None,
        }
    }
}

/// Why one name under a prefix cannot be moved.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum NamedWhy {
    /// The rule of a move refused it ([`MovedRefusal`]).
    #[error("{0}")]
    Move(MovedRefusal),
    /// The adapter could build no name under the new prefix.
    #[error("no name of `{space}` is the same name under the new prefix: {why}")]
    Unbuildable {
        /// The space.
        space: Space,
        /// What the adapter said.
        why: NonBlank,
    },
    /// The adapter of the space gave no answer for the name, so whether it is under
    /// the prefix cannot be told (nothing is read as outside that the adapter did not
    /// say is).
    #[error(
        "the adapter of `{space}` gave no answer for it, so whether it is under the prefix cannot be told"
    )]
    NoAnswer {
        /// The space.
        space: Space,
    },
}

/// Why a move of a prefix is refused.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum PrefixRefusal {
    /// No name pinned is under the prefix in any space, with why for each space.
    #[error(
        "no name of an approval that stands is under `{prefix}`{}",
        nothing_under(.spaces)
    )]
    NothingUnder {
        /// The prefix.
        prefix: NamePrefix,
        /// Each space a name is pinned in, with why nothing of it is under.
        spaces: Vec<(Space, NothingWhy)>,
    },
    /// Names under the prefix that cannot be moved: every one is listed, and none is
    /// moved.
    #[error("{} name(s) under the prefix cannot be moved: {}", .0.len(), listed(.0.iter(), Mark::Plain, Join::Semicolon))]
    Names(NonEmptyVec<NamedRefusal>),
}

/// The spaces of a [`PrefixRefusal::NothingUnder`], in words (a leading `: `, or
/// nothing when no name is pinned at all).
fn nothing_under(spaces: &[(Space, NothingWhy)]) -> String {
    if spaces.is_empty() {
        return String::new();
    }
    let each: Vec<String> = spaces
        .iter()
        .map(|(space, why)| match why {
            NothingWhy::NoneUnder => format!("`{space}`: none of its names is"),
            NothingWhy::NotUnder(why) => format!("`{space}`: {why}"),
            NothingWhy::NoPrefixes => {
                format!("`{space}`: it takes no prefix, so it cannot be told")
            }
        })
        .collect();
    format!(": {}", each.join("; "))
}

/// The names a move of a prefix moves, and what it leaves, as [`plan_prefix_moves`]
/// decided from the adapters' answers: the moves still to be held against what is
/// there ([`frame_prefix_moves`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrefixPlan {
    /// The prefix the names are under now.
    pub prefix: NamePrefix,
    /// The moves, one per name, in the order of the spaces and then of the names.
    pub moves: Vec<MovedOf>,
    /// The names that cannot be moved, each with why.
    pub refused: Vec<NamedRefusal>,
    /// The spaces whose adapter takes no prefix: names of them may be under it, and
    /// are not moved. Told with the answer, never silently.
    pub not_asked: Vec<Space>,
}

/// Which names a [`PrefixMove`](super::super::artifact::PrefixMove) moves, from what each space's adapter answered
/// (`answers`, asked with [`PinnedNames::prefix_asks`]): each name under the prefix
/// ([`Renamed::Under`]) is a move to the name the adapter built, in the one space the
/// name is known in ([`PinnedNames::space_of`]; pins in two spaces refuse it).
///
/// Refused as a whole ([`PrefixRefusal::NothingUnder`]): nothing is under the prefix
/// in any space, neither a name to move nor one that cannot be moved. A space whose
/// adapter says the prefix is none of its names ([`SpacePrefixes::NotUnder`]) has no
/// name under it and is not in the answer; one with no adapter that takes a prefix
/// ([`SpacePrefixes::NoPrefixes`], or no answer) cannot be told, and is in
/// [`PrefixPlan::not_asked`]. A name the adapter was asked about and gave no answer
/// for cannot be moved, with that as why. Overlaps of the moves (`a` to `a/b` moves
/// a name onto the old name of another) are not refused here: held against what is
/// there at once, [`frame_prefix_moves`] refuses one of them.
// The refusal is the answer, read once per run; it is not a hot path.
#[allow(clippy::result_large_err)]
pub fn plan_prefix_moves(
    pinned: &PinnedNames,
    answers: &PrefixAnswers,
) -> Result<PrefixPlan, PrefixRefusal> {
    let spaces_by_name = pinned.spaces_by_name();
    let spaces: BTreeSet<&Space> = pinned.0.keys().map(ArtifactKey::space).collect();
    let mut moves = Vec::new();
    let mut refused: Vec<NamedRefusal> = Vec::new();
    let mut not_asked = Vec::new();
    let mut nothing = Vec::new();
    let refuse = |refused: &mut Vec<NamedRefusal>, name: &ArtifactName, why: NamedWhy| {
        let one = NamedRefusal {
            name: name.clone(),
            why,
        };
        if !refused.contains(&one) {
            refused.push(one);
        }
    };
    for space in spaces {
        match answers.prefixes_of(space) {
            Some(SpacePrefixes::Answered) => {
                let mut under = false;
                for artifact in pinned.0.keys().filter(|a| a.space() == space) {
                    let name = artifact.name();
                    match answers.renamed(space, name) {
                        Some(Renamed::Outside) => {}
                        Some(Renamed::Under(new)) => {
                            under = true;
                            match space_among(name, spaces_by_name.get(name)) {
                                Ok(of) => match MovedOf::new(of, name.clone(), new.clone()) {
                                    Ok(one) => moves.push(one),
                                    Err(problem) => refuse(
                                        &mut refused,
                                        name,
                                        NamedWhy::Move(MovedRefusal::from(problem)),
                                    ),
                                },
                                Err(why) => refuse(&mut refused, name, NamedWhy::Move(why)),
                            }
                        }
                        Some(Renamed::Unbuildable(why)) => {
                            under = true;
                            refuse(
                                &mut refused,
                                name,
                                NamedWhy::Unbuildable {
                                    space: space.clone(),
                                    why: why.clone(),
                                },
                            );
                        }
                        None => {
                            under = true;
                            refuse(
                                &mut refused,
                                name,
                                NamedWhy::NoAnswer {
                                    space: space.clone(),
                                },
                            );
                        }
                    }
                }
                if !under {
                    nothing.push((space.clone(), NothingWhy::NoneUnder));
                }
            }
            Some(SpacePrefixes::NotUnder(why)) => {
                nothing.push((space.clone(), NothingWhy::NotUnder(why.clone())));
            }
            Some(SpacePrefixes::NoPrefixes) | None => {
                not_asked.push(space.clone());
                nothing.push((space.clone(), NothingWhy::NoPrefixes));
            }
        }
    }
    let prefix = answers.of().from().clone();
    if moves.is_empty() && refused.is_empty() {
        return Err(PrefixRefusal::NothingUnder {
            prefix,
            spaces: nothing,
        });
    }
    Ok(PrefixPlan {
        prefix,
        moves,
        refused,
        not_asked,
    })
}

/// Whether the moves of a [`PrefixPlan`] stand, given what the adapters answered for
/// both names of each (`readings`): each is held against [`frame_moved`], and the
/// refusals of both steps (the plan's, and each move's own) are one answer
/// ([`PrefixRefusal::Names`]), so a person mends every name at once.
///
/// Two moves that overlap (a name that is the old name of one and the new name of
/// another) are held against one reading, where one of them has something at its old
/// name or nothing at its new one: [`frame_moved`] refuses it.
// The refusal is the answer, read once per run; it is not a hot path.
#[allow(clippy::result_large_err)]
pub fn frame_prefix_moves(
    plan: PrefixPlan,
    readings: &ArtifactReadings,
) -> Result<NonEmptyVec<MovedOf>, PrefixRefusal> {
    let PrefixPlan {
        prefix,
        moves,
        mut refused,
        not_asked: _,
    } = plan;
    let mut framed = Vec::with_capacity(moves.len());
    for of in moves {
        let name = of.from_name().clone();
        match frame_moved(of, readings) {
            Ok(of) => framed.push(of),
            Err(why) => refused.push(NamedRefusal {
                name,
                why: NamedWhy::Move(why),
            }),
        }
    }
    if let Ok(refused) = NonEmptyVec::from_vec(refused) {
        return Err(PrefixRefusal::Names(refused));
    }
    NonEmptyVec::from_vec(framed).map_err(|_| PrefixRefusal::NothingUnder {
        prefix,
        spaces: Vec::new(),
    })
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
    /// space is left out, two of one name are one (the one [`MoveCandidate::cmp_rank`]
    /// puts first, the earlier of two that rank alike: [`FoundBy::SameContent`] over
    /// [`FoundBy::Renamed`] over [`FoundBy::SameEnd`], the larger similarity over the
    /// smaller; it has the moves of a prefix both of the two had), and they are in
    /// the order [`MoveCandidate`]'s page gives. A name pinned in more than one space
    /// has no candidate and the note [`NoteKind::InTwoSpaces`], so nothing is
    /// suggested that `judge moved` refuses. The notes of a name are in the order of
    /// their kinds, each kind once. The moves of a prefix to try are
    /// [`MoveSuggestions::prefix_trials`]'s.
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
                    let kept_one = match kept.remove(&candidate.name) {
                        Some(earlier) => earlier.or_better(candidate),
                        None => candidate,
                    };
                    kept.insert(kept_one.name.clone(), kept_one);
                }
                let mut candidates: Vec<MoveCandidate> = kept.into_values().collect();
                candidates.sort_by(|a, b| a.cmp_rank(b).then_with(|| a.name.cmp(&b.name)));
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
        let mut suggestions = MoveSuggestions {
            absent,
            cannot_suggest,
            ..MoveSuggestions::default()
        };
        suggestions.untried_prefixes = suggestions
            .shared_prefixes()
            .len()
            .saturating_sub(MAX_PREFIX_TRIALS);
        suggestions
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
    /// The name ends as the gone one does (the space's own reading of an end), and
    /// nothing tells more: neither the content nor the history says it is the same
    /// thing. It is the form of a name only.
    SameEnd,
}

/// A name a gone artifact may have moved to, as its space's adapter found it.
///
/// Candidates are in one order ([`MoveCandidate::cmp_rank`]): [`FoundBy::SameContent`]
/// first, then [`FoundBy::Renamed`] from the most alike to the least (one with no
/// similarity last), then [`FoundBy::SameEnd`]; of one rank, by name. A candidate is a
/// suggestion only: `judge moved` holds the move against what is there when it is
/// declared. It may carry the moves of a prefix that take the gone name to it
/// ([`MoveCandidate::under`]), each of which `judge moved --prefix` may declare for
/// every name under the prefix at once.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct MoveCandidate {
    name: ArtifactName,
    found: FoundBy,
    present: bool,
    prefixes: Vec<PrefixMove>,
}

impl MoveCandidate {
    /// A candidate at `name`, found so, with whether something is there now. It
    /// carries no move of a prefix.
    pub fn new(name: ArtifactName, found: FoundBy, present: bool) -> Self {
        Self {
            name,
            found,
            present,
            prefixes: Vec::new(),
        }
    }

    /// This candidate, reached also by the move of `prefix`, after those it already
    /// carries (a move it carries is not added twice). More than one move of a prefix
    /// may lead from a gone name to one name: a module that keeps its name under
    /// another parent is moved by its parent, and by the module itself.
    #[must_use]
    pub fn under(mut self, prefix: PrefixMove) -> Self {
        if !self.prefixes.contains(&prefix) {
            self.prefixes.push(prefix);
        }
        self
    }

    /// The moves of a prefix that take the gone name to this one, in the order the
    /// adapter gave them; none when it found none.
    pub fn prefixes(&self) -> &[PrefixMove] {
        &self.prefixes
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
            FoundBy::SameEnd => (2, Reverse(None)),
        }
    }

    /// How this one stands to `other` in the order of candidates, by how each was
    /// found and not by name: the one place the order is held.
    pub fn cmp_rank(&self, other: &Self) -> Ordering {
        self.rank().cmp(&other.rank())
    }

    /// Of two candidates at one name, the one that ranks first (this one when they
    /// rank alike), with the moves of a prefix both had (this one's first, each
    /// once).
    fn or_better(self, other: Self) -> Self {
        let mut prefixes = self.prefixes.clone();
        for prefix in &other.prefixes {
            if !prefixes.contains(prefix) {
                prefixes.push(prefix.clone());
            }
        }
        let better = if other.cmp_rank(&self) == Ordering::Less {
            other
        } else {
            self
        };
        Self { prefixes, ..better }
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
    /// The space has nothing to search in here, for this reason (where its names are
    /// kept cannot be told): no name was looked for.
    SearchUnavailable {
        /// What the adapter said.
        detail: NonBlank,
    },
    /// The search stopped at its bound, after this many entries (the things the
    /// adapter looks at: for files, the files read; for the items of docs, the names
    /// found and the pages read).
    ScanTruncated {
        /// How many were counted.
        read: usize,
    },
    /// This many things (files, directories, pages) could not be read, and were
    /// passed over by the search.
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
    /// More names were found than are kept: the candidates are the first of this
    /// many, in the order of candidates.
    CandidatesCut {
        /// How many were found.
        found: usize,
    },
    /// Pins are read as the name in these spaces, more than one: `judge moved`
    /// refuses a move of it, so nothing is suggested.
    InTwoSpaces {
        /// The spaces.
        spaces: BTreeSet<Space>,
    },
    /// Every pin of the name compares its presence only (`compare = "presence"`),
    /// so there is no content to look for: it was not searched by its content, and
    /// only the space's history (for files, a rename Git detects) can suggest.
    NoContent,
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
    /// [`MoveNote::SearchUnavailable`].
    SearchUnavailable,
    /// [`MoveNote::ScanTruncated`].
    ScanTruncated,
    /// [`MoveNote::UnreadableSkipped`].
    UnreadableSkipped,
    /// [`MoveNote::TooLargeSkipped`].
    TooLargeSkipped,
    /// [`MoveNote::CandidatesCut`].
    CandidatesCut,
    /// [`MoveNote::NoContent`].
    NoContent,
}

impl MoveNote {
    /// Its kind.
    pub fn kind(&self) -> NoteKind {
        match self {
            Self::NoRepository => NoteKind::NoRepository,
            Self::NoRevision => NoteKind::NoRevision,
            Self::UnknownRevision { .. } => NoteKind::UnknownRevision,
            Self::HistoryUnreadable { .. } => NoteKind::HistoryUnreadable,
            Self::SearchUnavailable { .. } => NoteKind::SearchUnavailable,
            Self::ScanTruncated { .. } => NoteKind::ScanTruncated,
            Self::UnreadableSkipped { .. } => NoteKind::UnreadableSkipped,
            Self::TooLargeSkipped { .. } => NoteKind::TooLargeSkipped,
            Self::CandidatesCut { .. } => NoteKind::CandidatesCut,
            Self::InTwoSpaces { .. } => NoteKind::InTwoSpaces,
            Self::NoContent => NoteKind::NoContent,
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

/// The fewest gone names that must share the move of a prefix for it to be tried: the
/// one place that threshold is held ([`MoveSuggestions::prefix_trials`]).
const MIN_PREFIX_HOLDERS: usize = 2;

/// The most moves of a prefix tried in one answer; the rest are counted
/// ([`MoveSuggestions::untried_prefixes`]).
const MAX_PREFIX_TRIALS: usize = 32;

/// A move of a prefix that stands against what is there now, for these names
/// ([`frame_prefix_moves`] framed it; whether the actor is admitted to declare it is
/// not tried). Made by [`MoveSuggestions::offer`] alone.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrefixSuggestion {
    of: PrefixMove,
    moved: NonEmptyVec<ArtifactKey>,
    not_asked: Vec<Space>,
}

impl PrefixSuggestion {
    /// The move of the prefix.
    pub fn of(&self) -> &PrefixMove {
        &self.of
    }

    /// The names it moves, as they are now (the old names), in the order of the
    /// plan's moves.
    pub fn moved(&self) -> &NonEmptyVec<ArtifactKey> {
        &self.moved
    }

    /// The spaces pinned in whose adapter takes no prefix: names of them may be under
    /// it, and are not moved ([`PrefixPlan::not_asked`]).
    pub fn not_asked(&self) -> &[Space] {
        &self.not_asked
    }
}

/// A move of a prefix that candidates carry, and that does not stand against what is
/// there now: told, so that a person sees why it is not suggested. Made by
/// [`MoveSuggestions::offer`] alone.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RefusedPrefix {
    of: PrefixMove,
    refused: usize,
}

impl RefusedPrefix {
    /// The move of the prefix.
    pub fn of(&self) -> &PrefixMove {
        &self.of
    }

    /// How many names under the prefix cannot be moved.
    pub fn refused(&self) -> usize {
        self.refused
    }
}

/// The answer of `moves`: every gone name, by space and then by name, the spaces
/// that cannot suggest, and the moves of a prefix that candidates share, those that
/// stand and those that are refused.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct MoveSuggestions {
    /// The gone names, each with its candidates.
    pub absent: Vec<MoveSuggestion>,
    /// The spaces of gone names whose adapter has no way to suggest.
    pub cannot_suggest: BTreeSet<Space>,
    /// The moves of a prefix that stand, in the order they were offered
    /// ([`MoveSuggestions::offer`]).
    pub prefixes: Vec<PrefixSuggestion>,
    /// The moves of a prefix that were refused, in the order they were offered.
    pub refused_prefixes: Vec<RefusedPrefix>,
    /// How many moves of a prefix that candidates share were not tried, for the bound
    /// of one answer ([`MoveSuggestions::prefix_trials`]).
    pub untried_prefixes: usize,
}

impl MoveSuggestions {
    /// The moves of a prefix to try, as `judge moved --prefix` would take each: those
    /// that two gone names or more have a candidate carrying (so a prefix only one name
    /// carries is not tried: it is that one name's move), the one a candidate found
    /// the best way carries first (the order of candidates,
    /// [`MoveCandidate::cmp_rank`], of the best of those that carry it, whichever gone
    /// name holds it), then the one the most gone names have, then by the move; at most
    /// `MAX_PREFIX_TRIALS`, and the rest are counted in
    /// [`MoveSuggestions::untried_prefixes`]. The one place that threshold and that
    /// order are held.
    pub fn prefix_trials(&self) -> Vec<PrefixMove> {
        let mut shared = self.shared_prefixes();
        shared.truncate(MAX_PREFIX_TRIALS);
        shared
    }

    /// Every move of a prefix that [`MIN_PREFIX_HOLDERS`] gone names or more have a
    /// candidate carrying, in the order [`MoveSuggestions::prefix_trials`] tells. So a
    /// move that candidates of the same content carry is tried before moves of the end
    /// of a name only, however many names hold those, when the bound of one answer is
    /// reached. A gone name counts once for a move, however many of its candidates
    /// carry it, and for each move its candidates carry.
    fn shared_prefixes(&self) -> Vec<PrefixMove> {
        let mut holders: BTreeMap<&PrefixMove, (BTreeSet<&ArtifactKey>, &MoveCandidate)> =
            BTreeMap::new();
        for one in &self.absent {
            for candidate in &one.candidates {
                for prefix in candidate.prefixes() {
                    let (names, best) = holders
                        .entry(prefix)
                        .or_insert_with(|| (BTreeSet::new(), candidate));
                    names.insert(one.gone.artifact());
                    if candidate.cmp_rank(best) == Ordering::Less {
                        *best = candidate;
                    }
                }
            }
        }
        let mut shared: Vec<(&PrefixMove, usize, &MoveCandidate)> = holders
            .into_iter()
            .map(|(prefix, (names, best))| (prefix, names.len(), best))
            .filter(|(_, count, _)| *count >= MIN_PREFIX_HOLDERS)
            .collect();
        shared.sort_by(|(a, a_count, a_best), (b, b_count, b_best)| {
            a_best
                .cmp_rank(b_best)
                .then_with(|| b_count.cmp(a_count))
                .then_with(|| a.cmp(b))
        });
        shared
            .into_iter()
            .map(|(prefix, _, _)| prefix.clone())
            .collect()
    }

    /// Take what holding a move of a prefix (one of [`MoveSuggestions::prefix_trials`])
    /// against what is there came to, as [`frame_prefix_moves`] framed it: a move that
    /// stands is a [`PrefixSuggestion`], with the names it moves and `not_asked`, the
    /// spaces whose adapter takes no prefix ([`PrefixPlan::not_asked`]); one that is
    /// refused is a [`RefusedPrefix`], with how many names under it cannot be moved
    /// (none when nothing is under it).
    pub fn offer(
        &mut self,
        of: PrefixMove,
        framed: Result<NonEmptyVec<MovedOf>, PrefixRefusal>,
        not_asked: Vec<Space>,
    ) {
        match framed {
            Ok(moved) => self.prefixes.push(PrefixSuggestion {
                of,
                moved: moved.map(|one| one.from()),
                not_asked,
            }),
            Err(PrefixRefusal::Names(names)) => self.refused_prefixes.push(RefusedPrefix {
                of,
                refused: names.len(),
            }),
            Err(PrefixRefusal::NothingUnder { .. }) => {
                self.refused_prefixes.push(RefusedPrefix { of, refused: 0 })
            }
        }
    }
}
