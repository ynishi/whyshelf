//! Why a declaration that an edit keeps the meaning is refused: [`SameMeaningRefusal`],
//! and why a pin of it is not compared now, [`NotComparedWhy`].
//!
//! The order in which the refusals of what `judge same` offers are decided is that of
//! the one place the pins of a name are read for them, in the module that compares the
//! pins now; this module only names them.

use crate::domain::actor::Actor;
use crate::domain::id::DocId;
use crate::domain::words::{Join, Mark, listed};

use super::super::artifact::{ArtifactKey, ArtifactName, Space, Untold};
use super::super::build::FailureTag;

/// Why a declaration is refused.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SameMeaningRefusal {
    /// The document is not a valid document now, so it has no hash to declare.
    #[error("{}", tag.said_of(id))]
    DocNotValid {
        /// The document.
        id: DocId,
        /// How it is not.
        tag: FailureTag,
    },
    /// No document links the name, so what is there was never read.
    #[error("no document links `{name}`")]
    ArtifactNotLinked {
        /// The name.
        name: ArtifactName,
    },
    /// Documents link the name in more than one space, so which artifact is meant
    /// cannot be told.
    #[error("documents link `{name}` in more than one space: {}", listed(.spaces.iter(), Mark::Backtick, Join::And))]
    ArtifactInSpaces {
        /// The name.
        name: ArtifactName,
        /// The spaces.
        spaces: Vec<Space>,
    },
    /// A pin of the earlier hash has nothing behind it now.
    #[error("{artifact} has nothing there now")]
    ArtifactAbsent {
        /// The artifact.
        artifact: ArtifactKey,
    },
    /// Its adapter cannot tell about a pin of the earlier hash now.
    #[error("{artifact} cannot be told about now: {why}")]
    ArtifactUnknown {
        /// The artifact.
        artifact: ArtifactKey,
        /// Why it cannot be told.
        why: Untold,
    },
    /// No pin of it holds a hash that stale tells as changed: each pin that pinned a
    /// hash is at the hash now or at one that already means the same as it, or none
    /// pinned a hash.
    #[error(
        "no pin of it holds an earlier hash stale tells as changed: there is nothing to declare"
    )]
    NothingToDeclare,
    /// The one who declares it declared the same edit already, in a declaration not
    /// revoked ([`Framed::not_declared_by`](super::Framed::not_declared_by)): one name
    /// counts once, so it would add nothing, and the edit awaits the declaration of
    /// another name.
    #[error("`{actor}` already declared this edit: it awaits the declaration of another name")]
    AlreadyDeclaredBy {
        /// The one who declares it.
        actor: Actor,
    },
    /// The hash named as the earlier one is no hash a pin of it holds as changed: no
    /// pin holds it, or it is the hash now, or already means the same as it.
    #[error(
        "no approval that stands pinned {hash} for it as a change: nothing pins it, it is the hash now, or it already means the same"
    )]
    NotPinned {
        /// The hash named, as text.
        hash: String,
    },
    /// A pin of the earlier hash is one stale does not compare now, for the reason
    /// given: an edit of it cannot be declared until that is mended, or the approval is
    /// made again.
    #[error("{}", not_compared(.hash.as_deref(), .why))]
    NotComparedNow {
        /// The hash named, as text; none when no hash was named.
        hash: Option<String>,
        /// Why stale does not compare the pin now.
        why: NotComparedWhy,
    },
    /// Approvals pinned earlier hashes that do not mean the same as one another, and
    /// none was named: which edit was read cannot be told.
    #[error("approvals pinned {} earlier hashes that do not mean the same: name the one the edit was read from", hashes.len())]
    SeveralEarlier {
        /// The hashes, as text.
        hashes: Vec<String>,
    },
}

/// Why stale does not compare a pin now, so that no edit of it can be declared.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NotComparedWhy {
    /// The link of the artifact declares another comparison than the one it was pinned
    /// with: the approval is to be made again.
    ComparisonChanged,
    /// The link no longer writes the name the pin is read as.
    LinkDropped,
    /// The document of the approval that pinned it is not valid now.
    DocumentNotValid,
}

impl std::fmt::Display for NotComparedWhy {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::ComparisonChanged => "the comparison its link declares changed",
            Self::LinkDropped => "its link no longer writes the name",
            Self::DocumentNotValid => "the document that pinned it is not valid",
        })
    }
}

/// The words of [`SameMeaningRefusal::NotComparedNow`].
fn not_compared(hash: Option<&str>, why: &NotComparedWhy) -> String {
    match hash {
        Some(hash) => format!(
            "{hash} is pinned, and stale does not compare that pin now ({why}): mend that, or approve again"
        ),
        None => format!(
            "an earlier hash of it is pinned, and stale does not compare those pins now ({why}): mend that, or approve again"
        ),
    }
}
