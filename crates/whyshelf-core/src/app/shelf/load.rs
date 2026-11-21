//! Building what every use case stands on: the schema, the shelf and the ledger.
//!
//! This is the whole of the application's part in the question "what is the state of
//! the documents": it asks the ports for what they hold and hands it to the three
//! entries of the domain ([`Schema::declared`], [`Shelf::build`], [`Ledger::build`]),
//! in that order, and decides nothing itself. [`View`] then shows an identity as a
//! [`Row`], with what the model said about it, and shows no more than the model said.
//!
//! ```text
//!   BookSchemaSource -> Schema::declared <- ArtifactSource::presets, ::spaces
//!                          |  (expand, then validate)
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

use std::collections::BTreeSet;

use crate::domain::id::DocId;
use crate::domain::nonempty::NonEmptyVec;
use crate::domain::shelf::artifact::ArtifactReadings;
use crate::domain::shelf::build::Shelf;
use crate::domain::shelf::freshness::{Assessment, assess};
use crate::domain::shelf::ledger::{Admit, AdmittedRecord, DecisionVerb, JudgedAt, Ledger, Record};
use crate::domain::shelf::schema::{Schema, SchemaProblem};

use super::error::ShelfError;
use super::ports::{
    ArtifactSource, BookSchemaSource, Clock, DocSource, LedgerRead, ShelfPort, ShelfRead,
    UnreadableRecord, port,
};
use super::report::{Health, Names, Row};

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

/// Read the project through the ports and build the shelf and the ledger.
///
/// A declaration that is not sound is [`ShelfError::Schema`]: nothing can be read
/// through it. (`check` wants the problems as an answer, so it builds the
/// schema itself with [`schema_of`].)
pub(super) async fn load<P: ShelfRead>(ports: &P) -> Result<Loaded, ShelfError> {
    let schema = schema_of(ports).await?.map_err(ShelfError::Schema)?;
    load_with(ports, &schema).await
}

/// The schema, or all its problems.
pub(super) async fn schema_of<P: ShelfRead>(
    ports: &P,
) -> Result<Result<Schema, NonEmptyVec<SchemaProblem>>, ShelfError> {
    let decl = ports
        .schemas()
        .declarations()
        .await
        .map_err(port(ShelfPort::Schema))?;
    let artifacts = ports.artifacts();
    Ok(Schema::declared(
        decl,
        &artifacts.presets(),
        &artifacts.spaces(),
    ))
}

/// Read the documents and the ledger and build the shelf and the ledger.
pub(super) async fn load_with<P: ShelfRead>(
    ports: &P,
    schema: &Schema,
) -> Result<Loaded, ShelfError> {
    let readings = ports
        .docs()
        .read_docs(&schema.ranges())
        .await
        .map_err(port(ShelfPort::Docs))?;
    let asks = Shelf::artifact_asks(schema, &readings);
    let artifacts = if asks.is_empty() {
        ArtifactReadings::new()
    } else {
        ports
            .artifacts()
            .resolve(&asks)
            .await
            .map_err(port(ShelfPort::Artifacts))?
    };
    let shelf = Shelf::build(schema, readings, &artifacts);
    let contents = ports
        .ledger()
        .read()
        .await
        .map_err(port(ShelfPort::LedgerRead))?;
    let admit = Admit::of(&shelf);
    let ledger = Ledger::build(contents.records.clone(), &admit);
    let now = ports.clock().now().await.map_err(port(ShelfPort::Clock))?;
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

    /// The freshness of everything, at the time of the load.
    pub(super) fn assess(&self) -> Assessment {
        assess(&self.shelf, &self.ledger, self.now)
    }

    /// The keys of the valid documents, and what each upstream a decision of the
    /// ledger pinned comes to now ([`Shelf::now_named`]), so that an answer can name a
    /// pin by the key its upstream has.
    pub(super) fn names(&self) -> Names {
        // Each upstream once: many approvals pin the same one.
        let upstreams: BTreeSet<DocId> = self
            .ledger
            .decisions()
            .filter_map(|decision| match decision.verb() {
                DecisionVerb::Approve { frame, .. } => Some(frame),
                DecisionVerb::Deny { .. }
                | DecisionVerb::Deprecate { .. }
                | DecisionVerb::Retire { .. } => None,
            })
            .flatten()
            .map(|pin| pin.target())
            .collect();
        let pinned = upstreams
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
