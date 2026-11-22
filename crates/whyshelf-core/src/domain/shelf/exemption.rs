//! The rule of an exemption from a covering link ([`Exempt`]): what one may be made
//! of, and how one reads now.
//!
//! The record is the ledger's ([`Exempt`], [`ExemptOf`], and its canonical form);
//! the rule reads the ledger and the documents both, so it is here, above them, and
//! the assessment of coverage tells its notices with it:
//!
//! ```text
//!   exemption ──► schema, artifact, doc, build, ledger, cover
//!   coverage  ──► cover, exemption       (gaps, the exemptions that stand, repeated)
//!   freshness ──► coverage, exemption    (NotCovered, Exempt, ExemptionUnneeded,
//!                                         ExemptionRepeated)
//! ```
//!
//! One reading, [`exemption_reading`], says whether an exemption is needed now
//! ([`ExemptionReading::Needed`]), why it is not ([`Unneeded`]), or that it cannot be
//! told ([`ExemptionReading::CannotTell`]: the names a selection takes are not all
//! listed, so that a name missing from the listing proves nothing). The subject of an
//! exemption is a document or an artifact ([`ExemptOf`]); the reading of an artifact
//! is given the listings as a [`ListedNames`] and never reads a listing itself.
//! [`frame_exempt`] and [`frame_exempt_artifact`] accept an exemption only when that
//! reading says it would be needed the moment it is written, so acceptance and
//! reading cannot disagree: whatever would read as unneeded or as not told at once is
//! refused, as an approval that would only repeat one is not made.
//!
//! ```text
//!   requirement.grounds  cover --> finding        F-1 reached          refused / exemption_unneeded
//!                                                 F-2 not reached      not_covered
//!   exempt F-2 from requirement.grounds           F-2 not reached      exempt
//!                                                 F-2 reached later    exemption_unneeded
//! ```

use std::collections::BTreeMap;

use thiserror::Error;

use crate::domain::id::DocId;
use crate::domain::nonempty::NonEmptyVec;
use crate::domain::words::{Join, Mark, listed};

use super::artifact::{ArtifactKey, Completeness, ListedNames, Space};
use super::build::{FailureTag, Shelf};
use super::cover::{Decided, Reached, covering_reach, decided_of};
use super::doc::{DocKey, DocLabel};
use super::ledger::{Exempt, ExemptOf, ExemptSubject, JudgementId, Ledger, NotStanding};
use super::schema::{ArtifactCover, BookName, CoverSeverity, EntryName, Schema};

/// Why a link of a book is no covering link of a document's book, or of an artifact
/// of a space.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum CoverMiss {
    /// The book is not declared, or declares no link of that name into a book.
    #[error("no link of that name into a book is declared")]
    NotDeclared,
    /// The book is not declared, or declares no link of that name to artifacts.
    #[error("no link of that name to artifacts is declared")]
    NotDeclaredToArtifacts,
    /// The link is declared and expects nothing (`cover` is false, or not written).
    #[error("the link is declared with no cover")]
    NoCover,
    /// The link covers another book than the document's.
    #[error("the link points into `{target}`, and the document is of `{doc_book}`")]
    OtherBook {
        /// The book the link points into.
        target: BookName,
        /// The document's book.
        doc_book: BookName,
    },
    /// The link covers the names of another space than the artifact's.
    #[error(
        "the link covers the names of `{link_space}`, and the artifact is in `{artifact_space}`"
    )]
    OtherSpace {
        /// The space the link covers.
        link_space: Space,
        /// The artifact's space.
        artifact_space: Space,
    },
    /// The selection of the link does not take the artifact: it is not in the
    /// listing now. The domain says no more than that, as a selection is opaque to it.
    #[error("the selection of the link does not list it now")]
    NotListed,
}

/// The strength with which the link `link` of the book `book` expects to reach every
/// document of `doc_book`, or why it expects nothing of it. With no `doc_book` (a
/// document that is not valid now has no book to tell), only whether the link is
/// declared with a cover is asked.
pub fn cover_of(
    schema: &Schema,
    book: &BookName,
    link: &EntryName,
    doc_book: Option<&BookName>,
) -> Result<CoverSeverity, CoverMiss> {
    let kind = schema
        .book(book)
        .and_then(|b| b.links().get(link))
        .ok_or(CoverMiss::NotDeclared)?;
    let severity = kind.cover.ok_or(CoverMiss::NoCover)?;
    match doc_book {
        Some(doc_book) if *doc_book != kind.target => Err(CoverMiss::OtherBook {
            target: kind.target.clone(),
            doc_book: doc_book.clone(),
        }),
        _ => Ok(severity),
    }
}

/// The cover the link `link` of the book `book` declares over the names of `space`,
/// or why it expects nothing of them: the artifact counterpart of [`cover_of`].
pub fn artifact_cover_of<'a>(
    schema: &'a Schema,
    book: &BookName,
    link: &EntryName,
    space: &Space,
) -> Result<&'a ArtifactCover, CoverMiss> {
    let kind = schema
        .book(book)
        .and_then(|b| b.artifact_links().get(link))
        .ok_or(CoverMiss::NotDeclaredToArtifacts)?;
    let cover = kind.cover.as_ref().ok_or(CoverMiss::NoCover)?;
    if kind.space != *space {
        return Err(CoverMiss::OtherSpace {
            link_space: kind.space.clone(),
            artifact_space: space.clone(),
        });
    }
    Ok(cover)
}

/// The space of the names the link `link` of the book `book` points at, or why the
/// link is no link to artifacts: what names an artifact to exempt from the link, which
/// is told by the link, not by the person who asks.
pub fn artifact_space_of<'a>(
    schema: &'a Schema,
    book: &BookName,
    link: &EntryName,
) -> Result<&'a Space, CoverMiss> {
    schema
        .book(book)
        .and_then(|b| b.artifact_links().get(link))
        .map(|kind| &kind.space)
        .ok_or(CoverMiss::NotDeclaredToArtifacts)
}

/// Why an exemption is not needed now.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Unneeded {
    /// The link reaches the document or the artifact now, from these pending or
    /// approved documents, by identity.
    Reached {
        /// The documents it is reached from.
        by: NonEmptyVec<DocId>,
    },
    /// The link is no covering link of the subject now: no longer declared, declared
    /// with no cover, pointing into another book than the document's, covering
    /// another space than the artifact's, or with a selection that does not list the
    /// artifact now ([`cover_of`], [`artifact_cover_of`], [`CoverMiss`]).
    Cover(CoverMiss),
    /// The document is no valid document now: no book of it can be told.
    NotValid(FailureTag),
    /// The document was decided against ([`NotStanding`], which says how): no
    /// covering link expects to reach it.
    DecidedAgainst(NotStanding),
}

/// How an exemption of `of` reads now: needed, with the strength of the link;
/// unneeded, with why; or that it cannot be told. The one rule of it, for its
/// readers: the assessment of coverage tells it as a notice of every exemption that
/// stands, and [`frame_exempt`] and [`frame_exempt_artifact`] refuse an exemption it
/// would read as unneeded, or as not told, the moment it is written.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExemptionReading {
    /// The link does not reach the subject now, and expects to: the exemption is
    /// needed, for a link of this strength.
    Needed(CoverSeverity),
    /// The exemption is not needed, for this reason.
    Unneeded(Unneeded),
    /// The names the link's selection takes are not all listed (partial, cannot be
    /// listed, or not asked), so that whether the artifact is one of them cannot be
    /// told. Never an unneeded exemption: nothing is said of it.
    CannotTell(Completeness),
}

impl ExemptionReading {
    /// Whether the exemption is needed now.
    pub fn is_needed(&self) -> bool {
        matches!(self, Self::Needed(_))
    }
}

/// How an exemption of `of` reads now.
///
/// | subject | case, in this order | reading |
/// |---|---|---|
/// | a document | the link is no covering link into the document's book now ([`cover_of`]; of a document not valid, only whether the link is declared with a cover) | [`Unneeded::Cover`] |
/// | | the document is no valid document now | [`Unneeded::NotValid`] |
/// | | the document was decided against ([`NotStanding`]: it does not count) | [`Unneeded::DecidedAgainst`] |
/// | | the link reaches the document now, from a document that counts | [`Unneeded::Reached`], with those documents |
/// | | otherwise | [`ExemptionReading::Needed`] |
/// | an artifact | the link is no covering link of the names of the artifact's space now ([`artifact_cover_of`]) | [`Unneeded::Cover`] |
/// | | the link reaches the name now, from a document that counts | [`Unneeded::Reached`], with those documents |
/// | | the names the link's selection takes are not all listed | [`ExemptionReading::CannotTell`] |
/// | | the name is not among them | [`Unneeded::Cover`] of [`CoverMiss::NotListed`] |
/// | | otherwise | [`ExemptionReading::Needed`] |
///
/// What is listed is `listed`, read by [`ListedNames::of`] from what the command
/// asked of the adapters; this reads none of it itself.
pub fn exemption_reading(
    shelf: &Shelf,
    ledger: &Ledger,
    of: &ExemptOf,
    listed: &ListedNames,
) -> ExemptionReading {
    let decided = |id| decided_of(ledger, id);
    read_exemption(shelf, decided, &covering_reach(shelf, decided), listed, of)
}

/// [`exemption_reading`], on what was decided of each document and what the covering
/// links reach, as an assessment has them already.
pub(super) fn read_exemption(
    shelf: &Shelf,
    decided: impl Fn(DocId) -> Decided,
    reached: &Reached,
    listed: &ListedNames,
    of: &ExemptOf,
) -> ExemptionReading {
    match of.subject() {
        ExemptSubject::Doc(id) => {
            match read_doc(shelf, decided, reached, *id, of.book(), of.link()) {
                Ok(severity) => ExemptionReading::Needed(severity),
                Err(why) => ExemptionReading::Unneeded(why),
            }
        }
        ExemptSubject::Artifact(artifact) => {
            match read_artifact(shelf, reached, listed, artifact, of.book(), of.link()) {
                ArtifactRead::Needed(severity) => ExemptionReading::Needed(severity),
                ArtifactRead::Cover(miss) => ExemptionReading::Unneeded(Unneeded::Cover(miss)),
                ArtifactRead::Reached { by } => {
                    ExemptionReading::Unneeded(Unneeded::Reached { by })
                }
                ArtifactRead::CannotTell(completeness) => {
                    ExemptionReading::CannotTell(completeness)
                }
            }
        }
    }
}

/// The reading of an exemption of a document.
fn read_doc(
    shelf: &Shelf,
    decided: impl Fn(DocId) -> Decided,
    reached: &Reached,
    id: DocId,
    book: &BookName,
    link: &EntryName,
) -> Result<CoverSeverity, Unneeded> {
    let doc = shelf.state_of(id).valid_or_failure();
    let doc_book = doc.as_ref().ok().map(|d| &d.key().book);
    let severity = cover_of(shelf.schema(), book, link, doc_book).map_err(Unneeded::Cover)?;
    if let Err(failure) = doc {
        return Err(Unneeded::NotValid(failure.tag()));
    }
    if let Decided::Against(status) = decided(id) {
        return Err(Unneeded::DecidedAgainst(status));
    }
    let from = reached
        .doc_from(id, book, link)
        .map(|from| from.iter().copied().collect::<Vec<_>>())
        .and_then(|from| NonEmptyVec::from_vec(from).ok());
    match from {
        Some(by) => Err(Unneeded::Reached { by }),
        None => Ok(severity),
    }
}

/// How an exemption of an artifact reads: [`ExemptionReading`] without the reasons
/// only a document has ([`Unneeded::NotValid`], [`Unneeded::DecidedAgainst`]), so that
/// what refuses an exemption of an artifact has no case that cannot come.
enum ArtifactRead {
    Needed(CoverSeverity),
    Cover(CoverMiss),
    Reached { by: NonEmptyVec<DocId> },
    CannotTell(Completeness),
}

/// The reading of an exemption of an artifact.
fn read_artifact(
    shelf: &Shelf,
    reached: &Reached,
    listed: &ListedNames,
    artifact: &ArtifactKey,
    book: &BookName,
    link: &EntryName,
) -> ArtifactRead {
    let cover = match artifact_cover_of(shelf.schema(), book, link, artifact.space()) {
        Ok(cover) => cover,
        Err(miss) => return ArtifactRead::Cover(miss),
    };
    let from = reached
        .artifact_from(book, link, artifact.name())
        .map(|from| from.iter().copied().collect::<Vec<_>>())
        .and_then(|from| NonEmptyVec::from_vec(from).ok());
    if let Some(by) = from {
        return ArtifactRead::Reached { by };
    }
    match listed.names(artifact.space(), cover.selection()) {
        Err(completeness) => ArtifactRead::CannotTell(completeness),
        Ok(names) if names.contains(artifact.name()) => ArtifactRead::Needed(cover.severity()),
        Ok(_) => ArtifactRead::Cover(CoverMiss::NotListed),
    }
}

/// The exemptions that stand, by what they are of, each group in the order of
/// identity. Two or more of one (document, book, link) come of branches merged,
/// each of which made one: freshness tells them as repeated.
pub(super) fn by_subject(ledger: &Ledger) -> BTreeMap<&ExemptOf, Vec<&Exempt>> {
    let mut groups: BTreeMap<&ExemptOf, Vec<&Exempt>> = BTreeMap::new();
    for exempt in ledger.exemptions() {
        groups.entry(exempt.of()).or_default().push(exempt);
    }
    groups
}

/// Why an exemption is refused.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum ExemptRefusal {
    /// The document is not a valid document now, so its book cannot be told.
    #[error("{}", tag.said_of(id))]
    DocNotValid {
        /// The document.
        id: DocId,
        /// How it is not.
        tag: FailureTag,
    },
    /// The link is no covering link into the document's book. The cause is the
    /// source, told once.
    #[error("{book}.{link} cannot exempt it")]
    NotCovering {
        /// The book named.
        book: BookName,
        /// The link named.
        link: EntryName,
        /// Why.
        #[source]
        miss: CoverMiss,
    },
    /// The document was decided against ([`NotStanding`], which says how): no
    /// covering link expects to reach it, so there is nothing to exempt it from.
    #[error("{} is decided against: no covering link expects to reach it", DocLabel::new(*id, key.clone()))]
    DecidedAgainst {
        /// The document.
        id: DocId,
        /// Its key, by which the message names it; the identity names it when it has
        /// none.
        key: Option<DocKey>,
        /// How it was decided against. The message does not say it (the refusal is the
        /// same whichever decision it was); the field is for a library caller that
        /// reads the refusal.
        status: NotStanding,
    },
    /// The link reaches the document now: the exemption would be unneeded at once.
    #[error("{book}.{link} reaches it now, from {}", listed(.by.iter(), Mark::Plain, Join::Comma))]
    Reached {
        /// The book.
        book: BookName,
        /// The link.
        link: EntryName,
        /// The documents it is reached from, by their keys, in the order of identity.
        by: Vec<DocKey>,
    },
    /// The names the link's selection takes are not all listed, so that whether the
    /// artifact is reached by the link, or is one it expects, cannot be told: the
    /// exemption would read as not told at once.
    #[error("{book}.{link} cannot tell whether it needs an exemption: {completeness}")]
    CannotTell {
        /// The book.
        book: BookName,
        /// The link.
        link: EntryName,
        /// How complete the listing is: never all of the names.
        completeness: Completeness,
    },
    /// An exemption that stands is already of the same document or artifact from the
    /// same link.
    #[error("{record} already exempts it from {book}.{link}")]
    AlreadyExempt {
        /// The book.
        book: BookName,
        /// The link.
        link: EntryName,
        /// The exemption that stands.
        record: JudgementId,
    },
    /// An earlier exemption of the same run, not written yet, is of the same
    /// document from the same link: the document is named twice. The application
    /// tells this apart from [`ExemptRefusal::AlreadyExempt`], since only it knows
    /// which records are of the run.
    #[error("an earlier exemption of this run already exempts it from {book}.{link}")]
    AlreadyExemptInRun {
        /// The book.
        book: BookName,
        /// The link.
        link: EntryName,
    },
}

/// What an exemption of the document `id` from the link `link` of the book `book`
/// would be of.
///
/// Accepted when the exemption would be needed the moment it is written, as
/// [`exemption_reading`] reads it (the document valid now, the link declared with a
/// cover of either strength into its book, the document not decided against, and
/// not reached by the link now), and no exemption that stands
/// ([`Ledger::exemptions`]) is of the same document from the same link. Refused
/// otherwise, before any record exists: an exemption unneeded at once adds nothing
/// to the ledger. A second exemption is refused, not repeated: it would be a second
/// record of one fact, with a reason of its own, and withdrawing one of the two
/// would leave the document exempt with no word of it.
///
/// The one place what an exemption of a document may be made of is decided; of an
/// artifact, [`frame_exempt_artifact`], by the same rule.
pub fn frame_exempt(
    shelf: &Shelf,
    ledger: &Ledger,
    id: DocId,
    book: BookName,
    link: EntryName,
) -> Result<ExemptOf, ExemptRefusal> {
    if let Err(failure) = shelf.state_of(id).valid_or_failure() {
        return Err(ExemptRefusal::DocNotValid {
            id,
            tag: failure.tag(),
        });
    }
    let of = ExemptOf::new(id, book, link);
    let decided = |id| decided_of(ledger, id);
    let refusal = match read_doc(
        shelf,
        decided,
        &covering_reach(shelf, decided),
        id,
        of.book(),
        of.link(),
    ) {
        Ok(_) => None,
        Err(Unneeded::Cover(miss)) => Some(not_covering(&of, miss)),
        Err(Unneeded::NotValid(tag)) => Some(ExemptRefusal::DocNotValid { id, tag }),
        Err(Unneeded::DecidedAgainst(status)) => Some(ExemptRefusal::DecidedAgainst {
            id,
            key: shelf.doc(id).map(|doc| doc.key().clone()),
            status,
        }),
        Err(Unneeded::Reached { by }) => Some(reached_refusal(shelf, &of, &by)),
    };
    first_exemption(ledger, of, refusal)
}

/// What an exemption of the artifact `artifact` from the link `link` of the book
/// `book` would be of: [`frame_exempt`] for an artifact, by the same rule.
///
/// Accepted when the exemption would be needed the moment it is written, as
/// [`exemption_reading`] reads it of an artifact (the link declared with a cover over
/// the artifact's space, no document that counts writes the name in it, the
/// artifact's space listed whole in `listed`, and the name among those the selection
/// takes), and no exemption that stands is of the same artifact from the same link.
/// Refused otherwise, before any record exists: one that reads as unneeded or as not
/// told at once adds nothing to the ledger, and a second is refused, not repeated.
pub fn frame_exempt_artifact(
    shelf: &Shelf,
    ledger: &Ledger,
    artifact: ArtifactKey,
    book: BookName,
    link: EntryName,
    listed: &ListedNames,
) -> Result<ExemptOf, ExemptRefusal> {
    let of = ExemptOf::of_artifact(artifact.clone(), book, link);
    let refusal = match read_artifact(
        shelf,
        &covering_reach(shelf, |id| decided_of(ledger, id)),
        listed,
        &artifact,
        of.book(),
        of.link(),
    ) {
        ArtifactRead::Needed(_) => None,
        ArtifactRead::Cover(miss) => Some(not_covering(&of, miss)),
        ArtifactRead::Reached { by } => Some(reached_refusal(shelf, &of, &by)),
        ArtifactRead::CannotTell(completeness) => Some(ExemptRefusal::CannotTell {
            book: of.book().clone(),
            link: of.link().clone(),
            completeness,
        }),
    };
    first_exemption(ledger, of, refusal)
}

/// The refusal that the link is no covering link of the subject.
fn not_covering(of: &ExemptOf, miss: CoverMiss) -> ExemptRefusal {
    ExemptRefusal::NotCovering {
        book: of.book().clone(),
        link: of.link().clone(),
        miss,
    }
}

/// The refusal that the link reaches the subject now. A document a covering link
/// reaches from is valid, so it has a key.
fn reached_refusal(shelf: &Shelf, of: &ExemptOf, by: &NonEmptyVec<DocId>) -> ExemptRefusal {
    ExemptRefusal::Reached {
        book: of.book().clone(),
        link: of.link().clone(),
        by: by
            .iter()
            .filter_map(|from| shelf.doc(*from).map(|doc| doc.key().clone()))
            .collect(),
    }
}

/// `of`, unless the reading refused it, or an exemption that stands is already of
/// the same subject from the same link.
fn first_exemption(
    ledger: &Ledger,
    of: ExemptOf,
    refusal: Option<ExemptRefusal>,
) -> Result<ExemptOf, ExemptRefusal> {
    if let Some(refusal) = refusal {
        return Err(refusal);
    }
    if let Some(standing) = ledger.exemptions().find(|e| *e.of() == of) {
        return Err(ExemptRefusal::AlreadyExempt {
            book: of.book().clone(),
            link: of.link().clone(),
            record: standing.id(),
        });
    }
    Ok(of)
}

#[cfg(test)]
mod tests;
