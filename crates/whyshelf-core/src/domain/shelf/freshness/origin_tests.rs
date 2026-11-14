//! Tests of where degradations start, grouped across documents
//! ([`Assessment::origins`]).

use crate::domain::shelf::artifact::ArtifactHash;
use crate::domain::shelf::fixtures_tests::{
    approve, approved, deny, docid, file_artifact, fr, fr_titled, ledger, lines, shelf, shelf_with,
    sp_testing,
};
use crate::domain::shelf::graph::CycleSource;
use crate::domain::shelf::ledger::{JudgedAt, Record};
use crate::domain::shelf::order;
use crate::domain::shelf::settle::{Settlement, settlements};

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
        .filter(|(_, s)| matches!(s, Standing::Approved(Freshness::Degraded(_))))
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
    let Standing::Approved(Freshness::Degraded(degradation)) =
        assess(&now, &l, JudgedAt::new(100)).standing_of(docid(2))
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
        DeclaredStatus::Approved { frame, .. } => frame.into_iter().next().unwrap(),
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
    let deprecated = |successors| UpstreamStatus::Deprecated { successors };
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
            declared(UpstreamStatus::Pending),
            upstream(OriginEvent::UpstreamPending),
        ),
        (
            declared(deprecated(Successors::Some(NonEmptyVec::new(
                docid(9),
                vec![],
            )))),
            upstream(OriginEvent::UpstreamSuperseded),
        ),
        (
            declared(deprecated(Successors::None)),
            upstream(OriginEvent::UpstreamRefused),
        ),
        (
            declared(UpstreamStatus::Denied),
            upstream(OriginEvent::UpstreamRefused),
        ),
        (
            declared(UpstreamStatus::Retired),
            upstream(OriginEvent::UpstreamRefused),
        ),
        (
            Cause::Via {
                pin: up.clone(),
                severity: Severity::Stale,
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
    let taken = order::stale_taken(&now, &l, &assessment);
    assert_eq!(
        settlements(group, &taken).first(),
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
    assert!(!settlements(edited, &taken).contains(&Settlement::ApproveStale));
}
