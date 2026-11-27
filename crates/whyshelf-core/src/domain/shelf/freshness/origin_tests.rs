//! Tests of where degradations start, grouped across documents
//! ([`Assessment::origins`]).

use crate::domain::actor::{Actor, Attribution};
use crate::domain::nonempty::NonBlank;
use crate::domain::shelf::artifact::{ArtifactHash, ArtifactReading};
use crate::domain::shelf::fixtures_tests::{
    admitted, approve, approve_as, approved, asking, deny, deprecate, docid, file_artifact, fr,
    fr_titled, jid, ledger, lines, list, reading, ruled_shelf, shelf, shelf_answered, shelf_with,
    sp_testing, text,
};
use crate::domain::shelf::graph::CycleSource;
use crate::domain::shelf::ledger::{
    ApproveFrame, Decision, FrameRefusal, JudgedAt, Record, frame_approve, frame_present,
};
use crate::domain::shelf::order::{self, Foreseen};
use crate::domain::shelf::settle::{
    Approval, Settled, Settlement, origins_asked, settle_all, settlements,
};

use super::assess_unasked as assess;
use super::*;

fn h(text: &str) -> ArtifactHash {
    ArtifactHash::of_content(text.as_bytes())
}

fn ids(ns: &[u128]) -> BTreeSet<DocId> {
    ns.iter().map(|n| docid(*n)).collect()
}

/// An origin with the approvals under it: what most of these tests compare. The
/// approvals that hold a cause and the events of an origin are what the tests of
/// `holders` and `events` below look at, so a group is read as this.
#[derive(Debug, PartialEq, Eq)]
struct Under {
    origin: Origin,
    degraded: BTreeSet<DocId>,
}

fn group(origin: Origin, degraded: &[u128]) -> Under {
    Under {
        origin,
        degraded: ids(degraded),
    }
}

/// The origins, after checking that every degraded approval is under one of them
/// and that only degraded approvals are.
fn origins(assessment: &Assessment) -> Vec<Under> {
    let origins = assessment.origins();
    let degraded: BTreeSet<DocId> = assessment
        .standings()
        .iter()
        .filter(|(_, s)| {
            matches!(
                s,
                Standing::Approved {
                    freshness: Freshness::Degraded(_),
                    ..
                }
            )
        })
        .map(|(id, _)| *id)
        .collect();
    let under: BTreeSet<DocId> = origins
        .iter()
        .flat_map(|g| g.degraded.iter().copied())
        .collect();
    assert_eq!(under, degraded, "every degraded approval has an origin");
    // The holders are among the degraded approvals, whatever else a test looks at.
    for g in &origins {
        assert!(
            g.holders().iter().all(|h| g.degraded.contains(h)),
            "{g:?}: a holder is a degraded approval of its origin"
        );
    }
    origins
        .into_iter()
        .map(|g| Under {
            origin: g.origin,
            degraded: g.degraded,
        })
        .collect()
}

#[test]
fn nothing_degraded_has_no_origin() {
    let s = shelf(vec![fr(1, &[]), fr(2, &["FR-1"])]);
    let l = approved(&s, &[1, 2]);
    assert_eq!(origins(&assess(&s, &l, JudgedAt::new(100))), Vec::new());
}

#[test]
fn an_edited_document_is_one_origin_of_everything_below_it() {
    let at_approval = shelf(vec![fr(1, &[]), fr(2, &["FR-1"]), fr(3, &["FR-2"])]);
    let l = approved(&at_approval, &[1, 2, 3]);
    let now = shelf(vec![
        fr_titled(1, "edited", &[]),
        fr(2, &["FR-1"]),
        fr(3, &["FR-2"]),
    ]);
    assert_eq!(
        origins(&assess(&now, &l, JudgedAt::new(100))),
        vec![group(Origin::Doc(docid(1)), &[1, 2, 3])]
    );
}

#[test]
fn an_upstream_approved_again_is_the_origin_of_what_saw_its_older_text() {
    let at_approval = shelf(vec![fr(1, &[]), fr(2, &["FR-1"]), fr(3, &["FR-1"])]);
    let now = shelf(vec![
        fr_titled(1, "edited", &[]),
        fr(2, &["FR-1"]),
        fr(3, &["FR-1"]),
    ]);
    let mut records: Vec<Record> = Vec::new();
    for subject in [1u128, 2, 3] {
        let so_far = ledger(records.clone());
        records.push(approve(
            &at_approval,
            &so_far,
            subject,
            1000 + subject,
            10,
            None,
        ));
    }
    records.push(approve(&now, &ledger(records.clone()), 1, 2001, 20, None));
    let assessment = assess(&now, &ledger(records), JudgedAt::new(100));
    // FR-1 holds again; FR-2 and FR-3 each saw its older text, from one edit.
    assert_eq!(
        origins(&assessment),
        vec![group(Origin::Doc(docid(1)), &[2, 3])]
    );
}

#[test]
fn one_file_pinned_by_several_approvals_is_one_origin_and_the_largest_comes_first() {
    let files = |a: &str, b: Option<&str>| [("tests/a.rs", Some(h(a))), ("tests/b.rs", b.map(h))];
    let docs = |title: &str| {
        vec![
            fr_titled(1, title, &[]),
            sp_testing(2, &["FR-1"], &["tests/a.rs"]),
            sp_testing(3, &["FR-1"], &["tests/a.rs", "tests/b.rs"]),
            sp_testing(4, &[], &["tests/a.rs"]),
            sp_testing(5, &[], &["tests/a.rs"]),
        ]
    };
    let at_approval = shelf_with(docs("title"), &files("a", Some("b")));
    let l = approved(&at_approval, &[1, 2, 3, 4, 5]);

    // Only the file is edited: one origin for the four approvals that pin it.
    let now = shelf_with(docs("title"), &files("a, edited", Some("b")));
    assert_eq!(
        origins(&assess(&now, &l, JudgedAt::new(100))),
        vec![group(
            Origin::Artifact(file_artifact("tests/a.rs")),
            &[2, 3, 4, 5]
        )]
    );

    // The finding is edited and another file is gone too. SP-2 and SP-3 are under
    // more than one origin. The file of four comes before the document of three,
    // though a document orders before a file; the gone file degrades SP-3 alone.
    let now = shelf_with(docs("edited"), &files("a, edited", None));
    assert_eq!(
        origins(&assess(&now, &l, JudgedAt::new(100))),
        vec![
            group(Origin::Artifact(file_artifact("tests/a.rs")), &[2, 3, 4, 5]),
            group(Origin::Doc(docid(1)), &[1, 2, 3]),
            group(Origin::Artifact(file_artifact("tests/b.rs")), &[3]),
        ]
    );
}

#[test]
fn an_edit_inside_a_cycle_of_frames_is_still_the_origin_of_what_pins_it() {
    // FR-1 was approved standing on FR-2, and FR-2 standing on FR-1: a cycle of
    // frames, across which no Via passes.
    let one = shelf(vec![fr(1, &["FR-2"]), fr(2, &[])]);
    let other = shelf(vec![fr(1, &[]), fr(2, &["FR-1"])]);
    let l = ledger(vec![
        approve(&one, &ledger(vec![]), 1, 1001, 10, None),
        approve(&other, &ledger(vec![]), 2, 1002, 10, None),
    ]);
    let now = shelf(vec![fr(1, &["FR-2"]), fr_titled(2, "edited", &[])]);
    let assessment = assess(&now, &l, JudgedAt::new(100));

    // FR-1 saw FR-2 before the edit: with no Via to lead there, the change is a
    // root of FR-1 itself.
    assert!(
        assessment.roots_of(docid(1)).iter().any(
            |r| matches!(&r.cause, Cause::UpstreamChanged { pin, .. } if pin.target() == docid(2))
        ),
        "{:?}",
        assessment.roots_of(docid(1))
    );
    let groups = origins(&assessment);
    assert!(
        groups.contains(&group(Origin::Doc(docid(2)), &[1, 2])),
        "{groups:?}"
    );
    // The cycle is one origin of both its members, not one per member.
    let cycles: Vec<_> = groups
        .iter()
        .filter(|g| matches!(g.origin, Origin::Cycle(id) if id.source == CycleSource::Frames))
        .collect();
    assert_eq!(cycles.len(), 1, "{groups:?}");
    assert_eq!(cycles[0].degraded, ids(&[1, 2]));
}

#[test]
fn a_via_starts_nowhere_and_a_pin_starts_at_what_it_names() {
    let at_approval = shelf(vec![fr(1, &[]), fr(2, &["FR-1"])]);
    let l = approved(&at_approval, &[1, 2]);
    let now = shelf(vec![fr_titled(1, "edited", &[]), fr(2, &["FR-1"])]);
    let Standing::Approved {
        freshness: Freshness::Degraded(degradation),
        ..
    } = assess(&now, &l, JudgedAt::new(100)).standing_of(docid(2))
    else {
        panic!("FR-2 stands on an edited upstream")
    };
    let mut seen = 0;
    for cause in degradation.causes().iter() {
        match cause {
            Cause::Via { .. } => assert_eq!(cause.origin(docid(2)), None),
            Cause::UpstreamChanged { .. } => {
                assert_eq!(cause.origin(docid(2)), Some(Origin::Doc(docid(1))));
            }
            other => panic!("unexpected {other:?}"),
        }
        seen += 1;
    }
    assert_eq!(seen, 2, "a Via and the change it leads to");
    let lapsed = Cause::Expired {
        since: JudgedAt::new(1),
    };
    assert_eq!(lapsed.origin(docid(2)), Some(Origin::Doc(docid(2))));
}

#[test]
fn an_upstream_denied_or_gone_is_the_origin_of_what_stands_on_it() {
    let s = shelf(vec![fr(1, &[]), fr(2, &["FR-1"]), fr(3, &["FR-1"])]);
    let mut records: Vec<Record> = Vec::new();
    for subject in [1u128, 2, 3] {
        let so_far = ledger(records.clone());
        records.push(approve(&s, &so_far, subject, 1000 + subject, 10, None));
    }
    let mut denied = records.clone();
    denied.push(deny(&s, 1, 2001, 20));
    // FR-1 denied is no approval of its own; FR-2 and FR-3 stand on it.
    assert_eq!(
        origins(&assess(&s, &ledger(denied), JudgedAt::new(100))),
        vec![group(Origin::Doc(docid(1)), &[2, 3])]
    );
    // FR-1's file gone: one origin for FR-1 itself and all that stand on it.
    let gone = shelf(vec![fr(2, &["FR-1"]), fr(3, &["FR-1"])]);
    assert_eq!(
        origins(&assess(&gone, &ledger(records), JudgedAt::new(100)))
            .first()
            .map(|g| (g.origin.clone(), g.degraded.clone())),
        Some((Origin::Doc(docid(1)), ids(&[1, 2, 3])))
    );
}

/// The pin FR-2's approval keeps of FR-1, from a real approval.
fn pin_of_fr1() -> FramePin {
    let s = shelf(vec![fr(1, &[]), fr(2, &["FR-1"])]);
    match approved(&s, &[2]).status_of(docid(2)) {
        DeclaredStatus::Approved(approval) => approval.frame().iter().next().unwrap().clone(),
        other => panic!("an approval, got {other:?}"),
    }
}

/// The pin of `tests/a.rs` that SP-2's approval keeps and reads, from a real edit of
/// the file (a pin cannot be made by hand: the ledger makes it).
fn read_pin_of_a() -> ReadPin {
    let docs = || vec![fr(1, &[]), sp_testing(2, &["FR-1"], &["tests/a.rs"])];
    let then = shelf_with(docs(), &[("tests/a.rs", Some(h("a")))]);
    let l = approved(&then, &[1, 2]);
    let now = shelf_with(docs(), &[("tests/a.rs", Some(h("edited")))]);
    assess(&now, &l, JudgedAt::new(100))
        .roots_of(docid(2))
        .into_iter()
        .find_map(|root| match root.cause {
            Cause::ArtifactChanged { pin, .. } => Some(pin),
            _ => None,
        })
        .expect("an edited file is an ArtifactChanged of the approval that pins it")
}

#[test]
fn every_cause_is_located_at_its_origin_with_the_event_that_happened_there() {
    let holder = docid(2);
    let here = |event| Some((Origin::Doc(holder), event));
    let upstream = |event| Some((Origin::Doc(docid(1)), event));
    let file = |event| Some((Origin::Artifact(file_artifact("tests/a.rs")), event));
    let doc_hash = |title: &str| {
        *shelf(vec![fr_titled(1, title, &[])])
            .doc(docid(1))
            .unwrap()
            .hash()
    };
    let up = pin_of_fr1();
    let read = read_pin_of_a();
    let declared = |status| Cause::UpstreamDeclared {
        pin: up.clone(),
        status,
    };
    let superseded = |successors| UpstreamStatus::Unapproved(Unapproved::Superseded { successors });
    let refused = |refused| UpstreamStatus::Unapproved(Unapproved::Refused(refused));
    let cycle = CycleId {
        source: CycleSource::Frames,
        min: docid(1),
    };
    let cases: Vec<(Cause, Option<(Origin, OriginEvent)>)> = vec![
        (
            Cause::SelfChanged {
                approved: doc_hash("a"),
                now: doc_hash("b"),
            },
            here(OriginEvent::Edited),
        ),
        (
            Cause::SelfNotValid(DocFailure::Gone),
            here(OriginEvent::NotValid),
        ),
        (
            Cause::UpstreamNotValid {
                pin: up.clone(),
                failure: DocFailure::Gone,
            },
            upstream(OriginEvent::NotValid),
        ),
        (
            Cause::UpstreamChanged {
                pin: up.clone(),
                now: doc_hash("b"),
            },
            upstream(OriginEvent::Edited),
        ),
        // The three ways an upstream is declared, which `Cause::severity` splits the
        // same way.
        (
            declared(UpstreamStatus::Unapproved(Unapproved::Pending)),
            upstream(OriginEvent::UpstreamPending),
        ),
        (
            declared(superseded(NonEmptyVec::new(docid(9), vec![]))),
            upstream(OriginEvent::UpstreamSuperseded),
        ),
        (
            declared(refused(Refused::Unreplaced)),
            upstream(OriginEvent::UpstreamRefused),
        ),
        (
            declared(refused(Refused::Denied)),
            upstream(OriginEvent::UpstreamRefused),
        ),
        (
            declared(refused(Refused::Retired)),
            upstream(OriginEvent::UpstreamRefused),
        ),
        (
            Cause::Via {
                pin: up.clone(),
                carried: Carried::Stale,
            },
            None,
        ),
        (
            Cause::LinksChanged {
                link: EntryName::new("base").unwrap(),
                added: vec![docid(3)],
                removed: Vec::new(),
                unresolved: Vec::new(),
            },
            here(OriginEvent::Relinked),
        ),
        (
            Cause::ArtifactChanged {
                pin: read.clone(),
                now: h("b"),
            },
            file(OriginEvent::Edited),
        ),
        (
            Cause::ArtifactGone { pin: read.clone() },
            file(OriginEvent::Gone),
        ),
        (
            Cause::ArtifactArrived {
                pin: read.clone(),
                now: h("b"),
            },
            file(OriginEvent::Arrived),
        ),
        (
            Cause::ArtifactComparisonChanged {
                pin: read.clone(),
                now: lines(&["#"]),
            },
            Some((Origin::Declaration, OriginEvent::Relinked)),
        ),
        (
            Cause::ArtifactMoved {
                link: EntryName::new("tests").unwrap(),
                written: file_artifact("tests/old.rs"),
                pin: read,
            },
            here(OriginEvent::LinkBehindMove),
        ),
        (
            Cause::ArtifactLinksChanged {
                link: EntryName::new("tests").unwrap(),
                added: vec![file_artifact("tests/b.rs")],
                removed: Vec::new(),
            },
            here(OriginEvent::Relinked),
        ),
        (
            Cause::InCycle { cycle },
            Some((Origin::Cycle(cycle), OriginEvent::Cycle)),
        ),
        (
            Cause::Expired {
                since: JudgedAt::new(1),
            },
            here(OriginEvent::Expired),
        ),
    ];
    // Every kind of cause is in the table, so a new one is not forgotten here.
    let kinds: std::collections::HashSet<_> = cases
        .iter()
        .map(|(cause, _)| std::mem::discriminant(cause))
        .collect();
    assert_eq!(kinds.len(), 15);
    for (cause, expected) in cases {
        assert_eq!(cause.located(holder), expected, "{cause:?}");
        // `origin` is `located` without the event, never a second reading.
        assert_eq!(
            cause.origin(holder),
            expected.map(|(origin, _)| origin),
            "{cause:?}"
        );
    }
}

/// The group of an origin, with what it keeps beside the approvals under it.
fn group_of(groups: &[OriginGroup], origin: &Origin) -> (Vec<DocId>, Vec<DocId>, Vec<OriginEvent>) {
    let group = groups
        .iter()
        .find(|g| &g.origin == origin)
        .unwrap_or_else(|| panic!("an origin {origin:?} in {groups:?}"));
    (
        group.degraded.iter().copied().collect(),
        group.holders().into_iter().collect(),
        group.events().into_iter().collect(),
    )
}

#[test]
fn an_approval_that_sees_an_edit_through_an_upstream_is_degraded_and_holds_nothing() {
    let at_approval = shelf(vec![fr(1, &[]), fr(2, &["FR-1"]), fr(3, &["FR-2"])]);
    let l = approved(&at_approval, &[1, 2, 3]);
    let now = shelf(vec![
        fr_titled(1, "edited", &[]),
        fr(2, &["FR-1"]),
        fr(3, &["FR-2"]),
    ]);
    let groups = assess(&now, &l, JudgedAt::new(100)).origins();
    // FR-2 and FR-3 are degraded by the edit; only FR-1 holds it.
    assert_eq!(
        group_of(&groups, &Origin::Doc(docid(1))),
        (
            vec![docid(1), docid(2), docid(3)],
            vec![docid(1)],
            vec![OriginEvent::Edited]
        )
    );
}

#[test]
fn an_upstream_approved_again_is_held_by_what_saw_its_older_text() {
    let at_approval = shelf(vec![fr(1, &[]), fr(2, &["FR-1"]), fr(3, &["FR-1"])]);
    let now = shelf(vec![
        fr_titled(1, "edited", &[]),
        fr(2, &["FR-1"]),
        fr(3, &["FR-1"]),
    ]);
    let mut records: Vec<Record> = Vec::new();
    for subject in [1u128, 2, 3] {
        let so_far = ledger(records.clone());
        records.push(approve(
            &at_approval,
            &so_far,
            subject,
            1000 + subject,
            10,
            None,
        ));
    }
    records.push(approve(&now, &ledger(records.clone()), 1, 2001, 20, None));
    let groups = assess(&now, &ledger(records), JudgedAt::new(100)).origins();
    // FR-1 holds again: the two that saw the older text hold the cause, each of itself.
    assert_eq!(
        group_of(&groups, &Origin::Doc(docid(1))),
        (
            vec![docid(2), docid(3)],
            vec![docid(2), docid(3)],
            vec![OriginEvent::Edited]
        )
    );
}

#[test]
fn an_origin_of_two_kinds_of_cause_has_both_events() {
    // FR-1 is approved to lapse at 50, and is edited: both start at FR-1.
    let at_approval = shelf(vec![fr(1, &[])]);
    let now = shelf(vec![fr_titled(1, "edited", &[])]);
    let l = ledger(vec![approve(
        &at_approval,
        &ledger(vec![]),
        1,
        1001,
        10,
        Some(50),
    )]);
    let groups = assess(&now, &l, JudgedAt::new(100)).origins();
    assert_eq!(
        group_of(&groups, &Origin::Doc(docid(1))),
        (
            vec![docid(1)],
            vec![docid(1)],
            vec![OriginEvent::Edited, OriginEvent::Expired]
        )
    );
}

#[test]
fn an_upstream_with_no_decision_or_decided_against_is_held_by_what_stands_on_it() {
    let s = shelf(vec![fr(1, &[]), fr(2, &["FR-1"])]);
    // FR-2 is approved on a FR-1 nobody decided about.
    let groups = assess(&s, &approved(&s, &[2]), JudgedAt::new(100)).origins();
    assert_eq!(
        group_of(&groups, &Origin::Doc(docid(1))),
        (
            vec![docid(2)],
            vec![docid(2)],
            vec![OriginEvent::UpstreamPending]
        )
    );
    // FR-1 is approved and then denied.
    let first = approve(&s, &ledger(vec![]), 1, 1001, 10, None);
    let second = approve(&s, &ledger(vec![first.clone()]), 2, 1002, 10, None);
    let l = ledger(vec![first, second, deny(&s, 1, 2001, 20)]);
    let groups = assess(&s, &l, JudgedAt::new(100)).origins();
    assert_eq!(
        group_of(&groups, &Origin::Doc(docid(1))),
        (
            vec![docid(2)],
            vec![docid(2)],
            vec![OriginEvent::UpstreamRefused]
        )
    );
}

#[test]
fn an_origin_edited_and_lapsed_keeps_which_approval_holds_which_event() {
    // FR-2 pins FR-1 as it was; FR-1 is edited and approved again on the edit with an
    // expiry, which lapses. FR-1 holds the lapse and FR-2 the edit.
    let at_approval = shelf(vec![fr(1, &[]), fr(2, &["FR-1"])]);
    let now = shelf(vec![fr_titled(1, "edited", &[]), fr(2, &["FR-1"])]);
    let first = approve(&at_approval, &ledger(vec![]), 1, 1001, 10, None);
    let second = approve(
        &at_approval,
        &ledger(vec![first.clone()]),
        2,
        1002,
        10,
        None,
    );
    let mut records: Vec<Record> = vec![first, second];
    records.push(approve(
        &now,
        &ledger(records.clone()),
        1,
        2001,
        20,
        Some(50),
    ));
    let l = ledger(records);
    let assessment = assess(&now, &l, JudgedAt::new(100));
    let groups = assessment.origins();
    let group = groups
        .iter()
        .find(|g| g.origin == Origin::Doc(docid(1)))
        .expect("an origin at FR-1");
    assert_eq!(
        group.held.iter().copied().collect::<Vec<_>>(),
        vec![
            (OriginEvent::Edited, docid(2)),
            (OriginEvent::Expired, docid(1))
        ]
    );
    // The diff that shows the edit is FR-2's: FR-1's own approval saw the edit.
    assert_eq!(
        next_at(&settled(&now, &l), 1).first(),
        Some(&Settlement::ReadDiff(docid(2)))
    );
}

#[test]
fn an_edit_of_an_approval_on_a_pending_upstream_is_not_settled_by_stale() {
    // FR-1 is approved standing on FR-9, which nobody decided about, and is edited:
    // `--stale` holds FR-1 back on the pending FR-9, so it is not listed for the edit.
    let at_approval = shelf(vec![fr(1, &["FR-9"]), fr(9, &[])]);
    let now = shelf(vec![fr_titled(1, "edited", &["FR-9"]), fr(9, &[])]);
    let l = approved(&at_approval, &[1]);
    let assessment = assess(&now, &l, JudgedAt::new(100));
    assert!(order::stale(&assessment).again.contains(&docid(1)));
    let taken = order::stale_taken(&now, &l, &assessment);
    assert!(taken.is_empty(), "{taken:?}");
    let groups = assessment.origins();
    let edited = groups
        .iter()
        .find(|g| g.origin == Origin::Doc(docid(1)))
        .expect("an origin at FR-1");
    let foreseen = Foreseen::of_parts(taken, BTreeMap::new());
    assert!(
        !settlements(edited, &foreseen)
            .next
            .contains(&Settlement::ApproveStale)
    );
}

/// FR-1 approved on `then` and then on `now` with nothing else stale, and the origin
/// at FR-1: `--stale` takes FR-1 again, the frame refuses it, so it would approve
/// nothing of that origin and is not listed for it, though it would be were FR-1
/// taken.
fn not_settled_by_stale_where_the_frame_refuses(now: &Shelf, l: &Ledger) {
    let assessment = assess(now, l, JudgedAt::new(100));
    assert!(order::stale(&assessment).again.contains(&docid(1)));
    let taken = order::stale_taken(now, l, &assessment);
    assert!(taken.is_empty(), "{taken:?}");
    let groups = assessment.origins();
    let at_fr1 = groups
        .iter()
        .find(|g| g.origin == Origin::Doc(docid(1)))
        .expect("an origin at FR-1");
    let taking = |again| Foreseen::of_parts(again, BTreeMap::new());
    assert!(
        settlements(at_fr1, &taking(ids(&[1])))
            .next
            .contains(&Settlement::ApproveStale)
    );
    assert!(
        !settlements(at_fr1, &taking(taken))
            .next
            .contains(&Settlement::ApproveStale)
    );
}

#[test]
fn an_edit_of_an_approval_on_an_upstream_deprecated_with_a_successor_is_not_settled_by_stale() {
    // FR-1 is approved standing on FR-9, which is then deprecated with a successor,
    // FR-3, so FR-1 is only stale on it; FR-1 is then edited.
    let replacing = || {
        reading(
            "findings/fr-3.md",
            3,
            "FR-3",
            &[("title", text("new")), ("supersedes", list(&["FR-9"]))],
        )
    };
    let at_approval = shelf(vec![fr(1, &["FR-9"]), fr(9, &[]), replacing()]);
    let now = shelf(vec![
        fr_titled(1, "edited", &["FR-9"]),
        fr(9, &[]),
        replacing(),
    ]);
    let first = approve(&at_approval, &ledger(vec![]), 9, 1009, 10, None);
    let second = approve(
        &at_approval,
        &ledger(vec![first.clone()]),
        1,
        1001,
        10,
        None,
    );
    let l = ledger(vec![first, second, deprecate(&at_approval, 9, 1010, 20)]);
    not_settled_by_stale_where_the_frame_refuses(&now, &l);
}

#[test]
fn a_link_rewritten_to_a_denied_document_is_not_settled_by_stale() {
    // FR-1 is approved standing on FR-8, and its link is then rewritten to FR-9, which
    // is denied.
    let at_approval = shelf(vec![fr(1, &["FR-8"]), fr(8, &[]), fr(9, &[])]);
    let now = shelf(vec![fr(1, &["FR-9"]), fr(8, &[]), fr(9, &[])]);
    let first = approve(&at_approval, &ledger(vec![]), 8, 1008, 10, None);
    let second = approve(
        &at_approval,
        &ledger(vec![first.clone()]),
        1,
        1001,
        10,
        None,
    );
    let l = ledger(vec![first, second, deny(&at_approval, 9, 1010, 20)]);
    not_settled_by_stale_where_the_frame_refuses(&now, &l);
}

/// What settles each origin at time 100, read as `stale` reads it: the documents the
/// settlements approve by name asked about once for the shelf, and the origins
/// settled with what was foreseen of them.
fn settled(now: &Shelf, l: &Ledger) -> Vec<(OriginGroup, Settled)> {
    let assessment = assess(now, l, JudgedAt::new(100));
    settle_all(Foreseen::of(
        now,
        l,
        &assessment,
        origins_asked(assessment.origins()),
    ))
}

/// The origin at document `n`, with its settlements.
fn settled_at(settled: &[(OriginGroup, Settled)], n: u128) -> &(OriginGroup, Settled) {
    settled
        .iter()
        .find(|(group, _)| group.origin == Origin::Doc(docid(n)))
        .unwrap_or_else(|| panic!("an origin at {n} in {settled:?}"))
}

/// The settlements of the origin at document `n`.
fn next_at(settled: &[(OriginGroup, Settled)], n: u128) -> &[Settlement] {
    &settled_at(settled, n).1.next
}

/// Whether `stale` offers `settlement` for the origin at document `n`.
fn offers(now: &Shelf, l: &Ledger, n: u128, settlement: &Settlement) -> bool {
    next_at(&settled(now, l), n).contains(settlement)
}

#[test]
fn every_origin_asked_about_is_settled_in_the_order_given() {
    // FR-1 is edited, and the approval of FR-2 lapsed: two origins.
    let then = shelf(vec![fr(1, &[]), fr(2, &[])]);
    let now = shelf(vec![fr_titled(1, "edited", &[]), fr(2, &[])]);
    let first = approve(&then, &ledger(vec![]), 1, 1001, 10, None);
    let second = approve(&then, &ledger(vec![first.clone()]), 2, 1002, 10, Some(50));
    let l = ledger(vec![first, second]);
    let origins = assess(&now, &l, JudgedAt::new(100)).origins();
    assert_eq!(origins.len(), 2, "{origins:?}");
    let settled = settled(&now, &l);
    let groups: Vec<OriginGroup> = settled.iter().map(|(group, _)| group.clone()).collect();
    assert_eq!(groups, origins);
    assert_eq!(
        next_at(&settled, 2),
        [Settlement::Approve(Approval::Until(docid(2)))]
    );
}

/// FR-1 stands on FR-9 and is approved to lapse at 50; FR-3 replaces FR-9, which is
/// deprecated after the approval when `deprecated`.
fn lapsed_on_a_replaced_upstream(deprecated: bool) -> (Shelf, Ledger) {
    let replacing = reading(
        "findings/fr-3.md",
        3,
        "FR-3",
        &[("title", text("new")), ("supersedes", list(&["FR-9"]))],
    );
    let s = shelf(vec![fr(1, &["FR-9"]), fr(9, &[]), replacing]);
    let first = approve(&s, &ledger(vec![]), 9, 1009, 10, None);
    let second = approve(&s, &ledger(vec![first.clone()]), 1, 1001, 10, Some(50));
    let mut records = vec![first, second];
    if deprecated {
        records.push(deprecate(&s, 9, 1010, 20));
    }
    (s, ledger(records))
}

#[test]
fn an_expiry_on_an_upstream_deprecated_with_a_successor_is_not_offered_its_approval() {
    let (s, l) = lapsed_on_a_replaced_upstream(true);
    let settled = settled(&s, &l);
    let expiry = Settlement::Approve(Approval::Until(docid(1)));
    assert!(!next_at(&settled, 1).contains(&expiry), "{settled:?}");
    // The refusal is seen at the upstream: superseded, with FR-1 under it.
    let (upstream, _) = settled_at(&settled, 9);
    assert!(upstream.events().contains(&OriginEvent::UpstreamSuperseded));
    assert!(upstream.degraded.contains(&docid(1)));
}

#[test]
fn an_expiry_on_an_upstream_that_stands_is_offered_its_approval() {
    let (s, l) = lapsed_on_a_replaced_upstream(false);
    assert!(offers(
        &s,
        &l,
        1,
        &Settlement::Approve(Approval::Until(docid(1)))
    ));
}

/// FR-2 is approved standing on FR-1, which nobody decided about, and a link of FR-1
/// names FR-9, which is denied when `denied`.
fn on_a_pending_upstream_on(denied: bool) -> (Shelf, Ledger) {
    let s = shelf(vec![fr(1, &["FR-9"]), fr(2, &["FR-1"]), fr(9, &[])]);
    let mut records = vec![approve(&s, &ledger(vec![]), 2, 1002, 10, None)];
    if denied {
        records.push(deny(&s, 9, 1009, 20));
    }
    (s, ledger(records))
}

#[test]
fn a_pending_upstream_whose_link_names_a_denied_document_is_not_offered_its_approval() {
    let (s, l) = on_a_pending_upstream_on(true);
    assert!(!offers(
        &s,
        &l,
        1,
        &Settlement::Approve(Approval::Pending(docid(1)))
    ));
}

#[test]
fn a_pending_upstream_whose_links_name_what_stands_is_offered_its_approval() {
    // FR-9 is pending too: the frame takes FR-1, whose approval is stale from the
    // start.
    let (s, l) = on_a_pending_upstream_on(false);
    assert!(offers(
        &s,
        &l,
        1,
        &Settlement::Approve(Approval::Pending(docid(1)))
    ));
}

/// FR-2 is approved standing on FR-1, which nobody decided about, and a link of FR-1
/// names FR-7, which is on the shelf only when `there`.
fn on_a_pending_upstream_linking(there: bool) -> (Shelf, Ledger) {
    let mut readings = vec![fr(1, &["FR-7"]), fr(2, &["FR-1"])];
    if there {
        readings.push(fr(7, &[]));
    }
    let s = shelf(readings);
    let l = ledger(vec![approve(&s, &ledger(vec![]), 2, 1002, 10, None)]);
    (s, l)
}

#[test]
fn a_pending_upstream_whose_link_cannot_be_followed_is_not_offered_its_approval() {
    let (s, l) = on_a_pending_upstream_linking(false);
    assert!(
        matches!(
            frame_approve(&s, &l, docid(1)),
            Err(FrameRefusal::UnresolvedDepends { .. })
        ),
        "{:?}",
        frame_approve(&s, &l, docid(1))
    );
    assert!(!offers(
        &s,
        &l,
        1,
        &Settlement::Approve(Approval::Pending(docid(1)))
    ));
}

#[test]
fn a_pending_upstream_whose_link_is_followed_is_offered_its_approval() {
    let (s, l) = on_a_pending_upstream_linking(true);
    assert!(offers(
        &s,
        &l,
        1,
        &Settlement::Approve(Approval::Pending(docid(1)))
    ));
}

/// The design `cache` is approved standing on SP-2, which nobody decided about and
/// which is tested by `tests/a.rs`; its adapter can tell about the file only when
/// `told`.
fn on_a_pending_spec(told: bool) -> (Shelf, Ledger) {
    let readings = vec![
        fr(1, &[]),
        sp_testing(2, &["FR-1"], &["tests/a.rs"]),
        reading(
            "designs/d.md",
            3,
            "cache",
            &[("title", text("d")), ("specs", list(&["SP-2"]))],
        ),
    ];
    let s = if told {
        shelf_with(readings, &[("tests/a.rs", Some(h("a")))])
    } else {
        let offline = ArtifactReading::cannot_tell(NonBlank::new("offline").unwrap());
        shelf_answered(readings, &[("tests/a.rs", offline)])
    };
    let l = ledger(vec![approve(&s, &ledger(vec![]), 3, 1003, 10, None)]);
    (s, l)
}

#[test]
fn a_pending_upstream_on_an_artifact_that_cannot_be_told_is_not_offered_its_approval() {
    let (s, l) = on_a_pending_spec(false);
    assert!(
        matches!(
            frame_approve(&s, &l, docid(2)),
            Err(FrameRefusal::ArtifactUnknown { .. })
        ),
        "{:?}",
        frame_approve(&s, &l, docid(2))
    );
    assert!(!offers(
        &s,
        &l,
        2,
        &Settlement::Approve(Approval::Pending(docid(2)))
    ));
}

#[test]
fn a_pending_upstream_on_an_artifact_that_is_told_is_offered_its_approval() {
    let (s, l) = on_a_pending_spec(true);
    assert!(offers(
        &s,
        &l,
        2,
        &Settlement::Approve(Approval::Pending(docid(2)))
    ));
}

/// Under a rule of two names: FR-9 approved by `ana` and `bo`, FR-1 on it by `ana`
/// alone, FR-2 on FR-1 by both; FR-9 is then denied when `denied`.
fn awaiting_on(denied: bool) -> (Shelf, Ledger) {
    let s = ruled_shelf(
        asking(2, None),
        vec![fr(1, &["FR-9"]), fr(2, &["FR-1"]), fr(9, &[])],
    );
    let mut records: Vec<Record> = Vec::new();
    for (subject, judgement, by) in [
        (9, 1009, "ana"),
        (9, 1010, "bo"),
        (1, 1001, "ana"),
        (2, 1002, "ana"),
        (2, 1003, "bo"),
    ] {
        let so_far = admitted(&s, records.clone());
        records.push(approve_as(&s, &so_far, subject, judgement, (by, 10, None)));
    }
    if denied {
        let frame = frame_present(&s, docid(9)).unwrap();
        let by = Attribution::one(Actor::new("ana").unwrap());
        let denial = Decision::deny(jid(1011), frame, by, JudgedAt::new(20), None).unwrap();
        records.push(Record::Decision(denial));
    }
    let l = admitted(&s, records);
    (s, l)
}

#[test]
fn an_awaiting_upstream_whose_own_upstream_is_denied_is_not_offered_another_name() {
    let (s, l) = awaiting_on(true);
    assert!(!offers(
        &s,
        &l,
        1,
        &Settlement::Approve(Approval::AsAnother(docid(1)))
    ));
}

#[test]
fn an_awaiting_upstream_whose_own_upstream_stands_is_offered_another_name() {
    let (s, l) = awaiting_on(false);
    assert!(offers(
        &s,
        &l,
        1,
        &Settlement::Approve(Approval::AsAnother(docid(1)))
    ));
}

#[test]
fn an_expiry_of_a_document_not_valid_is_not_offered_its_approval() {
    // FR-1 is approved to lapse at 50, and its file is then gone: the lapse is told
    // beside it.
    let then = shelf(vec![fr(1, &[]), fr(2, &[])]);
    let l = ledger(vec![approve(&then, &ledger(vec![]), 1, 1001, 10, Some(50))]);
    let now = shelf(vec![fr(2, &[])]);
    let settled = settled(&now, &l);
    let (at_fr1, at_fr1_settled) = settled_at(&settled, 1);
    assert_eq!(
        at_fr1.events(),
        BTreeSet::from([OriginEvent::Expired, OriginEvent::NotValid])
    );
    assert!(
        !at_fr1_settled
            .next
            .contains(&Settlement::Approve(Approval::Until(docid(1))))
    );
}

/// FR-2 is approved standing on FR-1, which nobody decided about; then the link of
/// FR-2 no longer names FR-1, and, when `closing`, a link of FR-1 names FR-2.
fn relinked(closing: bool) -> (Shelf, Ledger) {
    let then = shelf(vec![fr(1, &[]), fr(2, &["FR-1"])]);
    let l = approved(&then, &[2]);
    let back: &[&str] = if closing { &["FR-2"] } else { &[] };
    (shelf(vec![fr(1, back), fr(2, &[])]), l)
}

#[test]
fn a_pending_upstream_whose_approval_would_close_a_cycle_of_frames_is_not_offered_it() {
    let (now, l) = relinked(true);
    // There is no cycle of links. The frame of FR-2's approval still pins FR-1, so
    // an approval of FR-1 on FR-2 closes a cycle among the frames: what is foreseen
    // of `--stale` does not tell it, and the frame a run of one meets does.
    assert_eq!(ApproveFrame::foresee(&now, &l, docid(1)), Ok(()));
    assert!(
        matches!(
            frame_approve(&now, &l, docid(1)),
            Err(FrameRefusal::InCycle(_))
        ),
        "{:?}",
        frame_approve(&now, &l, docid(1))
    );
    assert!(!offers(
        &now,
        &l,
        1,
        &Settlement::Approve(Approval::Pending(docid(1)))
    ));
}

#[test]
fn a_pending_upstream_whose_approval_closes_no_cycle_is_offered_it() {
    let (now, l) = relinked(false);
    assert!(offers(
        &now,
        &l,
        1,
        &Settlement::Approve(Approval::Pending(docid(1)))
    ));
}
