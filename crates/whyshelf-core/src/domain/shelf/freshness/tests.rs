//! Tests of freshness.

use crate::domain::shelf::build::DocProblem;
use crate::domain::shelf::fixtures_tests::{
    approve, approved, deny, deprecate, docid, fr, fr_titled, ledger, reading, retire, shelf,
};
use crate::domain::shelf::ledger::{JudgedAt, Record};

use super::*;

fn standing_of(assessment: &Assessment, n: u128) -> Standing {
    assessment.standings().get(&docid(n)).cloned().unwrap()
}

#[test]
fn an_approval_of_what_is_unchanged_holds() {
    let at_approval = shelf(vec![fr(1, &[]), fr(2, &["FR-1"])]);
    let ledger = approved(&at_approval, &[1, 2]);
    let assessment = assess(&at_approval, &ledger, JudgedAt::new(100));
    assert_eq!(
        standing_of(&assessment, 1),
        Standing::Approved(Freshness::Fresh)
    );
    assert_eq!(
        standing_of(&assessment, 2),
        Standing::Approved(Freshness::Fresh)
    );
}

#[test]
fn an_edited_upstream_degrades_what_stands_on_it_and_names_where_it_starts() {
    let at_approval = shelf(vec![fr(1, &[]), fr(2, &["FR-1"])]);
    let ledger = approved(&at_approval, &[1, 2]);
    let now = shelf(vec![fr_titled(1, "edited", &[]), fr(2, &["FR-1"])]);
    let assessment = assess(&now, &ledger, JudgedAt::new(100));

    let Standing::Approved(Freshness::Degraded(d)) = standing_of(&assessment, 1) else {
        panic!("the edited document is not what was approved");
    };
    let (level, causes) = (d.level(), d.causes());
    assert_eq!(level, Severity::Stale);
    assert!(
        causes
            .iter()
            .any(|cause| matches!(cause, Cause::SelfChanged { .. }))
    );

    let Standing::Approved(Freshness::Degraded(d)) = standing_of(&assessment, 2) else {
        panic!("what stands on an edited document is degraded");
    };
    let (level, causes) = (d.level(), d.causes());
    assert_eq!(level, Severity::Stale);
    assert!(causes.iter().any(
        |cause| matches!(cause, Cause::UpstreamChanged { pin, .. } if pin.target() == docid(1))
    ));
    let roots = assessment.roots_of(docid(2));
    assert!(
        roots
            .iter()
            .all(|root| !matches!(root.cause, Cause::Via { .. }))
    );
    assert!(roots.iter().any(|root| root.path.last() == Some(&docid(1))));
}

#[test]
fn an_unapproved_edit_upstream_is_one_root_not_two() {
    let at_approval = shelf(vec![fr(1, &[]), fr(2, &["FR-1"]), fr(3, &["FR-2"])]);
    let ledger = approved(&at_approval, &[1, 2, 3]);
    let now = shelf(vec![
        fr_titled(1, "edited", &[]),
        fr(2, &["FR-1"]),
        fr(3, &["FR-2"]),
    ]);
    let assessment = assess(&now, &ledger, JudgedAt::new(100));

    for (start, path) in [
        (2, vec![docid(2), docid(1)]),
        (3, vec![docid(3), docid(2), docid(1)]),
    ] {
        let roots = assessment.roots_of(docid(start));
        assert_eq!(roots.len(), 1, "one edit, one root: {roots:?}");
        assert_eq!(roots[0].path, path);
        assert!(matches!(roots[0].cause, Cause::SelfChanged { .. }));
    }
}

#[test]
fn an_edit_approved_again_is_still_a_root_of_what_saw_the_older_text() {
    let at_approval = shelf(vec![fr(1, &[]), fr(2, &["FR-1"])]);
    let first = approved(&at_approval, &[1, 2]);
    let now = shelf(vec![fr_titled(1, "edited", &[]), fr(2, &["FR-1"])]);
    let mut records: Vec<Record> = vec![
        approve(&at_approval, &ledger(Vec::new()), 1, 1001, 10, None),
        approve(&at_approval, &first, 2, 1002, 10, None),
    ];
    records.push(approve(&now, &ledger(records.clone()), 1, 1003, 20, None));
    let assessment = assess(&now, &ledger(records), JudgedAt::new(100));

    assert_eq!(
        standing_of(&assessment, 1),
        Standing::Approved(Freshness::Fresh)
    );
    let roots = assessment.roots_of(docid(2));
    assert_eq!(roots.len(), 1, "{roots:?}");
    assert_eq!(roots[0].path, vec![docid(2)]);
    assert!(matches!(
        roots[0].cause,
        Cause::UpstreamChanged { ref pin, .. } if pin.target() == docid(1)
    ));
}

#[test]
fn an_approval_lapses_at_its_expiry_and_not_before() {
    let shelf = shelf(vec![fr(1, &[])]);
    let ledger = ledger(vec![approve(
        &shelf,
        &ledger(vec![]),
        1,
        1001,
        10,
        Some(50),
    )]);
    let before = assess(&shelf, &ledger, JudgedAt::new(49));
    assert_eq!(
        standing_of(&before, 1),
        Standing::Approved(Freshness::Fresh)
    );
    let at = assess(&shelf, &ledger, JudgedAt::new(50));
    let Standing::Approved(Freshness::Degraded(d)) = standing_of(&at, 1) else {
        panic!("an approval lapses when now reaches its expiry");
    };
    let (level, causes) = (d.level(), d.causes());
    assert_eq!(level, Severity::Expired);
    assert!(
        causes
            .iter()
            .any(|cause| matches!(cause, Cause::Expired { .. }))
    );
}

#[test]
fn a_document_that_is_gone_after_its_approval_is_broken() {
    let at_approval = shelf(vec![fr(1, &[])]);
    let ledger = approved(&at_approval, &[1]);
    let now = shelf(vec![]);
    let assessment = assess(&now, &ledger, JudgedAt::new(100));
    let Standing::Approved(Freshness::Degraded(d)) = standing_of(&assessment, 1) else {
        panic!("an approved document with no file does not hold");
    };
    let (level, causes) = (d.level(), d.causes());
    assert_eq!(level, Severity::Broken);
    assert!(
        causes
            .iter()
            .any(|cause| matches!(cause, Cause::SelfNotValid(DocFailure::Gone)))
    );
}

#[test]
fn an_upstream_that_turned_invalid_breaks_what_stands_on_it_and_says_why() {
    let at_approval = shelf(vec![fr(1, &[]), fr(2, &["FR-1"])]);
    let ledger = approved(&at_approval, &[1, 2]);
    // FR-1 loses its required title.
    let now = shelf(vec![
        reading("findings/fr-1.md", 1, "FR-1", &[]),
        fr(2, &["FR-1"]),
    ]);
    let assessment = assess(&now, &ledger, JudgedAt::new(100));
    let Standing::Approved(Freshness::Degraded(d)) = standing_of(&assessment, 2) else {
        panic!("what stands on an invalid upstream does not hold");
    };
    assert_eq!(d.level(), Severity::Broken);
    assert!(d.causes().iter().any(|cause| matches!(
        cause,
        Cause::UpstreamNotValid { pin, failure: DocFailure::Invalid(problems) }
            if pin.target() == docid(1)
                && problems.iter().any(|p| matches!(p, DocProblem::FieldMissing { .. }))
    )));
}

#[test]
fn a_link_to_a_gone_upstream_is_said_once_and_another_key_is_a_change_of_links() {
    let at_approval = shelf(vec![fr(1, &[]), fr(2, &["FR-1"])]);
    let ledger = approved(&at_approval, &[1, 2]);
    let causes_of_2 = |now: &Shelf| {
        let Standing::Approved(Freshness::Degraded(d)) =
            standing_of(&assess(now, &ledger, JudgedAt::new(100)), 2)
        else {
            panic!("what stands on a gone upstream does not hold");
        };
        d.causes().clone().into_vec()
    };
    let gone = |c: &Cause| {
        matches!(c, Cause::UpstreamNotValid { pin, failure: DocFailure::Gone }
            if pin.target() == docid(1))
    };

    // FR-1 is gone and FR-2 still writes `FR-1`: the edge rule says it, and the
    // unresolved key, which is the pin's, is not said again.
    let causes = causes_of_2(&shelf(vec![fr(2, &["FR-1"])]));
    assert!(causes.iter().any(gone), "{causes:?}");
    assert!(
        !causes
            .iter()
            .any(|c| matches!(c, Cause::LinksChanged { .. })),
        "{causes:?}"
    );

    // FR-2 writes another key no document has: that is a change of the link.
    let causes = causes_of_2(&shelf(vec![fr(2, &["FR-9"])]));
    assert!(causes.iter().any(gone), "{causes:?}");
    assert!(
        causes.iter().any(|c| matches!(
            c,
            Cause::LinksChanged { added, removed, unresolved, .. }
                if added.is_empty() && removed.is_empty()
                    && unresolved.len() == 1 && unresolved[0].written() == "FR-9"
        )),
        "{causes:?}"
    );
}

#[test]
fn the_other_decisions_are_standings_without_a_freshness() {
    let shelf = shelf(vec![fr(1, &[]), fr(2, &[]), fr(3, &[])]);
    let ledger = ledger(vec![
        deny(&shelf, 1, 1001, 10),
        deprecate(&shelf, 2, 1002, 10),
        retire(&shelf, 3, 1003, 10),
    ]);
    let assessment = assess(&shelf, &ledger, JudgedAt::new(100));
    assert_eq!(standing_of(&assessment, 1), Standing::Denied);
    assert_eq!(
        standing_of(&assessment, 2),
        Standing::Deprecated {
            successors: Successors::None
        }
    );
    assert_eq!(standing_of(&assessment, 3), Standing::Retired);
}

#[test]
fn standing_of_a_listed_id_is_its_standing_and_of_an_unknown_id_is_pending() {
    let s = shelf(vec![fr(1, &[])]);
    let ledger = approved(&s, &[1]);
    let assessment = assess(&s, &ledger, JudgedAt::new(100));
    assert_eq!(
        assessment.standing_of(docid(1)),
        Standing::Approved(Freshness::Fresh)
    );
    assert!(!assessment.standings().contains_key(&docid(77)));
    assert_eq!(assessment.standing_of(docid(77)), Standing::Pending);
    assert!(assessment.notices_of(docid(77)).is_empty());
    assert!(assessment.notices_of(docid(1)).is_empty());
}

#[test]
fn an_upstream_approved_again_then_edited_again_is_two_roots_below() {
    // FR-2 approved on FR-1's first text; FR-1 then edited, approved again, and
    // edited again. FR-2 saw a text the re-approval replaced, and FR-1 has an edit
    // of its own since: two events.
    let first = shelf(vec![fr(1, &[]), fr(2, &["FR-1"])]);
    let second = shelf(vec![fr_titled(1, "second", &[]), fr(2, &["FR-1"])]);
    let third = shelf(vec![fr_titled(1, "third", &[]), fr(2, &["FR-1"])]);
    let mut records: Vec<Record> = vec![approve(&first, &ledger(Vec::new()), 1, 1001, 10, None)];
    records.push(approve(&first, &ledger(records.clone()), 2, 1002, 10, None));
    records.push(approve(
        &second,
        &ledger(records.clone()),
        1,
        1003,
        20,
        None,
    ));
    let assessment = assess(&third, &ledger(records), JudgedAt::new(100));

    let roots = assessment.roots_of(docid(2));
    assert!(
        roots.iter().any(|r| r.path == vec![docid(2)]
            && matches!(&r.cause, Cause::UpstreamChanged { pin, .. } if pin.target() == docid(1))),
        "the text FR-2 saw was replaced: {roots:?}"
    );
    assert!(
        roots
            .iter()
            .any(|r| r.path == vec![docid(2), docid(1)]
                && matches!(r.cause, Cause::SelfChanged { .. })),
        "FR-1's own edit since its approval: {roots:?}"
    );
    assert_eq!(roots.len(), 2, "{roots:?}");
}

/// FR-1 approved standing on FR-2 and FR-2 standing on FR-1 (a cycle of frames,
/// across which no Via passes), FR-3 standing on both from outside, and FR-2 then
/// edited.
fn a_cycle_with_an_edited_member() -> Assessment {
    let one = shelf(vec![fr(1, &["FR-2"]), fr(2, &[]), fr(3, &["FR-1", "FR-2"])]);
    let other = shelf(vec![fr(1, &[]), fr(2, &["FR-1"])]);
    let first = vec![
        approve(&one, &ledger(vec![]), 1, 1001, 10, None),
        approve(&other, &ledger(vec![]), 2, 1002, 10, None),
    ];
    let mut records = first.clone();
    records.push(approve(&one, &ledger(first), 3, 1003, 10, None));
    let now = shelf(vec![
        fr(1, &["FR-2"]),
        fr_titled(2, "edited", &[]),
        fr(3, &["FR-1", "FR-2"]),
    ]);
    assess(&now, &ledger(records), JudgedAt::new(100))
}

fn roots_naming_the_edit_of_fr_2(roots: &[RootCause]) -> usize {
    roots
        .iter()
        .filter(|r| match &r.cause {
            Cause::SelfChanged { .. } => r.path.last() == Some(&docid(2)),
            Cause::UpstreamChanged { pin, .. } => pin.target() == docid(2),
            _ => false,
        })
        .count()
}

#[test]
fn an_edit_inside_a_cycle_of_frames_is_a_root_of_the_member_that_pinned_it() {
    let assessment = a_cycle_with_an_edited_member();
    // With no Via to lead to FR-2, the change FR-1 sees is a root of FR-1 itself.
    let roots = assessment.roots_of(docid(1));
    assert_eq!(roots_naming_the_edit_of_fr_2(&roots), 1, "{roots:?}");
    assert!(
        roots
            .iter()
            .any(|r| matches!(r.cause, Cause::InCycle { .. }))
    );
}

#[test]
fn from_outside_a_cycle_one_edit_and_one_cycle_are_one_root_each() {
    let assessment = a_cycle_with_an_edited_member();
    // FR-3 reaches FR-2's edit directly and through FR-1, and the cycle through
    // both members: each event is named once.
    let roots = assessment.roots_of(docid(3));
    assert_eq!(roots_naming_the_edit_of_fr_2(&roots), 1, "{roots:?}");
    assert_eq!(
        roots
            .iter()
            .filter(|r| matches!(r.cause, Cause::InCycle { .. }))
            .count(),
        1,
        "{roots:?}"
    );
}

#[test]
fn every_kind_of_standing_is_listed_once_in_order() {
    // Naming each kind in a match keeps this test whole: a kind added breaks it.
    let named = |kind: StandingKind| match kind {
        StandingKind::Pending => 0,
        StandingKind::Denied => 1,
        StandingKind::Deprecated => 2,
        StandingKind::Retired => 3,
        StandingKind::Approved(FreshnessLevel::Fresh) => 4,
        StandingKind::Approved(FreshnessLevel::Expired) => 5,
        StandingKind::Approved(FreshnessLevel::Stale) => 6,
        StandingKind::Approved(FreshnessLevel::Broken) => 7,
    };
    let positions: Vec<usize> = StandingKind::ALL.into_iter().map(named).collect();
    assert_eq!(positions, (0..8).collect::<Vec<_>>());
    assert!(StandingKind::ALL.windows(2).all(|w| w[0] < w[1]));
}
