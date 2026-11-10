//! Building what every use case stands on: the schema, the shelf and the ledger.
//!
//! This is the whole of the application's part in the question "what is the state of
//! the documents": it asks the ports for what they hold and hands it to the three
//! entries of the domain ([`Schema::validate`], [`Shelf::build`], [`Ledger::build`]),
//! in that order, and decides nothing itself. [`View`] then shows an identity as a
//! [`Row`], with what the model said about it, and shows no more than the model said.
//!
//! ```text
//!   BookSchemaSource -> Schema::validate -+
//!                                         |  dirs of the books
//!   DocSource <---------------------------+
//!        |
//!        v  readings
//!   Shelf::build --------+
//!                        +--> Loaded --assess--> Assessment --> View --> reports
//!   LedgerRead -> Ledger::build
//!   Clock ---------------------------------------^ (now)
//! ```

use crate::domain::id::DocId;
use crate::domain::nonempty::NonEmptyVec;
use crate::domain::shelf::build::{DocState, DocStateKind, Shelf};
use crate::domain::shelf::freshness::{Assessment, Standing, assess};
use crate::domain::shelf::ledger::{JudgedAt, Ledger};
use crate::domain::shelf::schema::{BookDir, Schema, SchemaProblem};

use super::error::ShelfError;
use super::ports::{
    BookSchemaSource, Clock, DocSource, LedgerRead, ShelfPort, ShelfRead, UnreadableRecord, port,
};
use super::report::{Health, Names, Presence, Row};

/// What the ports gave, built by the domain.
#[derive(Debug)]
pub(super) struct Loaded {
    pub(super) shelf: Shelf,
    pub(super) ledger: Ledger,
    pub(super) unreadable: Vec<UnreadableRecord>,
    pub(super) now: JudgedAt,
}

/// Read the project through the ports and build the shelf and the ledger.
///
/// A declaration that is not sound is [`ShelfError::Schema`]: nothing can be read
/// through it. (`books check` wants the problems as an answer, so it builds the
/// schema itself with [`schema_of`].)
pub(super) async fn load<P: ShelfRead>(ports: &P) -> Result<Loaded, ShelfError> {
    let schema = schema_of(ports).await?.map_err(ShelfError::Schema)?;
    load_with(ports, &schema).await
}

/// The schema, or all its problems.
pub(super) async fn schema_of<P: ShelfRead>(
    ports: &P,
) -> Result<Result<Schema, NonEmptyVec<SchemaProblem>>, ShelfError> {
    let decls = ports
        .schemas()
        .declarations()
        .await
        .map_err(port(ShelfPort::Schema))?;
    Ok(Schema::validate(decls))
}

/// Read the documents and the ledger and build the shelf and the ledger.
pub(super) async fn load_with<P: ShelfRead>(
    ports: &P,
    schema: &Schema,
) -> Result<Loaded, ShelfError> {
    let dirs: Vec<BookDir> = schema.books().values().map(|b| b.dir().clone()).collect();
    let readings = ports
        .docs()
        .read_docs(&dirs)
        .await
        .map_err(port(ShelfPort::Docs))?;
    let shelf = Shelf::build(schema, readings);
    let contents = ports
        .ledger()
        .read()
        .await
        .map_err(port(ShelfPort::LedgerRead))?;
    let ledger = Ledger::build(contents.records);
    let now = ports.clock().now().await.map_err(port(ShelfPort::Clock))?;
    Ok(Loaded {
        shelf,
        ledger,
        unreadable: contents.unreadable,
        now,
    })
}

impl Loaded {
    /// The freshness of everything, at the time of the load.
    pub(super) fn assess(&self) -> Assessment {
        assess(&self.shelf, &self.ledger, self.now)
    }

    /// The keys of the valid documents.
    pub(super) fn names(&self) -> Names {
        Names(
            self.shelf
                .docs()
                .iter()
                .map(|(id, doc)| (*id, doc.key().clone()))
                .collect(),
        )
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

    /// What the shelf has for an identity now, from [`Shelf::state_of`] and
    /// [`DocState::valid_or_kind`].
    pub(super) fn presence(&self, id: DocId) -> Presence {
        let state = self.loaded.shelf.state_of(id);
        match state.valid_or_kind() {
            Ok(doc) => Presence::Valid {
                key: doc.key().clone(),
                origin: doc.origin().clone(),
                hash: *doc.hash(),
            },
            Err(DocStateKind::Invalid(problems)) => Presence::Invalid {
                files: match &state {
                    DocState::Invalid(docs) => docs.iter().map(|d| d.origin().clone()).collect(),
                    _ => Vec::new(),
                },
                problems,
            },
            Err(DocStateKind::Gone) => Presence::Gone,
            Err(DocStateKind::Unknown(unreadable)) => Presence::Unknown { unreadable },
        }
    }

    /// An identity as the answers show it.
    ///
    /// An identity the assessment does not list has no file and no record, so what
    /// stands about it is what [`Ledger::status_of`] says of an identity with no
    /// decision: it is pending.
    pub(super) fn row(&self, id: DocId) -> Row {
        Row {
            id,
            presence: self.presence(id),
            declared: self.loaded.ledger.status_of(id),
            standing: self
                .assessment
                .standings()
                .get(&id)
                .cloned()
                .unwrap_or(Standing::Pending),
            notices: self
                .assessment
                .notices()
                .get(&id)
                .cloned()
                .unwrap_or_default(),
        }
    }
}
