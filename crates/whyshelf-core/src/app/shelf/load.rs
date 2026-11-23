//! Building what every use case stands on: the schema, the shelf and the ledger.
//!
//! This is the whole of the application's part in the question "what is the state of
//! the documents": it asks the ports for what they hold and hands it to the three
//! entries of the domain ([`Schema::declared`], [`Shelf::build`], [`Ledger::build`]),
//! in that order, and decides nothing itself. [`View`] then shows an identity as a
//! [`Row`], with what the model said about it, and shows no more than the model said.
//!
//! ```text
//!   BookSchemaSource -> declared_schema --Schema::selection_asks--> ArtifactSource::select
//!                          |  (the answers, then Schema::declared: expand, then validate,
//!                          |   with ArtifactSource::presets, ::spaces)
//!                          |  dirs of the books
//!   DocSource <------------+
//!        |
//!        v  readings --Shelf::artifact_asks--> ArtifactSource::resolve (per space: present,
//!        |                                     absent, cannot tell, or not a name)
//!        |                                            |
//!        v                                            v  ArtifactReadings
//!   Shelf::build <------------------------------------+
//!        |
//!        +---------------+--> Admit::of (who may decide, the book of each document)
//!                        |        |
//!                        +--> Loaded --assess--> Assessment --> View --> reports
//!   LedgerRead -> Ledger::build <-+
//!   Clock ---------------------------------------^ (now)
//!
//!   (no name written in a link to artifacts: the ArtifactSource is not asked to resolve)
//! ```
//!
//! Every port is read at one [`Side`] of a change, and the time is read once, by the
//! use case, before it ([`now`]): [`load_at`] is the one reading of a side, with that
//! time, and [`load`] is the reading of [`Side::After`] with a time read for it. A use
//! case that reads both sides of a change reads the time once, and both sides with it.

use std::collections::{BTreeMap, BTreeSet};

use crate::domain::id::DocId;
use crate::domain::nonempty::NonEmptyVec;
use crate::domain::shelf::artifact::{
    ArtifactReadings, Listings, Selection, SelectionChecks, Space,
};
use crate::domain::shelf::build::Shelf;
use crate::domain::shelf::freshness::{Assessment, assess};
use crate::domain::shelf::ledger::{Admit, AdmittedRecord, JudgedAt, Ledger, Record};
use crate::domain::shelf::schema::{Schema, SchemaProblem, ShelfDecl};

use super::error::ShelfError;
use super::ports::{
    ArtifactSource, BookSchemaSource, Clock, DocSource, LedgerRead, ShelfPort, ShelfRead,
    UnreadableRecord, port,
};
use super::report::{Health, Names, Row};
use super::state::Side;

/// What the ports gave, built by the domain.
#[derive(Debug)]
pub(super) struct Loaded {
    pub(super) shelf: Shelf,
    pub(super) ledger: Ledger,
    /// What the ledger was read with of who may decide, and is built again with.
    admit: Admit,
    /// The records the ledger was built from, so that a run of judgements can build it
    /// again with the ones it adds.
    pub(super) records: Vec<Record>,
    pub(super) unreadable: Vec<UnreadableRecord>,
    /// What the artifact source answered for the names the documents link to, with
    /// the addresses: a judgement keeps the address of what it pinned from here, so
    /// the address and the hash are of one reading.
    pub(super) artifacts: ArtifactReadings,
    pub(super) now: JudgedAt,
}

/// Read the project as it is ([`Side::After`]) through the ports and build the shelf
/// and the ledger, at a time read once for the reading ([`now`]).
///
/// A declaration that is not sound is [`ShelfError::Schema`]: nothing can be read
/// through it. (A use case that answers such a declaration, or holds what it is
/// asked against the declaration before the documents are read, as `check`,
/// `status` with a filter, `cover` and `unlinked` do, reads the schema itself with
/// [`schema_of`] and the rest with [`load_with`], at the side and the time it gives.)
pub(super) async fn load<P: ShelfRead>(ports: &P) -> Result<Loaded, ShelfError> {
    let now = now(ports).await?;
    load_at(ports, Side::After, now).await
}

/// Read the project at `side` through the ports and build the shelf and the ledger,
/// judged at `now`, which the caller read once: the reading of a side for a use case
/// that needs nothing of the declaration first.
pub(super) async fn load_at<P: ShelfRead>(
    ports: &P,
    side: Side,
    now: JudgedAt,
) -> Result<Loaded, ShelfError> {
    let schema = schema_of(ports, side).await?.map_err(ShelfError::Schema)?;
    load_with(ports, side, &schema, now).await
}

/// Now, read once from the clock: the time every side a use case reads is judged at.
pub(super) async fn now<P: ShelfRead>(ports: &P) -> Result<JudgedAt, ShelfError> {
    ports.clock().now().await.map_err(port(ShelfPort::Clock))
}

/// The schema at `side`, or all its problems.
pub(super) async fn schema_of<P: ShelfRead>(
    ports: &P,
    side: Side,
) -> Result<Result<Schema, NonEmptyVec<SchemaProblem>>, ShelfError> {
    let decl = ports
        .schemas()
        .declarations(side)
        .await
        .map_err(port(ShelfPort::Schema))?;
    declared_schema(decl, ports.artifacts())
}

/// The schema of a declaration, or all its problems: the one way a declaration
/// becomes a [`Schema`] in the application, for `load` and for `init`.
///
/// The selections of the `cover` of its links to artifacts
/// ([`Schema::selection_asks`]) are put to the adapters of their spaces
/// ([`ArtifactSource::select`], which does no I/O), and what they answered is handed
/// to [`Schema::declared`], which refuses a `cover` whose selection its space does
/// not read. The asks and the check read the same declaration the same way (the
/// domain's, once), so no selection is left without an answer; one that was would be
/// read as a space that cannot list, and refused.
// The problems are the answer, read once per run; it is not a hot path.
#[allow(clippy::result_large_err)]
pub(super) fn declared_schema<A: ArtifactSource>(
    decl: ShelfDecl,
    artifacts: &A,
) -> Result<Result<Schema, NonEmptyVec<SchemaProblem>>, ShelfError> {
    let (presets, spaces) = (artifacts.presets(), artifacts.spaces());
    let mut checks = SelectionChecks::new();
    for (space, selection) in Schema::selection_asks(&decl, &spaces, &presets) {
        let check = artifacts
            .select(&space, &selection)
            .map_err(port(ShelfPort::Artifacts))?;
        checks.insert((space, selection), check);
    }
    Ok(Schema::declared(decl, &presets, &spaces, &checks))
}

/// What the adapters list of each (space, selection) in `asks` at `side`, once for
/// each: the listings a command that reads an artifact cover reads it with. `check`
/// asks every selection the `cover` of a link to artifacts declares
/// ([`Schema::cover_selections`]); a command about some links asks theirs only. A
/// command that reads none passes [`Listings::NotAsked`] and asks no adapter.
pub(super) async fn listings_of<A: ArtifactSource>(
    artifacts: &A,
    side: Side,
    asks: BTreeSet<(Space, Selection)>,
) -> Result<Listings, ShelfError> {
    let mut asked = BTreeMap::new();
    for (space, selection) in asks {
        let listing = artifacts
            .list(side, &space, &selection)
            .await
            .map_err(port(ShelfPort::Artifacts))?;
        asked.insert((space, selection), listing);
    }
    Ok(Listings::Asked(asked))
}

/// Read the documents and the ledger at `side` and build the shelf and the ledger,
/// judged at `now`, which the caller read once ([`now`]).
pub(super) async fn load_with<P: ShelfRead>(
    ports: &P,
    side: Side,
    schema: &Schema,
    now: JudgedAt,
) -> Result<Loaded, ShelfError> {
    let readings = ports
        .docs()
        .read_docs(side, &schema.ranges())
        .await
        .map_err(port(ShelfPort::Docs))?;
    let asks = Shelf::artifact_asks(schema, &readings);
    let artifacts = if asks.is_empty() {
        ArtifactReadings::new()
    } else {
        ports
            .artifacts()
            .resolve(side, &asks)
            .await
            .map_err(port(ShelfPort::Artifacts))?
    };
    let shelf = Shelf::build(schema, readings, &artifacts);
    let contents = ports
        .ledger()
        .read(side)
        .await
        .map_err(port(ShelfPort::LedgerRead))?;
    let admit = Admit::of(&shelf);
    let ledger = Ledger::build(contents.records.clone(), &admit);
    Ok(Loaded {
        shelf,
        ledger,
        admit,
        records: contents.records,
        unreadable: contents.unreadable,
        artifacts,
        now,
    })
}

impl Loaded {
    /// `record`, accepted on the ledger as the run has left it ([`Ledger::accept`]),
    /// and taken into the reading, so that the next record of the run is decided and
    /// accepted on the ledger this one leaves: the one way a use case adds a record it
    /// writes. A record the rules of who may decide refuse is
    /// [`ShelfError::NotAdmitted`], and is not taken.
    // The error is read once and ends the use case; it is not a hot path.
    #[allow(clippy::result_large_err)]
    pub(super) fn take(&mut self, record: Record) -> Result<AdmittedRecord, ShelfError> {
        let admitted = self.ledger.accept(record)?;
        self.records.push(admitted.record().clone());
        self.ledger = Ledger::build(self.records.clone(), &self.admit);
        Ok(admitted)
    }

    /// The freshness of everything, at the time of the load, with the assessment of
    /// coverage made from `listings`: [`Listings::NotAsked`] for a use case that reads
    /// no cover of artifacts, what [`listings_of`] returned for one that does.
    pub(super) fn assess(&self, listings: &Listings) -> Assessment {
        assess(&self.shelf, &self.ledger, self.now, listings)
    }

    /// The keys of the valid documents, and what each upstream a decision of the
    /// ledger pinned comes to now ([`Shelf::now_named`]), so that an answer can name a
    /// pin by the key its upstream has.
    pub(super) fn names(&self) -> Names {
        // Each upstream once: many approvals pin the same one.
        let pinned = self
            .ledger
            .upstreams_pinned()
            .into_iter()
            .map(|id| (id, self.shelf.now_named(id)))
            .collect();
        Names {
            keys: self
                .shelf
                .docs()
                .iter()
                .map(|(id, doc)| (*id, doc.key().clone()))
                .collect(),
            pinned,
        }
    }
}

/// A loaded shelf with its assessment: what the reports are made from.
pub(super) struct View<'a> {
    pub(super) loaded: &'a Loaded,
    pub(super) assessment: &'a Assessment,
}

impl<'a> View<'a> {
    /// What is wrong around the documents.
    pub(super) fn health(&self) -> Health {
        Health {
            invalid: self.assessment.invalid().to_vec(),
            cycles: self.assessment.cycles().to_vec(),
            ledger_problems: self.assessment.ledger_problems().to_vec(),
            unreadable_records: self.loaded.unreadable.clone(),
        }
    }

    /// An identity as the answers show it: each part is one entry of the domain
    /// ([`DocState::presence`](crate::domain::shelf::build::DocState::presence),
    /// [`Ledger::status_of`], [`Ledger::standing_decision`], [`Assessment::standing_of`],
    /// [`Assessment::notices_of`]), and nothing is decided here.
    pub(super) fn row(&self, id: DocId) -> Row {
        Row {
            id,
            presence: self.loaded.shelf.state_of(id).presence(),
            declared: self.loaded.ledger.status_of(id),
            decision: self.loaded.ledger.standing_decision(id).cloned(),
            standing: self.assessment.standing_of(id),
            notices: self.assessment.notices_of(id).to_vec(),
        }
    }
}
