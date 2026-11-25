//! The use cases that only read: `check`, `map`, `status`, `get`, `trace`, `reach`,
//! `impact`, `stale`, `units`, `stats`.
//!
//! Each builds the schema, the shelf and the ledger ([`load`](super::load)) and asks
//! the entries of the domain; none writes, and the type they are generic over
//! ([`ShelfRead`]) has nothing to write with. What each asks, and of whom:
//!
//! | use case | asks |
//! |---|---|
//! | `books_check` | [`Schema::declared`] (the declaration expanded, then checked), then [`Shelf::tally_by_book`] and [`assess`] for the faults around the documents and the approvals that await other names, and [`Ledger::same_meaning_short`] |
//! | `map` | [`Schema::declared`], then [`ShelfMap::of`]; nothing else is read |
//! | `status` | [`assess`]: every identity's [`Standing`], with [`Ledger::status_of`] beside it; with a filter ([`status_filtered`](ShelfApp::status_filtered)), [`StatusFilter::new`] against the declaration and [`StatusFilter::admits`] per identity |
//! | `get` | [`resolve`] for the reference, then the same as `status` for the one document, and [`Assessment::roots_of`] |
//! | `book_links` | [`Shelf::book_of`] of each valid document, and [`Doc::links_named`] of those of the book (the artifacts alone), and [`Shelf::invalid_files_in`] |
//! | `trace` | [`resolve`], then [`trace`](domain_trace), and the same as `status` for every document it reaches |
//! | `reach` | [`resolve`] for each reference, then the reach of the documents named ([`reach`](crate::domain::shelf::reach): the walk down, the artifacts each document reached stands on), where each artifact lives ([`ArtifactSource::places`]), the files of all of it, and the same as `status` for every document reached |
//! | `impact` | both ends of the change, read at one time, and [`assess`] of each, then [`impact`](crate::domain::shelf::impact::impact); with references ([`impact_within`](ShelfApp::impact_within)), each resolved at the end before (at the end after when it names nothing before), the walk down from them on the end before ([`reach`](crate::domain::shelf::reach)), `impact` of the change read backwards, and the gate ([`within`](crate::domain::shelf::within)) |
//! | `stale` | [`assess`]: the approvals that are [`Freshness::Degraded`], with [`Assessment::roots_of`], what can settle each origin ([`origins_asked`] of the origins, what the approving commands would approve foreseen once, [`Foreseen::of`], then [`settle_all`]), and the names approvals stand on that cannot be told now |
//! | `units` | [`assess`], then [`units::plan`](crate::domain::shelf::units::plan) (the approvals that are [`Freshness::Degraded`], and, asked, the pending documents, with what each stands on and what holds it back), where the artifacts they stand on live ([`ArtifactSource::places`]), then [`units::divide`](crate::domain::shelf::units::divide) |
//! | `stats` | [`stats::of`](crate::domain::shelf::stats::of), from the ledger and the keys of the shelf; [`assess`] only for the health |
//!
//! [`Schema::declared`]: crate::domain::shelf::schema::Schema::declared
//! [`ShelfMap::of`]: crate::domain::shelf::map::ShelfMap::of
//! [`Shelf::tally_by_book`]: crate::domain::shelf::build::Shelf::tally_by_book
//! [`Shelf::book_of`]: crate::domain::shelf::build::Shelf::book_of
//! [`Shelf::invalid_files_in`]: crate::domain::shelf::build::Shelf::invalid_files_in
//! [`Doc::links_named`]: crate::domain::shelf::doc::Doc::links_named
//! [`assess`]: crate::domain::shelf::freshness::assess
//! [`Ledger::status_of`]: crate::domain::shelf::ledger::Ledger::status_of
//! [`Ledger::same_meaning_short`]: crate::domain::shelf::ledger::Ledger::same_meaning_short
//! [`Standing`]: crate::domain::shelf::freshness::Standing
//! [`StatusFilter::new`]: crate::domain::shelf::filter::StatusFilter::new
//! [`StatusFilter::admits`]: crate::domain::shelf::filter::StatusFilter::admits
//! [`Assessment::roots_of`]: crate::domain::shelf::freshness::Assessment::roots_of
//! [`Freshness::Degraded`]: crate::domain::shelf::freshness::Freshness::Degraded
//! [`origins_asked`]: crate::domain::shelf::settle::origins_asked
//! [`Foreseen::of`]: crate::domain::shelf::order::Foreseen::of
//! [`settle_all`]: crate::domain::shelf::settle::settle_all
//! [`resolve`]: crate::domain::shelf::lookup::resolve
//! [domain_trace]: crate::domain::shelf::trace::trace
//! [`ArtifactSource::places`]: super::ports::ArtifactSource::places

use std::collections::{BTreeMap, BTreeSet};

use crate::domain::id::DocId;
use crate::domain::location::RelPath;
use crate::domain::nonempty::{NonEmptySet, NonEmptyVec};
use crate::domain::shelf::artifact::{ArtifactKey, Listings};
use crate::domain::shelf::doc::LinkNamed;
use crate::domain::shelf::filter::{StatusFilter, StatusFilterAsk};
use crate::domain::shelf::freshness::{Cause, Holding, Notice};
use crate::domain::shelf::impact;
use crate::domain::shelf::ledger::{JudgedAt, MovedTo, Quorum};
use crate::domain::shelf::lookup::{Reference, Resolution, resolve};
use crate::domain::shelf::map::ShelfMap;
use crate::domain::shelf::order::Foreseen;
use crate::domain::shelf::reach::{Reach, walk};
use crate::domain::shelf::schema::BookName;
use crate::domain::shelf::settle::{origins_asked, settle_all};
use crate::domain::shelf::stats;
use crate::domain::shelf::trace::{LinkEnd, trace};
use crate::domain::shelf::units::{self, UnitsOf};
use crate::domain::shelf::within::End;

use super::ShelfApp;
use super::cover::artifact_lists;
use super::error::ShelfError;
use super::load::{Loaded, View, listings_of, load, load_at, load_with, now, schema_of};
use super::ports::{ArtifactSource, ShelfPort, ShelfRead, port};
use super::report::{
    Absent, Awaiting, BookCount, BookLinks, BookLinksReport, BooksCheck, DownstreamEdge,
    ExemptEntry, Gate, GetReport, ImpactReport, MapReport, MovedAway, ReachReport, ReachedArtifact,
    ReachedDoc, RepeatedExemption, SettledOrigin, StaleEntry, StaleReport, StatsReport,
    StatusReport, TraceReport, Uncovered, UnitsReport, Unknown, UnneededExemption, UpstreamEdge,
    UpstreamEnd,
};
use super::state::Side;

/// The identity a reference names, or why it names none.
// The error is read once and ends the use case; it is not a hot path.
#[allow(clippy::result_large_err)]
pub(super) fn identity(loaded: &Loaded, reference: &Reference) -> Result<DocId, ShelfError> {
    match resolve(&loaded.shelf, &loaded.ledger, reference) {
        Resolution::Found(id) | Resolution::LedgerOnly { id, .. } => Ok(id),
        Resolution::Missing => Err(ShelfError::NotFound {
            reference: reference.clone(),
        }),
        Resolution::Ambiguous(candidates) => Err(ShelfError::Ambiguous {
            reference: reference.clone(),
            candidates,
        }),
        Resolution::NoIdentity(files) => Err(ShelfError::NoIdentity {
            reference: reference.clone(),
            files,
        }),
        Resolution::UnknownBook(book) => Err(ShelfError::UnknownBook {
            reference: Some(reference.clone()),
            book,
        }),
        Resolution::NotAKeyOf { book, problem } => Err(ShelfError::NotAKeyOf {
            reference: reference.clone(),
            book,
            problem,
        }),
        Resolution::NotAKeyOfAnyBook => Err(ShelfError::NotAKeyOfAnyBook {
            reference: reference.clone(),
        }),
    }
}

/// The documents references name across the two ends of a change: each at the end
/// before, or, only when it names nothing there, at the end after, whose answer (a
/// document or an error) is then the answer. A reference names nothing before when
/// no document has it there, and when the declaration before cannot read it as one:
/// a book the change adds, or a key only the declaration after takes.
// The error is read once and ends the use case; it is not a hot path.
#[allow(clippy::result_large_err)]
fn named_across(
    before: &Loaded,
    after: &Loaded,
    references: &NonEmptyVec<Reference>,
) -> Result<NonEmptySet<DocId>, ShelfError> {
    let one = |reference: &Reference| match identity(before, reference) {
        Err(
            ShelfError::NotFound { .. }
            | ShelfError::UnknownBook { .. }
            | ShelfError::NotAKeyOf { .. }
            | ShelfError::NotAKeyOfAnyBook { .. },
        ) => identity(after, reference),
        answered => answered,
    };
    let mut named = NonEmptySet::new(one(references.first())?, []);
    for reference in references.iter().skip(1) {
        named.insert(one(reference)?);
    }
    Ok(named)
}

/// The files of the ledger of one end that are no record that could be read.
fn unread_records(loaded: &Loaded) -> Vec<RelPath> {
    loaded.unreadable.iter().map(|u| u.file.clone()).collect()
}

/// One end of a change as the gate reads it.
fn end_of<'a>(loaded: &'a Loaded, unreadable_records: &'a [RelPath]) -> End<'a> {
    End {
        shelf: &loaded.shelf,
        ledger: &loaded.ledger,
        records: &loaded.records,
        unreadable_records,
    }
}

impl<P: ShelfRead> ShelfApp<P> {
    /// Check the declaration of the books, the documents and the ledger, and say
    /// everything that is wrong: the problems of the declaration, the files that are
    /// not valid documents, the cycles, the records that contradict one another or
    /// could not be read.
    ///
    /// A declaration that is not sound is an answer, not an error, and then nothing
    /// else is read: there is no way to tell which files are books without it.
    pub async fn books_check(&self) -> Result<BooksCheck, ShelfError> {
        let schema = match schema_of(&self.ports, Side::After).await? {
            Ok(schema) => schema,
            Err(problems) => return Ok(BooksCheck::SchemaInvalid(problems)),
        };
        let loaded = load_with(&self.ports, Side::After, &schema, now(&self.ports).await?).await?;
        // `check` is the gate on a cover of artifacts, so it always asks the adapters
        // for the names the selections take.
        let listings = listings_of(
            self.ports.artifacts(),
            Side::After,
            schema.cover_selections(),
        )
        .await?;
        let assessment = loaded.assess(&listings);
        let view = View {
            loaded: &loaded,
            assessment: &assessment,
        };
        let tally = loaded.shelf.tally_by_book();
        let books = schema
            .books()
            .values()
            .map(|book| BookCount {
                name: book.name().clone(),
                dir: book.dir().clone(),
                tally: tally.get(book.name()).copied().unwrap_or_default(),
                skipped: loaded
                    .shelf
                    .skipped()
                    .iter()
                    .filter(|file| file.book() == book.name())
                    .cloned()
                    .collect(),
            })
            .collect();
        let uncovered = assessment
            .standings()
            .keys()
            .flat_map(|id| {
                let key = loaded.shelf.doc(*id).map(|doc| doc.key().clone());
                assessment
                    .notices_of(*id)
                    .iter()
                    .filter_map(move |notice| match notice {
                        Notice::NotCovered {
                            book,
                            link,
                            severity,
                        } => Some(Uncovered {
                            doc: *id,
                            key: key.clone(),
                            book: book.clone(),
                            link: link.clone(),
                            severity: *severity,
                        }),
                        _ => None,
                    })
            })
            .collect();
        let absent = assessment
            .standings()
            .keys()
            .flat_map(|id| {
                let key = loaded.shelf.doc(*id).map(|doc| doc.key().clone());
                assessment
                    .notices_of(*id)
                    .iter()
                    .filter_map(move |notice| match notice {
                        Notice::ArtifactAbsent { link, artifact } => Some(Absent {
                            doc: *id,
                            key: key.clone(),
                            link: link.clone(),
                            artifact: artifact.clone(),
                        }),
                        _ => None,
                    })
            })
            .collect();
        let unknown = assessment
            .standings()
            .keys()
            .flat_map(|id| {
                let key = loaded.shelf.doc(*id).map(|doc| doc.key().clone());
                assessment
                    .notices_of(*id)
                    .iter()
                    .filter_map(move |notice| match notice {
                        Notice::ArtifactUnknown {
                            link,
                            artifact,
                            why,
                        } => Some(Unknown {
                            doc: *id,
                            key: key.clone(),
                            link: link.clone(),
                            artifact: artifact.clone(),
                            why: why.clone(),
                        }),
                        _ => None,
                    })
            })
            .collect();
        // The links behind a move: the `ArtifactMoved` causes of every approval.
        let moved = assessment
            .degraded()
            .flat_map(|(id, degradation)| {
                let key = loaded.shelf.doc(id).map(|doc| doc.key().clone());
                degradation
                    .causes()
                    .iter()
                    .filter_map(move |cause| match cause {
                        Cause::ArtifactMoved { link, written, pin } => Some(MovedAway {
                            doc: id,
                            key: key.clone(),
                            link: link.clone(),
                            artifact: written.clone(),
                            moved_to: pin.read_as().clone(),
                            declared: pin.moved().map(MovedTo::declared)?,
                        }),
                        _ => None,
                    })
            })
            .collect();
        // The exemptions, as freshness read them: those that take the place of a gap,
        // and those no longer needed. Each names its record, which the ledger keeps.
        let mut exempt = Vec::new();
        let mut unneeded = Vec::new();
        let mut repeated = Vec::new();
        for (id, notices) in assessment.noticed() {
            let key = loaded.shelf.doc(id).map(|doc| doc.key().clone());
            for notice in notices {
                match notice {
                    Notice::Exempt {
                        severity,
                        exemption,
                        ..
                    } => {
                        if let Some(record) = loaded.ledger.exemption(*exemption) {
                            exempt.push(ExemptEntry {
                                doc: id,
                                key: key.clone(),
                                severity: *severity,
                                record: record.clone(),
                            });
                        }
                    }
                    Notice::ExemptionRepeated {
                        book,
                        link,
                        exemptions,
                    } => repeated.push(RepeatedExemption {
                        doc: id,
                        key: key.clone(),
                        book: book.clone(),
                        link: link.clone(),
                        exemptions: exemptions.clone(),
                    }),
                    Notice::ExemptionUnneeded { exemption, why, .. } => {
                        if let Some(record) = loaded.ledger.exemption(*exemption) {
                            unneeded.push(UnneededExemption {
                                doc: id,
                                key: key.clone(),
                                record: record.clone(),
                                why: why.clone(),
                            });
                        }
                    }
                    _ => {}
                }
            }
        }
        // The approvals that await other names, at any freshness, as the ledger
        // counted them for the assessment.
        let awaiting = assessment
            .standings()
            .iter()
            .filter_map(|(id, standing)| match standing.quorum()? {
                Quorum::Met => None,
                Quorum::Short { readers, need } => Some(Awaiting {
                    doc: *id,
                    key: loaded.shelf.doc(*id).map(|doc| doc.key().clone()),
                    standing: standing.kind(),
                    readers: readers.clone(),
                    need: *need,
                }),
            })
            .collect();
        Ok(BooksCheck::Checked {
            books,
            health: view.health(),
            uncovered,
            absent,
            unknown,
            moved,
            exempt,
            unneeded,
            repeated,
            awaiting,
            same_meaning_short: loaded.ledger.same_meaning_short(&loaded.shelf),
            collisions: loaded.shelf.collisions(),
            artifacts: artifact_lists(&loaded.ledger, assessment.cover(), None),
            cover: assessment.cover().clone(),
            names: loaded.names(),
        })
    }

    /// What the declaration of the books says: every book and every link, in its
    /// order ([`ShelfMap::of`]).
    ///
    /// It reads the declaration and nothing else, no document and no ledger. A
    /// declaration that is not sound is an answer, as for
    /// [`books_check`](Self::books_check).
    pub async fn map(&self) -> Result<MapReport, ShelfError> {
        Ok(match schema_of(&self.ports, Side::After).await? {
            Ok(schema) => MapReport::Mapped(ShelfMap::of(&schema)),
            Err(problems) => MapReport::SchemaInvalid(problems),
        })
    }

    /// Every document with what was decided about it and whether the decision still
    /// holds: [`status_filtered`](Self::status_filtered) with nothing asked.
    pub async fn status(&self) -> Result<StatusReport, ShelfError> {
        self.status_filtered(&StatusFilterAsk::default()).await
    }

    /// The documents that pass a filter by book, standing and field, each with what
    /// was decided about it and whether the decision still holds.
    ///
    /// Whether the filter fits the declaration, and whether a document passes it, are
    /// [`StatusFilter`]'s (its page says the rule); this reads the declaration, asks
    /// the first, loads, and keeps the rows the second admits. A declaration that is
    /// not sound is [`ShelfError::Schema`], as for [`status`](Self::status), and a
    /// filter that does not fit it is [`ShelfError::Filter`] with every problem; the
    /// documents are not read then. The answer carries the filter as given, and its
    /// `health` and `names` are of the whole project, not of the rows.
    pub async fn status_filtered(&self, ask: &StatusFilterAsk) -> Result<StatusReport, ShelfError> {
        let schema = schema_of(&self.ports, Side::After)
            .await?
            .map_err(ShelfError::Schema)?;
        let filter = StatusFilter::new(&schema, ask.clone()).map_err(ShelfError::Filter)?;
        let loaded = load_with(&self.ports, Side::After, &schema, now(&self.ports).await?).await?;
        let assessment = loaded.assess(&Listings::NotAsked);
        let view = View {
            loaded: &loaded,
            assessment: &assessment,
        };
        let rows = assessment
            .standings()
            .iter()
            .filter(|(id, standing)| filter.admits(&loaded.shelf, **id, standing))
            .map(|(id, _)| view.row(*id))
            .collect();
        Ok(StatusReport {
            rows,
            filters: filter.ask().clone(),
            names: loaded.names(),
            health: view.health(),
        })
    }

    /// One document: what the reference names, with its standing, its notices and
    /// where the degradation of its approval starts.
    pub async fn get(&self, reference: &Reference) -> Result<GetReport, ShelfError> {
        let loaded = load(&self.ports).await?;
        let id = identity(&loaded, reference)?;
        let assessment = loaded.assess(&Listings::NotAsked);
        let view = View {
            loaded: &loaded,
            assessment: &assessment,
        };
        Ok(GetReport {
            row: view.row(id),
            doc: loaded.shelf.doc(id).cloned(),
            roots: assessment.roots_of(id),
            names: loaded.names(),
            health: view.health(),
        })
    }

    /// What each valid document of the book `book` names in its links to artifacts,
    /// read once at the side after as a check reads them: each link in which it names
    /// an artifact, with the artifacts it names in the order written, by their space
    /// and name ([`Doc::links_named`], the one place a document's links are
    /// read; a blank item, a null, an empty list or a text is told there, and a file
    /// that writes one that is no name is no valid document). Beside them, every file
    /// of the book that is no valid document ([`Shelf::invalid_files_in`], a file with
    /// no `id:` among them): what it names cannot be told. Nothing is decided here.
    ///
    /// A book the declaration does not declare is [`ShelfError::UnknownBook`], never
    /// an empty answer; a declaration that is not sound is [`ShelfError::Schema`].
    ///
    /// [`Doc::links_named`]: crate::domain::shelf::doc::Doc::links_named
    /// [`Shelf::invalid_files_in`]: crate::domain::shelf::build::Shelf::invalid_files_in
    pub async fn book_links(&self, book: &BookName) -> Result<BookLinksReport, ShelfError> {
        let loaded = load(&self.ports).await?;
        let shelf = &loaded.shelf;
        if shelf.schema().book(book).is_none() {
            return Err(ShelfError::UnknownBook {
                reference: None,
                book: book.clone(),
            });
        }
        let docs = shelf
            .docs()
            .iter()
            .filter(|(id, _)| shelf.book_of(**id) == Some(book))
            .map(|(_, doc)| BookLinks {
                key: doc.key().clone(),
                file: doc.origin().clone(),
                links: doc
                    .links_named()
                    .into_iter()
                    .filter_map(|(name, named)| {
                        let artifacts: Vec<ArtifactKey> = named
                            .into_iter()
                            .filter_map(|named| match named {
                                LinkNamed::Artifact(key) => Some(key.clone()),
                                LinkNamed::Doc(_) | LinkNamed::Written(_) => None,
                            })
                            .collect();
                        (!artifacts.is_empty()).then(|| (name.clone(), artifacts))
                    })
                    .collect(),
            })
            .collect();
        let not_valid = shelf
            .invalid_files_in(book)
            .iter()
            .map(|file| file.origin().clone())
            .collect();
        Ok(BookLinksReport { docs, not_valid })
    }

    /// What a document is linked to, and what is linked to it, each node with where
    /// it stands. The walk is the domain's ([`trace`]); this shows each document it
    /// reaches as a row.
    pub async fn trace(&self, reference: &Reference) -> Result<TraceReport, ShelfError> {
        let loaded = load(&self.ports).await?;
        let id = identity(&loaded, reference)?;
        let assessment = loaded.assess(&Listings::NotAsked);
        let view = View {
            loaded: &loaded,
            assessment: &assessment,
        };
        let walked = trace(&loaded.shelf, id);
        let upstream = walked
            .upstream
            .into_iter()
            .map(|edge| UpstreamEdge {
                from: edge.holder,
                link: edge.link,
                propagation: edge.propagation,
                depth: edge.depth,
                to: match edge.end {
                    LinkEnd::Doc(target) => UpstreamEnd::Doc(Box::new(view.row(target))),
                    LinkEnd::Unresolved(unresolved) => UpstreamEnd::Unresolved(unresolved),
                    LinkEnd::Artifact(artifact) => UpstreamEnd::Artifact(artifact),
                },
            })
            .collect();
        let downstream = walked
            .downstream
            .into_iter()
            .filter_map(|edge| match edge.end {
                LinkEnd::Doc(to) => Some(DownstreamEdge {
                    to,
                    link: edge.link,
                    propagation: edge.propagation,
                    depth: edge.depth,
                    holder: Box::new(view.row(edge.holder)),
                }),
                // A walk down only follows resolved links to documents.
                LinkEnd::Unresolved(_) | LinkEnd::Artifact(_) => None,
            })
            .collect();
        Ok(TraceReport {
            root: view.row(id),
            upstream,
            downstream,
            names: loaded.names(),
            health: view.health(),
        })
    }

    /// What depends on the documents the references name, and what all of them stand
    /// on: every document reached by a walk down their `Depends` links, the artifacts
    /// each stands on (its links to artifacts now and the pins of its approval) with
    /// where each lives, the files of the project all of it is in, and what may depend
    /// on them that the walk could not read. The rules are the domain's
    /// ([`reach`](crate::domain::shelf::reach)); where an artifact lives is its
    /// adapter's ([`ArtifactSource::places`]). It reads the side after, as every
    /// command but one that compares two ends of a change does.
    ///
    /// A reference that names no document, or several, is the error it is for `get`;
    /// a document only the ledger knows is reached, and gone.
    pub async fn reach(
        &self,
        references: &NonEmptyVec<Reference>,
    ) -> Result<ReachReport, ShelfError> {
        let loaded = load(&self.ports).await?;
        let first = identity(&loaded, references.first())?;
        let mut named = NonEmptySet::new(first, []);
        for reference in references.iter().skip(1) {
            named.insert(identity(&loaded, reference)?);
        }
        let assessment = loaded.assess(&Listings::NotAsked);
        let view = View {
            loaded: &loaded,
            assessment: &assessment,
        };
        let reach = Reach::of(&named, &loaded.shelf, &loaded.ledger);
        let asked: BTreeSet<ArtifactKey> = reach.artifacts().keys().cloned().collect();
        let placements = match asked.is_empty() {
            true => BTreeMap::new(),
            false => self
                .ports
                .artifacts()
                .places(Side::After, &asked)
                .await
                .map_err(port(ShelfPort::Artifacts))?,
        };
        let found = reach.files(&loaded.shelf, &placements);
        let docs = found
            .docs
            .into_iter()
            .map(|(id, files)| ReachedDoc {
                row: view.row(id),
                named: named.contains(&id),
                files,
            })
            .collect();
        let artifacts = found
            .artifacts
            .into_iter()
            .map(|(artifact, placed)| ReachedArtifact {
                artifact,
                by: placed.by,
                placement: placed.placement,
            })
            .collect();
        Ok(ReachReport {
            docs,
            artifacts,
            files: found.files,
            gone: found.gone.into_iter().collect(),
            untold: reach.untold().copied(),
            names: loaded.names(),
            health: view.health(),
        })
    }

    /// Whether a change makes approvals not hold: the two ends of it, read the one way
    /// the run names ([`ShelfRead::way`]), each judged at the one time read once from
    /// the clock, so an expiry changes with the records alone; and for every document
    /// either lists, its holding at each end and what the change made of it. The
    /// rules are the domain's ([`impact`]).
    pub async fn impact(&self) -> Result<ImpactReport, ShelfError> {
        self.impact_of(None).await
    }

    /// [`impact`](Self::impact), and whether the change stays inside the reach of the
    /// documents the references name, as the shelf stood before it: for every
    /// document, whether the change reaches it and where it stands against that
    /// reach. The rules are the domain's ([`within`](crate::domain::shelf::within)).
    ///
    /// A reference is resolved at the end before the change; only when it names
    /// nothing there (no document has it, or the declaration before cannot read it: a
    /// book the change adds, a key only the declaration after takes) is it resolved
    /// at the end after (so a document the change adds can be named), and that end's
    /// answer, a document or an error, is the answer. An error is the one it is for
    /// `get`.
    pub async fn impact_within(
        &self,
        references: &NonEmptyVec<Reference>,
    ) -> Result<ImpactReport, ShelfError> {
        self.impact_of(Some(references)).await
    }

    /// The one reading of both ends of a change, and the gate on it when `within`
    /// names documents.
    async fn impact_of(
        &self,
        within: Option<&NonEmptyVec<Reference>>,
    ) -> Result<ImpactReport, ShelfError> {
        let now = now(&self.ports).await?;
        let before = load_at(&self.ports, Side::Before, now).await?;
        let after = load_at(&self.ports, Side::After, now).await?;
        let named = match within {
            Some(references) => Some(named_across(&before, &after, references)?),
            None => None,
        };
        let (was, is) = (
            before.assess(&Listings::NotAsked),
            after.assess(&Listings::NotAsked),
        );
        let ends = ((&before.shelf, &was), (&after.shelf, &is));
        let (entries, within) = match named {
            None => (impact::impact(ends.0, ends.1), None),
            // Both directions from one set of identities, each row paired with its
            // other; the rows of the answer are the forward ones, as `impact` builds them.
            Some(named) => {
                let rows = impact::both_ways(ends.0, ends.1);
                let walked = walk(&named, &before.shelf);
                let (unread_before, unread_after) =
                    (unread_records(&before), unread_records(&after));
                let gate = crate::domain::shelf::within::within(
                    named,
                    walked,
                    end_of(&before, &unread_before),
                    end_of(&after, &unread_after),
                    &rows,
                );
                let entries = rows.into_iter().map(|(forward, _)| forward).collect();
                (entries, Some(gate))
            }
        };
        let health = |loaded: &Loaded, assessment| View { loaded, assessment }.health();
        Ok(ImpactReport {
            way: self.ports.way(),
            entries,
            names_before: before.names(),
            names_after: after.names(),
            health_before: health(&before, &was),
            health_after: health(&after, &is),
            within,
        })
    }

    /// The approvals that no longer hold, the strongest degradation first, each with
    /// where it starts to fail.
    pub async fn stale(&self) -> Result<StaleReport, ShelfError> {
        let loaded = load(&self.ports).await?;
        let assessment = loaded.assess(&Listings::NotAsked);
        let view = View {
            loaded: &loaded,
            assessment: &assessment,
        };
        // Each document is asked once whether its approval holds, and both lists are
        // made from that answer alone.
        let mut entries: Vec<StaleEntry> = Vec::new();
        let mut unknown: Vec<Unknown> = Vec::new();
        let mut gates = Vec::new();
        for id in assessment.standings().keys() {
            let holding = assessment.holding(&loaded.shelf, *id);
            gates.push(Gate::from(&holding));
            let untold = match holding {
                Holding::DoesNotHold {
                    level,
                    roots,
                    untold,
                } => {
                    entries.push(StaleEntry {
                        row: view.row(*id),
                        level,
                        roots,
                    });
                    untold
                }
                Holding::CannotTell { untold, .. } => untold.into_vec(),
                Holding::NotThere | Holding::Pending | Holding::Against(_) | Holding::Holds(_) => {
                    continue;
                }
            };
            let key = loaded.shelf.doc(*id).map(|doc| doc.key().clone());
            unknown.extend(untold.into_iter().map(|pin| Unknown {
                doc: *id,
                key: key.clone(),
                link: pin.link,
                artifact: pin.artifact,
                why: pin.why,
            }));
        }
        entries.sort_by(|a, b| b.level.cmp(&a.level).then(a.row.id.cmp(&b.row.id)));
        // What the approving commands would approve, foreseen once for every origin,
        // and each origin settled with it.
        let foreseen = Foreseen::of(
            &loaded.shelf,
            &loaded.ledger,
            &assessment,
            origins_asked(assessment.origins()),
        );
        let origins = settle_all(foreseen)
            .into_iter()
            .map(|(group, next)| SettledOrigin { group, next })
            .collect();
        Ok(StaleReport {
            entries,
            origins,
            unknown,
            held: Gate::fold(gates),
            names: loaded.names(),
            health: view.health(),
        })
    }

    /// What does not hold, divided into units of work that share nothing: the
    /// approvals that no longer hold, and with [`UnitsOf::DegradedAndPending`] the
    /// pending documents too. The rules are the domain's
    /// ([`units`]): it reads what to divide once
    /// ([`plan`](units::plan)), asks the adapters where the artifacts those stand on
    /// live ([`ArtifactSource::places`], once, at the side after, and not at all when
    /// none is stood on), and divides by the answers ([`divide`](units::divide)). An
    /// adapter that cannot say where a name lives answers so for that name; an error
    /// of the ask as a whole is [`ShelfError::Port`].
    pub async fn units(&self, of: UnitsOf) -> Result<UnitsReport, ShelfError> {
        let loaded = load(&self.ports).await?;
        let assessment = loaded.assess(&Listings::NotAsked);
        let view = View {
            loaded: &loaded,
            assessment: &assessment,
        };
        let plan = units::plan(&loaded.shelf, &loaded.ledger, &assessment, of);
        let asked = plan.asks();
        let placements = match asked.is_empty() {
            true => BTreeMap::new(),
            false => self
                .ports
                .artifacts()
                .places(Side::After, &asked)
                .await
                .map_err(port(ShelfPort::Artifacts))?,
        };
        Ok(UnitsReport {
            units: units::divide(plan, &placements),
            names: loaded.names(),
            health: view.health(),
        })
    }

    /// How the work went, from the ledger and the documents: the measures of
    /// [`stats::of`], of the records at or after
    /// `since` when it is given.
    ///
    /// No measure reads a clock: each time is one a record or an identity carries.
    /// The project is read as for every answer, and `health` beside the measures is
    /// read as every answer's is (the load reads the clock for the assessment it
    /// comes from, which changes no measure).
    pub async fn stats(&self, since: Option<JudgedAt>) -> Result<StatsReport, ShelfError> {
        let loaded = load(&self.ports).await?;
        let assessment = loaded.assess(&Listings::NotAsked);
        let view = View {
            loaded: &loaded,
            assessment: &assessment,
        };
        Ok(StatsReport {
            stats: stats::of(&loaded.ledger, &loaded.shelf, since),
            names: loaded.names(),
            health: view.health(),
        })
    }
}
