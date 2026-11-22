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

#![deny(clippy::wildcard_enum_match_arm)]

use crate::domain::nonempty::NonEmptyVec;

use super::super::build::{DocFailure, Shelf};
use super::super::doc::{DependsNow, Doc, DocHash, PinnedNow};
use super::super::drift::{ArtifactDrift, DocDrift, Edit, doc_drift, edit_of};
use super::{ApprovalView, FramePin, JudgementId, ReadPin, StandingApproval};

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
