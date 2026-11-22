//! What an approval saw, as a set, and whether its content can be read back.
//!
//! An approval pins hashes, and freshness compares them; that says *that* something
//! changed, not *what*. To say what, a person needs the content as it was approved:
//! the approved document, the upstreams it stood on and the artifacts it pinned. The
//! model keeps no copy of any content. A record keeps, beside each hash, a
//! [`ContentAddress`] where the content may be read back (rule A5), and this module
//! is the one place that says what an approval saw and whether its content came
//! back (rule A6).
//!
//! ```text
//!   Ledger::declared (StandingApproval) ──► Snapshot::of ──► Snapshot
//!                                                    │  items: the subject, each upstream pin,
//!                                                    │         each artifact pin, with its
//!                                                    │         hash and address (if any)
//!   content the store gave back, by address ─────────┴──► SnapshotItem::retrieval ──► Retrieval
//!   the shelf now / what an adapter read now ────────────► SnapshotItem::compared ──► ItemCompared
//! ```
//!
//! An artifact is shown as it was pinned, and as the pin it is read as now
//! ([`ItemKind::Artifact`], a [`ReadPin`]): when people declared it moved since, what
//! it is now is asked of the name it is read as ([`ReadPin::read_as`]), as freshness
//! asks it.
//!
//! Reading content back is a port of the application, so the domain is handed what
//! the store gave and decides only what that comes to. A [`Retrieval`] is one of
//! three answers, and a content that could not be read always says why: there was
//! no address, or there was one and nothing could be read through it. Whether an item
//! changed is the content as it was approved, compared the way it was pinned, whether
//! or not stale compares it now (rule A4: an address never counts); a declaration
//! keeps the meaning for stale, not here.

use std::collections::{BTreeMap, BTreeSet};
use std::convert::Infallible;
use std::fmt;

use crate::domain::actor::{Actor, Attribution};
use crate::domain::id::DocId;
use crate::domain::location::RelPath;

use super::artifact::{
    ArtifactHash, ArtifactKey, ArtifactReadings, Comparison, ContentAddress, Revision,
};
use super::build::{FailureTag, Shelf};
use super::doc::DocHash;
use super::drift::{ArtifactChange, DocDrift, Edit, doc_drift};
use super::ledger::{DeclaredNow, FramePin, JudgedAt, JudgementId, Ledger, ReadPin};

/// Which document of an approval an item is, with what the approval saw of it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DocRole {
    /// The approved document.
    Subject {
        /// Its identity.
        id: DocId,
        /// The hash approved.
        seen: DocHash,
        /// Where its content, as it was approved, can be read back.
        address: Option<ContentAddress>,
    },
    /// A document the approval stood on, as it was pinned (the link, its identity, its
    /// key, hash and address then).
    Upstream(FramePin),
}

impl DocRole {
    /// The document's identity.
    pub fn id(&self) -> DocId {
        match self {
            Self::Subject { id, .. } => *id,
            Self::Upstream(pin) => pin.target(),
        }
    }

    /// The hash approved.
    pub fn seen(&self) -> &DocHash {
        match self {
            Self::Subject { seen, .. } => seen,
            Self::Upstream(pin) => pin.hash(),
        }
    }

    /// Where its content, as it was approved, can be read back.
    pub fn address(&self) -> Option<&ContentAddress> {
        match self {
            Self::Subject { address, .. } => address.as_ref(),
            Self::Upstream(pin) => pin.address(),
        }
    }
}

/// What one item of a snapshot is, with what the approval saw of it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ItemKind {
    /// A document: which one of the approval, with the hash approved and where its
    /// content can be read back.
    Doc(DocRole),
    /// An artifact, as it was pinned and as it is read now
    /// ([`ApprovalView::read_pins`](super::ledger::ApprovalView::read_pins)).
    Artifact(ReadPin),
}

/// One item of what an approval saw.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SnapshotItem {
    kind: ItemKind,
}

/// Whether the content of an item, as it was approved, could be read back (rule
/// A6): read, or why not.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Retrieval {
    /// The content, read back through its address.
    Read(Vec<u8>),
    /// There is an address, and nothing could be read through it: the content was
    /// never kept where the address points, or is no longer there.
    AddressNotReadable(ContentAddress),
    /// The record keeps no address: it was made outside version control, by a
    /// version that kept none, the adapter could name none, or the item was not
    /// there.
    NoAddress,
}

/// What an artifact item of a snapshot asks of its space's adapter to be compared
/// now: the name it is read as, under the comparison it was pinned with.
pub(crate) struct ItemAsk<'i> {
    /// The name it is read as.
    pub(crate) artifact: &'i ArtifactKey,
    /// The comparison it was pinned with.
    pub(crate) comparison: &'i Comparison,
}

impl SnapshotItem {
    /// What the item is, with what the approval saw of it.
    pub fn kind(&self) -> &ItemKind {
        &self.kind
    }

    /// Where its content, as it was approved, can be read back.
    pub fn address(&self) -> Option<&ContentAddress> {
        match &self.kind {
            ItemKind::Doc(role) => role.address(),
            ItemKind::Artifact(read) => read.pin().seen().address(),
        }
    }

    /// What reading it back came to, given the contents the store gave back by
    /// address (an address that is not in `readable` could not be read): the one
    /// place a [`Retrieval`] is decided.
    pub fn retrieval(&self, readable: &BTreeMap<ContentAddress, Vec<u8>>) -> Retrieval {
        match self.address() {
            None => Retrieval::NoAddress,
            Some(address) => match readable.get(address) {
                Some(bytes) => Retrieval::Read(bytes.clone()),
                None => Retrieval::AddressNotReadable(address.clone()),
            },
        }
    }

    /// What an adapter is asked of it to compare it now: for an artifact, the name it
    /// is read as, under the comparison it was pinned with; nothing for a document,
    /// which the shelf holds.
    pub(crate) fn ask(&self) -> Option<ItemAsk<'_>> {
        match &self.kind {
            ItemKind::Doc(_) => None,
            ItemKind::Artifact(read) => Some(ItemAsk {
                artifact: read.read_as(),
                comparison: read.pin().comparison(),
            }),
        }
    }

    /// The item beside what it is now: a document in `shelf`, an artifact in what its
    /// adapter read (`readings`) by the name it is read as, its hash made by the
    /// comparison it was pinned with. The content as it was approved, compared the way
    /// it was pinned, whether or not stale compares it now: no declaration is asked,
    /// so an edit declared to keep the meaning is an edit here, which is what a diff
    /// shows (stale tells it as a notice).
    pub fn compared(&self, shelf: &Shelf, readings: &ArtifactReadings) -> ItemCompared {
        let now = match &self.kind {
            ItemKind::Doc(role) => {
                let DocNow { now, file } = doc_now(shelf, role.id());
                Now::Doc {
                    file,
                    drift: doc_drift(*role.seen(), now, |_, _| None),
                }
            }
            ItemKind::Artifact(read) => {
                let state = readings.state_of(read.read_as(), read.pin().comparison());
                Now::Artifact {
                    drift: ArtifactChange::of(read.pin().seen().state(), state.told(), |_, _| None),
                }
            }
        };
        ItemCompared {
            item: self.clone(),
            now,
        }
    }
}

/// An item of a snapshot beside what it is now ([`SnapshotItem::compared`], its only
/// maker): what its content came to, from which its hash now and whether it changed are
/// read.
///
/// ```compile_fail
/// use whyshelf_core::domain::shelf::artifact::ArtifactReadings;
/// use whyshelf_core::domain::shelf::build::Shelf;
/// use whyshelf_core::domain::shelf::snapshot::{ItemCompared, SnapshotItem};
///
/// // Its parts are its own: no one else makes one.
/// fn made(item: SnapshotItem, shelf: &Shelf, readings: &ArtifactReadings) -> ItemCompared {
///     ItemCompared { item, now: todo!() }
/// }
/// ```
///
/// The same, made by the item, builds:
///
/// ```
/// use whyshelf_core::domain::shelf::artifact::ArtifactReadings;
/// use whyshelf_core::domain::shelf::build::Shelf;
/// use whyshelf_core::domain::shelf::snapshot::{ItemCompared, SnapshotItem};
///
/// fn made(item: SnapshotItem, shelf: &Shelf, readings: &ArtifactReadings) -> ItemCompared {
///     item.compared(shelf, readings)
/// }
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ItemCompared {
    item: SnapshotItem,
    now: Now,
}

/// What an item is now, by the kind of its content.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Now {
    Doc {
        file: Option<RelPath>,
        drift: DocDrift<Infallible, FailureTag>,
    },
    Artifact {
        drift: ArtifactChange<Infallible>,
    },
}

/// What an item came to now, by the kind of its content ([`ItemCompared::view`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ItemView<'c> {
    /// A document: the file that carries it now, if any, and what it came to (in which
    /// way it is not valid, when it is not; a document that cannot be told is not read
    /// as one that is gone).
    Doc {
        /// The file that carries it now, if any (a valid document's file, or the first
        /// invalid file with its identity).
        file: Option<&'c RelPath>,
        /// What it came to.
        drift: &'c DocDrift<Infallible, FailureTag>,
    },
    /// An artifact: what it came to by the name it is read as.
    Artifact {
        /// What it came to.
        drift: &'c ArtifactChange<Infallible>,
    },
}

/// The hash an item has now.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HashNow {
    /// A document's.
    Doc(DocHash),
    /// An artifact's, by the comparison it was pinned with.
    Artifact(ArtifactHash),
}

impl fmt::Display for HashNow {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Doc(hash) => fmt::Display::fmt(hash, f),
            Self::Artifact(hash) => fmt::Display::fmt(hash, f),
        }
    }
}

impl ItemCompared {
    /// The item as it was approved.
    pub fn item(&self) -> &SnapshotItem {
        &self.item
    }

    /// What it came to now.
    pub fn view(&self) -> ItemView<'_> {
        match &self.now {
            Now::Doc { file, drift } => ItemView::Doc {
                file: file.as_ref(),
                drift,
            },
            Now::Artifact { drift } => ItemView::Artifact { drift },
        }
    }

    /// Its hash now; none for a document that is no valid document now, and for an
    /// artifact that is not there or cannot be told now.
    pub fn now_hash(&self) -> Option<HashNow> {
        match &self.now {
            Now::Doc { drift, .. } => match drift {
                Ok(edit) => Some(HashNow::Doc(edit.now())),
                Err(_) => None,
            },
            Now::Artifact { drift } => match drift {
                ArtifactChange::Content(edit) => Some(HashNow::Artifact(edit.now())),
                ArtifactChange::Arrived { now } => Some(HashNow::Artifact(*now)),
                ArtifactChange::Gone { .. }
                | ArtifactChange::StillAbsent
                | ArtifactChange::Untold(_) => None,
            },
        }
    }

    /// The name an artifact item is held by now: the name it is read as, when it is
    /// there now ([`ItemCompared::now_hash`] is then its hash); none for a document, and
    /// for an artifact that is gone, still absent or not told.
    pub fn artifact_now(&self) -> Option<&ArtifactKey> {
        let ItemKind::Artifact(read) = &self.item.kind else {
            return None;
        };
        let Now::Artifact { drift } = &self.now else {
            return None;
        };
        match drift {
            ArtifactChange::Content(_) | ArtifactChange::Arrived { .. } => Some(read.read_as()),
            ArtifactChange::Gone { .. }
            | ArtifactChange::StillAbsent
            | ArtifactChange::Untold(_) => None,
        }
    }

    /// Whether it is not known to be what was approved: its hash differs, it is a
    /// document that is no valid document now, it is an artifact gone or come, or one
    /// its adapter cannot tell about now (stale tells that as a notice). An artifact not
    /// there then and not there now has not changed.
    pub fn changed(&self) -> bool {
        match &self.now {
            Now::Doc { drift, .. } => match drift {
                Ok(edit) => edited(edit),
                Err(_) => true,
            },
            Now::Artifact { drift } => match drift {
                ArtifactChange::Content(edit) => edited(edit),
                ArtifactChange::StillAbsent => false,
                ArtifactChange::Gone { .. }
                | ArtifactChange::Arrived { .. }
                | ArtifactChange::Untold(_) => true,
            },
        }
    }
}

/// Whether an edit asked of no declaration is one.
fn edited<H>(edit: &Edit<H, Infallible>) -> bool {
    match edit {
        Edit::Same { .. } => false,
        Edit::Edited { .. } => true,
        Edit::KeptMeaning { declared, .. } => match *declared {},
    }
}

/// What an approval that stands saw: the approved document, the upstreams and the
/// artifacts it pinned, each with its hash and address, and the revision it was
/// made at.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Snapshot {
    judgement: JudgementId,
    subject: DocId,
    by: Attribution,
    at: JudgedAt,
    revision: Option<Revision>,
    items: Vec<SnapshotItem>,
}

impl Snapshot {
    /// The snapshot of the approval that stands about `id` ([`Ledger::declared`]),
    /// read from its view; none when the decision that stands is no approval, or there
    /// is none.
    pub fn of(ledger: &Ledger, id: DocId) -> Option<Self> {
        let DeclaredNow::Approved(standing) = ledger.declared(id) else {
            return None;
        };
        let view = standing.view();
        let mut items = vec![SnapshotItem {
            kind: ItemKind::Doc(DocRole::Subject {
                id,
                seen: *view.seen(),
                address: view.seen_address().cloned(),
            }),
        }];
        items.extend(view.frame().iter().map(|pin| SnapshotItem {
            kind: ItemKind::Doc(DocRole::Upstream(pin.clone())),
        }));
        // The pins as the ledger reads them: each as it was made, with the move it is
        // read through, if any.
        items.extend(view.read_pins().into_iter().map(|read| SnapshotItem {
            kind: ItemKind::Artifact(read),
        }));
        Some(Self {
            judgement: view.judgement(),
            subject: id,
            by: view.attribution(),
            at: view.at(),
            revision: view.revision().cloned(),
            items,
        })
    }

    /// The approval.
    pub fn judgement(&self) -> JudgementId {
        self.judgement
    }

    /// The approved document.
    pub fn subject(&self) -> DocId {
        self.subject
    }

    /// Who approved.
    pub fn actor(&self) -> &Actor {
        self.by.actor()
    }

    /// Who approved, and who ran the command when it was another.
    pub fn attribution(&self) -> &Attribution {
        &self.by
    }

    /// When.
    pub fn at(&self) -> JudgedAt {
        self.at
    }

    /// The point of the history it was made at, when the record keeps one.
    pub fn revision(&self) -> Option<&Revision> {
        self.revision.as_ref()
    }

    /// The items: the approved document first, then each upstream, then each
    /// artifact, in the order of the record.
    pub fn items(&self) -> &[SnapshotItem] {
        &self.items
    }

    /// Every address the items keep: what a store is asked to read back.
    pub fn addresses(&self) -> BTreeSet<ContentAddress> {
        self.items
            .iter()
            .filter_map(|item| item.address().cloned())
            .collect()
    }
}

/// What a document of a snapshot is now: its hash when it is valid, or the tag of
/// how it is not, and the file it is in when one carries it (a valid document's
/// file, or the first invalid file with its identity).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct DocNow {
    /// Its hash now when it is valid, else in which way it is not.
    pub(super) now: Result<DocHash, FailureTag>,
    /// The file that carries it now, if any.
    pub(super) file: Option<RelPath>,
}

/// What a document is now in the shelf: the one place a snapshot asks the shelf.
pub(super) fn doc_now(shelf: &Shelf, id: DocId) -> DocNow {
    let state = shelf.state_of(id);
    DocNow {
        // The tag of a document that is not valid is `DocFailure::tag`, the one place
        // that makes it; a diff reads a few items, so it is not a hot path.
        now: state
            .valid_or_failure()
            .map(|doc| *doc.hash())
            .map_err(|failure| failure.tag()),
        file: state.files().first().map(|file| (*file).clone()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::shelf::artifact::{ArtifactReading, ArtifactSeen, Untold};
    use crate::domain::shelf::fixtures_tests::{
        approve, docid, file_space, fr, fr_titled, ledger, name, shelf, shelf_with, sp_testing,
    };
    use crate::domain::shelf::ledger::{Decision, Record, frame_approve};

    fn address(text: &str) -> ContentAddress {
        ContentAddress::from_validated(text.to_owned()).unwrap()
    }

    #[test]
    fn a_document_with_no_standing_approval_has_no_snapshot() {
        let s = shelf(vec![fr(1, &[])]);
        assert_eq!(Snapshot::of(&ledger(vec![]), docid(1)), None);
        let denied = ledger(vec![crate::domain::shelf::fixtures_tests::deny(
            &s, 1, 1001, 10,
        )]);
        assert_eq!(Snapshot::of(&denied, docid(1)), None);
    }

    /// An approval of SP-2, standing on FR-1 and on two tests, a.rs there and b.rs
    /// absent, with addresses for each file and a revision.
    fn addressed() -> (Ledger, Revision) {
        let hash = ArtifactHash::of_content(b"test");
        let s = shelf_with(
            vec![
                fr(1, &[]),
                sp_testing(2, &["FR-1"], &["tests/a.rs", "tests/b.rs"]),
            ],
            &[("tests/a.rs", Some(hash))],
        );
        let frame = frame_approve(&s, &ledger(vec![]), docid(2)).unwrap();
        let files = frame.files();
        assert!(files.iter().any(|f| f.as_str() == "specs/sp-2.md"));
        assert!(files.iter().any(|f| f.as_str() == "findings/fr-1.md"));
        // Only the files of documents: an artifact's address is its adapter's answer.
        assert_eq!(files.len(), 2);

        let addresses: BTreeMap<RelPath, ContentAddress> = files
            .iter()
            .map(|f| (f.clone(), address(&format!("blob-{}", f.as_str().len()))))
            .collect();
        let mut artifacts = ArtifactReadings::new();
        for at in ["tests/a.rs", "tests/b.rs"] {
            let mut reading = ArtifactReading::of_bytes(b"test", &BTreeSet::new(), None);
            reading.set_address(address(&format!("blob-{at}")));
            artifacts.insert(file_space(), name(at), reading);
        }
        let revision = Revision::from_validated("c0ffee".to_owned()).unwrap();
        let frame = frame.addressed(&addresses, &artifacts, Some(revision.clone()));
        let record = Record::Decision(
            Decision::approve(
                crate::domain::shelf::fixtures_tests::jid(1001),
                frame,
                None,
                crate::domain::shelf::fixtures_tests::by(),
                JudgedAt::new(10),
                None,
            )
            .unwrap(),
        );
        (ledger(vec![record]), revision)
    }

    #[test]
    fn a_snapshot_lists_the_subject_the_upstreams_and_the_artifacts_with_their_addresses() {
        let (l, revision) = addressed();
        let snap = Snapshot::of(&l, docid(2)).unwrap();
        assert_eq!(snap.revision(), Some(&revision));
        assert_eq!(snap.items().len(), 4);
        assert!(matches!(
            snap.items()[0].kind(),
            ItemKind::Doc(DocRole::Subject { id, .. }) if *id == docid(2)
        ));
        assert!(matches!(
            snap.items()[1].kind(),
            ItemKind::Doc(DocRole::Upstream(pin)) if pin.target() == docid(1)
        ));
        let absent = snap
            .items()
            .iter()
            .find(|i| matches!(i.kind(), ItemKind::Artifact(read) if read.pin().artifact().name().as_str() == "tests/b.rs"))
            .unwrap();
        // Absent when pinned: no address, whatever the adapter answers now.
        assert_eq!(absent.address(), None);
        assert!(matches!(
            absent.kind(),
            ItemKind::Artifact(read) if read.pin().seen().state() == ArtifactSeen::Absent
        ));
        assert_eq!(snap.addresses().len(), 3);
    }

    #[test]
    fn a_snapshot_is_read_from_the_view_of_the_approval_that_stands() {
        let (l, _) = addressed();
        let snap = Snapshot::of(&l, docid(2)).unwrap();
        let DeclaredNow::Approved(standing) = l.declared(docid(2)) else {
            panic!("an approval stands");
        };
        let view = standing.view();
        assert_eq!(snap.judgement(), view.judgement());
        assert_eq!(snap.at(), view.at());
        assert_eq!(snap.attribution(), &view.attribution());
        assert_eq!(snap.revision(), view.revision());
        let ItemKind::Doc(role @ DocRole::Subject { .. }) = snap.items()[0].kind() else {
            panic!("the subject first");
        };
        assert_eq!(
            (role.seen(), role.address()),
            (view.seen(), view.seen_address())
        );
        let upstreams: Vec<&FramePin> = snap
            .items()
            .iter()
            .filter_map(|item| match item.kind() {
                ItemKind::Doc(DocRole::Upstream(pin)) => Some(pin),
                ItemKind::Doc(DocRole::Subject { .. }) | ItemKind::Artifact(_) => None,
            })
            .collect();
        assert_eq!(upstreams, view.frame().iter().collect::<Vec<_>>());
        let read: Vec<&ReadPin> = snap
            .items()
            .iter()
            .filter_map(|item| match item.kind() {
                ItemKind::Artifact(read) => Some(read),
                ItemKind::Doc(_) => None,
            })
            .collect();
        assert_eq!(read, view.read_pins().iter().collect::<Vec<_>>());
    }

    #[test]
    fn a_retrieval_is_read_or_says_why_not() {
        let s = shelf(vec![fr_titled(1, "a", &[])]);
        let l = ledger(vec![approve(&s, &ledger(vec![]), 1, 1001, 10, None)]);
        let snap = Snapshot::of(&l, docid(1)).unwrap();
        // The fixture's approval keeps no address.
        assert_eq!(
            snap.items()[0].retrieval(&BTreeMap::new()),
            Retrieval::NoAddress
        );

        let ItemKind::Doc(role) = snap.items()[0].kind() else {
            panic!("the subject first");
        };
        let item = SnapshotItem {
            kind: ItemKind::Doc(DocRole::Subject {
                id: docid(1),
                seen: *role.seen(),
                address: Some(address("abc")),
            }),
        };
        assert_eq!(
            item.retrieval(&BTreeMap::new()),
            Retrieval::AddressNotReadable(address("abc"))
        );
        let read = BTreeMap::from([(address("abc"), b"text".to_vec())]);
        assert_eq!(item.retrieval(&read), Retrieval::Read(b"text".to_vec()));
    }

    #[test]
    fn a_document_item_is_the_same_edited_or_not_valid_and_never_for_its_address() {
        let s = shelf(vec![fr_titled(1, "a", &[])]);
        let l = ledger(vec![approve(&s, &ledger(vec![]), 1, 1001, 10, None)]);
        let item = Snapshot::of(&l, docid(1)).unwrap().items()[0].clone();
        let none = ArtifactReadings::new();
        let then = *s.doc(docid(1)).unwrap().hash();

        let same = item.compared(&s, &none);
        assert!(!same.changed());
        assert_eq!(same.now_hash(), Some(HashNow::Doc(then)));
        assert!(matches!(
            same.view(),
            ItemView::Doc { file: Some(file), drift: Ok(Edit::Same { .. }) } if file.as_str() == "findings/fr-1.md"
        ));

        let edited = shelf(vec![fr_titled(1, "b", &[])]);
        let now = *edited.doc(docid(1)).unwrap().hash();
        let changed = item.compared(&edited, &none);
        assert!(changed.changed());
        assert_eq!(changed.now_hash(), Some(HashNow::Doc(now)));
        assert_eq!(changed.now_hash().unwrap().to_string(), now.to_string());

        let gone = item.compared(&shelf(vec![]), &none);
        assert!(gone.changed());
        assert_eq!(gone.now_hash(), None);
        assert_eq!(
            gone.view(),
            ItemView::Doc {
                file: None,
                drift: &Err(FailureTag::Gone)
            }
        );
        assert_eq!(gone.item(), &item);
    }

    #[test]
    fn an_artifact_item_is_the_same_edited_gone_or_not_told_by_its_comparison() {
        let hash = ArtifactHash::of_content(b"test");
        let s = shelf_with(
            vec![fr(1, &[]), sp_testing(2, &["FR-1"], &["tests/a.rs"])],
            &[("tests/a.rs", Some(hash))],
        );
        let l = ledger(vec![approve(&s, &ledger(vec![]), 2, 1001, 10, None)]);
        let snap = Snapshot::of(&l, docid(2)).unwrap();
        let item = snap
            .items()
            .iter()
            .find(|i| matches!(i.kind(), ItemKind::Artifact(_)))
            .unwrap();
        let ask = item.ask().unwrap();
        assert_eq!(ask.artifact.name().as_str(), "tests/a.rs");
        let reading = |bytes: Option<&[u8]>| {
            let mut readings = ArtifactReadings::new();
            let read = match bytes {
                Some(bytes) => ArtifactReading::of_bytes(bytes, &BTreeSet::new(), None),
                None => ArtifactReading::absent(),
            };
            readings.insert(file_space(), name("tests/a.rs"), read);
            readings
        };

        let same = item.compared(&s, &reading(Some(b"test")));
        assert!(!same.changed());
        assert_eq!(same.now_hash(), Some(HashNow::Artifact(hash)));
        let edited = item.compared(&s, &reading(Some(b"other")));
        assert!(edited.changed());
        assert_eq!(
            edited.now_hash(),
            Some(HashNow::Artifact(ArtifactHash::of_content(b"other")))
        );
        let gone = item.compared(&s, &reading(None));
        assert!(gone.changed());
        assert_eq!(gone.now_hash(), None);
        assert_eq!(
            gone.view(),
            ItemView::Artifact {
                drift: &ArtifactChange::Gone { then: hash }
            }
        );
        // Not asked: its adapter gave no answer, which is not read as absent.
        let untold = item.compared(&s, &ArtifactReadings::new());
        assert!(untold.changed());
        assert_eq!(untold.now_hash(), None);
        assert!(matches!(
            untold.view(),
            ItemView::Artifact {
                drift: ArtifactChange::Untold(Untold::NoAnswer)
            }
        ));
    }

    #[test]
    fn an_artifact_is_held_by_the_name_it_is_read_as_exactly_when_it_has_a_hash_now() {
        let hash = ArtifactHash::of_content(b"test");
        let s = shelf_with(
            vec![fr(1, &[]), sp_testing(2, &["FR-1"], &["tests/a.rs"])],
            &[("tests/a.rs", Some(hash))],
        );
        let l = ledger(vec![approve(&s, &ledger(vec![]), 2, 1001, 10, None)]);
        let snap = Snapshot::of(&l, docid(2)).unwrap();
        let item = snap
            .items()
            .iter()
            .find(|i| matches!(i.kind(), ItemKind::Artifact(_)))
            .unwrap();
        let read_as = item.ask().unwrap().artifact;
        let reading = |bytes: Option<&[u8]>| {
            let mut readings = ArtifactReadings::new();
            let read = match bytes {
                Some(bytes) => ArtifactReading::of_bytes(bytes, &BTreeSet::new(), None),
                None => ArtifactReading::absent(),
            };
            readings.insert(file_space(), name("tests/a.rs"), read);
            readings
        };

        // There now, as it was or edited: held by the name it is read as.
        for bytes in [&b"test"[..], &b"other"[..]] {
            let present = item.compared(&s, &reading(Some(bytes)));
            assert_eq!(present.artifact_now(), Some(read_as));
            assert!(present.now_hash().is_some());
        }
        // Gone, and not told: held by none, as with no hash now.
        let gone = item.compared(&s, &reading(None));
        assert_eq!(gone.artifact_now(), None);
        assert_eq!(gone.now_hash(), None);
        let untold = item.compared(&s, &ArtifactReadings::new());
        assert_eq!(untold.artifact_now(), None);
        assert_eq!(untold.now_hash(), None);

        // A document is held by no artifact name, whatever it is now.
        let doc = snap.items()[0].compared(&s, &ArtifactReadings::new());
        assert!(doc.now_hash().is_some());
        assert_eq!(doc.artifact_now(), None);
    }

    #[test]
    fn an_artifact_pinned_absent_has_not_changed_while_nothing_is_there_and_has_once_it_comes() {
        let (l, _) = addressed();
        let snap = Snapshot::of(&l, docid(2)).unwrap();
        let absent = snap
            .items()
            .iter()
            .find(|i| matches!(i.kind(), ItemKind::Artifact(read) if read.pin().seen().state() == ArtifactSeen::Absent))
            .unwrap();
        let s = shelf(vec![]);
        let mut nothing = ArtifactReadings::new();
        nothing.insert(file_space(), name("tests/b.rs"), ArtifactReading::absent());
        let still = absent.compared(&s, &nothing);
        assert!(!still.changed());
        assert_eq!(still.now_hash(), None);
        assert_eq!(
            still.view(),
            ItemView::Artifact {
                drift: &ArtifactChange::StillAbsent
            }
        );
        let mut there = ArtifactReadings::new();
        there.insert(
            file_space(),
            name("tests/b.rs"),
            ArtifactReading::of_bytes(b"x", &BTreeSet::new(), None),
        );
        let arrived = absent.compared(&s, &there);
        assert!(arrived.changed());
        assert_eq!(
            arrived.now_hash(),
            Some(HashNow::Artifact(ArtifactHash::of_content(b"x")))
        );
    }
}
