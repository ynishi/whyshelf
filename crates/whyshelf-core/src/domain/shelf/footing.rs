//! What a document stands on: the one place it is read.
//!
//! A document stands on the documents its `Depends` links resolve to now
//! (`Doc::depends_links`, the document's own rule) and on those the approval that
//! stands about it pinned ([`Ledger::declared`]). [`Footing::of`] reads both, once,
//! for one document; the order of approvals
//! ([`upstream_first`](super::order::upstream_first)) reads them from it.

use std::collections::BTreeSet;

use crate::domain::id::DocId;

use super::build::Shelf;
use super::ledger::{DeclaredNow, FramePin, Ledger};

/// What one document stands on: what its `Depends` links resolve to now, and what the
/// approval that stands about it pinned.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Footing {
    linked: BTreeSet<DocId>,
    pinned: BTreeSet<DocId>,
}

impl Footing {
    /// What `id` stands on in `shelf` and `ledger`: the targets of its `Depends` links
    /// that resolve now (none for an identity with no valid document), and the
    /// upstreams the frame of its approval pinned (none when no approval stands).
    pub(crate) fn of(shelf: &Shelf, id: DocId, ledger: &Ledger) -> Self {
        let linked = shelf
            .doc(id)
            .map(|doc| doc.depends_links().map(|(_, target)| target).collect())
            .unwrap_or_default();
        let pinned = match ledger.declared(id) {
            DeclaredNow::Approved(standing) => standing
                .view()
                .frame()
                .iter()
                .map(FramePin::target)
                .collect(),
            _ => BTreeSet::new(),
        };
        Self { linked, pinned }
    }

    /// What its `Depends` links resolve to now.
    pub(crate) fn linked(&self) -> &BTreeSet<DocId> {
        &self.linked
    }

    /// Every document it stands on: what its links resolve to and what its approval
    /// pinned.
    pub(crate) fn docs(&self) -> BTreeSet<DocId> {
        self.linked.union(&self.pinned).copied().collect()
    }
}
