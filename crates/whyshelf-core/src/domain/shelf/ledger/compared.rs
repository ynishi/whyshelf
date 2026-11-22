//! What the pins of the approval that stands come to now: [`ApprovalPins`], asked of
//! the approval that stands ([`StandingApproval::compared`]).
//!
//! Each pin is read by the kind of its content, once: the document itself and each
//! upstream by what a document comes to ([`DocDrift`]), each artifact by what an
//! artifact comes to under the comparison its link declares now ([`ArtifactDrift`]),
//! and an edit is joined by the declarations this ledger reads for that pin. What each
//! comes to is told in the words of its role by freshness; nothing here is a cause.
//!
//! A pin of a kind not written here is a new arm of each match, which the compiler
//! stops at: no arm of this file is a wildcard.
//!
//! What `judge same` offers to declare, and what `check` tells as short of names, read
//! the same comparison, of every approval that stands on one shelf ([`PinsNow`]), as
//! one list of every pin in the order of the pins ([`ComparedNow`]). Each pin is told
//! apart once ([`ApprovalPins::pins`]), whether stale reads it now is decided once (a
//! private `read_now` of a [`Compared`]), the pins of one document or one name are a
//! group of that list ([`DocGroup`], [`ArtifactGroup`]), and why nothing is offered is
//! decided by one refusal, in one order, to its last step:
//!
//! ```text
//!   PinsNow::of(ledger, shelf) ──► ComparedNow::of: every pin, in the order of the pins
//!     Content (Doc: Own | Upstream, Artifact) | Rest (Remade | Gone | Arrived |
//!     StillAbsent | Untold) | NotCompared (SubjectNotValid | UpstreamNotValid | NotWritten)
//!       ├─► doc(id) / artifact(&key): the group, its edits (open) and its refusal
//!       └─► thens: the pins stale reads now, with the hash each pinned
//! ```

#![deny(clippy::wildcard_enum_match_arm)]

use std::collections::{BTreeMap, BTreeSet};

use crate::domain::id::DocId;
use crate::domain::nonempty::NonEmptyVec;

use super::super::artifact::{ArtifactHash, ArtifactKey, Comparison, Untold};
use super::super::build::{DocFailure, Shelf};
use super::super::doc::{DependsNow, Doc, DocHash, PinnedNow};
use super::super::drift::{ArtifactChange, ArtifactDrift, DocDrift, Edit, doc_drift, edit_of};
use super::refusal::{NotComparedWhy, SameMeaningRefusal};
use super::{
    ApprovalView, FramePin, JudgementId, Ledger, PinnedArtifact, ReadPin, StandingApproval,
};

/// What the pins of the approval that stands come to now: nothing compared when its
/// document is no valid document now (why it is not), else each pin.
pub(in crate::domain::shelf) struct ApprovalPins<'l> {
    /// The approval.
    pub(in crate::domain::shelf) approval: ApprovalView<'l>,
    /// What its pins come to, when its document is valid.
    pub(in crate::domain::shelf) subject: Result<SubjectPins<'l>, DocFailure>,
}

/// What the pins of an approval whose document is valid come to now.
pub(in crate::domain::shelf) struct SubjectPins<'l> {
    /// The document.
    pub(in crate::domain::shelf) doc: &'l Doc,
    /// What its `Depends` links to artifacts write now.
    pub(in crate::domain::shelf) depends: DependsNow<'l>,
    /// Its own hash, then and now.
    pub(in crate::domain::shelf) seen: Edit<DocHash, NonEmptyVec<JudgementId>>,
    /// Each upstream, in the order of the record.
    pub(in crate::domain::shelf) frame:
        Vec<(&'l FramePin, DocDrift<NonEmptyVec<JudgementId>, DocFailure>)>,
    /// Each artifact as it is read now, in the order of the record.
    pub(in crate::domain::shelf) artifacts: Vec<(ReadPin, PinNow)>,
}

/// What an artifact pin comes to now.
pub(in crate::domain::shelf) enum PinNow {
    /// Its link writes the name it is read as: what that comes to.
    Written(ArtifactDrift<NonEmptyVec<JudgementId>>),
    /// Its link does not write it now.
    NotWritten,
}

impl<'l> StandingApproval<'l> {
    /// What its pins come to in `shelf` now ([`ApprovalPins`]): the one place the
    /// pins of the approval that stands are compared with what is there.
    pub(in crate::domain::shelf) fn compared(&self, shelf: &'l Shelf) -> ApprovalPins<'l> {
        let approval = self.view();
        let subject = match shelf.state_of(approval.subject()).valid_or_failure() {
            Ok(doc) => Ok(subject_pins(approval, shelf, doc)),
            Err(failure) => Err(failure),
        };
        ApprovalPins { approval, subject }
    }
}

/// What the pins of `approval`, whose document `doc` is valid, come to now.
fn subject_pins<'l>(approval: ApprovalView<'l>, shelf: &'l Shelf, doc: &'l Doc) -> SubjectPins<'l> {
    let ledger = approval.ledger;
    let id = approval.subject();
    let seen = edit_of(*approval.seen(), *doc.hash(), |a, b| {
        ledger.doc_same_meaning(id, a, b)
    });
    let frame = approval
        .frame()
        .iter()
        .map(|pin| (pin, upstream_now(approval, shelf, pin)))
        .collect();
    let depends = doc.depends_now();
    let artifacts = approval
        .read_pins()
        .into_iter()
        .map(|read| {
            let now = pin_now(
                approval,
                &read,
                depends.get(read.pin().link(), read.read_as()),
            );
            (read, now)
        })
        .collect();
    SubjectPins {
        doc,
        depends,
        seen,
        frame,
        artifacts,
    }
}

/// What the upstream `pin` names comes to now in `shelf`: why it is no valid document,
/// as it is, or its hash then and now.
// The failure is the answer, carried as it is to the cause that tells it, as
// `DocState::valid_or_failure` gives it; a failure is the rare case.
#[allow(clippy::result_large_err)]
fn upstream_now(
    approval: ApprovalView<'_>,
    shelf: &Shelf,
    pin: &FramePin,
) -> DocDrift<NonEmptyVec<JudgementId>, DocFailure> {
    let target = pin.target();
    doc_drift(
        *pin.hash(),
        shelf
            .state_of(target)
            .valid_or_failure()
            .map(|upstream| *upstream.hash()),
        |a, b| approval.ledger.doc_same_meaning(target, a, b),
    )
}

/// What the artifact pin `read` comes to, given what its link writing the name it is
/// read as comes to now (`written`), if it does.
fn pin_now(approval: ApprovalView<'_>, read: &ReadPin, written: Option<PinnedNow<'_>>) -> PinNow {
    match written {
        Some(now) => PinNow::Written(ArtifactDrift::of(
            read.pin().seen().state(),
            read.pin().comparison(),
            now.state().told(),
            now.comparison(),
            |a, b| approval.ledger.pin_same_meaning(read, a, b),
        )),
        None => PinNow::NotWritten,
    }
}

/// What the pins of every approval that stands come to on one shelf, held with that
/// shelf: a group reads whether a document is valid on the shelf its pins were compared
/// on, and no caller can hand it another.
pub(super) struct PinsNow<'l> {
    shelf: &'l Shelf,
    approvals: Vec<ApprovalPins<'l>>,
}

impl<'l> PinsNow<'l> {
    /// What the pins of every approval that stands in `ledger` come to on `shelf`, by
    /// the identity of their documents.
    pub(super) fn of(ledger: &'l Ledger, shelf: &'l Shelf) -> Self {
        Self {
            shelf,
            approvals: ledger
                .approvals_that_stand()
                .map(|standing| standing.compared(shelf))
                .collect(),
        }
    }
}

/// Every pin of every approval that stands, as [`ApprovalPins::pins`] tells it apart, in
/// the order of the pins: the approvals by the identity of their documents, each one's
/// own pin, then its upstreams, then its artifacts, in the order of the record.
pub(super) struct ComparedNow<'p> {
    shelf: &'p Shelf,
    pins: Vec<Compared<'p>>,
}

impl<'p> ComparedNow<'p> {
    /// Every pin of `pins`, each told apart once.
    pub(super) fn of(pins: &'p PinsNow<'_>) -> Self {
        Self {
            shelf: pins.shelf,
            pins: pins.approvals.iter().flat_map(ApprovalPins::pins).collect(),
        }
    }

    /// The pins of the document `id`, once it is valid on the shelf the pins were
    /// compared on: its own pin and every pin of an upstream that is `id`, of an
    /// approval whose document is valid or not; else why it is not valid.
    // The failure is the answer, carried as it is to the refusal that tells it, as
    // `DocState::valid_or_failure` gives it; a failure is the rare case.
    #[allow(clippy::result_large_err)]
    pub(super) fn doc(&self, id: DocId) -> Result<DocGroup<'_, 'p>, DocFailure> {
        self.shelf.state_of(id).valid_or_failure()?;
        Ok(DocGroup {
            pins: self.pins.iter().filter(|pin| pin.of_doc(id)).collect(),
        })
    }

    /// The pins read as the artifact `key`, whatever they come to.
    pub(super) fn artifact(&self, key: &ArtifactKey) -> ArtifactGroup<'_, 'p> {
        ArtifactGroup {
            pins: self
                .pins
                .iter()
                .filter(|pin| pin.of_artifact(key))
                .collect(),
        }
    }

    /// The pins stale reads now, each with the hash it pinned, in the order of the pins.
    pub(super) fn thens(&self) -> Vec<Then<'_>> {
        self.pins
            .iter()
            .filter_map(|pin| match pin.read_now() {
                ReadNow::Read(then) => Some(then),
                ReadNow::NotCompared { .. } | ReadNow::NoHash => None,
            })
            .collect()
    }
}

/// A pin of an approval that stands, told apart by what it comes to now: a content stale
/// compares, an artifact it does not compare as content, or a pin it does not compare
/// now at all.
pub(super) enum Compared<'a> {
    /// A content compared now: what it came to.
    Content(Content<'a>),
    /// An artifact under a link that still writes the name it is read as, whose content
    /// is not compared.
    Rest {
        /// The pin, as it is read now.
        read: &'a ReadPin,
        /// What it came to.
        rest: Rest<'a>,
    },
    /// A pin stale does not compare now.
    NotCompared(NotCompared<'a>),
}

/// A content compared now, with what it came to.
pub(super) enum Content<'a> {
    /// A document: the approved one or an upstream, as its role says.
    Doc {
        /// The document.
        id: DocId,
        /// Its role in the approval.
        role: DocPinRole,
        /// Its hash then and now.
        edit: &'a Edit<DocHash, NonEmptyVec<JudgementId>>,
    },
    /// An artifact there then and now, its hashes made by the comparison it was pinned
    /// with.
    Artifact {
        /// The pin, as it is read now.
        read: &'a ReadPin,
        /// Its hash then and now.
        edit: &'a Edit<ArtifactHash, NonEmptyVec<JudgementId>>,
    },
}

/// The role of a document's pin in an approval.
pub(super) enum DocPinRole {
    /// The approved document itself.
    Own,
    /// An upstream it stands on.
    Upstream,
}

/// What an artifact pin whose content is not compared came to.
pub(super) enum Rest<'a> {
    /// There then and now, under another comparison than the one it was pinned with.
    Remade,
    /// There then, nothing there now.
    Gone {
        /// The hash pinned.
        then: ArtifactHash,
    },
    /// Nothing there then, there now.
    Arrived,
    /// Nothing there then, nothing there now.
    StillAbsent,
    /// What is there cannot be told now.
    Untold(&'a Untold),
}

/// A pin stale does not compare now.
pub(super) enum NotCompared<'a> {
    /// A pin of an approval whose document is not valid now.
    SubjectNotValid {
        /// The pin.
        pin: PinRef<'a>,
    },
    /// An upstream that is not a valid document now.
    UpstreamNotValid(&'a FramePin),
    /// An artifact whose link no longer writes the name it is read as.
    NotWritten(&'a ReadPin),
}

/// A pin of an approval whose document is not valid now, as the view of the approval
/// reads it.
pub(super) enum PinRef<'a> {
    /// The hash of the approved document (its identity, and the hash it saw).
    Own(DocId, DocHash),
    /// An upstream.
    Upstream(&'a FramePin),
    /// An artifact, as it is read now: held as a value, since the view makes it each
    /// time it is asked, and boxed, as the rare case it is.
    Artifact(Box<ReadPin>),
}

/// A pin stale reads now, with the hash it pinned: the identity of its document, or the
/// pin of its artifact as it is read now.
pub(super) enum Then<'a> {
    /// A document, and the hash pinned.
    Doc(DocId, DocHash),
    /// An artifact, and the hash pinned.
    Artifact(&'a ReadPin, ArtifactHash),
}

impl Then<'_> {
    /// The hash pinned.
    fn hash(&self) -> PinnedHash {
        match self {
            Then::Doc(.., hash) => PinnedHash::Doc(*hash),
            Then::Artifact(.., hash) => PinnedHash::Artifact(*hash),
        }
    }
}

/// Whether stale reads a pin now: it does, with the hash pinned; it does not, with why
/// and the hash pinned when there is one; or it pinned no hash.
enum ReadNow<'a> {
    Read(Then<'a>),
    NotCompared {
        why: NotComparedWhy,
        then: Option<Then<'a>>,
    },
    NoHash,
}

/// A hash pinned, of either kind of content: read only by [`refusal`], which counts the
/// pins of a group by the hash they pinned.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum PinnedHash {
    Doc(DocHash),
    Artifact(ArtifactHash),
}

impl std::fmt::Display for PinnedHash {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Doc(hash) => hash.fmt(f),
            Self::Artifact(hash) => hash.fmt(f),
        }
    }
}

impl ApprovalPins<'_> {
    /// Each pin of the approval, told apart by what it comes to now: the one place a pin
    /// is told apart. Of an approval whose document is valid, its own pin and each
    /// upstream are contents, but an upstream that is not a valid document; each
    /// artifact is a content, the rest of what an artifact comes to, or not written.
    /// Of an approval whose document is not valid, every pin is not compared.
    pub(super) fn pins(&self) -> Vec<Compared<'_>> {
        match &self.subject {
            Ok(subject) => subject_compared(self.approval.subject(), subject),
            Err(..) => not_valid(self.approval),
        }
    }
}

/// The pins of the approval of `id`, whose document is valid, as they came to
/// (`subject`), in the order of the pins.
fn subject_compared<'a>(id: DocId, subject: &'a SubjectPins<'_>) -> Vec<Compared<'a>> {
    let own = Compared::Content(Content::Doc {
        id,
        role: DocPinRole::Own,
        edit: &subject.seen,
    });
    let upstreams = subject.frame.iter().map(|(pin, drift)| match drift {
        Ok(edit) => Compared::Content(Content::Doc {
            id: pin.target(),
            role: DocPinRole::Upstream,
            edit,
        }),
        Err(..) => Compared::NotCompared(NotCompared::UpstreamNotValid(pin)),
    });
    let artifacts = subject
        .artifacts
        .iter()
        .map(|(read, now)| artifact_compared(read, now));
    std::iter::once(own)
        .chain(upstreams)
        .chain(artifacts)
        .collect()
}

/// The artifact pin `read`, told apart by what it came to (`now`).
fn artifact_compared<'a>(read: &'a ReadPin, now: &'a PinNow) -> Compared<'a> {
    match now {
        PinNow::Written(ArtifactDrift::Change(ArtifactChange::Content(edit))) => {
            Compared::Content(Content::Artifact { read, edit })
        }
        PinNow::Written(ArtifactDrift::Change(ArtifactChange::Gone { then })) => Compared::Rest {
            read,
            rest: Rest::Gone { then: *then },
        },
        PinNow::Written(ArtifactDrift::Change(ArtifactChange::Arrived { .. })) => Compared::Rest {
            read,
            rest: Rest::Arrived,
        },
        PinNow::Written(ArtifactDrift::Change(ArtifactChange::StillAbsent)) => Compared::Rest {
            read,
            rest: Rest::StillAbsent,
        },
        PinNow::Written(ArtifactDrift::Change(ArtifactChange::Untold(why))) => Compared::Rest {
            read,
            rest: Rest::Untold(why),
        },
        PinNow::Written(ArtifactDrift::Remade { .. }) => Compared::Rest {
            read,
            rest: Rest::Remade,
        },
        PinNow::NotWritten => Compared::NotCompared(NotCompared::NotWritten(read)),
    }
}

/// Every pin of `approval`, whose document is not valid, as its view reads it: none is
/// compared.
fn not_valid(approval: ApprovalView<'_>) -> Vec<Compared<'_>> {
    std::iter::once(PinRef::Own(approval.subject(), *approval.seen()))
        .chain(approval.frame().iter().map(PinRef::Upstream))
        .chain(
            approval
                .read_pins()
                .into_iter()
                .map(|read| PinRef::Artifact(Box::new(read))),
        )
        .map(|pin| Compared::NotCompared(NotCompared::SubjectNotValid { pin }))
        .collect()
}

impl Compared<'_> {
    /// Whether it is a pin of the document `id`, valid on the shelf the pins were
    /// compared on.
    fn of_doc(&self, id: DocId) -> bool {
        match self {
            Compared::Content(Content::Doc { id: of, .. }) => *of == id,
            Compared::NotCompared(NotCompared::SubjectNotValid {
                pin: PinRef::Upstream(pin),
            }) => pin.target() == id,
            // The approval's own document is not valid on the shelf `id` is valid on,
            // so it is another document.
            Compared::NotCompared(NotCompared::SubjectNotValid {
                pin: PinRef::Own(..),
            }) => false,
            // An upstream not valid on the shelf `id` is valid on is another document.
            Compared::NotCompared(NotCompared::UpstreamNotValid(..)) => false,
            Compared::Content(Content::Artifact { .. })
            | Compared::Rest { .. }
            | Compared::NotCompared(
                NotCompared::NotWritten(..)
                | NotCompared::SubjectNotValid {
                    pin: PinRef::Artifact(..),
                },
            ) => false,
        }
    }

    /// Whether it is a pin read as the artifact `key`.
    fn of_artifact(&self, key: &ArtifactKey) -> bool {
        match self {
            Compared::Content(Content::Artifact { read, .. })
            | Compared::Rest { read, .. }
            | Compared::NotCompared(NotCompared::NotWritten(read)) => read.read_as() == key,
            Compared::NotCompared(NotCompared::SubjectNotValid {
                pin: PinRef::Artifact(read),
            }) => read.read_as() == key,
            Compared::Content(Content::Doc { .. })
            | Compared::NotCompared(
                NotCompared::UpstreamNotValid(..)
                | NotCompared::SubjectNotValid {
                    pin: PinRef::Own(..) | PinRef::Upstream(..),
                },
            ) => false,
        }
    }

    /// Whether stale reads it now: the one place that is decided. It reads a content, an
    /// artifact gone, or not told when it pinned a hash, and an upstream that is not
    /// valid (and says so); it does not compare a pin whose comparison changed, whose
    /// link no longer writes its name, or of an approval whose document is not valid;
    /// an artifact that arrived or is still absent pinned no hash.
    fn read_now(&self) -> ReadNow<'_> {
        match self {
            Compared::Content(Content::Doc { id, edit, .. }) => {
                ReadNow::Read(Then::Doc(*id, edit.then()))
            }
            Compared::Content(Content::Artifact { read, edit }) => {
                ReadNow::Read(Then::Artifact(read, edit.then()))
            }
            Compared::Rest {
                read,
                rest: Rest::Gone { then },
            } => ReadNow::Read(Then::Artifact(read, *then)),
            Compared::Rest {
                read,
                rest: Rest::Untold(..),
            } => match artifact_then(read) {
                Some(then) => ReadNow::Read(then),
                None => ReadNow::NoHash,
            },
            Compared::Rest {
                rest: Rest::Arrived | Rest::StillAbsent,
                ..
            } => ReadNow::NoHash,
            Compared::Rest {
                read,
                rest: Rest::Remade,
            } => ReadNow::NotCompared {
                why: NotComparedWhy::ComparisonChanged,
                then: artifact_then(read),
            },
            Compared::NotCompared(NotCompared::UpstreamNotValid(pin)) => {
                ReadNow::Read(Then::Doc(pin.target(), *pin.hash()))
            }
            Compared::NotCompared(NotCompared::NotWritten(read)) => ReadNow::NotCompared {
                why: NotComparedWhy::LinkDropped,
                then: artifact_then(read),
            },
            Compared::NotCompared(NotCompared::SubjectNotValid { pin }) => ReadNow::NotCompared {
                why: NotComparedWhy::DocumentNotValid,
                then: match pin {
                    PinRef::Own(id, seen) => Some(Then::Doc(*id, *seen)),
                    PinRef::Upstream(pin) => Some(Then::Doc(pin.target(), *pin.hash())),
                    PinRef::Artifact(read) => artifact_then(read),
                },
            },
        }
    }

    /// The hash it pinned, when it pinned one, read now or not.
    fn then_hash(&self) -> Option<PinnedHash> {
        match self.read_now() {
            ReadNow::Read(then) => Some(then.hash()),
            ReadNow::NotCompared { then, .. } => then.map(|then| then.hash()),
            ReadNow::NoHash => None,
        }
    }

    /// Why stale does not compare it now, when it does not.
    fn not_compared_why(&self) -> Option<NotComparedWhy> {
        match self.read_now() {
            ReadNow::NotCompared { why, .. } => Some(why),
            ReadNow::Read(..) | ReadNow::NoHash => None,
        }
    }

    /// Whether it is a content that holds: the same hash, or an edit kept in its
    /// meaning.
    fn holds(&self) -> bool {
        match self {
            Compared::Content(Content::Doc { edit, .. }) => kept(edit),
            Compared::Content(Content::Artifact { edit, .. }) => kept(edit),
            Compared::Rest { .. } | Compared::NotCompared(..) => false,
        }
    }
}

/// Whether `edit` holds: no edit, or one kept in its meaning.
fn kept<H, D>(edit: &Edit<H, D>) -> bool {
    match edit {
        Edit::Same { .. } | Edit::KeptMeaning { .. } => true,
        Edit::Edited { .. } => false,
    }
}

/// The artifact pin `read` with the hash it pinned, when it pinned one.
fn artifact_then(read: &ReadPin) -> Option<Then<'_>> {
    match read.pin().seen() {
        PinnedArtifact::Present { hash, .. } => Some(Then::Artifact(read, *hash)),
        PinnedArtifact::Absent => None,
    }
}

/// The pins of one document ([`ComparedNow::doc`]), in the order of the pins.
pub(super) struct DocGroup<'g, 'p> {
    pins: Vec<&'g Compared<'p>>,
}

/// The edits of a document's pins: the hash now, and every hash pinned that is edited.
pub(super) struct DocOpen {
    /// The hash now.
    pub(super) now: DocHash,
    /// The hashes pinned that are edited.
    pub(super) from: BTreeSet<DocHash>,
}

impl<'g, 'p> DocGroup<'g, 'p> {
    /// The pins.
    pub(super) fn pins(&self) -> &[&'g Compared<'p>] {
        &self.pins
    }

    /// The hash the document's own approval saw, when stale tells it as edited.
    pub(super) fn own_edited(&self) -> Option<DocHash> {
        self.pins.iter().find_map(|pin| match pin {
            Compared::Content(Content::Doc {
                role: DocPinRole::Own,
                edit,
                ..
            }) => match edit {
                Edit::Edited { then, .. } => Some(*then),
                Edit::Same { .. } | Edit::KeptMeaning { .. } => None,
            },
            Compared::Content(
                Content::Doc {
                    role: DocPinRole::Upstream,
                    ..
                }
                | Content::Artifact { .. },
            )
            | Compared::Rest { .. }
            | Compared::NotCompared(..) => None,
        })
    }

    /// The edits stale tells of its pins, when there is one.
    pub(super) fn open(&self) -> Option<DocOpen> {
        let mut open: Option<DocOpen> = None;
        for pin in &self.pins {
            match pin {
                Compared::Content(Content::Doc { edit, .. }) => match edit {
                    Edit::Edited { then, now } => {
                        open.get_or_insert_with(|| DocOpen {
                            now: *now,
                            from: BTreeSet::new(),
                        })
                        .from
                        .insert(*then);
                    }
                    Edit::Same { .. } | Edit::KeptMeaning { .. } => {}
                },
                Compared::Content(Content::Artifact { .. })
                | Compared::Rest { .. }
                | Compared::NotCompared(..) => {}
            }
        }
        open
    }

    /// Why nothing is offered of it, for the hash `named` or none ([`refusal`]).
    pub(super) fn refusal(&self, named: Option<DocHash>) -> SameMeaningRefusal {
        refusal(self.pins(), named.map(PinnedHash::Doc))
    }
}

/// The pins read as one artifact ([`ComparedNow::artifact`]), in the order of the pins.
pub(super) struct ArtifactGroup<'g, 'p> {
    pins: Vec<&'g Compared<'p>>,
}

/// The edits of an artifact's pins under one comparison: the hash now under it, and
/// every hash pinned that is edited, each with the pins that hold it.
pub(super) struct ArtifactOpen<'p> {
    /// The hash now.
    pub(super) now: ArtifactHash,
    /// The hashes pinned that are edited, each with the pins that hold it.
    pub(super) from: BTreeMap<ArtifactHash, NonEmptyVec<&'p ReadPin>>,
}

impl<'g, 'p> ArtifactGroup<'g, 'p> {
    /// The pins.
    pub(super) fn pins(&self) -> &[&'g Compared<'p>] {
        &self.pins
    }

    /// The comparisons of the pins stale tells as edited.
    pub(super) fn comparisons(&self) -> BTreeSet<&'p Comparison> {
        self.pins
            .iter()
            .filter_map(|pin| match pin {
                Compared::Content(Content::Artifact { read, edit }) => match edit {
                    Edit::Edited { .. } => {
                        let read: &'p ReadPin = read;
                        Some(read.pin().comparison())
                    }
                    Edit::Same { .. } | Edit::KeptMeaning { .. } => None,
                },
                Compared::Content(Content::Doc { .. })
                | Compared::Rest { .. }
                | Compared::NotCompared(..) => None,
            })
            .collect()
    }

    /// The edits stale tells of its pins made by the comparison `comparison`, when there
    /// is one.
    pub(super) fn open(&self, comparison: &Comparison) -> Option<ArtifactOpen<'p>> {
        let mut open: Option<ArtifactOpen<'p>> = None;
        for pin in &self.pins {
            match pin {
                Compared::Content(Content::Artifact { read, edit }) => match edit {
                    Edit::Edited { then, now } => {
                        let read: &'p ReadPin = read;
                        if read.pin().comparison() == comparison {
                            open.get_or_insert_with(|| ArtifactOpen {
                                now: *now,
                                from: BTreeMap::new(),
                            })
                            .from
                            .entry(*then)
                            .and_modify(|reads| reads.push(read))
                            .or_insert_with(|| NonEmptyVec::new(read, Vec::new()));
                        }
                    }
                    Edit::Same { .. } | Edit::KeptMeaning { .. } => {}
                },
                Compared::Content(Content::Doc { .. })
                | Compared::Rest { .. }
                | Compared::NotCompared(..) => {}
            }
        }
        open
    }

    /// Why nothing is offered of it, for the hash `named` or none ([`refusal`]).
    pub(super) fn refusal(&self, named: Option<ArtifactHash>) -> SameMeaningRefusal {
        refusal(self.pins(), named.map(PinnedHash::Artifact))
    }
}

/// Why nothing is offered of a group of `pins`, for the hash `named` or none: the one
/// place the refusals are decided, in one order, to the last step. Only the pins that
/// pinned a hash are read, and of those only the ones of the hash named, when one is.
/// In order: one that holds (at the hash now, or kept in its meaning) is not pinned as
/// changed when a hash was named, else nothing to declare; one not told now cannot be
/// told; one gone has nothing there; one stale does not compare now is refused with the
/// first such pin's reason; and with none of these, not pinned when a hash was named,
/// else nothing to declare.
fn refusal(pins: &[&Compared<'_>], named: Option<PinnedHash>) -> SameMeaningRefusal {
    let held: Vec<&Compared<'_>> = pins
        .iter()
        .copied()
        .filter(|pin| match (pin.then_hash(), named) {
            (Some(then), Some(named)) => then == named,
            (Some(..), None) => true,
            (None, Some(..) | None) => false,
        })
        .collect();
    if held.iter().any(|pin| pin.holds()) {
        return last_step(named);
    }
    held.iter()
        .find_map(|pin| match pin {
            Compared::Rest {
                read,
                rest: Rest::Untold(why),
            } => Some(SameMeaningRefusal::ArtifactUnknown {
                artifact: read.read_as().clone(),
                why: (*why).clone(),
            }),
            Compared::Rest {
                rest: Rest::Remade | Rest::Gone { .. } | Rest::Arrived | Rest::StillAbsent,
                ..
            }
            | Compared::Content(..)
            | Compared::NotCompared(..) => None,
        })
        .or_else(|| {
            held.iter().find_map(|pin| match pin {
                Compared::Rest {
                    read,
                    rest: Rest::Gone { .. },
                } => Some(SameMeaningRefusal::ArtifactAbsent {
                    artifact: read.read_as().clone(),
                }),
                Compared::Rest {
                    rest: Rest::Remade | Rest::Arrived | Rest::StillAbsent | Rest::Untold(..),
                    ..
                }
                | Compared::Content(..)
                | Compared::NotCompared(..) => None,
            })
        })
        .or_else(|| {
            held.iter()
                .find_map(|pin| pin.not_compared_why())
                .map(|why| SameMeaningRefusal::NotComparedNow {
                    hash: named.map(|hash| hash.to_string()),
                    why,
                })
        })
        .unwrap_or_else(|| last_step(named))
}

/// The last step of [`refusal`]: not pinned as changed when a hash was named, else
/// nothing to declare.
fn last_step(named: Option<PinnedHash>) -> SameMeaningRefusal {
    match named {
        Some(hash) => SameMeaningRefusal::NotPinned {
            hash: hash.to_string(),
        },
        None => SameMeaningRefusal::NothingToDeclare,
    }
}

#[cfg(test)]
mod tests;
