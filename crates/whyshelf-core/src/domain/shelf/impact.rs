//! Whether a change makes approvals not hold: the two ends of the change, each read
//! the same one way and assessed by the same rule (`stale`'s, through
//! [`Assessment::holding`]), compared document by document.
//!
//! | # | rule |
//! |---|---|
//! | I1 | a row for every document the standings of either end list, each end's [`Holding`] by [`Assessment::holding`] |
//! | I2 | a cause is compared by the document that holds it (the last of its root's way) and the cause itself, so another way to the same cause is no new one: what the change adds is the causes after that were not before (before, only when it did not hold) |
//! | I3 | of those, a cause that starts at an artifact ([`Cause::located`]) the document holding it stood on, before, by a name that could not be told is `uncertain`: it may have been there before the change; every other is `certain` (a cause that starts nowhere of its own, or not at an artifact, stands on no such name). The name is matched through the moves declared of the pin ([`Cause::pin_names`]): a move the change declares leaves the name before at the old one |
//! | I4 | [`Made`]: after, not holding with a certain cause, [`Made::Yes`]; else an uncertain cause, [`Made::CannotTell`] from the side before (both, when the side after has names that cannot be told too); else, after, not holding or not told, with names that cannot be told, [`Made::CannotTell`] from the side after (such a name may hide a cause), or from both when the side before could not tell the same name; each name listed once; else [`Made::No`] |
//! | I5 | `changed`: the two holdings differ |
//!
//! A cause added is one the end before did not hold: a new edit of what already did
//! not hold before is a cause added too, so [`Made::Yes`] does not say the approval
//! held before the change ([`ImpactEntry::before`] does).
//!
//! What cannot be told is what the adapters and the assessment said cannot be
//! ([`UntoldPin`], [`Holding::CannotTell`]), carried as it is: no new kind is made.

use std::collections::BTreeSet;

use crate::domain::id::DocId;
use crate::domain::nonempty::NonEmptyVec;

use super::build::Shelf;
use super::freshness::{Assessment, Cause, Holding, Origin, RootCause, UntoldPin};

/// Whether the change adds a cause that makes the approval not hold.
// `Yes` holds its first cause inline, as `Freshness::Degraded` does: one `Made` per
// document of one answer, and boxing it would add an allocation to each one that is
// made not to hold and a deref to each read of it.
#[allow(clippy::large_enum_variant)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Made {
    /// It does: `certain` stand on what the side before could tell; `uncertain` stand
    /// on a name it could not, and may have been there before the change.
    Yes(MadeYes),
    /// It does not.
    No,
    /// It cannot be told: a name that cannot be told may hide a cause, or every added
    /// cause stands on a name the side before could not tell (`uncertain`).
    CannotTell(MadeCannotTell),
}

/// What the change adds: only [`impact`] makes one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MadeYes {
    certain: NonEmptyVec<RootCause>,
    uncertain: Vec<RootCause>,
}

impl MadeYes {
    /// The causes added that stand on what the side before could tell.
    pub fn certain(&self) -> &NonEmptyVec<RootCause> {
        &self.certain
    }

    /// The causes added that stand on a name the side before could not tell.
    pub fn uncertain(&self) -> &[RootCause] {
        &self.uncertain
    }
}

/// Why it cannot be told: only [`impact`] makes one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MadeCannotTell {
    side: Sides,
    untold: NonEmptyVec<UntoldPin>,
    uncertain: Vec<RootCause>,
}

impl MadeCannotTell {
    /// Which side could not tell.
    pub fn side(&self) -> Sides {
        self.side
    }

    /// The names that could not be told.
    pub fn untold(&self) -> &NonEmptyVec<UntoldPin> {
        &self.untold
    }

    /// The causes added that stand on a name the side before could not tell.
    pub fn uncertain(&self) -> &[RootCause] {
        &self.uncertain
    }
}

/// Which side of a change could not tell.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Sides {
    /// The side before.
    Before,
    /// The side after.
    After,
    /// Both.
    Both,
}

/// One document's row: its holding at each side, and what the change made of it.
/// Only [`impact`] makes one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImpactEntry {
    id: DocId,
    before: Holding,
    after: Holding,
    made: Made,
}

impl ImpactEntry {
    /// The document.
    pub fn id(&self) -> DocId {
        self.id
    }

    /// Whether its approval held before the change.
    pub fn before(&self) -> &Holding {
        &self.before
    }

    /// Whether its approval holds after the change.
    pub fn after(&self) -> &Holding {
        &self.after
    }

    /// Whether the two differ (I5).
    pub fn changed(&self) -> bool {
        self.before != self.after
    }

    /// Whether the change adds a cause that makes it not hold (I4).
    pub fn made(&self) -> &Made {
        &self.made
    }
}

/// Every document either end lists, by identity, with its holding at each end and
/// what the change made of it (the rules on the page of the module).
pub fn impact(before: (&Shelf, &Assessment), after: (&Shelf, &Assessment)) -> Vec<ImpactEntry> {
    rows_of(before, after)
        .into_iter()
        .map(|id| row(id, before, after))
        .collect()
}

/// [`impact`] of a change and of the change read backwards (the two ends swapped),
/// each identity's two rows paired: both directions are built from the one set of
/// identities, so neither can lack a row the other has.
pub(crate) fn both_ways(
    before: (&Shelf, &Assessment),
    after: (&Shelf, &Assessment),
) -> Vec<(ImpactEntry, ImpactEntry)> {
    rows_of(before, after)
        .into_iter()
        .map(|id| (row(id, before, after), row(id, after, before)))
        .collect()
}

/// The identities of the rows (I1): every one either end's standings list.
fn rows_of(before: (&Shelf, &Assessment), after: (&Shelf, &Assessment)) -> BTreeSet<DocId> {
    before
        .1
        .standings()
        .keys()
        .chain(after.1.standings().keys())
        .copied()
        .collect()
}

/// One identity's row, read from `before` to `after`.
fn row(id: DocId, before: (&Shelf, &Assessment), after: (&Shelf, &Assessment)) -> ImpactEntry {
    let was = before.1.holding(before.0, id);
    let is = after.1.holding(after.0, id);
    let made = made(&was, &is, |holder| before.1.holding(before.0, holder));
    ImpactEntry {
        id,
        before: was,
        after: is,
        made,
    }
}

/// The causes of a holding that does not hold, and the names it cannot tell.
fn parts(holding: &Holding) -> (&[RootCause], Vec<UntoldPin>) {
    match holding {
        Holding::DoesNotHold { roots, untold, .. } => (roots, untold.clone()),
        Holding::CannotTell { untold, .. } => (&[], untold.iter().cloned().collect()),
        Holding::NotThere | Holding::Pending | Holding::Against(_) | Holding::Holds(_) => {
            (&[], Vec::new())
        }
    }
}

/// The document that holds a root's cause: the last of its way.
fn holder(root: &RootCause) -> Option<DocId> {
    root.path.last().copied()
}

/// I2–I4, `before_of` the holding before of the document that holds a cause.
fn made(was: &Holding, is: &Holding, before_of: impl Fn(DocId) -> Holding) -> Made {
    let (was_roots, was_untold) = parts(was);
    let (is_roots, is_untold) = parts(is);
    let held: Vec<(Option<DocId>, &Cause)> = was_roots
        .iter()
        .map(|root| (holder(root), &root.cause))
        .collect();
    let mut certain = Vec::new();
    let mut uncertain = Vec::new();
    let mut untold_before: Vec<UntoldPin> = Vec::new();
    for root in is_roots {
        if held
            .iter()
            .any(|(h, cause)| *h == holder(root) && **cause == root.cause)
        {
            continue;
        }
        // The pin is matched by every name it is read through: a move declared by the
        // change itself leaves the name before at the old one, and the cause at the
        // new one.
        let stood_on = holder(root).and_then(|owner| match root.cause.located(owner) {
            Some((Origin::Artifact(_), _)) => {
                let names = root.cause.pin_names();
                let before = before_of(owner);
                parts(&before)
                    .1
                    .iter()
                    .find(|pin| names.contains(&&pin.artifact))
                    .cloned()
            }
            _ => None,
        });
        match stood_on {
            Some(pin) => {
                if !untold_before.contains(&pin) {
                    untold_before.push(pin);
                }
                uncertain.push(root.clone());
            }
            None => certain.push(root.clone()),
        }
    }
    let not_holding = matches!(is, Holding::DoesNotHold { .. });
    if not_holding && let Ok(certain) = NonEmptyVec::from_vec(certain) {
        return Made::Yes(MadeYes { certain, uncertain });
    }
    if let Ok(pins) = NonEmptyVec::from_vec(untold_before) {
        let side = if is_untold.is_empty() {
            Sides::Before
        } else {
            Sides::Both
        };
        // A name neither side could tell is one name: listed once.
        let mut untold = pins.into_vec();
        for pin in &is_untold {
            if !untold.contains(pin) {
                untold.push(pin.clone());
            }
        }
        if let Ok(untold) = NonEmptyVec::from_vec(untold) {
            return Made::CannotTell(MadeCannotTell {
                side,
                untold,
                uncertain,
            });
        }
    }
    let told_not = matches!(is, Holding::DoesNotHold { .. } | Holding::CannotTell { .. });
    if told_not && !is_untold.is_empty() {
        // A name the side before could not tell either is a name of both sides,
        // listed once.
        let both = is_untold
            .iter()
            .any(|pin| was_untold.iter().any(|was| was.artifact == pin.artifact));
        let mut untold = Vec::new();
        for pin in is_untold
            .into_iter()
            .chain(if both { was_untold } else { Vec::new() })
        {
            if !untold
                .iter()
                .any(|kept: &UntoldPin| kept.artifact == pin.artifact)
            {
                untold.push(pin);
            }
        }
        if let Ok(untold) = NonEmptyVec::from_vec(untold) {
            return Made::CannotTell(MadeCannotTell {
                side: if both { Sides::Both } else { Sides::After },
                untold,
                uncertain: Vec::new(),
            });
        }
    }
    Made::No
}

#[cfg(test)]
#[path = "impact/tests.rs"]
mod tests;
