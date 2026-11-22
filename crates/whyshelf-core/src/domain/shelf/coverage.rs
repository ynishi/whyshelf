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
//!                │                is, and whether its selection takes nothing
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
//! - **Repeated.** Two or more exemptions that stand of one subject from one link
//!   (branches merged leave them), told once, whatever the subject.
//! - **Selects nothing.** A link to artifacts whose selection takes no name of a
//!   listing that is the whole.
//!
//! The listings are an argument: [`Listings::NotAsked`] for a command that reads no
//! cover of artifacts, which then has the completeness [`Completeness::NotAsked`],
//! and never a gap of an artifact. Nothing here reads a listing but through
//! [`ListedNames::of`].
//!
//! [`LinkKind::cover`]: super::schema::LinkKind::cover
//! [`ArtifactLinkKind::cover`]: super::schema::ArtifactLinkKind::cover

use std::collections::BTreeMap;

use crate::domain::id::DocId;
use crate::domain::nonempty::TwoOrMore;

use super::artifact::{
    ArtifactKey, ArtifactName, Completeness, ListedNames, Listings, Selection, Space,
};
use super::build::Shelf;
use super::cover::{counts, covering_reach, decided_of};
use super::exemption::{ExemptionReading, by_subject, read_exemption};
use super::ledger::{ExemptOf, JudgementId, Ledger};
use super::schema::{BookName, CoverSeverity, EntryName};

/// What a covering link expects to be reached: a document, or a name of an artifact.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum CoverTarget {
    /// A document, by its identity.
    Doc(DocId),
    /// A name in the space of the covering link.
    Artifact(ArtifactName),
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
    selects_nothing: bool,
    /// Each name the selection takes, when its listing is the whole, in the order of
    /// the names; none otherwise.
    names: Vec<(ArtifactName, NameCover)>,
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

    /// How complete the listing of the names the selection takes is.
    pub fn completeness(&self) -> &Completeness {
        &self.completeness
    }

    /// Whether the selection takes no name of a listing that is the whole: a cover
    /// that expects nothing, which is a mistake of the declaration more than a
    /// fulfilled one.
    pub fn selects_nothing(&self) -> bool {
        self.selects_nothing
    }

    /// Each name the selection takes with how it stands (reached, exempt or a gap),
    /// in the order of the names. Empty unless the listing is the whole
    /// ([`completeness`](Self::completeness) says whether it is).
    pub fn names(&self) -> &[(ArtifactName, NameCover)] {
        &self.names
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
        self.links.iter().filter(|link| link.selects_nothing)
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
            let mut covered = Vec::new();
            let selects_nothing = match listed.names(space, selection) {
                Ok(names) => {
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
                    names.is_empty()
                }
                Err(_) => false,
            };
            links.push(LinkCover {
                book: book.clone(),
                link: link.clone(),
                severity: cover.severity(),
                space: space.clone(),
                selection: selection.clone(),
                completeness: listed.completeness(space, selection),
                selects_nothing,
                names: covered,
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
