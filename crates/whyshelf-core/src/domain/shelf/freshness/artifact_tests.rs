//! Tests of freshness for the artifacts an approval pinned (rules A3 and A4).

use std::collections::BTreeMap;

use crate::domain::shelf::artifact::{ArtifactHash, ContentAddress, Revision};
use crate::domain::shelf::fixtures_tests::{
    actor, docid, fr, jid, ledger, path, shelf_with, sp_testing,
};
use crate::domain::shelf::ledger::{Decision, JudgedAt, Record, frame_approve};

use super::*;

fn h(text: &str) -> ArtifactHash {
    ArtifactHash::of_content(text.as_bytes())
}

/// The shelf of FR-1 and SP-2, which depends on FR-1 and is tested by `tests`, with
/// these artifacts.
fn spec_shelf(tests: &[&str], artifacts: &[(&str, Option<ArtifactHash>)]) -> Shelf {
    shelf_with(vec![fr(1, &[]), sp_testing(2, &["FR-1"], tests)], artifacts)
}

/// The ledger that approves FR-1, then SP-2, on `at_approval`, with addresses on
/// every file when `addressed`.
fn approved_on(at_approval: &Shelf, addressed: bool) -> Ledger {
    let mut records = Vec::new();
    for subject in [1u128, 2] {
        let so_far = ledger(records.clone());
        let mut frame = frame_approve(at_approval, &so_far, docid(subject)).unwrap();
        if addressed {
            let addresses: BTreeMap<_, _> = frame
                .files()
                .into_iter()
                .map(|f| {
                    let a = ContentAddress::from_validated(format!("blob:{f}")).unwrap();
                    (f, a)
                })
                .collect();
            let revision = Revision::from_validated("r1".to_owned()).unwrap();
            frame = frame.addressed(&addresses, Some(revision));
        }
        records.push(Record::Decision(
            Decision::approve(
                jid(1000 + subject),
                frame,
                None,
                actor(),
                JudgedAt::new(10),
                None,
            )
            .unwrap(),
        ));
    }
    ledger(records)
}

fn causes_of(assessment: &Assessment, n: u128) -> Vec<Cause> {
    match assessment.standing_of(docid(n)) {
        Standing::Approved(Freshness::Fresh) => Vec::new(),
        Standing::Approved(Freshness::Degraded(d)) => d.causes().iter().cloned().collect(),
        other => panic!("not an approval: {other:?}"),
    }
}

#[test]
fn an_artifact_that_is_unchanged_keeps_the_approval_fresh() {
    let s = spec_shelf(&["tests/a.rs"], &[("tests/a.rs", Some(h("a")))]);
    let l = approved_on(&s, false);
    assert!(causes_of(&assess(&s, &l, JudgedAt::new(100)), 2).is_empty());
}

#[test]
fn a_changed_artifact_makes_the_approval_stale_and_is_its_root() {
    let then = spec_shelf(&["tests/a.rs"], &[("tests/a.rs", Some(h("a")))]);
    let l = approved_on(&then, false);
    let now = spec_shelf(&["tests/a.rs"], &[("tests/a.rs", Some(h("b")))]);
    let assessment = assess(&now, &l, JudgedAt::new(100));
    let causes = causes_of(&assessment, 2);
    assert!(
        matches!(causes.as_slice(), [Cause::ArtifactChanged { pin, now }]
            if pin.path() == &path("tests/a.rs") && *now == h("b")),
        "{causes:?}"
    );
    assert_eq!(causes[0].severity(), Severity::Stale);
    assert!(
        assessment
            .roots_of(docid(2))
            .iter()
            .any(|r| matches!(r.cause, Cause::ArtifactChanged { .. }))
    );
}

#[test]
fn an_artifact_that_went_breaks_the_approval() {
    let then = spec_shelf(&["tests/a.rs"], &[("tests/a.rs", Some(h("a")))]);
    let l = approved_on(&then, false);
    let now = spec_shelf(&["tests/a.rs"], &[]);
    let causes = causes_of(&assess(&now, &l, JudgedAt::new(100)), 2);
    assert!(
        matches!(causes.as_slice(), [Cause::ArtifactGone { .. }]),
        "{causes:?}"
    );
    assert_eq!(causes[0].severity(), Severity::Broken);
}

#[test]
fn an_artifact_absent_then_and_now_changes_nothing() {
    let then = spec_shelf(&["tests/a.rs"], &[]);
    let l = approved_on(&then, false);
    assert!(causes_of(&assess(&then, &l, JudgedAt::new(100)), 2).is_empty());
}

#[test]
fn an_artifact_that_arrived_makes_the_approval_stale() {
    let then = spec_shelf(&["tests/a.rs"], &[]);
    let l = approved_on(&then, false);
    let now = spec_shelf(&["tests/a.rs"], &[("tests/a.rs", Some(h("a")))]);
    let causes = causes_of(&assess(&now, &l, JudgedAt::new(100)), 2);
    assert!(
        matches!(causes.as_slice(), [Cause::ArtifactArrived { now, .. }] if *now == h("a")),
        "{causes:?}"
    );
    assert_eq!(causes[0].severity(), Severity::Stale);
}

#[test]
fn a_path_written_or_dropped_since_is_a_change_of_the_links_to_artifacts() {
    let then = spec_shelf(&["tests/a.rs"], &[("tests/a.rs", Some(h("a")))]);
    let l = approved_on(&then, false);
    let now = spec_shelf(
        &["tests/b.rs"],
        &[("tests/a.rs", Some(h("a"))), ("tests/b.rs", Some(h("b")))],
    );
    let causes = causes_of(&assess(&now, &l, JudgedAt::new(100)), 2);
    match causes.as_slice() {
        [
            Cause::ArtifactLinksChanged {
                link,
                added,
                removed,
            },
        ] => {
            assert_eq!(link.as_str(), "tests");
            assert_eq!(added, &vec![path("tests/b.rs")]);
            assert_eq!(removed, &vec![path("tests/a.rs")]);
        }
        other => panic!("{other:?}"),
    }
    assert_eq!(causes[0].severity(), Severity::Stale);
}

#[test]
fn an_informs_link_to_an_artifact_changes_no_freshness() {
    let with_code = |hash: &str| {
        let mut spec = sp_testing(2, &["FR-1"], &[]);
        if let crate::domain::shelf::build::DocReading::Parsed { entries, .. } = &mut spec {
            entries.insert(
                "code".to_owned(),
                crate::domain::shelf::build::RawValue::List(vec!["src/a.rs".to_owned()]),
            );
        }
        shelf_with(vec![fr(1, &[]), spec], &[("src/a.rs", Some(h(hash)))])
    };
    let then = with_code("a");
    let l = approved_on(&then, false);
    assert!(causes_of(&assess(&with_code("b"), &l, JudgedAt::new(100)), 2).is_empty());
}

#[test]
fn addresses_and_revisions_never_change_a_freshness() {
    let then = spec_shelf(&["tests/a.rs"], &[("tests/a.rs", Some(h("a")))]);
    let now = spec_shelf(&["tests/a.rs"], &[("tests/a.rs", Some(h("b")))]);
    for when in [&then, &now] {
        let plain = assess(when, &approved_on(&then, false), JudgedAt::new(100));
        let addressed = assess(when, &approved_on(&then, true), JudgedAt::new(100));
        for n in [1, 2] {
            assert_eq!(
                plain.standing_of(docid(n)).kind(),
                addressed.standing_of(docid(n)).kind()
            );
        }
    }
}

#[test]
fn a_downstream_of_a_document_whose_artifact_changed_is_stale_through_it() {
    // FR-1 <- SP-2 (tested by tests/a.rs) <- the design `cache`.
    use crate::domain::shelf::fixtures_tests::{approved, reading, text};
    let design = |artifacts: &[(&str, Option<ArtifactHash>)]| {
        shelf_with(
            vec![
                fr(1, &[]),
                sp_testing(2, &["FR-1"], &["tests/a.rs"]),
                reading(
                    "designs/d.md",
                    3,
                    "cache",
                    &[
                        ("title", text("d")),
                        (
                            "specs",
                            crate::domain::shelf::build::RawValue::List(vec!["SP-2".to_owned()]),
                        ),
                    ],
                ),
            ],
            artifacts,
        )
    };
    let then = design(&[("tests/a.rs", Some(h("a")))]);
    let records = approved(&then, &[1, 2, 3]);
    let now = design(&[("tests/a.rs", Some(h("b")))]);
    let causes = causes_of(&assess(&now, &records, JudgedAt::new(100)), 3);
    assert!(
        causes
            .iter()
            .any(|c| matches!(c, Cause::Via { pin, severity: Severity::Stale } if pin.target() == docid(2))),
        "{causes:?}"
    );
}
