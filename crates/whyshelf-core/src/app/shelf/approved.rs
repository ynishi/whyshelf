//! The use cases that look at what an approval saw: `show_approved` and `diff`.
//!
//! Both read only (they are generic over [`ShelfRead`]); bringing content back is
//! for a person to read, and putting it back in place is version control's work and a
//! person's, never this program's. Each asks the domain what the approval that stands
//! saw ([`Snapshot::of`]), asks the [`ContentStore`] for the content at every address
//! it keeps, and lets the domain say what that came to for each item
//! ([`SnapshotItem::retrieval`]).
//!
//! ```text
//!   reference --resolve--> id --Snapshot::of--> Snapshot
//!                                                 | addresses
//!                                                 v
//!                                ContentStore::read_back --> readable contents
//!                                                 |
//!   show_approved:  each item + SnapshotItem::retrieval
//!   diff:           each item + retrieval + what it is now (doc_now, or the
//!                   ArtifactSource for an artifact) + the bytes now (DocSource,
//!                   or ArtifactSource) + SnapshotItem::changed_from
//! ```
//!
//! [`ShelfRead`]: super::ShelfRead
//! [`ContentStore`]: super::ports::ContentStore

use std::collections::{BTreeMap, BTreeSet};

use crate::domain::location::RelPath;
use crate::domain::shelf::artifact::{
    ArtifactAsks, ArtifactReadings, ArtifactState, ContentAddress,
};
use crate::domain::shelf::ledger::MovedTo;
use crate::domain::shelf::snapshot::{
    ItemNow, ItemRole, Retrieval, Snapshot, SnapshotItem, doc_now,
};

use super::ShelfApp;
use super::error::ShelfError;
use super::load::{Loaded, View, load};
use super::ports::{
    ArtifactSource, ContentOf, ContentStore, DocSource, ShelfPort, ShelfRead, port,
};
use super::read::identity;
use super::report::{ApprovedItem, ApprovedReport, DiffItem, DiffReport};
use crate::domain::shelf::lookup::Reference;

impl<P: ShelfRead> ShelfApp<P> {
    /// What the approval that stands about a document saw, each item with its
    /// content as it was approved, or why that could not be read back.
    ///
    /// A document no approval stands about is [`ShelfError::NotApproved`].
    pub async fn show_approved(&self, reference: &Reference) -> Result<ApprovedReport, ShelfError> {
        let (loaded, snapshot) = self.approved(reference).await?;
        let readable = self.read_back(&snapshot).await?;
        let items = snapshot
            .items()
            .iter()
            .map(|item| ApprovedItem {
                item: item.clone(),
                retrieval: retrieval_of(item, &readable),
            })
            .collect();
        let assessment = loaded.assess();
        let view = View {
            loaded: &loaded,
            assessment: &assessment,
        };
        Ok(ApprovedReport {
            row: view.row(snapshot.subject()),
            items,
            names: loaded.names(),
            health: view.health(),
            snapshot,
        })
    }

    /// Each item of the approval that stands about a document, as it was approved and
    /// as it is now: the contents as values and whether the hash changed. Writing the
    /// difference as text is the surface's work.
    pub async fn diff(&self, reference: &Reference) -> Result<DiffReport, ShelfError> {
        let (loaded, snapshot) = self.approved(reference).await?;
        let readable = self.read_back(&snapshot).await?;

        // An artifact is asked by the name it is read as (the one a declared move
        // took it to), as freshness compares it.
        let mut asks = ArtifactAsks::new();
        for item in snapshot.items() {
            if let ItemRole::Artifact {
                artifact,
                comparison,
                ..
            } = item.role()
            {
                let artifact = item.moved().map_or(artifact, MovedTo::artifact);
                asks.ask(
                    artifact.space().clone(),
                    artifact.name().clone(),
                    comparison.clone(),
                );
            }
        }
        let artifacts_now = if asks.is_empty() {
            ArtifactReadings::new()
        } else {
            self.ports
                .artifacts()
                .resolve(&asks)
                .await
                .map_err(port(ShelfPort::Artifacts))?
        };

        let mut items = Vec::with_capacity(snapshot.items().len());
        for item in snapshot.items() {
            let (now, file, current) = match item.role() {
                ItemRole::Subject { id } | ItemRole::Upstream { target: id, .. } => {
                    let now = doc_now(&loaded.shelf, *id);
                    let current = match &now.file {
                        Some(file) => self.doc_bytes(file).await?,
                        None => None,
                    };
                    (now.now, now.file, current)
                }
                ItemRole::Artifact {
                    artifact,
                    comparison,
                    ..
                } => {
                    let artifact = item.moved().map_or(artifact, MovedTo::artifact);
                    let state = artifacts_now.state_of(artifact, comparison);
                    let current = match state {
                        ArtifactState::Present(_) => self
                            .ports
                            .artifacts()
                            .current(artifact)
                            .await
                            .map_err(port(ShelfPort::Artifacts))?,
                        ArtifactState::Absent | ArtifactState::Unknown(_) => None,
                    };
                    (ItemNow::Artifact(state), None, current)
                }
            };
            items.push(diff_item(item, &readable, now, file, current));
        }

        let assessment = loaded.assess();
        let view = View {
            loaded: &loaded,
            assessment: &assessment,
        };
        Ok(DiffReport {
            row: view.row(snapshot.subject()),
            items,
            names: loaded.names(),
            health: view.health(),
            snapshot,
        })
    }

    /// The bytes of a document's file now.
    async fn doc_bytes(&self, file: &RelPath) -> Result<Option<Vec<u8>>, ShelfError> {
        self.ports
            .docs()
            .current(file)
            .await
            .map_err(port(ShelfPort::Docs))
    }

    /// The project, loaded, and the snapshot of the approval that stands about what
    /// the reference names.
    async fn approved(&self, reference: &Reference) -> Result<(Loaded, Snapshot), ShelfError> {
        let loaded = load(&self.ports).await?;
        let id = identity(&loaded, reference)?;
        let snapshot =
            Snapshot::of(&loaded.ledger, id).ok_or(ShelfError::NotApproved { subject: id })?;
        Ok((loaded, snapshot))
    }

    /// The contents the store gives back for the addresses the snapshot keeps, each
    /// asked of the adapter that named it (an address is one adapter's: the same
    /// address from the documents and from a space, or from two spaces, is two
    /// contents); an address it cannot read is left out, which the domain reads as
    /// not readable.
    async fn read_back(&self, snapshot: &Snapshot) -> Result<Readable, ShelfError> {
        let asked: BTreeSet<(ContentOf, ContentAddress)> = snapshot
            .items()
            .iter()
            .filter_map(|item| Some((content_of(item), item.address()?.clone())))
            .collect();
        let mut readable = Readable::new();
        for (of, address) in asked {
            let content = self
                .ports
                .store()
                .read_back(&of, &address)
                .await
                .map_err(port(ShelfPort::Store))?;
            if let Some(content) = content {
                readable.insert((of, address), content);
            }
        }
        Ok(readable)
    }
}

/// The contents read back, by whose they are and their address.
type Readable = BTreeMap<(ContentOf, ContentAddress), Vec<u8>>;

/// Whose content an item is: the adapter that named its address.
fn content_of(item: &SnapshotItem) -> ContentOf {
    match item.role() {
        ItemRole::Subject { .. } | ItemRole::Upstream { .. } => ContentOf::Doc,
        ItemRole::Artifact { artifact, .. } => ContentOf::Artifact(artifact.space().clone()),
    }
}

/// What reading one item back came to ([`SnapshotItem::retrieval`]), given only the
/// content its own adapter gave back for its address.
fn retrieval_of(item: &SnapshotItem, readable: &Readable) -> Retrieval {
    let own: BTreeMap<ContentAddress, Vec<u8>> = item
        .address()
        .and_then(|address| {
            readable
                .get(&(content_of(item), address.clone()))
                .map(|content| (address.clone(), content.clone()))
        })
        .into_iter()
        .collect();
    item.retrieval(&own)
}

fn diff_item(
    item: &SnapshotItem,
    readable: &Readable,
    now: ItemNow,
    file: Option<RelPath>,
    current: Option<Vec<u8>>,
) -> DiffItem {
    DiffItem {
        item: item.clone(),
        approved: retrieval_of(item, readable),
        changed: item.changed_from(&now),
        now,
        file,
        current,
    }
}
