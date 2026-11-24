//! What a covering link reaches: the fact of `cover` that the assessment of coverage
//! and the rule of an exemption both read.
//!
//! ```text
//!   cover     ──► schema, artifact, doc,    decided_of ──► what was decided of a document
//!                 build, ledger               counts ──► whether a document counts
//!                                             covering_reach ──► Reached { docs, artifacts }
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
//! declares ([`decided_of`]) and a [`Standing`](super::freshness::Standing) each come
//! to a [`Decided`], and [`Standing::stands`] is `counts` of its own. What a covering
//! link reaches is [`covering_reach`], the one walker, for documents and for artifacts
//! alike ([`Reached`]). Which decisions go against a document is not said here: it is
//! [`NotStanding`]'s, which a [`Decided`] only carries.
//!
//! [`Standing::stands`]: super::freshness::Standing::stands

use std::collections::{BTreeMap, BTreeSet};

use crate::domain::id::DocId;

use super::artifact::{ArtifactKey, ArtifactName, Completeness, ListedNames, ListingPart, Space};
use super::build::Shelf;
use super::doc::LinkState;
use super::ledger::{DeclaredStatus, Ledger, NotStanding};
use super::schema::{ArtifactCover, BookName, EntryName};

/// What was decided about a document, without what each decision carries: the one
/// shape that a declared status and a standing both come to. A decision against it
/// carries which one ([`NotStanding`]); this says no more of which decisions those are.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Decided {
    /// No decision stands.
    Pending,
    /// An approval stands, at whatever freshness.
    Approved,
    /// A decision against it stands.
    Against(NotStanding),
}

impl From<&DeclaredStatus> for Decided {
    fn from(status: &DeclaredStatus) -> Self {
        match status {
            DeclaredStatus::Pending => Self::Pending,
            DeclaredStatus::Approved(_) => Self::Approved,
            DeclaredStatus::Against { status, .. } => Self::Against(*status),
        }
    }
}

/// What the ledger declares of the document `id`, as a [`Decided`]: the one place that
/// is made of a ledger, for what reads it of any document (the assessment of
/// coverage, the rule of an exemption).
pub(crate) fn decided_of(ledger: &Ledger, id: DocId) -> Decided {
    Decided::from(&ledger.status_of(id))
}

/// Whether a document counts for coverage (and stands): no decision went against
/// it. What was decided against neither reaches nor is expected to be reached. The one
/// statement of it: [`Standing::stands`] is this of the standing, and an exemption
/// reads it of what the ledger declares.
///
/// [`Standing::stands`]: super::freshness::Standing::stands
pub(crate) fn counts(decided: Decided) -> bool {
    !matches!(decided, Decided::Against(_))
}

/// What the covering links reach: each document, by (document, book of the link,
/// link), and each name of an artifact, by (book of the link, link, name), with the
/// documents it is reached from.
///
/// An artifact is reached when a document that counts writes its name in the same
/// link (the names are equal, [`ArtifactName`]); the space is the link's, so it is not
/// part of the key.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct Reached {
    docs: BTreeMap<(DocId, BookName, EntryName), BTreeSet<DocId>>,
    artifacts: BTreeMap<(BookName, EntryName, ArtifactName), BTreeSet<DocId>>,
}

impl Reached {
    /// The documents that reach the document `target` through the link `link` of the
    /// book `book`, when some does.
    pub(crate) fn doc_from(
        &self,
        target: DocId,
        book: &BookName,
        link: &EntryName,
    ) -> Option<&BTreeSet<DocId>> {
        self.docs.get(&(target, book.clone(), link.clone()))
    }

    /// The documents that reach the name `name` through the link `link` of the book
    /// `book`, when some does.
    pub(crate) fn artifact_from(
        &self,
        book: &BookName,
        link: &EntryName,
        name: &ArtifactName,
    ) -> Option<&BTreeSet<DocId>> {
        self.artifacts
            .get(&(book.clone(), link.clone(), name.clone()))
    }
}

/// The documents that reach the part `part` of a listing that the cover of a link of
/// the book `book` takes whole through its link `whole`, when some do: the one rule of
/// how a part stands, which the assessment of coverage and the reading of an
/// exemption of a part both read. A part is expected whatever was read of it, and
/// nothing but a document that counts and writes its name under `whole` reaches it.
pub(crate) fn part_reach<'a>(
    reached: &'a Reached,
    book: &BookName,
    whole: &EntryName,
    part: &ArtifactKey,
) -> Option<&'a BTreeSet<DocId>> {
    reached.artifact_from(book, whole, part.name())
}

/// The listing of the selection of a cover as the cover reads it: by name, and by
/// part when it takes each part whole through a link of `whole_space`.
pub(crate) enum ListingRead<'a> {
    /// Everything it takes is assessed: the names listed, and the parts.
    Whole {
        names: &'a BTreeSet<ArtifactName>,
        parts: &'a [ListingPart],
    },
    /// The listing is not the whole, and the cover does not take it whole.
    NotWhole(Completeness),
    /// Parts are in another space than the link of parts: no document can reach
    /// them, and nothing of the link is assessed.
    OtherSpace(Vec<&'a ListingPart>),
}

/// How the cover `cover` over `space` reads the listing of its selection, with
/// `whole_space` the space of the link it takes parts whole through (none when it
/// declares no `parts`): the one rule of what of a link is assessed, which the
/// assessment of coverage and the reading of an exemption both read.
pub(crate) fn read_listing<'a>(
    listed: &'a ListedNames,
    space: &Space,
    cover: &ArtifactCover,
    whole_space: Option<&Space>,
) -> ListingRead<'a> {
    let selection = cover.selection();
    let Some(whole_space) = whole_space else {
        return match listed.names(space, selection) {
            Ok(names) => ListingRead::Whole { names, parts: &[] },
            Err(completeness) => ListingRead::NotWhole(completeness),
        };
    };
    match listed.names_and_parts(space, selection) {
        Err(completeness) => ListingRead::NotWhole(completeness),
        Ok((names, parts)) => {
            let other: Vec<&ListingPart> = parts
                .iter()
                .filter(|part| part.key().space() != whole_space)
                .collect();
            if other.is_empty() {
                ListingRead::Whole { names, parts }
            } else {
                ListingRead::OtherSpace(other)
            }
        }
    }
}

/// The space of the link a cover of the book `book` takes parts whole through, when
/// it declares one that the book declares.
pub(crate) fn whole_space_of<'a>(
    shelf: &'a Shelf,
    book: &BookName,
    cover: &ArtifactCover,
) -> Option<&'a Space> {
    let whole = cover.parts_whole()?;
    shelf
        .schema()
        .book(book)?
        .artifact_links()
        .get(whole)
        .map(|kind| &kind.space)
}

/// What the covering links (a [`LinkKind::cover`] or an
/// [`ArtifactLinkKind::cover`] of some strength) reach from the valid documents
/// that count (`decided` says what was decided of each, and [`counts`] reads it): the
/// one place a covering link is followed. A link to artifacts that the cover of
/// another link of its book names for its parts ([`ArtifactCover::parts_whole`]) is
/// followed as well, so that a part is reached through it.
///
/// [`LinkKind::cover`]: super::schema::LinkKind::cover
/// [`ArtifactLinkKind::cover`]: super::schema::ArtifactLinkKind::cover
/// [`ArtifactCover::parts_whole`]: super::schema::ArtifactCover::parts_whole
pub(crate) fn covering_reach(shelf: &Shelf, decided: impl Fn(DocId) -> Decided) -> Reached {
    let mut reached = Reached::default();
    let mut wholes: BTreeSet<(&BookName, &EntryName)> = BTreeSet::new();
    for (book, schema) in shelf.schema().books() {
        for (_, kind) in schema.artifact_links() {
            if let Some(whole) = kind.cover.as_ref().and_then(|c| c.parts_whole()) {
                wholes.insert((book, whole));
            }
        }
    }
    for doc in shelf.docs().values() {
        if !counts(decided(doc.id())) {
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
            if link.kind().cover.is_none() && !wholes.contains(&(&doc.key().book, name)) {
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
