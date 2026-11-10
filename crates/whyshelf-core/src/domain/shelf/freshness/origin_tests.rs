//! Tests of where degradations start, grouped across documents
//! ([`Assessment::origins`]).

use crate::domain::shelf::artifact::ArtifactHash;
use crate::domain::shelf::fixtures_tests::{
    approve, approved, deny, docid, file_artifact, fr, fr_titled, ledger, shelf, shelf_with,
    sp_testing,
};
use crate::domain::shelf::graph::CycleSource;
use crate::domain::shelf::ledger::{JudgedAt, Record};

use super::*;

fn h(text: &str) -> ArtifactHash {
    ArtifactHash::of_content(text.as_bytes())
}

fn ids(ns: &[u128]) -> BTreeSet<DocId> {
    ns.iter().map(|n| docid(*n)).collect()
}

fn group(origin: Origin, degraded: &[u128]) -> OriginGroup {
    OriginGroup {
        origin,
        degraded: ids(degraded),
    }
}

/// The origins, after checking that every degraded approval is under one of them
/// and that only degraded approvals are.
fn origins(assessment: &Assessment) -> Vec<OriginGroup> {
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
    origins
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
