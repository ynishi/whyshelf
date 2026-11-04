//! The derived state of an entity: two axes and completeness, plus integration and
//! progress.
//!
//! State is never written down. It is derived on demand from the content and the
//! event log, so it cannot be written early or late.
//!
//! # Two axes, and a third fact
//!
//! | fact | values | meaning |
//! |---|---|---|
//! | [`Approval`] | Invalid / Draft / Approved / Modified | the entity's own content against its own approvals |
//! | [`Upstream`] | Ok / Stale / Blocked | whether what it stands on is current (Stale) or not yet approved (Blocked) |
//! | [`Completeness`] | Complete / Partial | whether everything it stands on can be read in this environment |
//!
//! Completeness is a fact of its own and not a value of [`Upstream`], because
//! an entity can be Stale or Blocked and Partial at once, and a value of one axis
//! keeps only the reason that won. A Partial entity is shown as such; it is never
//! Invalid for that. See [`Completeness`] for how it travels downstream.
//!
//! - **Invalid**: a syntax error, missing metadata, a reference to nothing, a PIN
//!   left unassigned, a path or alias shared with another entity, or a file that
//!   did not parse.
//! - **Draft**: no approval yet.
//! - **Approved**: an approval (not revoked) exists for the *current* hash.
//! - **Modified**: approvals exist, none for the current hash; the entity was
//!   edited after it was approved.
//!
//! How the approval axis is decided, in order (the first question answered "yes"
//! settles it):
//!
//! ```text
//!   is the entity's own content unusable?          -- yes --> Invalid
//!        | no
//!   an unrevoked approval for the current hash?    -- yes --> Approved
//!        | no
//!   an unrevoked approval for an earlier hash?     -- yes --> Modified
//!        | no
//!        v
//!      Draft
//! ```
//!
//! The two axes are independent, with one exception that follows from how Stale is
//! judged. A Stale compares the upstream hashes an approval *recorded* with the
//! upstream's hashes today, so only an entity that is **Approved** at its current
//! hash can be Stale; every other entity is Ok or Blocked.
//!
//! ```text
//!                      upstream axis
//!                      Ok      Stale   Blocked
//!   approval axis    +-------+-------+---------+
//!     Invalid        |   x   |   -   |    x    |
//!     Draft          |   x   |   -   |    x    |
//!     Modified       |   x   |   -   |    x    |
//!     Approved       |   x   |   x   |    x    |
//!                    +-------+-------+---------+
//!     x  can occur      -  cannot (a Draft or Modified entity that sits on a
//!                          Stale upstream is shown Blocked, since it has no
//!                          approval to be stale against)
//! ```
//!
//! A Finding that has not passed CR makes the Specs on it **Blocked**, not
//! Stale. A Draft entity has recorded no upstream hashes, so it is never Stale: its
//! upstream health is Ok or Blocked.
//!
//! [`Completeness`] is independent of both axes: any cell of the matrix can also be
//! Partial, and a Partial entity keeps every other fact it has.
//!
//! # Stale
//!
//! The approval event of an entity records the hashes of its upstream at approval
//! time. The entity is Stale when those recorded hashes differ from the upstream's
//! hashes today. Evaluation runs upstream to downstream (Finding, Spec, Design,
//! Item) and passes causes along, so correcting a Finding makes everything below
//! it Stale.
//!
//! # Integration and progress
//!
//! [`Integration`] says whether the approved revision of an item has reached the
//! baseline, a fact asked of the revision port. [`ItemProgress`] says which of an
//! item's pins the tests at the current revision already carry; an item whose
//! tests do not yet cover its pins is shown as "in progress", and that is never
//! Invalid. Only the CA lint stops on it.
//!
//! # Reading a reason, and clearing it
//!
//! The reasons are values, so a view can list every one of them, not the first.
//! `whyshelf status` prints them under its table, one line each, and `whyshelf get`
//! under the entity.
//!
//! | reason | axis | what it means | how to clear it |
//! |---|---|---|---|
//! | [`UnparsedReason::MergeConflictMarker`] | Invalid | a line starts with seven `<` or `>` | resolve the merge in the file |
//! | [`UnparsedReason::MalformedFrontmatter`] | Invalid | no `---` pair, YAML that does not read, or not a mapping | repair the front matter |
//! | [`UnparsedReason::MissingField`] | Invalid | a required key or section is absent | add it |
//! | [`UnparsedReason::InvalidField`] | Invalid | a key holds a value its type refuses | correct the value (the reason names the key and why) |
//! | [`UnparsedReason::DanglingRef`] | Invalid | an alias in `grounds` or `spec` names nothing, so there is no identifier to keep | register the target, or correct the alias |
//! | [`UnparsedReason::AmbiguousRef`] | Invalid | an alias in `grounds` or `spec` names several entities | give the alias to one entity, or name the entity by its identifier |
//! | [`UnparsedReason::WrongKind`] | Invalid | a reference in `grounds` or `spec` names an entity of another kind | name an entity of the kind the key takes |
//! | [`UnparsedReason::KindChanged`] | Invalid | the file is of another kind than the one it was registered as | put the file back, or register the new content as its own entity |
//! | [`UnparsedReason::RevisionRejected`] | Invalid | a revision the file names is not one the version-control tool knows | name a revision that exists |
//! | [`UnparsedReason::ExternalRefNotAllowed`] | Invalid | `grounds` or `spec` holds a URL or an `ext:` reference | name an entity of this project |
//! | [`UnparsedReason::EmptyGrounds`], [`UnparsedReason::DuplicateGround`] | Invalid | a Spec with no ground, or one Finding twice among them | list each Finding once, and at least one |
//! | [`InvalidReason::DanglingRef`] | Invalid | an identifier a reference spells (plain, or `private:` where the private store can be read) is not an entity here | bring the entity back, or correct the reference |
//! | [`InvalidReason::ClassificationConflict`] | Invalid | two branches set the kind or the path differently | set it again (the application's `reclassify`; no command does it), or `whyshelf mv` for the path |
//! | [`InvalidReason::PinCoverage`] | Invalid | the Design and its Spec disagree about a pin | assign each declared pin to an item; assign no undeclared one |
//! | [`InvalidReason::PathCollision`] | Invalid | two entities sit at one path | move one with `whyshelf mv` |
//! | [`InvalidReason::AliasCollision`] | Invalid | two entities have one alias | one of them must take another alias (the application's `reclassify` does it; no command does) |
//! | [`StaleReason::UpstreamChanged`] | Stale | an upstream's hash today is not the one this approval recorded | check and approve this entity again |
//! | [`StaleReason::UpstreamNotRecorded`] | Stale | the approval recorded no hash for an upstream that is there today | check and approve this entity again |
//! | [`StaleReason::UpstreamStale`] | Stale | the upstream is itself stale | clear the upstream first, then this entity |
//! | [`BlockReason::UpstreamInvalid`] | Blocked | the upstream is invalid or did not parse | repair the upstream |
//! | [`BlockReason::UpstreamUnapproved`] | Blocked | the upstream is a Draft, or was edited after it was approved | approve the upstream |
//! | [`BlockReason::UpstreamStale`] | Blocked | the upstream is stale and this entity has no approval to be stale against | clear the upstream, then approve this entity |
//! | [`BlockReason::UpstreamBlocked`] | Blocked | the upstream is itself blocked | follow the chain up (the causes of the view name it) |
//! | [`UnresolvedReason`] | Partial | an entity it stands on, directly or further up, is in a store that cannot be read here (a `private:` reference in a clone that has no private store) | open the project where that store is; approving needs it too |
//!
//! When the cause is not the direct upstream but something further up, the evaluation
//! also keeps the chain of causes, root first (see `evaluate`). A library caller reads
//! it from the evaluated entity; the command line uses the root of the chain for the
//! `NEXT` column ("approve upstream *x* first"). A Partial entity keeps its own chain
//! inside [`Completeness`], so a reason is never lost to one that outranks it.
//!
//! What one upstream says about itself to the entity that stands on it is
//! [`UpstreamFacts`], the one mapping from the upstream's state; its three facts
//! (approval, health, completeness) hold together and none hides another. Which
//! upstream problem to show first, when several apply, is decided in one place:
//! `upstream_problems` of the lint module, which the gates and the `NEXT` column both
//! call, over those facts.

use std::collections::BTreeSet;
use std::fmt;

use super::classification::ConflictField;
use super::design::CoverageIssue;
use super::event::{DocKind, EntityKind};
use super::hash::ContentHash;
use super::id::{Alias, EntityId};
use super::location::RelPath;
use super::nonempty::NonEmptyVec;
use super::pin::PinId;
use super::reference::Ref;
use super::revision::RevisionId;

/// Why the text of a file could not become an entity.
///
/// The adapters that parse files report one of these per problem found; the
/// domain only records them.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum UnparsedReason {
    /// The file holds an unresolved merge-conflict marker.
    ///
    /// Fix: resolve the merge in the file.
    #[error("the file holds a merge-conflict marker")]
    MergeConflictMarker,
    /// The front matter is not well formed.
    ///
    /// Fix: the file starts with a `---` line and the front matter ends with another;
    /// the YAML between them is a mapping.
    #[error("the front matter is malformed: {detail}")]
    MalformedFrontmatter {
        /// What the parser reported, free text.
        detail: String,
    },
    /// A required key is missing.
    ///
    /// Fix: add it.
    #[error("the key `{field}` is missing")]
    MissingField {
        /// The key, as written in the file format.
        field: String,
    },
    /// A key has a value that its type refuses.
    ///
    /// Fix: correct the value; `detail` says what the type wants.
    #[error("the key `{field}` is invalid: {detail}")]
    InvalidField {
        /// The key, as written in the file format.
        field: String,
        /// What was wrong, free text.
        detail: String,
    },
    /// An alias in the file names no entity, so there is no identifier to keep.
    ///
    /// A reference that spells an identifier is not this: the identifier is
    /// available, so the entity is built and is Invalid with
    /// [`InvalidReason::DanglingRef`] if nothing holds it.
    ///
    /// Fix: register the entity the alias should name, or correct the alias.
    #[error("the reference `{reference}` names nothing")]
    DanglingRef {
        /// The alias that could not be resolved.
        reference: Ref,
    },
    /// An alias in the file names several entities, so which one is meant is not
    /// known.
    ///
    /// Fix: give the alias to one entity, or name the entity by its identifier.
    #[error("the alias `{reference}` names several entities")]
    AmbiguousRef {
        /// The alias that names several.
        reference: Ref,
    },
    /// A reference names an entity of another kind than the key wants.
    ///
    /// Fix: name an entity of the kind the key takes.
    #[error("`{reference}` is a {}, not a {}", .found.name(), .expected.name())]
    WrongKind {
        /// The reference as written.
        reference: Ref,
        /// The kind the key takes.
        expected: EntityKind,
        /// The kind of the entity it names.
        found: EntityKind,
    },
    /// The file is of another kind than the one it was registered as.
    ///
    /// Fix: put the file back, or register the new content as its own entity.
    #[error("registered as a {registered}, the file is a {file}")]
    KindChanged {
        /// The kind it was registered as.
        registered: DocKind,
        /// The kind the file is now.
        file: DocKind,
    },
    /// A revision the file names is not one the version-control tool knows.
    ///
    /// Fix: name a revision that exists.
    #[error("`{text}` is not a revision")]
    RevisionRejected {
        /// The text as written.
        text: String,
    },
    /// A key holds a reference to another project or to a URL, which could never
    /// be approved here.
    ///
    /// Fix: name an entity of this project.
    #[error("the key `{field}` cannot hold `{reference}`: it is not an entity of this project")]
    ExternalRefNotAllowed {
        /// The key.
        field: String,
        /// The reference as written.
        reference: Ref,
    },
    /// A Spec was left with no ground.
    ///
    /// Fix: name at least one Finding.
    #[error("the Spec has no ground")]
    EmptyGrounds,
    /// Two grounds of a Spec are the same Finding.
    ///
    /// Fix: list each Finding once.
    #[error("the Finding {finding} is listed twice among the grounds")]
    DuplicateGround {
        /// The repeated Finding.
        finding: EntityId,
    },
}

/// Why an entity is Invalid.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum InvalidReason {
    /// The file did not parse.
    #[error("the file did not parse: {0}")]
    Unparsed(UnparsedReason),
    /// An upstream identifier is known to the content but to no entity that has a
    /// file here (the file of a registered entity is gone, say).
    ///
    /// A reference into a store that cannot be read here is not this: it leaves the
    /// entity Partial. Fix: bring the entity back, or correct the reference.
    #[error("the upstream {upstream} does not exist")]
    DanglingRef {
        /// The identifier that points at nothing.
        upstream: EntityId,
    },
    /// The Design does not cover the Spec's pins exactly.
    ///
    /// Fix: assign each declared pin to an item and no undeclared one.
    #[error("the pin coverage is wrong: {0:?}")]
    PinCoverage(CoverageIssue),
    /// Another entity sits at the same path.
    ///
    /// Fix: move one of them with `whyshelf mv`.
    #[error("another entity sits at {path}")]
    PathCollision {
        /// The shared path.
        path: RelPath,
    },
    /// Another entity has the same alias.
    ///
    /// Fix: give one of them another alias (the application's `reclassify`; there is
    /// no command for it).
    #[error("another entity has the alias {alias}")]
    AliasCollision {
        /// The shared alias.
        alias: Alias,
    },
    /// Two branches set the kind or the path differently, so which it is cannot be
    /// told and the file cannot be read.
    ///
    /// Fix: set the value again (the application's `reclassify` for a kind; no
    /// command does it) or `whyshelf mv` for a path: the new event lists both
    /// branches among its heads and settles the value.
    #[error("the {field} was set differently on two branches")]
    ClassificationConflict {
        /// The value in conflict: the kind or the path.
        field: ConflictField,
    },
}

/// Why an approved entity is Stale.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum StaleReason {
    /// An upstream hash today differs from the one the approval recorded.
    ///
    /// Fix: check and approve this entity again.
    #[error("{upstream} changed since the approval")]
    UpstreamChanged {
        /// The upstream entity.
        upstream: EntityId,
        /// The hash the approval recorded.
        recorded: ContentHash,
        /// The hash today.
        current: ContentHash,
    },
    /// The approval recorded nothing for an upstream that is there today.
    ///
    /// Fix: check and approve this entity again.
    #[error("the approval recorded nothing for {upstream}")]
    UpstreamNotRecorded {
        /// The upstream entity.
        upstream: EntityId,
    },
    /// The upstream entity is itself Stale.
    ///
    /// Fix: clear the upstream first, then this entity.
    #[error("{upstream} is stale")]
    UpstreamStale {
        /// The upstream entity.
        upstream: EntityId,
    },
}

/// Why an entity is Blocked.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum BlockReason {
    /// An upstream entity is Invalid or did not parse.
    ///
    /// Fix: repair the upstream.
    #[error("{upstream} is invalid")]
    UpstreamInvalid {
        /// The upstream entity.
        upstream: EntityId,
    },
    /// An upstream entity has no approval for its current content.
    ///
    /// Fix: approve the upstream.
    #[error("{upstream} is not approved")]
    UpstreamUnapproved {
        /// The upstream entity.
        upstream: EntityId,
    },
    /// An upstream entity is Stale and this entity has no approval to be stale
    /// against.
    ///
    /// Fix: clear the upstream, then approve this entity.
    #[error("{upstream} is stale")]
    UpstreamStale {
        /// The upstream entity.
        upstream: EntityId,
    },
    /// An upstream entity is itself Blocked.
    ///
    /// Fix: follow the chain up to the first entity that is not ready.
    #[error("{upstream} is blocked")]
    UpstreamBlocked {
        /// The upstream entity.
        upstream: EntityId,
    },
}

/// The entity's own content against its own approvals.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Approval {
    /// The entity cannot be evaluated for approval.
    Invalid(NonEmptyVec<InvalidReason>),
    /// No approval yet.
    Draft,
    /// An approval exists for the current hash.
    Approved,
    /// Approved before, but not at the current hash.
    Modified {
        /// The hash that was approved.
        approved: ContentHash,
    },
}

/// Whether what the entity stands on is current and approved.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Upstream {
    /// Nothing wrong upstream.
    Ok,
    /// The approval was made against an older upstream.
    Stale(NonEmptyVec<StaleReason>),
    /// Something upstream is not approved or not valid.
    Blocked(NonEmptyVec<BlockReason>),
}

/// What is wrong with an upstream on the approval axis, as the entity that stands
/// on it sees it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ApprovalProblem {
    /// The upstream is Invalid.
    Invalid,
    /// The upstream has no approval for its current content (a Draft, or a Modified
    /// one that does not explain this entity's staleness).
    NotApproved,
}

/// What is wrong with an upstream on the upstream axis.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HealthProblem {
    /// The upstream is itself Stale.
    Stale,
    /// The upstream is itself Blocked.
    Blocked,
}

/// What one upstream says about itself to the entity that stands on it: the one
/// mapping from an upstream's state to facts.
///
/// The three facts hold at the same time and none hides another: an upstream can
/// be Approved, Stale and Partial at once. The mapping is here and nowhere else;
/// the evaluator turns `approval` and `health` into its block and stale reasons,
/// the completeness stays a fact of its own ([`Completeness`]), and the order in
/// which problems are shown first is the lint module's
/// (`upstream_problems`), applied over the facts of each upstream.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UpstreamFacts {
    approval: Option<ApprovalProblem>,
    health: Option<HealthProblem>,
    partial: bool,
}

impl UpstreamFacts {
    /// The facts of the upstream whose state is `up`.
    ///
    /// `target_approved` says whether the entity that stands on it holds an
    /// approval for its current content, and `changed` whether this upstream's hash
    /// today is not the one that approval recorded. An approved entity whose
    /// upstream was edited since is Stale because of it; that upstream being
    /// Modified is then the reason itself, and not a second reason for a block.
    pub fn of(up: &EntityState, target_approved: bool, changed: bool) -> Self {
        let approval = match up.approval() {
            Approval::Invalid(_) => Some(ApprovalProblem::Invalid),
            Approval::Draft => Some(ApprovalProblem::NotApproved),
            Approval::Modified { .. } if !(target_approved && changed) => {
                Some(ApprovalProblem::NotApproved)
            }
            Approval::Modified { .. } | Approval::Approved => None,
        };
        let health = match up.upstream() {
            Upstream::Ok => None,
            Upstream::Stale(_) => Some(HealthProblem::Stale),
            Upstream::Blocked(_) => Some(HealthProblem::Blocked),
        };
        Self {
            approval,
            health,
            partial: up.completeness().is_partial(),
        }
    }

    /// The problem on the approval axis, if any.
    pub fn approval(&self) -> Option<ApprovalProblem> {
        self.approval
    }

    /// The problem on the upstream axis, if any.
    pub fn health(&self) -> Option<HealthProblem> {
        self.health
    }

    /// Whether the upstream, or something it stands on, cannot be read here.
    pub fn is_partial(&self) -> bool {
        self.partial
    }
}

/// An entity that cannot be read here, and the way to it.
///
/// `root` is the entity that cannot be read in this environment; `upstream` is the
/// direct upstream of the holder through which it is reached, equal to `root` when
/// the holder names it directly. The same shape as the link of a chain of causes
/// (`StaleReason::UpstreamStale`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct UnresolvedReason {
    root: EntityId,
    upstream: EntityId,
}

impl UnresolvedReason {
    /// The entity that cannot be read, reached through this direct upstream.
    pub fn new(root: EntityId, upstream: EntityId) -> Self {
        Self { root, upstream }
    }

    /// The entity that cannot be read here.
    pub fn root(&self) -> EntityId {
        self.root
    }

    /// The direct upstream through which it is reached.
    pub fn upstream(&self) -> EntityId {
        self.upstream
    }
}

impl fmt::Display for UnresolvedReason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.root == self.upstream {
            write!(f, "{} cannot be read here", self.root)
        } else {
            write!(
                f,
                "{} cannot be read here (reached through {})",
                self.root, self.upstream
            )
        }
    }
}

/// Whether everything an entity stands on can be read in this environment.
///
/// An upstream in a store that cannot be read here (a `private:` reference in a
/// public clone) is not an error, and it hides the upstream's state, so the entity
/// is judged without it. Completeness says so, and the gates that need the upstream
/// stop on it. It travels downstream: an entity on a Partial upstream is Partial,
/// with the same `root` and `upstream` set to that upstream.
///
/// The chain lives here and not in the causes of the evaluation, because a Stale or
/// a Blocked can hold the same entity at the same time and each fact keeps its own.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Completeness {
    /// Everything it stands on can be read.
    Complete,
    /// Something it stands on cannot, for these reasons.
    Partial(NonEmptyVec<UnresolvedReason>),
}

impl Completeness {
    /// The reasons collected, `Complete` when there are none. Each
    /// `(root, upstream)` pair counts once.
    pub fn from_reasons(reasons: BTreeSet<UnresolvedReason>) -> Self {
        match NonEmptyVec::from_vec(reasons.into_iter().collect()) {
            Ok(reasons) => Self::Partial(reasons),
            Err(_) => Self::Complete,
        }
    }

    /// Whether something cannot be read.
    pub fn is_partial(&self) -> bool {
        matches!(self, Self::Partial(_))
    }

    /// The reasons; none when complete.
    pub fn reasons(&self) -> impl Iterator<Item = &UnresolvedReason> {
        match self {
            Self::Complete => None,
            Self::Partial(reasons) => Some(reasons.iter()),
        }
        .into_iter()
        .flatten()
    }

    /// Whether the entity that cannot be read is reached through this direct
    /// upstream.
    pub fn names_upstream(&self, upstream: EntityId) -> bool {
        self.reasons().any(|r| r.upstream() == upstream)
    }
}

/// The two axes and completeness.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EntityState {
    approval: Approval,
    upstream: Upstream,
    completeness: Completeness,
}

impl EntityState {
    /// A state from its two axes and its completeness.
    pub fn new(approval: Approval, upstream: Upstream, completeness: Completeness) -> Self {
        Self {
            approval,
            upstream,
            completeness,
        }
    }

    /// The approval axis.
    pub fn approval(&self) -> &Approval {
        &self.approval
    }

    /// The upstream axis.
    pub fn upstream(&self) -> &Upstream {
        &self.upstream
    }

    /// Whether everything it stands on can be read here.
    pub fn completeness(&self) -> &Completeness {
        &self.completeness
    }
}

/// Where the approved revision of an item stands against the baseline.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Integration {
    /// There is no CA approval for the item's current content.
    Unapproved,
    /// Approved at a revision that has not reached the baseline.
    ApprovedPending {
        /// The approved revision (the base of a patch).
        revision: RevisionId,
    },
    /// Approved at a revision that has reached the baseline.
    Integrated {
        /// The approved revision (the base of a patch).
        revision: RevisionId,
    },
}

/// Which of an item's pins the tests at the current revision carry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ItemProgress {
    found: BTreeSet<PinId>,
    declared: BTreeSet<PinId>,
}

impl ItemProgress {
    /// Progress from the pins found in tests and the pins the item declares.
    pub fn new(found: BTreeSet<PinId>, declared: BTreeSet<PinId>) -> Self {
        Self { found, declared }
    }

    /// The pins found in the tests.
    pub fn found(&self) -> &BTreeSet<PinId> {
        &self.found
    }

    /// The pins assigned to the item.
    pub fn declared(&self) -> &BTreeSet<PinId> {
        &self.declared
    }

    /// Whether the tests do not yet carry every assigned pin.
    ///
    /// Shown as "implementing". It is a display, not a validity: an item can be
    /// Draft and implementing at the same time.
    pub fn is_implementing(&self) -> bool {
        !self.found.is_superset(&self.declared)
    }
}

/// Which of a Spec's declared pins the tests at the current revision carry: the
/// same shape as [`ItemProgress`], for the Spec as a whole.
///
/// It is a count to show, never a validity: a Spec whose pins are not all in the
/// tests yet is not Invalid.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpecProgress {
    found: BTreeSet<PinId>,
    declared: BTreeSet<PinId>,
}

impl SpecProgress {
    /// Progress from the declared pins found in tests and the pins the Spec
    /// declares.
    pub fn new(found: BTreeSet<PinId>, declared: BTreeSet<PinId>) -> Self {
        Self { found, declared }
    }

    /// The pins found in the tests.
    pub fn found(&self) -> &BTreeSet<PinId> {
        &self.found
    }

    /// The pins the Spec declares.
    pub fn declared(&self) -> &BTreeSet<PinId> {
        &self.declared
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::id::test_support::eid;

    fn pins(ids: &[&str]) -> BTreeSet<PinId> {
        ids.iter().map(|p| p.parse().unwrap()).collect()
    }

    #[test]
    fn implementing_means_found_does_not_cover_declared() {
        let declared = pins(&["PIN-A", "PIN-B"]);
        assert!(ItemProgress::new(pins(&[]), declared.clone()).is_implementing());
        assert!(ItemProgress::new(pins(&["PIN-A"]), declared.clone()).is_implementing());
        assert!(!ItemProgress::new(pins(&["PIN-A", "PIN-B"]), declared.clone()).is_implementing());
        // Extra pins found do not hide a missing one, and do not count as missing.
        assert!(
            !ItemProgress::new(pins(&["PIN-A", "PIN-B", "PIN-C"]), declared.clone())
                .is_implementing()
        );
        assert!(ItemProgress::new(pins(&["PIN-C"]), declared).is_implementing());
        assert!(!ItemProgress::new(pins(&[]), pins(&[])).is_implementing());
    }

    fn invalid() -> Approval {
        Approval::Invalid(NonEmptyVec::new(
            InvalidReason::DanglingRef { upstream: eid(9) },
            vec![],
        ))
    }

    fn modified() -> Approval {
        Approval::Modified {
            approved: ContentHash::raw(b"old"),
        }
    }

    fn stale() -> Upstream {
        Upstream::Stale(NonEmptyVec::new(
            StaleReason::UpstreamStale { upstream: eid(9) },
            vec![],
        ))
    }

    fn blocked() -> Upstream {
        Upstream::Blocked(NonEmptyVec::new(
            BlockReason::UpstreamBlocked { upstream: eid(9) },
            vec![],
        ))
    }

    fn partial() -> Completeness {
        Completeness::from_reasons(BTreeSet::from([UnresolvedReason::new(eid(5), eid(5))]))
    }

    /// The table: an upstream's state, whether the target is approved, whether this
    /// upstream changed since, and the facts.
    #[test]
    fn upstream_facts_of_is_this_table() {
        use ApprovalProblem::{Invalid as Bad, NotApproved};
        use HealthProblem::{Blocked, Stale};
        #[allow(clippy::type_complexity)]
        let rows: Vec<(
            Approval,
            Upstream,
            Completeness,
            bool,
            bool,
            Option<ApprovalProblem>,
            Option<HealthProblem>,
            bool,
        )> = vec![
            // approval axis alone
            (
                invalid(),
                Upstream::Ok,
                Completeness::Complete,
                false,
                false,
                Some(Bad),
                None,
                false,
            ),
            (
                Approval::Draft,
                Upstream::Ok,
                Completeness::Complete,
                false,
                false,
                Some(NotApproved),
                None,
                false,
            ),
            (
                Approval::Approved,
                Upstream::Ok,
                Completeness::Complete,
                false,
                false,
                None,
                None,
                false,
            ),
            (
                modified(),
                Upstream::Ok,
                Completeness::Complete,
                false,
                false,
                Some(NotApproved),
                None,
                false,
            ),
            // an approved target and a Modified upstream that changed since: the
            // change is the target's staleness, not a second reason
            (
                modified(),
                Upstream::Ok,
                Completeness::Complete,
                true,
                true,
                None,
                None,
                false,
            ),
            (
                modified(),
                Upstream::Ok,
                Completeness::Complete,
                true,
                false,
                Some(NotApproved),
                None,
                false,
            ),
            (
                modified(),
                Upstream::Ok,
                Completeness::Complete,
                false,
                true,
                Some(NotApproved),
                None,
                false,
            ),
            // upstream axis alone
            (
                Approval::Approved,
                stale(),
                Completeness::Complete,
                false,
                false,
                None,
                Some(Stale),
                false,
            ),
            (
                Approval::Approved,
                blocked(),
                Completeness::Complete,
                false,
                false,
                None,
                Some(Blocked),
                false,
            ),
            // together: nothing hides anything
            (
                Approval::Approved,
                stale(),
                partial(),
                true,
                false,
                None,
                Some(Stale),
                true,
            ),
            (
                Approval::Draft,
                blocked(),
                partial(),
                false,
                false,
                Some(NotApproved),
                Some(Blocked),
                true,
            ),
            (
                invalid(),
                blocked(),
                Completeness::Complete,
                false,
                false,
                Some(Bad),
                Some(Blocked),
                false,
            ),
            // completeness alone
            (
                Approval::Approved,
                Upstream::Ok,
                partial(),
                false,
                false,
                None,
                None,
                true,
            ),
        ];
        for (
            approval,
            upstream,
            completeness,
            approved,
            changed,
            want_approval,
            want_health,
            want_partial,
        ) in rows
        {
            let state = EntityState::new(approval.clone(), upstream.clone(), completeness);
            let facts = UpstreamFacts::of(&state, approved, changed);
            let context =
                format!("{approval:?} / {upstream:?} / approved={approved} changed={changed}");
            assert_eq!(facts.approval(), want_approval, "{context}");
            assert_eq!(facts.health(), want_health, "{context}");
            assert_eq!(facts.is_partial(), want_partial, "{context}");
        }
    }

    /// The three facts are read from three different axes of the state, so any
    /// combination of the axes gives the combination of the facts.
    #[test]
    fn upstream_facts_are_independent_of_each_other() {
        for approval in [invalid(), Approval::Draft, Approval::Approved, modified()] {
            for upstream in [Upstream::Ok, stale(), blocked()] {
                for completeness in [Completeness::Complete, partial()] {
                    let all = UpstreamFacts::of(
                        &EntityState::new(approval.clone(), upstream.clone(), completeness.clone()),
                        false,
                        false,
                    );
                    let approval_only = UpstreamFacts::of(
                        &EntityState::new(approval.clone(), Upstream::Ok, Completeness::Complete),
                        false,
                        false,
                    );
                    let health_only = UpstreamFacts::of(
                        &EntityState::new(
                            Approval::Approved,
                            upstream.clone(),
                            Completeness::Complete,
                        ),
                        false,
                        false,
                    );
                    assert_eq!(all.approval(), approval_only.approval());
                    assert_eq!(all.health(), health_only.health());
                    assert_eq!(all.is_partial(), completeness.is_partial());
                }
            }
        }
    }

    #[test]
    fn completeness_keeps_each_root_and_upstream_pair_once() {
        let direct = UnresolvedReason::new(eid(1), eid(1));
        let through = UnresolvedReason::new(eid(1), eid(2));
        let partial = Completeness::from_reasons(BTreeSet::from([direct, through, direct]));
        assert!(partial.is_partial());
        assert_eq!(partial.reasons().count(), 2);
        assert!(partial.names_upstream(eid(1)));
        assert!(partial.names_upstream(eid(2)));
        assert!(!partial.names_upstream(eid(3)));
    }

    #[test]
    fn no_reason_is_complete() {
        let complete = Completeness::from_reasons(BTreeSet::new());
        assert_eq!(complete, Completeness::Complete);
        assert!(!complete.is_partial());
        assert_eq!(complete.reasons().count(), 0);
        assert!(!complete.names_upstream(eid(1)));
    }

    #[test]
    fn a_reason_says_how_the_unreadable_entity_is_reached() {
        let direct = UnresolvedReason::new(eid(1), eid(1)).to_string();
        let through = UnresolvedReason::new(eid(1), eid(2)).to_string();
        assert!(direct.contains(&eid(1).to_string()) && !direct.contains("through"));
        assert!(through.contains(&eid(1).to_string()));
        assert!(through.contains(&format!("through {}", eid(2))));
    }

    #[test]
    fn completeness_is_a_fact_beside_the_two_axes() {
        let blocked = Upstream::Blocked(NonEmptyVec::new(
            BlockReason::UpstreamUnapproved { upstream: eid(1) },
            vec![],
        ));
        let partial =
            Completeness::from_reasons(BTreeSet::from([UnresolvedReason::new(eid(2), eid(2))]));
        let state = EntityState::new(Approval::Draft, blocked, partial.clone());
        // Blocked and Partial at once: neither hides the other.
        assert!(matches!(state.upstream(), Upstream::Blocked(_)));
        assert_eq!(state.completeness(), &partial);
    }
}
