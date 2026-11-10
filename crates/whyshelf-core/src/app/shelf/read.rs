//! The use cases that only read: `books check`, `status`, `get`, `trace`, `stale`.
//!
//! Each builds the schema, the shelf and the ledger ([`load`](super::load)) and asks
//! the entries of the domain; none writes, and the type they are generic over
//! ([`ShelfRead`]) has nothing to write with. What each asks, and of whom:
//!
//! | use case | asks |
//! |---|---|
//! | `books_check` | [`Schema::validate`], then [`Shelf::build`] and [`Ledger::build`] through [`assess`](crate::domain::shelf::freshness::assess) for the faults around the documents |
//! | `status` | [`assess`](crate::domain::shelf::freshness::assess): every identity's [`Standing`], with [`Ledger::status_of`] beside it |
//! | `get` | [`resolve`] for the reference, then the same as `status` for the one document, and [`Assessment::roots_of`] |
//! | `trace` | [`resolve`], then [`Shelf::links_of`] for every kind of link, up and down |
//! | `stale` | [`assess`](crate::domain::shelf::freshness::assess): the approvals that are [`Freshness::Degraded`], with [`Assessment::roots_of`] |
//!
//! [`Schema::validate`]: crate::domain::shelf::schema::Schema::validate
//! [`Shelf::build`]: crate::domain::shelf::build::Shelf::build
//! [`Ledger::build`]: crate::domain::shelf::ledger::Ledger::build
//! [`Ledger::status_of`]: crate::domain::shelf::ledger::Ledger::status_of
//! [`Standing`]: crate::domain::shelf::freshness::Standing
//! [`Assessment::roots_of`]: crate::domain::shelf::freshness::Assessment::roots_of
//! [`Shelf::links_of`]: crate::domain::shelf::build::Shelf::links_of
//! [`Freshness::Degraded`]: crate::domain::shelf::freshness::Freshness::Degraded
//! [`resolve`]: crate::domain::shelf::lookup::resolve

use std::collections::BTreeSet;

use crate::domain::id::DocId;
use crate::domain::shelf::doc::LinkState;
use crate::domain::shelf::freshness::{Freshness, Standing};
use crate::domain::shelf::lookup::{Resolution, resolve};
use crate::domain::shelf::schema::{Membership, Propagation};

use super::ShelfApp;
use super::error::ShelfError;
use super::load::{Loaded, View, load, load_with, schema_of};
use super::ports::ShelfRead;
use super::report::{
    BookCount, BooksCheck, DownstreamEdge, GetReport, StaleEntry, StaleReport, StatusReport,
    TraceReport, UpstreamEdge, UpstreamEnd,
};

/// The kinds of link a document can hold, which [`Shelf::links_of`] answers one at a
/// time.
///
/// [`Shelf::links_of`]: crate::domain::shelf::build::Shelf::links_of
const PROPAGATIONS: [Propagation; 3] = [
    Propagation::Depends,
    Propagation::Informs,
    Propagation::Replaces,
];

/// The identity a reference names, or why it names none.
// The error is read once and ends the use case; it is not a hot path.
#[allow(clippy::result_large_err)]
pub(super) fn identity(loaded: &Loaded, reference: &str) -> Result<DocId, ShelfError> {
    match resolve(&loaded.shelf, &loaded.ledger, reference) {
        Resolution::Found(id) | Resolution::LedgerOnly { id, .. } => Ok(id),
        Resolution::Missing => Err(ShelfError::NotFound {
            reference: reference.to_owned(),
        }),
        Resolution::Ambiguous(candidates) => Err(ShelfError::Ambiguous {
            reference: reference.to_owned(),
            candidates,
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
        let health = view.health();
        let books = schema
            .books()
            .values()
            .map(|book| BookCount {
                name: book.name().clone(),
                dir: book.dir().clone(),
                valid: loaded
                    .shelf
                    .docs()
                    .values()
                    .filter(|doc| doc.key().book == *book.name())
                    .count(),
                invalid: health
                    .invalid
                    .iter()
                    .filter(|file| {
                        schema.membership(file.origin()) == Membership::In(book.name().clone())
                    })
                    .count(),
            })
            .collect();
        Ok(BooksCheck::Checked { books, health })
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
    pub async fn get(&self, reference: &str) -> Result<GetReport, ShelfError> {
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
    /// it stands.
    ///
    /// Every kind of link is followed (what a document stands on, what it informs of,
    /// what it replaces), transitively, each document expanded once so a cycle ends.
    pub async fn trace(&self, reference: &str) -> Result<TraceReport, ShelfError> {
        let loaded = load(&self.ports).await?;
        let id = identity(&loaded, reference)?;
        let assessment = loaded.assess();
        let view = View {
            loaded: &loaded,
            assessment: &assessment,
        };
        Ok(TraceReport {
            root: view.row(id),
            upstream: upstream(&loaded, &view, id),
            downstream: downstream(&loaded, &view, id),
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
                Standing::Approved(Freshness::Degraded { level, .. }) => Some(StaleEntry {
                    row: view.row(*id),
                    level: *level,
                    roots: assessment.roots_of(*id),
                }),
                _ => None,
            })
            .collect();
        entries.sort_by(|a, b| b.level.cmp(&a.level).then(a.row.id.cmp(&b.row.id)));
        Ok(StaleReport {
            entries,
            names: loaded.names(),
            health: view.health(),
        })
    }
}

/// The links of `start` and of everything they reach, upstream.
fn upstream(loaded: &Loaded, view: &View<'_>, start: DocId) -> Vec<UpstreamEdge> {
    let mut edges = Vec::new();
    let mut expanded = BTreeSet::from([start]);
    let mut frontier = vec![start];
    let mut depth = 0;
    while !frontier.is_empty() {
        depth += 1;
        let mut next = Vec::new();
        for from in frontier {
            let Some(doc) = loaded.shelf.doc(from) else {
                continue;
            };
            for propagation in PROPAGATIONS {
                for (link, states) in loaded.shelf.links_of(doc, propagation) {
                    for state in states {
                        let to = match state {
                            LinkState::Resolved { target } => {
                                if expanded.insert(*target) {
                                    next.push(*target);
                                }
                                UpstreamEnd::Doc(Box::new(view.row(*target)))
                            }
                            LinkState::Unresolved(unresolved) => {
                                UpstreamEnd::Unresolved(unresolved.clone())
                            }
                        };
                        edges.push(UpstreamEdge {
                            from,
                            link: link.clone(),
                            propagation,
                            depth,
                            to,
                        });
                    }
                }
            }
        }
        frontier = next;
    }
    edges
}

/// The documents that link to `start`, and those that link to them.
fn downstream(loaded: &Loaded, view: &View<'_>, start: DocId) -> Vec<DownstreamEdge> {
    let mut edges = Vec::new();
    let mut expanded = BTreeSet::from([start]);
    let mut frontier = vec![start];
    let mut depth = 0;
    while !frontier.is_empty() {
        depth += 1;
        let mut next = Vec::new();
        for to in frontier {
            for holder in loaded.shelf.docs().values() {
                for propagation in PROPAGATIONS {
                    for (link, states) in loaded.shelf.links_of(holder, propagation) {
                        let names_it = states.iter().any(
                            |state| matches!(state, LinkState::Resolved { target } if *target == to),
                        );
                        if !names_it {
                            continue;
                        }
                        if expanded.insert(holder.id()) {
                            next.push(holder.id());
                        }
                        edges.push(DownstreamEdge {
                            to,
                            link: link.clone(),
                            propagation,
                            depth,
                            holder: Box::new(view.row(holder.id())),
                        });
                    }
                }
            }
        }
        frontier = next;
    }
    edges
}
