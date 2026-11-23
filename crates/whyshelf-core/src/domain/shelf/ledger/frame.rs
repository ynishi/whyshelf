//! The facts a decision stands on, which the shelf supplies: [`frame_approve`],
//! [`frame_present`] and [`frame_retire`] take an [`ApproveFrame`], a [`PresentFrame`]
//! or a [`RetireFrame`], or refuse with a [`FrameRefusal`]. Each frame also carries the
//! book the shelf tells its document is in now ([`Shelf::book_of`]), which the
//! decision names in its key `book`.

use std::collections::{BTreeMap, BTreeSet};

use thiserror::Error;

use crate::domain::id::DocId;
use crate::domain::location::RelPath;

use super::super::artifact::{
    ArtifactKey, ArtifactReading, ArtifactReadings, ArtifactSeen, ContentAddress, Revision, Untold,
};
use super::super::build::{Detail, DocFailure, Shelf};
use super::super::doc::{DocHash, DocKey, LinkState, UnresolvedLink};
use super::super::graph::{Cycle, LabelledCycle};
use super::super::schema::{ArtifactPropagation, BookName, EntryName, Propagation};
use super::{
    ArtifactPin, DeclaredNow, FramePin, Ledger, NameForm, NotStanding, PinnedArtifact, Seen,
};

impl Ledger {
    /// Whether a new approval may stand on `upstream`, by what the records say of it
    /// now: not when a decision against it stands, which is the answer
    /// ([`NotStanding`]), since an approval pinned to it would not hold from the start
    /// (a deprecated one is to be replaced: pin its successor). An upstream that is
    /// only pending, or approved and itself degraded, may be stood on: how it stands
    /// now can change without anyone deciding anything about what stands on it, so an
    /// approval on it is left to freshness, which tells it stale (or worse) from the
    /// start.
    ///
    /// The one place this refusal is decided. Whether a document counts, as the module
    /// `cover` says, is the same set and another rule: the two part on the day a
    /// deprecated document with successors may be stood on.
    pub fn may_stand_on(&self, upstream: DocId) -> Result<(), NotStanding> {
        match self.declared(upstream) {
            DeclaredNow::Against { status, .. } => Err(status),
            DeclaredNow::Pending | DeclaredNow::Approved(_) => Ok(()),
        }
    }
}

/// The facts an approval stands on, taken by [`frame_approve`].
///
/// It knows the file of the approved document and of each pinned upstream, so that
/// [`ApproveFrame::addressed`] can put the [`ContentAddress`] of each on the right
/// item; the files are not kept in the record (a file that moves keeps its
/// approval).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApproveFrame {
    pub(super) subject: DocId,
    pub(super) seen: DocHash,
    pub(super) frame: BTreeSet<FramePin>,
    pub(super) artifacts: BTreeSet<ArtifactPin>,
    pub(super) seen_address: Option<ContentAddress>,
    pub(super) revision: Option<Revision>,
    pub(super) book: Option<BookName>,
    subject_file: RelPath,
    upstream_files: BTreeMap<DocId, RelPath>,
}

impl ApproveFrame {
    /// The files of documents whose [`ContentAddress`] the record can keep: the
    /// approved document's and each pinned upstream's. (The address of a pinned
    /// artifact is the one its adapter answered with its hash.)
    pub fn files(&self) -> BTreeSet<RelPath> {
        let mut files = BTreeSet::from([self.subject_file.clone()]);
        files.extend(self.upstream_files.values().cloned());
        files
    }

    /// The frame with the addresses of the files of its documents, by path, the
    /// addresses of its artifacts, as `artifacts` answered them, and the revision it
    /// is taken at (rule A5): the one place an address is put on an item of an
    /// approval. An item with no address there keeps none; an artifact that is
    /// absent never has one. Nothing a freshness reads changes (A4).
    pub fn addressed(
        mut self,
        addresses: &BTreeMap<RelPath, ContentAddress>,
        artifacts: &ArtifactReadings,
        revision: Option<Revision>,
    ) -> Self {
        self.seen_address = addresses.get(&self.subject_file).cloned();
        self.frame = std::mem::take(&mut self.frame)
            .into_iter()
            .map(|mut pin| {
                pin.address = self
                    .upstream_files
                    .get(&pin.target)
                    .and_then(|file| addresses.get(file))
                    .cloned();
                pin
            })
            .collect();
        self.artifacts = std::mem::take(&mut self.artifacts)
            .into_iter()
            .map(|mut pin| {
                if let PinnedArtifact::Present { address, .. } = &mut pin.seen {
                    *address = artifacts
                        .of(&pin.artifact)
                        .and_then(ArtifactReading::address)
                        .cloned();
                }
                pin
            })
            .collect();
        self.revision = revision;
        self
    }

    /// The pins of artifacts the approval would stand on.
    pub fn artifacts(&self) -> &BTreeSet<ArtifactPin> {
        &self.artifacts
    }

    /// Whether [`frame_approve`] would refuse an approval of `id` for what the shelf
    /// and the declared statuses tell: every refusal of it but that the approval would
    /// close a cycle among the frames of approvals, which the frames standing when it
    /// is made decide. No frame is returned, so none leaves this module that was not
    /// asked about such a cycle. Why a run may be foreseen with it is
    /// [`stale_taken`](crate::domain::shelf::order::stale_taken)'s.
    ///
    /// A refusal added to the frame is told here when it reads only the shelf and the
    /// declared statuses; one that reads what approvals made since write goes with the
    /// cycle among the frames, which this does not tell.
    // The refusal is the answer and is asked once per document foreseen; it is not a
    // hot path.
    #[allow(clippy::result_large_err)]
    pub fn foresee(shelf: &Shelf, ledger: &Ledger, id: DocId) -> Result<(), FrameRefusal> {
        frame_on_shelf(shelf, ledger, id).map(|_| ())
    }
}

/// The facts a denial or a deprecation stands on, taken by [`frame_present`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PresentFrame {
    pub(super) subject: DocId,
    pub(super) seen: DocHash,
    pub(super) book: Option<BookName>,
}

/// The facts a retirement stands on, taken by [`frame_retire`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RetireFrame {
    pub(super) subject: DocId,
    pub(super) seen: Seen,
    pub(super) book: Option<BookName>,
}

/// Why a frame cannot be taken. It owns what it says, so it is a `'static` error.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum FrameRefusal {
    /// The document is not valid, so there is nothing sound to judge.
    #[error("{}", .0.said_of(&"the document", Detail::Full))]
    NotValid(DocFailure),
    /// A `Depends` link of the document cannot be followed, so an approval would
    /// stand on nothing.
    #[error("the link `{link}`: {unresolved}")]
    UnresolvedDepends {
        /// The link.
        link: EntryName,
        /// What was written and why it fails.
        unresolved: UnresolvedLink,
    },
    /// The document is in a cycle: of `Depends` links, or of the frames of
    /// approvals, which this approval would close. Which one the cycle says.
    #[error("the document is in {0}")]
    InCycle(LabelledCycle),
    /// A `Depends` link names a document people decided against, which a new
    /// approval may not stand on ([`Ledger::may_stand_on`]).
    #[error("the link `{link}` names {key}, which is {status}: an approval cannot stand on it")]
    UpstreamNotStanding {
        /// The link.
        link: EntryName,
        /// The document it names.
        target: DocId,
        /// Its key, by which the message names it.
        key: DocKey,
        /// What was decided about it.
        status: NotStanding,
    },
    /// A `Depends` link names an artifact its adapter cannot tell about now: an
    /// approval would stand on something nobody saw.
    #[error(
        "the link `{link}` names {artifact}, which cannot be told about now ({why}): an approval cannot stand on it"
    )]
    ArtifactUnknown {
        /// The link.
        link: EntryName,
        /// The artifact.
        artifact: ArtifactKey,
        /// Why it cannot be told.
        why: Untold,
    },
}

/// The frame of an approval of `id`: the facts it would stand on.
///
/// Refuses when the document is not valid, when a `Depends` link cannot be followed
/// (a link to a document that is not valid is one: the build leaves it unresolved),
/// when it names a document a new approval may not stand on
/// ([`Ledger::may_stand_on`]), when a `Depends` link names an artifact that cannot be
/// told about now, when the document is in a cycle of links, and when the approval
/// would close a cycle among the frames of approvals, in that order, the links in
/// the order the document holds them. A resolved link carries the key and hash of
/// what it names, and the pin is taken from it as it is. Which of these refusals can
/// be told before the approval is made is [`ApproveFrame::foresee`]'s.
// The refusal is the answer and is asked once per judgement; it is not a hot path.
#[allow(clippy::result_large_err)]
pub fn frame_approve(
    shelf: &Shelf,
    ledger: &Ledger,
    id: DocId,
) -> Result<ApproveFrame, FrameRefusal> {
    closing_no_cycle(frame_on_shelf(shelf, ledger, id)?, shelf, ledger)
}

/// The frame of an approval of `id`, or the first refusal of [`frame_approve`] that
/// [`ApproveFrame::foresee`] tells.
///
/// The frame it makes has not been asked whether it closes a cycle among the frames
/// of approvals, so it does not leave this module but through [`closing_no_cycle`].
// The refusal is the answer; see `frame_approve`.
#[allow(clippy::result_large_err)]
fn frame_on_shelf(shelf: &Shelf, ledger: &Ledger, id: DocId) -> Result<ApproveFrame, FrameRefusal> {
    let doc = shelf
        .state_of(id)
        .valid_or_failure()
        .map_err(FrameRefusal::NotValid)?;

    let mut pins = BTreeSet::new();
    let mut upstream_files = BTreeMap::new();
    for (link, states) in doc.links_of(Propagation::Depends) {
        for state in states {
            match state {
                LinkState::Unresolved(unresolved) => {
                    return Err(FrameRefusal::UnresolvedDepends {
                        link: link.clone(),
                        unresolved: unresolved.clone(),
                    });
                }
                LinkState::Resolved(upstream) => {
                    let target = upstream.id();
                    if let Err(status) = ledger.may_stand_on(target) {
                        return Err(FrameRefusal::UpstreamNotStanding {
                            link: link.clone(),
                            target,
                            key: upstream.key().clone(),
                            status,
                        });
                    }
                    if let Some(file) = shelf.doc(target).map(|d| d.origin().clone()) {
                        upstream_files.insert(target, file);
                    }
                    pins.insert(FramePin::new(
                        link.clone(),
                        target,
                        upstream.key().clone(),
                        *upstream.hash(),
                        None,
                    ));
                }
            }
        }
    }

    // Rule A2: every artifact a `Depends` link names is pinned as it is, there or
    // not. An artifact has no declared status; the one thing refused for it is that
    // its adapter cannot tell about it now, since the approval would stand on
    // something nobody saw. Each pin keeps the comparison its hash was made with,
    // the link's.
    let mut artifacts: BTreeSet<ArtifactPin> = BTreeSet::new();
    let depends = doc
        .artifact_links()
        .iter()
        .filter(|(_, link)| link.kind().propagation == ArtifactPropagation::Depends);
    for (name, link) in depends {
        let comparison = &link.kind().comparison;
        for artifact in link.artifacts() {
            let seen = match artifact.state().told() {
                Ok(ArtifactSeen::Present(hash)) => PinnedArtifact::Present {
                    hash,
                    address: None,
                },
                Ok(ArtifactSeen::Absent) => PinnedArtifact::Absent,
                Err(why) => {
                    return Err(FrameRefusal::ArtifactUnknown {
                        link: name.clone(),
                        artifact: artifact.key().clone(),
                        why: why.clone(),
                    });
                }
            };
            artifacts.insert(ArtifactPin {
                link: name.clone(),
                artifact: artifact.key().clone(),
                seen,
                comparison: comparison.clone(),
                form: NameForm::Spaced,
            });
        }
    }

    if let Some(cycle) = shelf.cycles().iter().find(|c| c.contains(id)) {
        return Err(in_cycle(shelf, cycle.clone()));
    }
    Ok(ApproveFrame {
        subject: id,
        seen: *doc.hash(),
        frame: pins,
        artifacts,
        seen_address: None,
        revision: None,
        book: shelf.book_to_record(id).cloned(),
        subject_file: doc.origin().clone(),
        upstream_files,
    })
}

/// `frame`, unless the approval would close a cycle among the frames of approvals
/// standing in `ledger`: the one refusal of [`frame_approve`] that
/// [`ApproveFrame::foresee`] does not tell.
// The refusal is the answer; see `frame_approve`.
#[allow(clippy::result_large_err)]
fn closing_no_cycle(
    frame: ApproveFrame,
    shelf: &Shelf,
    ledger: &Ledger,
) -> Result<ApproveFrame, FrameRefusal> {
    let targets: BTreeSet<DocId> = frame.frame.iter().map(FramePin::target).collect();
    match ledger.frame_graph().would_cycle(frame.subject, &targets) {
        Some(cycle) => Err(in_cycle(shelf, cycle)),
        None => Ok(frame),
    }
}

/// A cycle as a refusal, its members named by the keys the shelf tells.
fn in_cycle(shelf: &Shelf, cycle: Cycle) -> FrameRefusal {
    FrameRefusal::InCycle(LabelledCycle::new(cycle, |member| {
        shelf.doc(member).map(|doc| doc.key().clone())
    }))
}

/// The frame of a denial or a deprecation of `id`: the hash it would see.
///
/// Refuses when the document is not valid.
// The refusal is the answer and is asked once per judgement; it is not a hot path.
#[allow(clippy::result_large_err)]
pub fn frame_present(shelf: &Shelf, id: DocId) -> Result<PresentFrame, FrameRefusal> {
    let doc = shelf
        .state_of(id)
        .valid_or_failure()
        .map_err(FrameRefusal::NotValid)?;
    Ok(PresentFrame {
        subject: id,
        seen: *doc.hash(),
        book: shelf.book_to_record(id).cloned(),
    })
}

/// The frame of a retirement of `id`: what is seen of the document now, whatever it
/// is. It does not fail: a document can be retired after its file is gone.
pub fn frame_retire(shelf: &Shelf, id: DocId) -> RetireFrame {
    // The tag of a document that is not valid is `DocFailure::tag`, the one place
    // that makes it; a retirement is not a hot path.
    let seen = match shelf.state_of(id).valid_or_failure() {
        Ok(doc) => Seen::At(*doc.hash()),
        Err(failure) => Seen::Not(failure.tag()),
    };
    RetireFrame {
        subject: id,
        seen,
        book: shelf.book_to_record(id).cloned(),
    }
}
