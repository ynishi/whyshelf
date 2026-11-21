//! What a covering link reaches: the fact of `cover` that the assessment of coverage
//! and the rule of an exemption both read.
//!
//! ```text
//!   cover     ──► schema, artifact, doc,    counts ──► whether a document counts
//!                 build, ledger               covering_reach ──► Reached { docs, artifacts }
//!   exemption ──► cover, ledger, ...         reads it, to accept and to read an exemption
//!   coverage  ──► cover, exemption, ...      gaps, exemptions, repeated: what a covering link
//!                                            expects and does not reach (CoverAssessment)
//!   freshness ──► coverage, cover,           NotCovered, Exempt, ExemptionUnneeded,
//!                 exemption                  ExemptionRepeated, told from CoverAssessment
//! ```
//!
//! It is here, at the bottom of the three, so that none imports another's above it:
//! coverage decides what is missing and what is exempt with what
//! [`exemption`](super::exemption) reads, freshness tells the notices from what
//! [`coverage`](super::coverage) decided, and the exemption reads nothing of either.
//! Which documents count is [`counts`], the one statement of it: what the ledger
//! declares and a [`Standing`](super::freshness::Standing) each come to a
//! [`Decided`], and [`Standing::stands`] is `counts` of its own. What a covering link
//! reaches is [`covering_reach`], the one walker, for documents and for artifacts
//! alike ([`Reached`]).
//!
//! [`Standing::stands`]: super::freshness::Standing::stands

use std::collections::{BTreeMap, BTreeSet};

use crate::domain::id::DocId;

use super::artifact::ArtifactName;
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

/// What the covering links reach: each document, by (document, book of the link,
/// link), and each name of an artifact, by (book of the link, link, name), with the
/// documents it is reached from.
///
/// An artifact is reached when a document that counts writes its name in the same
/// link (the names are equal, [`ArtifactName`]); the space is the link's, so it is not
/// part of the key.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Reached {
    docs: BTreeMap<(DocId, BookName, EntryName), BTreeSet<DocId>>,
    artifacts: BTreeMap<(BookName, EntryName, ArtifactName), BTreeSet<DocId>>,
}

impl Reached {
    /// The documents that reach the document `target` through the link `link` of the
    /// book `book`, when some does.
    pub fn doc_from(
        &self,
        target: DocId,
        book: &BookName,
        link: &EntryName,
    ) -> Option<&BTreeSet<DocId>> {
        self.docs.get(&(target, book.clone(), link.clone()))
    }

    /// The documents that reach the name `name` through the link `link` of the book
    /// `book`, when some does.
    pub fn artifact_from(
        &self,
        book: &BookName,
        link: &EntryName,
        name: &ArtifactName,
    ) -> Option<&BTreeSet<DocId>> {
        self.artifacts
            .get(&(book.clone(), link.clone(), name.clone()))
    }
}

/// What the covering links (a [`LinkKind::cover`] or an
/// [`ArtifactLinkKind::cover`] of some strength) reach from the valid documents
/// `counted` says count ([`counts`]): the one place a covering link is followed.
///
/// [`LinkKind::cover`]: super::schema::LinkKind::cover
/// [`ArtifactLinkKind::cover`]: super::schema::ArtifactLinkKind::cover
pub fn covering_reach(shelf: &Shelf, counted: impl Fn(DocId) -> bool) -> Reached {
    let mut reached = Reached::default();
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
                        .docs
                        .entry((target.id(), doc.key().book.clone(), name.clone()))
                        .or_default()
                        .insert(doc.id());
                }
            }
        }
        for (name, link) in doc.artifact_links() {
            if link.kind().cover.is_none() {
                continue;
            }
            for artifact in link.artifacts() {
                reached
                    .artifacts
                    .entry((
                        doc.key().book.clone(),
                        name.clone(),
                        artifact.name().clone(),
                    ))
                    .or_default()
                    .insert(doc.id());
            }
        }
    }
    reached
}
