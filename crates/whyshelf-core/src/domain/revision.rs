//! Revisions: the version of the code an approval refers to.
//!
//! The domain speaks only of a *revision*, of being *integrated* into a
//! baseline, and of the *approved revision*. Branch names, merge strategies and
//! hash lengths belong to the version-control adapter behind a port.
//!
//! # Opaque identifiers
//!
//! A [`RevisionId`] is an opaque, non-empty identifier. Whether it has the right
//! form (for Git, 40 hex digits) is the adapter's rule: the adapter validates
//! the text through its port, and only then makes a `RevisionId` with
//! [`RevisionId::from_validated`]. The domain trusts the value and offers
//! comparison and display only. Nothing in the types forces the call through
//! the port; that is a promise of the adapters.
//!
//! # Variants
//!
//! [`Revision`] is an enum so that a second way of naming a revision (a patch
//! on a base) can be added without touching the code that handles commits.

use std::fmt;

use super::canon::{Canon, ToCanon};
use super::error::ValueError;
use super::hash::ContentHash;

/// What the version-control side can say of one revision against the baseline.
///
/// Three answers, not two: a clone that lacks the baseline ref or the approved commit
/// (a shallow or single-branch checkout of a CI machine) cannot say "no", and calling
/// it "no" would show work as pending that may well be integrated. The command goes
/// on with the question open.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum IntegrationFact {
    /// The revision has reached the baseline.
    Integrated,
    /// The revision has not reached the baseline.
    NotIntegrated,
    /// It cannot be told here.
    Unknown {
        /// Why not.
        reason: UnknownIntegration,
    },
}

/// Why [`IntegrationFact::Unknown`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum UnknownIntegration {
    /// The baseline is not a revision of this repository (the ref is not fetched).
    BaselineUnresolved,
    /// The revision is not in this repository (not fetched, or beyond a shallow cut).
    RevisionAbsent,
}

impl fmt::Display for UnknownIntegration {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::BaselineUnresolved => "the baseline is not in this clone",
            Self::RevisionAbsent => "the approved revision is not in this clone",
        })
    }
}

/// An opaque identifier of a revision of the code.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct RevisionId(String);

impl RevisionId {
    /// Wrap an identifier that the revision port already validated.
    ///
    /// Only an adapter calls this, after its `validate`. Only the weakest
    /// rules are checked here: not empty, no white space or control character.
    pub fn from_validated(raw: String) -> Result<Self, ValueError> {
        if raw.is_empty() {
            return Err(ValueError::RevisionEmpty);
        }
        if raw.chars().any(|c| c.is_whitespace() || c.is_control()) {
            return Err(ValueError::RevisionBadChar);
        }
        Ok(Self(raw))
    }

    /// The identifier as text.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for RevisionId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl ToCanon for RevisionId {
    fn to_canon(&self) -> Canon {
        Canon::str(self.0.as_str())
    }
}

/// How a revision of the code is named.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Revision {
    /// A committed revision.
    Commit(RevisionId),
    /// An uncommitted change kept as a patch on a base revision.
    Patch {
        /// The revision the patch applies to.
        base: RevisionId,
        /// The hash of the patch content.
        patch: ContentHash,
    },
}

impl Revision {
    /// The committed revision, or the base a patch applies to.
    ///
    /// Integration is asked of this identifier: a patch itself is never
    /// integrated, only its base can be.
    pub fn base(&self) -> &RevisionId {
        match self {
            Self::Commit(id) => id,
            Self::Patch { base, .. } => base,
        }
    }

    /// Whether this names a committed revision.
    pub fn is_commit(&self) -> bool {
        matches!(self, Self::Commit(_))
    }
}

impl ToCanon for Revision {
    fn to_canon(&self) -> Canon {
        match self {
            Self::Commit(id) => Canon::map([("form", Canon::str("commit")), ("id", id.to_canon())]),
            Self::Patch { base, patch } => Canon::map([
                ("form", Canon::str("patch")),
                ("base", base.to_canon()),
                ("patch", patch.to_canon()),
            ]),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn revision_id_is_opaque_but_not_empty() {
        assert_eq!(
            RevisionId::from_validated(String::new()),
            Err(ValueError::RevisionEmpty)
        );
        assert_eq!(
            RevisionId::from_validated("a b".into()),
            Err(ValueError::RevisionBadChar)
        );
        assert_eq!(
            RevisionId::from_validated("f4e5d6c".into())
                .unwrap()
                .as_str(),
            "f4e5d6c"
        );
    }

    #[test]
    fn a_patch_is_integrated_through_its_base() {
        let base = RevisionId::from_validated("b1".into()).unwrap();
        let patch = Revision::Patch {
            base: base.clone(),
            patch: ContentHash::raw(b"diff"),
        };
        assert_eq!(patch.base(), &base);
        assert!(!patch.is_commit());
        assert!(Revision::Commit(base).is_commit());
    }

    #[test]
    fn commit_and_patch_have_distinct_canonical_forms() {
        let id = RevisionId::from_validated("b1".into()).unwrap();
        let commit = Revision::Commit(id.clone());
        let patch = Revision::Patch {
            base: id,
            patch: ContentHash::raw(b"diff"),
        };
        assert_ne!(commit.to_canon(), patch.to_canon());
    }
}
