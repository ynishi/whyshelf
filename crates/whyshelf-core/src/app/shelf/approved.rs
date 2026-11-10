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
//!                   ArtifactSource for an artifact) + the bytes now
//!                   + SnapshotItem::changed_from
//! ```
//!
//! [`ShelfRead`]: super::ShelfRead
//! [`ContentStore`]: super::ports::ContentStore

use std::collections::BTreeMap;

use crate::domain::location::RelPath;
use crate::domain::shelf::artifact::{ArtifactAsks, ArtifactState, ContentAddress};
use crate::domain::shelf::snapshot::{ItemNow, ItemRole, Snapshot, SnapshotItem, doc_now};

use super::ShelfApp;
use super::error::ShelfError;
use super::load::{Loaded, View, load};
use super::ports::{ArtifactSource, ContentStore, ShelfPort, ShelfRead, port};
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
                retrieval: item.retrieval(&readable),
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

        let mut asks = ArtifactAsks::new();
        for item in snapshot.items() {
            if let ItemRole::Artifact {
                path, comparison, ..
            } = item.role()
            {
                asks.entry(path.clone())
                    .or_default()
                    .insert(comparison.clone());
            }
        }
        let artifacts_now = if asks.is_empty() {
            BTreeMap::new()
        } else {
            self.ports
                .artifacts()
                .read_artifacts(&asks)
                .await
                .map_err(port(ShelfPort::Artifacts))?
        };

        let mut items = Vec::with_capacity(snapshot.items().len());
        for item in snapshot.items() {
            let (now, file) = match item.role() {
                ItemRole::Subject { id } | ItemRole::Upstream { target: id, .. } => {
                    let now = doc_now(&loaded.shelf, *id);
                    (now.now, now.file)
                }
                ItemRole::Artifact {
                    path, comparison, ..
                } => {
                    let state = artifacts_now
                        .get(path)
                        .map_or(ArtifactState::Absent, |reading| reading.state(comparison));
                    let file = matches!(state, ArtifactState::Present(_)).then(|| path.clone());
                    (ItemNow::Artifact(state), file)
                }
            };
            let current = match &file {
                Some(file) => self
                    .ports
                    .artifacts()
                    .current(file)
                    .await
                    .map_err(port(ShelfPort::Artifacts))?,
                None => None,
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

    /// The project, loaded, and the snapshot of the approval that stands about what
    /// the reference names.
    async fn approved(&self, reference: &Reference) -> Result<(Loaded, Snapshot), ShelfError> {
        let loaded = load(&self.ports).await?;
        let id = identity(&loaded, reference)?;
        let snapshot =
            Snapshot::of(&loaded.ledger, id).ok_or(ShelfError::NotApproved { subject: id })?;
        Ok((loaded, snapshot))
    }

    /// The contents the store gives back for the addresses the snapshot keeps; an
    /// address it cannot read is left out, which the domain reads as not readable.
    async fn read_back(
        &self,
        snapshot: &Snapshot,
    ) -> Result<BTreeMap<ContentAddress, Vec<u8>>, ShelfError> {
        let mut readable = BTreeMap::new();
        for address in snapshot.addresses() {
            let content = self
                .ports
                .store()
                .read_back(&address)
                .await
                .map_err(port(ShelfPort::Store))?;
            if let Some(content) = content {
                readable.insert(address, content);
            }
        }
        Ok(readable)
    }
}

fn diff_item(
    item: &SnapshotItem,
    readable: &BTreeMap<ContentAddress, Vec<u8>>,
    now: ItemNow,
    file: Option<RelPath>,
    current: Option<Vec<u8>>,
) -> DiffItem {
    DiffItem {
        item: item.clone(),
        approved: item.retrieval(readable),
        changed: item.changed_from(&now),
        now,
        file,
        current,
    }
}
