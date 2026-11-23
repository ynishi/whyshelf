//! What a document stands on: the one place it is read.
//!
//! A document stands on the documents its `Depends` links resolve to now
//! (`Doc::depends_links`, the document's own rule) and on those the approval that
//! stands about it pinned ([`Ledger::declared`]); and on the artifacts its `Depends`
//! links to artifacts name now and those that approval pinned, each pin as it is read
//! now (`ApprovalView::read_pins`). [`Footing::of`] reads the documents, once, for one
//! document, and [`Footing::artifacts`] the artifacts, when they are asked for: the
//! order of approvals ([`upstream_first`](super::order::upstream_first)) reads the
//! documents alone, and what a reach of documents stands on (`reach`) the artifacts.

use std::collections::BTreeSet;

use crate::domain::id::DocId;

use super::artifact::ArtifactKey;
use super::build::Shelf;
use super::ledger::{DeclaredNow, FramePin, Ledger};
use super::schema::ArtifactPropagation;

/// What one document stands on: what its `Depends` links resolve to now, and what the
/// approval that stands about it pinned; and, asked for, the artifacts of both.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Footing {
    id: DocId,
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
        Self { id, linked, pinned }
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

    /// Every artifact it stands on in `shelf` and `ledger` (the ones it was read
    /// from), by its space and name: what its `Depends` links to artifacts name now,
    /// there or not, and what the approval that stands about it pinned, each pin by
    /// the name it is read as now (through the moves declared). Read when asked, as
    /// reading the pins through the moves is work the order of approvals never needs.
    pub(crate) fn artifacts(&self, shelf: &Shelf, ledger: &Ledger) -> BTreeSet<ArtifactKey> {
        let mut artifacts: BTreeSet<ArtifactKey> = shelf
            .doc(self.id)
            .map(|doc| {
                doc.artifact_links_of(ArtifactPropagation::Depends)
                    .into_iter()
                    .flat_map(|(_, written)| written.iter().map(|a| a.key().clone()))
                    .collect()
            })
            .unwrap_or_default();
        if let DeclaredNow::Approved(standing) = ledger.declared(self.id) {
            let pins = standing.view().read_pins();
            artifacts.extend(pins.iter().map(|pin| pin.read_as().clone()));
        }
        artifacts
    }
}

#[cfg(test)]
mod tests {
    use crate::domain::nonempty::NonBlank;
    use crate::domain::shelf::artifact::ArtifactHash;
    use crate::domain::shelf::fixtures_tests::{
        approve, by, docid, file_artifact, file_space, fr, jid, ledger, name, shelf_with,
        sp_testing,
    };
    use crate::domain::shelf::ledger::{JudgedAt, Moved, MovedOf, Record};

    use super::*;

    #[test]
    fn a_pin_is_stood_on_by_the_name_it_is_read_as_through_a_declared_move() {
        let hash = Some(ArtifactHash::of_content(b"old\n"));
        let before = shelf_with(
            vec![fr(1, &[]), sp_testing(2, &["FR-1"], &["t/old.rs"])],
            &[("t/old.rs", hash)],
        );
        let first = approve(&before, &ledger(vec![]), 1, 1001, 10, None);
        let second = approve(&before, &ledger(vec![first.clone()]), 2, 1002, 10, None);
        let mut records = vec![first, second];
        records.push(Record::Moved(Moved::new(
            jid(3001),
            MovedOf::new(file_space(), name("t/old.rs"), name("t/new.rs")).unwrap(),
            by(),
            JudgedAt::new(20),
            NonBlank::new("the tests moved").unwrap(),
        )));
        let records = ledger(records);
        // Its link names another file now; the approval's pin is read as the new name.
        let now = shelf_with(
            vec![fr(1, &[]), sp_testing(2, &["FR-1"], &["t/a.rs"])],
            &[("t/new.rs", hash)],
        );
        let footing = Footing::of(&now, docid(2), &records);
        assert_eq!(
            footing.artifacts(&now, &records),
            BTreeSet::from([file_artifact("t/a.rs"), file_artifact("t/new.rs")])
        );
        // The documents it stands on are read without them.
        assert_eq!(footing.docs(), BTreeSet::from([docid(1)]));
    }
}
