//! What the covering links expect and do not reach: the one assessment of coverage,
//! for documents and for artifacts alike.
//!
//! A link declared with `cover` expects its targets to be reached by a document that
//! counts: every document of the book a link into a book points into that no
//! decision went against ([`LinkKind::cover`]), and every name its selection takes in
//! the space of a link to artifacts ([`ArtifactLinkKind::cover`]). [`assess`] is the
//! one place that
//! says which of them nobody reaches, which exemptions stand and what each reads as,
//! which are repeated, and how complete each listing of names is:
//!
//! ```text
//!   cover      covering_reach ──► Reached { docs, artifacts }
//!   exemption  exemption_reading, by_subject ──► what each exemption that stands reads as
//!   coverage   assess(shelf, ledger, &Listings) ──► CoverAssessment
//!                ├─ gaps          target, link, strength: expected, not reached, not exempt
//!                ├─ exemptions    each (subject, link): its readings, and whether repeated
//!                ├─ links         each covering link to artifacts: how complete its listing
//!                │                is, and what it is assessed as (Assessed: whole, with
//!                │                each name and part and how it stands; not whole; or
//!                │                unsound, with its problems)
//!                └─ listed        the ListedNames the answer was made from
//!   freshness  assess ──► Assessment, which holds the CoverAssessment and tells the notices
//!              of documents from it; a report and a command read it from there
//! ```
//!
//! The rules, each decided here and nowhere else:
//!
//! - **Gap.** A target a covering link expects, that no document that counts reaches
//!   through that link, and that no exemption that stands exempts it
//!   from ([`ExemptionReading::Needed`]). A document is a target of every link into
//!   its book when it counts (no decision was made against it, as the module `cover`
//!   reads it); a name is a target of a link to artifacts
//!   when the listing of its selection is the whole ([`Completeness::All`]) and
//!   holds it. A listing that is not the whole proves no gap and no absence of one:
//!   it is told as its [`Completeness`], and a gate on it is the report's.
//! - **Part.** A cover that declares `parts = { whole = "<link>" }`
//!   ([`ArtifactCover::parts_whole`]) reads a listing in parts
//!   ([`NameListing::Parted`]) as assessed whole: each name listed is a target as
//!   above, and each part is one target, the artifact the part is, reached only
//!   through `<link>` (by a document that counts and writes the part's name under it,
//!   the one way a name is reached, by the one rule `cover::part_reach`, which the
//!   reading of an exemption of a part reads too). A part that is not reached is a
//!   gap of the covering link, with its strength, whatever was read of it
//!   ([`PartRead`]): one read with no name found in it is no exception, since what
//!   keeps it from being listed whole may make a name, so whether it holds one cannot
//!   be told; a person who holds it to hold none says so with an exemption from the
//!   covering link, with a reason. A part in another space than `<link>`'s cannot be
//!   reached at all: it is a problem of the link
//!   ([`CoverProblem::PartOfAnotherSpace`]), whatever its strength, and nothing of
//!   the link is assessed ([`Assessed::Unsound`]). A cover that declares no `parts`
//!   reads a listing in parts as one that is not the whole ([`Assessed::NotWhole`]).
//! - **Repeated.** Two or more exemptions that stand of one subject from one link
//!   (branches merged leave them), told once, whatever the subject.
//! - **Selects nothing.** A link to artifacts whose selection takes nothing of what
//!   is assessed whole.
//!
//! The listings are an argument: [`Listings::NotAsked`] for a command that reads no
//! cover of artifacts, which then has the completeness [`Completeness::NotAsked`],
//! and never a gap of an artifact. Nothing here reads a listing but through
//! [`ListedNames::of`].
//!
//! [`LinkKind::cover`]: super::schema::LinkKind::cover
//! [`ArtifactLinkKind::cover`]: super::schema::ArtifactLinkKind::cover
//! [`ArtifactCover::parts_whole`]: super::schema::ArtifactCover::parts_whole
//! [`NameListing::Parted`]: super::artifact::NameListing::Parted
//! [`PartRead`]: super::artifact::PartRead

use std::collections::BTreeMap;

use crate::domain::id::DocId;
use crate::domain::nonempty::{NonEmptyVec, TwoOrMore};

use super::artifact::{
    ArtifactKey, ArtifactName, Completeness, ListedNames, ListingPart, Listings, Selection, Space,
};
use super::build::Shelf;
use super::cover::{
    ListingRead, counts, covering_reach, decided_of, part_reach, read_listing, whole_space_of,
};
use super::exemption::{ExemptionReading, by_subject, read_exemption};
use super::ledger::{ExemptOf, JudgementId, Ledger};
use super::schema::{BookName, CoverSeverity, EntryName};

/// What a covering link expects to be reached: a document, a name of an artifact, or
/// a part its listing could not list whole.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum CoverTarget {
    /// A document, by its identity.
    Doc(DocId),
    /// A name in the space of the covering link.
    Artifact(ArtifactName),
    /// A part the listing of the link's selection could not list whole, expected to be
    /// reached whole through the link the cover names for its parts.
    Part(ArtifactKey),
}

/// A target that a covering link expects and nobody reaches, with the strength of the
/// link.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CoverGap {
    target: CoverTarget,
    book: BookName,
    link: EntryName,
    severity: CoverSeverity,
}

impl CoverGap {
    /// What is not reached.
    pub fn target(&self) -> &CoverTarget {
        &self.target
    }

    /// The book that declares the covering link.
    pub fn book(&self) -> &BookName {
        &self.book
    }

    /// The covering link.
    pub fn link(&self) -> &EntryName {
        &self.link
    }

    /// How strongly the link expects the target to be reached.
    pub fn severity(&self) -> CoverSeverity {
        self.severity
    }
}

/// A covering link by the book that declares it and its name.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct CoverLink {
    book: BookName,
    link: EntryName,
}

impl CoverLink {
    /// The book that declares the link.
    pub fn book(&self) -> &BookName {
        &self.book
    }

    /// The link.
    pub fn link(&self) -> &EntryName {
        &self.link
    }
}

/// One exemption that stands, with what it reads as now.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReadExemption {
    id: JudgementId,
    reading: ExemptionReading,
}

impl ReadExemption {
    /// The record.
    pub fn id(&self) -> JudgementId {
        self.id
    }

    /// How it reads now.
    pub fn reading(&self) -> &ExemptionReading {
        &self.reading
    }
}

/// The exemptions that stand of one subject from one covering link, each read, in
/// the order of the ledger; and, when there are two or more, that they are repeated.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExemptionGroup {
    of: ExemptOf,
    reads: Vec<ReadExemption>,
    repeated: Option<TwoOrMore<JudgementId>>,
}

impl ExemptionGroup {
    /// What they are of: the subject and the link.
    pub fn of(&self) -> &ExemptOf {
        &self.of
    }

    /// Each exemption that stands, with what it reads as, in the order of the ledger.
    pub fn reads(&self) -> &[ReadExemption] {
        &self.reads
    }

    /// The exemptions, in the order of identity, when two or more stand: one is
    /// enough, so the others are to be withdrawn.
    pub fn repeated(&self) -> Option<&TwoOrMore<JudgementId>> {
        self.repeated.as_ref()
    }

    /// The first of them that is needed, in the order of the ledger: the one that
    /// makes the subject exempt from the link.
    fn exempting(&self) -> Option<JudgementId> {
        self.reads
            .iter()
            .find(|read| read.reading.is_needed())
            .map(|read| read.id)
    }
}

/// How a name that a selection takes stands with the covering link that expects it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NameCover {
    /// A document that counts writes it under the link.
    Reached,
    /// Nobody reaches it, and an exemption that stands exempts it.
    Exempt(JudgementId),
    /// Nobody reaches it, and nothing exempts it: a gap.
    Gap,
}

/// How a part that a listing could not list whole stands with the cover that takes
/// it whole, through the link the cover names for its parts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PartCover {
    /// A document that counts writes the part's name under that link.
    Reached,
    /// Nobody reaches it, and an exemption that stands exempts it from the covering
    /// link.
    Exempt(JudgementId),
    /// Nobody reaches it, and nothing exempts it: a gap, whatever was read of it. A
    /// part read with no name found in it is no exception: something in it may make
    /// a name the adapter does not see, so whether it holds one cannot be told.
    Gap,
}

/// What a covering link to artifacts is assessed as: everything its selection takes,
/// nothing (its listing is not the whole, and the cover does not take its parts
/// whole), or nothing for a problem of what the adapter answered.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Assessed {
    /// Everything the selection takes is assessed: the listing is the whole, or it is
    /// in parts and the cover takes each part whole through the link it declares for
    /// parts.
    Whole {
        /// Each name listed with how it stands, in the order of the names.
        names: Vec<(ArtifactName, NameCover)>,
        /// Each part with how it stands, in the order the adapter gave them; none
        /// when the listing is the whole.
        parts: Vec<(ListingPart, PartCover)>,
    },
    /// The listing is not the whole, and the cover does not take it whole: no gap is
    /// told, and the completeness of the link says why.
    NotWhole,
    /// What the adapter answered does not meet the declaration: nothing is assessed,
    /// and the link fails a check whatever its strength.
    Unsound(NonEmptyVec<CoverProblem>),
}

/// What keeps a covering link from being assessed as it declares: a problem of what
/// the adapter answered against the declaration, of the error strength whatever the
/// strength of the link (a check is not clean on one).
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum CoverProblem {
    /// A part is in another space than that of the link the cover takes parts whole
    /// through, so no document can reach it there.
    #[error(
        "the part {part} is not in `{space}`, the space of `{whole}`, through which the cover takes each part whole"
    )]
    PartOfAnotherSpace {
        /// The part, as the adapter named it.
        part: ArtifactKey,
        /// The link the cover takes parts whole through.
        whole: EntryName,
        /// The space of that link.
        space: Space,
    },
}

/// A covering link to artifacts, with how complete the listing of the names its
/// selection takes is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LinkCover {
    book: BookName,
    link: EntryName,
    severity: CoverSeverity,
    space: Space,
    selection: Selection,
    completeness: Completeness,
    /// The link the cover takes parts whole through, when it declares one.
    parts_whole: Option<EntryName>,
    assessed: Assessed,
}

impl LinkCover {
    /// The book that declares the link.
    pub fn book(&self) -> &BookName {
        &self.book
    }

    /// The link.
    pub fn link(&self) -> &EntryName {
        &self.link
    }

    /// How strongly the link expects its names to be reached.
    pub fn severity(&self) -> CoverSeverity {
        self.severity
    }

    /// The space of the names.
    pub fn space(&self) -> &Space {
        &self.space
    }

    /// The selection of the names the link expects, as it declares it.
    pub fn selection(&self) -> &Selection {
        &self.selection
    }

    /// How complete the listing of the names the selection takes is, as the adapter
    /// answered it (a listing in parts is partial, whether or not the cover takes
    /// its parts whole: [`assessed`](Self::assessed) says).
    pub fn completeness(&self) -> &Completeness {
        &self.completeness
    }

    /// The link the cover takes each part whole through, when it declares one.
    pub fn parts_whole(&self) -> Option<&EntryName> {
        self.parts_whole.as_ref()
    }

    /// What the link is assessed as: each name and part with how it stands, nothing
    /// (not the whole), or nothing for a problem.
    pub fn assessed(&self) -> &Assessed {
        &self.assessed
    }

    /// Whether everything the selection takes is assessed ([`Assessed::Whole`]):
    /// only then can the link be fulfilled.
    pub fn is_whole(&self) -> bool {
        matches!(self.assessed, Assessed::Whole { .. })
    }

    /// Whether the selection takes nothing of what is assessed whole: a cover that
    /// expects nothing, which is a mistake of the declaration more than a fulfilled
    /// one.
    pub fn selects_nothing(&self) -> bool {
        matches!(&self.assessed, Assessed::Whole { names, parts } if names.is_empty() && parts.is_empty())
    }
}

/// The answer of [`assess`]: what the covering links expect and do not reach, which
/// exemptions stand and what each reads as, which are repeated, and how complete each
/// listing of names is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CoverAssessment {
    /// The covering links into a book that expect each document that counts.
    expected: BTreeMap<DocId, Vec<CoverLink>>,
    gaps: BTreeMap<(CoverTarget, BookName, EntryName), CoverGap>,
    exemptions: BTreeMap<ExemptOf, ExemptionGroup>,
    links: Vec<LinkCover>,
    listed: ListedNames,
}

impl Default for CoverAssessment {
    /// The assessment of a shelf where no link covers: nothing is expected, so
    /// nothing is a gap, and no listing was asked.
    fn default() -> Self {
        Self {
            expected: BTreeMap::new(),
            gaps: BTreeMap::new(),
            exemptions: BTreeMap::new(),
            links: Vec::new(),
            listed: ListedNames::of(&Listings::NotAsked),
        }
    }
}

impl CoverAssessment {
    /// Every gap, in the order of the target (documents, then names), then the link.
    pub fn gaps(&self) -> impl Iterator<Item = &CoverGap> {
        self.gaps.values()
    }

    /// The gap of `target` through the link `link` of the book `book`, when there is
    /// one.
    pub fn gap_of(
        &self,
        target: &CoverTarget,
        book: &BookName,
        link: &EntryName,
    ) -> Option<&CoverGap> {
        self.gaps.get(&(target.clone(), book.clone(), link.clone()))
    }

    /// The covering links into a book that expect the document `id`, in the order the
    /// declaration writes the books and their links; none when the document does not
    /// count or none expects it.
    pub fn expected_of(&self, id: DocId) -> &[CoverLink] {
        self.expected.get(&id).map_or(&[], Vec::as_slice)
    }

    /// Every group of exemptions that stand, in the order of the subject and the link.
    pub fn exemptions(&self) -> impl Iterator<Item = &ExemptionGroup> {
        self.exemptions.values()
    }

    /// The exemptions that stand of `of`, when some do.
    pub fn exemptions_of(&self, of: &ExemptOf) -> Option<&ExemptionGroup> {
        self.exemptions.get(of)
    }

    /// Every covering link to artifacts, in the order the declaration writes the
    /// books and their links.
    pub fn links(&self) -> &[LinkCover] {
        &self.links
    }

    /// The covering links to artifacts whose selection takes no name of a listing
    /// that is the whole.
    pub fn nothing_selected(&self) -> impl Iterator<Item = &LinkCover> {
        self.links.iter().filter(|link| link.selects_nothing())
    }
}

/// The assessment of coverage of a shelf and a ledger, with the listings the command
/// asked of the adapters.
///
/// The one place a gap, a repeated exemption, the completeness of a listing and a
/// selection that takes nothing are decided (the rules are on the module page). It
/// reads what the covering links reach from the documents that count, and each
/// exemption as [`exemption_reading`](super::exemption::exemption_reading) does,
/// once, from the same shelf and ledger.
pub fn assess(shelf: &Shelf, ledger: &Ledger, listings: &Listings) -> CoverAssessment {
    let decided = |id| decided_of(ledger, id);
    let reached = covering_reach(shelf, decided);
    let listed = ListedNames::of(listings);

    // Each exemption that stands, read once, with whether it is repeated.
    let exemptions: BTreeMap<ExemptOf, ExemptionGroup> = by_subject(ledger)
        .into_iter()
        .map(|(of, group)| {
            // The reading is of the subject and the link, the same for each record.
            let reading = read_exemption(shelf, decided, &reached, &listed, of);
            let reads = group
                .iter()
                .map(|exempt| ReadExemption {
                    id: exempt.id(),
                    reading: reading.clone(),
                })
                .collect();
            let repeated = match group.as_slice() {
                [first, second, rest @ ..] => Some(TwoOrMore::new(
                    first.id(),
                    second.id(),
                    rest.iter().map(|e| e.id()).collect(),
                )),
                _ => None,
            };
            let group = ExemptionGroup {
                of: of.clone(),
                reads,
                repeated,
            };
            (of.clone(), group)
        })
        .collect();
    let exempting = |of: ExemptOf| exemptions.get(&of).and_then(ExemptionGroup::exempting);

    let mut gaps = BTreeMap::new();

    // The documents: each that counts is expected by every covering link into its
    // book.
    let mut covering: BTreeMap<&BookName, Vec<(&BookName, &EntryName, CoverSeverity)>> =
        BTreeMap::new();
    for (book, schema) in shelf.schema().books() {
        for (name, kind) in schema.links() {
            if let Some(severity) = kind.cover {
                covering
                    .entry(&kind.target)
                    .or_default()
                    .push((book, name, severity));
            }
        }
    }
    let mut expected: BTreeMap<DocId, Vec<CoverLink>> = BTreeMap::new();
    for doc in shelf.docs().values() {
        if !counts(decided(doc.id())) {
            continue;
        }
        for (book, link, severity) in covering.get(&doc.key().book).into_iter().flatten() {
            expected.entry(doc.id()).or_default().push(CoverLink {
                book: (*book).clone(),
                link: (*link).clone(),
            });
            let missing = reached.doc_from(doc.id(), book, link).is_none()
                && exempting(ExemptOf::new(doc.id(), (*book).clone(), (*link).clone())).is_none();
            if missing {
                let target = CoverTarget::Doc(doc.id());
                gaps.insert(
                    (target.clone(), (*book).clone(), (*link).clone()),
                    CoverGap {
                        target,
                        book: (*book).clone(),
                        link: (*link).clone(),
                        severity: *severity,
                    },
                );
            }
        }
    }

    // The names of artifacts: each that the listing of a selection holds is expected
    // by its link.
    let mut links = Vec::new();
    for (book, schema) in shelf.schema().books() {
        for (link, kind) in schema.artifact_links() {
            let Some(cover) = &kind.cover else {
                continue;
            };
            let (space, selection) = (&kind.space, cover.selection());
            let parts_whole = cover.parts_whole();
            let whole_space = whole_space_of(shelf, book, cover);
            // The listing as this cover reads it: by name, and by part when it takes
            // each part whole through a link (the one rule, `read_listing`).
            let assessed = match read_listing(&listed, space, cover, whole_space) {
                ListingRead::NotWhole(_) => Assessed::NotWhole,
                // A part in another space than the link of parts can be reached by no
                // document: nothing of the link is assessed.
                ListingRead::OtherSpace(other) => {
                    let problems = match (parts_whole, whole_space) {
                        (Some(whole), Some(whole_space)) => other
                            .into_iter()
                            .map(|part| CoverProblem::PartOfAnotherSpace {
                                part: part.key().clone(),
                                whole: whole.clone(),
                                space: whole_space.clone(),
                            })
                            .collect(),
                        _ => Vec::new(),
                    };
                    match NonEmptyVec::from_vec(problems) {
                        Ok(problems) => Assessed::Unsound(problems),
                        Err(_) => Assessed::NotWhole,
                    }
                }
                ListingRead::Whole {
                    names,
                    parts: listed_parts,
                } => {
                    let mut covered = Vec::new();
                    let mut parts = Vec::new();
                    for part in listed_parts {
                        let key = part.key();
                        // No link to reach it through: nobody reaches it.
                        let reached_through =
                            parts_whole.and_then(|whole| part_reach(&reached, book, whole, key));
                        let state = if reached_through.is_some() {
                            PartCover::Reached
                        } else {
                            match exempting(ExemptOf::of_artifact(
                                key.clone(),
                                book.clone(),
                                link.clone(),
                            )) {
                                Some(id) => PartCover::Exempt(id),
                                None => PartCover::Gap,
                            }
                        };
                        if state == PartCover::Gap {
                            let target = CoverTarget::Part(key.clone());
                            gaps.insert(
                                (target.clone(), book.clone(), link.clone()),
                                CoverGap {
                                    target,
                                    book: book.clone(),
                                    link: link.clone(),
                                    severity: cover.severity(),
                                },
                            );
                        }
                        parts.push((part.clone(), state));
                    }
                    for name in names {
                        let key = ArtifactKey::new(space.clone(), name.clone());
                        let state = if reached.artifact_from(book, link, name).is_some() {
                            NameCover::Reached
                        } else {
                            match exempting(ExemptOf::of_artifact(key, book.clone(), link.clone()))
                            {
                                Some(id) => NameCover::Exempt(id),
                                None => NameCover::Gap,
                            }
                        };
                        if state == NameCover::Gap {
                            let target = CoverTarget::Artifact(name.clone());
                            gaps.insert(
                                (target.clone(), book.clone(), link.clone()),
                                CoverGap {
                                    target,
                                    book: book.clone(),
                                    link: link.clone(),
                                    severity: cover.severity(),
                                },
                            );
                        }
                        covered.push((name.clone(), state));
                    }
                    Assessed::Whole {
                        names: covered,
                        parts,
                    }
                }
            };
            links.push(LinkCover {
                book: book.clone(),
                link: link.clone(),
                severity: cover.severity(),
                space: space.clone(),
                selection: selection.clone(),
                completeness: listed.completeness(space, selection),
                parts_whole: parts_whole.cloned(),
                assessed,
            });
        }
    }

    CoverAssessment {
        expected,
        gaps,
        exemptions,
        links,
        listed,
    }
}
