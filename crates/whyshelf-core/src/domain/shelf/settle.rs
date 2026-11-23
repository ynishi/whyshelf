//! What can settle an origin of stale approvals: the things a person can say or do
//! about it, in the order a person would try them.
//!
//! [`origins_asked`], then [`Foreseen::of`], then [`settle_all`] is the one way: it
//! reads the [`OriginGroup`]s (where degradations start, what happened there and who
//! holds each cause there) and what the approving commands would approve, foreseen
//! once for the shelf ([`Foreseen`]: the approvals `judge approve --stale` would make
//! again, [`stale_taken`], and whether the frame takes each approval by name), and
//! returns the [`Settlement`]s of each origin. whyshelf does not guess what an edit means: it
//! lists what can be said and says none of it, and it runs none of it.
//!
//! # What settles what
//!
//! One row per [`OriginEvent`]; the rule is stated here and nowhere else. `h` is the
//! smallest identity among the approvals that hold a cause of an `Edited`,
//! `Arrived` or `Relinked` event of the origin themselves ([`OriginGroup::held`]),
//! whose `diff` shows what happened; `d` the document the origin is, and `k` the
//! artifact it is.
//!
//! | what happened | what can settle it | can approving again settle it (`ApproveStale`) |
//! |---|---|---|
//! | `Edited` | `ReadDiff(h)`, and `DeclareSame` of `d` (an origin that is a document) or of `k` (an artifact) | yes |
//! | `Arrived`, `Relinked` | `ReadDiff(h)` | yes |
//! | `Gone` | `FindMoves`, `DeclareMoved(k)` | no: it is broken, and `--stale` leaves a broken approval out |
//! | `Expired` | `ApproveUntil(d)` | no: `--stale` approves with no expiry unless told one, so a lapsed approval is approved again with `ApproveUntil` |
//! | `UpstreamPending` | `ApprovePending(d)`, the upstream | no: until the upstream is decided, approving again changes nothing and is left out as repeated |
//! | `UpstreamAwaiting` | `ApproveAsAnother(d)`, the upstream, by a name that has not approved it | no: until another name approves the upstream, `--stale` holds back what stands on it (`stands_on`, `upstream_is` `awaiting`) |
//! | `UpstreamSuperseded` | none: write the link to the successor, and the change of the link is a stale approval of its own | no: the frame refuses an approval on an upstream decided against, a deprecated one with successors included, so `--stale` leaves it out as refused |
//! | `LinkBehindMove` | none: write the link to the new name, and the change of the link is a stale approval of its own | no: as above, repeated |
//! | `UpstreamRefused`, `NotValid`, `Cycle` | none: mend the file, or look at the decision again, until there is something to say | no: it is broken |
//!
//! The third column is one private function over the events, so an event cannot be
//! added without saying it.
//!
//! # What the list says
//!
//! - **The order is read, then declare, then approve.** A person reads what changed
//!   before saying the meaning is kept, and says that before approving what is left.
//!   Each settlement is listed once, however many events or approvals lead to it.
//! - **`ApproveStale` is last, and only when it would take something of this
//!   origin.** The event can be settled by approving again (the third column), and at
//!   least one approval degraded by the origin is one `--stale` would make again
//!   ([`stale_taken`]; strengths are not read again here).
//! - **`ApproveStale` acts on the whole shelf.** `judge approve --stale` takes every
//!   approval that is expired or stale, not those of this origin, and approves them
//!   with no expiry: an approval that had one loses it. When an origin holds both an
//!   expiry and an edit, `ApproveUntil` and `ApproveStale` are both listed, and the
//!   second drops the expiry that the first keeps.
//! - **An approval by name is listed only when the frame takes it.** `ApproveUntil`,
//!   `ApprovePending` and `ApproveAsAnother` each approve one document by naming it,
//!   in a run of one (`judge approve <d>`), and each is listed only when
//!   [`Foreseen`] says the frame takes an approval of it ([`origins_asked`] says
//!   which documents it is asked about), since a command the frame refuses settles
//!   nothing (REQ-31); what it does not foresee is on [`Foreseen`]. Nothing is listed
//!   in its place, since no other command settles that event.
//! - **`DeclareSame` may be refused.** `judge same` refuses when approvals pinned
//!   several earlier hashes that do not mean the same and `--from` names none, and
//!   says which hashes there are. The settlement does not choose one: choosing the
//!   hash the edit is read from is a person's, and the refusal lists the candidates.
//! - **An origin that is the declaration** (a link that now declares another
//!   comparison) is no one document or artifact, so there is nothing to declare the
//!   meaning of: it has `ReadDiff` and, when the shelf takes them, `ApproveStale`.
//!   A cycle has no settlement.
//!
//! The words a settlement is typed as belong to the command line, not to this
//! module: a settlement is a fact of the model, and the command that carries it out
//! is its surface's.
//!
//! [`stale_taken`]: super::order::stale_taken
//! [`order::stale`]: super::order::stale

use crate::domain::id::DocId;

use super::artifact::ArtifactKey;
use super::freshness::{Origin, OriginEvent, OriginGroup};
use super::order::{Foreseen, OriginsAsked};

/// One thing a person can say or do that may settle an origin.
///
/// Why a type and not a command: what can settle an origin is a fact of the model,
/// and the words that say it (a command line, a menu) are the surface's. whyshelf
/// does not guess what an edit means: it lists what can be said and says none of it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Settlement {
    /// Read what changed, in the approval of this document (one that holds a cause
    /// of the origin, so that its `diff` shows the change).
    ReadDiff(DocId),
    /// Declare that the edit keeps the meaning: of this document, or of this
    /// artifact. A person may be asked to name the earlier hash it is read from.
    DeclareSame(SameSubject),
    /// Look for where the gone artifacts went.
    FindMoves,
    /// Declare that this artifact moved, to a new name a person gives.
    DeclareMoved(ArtifactKey),
    /// Approve the pending upstream this origin is.
    ApprovePending(DocId),
    /// Approve the upstream this origin is, by a name that has not approved it: its
    /// approval awaits the approvals of other names, and the same name approving it
    /// again would only repeat its reading, and write nothing.
    ApproveAsAnother(DocId),
    /// Approve this document again, with a new expiry.
    ApproveUntil(DocId),
    /// Approve again every approval of the shelf that an edit made stale (`judge
    /// approve --stale`): not only those of this origin, and with no expiry.
    ApproveStale,
}

/// What a declaration that an edit keeps the meaning is about.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SameSubject {
    /// A document.
    Doc(DocId),
    /// An artifact.
    Artifact(ArtifactKey),
}

/// The origins, with the documents their settlements approve by naming them, each in
/// a run of one: what is to be foreseen of them ([`Foreseen::of`]) before they are
/// settled ([`settle_all`]).
pub fn origins_asked(groups: Vec<OriginGroup>) -> OriginsAsked {
    let named = groups
        .iter()
        .flat_map(candidates)
        .filter_map(|settlement| by_name(&settlement))
        .collect();
    OriginsAsked::new(groups, named)
}

/// What can settle each origin `foreseen` was asked about, in the order a person
/// would try it, with what was foreseen of it. The one way to settle origins: the
/// origins settled are those foreseen; what is offered of a document it was not asked
/// about is [`Foreseen`]'s.
pub fn settle_all(foreseen: Foreseen) -> Vec<(OriginGroup, Vec<Settlement>)> {
    let next: Vec<Vec<Settlement>> = foreseen
        .groups()
        .iter()
        .map(|group| settlements(group, &foreseen))
        .collect();
    foreseen.into_groups().into_iter().zip(next).collect()
}

/// What can settle one origin, in the order a person would try it: read first,
/// then declare, then approve; each once.
///
/// `foreseen` is what the approving commands would approve ([`Foreseen`]). The rule
/// is the table and the list of the module page.
pub(super) fn settlements(group: &OriginGroup, foreseen: &Foreseen) -> Vec<Settlement> {
    let mut found = candidates(group);
    found.retain(|settlement| by_name(settlement).is_none_or(|doc| foreseen.frame_takes(doc)));
    let again_takes_one = group
        .degraded
        .iter()
        .any(|id| foreseen.again().contains(id));
    if again_takes_one && group.events().iter().any(|e| approving_again_settles(*e)) {
        found.push(Settlement::ApproveStale);
    }
    found.sort_by_key(rank);
    found.dedup();
    found
}

/// The settlements of the events of an origin, as the second column of the table
/// gives them, before anything foreseen is read.
fn candidates(group: &OriginGroup) -> Vec<Settlement> {
    // The approval whose `diff` shows the change: one that holds a cause of an event
    // a `diff` shows, never one that is degraded only through an upstream, nor one
    // that holds only another event here (a lapse of an approval made on the edit).
    let read = group
        .held
        .iter()
        .filter(|(event, _)| shown_by_diff(*event))
        .map(|(_, holder)| *holder)
        .min();
    group
        .events()
        .iter()
        .flat_map(|event| of_event(*event, &group.origin, read))
        .collect()
}

/// The document a settlement approves by naming it, in a run of one; none for one
/// that approves nothing, and for `ApproveStale`, which takes what the rule of
/// `--stale` takes.
fn by_name(settlement: &Settlement) -> Option<DocId> {
    match settlement {
        Settlement::ApprovePending(doc)
        | Settlement::ApproveAsAnother(doc)
        | Settlement::ApproveUntil(doc) => Some(*doc),
        Settlement::ReadDiff(_)
        | Settlement::DeclareSame(_)
        | Settlement::FindMoves
        | Settlement::DeclareMoved(_)
        | Settlement::ApproveStale => None,
    }
}

/// Whether a `diff` of an approval that holds a cause of the event shows it: the
/// events whose settlements start with `ReadDiff`.
fn shown_by_diff(event: OriginEvent) -> bool {
    match event {
        OriginEvent::Edited | OriginEvent::Arrived | OriginEvent::Relinked => true,
        OriginEvent::Gone
        | OriginEvent::Expired
        | OriginEvent::UpstreamPending
        | OriginEvent::UpstreamAwaiting
        | OriginEvent::UpstreamSuperseded
        | OriginEvent::LinkBehindMove
        | OriginEvent::UpstreamRefused
        | OriginEvent::NotValid
        | OriginEvent::Cycle => false,
    }
}

/// The settlements of one event at an origin: the second column of the table.
/// `read` is the approval whose `diff` shows it, for an event [`shown_by_diff`].
fn of_event(event: OriginEvent, origin: &Origin, read: Option<DocId>) -> Vec<Settlement> {
    match event {
        OriginEvent::Edited => {
            let mut out: Vec<Settlement> = read.map(Settlement::ReadDiff).into_iter().collect();
            out.extend(
                match origin {
                    Origin::Doc(doc) => Some(SameSubject::Doc(*doc)),
                    Origin::Artifact(artifact) => Some(SameSubject::Artifact(artifact.clone())),
                    // Neither one document nor one artifact: nothing to declare it about.
                    Origin::Declaration | Origin::Cycle(_) => None,
                }
                .map(Settlement::DeclareSame),
            );
            out
        }
        OriginEvent::Arrived | OriginEvent::Relinked => {
            read.map(Settlement::ReadDiff).into_iter().collect()
        }
        OriginEvent::Gone => match origin {
            Origin::Artifact(artifact) => vec![
                Settlement::FindMoves,
                Settlement::DeclareMoved(artifact.clone()),
            ],
            Origin::Doc(_) | Origin::Declaration | Origin::Cycle(_) => Vec::new(),
        },
        OriginEvent::Expired => match origin {
            Origin::Doc(doc) => vec![Settlement::ApproveUntil(*doc)],
            Origin::Artifact(_) | Origin::Declaration | Origin::Cycle(_) => Vec::new(),
        },
        OriginEvent::UpstreamPending => match origin {
            Origin::Doc(doc) => vec![Settlement::ApprovePending(*doc)],
            Origin::Artifact(_) | Origin::Declaration | Origin::Cycle(_) => Vec::new(),
        },
        OriginEvent::UpstreamAwaiting => match origin {
            Origin::Doc(doc) => vec![Settlement::ApproveAsAnother(*doc)],
            Origin::Artifact(_) | Origin::Declaration | Origin::Cycle(_) => Vec::new(),
        },
        OriginEvent::UpstreamSuperseded
        | OriginEvent::LinkBehindMove
        | OriginEvent::UpstreamRefused
        | OriginEvent::NotValid
        | OriginEvent::Cycle => Vec::new(),
    }
}

/// Whether approving again settles the event: the third column of the table.
///
/// An approval of an event that stays (an upstream not decided, a link that writes an
/// old name) is repeated by one made now, which `--stale` leaves out; one on an
/// upstream that was deprecated is refused by the frame, which `--stale` leaves out
/// with that refusal; a broken one is left out; an expired one is approved again by
/// `--stale` without its expiry, which is not what settles it.
fn approving_again_settles(event: OriginEvent) -> bool {
    match event {
        OriginEvent::Edited | OriginEvent::Arrived | OriginEvent::Relinked => true,
        OriginEvent::Gone
        | OriginEvent::Expired
        | OriginEvent::UpstreamPending
        | OriginEvent::UpstreamAwaiting
        | OriginEvent::UpstreamSuperseded
        | OriginEvent::LinkBehindMove
        | OriginEvent::UpstreamRefused
        | OriginEvent::NotValid
        | OriginEvent::Cycle => false,
    }
}

/// Where a settlement stands in the list: read, then declare, then approve.
fn rank(settlement: &Settlement) -> u8 {
    match settlement {
        Settlement::ReadDiff(_) => 0,
        Settlement::DeclareSame(_) => 1,
        Settlement::FindMoves => 2,
        Settlement::DeclareMoved(_) => 3,
        Settlement::ApprovePending(_) => 4,
        Settlement::ApproveAsAnother(_) => 5,
        Settlement::ApproveUntil(_) => 6,
        Settlement::ApproveStale => 7,
    }
}

#[cfg(test)]
mod tests {
    use std::collections::{BTreeMap, BTreeSet};

    use crate::domain::nonempty::NonEmptySet;
    use crate::domain::shelf::build::DocFailure;
    use crate::domain::shelf::fixtures_tests::{docid, file_artifact};
    use crate::domain::shelf::graph::{CycleId, CycleSource};
    use crate::domain::shelf::ledger::FrameRefusal;

    use super::*;

    /// Every event, written out so that a new one fails to compile here until the
    /// tests say what they expect of it.
    fn every_event() -> [OriginEvent; 12] {
        let all = [
            OriginEvent::Edited,
            OriginEvent::Arrived,
            OriginEvent::Gone,
            OriginEvent::Expired,
            OriginEvent::Relinked,
            OriginEvent::LinkBehindMove,
            OriginEvent::UpstreamPending,
            OriginEvent::UpstreamAwaiting,
            OriginEvent::UpstreamSuperseded,
            OriginEvent::UpstreamRefused,
            OriginEvent::NotValid,
            OriginEvent::Cycle,
        ];
        for event in all {
            match event {
                OriginEvent::Edited
                | OriginEvent::Arrived
                | OriginEvent::Gone
                | OriginEvent::Expired
                | OriginEvent::Relinked
                | OriginEvent::LinkBehindMove
                | OriginEvent::UpstreamPending
                | OriginEvent::UpstreamAwaiting
                | OriginEvent::UpstreamSuperseded
                | OriginEvent::UpstreamRefused
                | OriginEvent::NotValid
                | OriginEvent::Cycle => {}
            }
        }
        all
    }

    /// An origin where each of `holders` holds a cause of each of `events`.
    fn group(
        origin: Origin,
        degraded: &[u128],
        holders: &[u128],
        events: &[OriginEvent],
    ) -> OriginGroup {
        let mut pairs = events
            .iter()
            .flat_map(|event| holders.iter().map(|n| (*event, docid(*n))));
        OriginGroup {
            origin,
            degraded: degraded.iter().map(|n| docid(*n)).collect(),
            held: NonEmptySet::new(pairs.next().unwrap(), pairs),
        }
    }

    /// What is foreseen: the approvals `--stale` would make again, and what the frame
    /// says of an approval of each document asked about.
    fn foreseen(again: &[u128], framed: &[(u128, Result<(), FrameRefusal>)]) -> Foreseen {
        Foreseen::of_parts(
            again.iter().map(|n| docid(*n)).collect::<BTreeSet<_>>(),
            framed
                .iter()
                .map(|(n, said)| (docid(*n), said.clone()))
                .collect::<BTreeMap<_, _>>(),
        )
    }

    /// A refusal of the frame, as any refusal reads here.
    fn refusal() -> FrameRefusal {
        FrameRefusal::NotValid(DocFailure::Gone)
    }

    /// The settlements of an origin that is document 1, held by 1, with `--stale`
    /// taking it and the frame taking an approval of it.
    fn of_doc(events: &[OriginEvent]) -> Vec<Settlement> {
        settlements(
            &group(Origin::Doc(docid(1)), &[1], &[1], events),
            &foreseen(&[1], &[(1, Ok(()))]),
        )
    }

    fn of_file(events: &[OriginEvent]) -> Vec<Settlement> {
        settlements(
            &group(
                Origin::Artifact(file_artifact("tests/a.rs")),
                &[2],
                &[2],
                events,
            ),
            &foreseen(&[2], &[]),
        )
    }

    #[test]
    fn an_edited_document_is_read_declared_and_approved_again_in_that_order() {
        assert_eq!(
            of_doc(&[OriginEvent::Edited]),
            vec![
                Settlement::ReadDiff(docid(1)),
                Settlement::DeclareSame(SameSubject::Doc(docid(1))),
                Settlement::ApproveStale,
            ]
        );
    }

    #[test]
    fn an_edited_artifact_is_declared_about_by_its_name() {
        assert_eq!(
            of_file(&[OriginEvent::Edited]),
            vec![
                Settlement::ReadDiff(docid(2)),
                Settlement::DeclareSame(SameSubject::Artifact(file_artifact("tests/a.rs"))),
                Settlement::ApproveStale,
            ]
        );
    }

    #[test]
    fn an_artifact_that_arrived_or_a_link_that_changed_is_read_and_approved_again() {
        for event in [OriginEvent::Arrived, OriginEvent::Relinked] {
            assert_eq!(
                of_file(&[event]),
                vec![Settlement::ReadDiff(docid(2)), Settlement::ApproveStale],
                "{event:?}"
            );
        }
    }

    #[test]
    fn a_gone_artifact_is_looked_for_and_declared_moved_and_never_approved_again() {
        assert_eq!(
            of_file(&[OriginEvent::Gone]),
            vec![
                Settlement::FindMoves,
                Settlement::DeclareMoved(file_artifact("tests/a.rs")),
            ]
        );
    }

    #[test]
    fn a_lapsed_approval_is_approved_again_with_a_new_expiry_and_not_by_stale() {
        assert_eq!(
            of_doc(&[OriginEvent::Expired]),
            vec![Settlement::ApproveUntil(docid(1))]
        );
    }

    #[test]
    fn a_pending_upstream_is_approved_and_approving_again_would_change_nothing() {
        assert_eq!(
            of_doc(&[OriginEvent::UpstreamPending]),
            vec![Settlement::ApprovePending(docid(1))]
        );
    }

    #[test]
    fn an_upstream_that_awaits_is_approved_by_another_name_and_not_by_stale() {
        assert_eq!(
            of_doc(&[OriginEvent::UpstreamAwaiting]),
            vec![Settlement::ApproveAsAnother(docid(1))]
        );
    }

    #[test]
    fn what_has_no_command_has_no_settlement() {
        for event in [
            OriginEvent::UpstreamSuperseded,
            OriginEvent::LinkBehindMove,
            OriginEvent::UpstreamRefused,
            OriginEvent::NotValid,
        ] {
            assert_eq!(of_doc(&[event]), Vec::new(), "{event:?}");
        }
        let cycle = group(
            Origin::Cycle(CycleId {
                source: CycleSource::Frames,
                min: docid(1),
            }),
            &[1, 2],
            &[1],
            &[OriginEvent::Cycle],
        );
        assert_eq!(settlements(&cycle, &foreseen(&[1, 2], &[])), Vec::new());
    }

    #[test]
    fn approving_again_is_listed_only_for_the_events_it_settles() {
        for event in every_event() {
            let listed = of_doc(&[event]).contains(&Settlement::ApproveStale);
            assert_eq!(
                listed,
                matches!(
                    event,
                    OriginEvent::Edited | OriginEvent::Arrived | OriginEvent::Relinked
                ),
                "{event:?}"
            );
        }
        // Whatever stands next to it: an upstream not decided, a link behind a move,
        // a gone artifact have no `ApproveStale`, with the shelf taking them.
        for event in [
            OriginEvent::UpstreamPending,
            OriginEvent::UpstreamAwaiting,
            OriginEvent::UpstreamSuperseded,
            OriginEvent::LinkBehindMove,
        ] {
            assert!(!of_doc(&[event]).contains(&Settlement::ApproveStale));
        }
        assert!(!of_file(&[OriginEvent::Gone]).contains(&Settlement::ApproveStale));
    }

    #[test]
    fn an_expiry_beside_an_edit_lists_both_and_the_second_drops_the_expiry() {
        assert_eq!(
            of_doc(&[OriginEvent::Edited, OriginEvent::Expired]),
            vec![
                Settlement::ReadDiff(docid(1)),
                Settlement::DeclareSame(SameSubject::Doc(docid(1))),
                Settlement::ApproveUntil(docid(1)),
                Settlement::ApproveStale,
            ]
        );
    }

    #[test]
    fn an_origin_none_of_whose_approvals_the_shelf_takes_has_no_approve_stale() {
        // Broken by another origin: the shelf leaves these out of `--stale`.
        let edited = group(Origin::Doc(docid(1)), &[2, 3], &[2], &[OriginEvent::Edited]);
        assert_eq!(
            settlements(&edited, &foreseen(&[], &[])),
            vec![
                Settlement::ReadDiff(docid(2)),
                Settlement::DeclareSame(SameSubject::Doc(docid(1))),
            ]
        );
        // One of them is taken: `--stale` approves it again.
        assert_eq!(
            settlements(&edited, &foreseen(&[3], &[])).last(),
            Some(&Settlement::ApproveStale)
        );
        // An approval the shelf takes that is not under the origin does not count.
        assert!(!settlements(&edited, &foreseen(&[9], &[])).contains(&Settlement::ApproveStale));
    }

    #[test]
    fn the_declaration_is_read_and_approved_again_and_nothing_is_declared_about_it() {
        let declaration = group(Origin::Declaration, &[2], &[2], &[OriginEvent::Relinked]);
        assert_eq!(
            settlements(&declaration, &foreseen(&[2], &[])),
            vec![Settlement::ReadDiff(docid(2)), Settlement::ApproveStale]
        );
        // Even an edit there is nothing to declare the meaning of.
        let edited = group(Origin::Declaration, &[2], &[2], &[OriginEvent::Edited]);
        assert!(
            settlements(&edited, &foreseen(&[2], &[]))
                .iter()
                .all(|s| !matches!(s, Settlement::DeclareSame(_)))
        );
    }

    #[test]
    fn the_diff_is_of_the_smallest_approval_that_holds_the_cause_and_not_one_below_it() {
        // 1, 3 and 5 are degraded by an edit of 7; 3 and 5 hold it, and 1 only sees it
        // through 3, so its diff shows nothing.
        let edited = group(
            Origin::Doc(docid(7)),
            &[1, 3, 5],
            &[5, 3],
            &[OriginEvent::Edited],
        );
        assert_eq!(
            settlements(&edited, &foreseen(&[1, 3, 5], &[])).first(),
            Some(&Settlement::ReadDiff(docid(3)))
        );
    }

    #[test]
    fn the_diff_is_of_an_approval_that_holds_the_edit_and_not_of_one_that_only_lapsed() {
        // Document 1 was approved again on its edited text, and that approval lapsed:
        // it holds the lapse. 2 still pins the text before the edit: it holds the
        // edit, and its diff is the one that shows it.
        let mixed = OriginGroup {
            origin: Origin::Doc(docid(1)),
            degraded: [docid(1), docid(2)].into(),
            held: NonEmptySet::new(
                (OriginEvent::Expired, docid(1)),
                [(OriginEvent::Edited, docid(2))],
            ),
        };
        assert_eq!(
            settlements(&mixed, &foreseen(&[1, 2], &[(1, Ok(()))])),
            vec![
                Settlement::ReadDiff(docid(2)),
                Settlement::DeclareSame(SameSubject::Doc(docid(1))),
                Settlement::ApproveUntil(docid(1)),
                Settlement::ApproveStale,
            ]
        );
    }

    #[test]
    fn many_events_and_approvals_list_a_settlement_once() {
        let many = group(
            Origin::Artifact(file_artifact("tests/a.rs")),
            &[2, 3, 4],
            &[2, 3],
            &[
                OriginEvent::Edited,
                OriginEvent::Arrived,
                OriginEvent::Relinked,
            ],
        );
        assert_eq!(
            settlements(&many, &foreseen(&[2, 3, 4], &[])),
            vec![
                Settlement::ReadDiff(docid(2)),
                Settlement::DeclareSame(SameSubject::Artifact(file_artifact("tests/a.rs"))),
                Settlement::ApproveStale,
            ]
        );
    }

    #[test]
    fn an_approval_by_name_the_frame_refuses_is_not_listed() {
        // Beside an edit, so that what is not an approval by name is seen to stay.
        for event in [
            OriginEvent::Expired,
            OriginEvent::UpstreamPending,
            OriginEvent::UpstreamAwaiting,
        ] {
            let origin = group(
                Origin::Doc(docid(1)),
                &[1],
                &[1],
                &[OriginEvent::Edited, event],
            );
            assert_eq!(
                settlements(&origin, &foreseen(&[1], &[(1, Err(refusal()))])),
                vec![
                    Settlement::ReadDiff(docid(1)),
                    Settlement::DeclareSame(SameSubject::Doc(docid(1))),
                    Settlement::ApproveStale,
                ],
                "{event:?}"
            );
        }
    }

    #[test]
    fn origins_asked_names_the_documents_the_settlements_approve_by_name() {
        let groups = vec![
            group(Origin::Doc(docid(1)), &[1], &[1], &[OriginEvent::Expired]),
            group(
                Origin::Doc(docid(2)),
                &[4],
                &[4],
                &[OriginEvent::UpstreamPending],
            ),
            group(Origin::Doc(docid(3)), &[3], &[3], &[OriginEvent::Edited]),
            group(
                Origin::Artifact(file_artifact("tests/a.rs")),
                &[5],
                &[5],
                &[OriginEvent::Gone],
            ),
        ];
        assert_eq!(
            origins_asked(groups.clone()),
            OriginsAsked::new(groups, [docid(1), docid(2)].into())
        );
    }

    #[test]
    fn a_document_not_asked_about_is_not_taken() {
        let expired = group(Origin::Doc(docid(1)), &[1], &[1], &[OriginEvent::Expired]);
        assert_eq!(
            settlements(&expired, &foreseen(&[1], &[])),
            Vec::<Settlement>::new()
        );
    }
}
