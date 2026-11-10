//! The use cases that only read: `check`, `map`, `status`, `get`, `trace`, `stale`.
//!
//! Each builds the schema, the shelf and the ledger ([`load`](super::load)) and asks
//! the entries of the domain; none writes, and the type they are generic over
//! ([`ShelfRead`]) has nothing to write with. What each asks, and of whom:
//!
//! | use case | asks |
//! |---|---|
//! | `books_check` | [`Schema::validate`], then [`Shelf::tally_by_book`] and [`assess`] for the faults around the documents |
//! | `map` | [`Schema::validate`], then [`ShelfMap::of`]; nothing else is read |
//! | `status` | [`assess`]: every identity's [`Standing`], with [`Ledger::status_of`] beside it |
//! | `get` | [`resolve`] for the reference, then the same as `status` for the one document, and [`Assessment::roots_of`] |
//! | `trace` | [`resolve`], then [`trace`](domain_trace), and the same as `status` for every document it reaches |
//! | `stale` | [`assess`]: the approvals that are [`Freshness::Degraded`], with [`Assessment::roots_of`] |
//!
//! [`Schema::validate`]: crate::domain::shelf::schema::Schema::validate
//! [`ShelfMap::of`]: crate::domain::shelf::map::ShelfMap::of
//! [`Shelf::tally_by_book`]: crate::domain::shelf::build::Shelf::tally_by_book
//! [`assess`]: crate::domain::shelf::freshness::assess
//! [`Ledger::status_of`]: crate::domain::shelf::ledger::Ledger::status_of
//! [`Standing`]: crate::domain::shelf::freshness::Standing
//! [`Assessment::roots_of`]: crate::domain::shelf::freshness::Assessment::roots_of
//! [`Freshness::Degraded`]: crate::domain::shelf::freshness::Freshness::Degraded
//! [`resolve`]: crate::domain::shelf::lookup::resolve
//! [domain_trace]: crate::domain::shelf::trace::trace

use crate::domain::id::DocId;
use crate::domain::shelf::freshness::{Freshness, Notice, Standing};
use crate::domain::shelf::lookup::{Reference, Resolution, resolve};
use crate::domain::shelf::map::ShelfMap;
use crate::domain::shelf::trace::{LinkEnd, trace};

use super::ShelfApp;
use super::error::ShelfError;
use super::load::{Loaded, View, load, load_with, schema_of};
use super::ports::ShelfRead;
use super::report::{
    Absent, BookCount, BooksCheck, DownstreamEdge, GetReport, MapReport, StaleEntry, StaleReport,
    StatusReport, TraceReport, Uncovered, Unknown, UpstreamEdge, UpstreamEnd,
};

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

impl<P: ShelfRead> ShelfApp<P> {
    /// Check the declaration of the books, the documents and the ledger, and say
    /// everything that is wrong: the problems of the declaration, the files that are
    /// not valid documents, the cycles, the records that contradict one another or
    /// could not be read.
    ///
    /// A declaration that is not sound is an answer, not an error, and then nothing
    /// else is read: there is no way to tell which files are books without it.
    pub async fn books_check(&self) -> Result<BooksCheck, ShelfError> {
        let schema = match schema_of(&self.ports).await? {
            Ok(schema) => schema,
            Err(problems) => return Ok(BooksCheck::SchemaInvalid(problems)),
        };
        let loaded = load_with(&self.ports, &schema).await?;
        let assessment = loaded.assess();
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
        Ok(BooksCheck::Checked {
            books,
            health: view.health(),
            uncovered,
            absent,
            unknown,
        })
    }

    /// What the declaration of the books says: every book and every link, in its
    /// order ([`ShelfMap::of`]).
    ///
    /// It reads the declaration and nothing else, no document and no ledger. A
    /// declaration that is not sound is an answer, as for
    /// [`books_check`](Self::books_check).
    pub async fn map(&self) -> Result<MapReport, ShelfError> {
        Ok(match schema_of(&self.ports).await? {
            Ok(schema) => MapReport::Mapped(ShelfMap::of(&schema)),
            Err(problems) => MapReport::SchemaInvalid(problems),
        })
    }

    /// Every document with what was decided about it and whether the decision still
    /// holds.
    pub async fn status(&self) -> Result<StatusReport, ShelfError> {
        let loaded = load(&self.ports).await?;
        let assessment = loaded.assess();
        let view = View {
            loaded: &loaded,
            assessment: &assessment,
        };
        let rows = assessment
            .standings()
            .keys()
            .map(|id| view.row(*id))
            .collect();
        Ok(StatusReport {
            rows,
            names: loaded.names(),
            health: view.health(),
        })
    }

    /// One document: what the reference names, with its standing, its notices and
    /// where the degradation of its approval starts.
    pub async fn get(&self, reference: &Reference) -> Result<GetReport, ShelfError> {
        let loaded = load(&self.ports).await?;
        let id = identity(&loaded, reference)?;
        let assessment = loaded.assess();
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

    /// What a document is linked to, and what is linked to it, each node with where
    /// it stands. The walk is the domain's ([`trace`]); this shows each document it
    /// reaches as a row.
    pub async fn trace(&self, reference: &Reference) -> Result<TraceReport, ShelfError> {
        let loaded = load(&self.ports).await?;
        let id = identity(&loaded, reference)?;
        let assessment = loaded.assess();
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

    /// The approvals that no longer hold, the strongest degradation first, each with
    /// where it starts to fail.
    pub async fn stale(&self) -> Result<StaleReport, ShelfError> {
        let loaded = load(&self.ports).await?;
        let assessment = loaded.assess();
        let view = View {
            loaded: &loaded,
            assessment: &assessment,
        };
        let mut entries: Vec<StaleEntry> = assessment
            .standings()
            .iter()
            .filter_map(|(id, standing)| match standing {
                Standing::Approved(Freshness::Degraded(degradation)) => Some(StaleEntry {
                    row: view.row(*id),
                    level: degradation.level(),
                    roots: assessment.roots_of(*id),
                }),
                _ => None,
            })
            .collect();
        entries.sort_by(|a, b| b.level.cmp(&a.level).then(a.row.id.cmp(&b.row.id)));
        Ok(StaleReport {
            entries,
            origins: assessment.origins(),
            names: loaded.names(),
            health: view.health(),
        })
    }
}
