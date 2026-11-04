//! The derived state of an entity: two axes, plus integration and progress.
//!
//! State is never written down. It is derived on demand from the content and the
//! event log, so it cannot be written early or late.
//!
//! # Two axes
//!
//! | axis | values | meaning |
//! |---|---|---|
//! | [`Approval`] | Invalid / Draft / Approved / Modified | the entity's own content against its own approvals |
//! | [`Upstream`] | Ok / Stale / Blocked | whether what it stands on is current (Stale) or not yet approved (Blocked) |
//!
//! - **Invalid**: a syntax error, missing metadata, a reference to nothing, a PIN
//!   left unassigned, a path or alias shared with another entity, or a file that
//!   did not parse.
//! - **Draft**: no approval yet.
//! - **Approved**: an approval (not revoked) exists for the *current* hash.
//! - **Modified**: approvals exist, none for the current hash; the entity was
//!   edited after it was approved.
//!
//! The axes combine, so "edited since approval" and "upstream changed" can both be
//! shown. A Finding that has not passed CR makes the Specs on it **Blocked**, not
//! Stale. A Draft entity has recorded no upstream hashes, so it is never Stale: its
//! upstream health is Ok or Blocked.
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

use std::collections::BTreeSet;

use super::design::CoverageIssue;
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
    #[error("the file holds a merge-conflict marker")]
    MergeConflictMarker,
    /// The front matter is not well formed.
    #[error("the front matter is malformed: {detail}")]
    MalformedFrontmatter {
        /// What the parser reported, free text.
        detail: String,
    },
    /// A required key is missing.
    #[error("the key `{field}` is missing")]
    MissingField {
        /// The key, as written in the file format.
        field: String,
    },
    /// A key has a value that its type refuses.
    #[error("the key `{field}` is invalid: {detail}")]
    InvalidField {
        /// The key, as written in the file format.
        field: String,
        /// What was wrong, free text.
        detail: String,
    },
    /// An alias or reference in the file names nothing.
    #[error("the reference `{reference}` names nothing")]
    UnresolvedRef {
        /// The reference that could not be resolved.
        reference: Ref,
    },
}

/// Why an entity is Invalid.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum InvalidReason {
    /// The file did not parse.
    #[error("the file did not parse: {0}")]
    Unparsed(UnparsedReason),
    /// An upstream identifier is known to the content but to no entity.
    #[error("the upstream {upstream} does not exist")]
    DanglingRef {
        /// The identifier that points at nothing.
        upstream: EntityId,
    },
    /// The Design does not cover the Spec's pins exactly.
    #[error("the pin coverage is wrong: {0:?}")]
    PinCoverage(CoverageIssue),
    /// Another entity sits at the same path.
    #[error("another entity sits at {path}")]
    PathCollision {
        /// The shared path.
        path: RelPath,
    },
    /// Another entity has the same alias.
    #[error("another entity has the alias {alias}")]
    AliasCollision {
        /// The shared alias.
        alias: Alias,
    },
}

/// Why an approved entity is Stale.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum StaleReason {
    /// An upstream hash today differs from the one the approval recorded.
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
    #[error("the approval recorded nothing for {upstream}")]
    UpstreamNotRecorded {
        /// The upstream entity.
        upstream: EntityId,
    },
    /// The upstream entity is itself Stale.
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
    #[error("{upstream} is invalid")]
    UpstreamInvalid {
        /// The upstream entity.
        upstream: EntityId,
    },
    /// An upstream entity has no approval for its current content.
    #[error("{upstream} is not approved")]
    UpstreamUnapproved {
        /// The upstream entity.
        upstream: EntityId,
    },
    /// An upstream entity is Stale and this entity has no approval to be stale
    /// against.
    #[error("{upstream} is stale")]
    UpstreamStale {
        /// The upstream entity.
        upstream: EntityId,
    },
    /// An upstream entity is itself Blocked.
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

/// The pair of axes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EntityState {
    approval: Approval,
    upstream: Upstream,
}

impl EntityState {
    /// A state from its two axes.
    pub fn new(approval: Approval, upstream: Upstream) -> Self {
        Self { approval, upstream }
    }

    /// The approval axis.
    pub fn approval(&self) -> &Approval {
        &self.approval
    }

    /// The upstream axis.
    pub fn upstream(&self) -> &Upstream {
        &self.upstream
    }

    /// Whether an approval of this entity may be recorded now: its content is
    /// valid and nothing upstream is stale or blocked.
    pub fn is_ready_to_approve(&self) -> bool {
        !matches!(self.approval, Approval::Invalid(_)) && matches!(self.upstream, Upstream::Ok)
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

    #[test]
    fn ready_to_approve_needs_valid_content_and_a_sound_upstream() {
        let blocked = Upstream::Blocked(NonEmptyVec::new(
            BlockReason::UpstreamUnapproved { upstream: eid(1) },
            vec![],
        ));
        let invalid = Approval::Invalid(NonEmptyVec::new(
            InvalidReason::DanglingRef { upstream: eid(1) },
            vec![],
        ));
        assert!(EntityState::new(Approval::Draft, Upstream::Ok).is_ready_to_approve());
        assert!(EntityState::new(Approval::Approved, Upstream::Ok).is_ready_to_approve());
        assert!(!EntityState::new(Approval::Draft, blocked).is_ready_to_approve());
        assert!(!EntityState::new(invalid, Upstream::Ok).is_ready_to_approve());
    }
}
