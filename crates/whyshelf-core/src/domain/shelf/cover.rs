//! What a covering link reaches: the fact of `cover` that the notices of freshness
//! and the rule of an exemption both read.
//!
//! ```text
//!   cover     ──► build, doc, ledger     counts ──► whether a document counts
//!                                        covering_reach ──► what covering links reach
//!   exemption ──► cover, ledger, ...     reads it, to accept and to read an exemption
//!   freshness ──► cover, exemption       NotCovered, Exempt, ExemptionUnneeded
//! ```
//!
//! It is here, below both, so that neither imports the other's: freshness tells the
//! notices with what [`exemption`](super::exemption) reads, and the exemption reads
//! nothing of freshness. Which documents count is [`counts`], the one statement of
//! it: what the ledger declares and a [`Standing`](super::freshness::Standing)
//! each come to a [`Decided`], and [`Standing::stands`] is `counts` of its own.
//!
//! [`Standing::stands`]: super::freshness::Standing::stands

use std::collections::{BTreeMap, BTreeSet};

use crate::domain::id::DocId;

use super::build::Shelf;
use super::doc::LinkState;
use super::ledger::DeclaredStatus;
use super::schema::{BookName, EntryName};

/// What was decided about a document, without what each decision carries: the one
/// shape that a declared status and a standing both come to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Decided {
    /// No decision stands.
    Pending,
    /// An approval stands, at whatever freshness.
    Approved,
    /// A denial stands.
    Denied,
    /// A deprecation stands.
    Deprecated,
    /// A retirement stands.
    Retired,
}

impl From<&DeclaredStatus> for Decided {
    fn from(status: &DeclaredStatus) -> Self {
        match status {
            DeclaredStatus::Pending => Self::Pending,
            DeclaredStatus::Approved { .. } => Self::Approved,
            DeclaredStatus::Denied { .. } => Self::Denied,
            DeclaredStatus::Deprecated { .. } => Self::Deprecated,
            DeclaredStatus::Retired { .. } => Self::Retired,
        }
    }
}

/// Whether a document counts for coverage (and stands): no decision went against
/// it, it is pending or approved. What was denied, deprecated or retired neither
/// reaches nor is expected to be reached. The one statement of it:
/// [`Standing::stands`] is this of the standing, and an exemption reads it of what
/// the ledger declares.
///
/// [`Standing::stands`]: super::freshness::Standing::stands
pub fn counts(decided: Decided) -> bool {
    matches!(decided, Decided::Pending | Decided::Approved)
}

/// Each document a covering link reaches, by (document, book of the link, link),
/// with the documents it is reached from.
pub type Reached = BTreeMap<(DocId, BookName, EntryName), BTreeSet<DocId>>;

/// What the covering links (a [`LinkKind::cover`] of some strength) reach from the
/// valid documents `counted` says count ([`counts`]): the one place a covering link
/// is followed.
///
/// [`LinkKind::cover`]: super::schema::LinkKind::cover
pub fn covering_reach(shelf: &Shelf, counted: impl Fn(DocId) -> bool) -> Reached {
    let mut reached: Reached = BTreeMap::new();
    for doc in shelf.docs().values() {
        if !counted(doc.id()) {
            continue;
        }
        for (name, link) in doc.links() {
            if link.kind().cover.is_none() {
                continue;
            }
            for state in link.states() {
                if let LinkState::Resolved(target) = state {
                    reached
                        .entry((target.id(), doc.key().book.clone(), name.clone()))
                        .or_default()
                        .insert(doc.id());
                }
            }
        }
    }
    reached
}
