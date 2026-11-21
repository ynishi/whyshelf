//! Tests of a quorum beside a freshness: a fresh approval short of names is shown
//! awaiting, and is no degradation; a degraded one keeps its level; an upstream short
//! of names, expired or not, makes what stands on it stale as a pending one does
//! ([`UpstreamStatus::Unseconded`]) and holds it back from a run
//! ([`Hold::Awaiting`]), and in a project that declares no rule the same records
//! change nothing.

use std::collections::BTreeSet;

use crate::domain::actor::Actor;
use crate::domain::shelf::fixtures_tests::{
    admitted, approve_as, asking, docid, fr, fr_titled, ledger, ruled_shelf,
};
use crate::domain::shelf::ledger::{ReaderCount, Record};
use crate::domain::shelf::order::{Hold, held_back, stale};

use super::assess_unasked as assess;
use super::*;

/// An approval of `subject` on `shelf` (who, when, until when), made after `before`.
fn approval(
    shelf: &Shelf,
    before: &[Record],
    (subject, judgement): (u128, u128),
    read: (&str, i64, Option<i64>),
) -> Record {
    let so_far = admitted(shelf, before.to_vec());
    approve_as(shelf, &so_far, subject, judgement, read)
}

/// Where `n` stands at time 100.
fn standing(shelf: &Shelf, ledger: &Ledger, n: u128) -> Standing {
    assess(shelf, ledger, JudgedAt::new(100)).standing_of(docid(n))
}

/// The names of a reading, in order.
fn names(set: &BTreeSet<Actor>) -> Vec<&str> {
    set.iter().map(Actor::as_str).collect()
}

#[test]
fn only_an_approval_has_readers() {
    for standing in [Standing::Pending, Standing::Denied, Standing::Retired] {
        assert_eq!(standing.read_by(), None, "{standing:?}");
    }
    let fresh = Standing::fresh();
    assert_eq!(fresh.read_by().map(ReadBy::count), Some(ReaderCount::One));
}

#[test]
fn an_approval_is_read_by_the_names_that_approved_its_content_in_every_project() {
    let shelf = ruled_shelf(asking(2, None), vec![fr(1, &[])]);
    let records = vec![
        approval(&shelf, &[], (1, 1001), ("ana", 10, None)),
        approval(&shelf, &[], (1, 1002), ("bo", 20, None)),
    ];
    // A project that declares no rule reads the same names, and its quorum is met.
    let unruled = standing(&shelf, &ledger(records.clone()), 1);
    let read_by = unruled.read_by().unwrap();
    assert_eq!(names(read_by.counted()), ["ana", "bo"]);
    assert_eq!(read_by.count(), ReaderCount::Several);
    assert_eq!(unruled.quorum(), Some(&Quorum::Met));
    // So does one that asks for two names.
    let ruled = standing(&shelf, &admitted(&shelf, records), 1);
    assert_eq!(ruled.read_by(), unruled.read_by());
}

#[test]
fn an_expired_approval_read_by_two_names_is_read_by_nobody_while_its_quorum_is_short() {
    let shelf = ruled_shelf(asking(2, None), vec![fr(1, &[])]);
    // ana until 50, then bo until 60, whose approval stands: both lapsed at 100.
    let ana = approval(&shelf, &[], (1, 1001), ("ana", 10, Some(50)));
    let bo = approval(
        &shelf,
        std::slice::from_ref(&ana),
        (1, 1002),
        ("bo", 20, Some(60)),
    );
    let ledger = admitted(&shelf, vec![ana, bo]);
    let expired = standing(&shelf, &ledger, 1);

    let read_by = expired.read_by().unwrap();
    assert_eq!(read_by.count(), ReaderCount::Nobody);
    assert!(read_by.counted().is_empty());
    assert_eq!(names(read_by.lapsed()), ["ana", "bo"]);
    // The name of the approval that stands counts alone in the quorum, and in no
    // reading.
    let Some(Quorum::Short { readers, need }) = expired.quorum() else {
        panic!("short: {expired:?}");
    };
    assert_eq!(
        readers.iter().map(Actor::as_str).collect::<Vec<_>>(),
        ["bo"]
    );
    assert_eq!(need.count(), 2);
    assert_eq!(
        expired.kind(),
        StandingKind::Approved(FreshnessLevel::Expired)
    );
}

#[test]
fn a_fresh_approval_short_of_names_is_shown_awaiting_and_a_degraded_one_keeps_its_level() {
    let then = ruled_shelf(asking(2, None), vec![fr(1, &[])]);
    let one = approval(&then, &[], (1, 1001), ("ana", 10, None));
    let ledger = admitted(&then, vec![one]);
    let awaiting = standing(&then, &ledger, 1);
    // Its freshness is fresh, and knows nothing of the names.
    assert_eq!(awaiting.freshness(), Some(&Freshness::Fresh));
    let level = awaiting.freshness().map(Freshness::level);
    assert_eq!(level, Some(FreshnessLevel::Fresh));
    assert!(matches!(awaiting.quorum(), Some(Quorum::Short { .. })));
    let shown = StandingKind::Approved(FreshnessLevel::Awaiting);
    assert_eq!(awaiting.kind(), shown);
    // `--stale` does not take it: it is not degraded.
    let assessment = assess(&then, &ledger, JudgedAt::new(100));
    assert!(stale(&assessment).again.is_empty());

    // Edited: stale, shown at the level of its causes, and short still.
    let now = ruled_shelf(asking(2, None), vec![fr_titled(1, "edited", &[])]);
    let edited = standing(&now, &ledger, 1);
    let shown = StandingKind::Approved(FreshnessLevel::Stale);
    assert_eq!(edited.kind(), shown);
    assert!(matches!(edited.quorum(), Some(Quorum::Short { .. })));
}

#[test]
fn an_upstream_short_of_names_stales_and_holds_back_what_stands_on_it_even_expired() {
    let shelf = ruled_shelf(asking(2, None), vec![fr(1, &[]), fr(2, &["FR-1"])]);
    // FR-1 by ana alone, until 50; FR-2 by ana and bo.
    let mut records = vec![approval(&shelf, &[], (1, 1001), ("ana", 10, Some(50)))];
    for (judgement, by) in [(1002, "ana"), (1003, "bo")] {
        let next = approval(&shelf, &records, (2, judgement), (by, 11, None));
        records.push(next);
    }
    let ruled = admitted(&shelf, records.clone());
    let assessment = assess(&shelf, &ruled, JudgedAt::new(100));

    // FR-1 lapsed and is short: shown expired, its quorum beside it.
    let upstream = assessment.standing_of(docid(1));
    let shown = StandingKind::Approved(FreshnessLevel::Expired);
    assert_eq!(upstream.kind(), shown);
    assert!(matches!(upstream.quorum(), Some(Quorum::Short { .. })));

    // FR-2 has its two names, and is stale through FR-1, as on a pending upstream.
    let downstream = assessment.standing_of(docid(2));
    assert_eq!(downstream.quorum(), Some(&Quorum::Met));
    let Some(Freshness::Degraded(d)) = downstream.freshness() else {
        panic!("what stands on an unseconded upstream is stale: {downstream:?}");
    };
    assert_eq!(d.level(), Severity::Stale);
    assert!(d.causes().iter().any(|cause| matches!(
        cause,
        Cause::UpstreamDeclared {
            status: UpstreamStatus::Unseconded,
            ..
        }
    )));

    // `--stale` takes both, and FR-1, expired, holds FR-2 back: its quorum is read
    // before the expiry would let it through.
    let run = stale(&assessment).again;
    assert_eq!(run, BTreeSet::from([docid(1), docid(2)]));
    let held = held_back(&shelf, &assessment, docid(2), &run, &BTreeSet::new()).unwrap();
    assert_eq!((held.upstream, held.hold), (docid(1), Hold::Awaiting));

    // The same records, read in a project that declares no rule: FR-1 only expired,
    // and FR-2 holds, with a notice.
    let assessment = assess(&shelf, &ledger(records), JudgedAt::new(100));
    assert!(assessment.standing_of(docid(2)).is_fresh_met());
    let run = stale(&assessment).again;
    assert_eq!(run, BTreeSet::from([docid(1)]));
    let held = held_back(&shelf, &assessment, docid(2), &run, &BTreeSet::new());
    assert_eq!(held, None);
}
