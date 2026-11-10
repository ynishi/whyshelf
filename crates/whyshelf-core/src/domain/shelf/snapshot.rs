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
//!   Ledger::standing_decision ──► Snapshot::of ──► Snapshot
//!                                                    │  items: the subject, each upstream pin,
//!                                                    │         each artifact pin, with its
//!                                                    │         hash and address (if any)
//!   content the store gave back, by address ─────────┴──► SnapshotItem::retrieval ──► Retrieval
//!   the shelf now / what an adapter read now ────────────► SnapshotItem::changed_from
//! ```
//!
//! Reading content back is a port of the application, so the domain is handed what
//! the store gave and decides only what that comes to. A [`Retrieval`] is one of
//! three answers, and a content that could not be read always says why: there was
//! no address, or there was one and nothing could be read through it. Whether an
//! item changed is the comparison of hashes freshness makes (rule A4); an address
//! never counts.

use std::collections::{BTreeMap, BTreeSet};

use crate::domain::actor::Actor;
use crate::domain::id::DocId;
use crate::domain::location::RelPath;

use super::artifact::{ArtifactState, ContentAddress, Revision};
use super::build::{DocState, Shelf};
use super::doc::{DocHash, DocKey};
use super::ledger::{DecisionVerb, JudgedAt, JudgementId, Ledger};
use super::schema::EntryName;

/// What one item of a snapshot is in the approval.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ItemRole {
    /// The approved document.
    Subject {
        /// Its identity.
        id: DocId,
    },
    /// A document the approval stood on.
    Upstream {
        /// The link it was pinned through.
        link: EntryName,
        /// Its identity.
        target: DocId,
        /// Its key when it was pinned.
        key: DocKey,
    },
    /// An artifact the approval stood on.
    Artifact {
        /// The link it was pinned through.
        link: EntryName,
        /// Its path.
        path: RelPath,
    },
}

/// What the approval saw of an item: the hash it pinned.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SeenHash {
    /// A document, with this hash.
    Doc(DocHash),
    /// An artifact, a file with this hash or none.
    Artifact(ArtifactState),
}

/// One item of what an approval saw.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SnapshotItem {
    role: ItemRole,
    seen: SeenHash,
    address: Option<ContentAddress>,
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
    /// version that kept none, or the item had no file.
    NoAddress,
}

/// What an item is now, for a comparison with what was approved.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ItemNow {
    /// A document: its hash now, when it is valid.
    Doc(Option<DocHash>),
    /// An artifact: a file with this hash, or none.
    Artifact(ArtifactState),
}

impl SnapshotItem {
    /// What the item is in the approval.
    pub fn role(&self) -> &ItemRole {
        &self.role
    }

    /// The hash the approval pinned.
    pub fn seen(&self) -> &SeenHash {
        &self.seen
    }

    /// Where its content, as it was approved, can be read back.
    pub fn address(&self) -> Option<&ContentAddress> {
        self.address.as_ref()
    }

    /// What reading it back came to, given the contents the store gave back by
    /// address (an address that is not in `readable` could not be read): the one
    /// place a [`Retrieval`] is decided.
    pub fn retrieval(&self, readable: &BTreeMap<ContentAddress, Vec<u8>>) -> Retrieval {
        match &self.address {
            None => Retrieval::NoAddress,
            Some(address) => match readable.get(address) {
                Some(bytes) => Retrieval::Read(bytes.clone()),
                None => Retrieval::AddressNotReadable(address.clone()),
            },
        }
    }

    /// Whether the item is not now what was approved: the hashes compared, as
    /// freshness compares them (rule A4). A document that is no valid document now
    /// has changed; an artifact with no file then and none now has not.
    pub fn changed_from(&self, now: &ItemNow) -> bool {
        match (&self.seen, now) {
            (SeenHash::Doc(then), ItemNow::Doc(now)) => now.as_ref() != Some(then),
            (SeenHash::Artifact(then), ItemNow::Artifact(now)) => then != now,
            (SeenHash::Doc(_), ItemNow::Artifact(_)) | (SeenHash::Artifact(_), ItemNow::Doc(_)) => {
                true
            }
        }
    }
}

/// What an approval that stands saw: the approved document, the upstreams and the
/// artifacts it pinned, each with its hash and address, and the revision it was
/// made at.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Snapshot {
    judgement: JudgementId,
    subject: DocId,
    actor: Actor,
    at: JudgedAt,
    revision: Option<Revision>,
    items: Vec<SnapshotItem>,
}

impl Snapshot {
    /// The snapshot of the approval that stands about `id`; none when the decision
    /// that stands is no approval, or there is none.
    pub fn of(ledger: &Ledger, id: DocId) -> Option<Self> {
        let decision = ledger.standing_decision(id)?;
        let DecisionVerb::Approve {
            seen,
            frame,
            artifacts,
            seen_address,
            revision,
            ..
        } = decision.verb()
        else {
            return None;
        };
        let mut items = vec![SnapshotItem {
            role: ItemRole::Subject { id },
            seen: SeenHash::Doc(*seen),
            address: seen_address.clone(),
        }];
        items.extend(frame.iter().map(|pin| SnapshotItem {
            role: ItemRole::Upstream {
                link: pin.link().clone(),
                target: pin.target(),
                key: pin.key().clone(),
            },
            seen: SeenHash::Doc(*pin.hash()),
            address: pin.address().cloned(),
        }));
        items.extend(artifacts.iter().map(|pin| SnapshotItem {
            role: ItemRole::Artifact {
                link: pin.link().clone(),
                path: pin.path().clone(),
            },
            seen: SeenHash::Artifact(pin.seen().state()),
            address: pin.seen().address().cloned(),
        }));
        Some(Self {
            judgement: decision.id(),
            subject: id,
            actor: decision.actor().clone(),
            at: decision.at(),
            revision: revision.clone(),
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
        &self.actor
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
            .filter_map(|item| item.address.clone())
            .collect()
    }
}

/// What a document of a snapshot is now: its hash when it is valid, and the file it
/// is in when one carries it (a valid document's file, or the first invalid file
/// with its identity).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DocNow {
    /// What it is now, for [`SnapshotItem::changed_from`].
    pub now: ItemNow,
    /// The file that carries it now, if any.
    pub file: Option<RelPath>,
}

/// What a document is now in the shelf: the one place a snapshot asks the shelf.
pub fn doc_now(shelf: &Shelf, id: DocId) -> DocNow {
    match shelf.state_of(id) {
        DocState::Valid(doc) => DocNow {
            now: ItemNow::Doc(Some(*doc.hash())),
            file: Some(doc.origin().clone()),
        },
        DocState::Invalid(files) => DocNow {
            now: ItemNow::Doc(None),
            file: Some(files.first().origin().clone()),
        },
        DocState::Gone | DocState::Unknown { .. } => DocNow {
            now: ItemNow::Doc(None),
            file: None,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::shelf::artifact::ArtifactHash;
    use crate::domain::shelf::fixtures_tests::{
        approve, docid, fr, fr_titled, ledger, shelf, shelf_with, sp_testing,
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

    #[test]
    fn a_snapshot_lists_the_subject_the_upstreams_and_the_artifacts_with_their_addresses() {
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
        assert!(files.iter().any(|f| f.as_str() == "tests/a.rs"));
        // No file, no address to ask for.
        assert!(!files.iter().any(|f| f.as_str() == "tests/b.rs"));

        let addresses: BTreeMap<RelPath, ContentAddress> = files
            .iter()
            .map(|f| (f.clone(), address(&format!("blob-{}", f.as_str().len()))))
            .collect();
        let revision = Revision::from_validated("c0ffee".to_owned()).unwrap();
        let frame = frame.addressed(&addresses, Some(revision.clone()));
        let record = Record::Decision(
            Decision::approve(
                crate::domain::shelf::fixtures_tests::jid(1001),
                frame,
                None,
                crate::domain::shelf::fixtures_tests::actor(),
                JudgedAt::new(10),
                None,
            )
            .unwrap(),
        );
        let l = ledger(vec![record]);
        let snap = Snapshot::of(&l, docid(2)).unwrap();
        assert_eq!(snap.revision(), Some(&revision));
        assert_eq!(snap.items().len(), 4);
        assert!(matches!(snap.items()[0].role(), ItemRole::Subject { id } if *id == docid(2)));
        assert!(
            matches!(snap.items()[1].role(), ItemRole::Upstream { target, .. } if *target == docid(1))
        );
        let absent = snap
            .items()
            .iter()
            .find(|i| matches!(i.role(), ItemRole::Artifact { path, .. } if path.as_str() == "tests/b.rs"))
            .unwrap();
        assert_eq!(absent.address(), None);
        assert_eq!(absent.seen(), &SeenHash::Artifact(ArtifactState::Absent));
        assert_eq!(snap.addresses().len(), 3);
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

        let item = SnapshotItem {
            role: ItemRole::Subject { id: docid(1) },
            seen: snap.items()[0].seen,
            address: Some(address("abc")),
        };
        assert_eq!(
            item.retrieval(&BTreeMap::new()),
            Retrieval::AddressNotReadable(address("abc"))
        );
        let read = BTreeMap::from([(address("abc"), b"text".to_vec())]);
        assert_eq!(item.retrieval(&read), Retrieval::Read(b"text".to_vec()));
    }

    #[test]
    fn an_item_has_changed_when_its_hash_has_and_never_for_its_address() {
        let s = shelf(vec![fr_titled(1, "a", &[])]);
        let l = ledger(vec![approve(&s, &ledger(vec![]), 1, 1001, 10, None)]);
        let item = Snapshot::of(&l, docid(1)).unwrap().items()[0].clone();
        assert!(!item.changed_from(&doc_now(&s, docid(1)).now));
        let edited = shelf(vec![fr_titled(1, "b", &[])]);
        assert!(item.changed_from(&doc_now(&edited, docid(1)).now));
        let gone = doc_now(&shelf(vec![]), docid(1));
        assert!(item.changed_from(&gone.now));
        assert_eq!(gone.file, None);

        let absent = SnapshotItem {
            role: ItemRole::Artifact {
                link: EntryName::new("tests").unwrap(),
                path: "t.rs".parse().unwrap(),
            },
            seen: SeenHash::Artifact(ArtifactState::Absent),
            address: None,
        };
        assert!(!absent.changed_from(&ItemNow::Artifact(ArtifactState::Absent)));
        assert!(
            absent.changed_from(&ItemNow::Artifact(ArtifactState::Present(
                ArtifactHash::of_content(b"x")
            )))
        );
    }
}
