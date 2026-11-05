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
//! # Beside the state: presence, readiness, basis
//!
//! Three more facts are read from the files and the ledgers
//! ([`corpus_eval`](super::corpus_eval)); none is a value of an axis.
//!
//! | fact | values | meaning |
//! |---|---|---|
//! | [`Presence`] | Present / Missing / Retired | whether the file of something judged is there. A document that is gone has no content, so no state; `Missing` is shown, `Retired` (an unrevoked retirement, while the file is absent) is not |
//! | [`Readiness`] | a list of [`Lack`]s | what the next gate needs that the entity lacks, each [`Required`](LackNeed::Required) (the gate refuses) or [`Informational`](LackNeed::Informational) (it does not, or it has passed) |
//! | basis | per approval | what an approval stands on: a gate's machine check (for CS, the policy it passed under) or an earlier decision taken in |
//!
//! Readiness is kept apart from [`Completeness`] because they answer different
//! questions. Completeness: can everything this stands on be read here? Readiness:
//! has this itself got what the next gate asks for? A Spec with no pin is Complete and
//! not ready; a Spec whose private ground is out of reach is ready and Partial.
//!
//! | [`LackField`] | lacking | Required while |
//! |---|---|---|
//! | `Body` | a Finding with nothing under its front matter | CR has not passed |
//! | `Pins` | a Spec that declares no pin | CS has not passed and the policy demands pins |
//! | `Hypotheses` | a Spec that declares no hypothesis | CS has not passed and the policy demands them |
//! | `Items` | a Design with no item (never Required: CI passes, but CA has nothing to approve) | |
//! | `PlainPin` | an approval holds a keyed pin of an upstream that is public now | always: approving again pins it plain |
//!
//! A Spec that passed CS under a relaxed policy with no pin still shows `Pins`, as
//! Informational: the approval stands, and CI and CA then have nothing to compare.
//!
//! The kind of the thing judged is one more cause of Invalid. A judgement made of a
//! Spec, under an identifier whose file is a Finding now, is
//! [`InvalidReason::KindChanged`].
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
//! baseline, a fact asked of the revision port (which may answer that it cannot tell,
//! as a clone without the baseline does: [`Integration::Unknown`]). [`ItemProgress`] says which of an
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
//! | [`UnparsedReason::ForeignSection`] | Invalid | a key of another kind of document (a `crux` in a Spec) | take the section out, or register the file as the kind it is |
//! | [`UnparsedReason::UnknownKey`] | Invalid | a key no kind of document has and that classifies nothing (a typing mistake) | correct the key, or take it out |
//! | [`UnparsedReason::KindUnknown`] | Invalid | neither the `kind:` key nor the place of the file says what kind of document it is | write `kind:` in the front matter |
//! | [`UnparsedReason::KindDisagrees`] | Invalid | the `kind:` key and the place of the file name different kinds | make them agree |
//! | [`UnparsedReason::EmptyGrounds`], [`UnparsedReason::DuplicateGround`] | Invalid | a Spec with no ground, or one Finding twice among them | list each Finding once, and at least one |
//! | [`UnparsedReason::DuplicateItemNumber`] | Invalid | two items of a Design have one number | give each item a number of its own |
//! | [`InvalidReason::Corpus`] | Invalid | the files of the project taken together cannot be trusted for this document (a shared id or alias, a private document Git would publish, items whose identities clash) | as the [`Invalidity`] says |
//! | [`InvalidReason::KindChanged`] | Invalid | a judgement was made of one kind of thing under this identifier, and the file there is another kind now | put the file back, or register the new content under an identifier of its own |
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
//! | [`UnresolvedReason`] with [`UnresolvedCause::Store`] | Partial | an entity it stands on, directly or further up, is in a store that cannot be read here (a `private:` reference in a clone that has no private store) | open the project where that store is; approving needs it too, for the pin the gate itself checks |
//! | [`UnresolvedReason`] with [`UnresolvedCause::KeyMissing`] | Partial | an approval of this entity pinned an upstream by a key this environment lacks, so whether the upstream changed cannot be told (the upstream itself can be read) | have the key, or approve this entity again, which pins under a key this environment has; an entity further down that merely inherits the reason has nothing to do about it |
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
use super::corpus::Invalidity;
use super::design::CoverageIssue;
use super::event::{DocKind, EntityKind};
use super::hash::ContentHash;
use super::id::{Alias, EntityId, ItemNumber};
use super::keyring::{Kid, UpstreamHash};
use super::location::RelPath;
use super::nonempty::NonEmptyVec;
use super::pin::PinId;
use super::reference::Ref;
use super::revision::{RevisionId, UnknownIntegration};

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
    /// The file has a key that belongs to another kind of document than the one it
    /// is registered as (a Spec with a `crux`).
    ///
    /// Fix: take the section out, or register the file as the kind it is.
    #[error(
        "the key `{key}` is a part of a {section}, and the file is registered as a {registered}"
    )]
    ForeignSection {
        /// The kind the key belongs to.
        section: DocKind,
        /// The kind the file is registered as.
        registered: DocKind,
        /// The key.
        key: String,
    },
    /// The file has a key that no kind of document has and that is no classification
    /// key, which is most often a typing mistake.
    ///
    /// Fix: correct the key, or take it out.
    #[error("the key `{key}` is none that a document has")]
    UnknownKey {
        /// The key.
        key: String,
    },
    /// Neither the front matter nor the place of the file says what kind of
    /// document it is.
    ///
    /// The kind is the `kind:` key; without it, the rule of the place the file
    /// sits in (the directory and the kind it holds). Fix: write `kind:` in the
    /// front matter.
    #[error("the kind of the document cannot be told")]
    KindUnknown,
    /// The front matter and the place of the file name different kinds.
    ///
    /// Fix: make the `kind:` key and the place agree, by correcting one or moving
    /// the file.
    #[error("the file says it is a {front}, and its place says {place}")]
    KindDisagrees {
        /// The kind the `kind:` key names.
        front: DocKind,
        /// The kind the rule of the place names.
        place: DocKind,
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
    /// Two items of a Design carry the same number.
    ///
    /// The number is the key of the items of a Design, so a Design cannot be built
    /// from such a file. Fix: give each item a number of its own.
    #[error("the item {number} appears twice")]
    DuplicateItemNumber {
        /// The repeated number.
        number: ItemNumber,
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
    /// The files of the project taken together cannot be trusted for this document:
    /// a shared id or alias, a private document Git would publish, or items whose
    /// identities clash ([`Invalidity`] says which).
    ///
    /// Fix: as the [`Invalidity`] says.
    #[error("{0}")]
    Corpus(Box<Invalidity>),
    /// A judgement was made of one kind of thing under this identifier, and the file
    /// there is another kind now.
    ///
    /// Fix: put the file back as it was, or register the new content under an
    /// identifier of its own.
    #[error("judged as a {}, and it is a {} now", .was.name(), .now.name())]
    KindChanged {
        /// The kind the judgement was about.
        was: EntityKind,
        /// The kind the file is now.
        now: EntityKind,
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
        /// The hash the approval recorded, plain or keyed.
        recorded: UpstreamHash,
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

/// Why an entity cannot be read here: two things with two remedies, kept apart.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum UnresolvedCause {
    /// It is in a store this environment cannot read (a `private:` reference in a
    /// clone that has no private store).
    ///
    /// Fix: open the project where that store is.
    Store,
    /// It is there, and an approval of the entity that holds the reason pinned it by
    /// a key that is not in this environment's keyring, so whether it changed cannot
    /// be told.
    ///
    /// Fix: have the key, or approve that entity again, which pins the upstream under
    /// a key this environment has.
    KeyMissing {
        /// The key the pin was made under.
        kid: Kid,
    },
}

/// An entity that cannot be read here, the way to it, and why.
///
/// `root` is the entity that cannot be read in this environment; `upstream` is the
/// direct upstream of the holder through which it is reached, equal to `root` when
/// the holder names it directly. The same shape as the link of a chain of causes
/// (`StaleReason::UpstreamStale`). The [`UnresolvedCause`] is kept down the chain: an
/// entity on a Partial upstream says what the upstream's reason was.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct UnresolvedReason {
    root: EntityId,
    upstream: EntityId,
    cause: UnresolvedCause,
}

impl UnresolvedReason {
    /// The entity in a store that cannot be read, reached through this direct
    /// upstream.
    pub fn new(root: EntityId, upstream: EntityId) -> Self {
        Self {
            root,
            upstream,
            cause: UnresolvedCause::Store,
        }
    }

    /// An upstream of the holder whose pin, in an approval of the holder, was made
    /// under the key `kid`, which this environment lacks.
    pub fn key_missing(upstream: EntityId, kid: Kid) -> Self {
        Self {
            root: upstream,
            upstream,
            cause: UnresolvedCause::KeyMissing { kid },
        }
    }

    /// The same reason, seen from the entity one step down, which reaches the root
    /// through `upstream`.
    pub(crate) fn reached_through(&self, upstream: EntityId) -> Self {
        Self {
            upstream,
            ..self.clone()
        }
    }

    /// The entity that cannot be read here.
    pub fn root(&self) -> EntityId {
        self.root
    }

    /// The direct upstream through which it is reached.
    pub fn upstream(&self) -> EntityId {
        self.upstream
    }

    /// Why it cannot be read.
    pub fn cause(&self) -> &UnresolvedCause {
        &self.cause
    }

    /// Whether the holder names the root itself, and no entity in between: for a
    /// missing key, whether the pin is the holder's own.
    pub fn is_direct(&self) -> bool {
        self.root == self.upstream
    }
}

impl fmt::Display for UnresolvedReason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.cause {
            UnresolvedCause::Store => write!(f, "{} cannot be read here", self.root)?,
            UnresolvedCause::KeyMissing { kid } => write!(
                f,
                "{} was pinned under the key `{kid}`, which is not here",
                self.root
            )?,
        }
        if self.is_direct() {
            Ok(())
        } else {
            write!(f, " (reached through {})", self.upstream)
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

/// Whether the file of a document, or the Design that holds an item, is there.
///
/// A fact of its own, beside the two axes: a document that is gone has no content
/// to be Invalid or Modified in, so it has no [`EntityState`] either. The rule that
/// decides it (which records count, and what a retirement does) is the one stated
/// under "Presence" in the documentation of [`ledger`](super::ledger), and
/// [`corpus_eval`](super::corpus_eval) applies it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Presence {
    /// The file is there.
    Present,
    /// The file is gone, and the thing has judgements that count: shown, never
    /// dropped without a word.
    ///
    /// Fix: bring the file back, or retire it (`Retire`).
    Missing,
    /// The file is gone and a retirement says that is meant. Nothing is shown.
    Retired,
}

/// What a lack concerns.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum LackField {
    /// A Finding with nothing under the front matter.
    Body,
    /// A Spec that declares no pin.
    Pins,
    /// A Spec that declares no hypothesis.
    Hypotheses,
    /// A Design that has no item, so there is nothing for CA to approve.
    Items,
    /// An approval pinned this upstream by a keyed pin and the upstream is public
    /// now: the keyed pin cannot be compared in an environment without the key.
    /// Approving again pins it plain.
    PlainPin {
        /// The upstream that is public now.
        upstream: EntityId,
    },
}

/// Whether a lack stops the next gate or only says something.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum LackNeed {
    /// The next gate refuses until it is made good.
    Required,
    /// The next gate does not refuse (the project's policy allows it, or the gate has
    /// passed already), and the lack is worth knowing.
    Informational,
}

/// One thing the next gate needs that the entity does not have.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Lack {
    field: LackField,
    need: LackNeed,
}

impl Lack {
    /// A lack of `field`, of the given weight.
    pub fn new(field: LackField, need: LackNeed) -> Self {
        Self { field, need }
    }

    /// What is lacking.
    pub fn field(&self) -> LackField {
        self.field
    }

    /// Whether it stops the next gate.
    pub fn need(&self) -> LackNeed {
        self.need
    }
}

/// What the entity needs before the next gate, kept apart from [`Completeness`].
///
/// Completeness is about what the entity stands on and whether it can be read here.
/// Readiness is about the entity's own content and its own pins: a Spec with no pin
/// is complete (everything it stands on is readable) and not ready (CS asks for one,
/// by default).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Readiness {
    lacks: Vec<Lack>,
}

impl Readiness {
    /// A readiness from its lacks, in a fixed order.
    pub fn new(mut lacks: Vec<Lack>) -> Self {
        lacks.sort();
        lacks.dedup();
        Self { lacks }
    }

    /// Every lack.
    pub fn lacks(&self) -> &[Lack] {
        &self.lacks
    }

    /// Whether nothing the next gate refuses on is lacking.
    pub fn is_ready(&self) -> bool {
        !self.lacks.iter().any(|l| l.need == LackNeed::Required)
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
    /// Approved, and whether the revision reached the baseline cannot be told here
    /// (the clone lacks the baseline or the commit). Not a failure: the rest of the
    /// state stands, and a clone that has them says more.
    Unknown {
        /// The approved revision (the base of a patch).
        revision: RevisionId,
        /// Why it cannot be told.
        reason: UnknownIntegration,
    },
}

impl Integration {
    /// Whether the approved revision has reached the baseline: the fact
    /// [`CmDue`](super::next::CmDue) is made from, which the type of the
    /// [`integration`](super::integration) module (that also tells a squash merge)
    /// gives as well.
    pub fn is_integrated(&self) -> bool {
        matches!(self, Self::Integrated { .. })
    }
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
        let partial = Completeness::from_reasons(BTreeSet::from([direct.clone(), through, direct]));
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
    fn a_missing_key_is_a_reason_of_its_own_and_is_told_apart_from_a_store() {
        let kid: Kid = "k1".parse().unwrap();
        let store = UnresolvedReason::new(eid(1), eid(1));
        let key = UnresolvedReason::key_missing(eid(1), kid.clone());
        assert_eq!(store.cause(), &UnresolvedCause::Store);
        assert_eq!(key.cause(), &UnresolvedCause::KeyMissing { kid });
        assert_ne!(store, key);
        assert!(key.is_direct());
        // Both are kept: a set tells them apart, though root and upstream agree.
        let partial = Completeness::from_reasons(BTreeSet::from([store, key.clone()]));
        assert_eq!(partial.reasons().count(), 2);
        // Reaching it through another upstream keeps what it is.
        let through = key.reached_through(eid(2));
        assert_eq!(through.cause(), key.cause());
        assert!(!through.is_direct());
        assert!(key.to_string().contains("`k1`"));
        assert!(through.to_string().contains(&format!("through {}", eid(2))));
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
