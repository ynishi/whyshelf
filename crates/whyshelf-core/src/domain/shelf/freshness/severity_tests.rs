//! Tests of how strong each cause is ([`Cause::severity`]), of the level of a
//! degradation ([`Degradation::level`]), and of a cycle of frames.

use crate::domain::shelf::fixtures_tests::{
    approve, approved, deny, docid, fr, fr_titled, ledger, retire, shelf,
};
use crate::domain::shelf::graph::CycleSource;
use crate::domain::shelf::ledger::{DeclaredStatus, JudgedAt};

use super::*;

/// The pin FR-2's approval keeps of FR-1, from a real approval.
fn pin_of_fr1() -> FramePin {
    let shelf = shelf(vec![fr(1, &[]), fr(2, &["FR-1"])]);
    match approved(&shelf, &[2]).status_of(docid(2)) {
        DeclaredStatus::Approved { frame, .. } => frame.into_iter().next().unwrap(),
        other => panic!("an approval, got {other:?}"),
    }
}

fn declared(status: UpstreamStatus) -> Cause {
    Cause::UpstreamDeclared {
        pin: pin_of_fr1(),
        status,
    }
}

/// The degradation of a standing that is a degraded approval.
fn degraded(d: Standing) -> Degradation {
    match d {
        Standing::Approved(Freshness::Degraded(d)) => d,
        other => panic!("a degraded approval, got {other:?}"),
    }
}

#[test]
fn a_cycle_is_broken() {
    for source in [CycleSource::Links, CycleSource::Frames] {
        let cause = Cause::InCycle {
            cycle: CycleId {
                source,
                min: docid(1),
            },
        };
        assert_eq!(cause.severity(), Severity::Broken, "{source:?}");
    }
}

#[test]
fn an_upstream_denied_or_retired_is_broken_and_one_pending_is_stale() {
    assert_eq!(
        declared(UpstreamStatus::Denied).severity(),
        Severity::Broken
    );
    assert_eq!(
        declared(UpstreamStatus::Retired).severity(),
        Severity::Broken
    );
    assert_eq!(
        declared(UpstreamStatus::Pending).severity(),
        Severity::Stale
    );
}

#[test]
fn a_denied_or_retired_upstream_breaks_what_stands_on_it() {
    let shelf = shelf(vec![fr(1, &[]), fr(2, &["FR-1"])]);
    let first = approve(&shelf, &ledger(vec![]), 1, 1001, 10, None);
    let second = approve(&shelf, &ledger(vec![first.clone()]), 2, 1002, 10, None);
    for (what, decision, status) in [
        ("denied", deny(&shelf, 1, 1003, 20), UpstreamStatus::Denied),
        (
            "retired",
            retire(&shelf, 1, 1003, 20),
            UpstreamStatus::Retired,
        ),
    ] {
        let ledger = ledger(vec![first.clone(), second.clone(), decision]);
        let d = degraded(assess(&shelf, &ledger, JudgedAt::new(100)).standing_of(docid(2)));
        assert_eq!(d.level(), Severity::Broken, "{what}");
        assert!(
            d.causes().iter().any(|c| matches!(
                c,
                Cause::UpstreamDeclared { status: s, .. } if *s == status
            )),
            "{what}: {:?}",
            d.causes()
        );
    }
}

#[test]
fn an_approval_on_a_pending_upstream_is_stale_from_the_start() {
    let shelf = shelf(vec![fr(1, &[]), fr(2, &["FR-1"])]);
    let ledger = approved(&shelf, &[2]);
    let d = degraded(assess(&shelf, &ledger, JudgedAt::new(100)).standing_of(docid(2)));
    assert_eq!(d.level(), Severity::Stale);
    assert!(d.causes().iter().any(|c| matches!(
        c,
        Cause::UpstreamDeclared {
            status: UpstreamStatus::Pending,
            ..
        }
    )));
}

#[test]
fn a_via_takes_the_strength_it_carries() {
    for severity in [Severity::Stale, Severity::Broken] {
        let cause = Cause::Via {
            pin: pin_of_fr1(),
            severity,
        };
        assert_eq!(cause.severity(), severity);
    }
}

#[test]
fn a_via_of_a_broken_upstream_is_broken() {
    let at_approval = shelf(vec![fr(1, &[]), fr(2, &["FR-1"]), fr(3, &["FR-2"])]);
    let ledger = approved(&at_approval, &[1, 2, 3]);
    // FR-1 is gone: FR-2 is broken, and FR-3 is broken through it.
    let now = shelf(vec![fr(2, &["FR-1"]), fr(3, &["FR-2"])]);
    let assessment = assess(&now, &ledger, JudgedAt::new(100));
    assert_eq!(
        degraded(assessment.standing_of(docid(2))).level(),
        Severity::Broken
    );
    let d = degraded(assessment.standing_of(docid(3)));
    assert_eq!(d.level(), Severity::Broken);
    assert!(
        d.causes().iter().any(|c| matches!(
            c,
            Cause::Via { pin, severity: Severity::Broken } if pin.target() == docid(2)
        )),
        "{:?}",
        d.causes()
    );
}

#[test]
fn a_change_of_links_is_broken_with_a_key_that_cannot_be_followed_and_stale_without() {
    let at_approval = shelf(vec![fr(1, &[]), fr(2, &["FR-1"]), fr(3, &[])]);
    let ledger = approved(&at_approval, &[1, 2, 3]);
    let links_changed = |now: &Shelf| -> (Cause, Severity) {
        let d = degraded(assess(now, &ledger, JudgedAt::new(100)).standing_of(docid(2)));
        let cause = d
            .causes()
            .iter()
            .find(|c| matches!(c, Cause::LinksChanged { .. }))
            .cloned()
            .unwrap_or_else(|| panic!("a change of links: {:?}", d.causes()));
        (cause, d.level())
    };

    // FR-2 now also names FR-9, which no document has.
    let (cause, level) = links_changed(&shelf(vec![
        fr(1, &[]),
        fr(2, &["FR-1", "FR-9"]),
        fr(3, &[]),
    ]));
    assert!(
        matches!(&cause, Cause::LinksChanged { unresolved, .. } if unresolved.len() == 1),
        "{cause:?}"
    );
    assert_eq!(cause.severity(), Severity::Broken);
    assert_eq!(level, Severity::Broken);

    // FR-2 now also names FR-3, which is there.
    let (cause, level) = links_changed(&shelf(vec![
        fr(1, &[]),
        fr(2, &["FR-1", "FR-3"]),
        fr(3, &[]),
    ]));
    assert!(
        matches!(&cause, Cause::LinksChanged { added, unresolved, .. }
            if *added == vec![docid(3)] && unresolved.is_empty()),
        "{cause:?}"
    );
    assert_eq!(cause.severity(), Severity::Stale);
    assert_eq!(level, Severity::Stale);
}

#[test]
fn the_level_of_a_degradation_is_its_strongest_cause_in_any_order() {
    let a = *shelf(vec![fr(1, &[])]).doc(docid(1)).unwrap().hash();
    let b = *shelf(vec![fr_titled(1, "edited", &[])])
        .doc(docid(1))
        .unwrap()
        .hash();
    let expired = Cause::Expired {
        since: JudgedAt::new(50),
    };
    let stale = Cause::SelfChanged {
        approved: a,
        now: b,
    };
    let broken = Cause::InCycle {
        cycle: CycleId {
            source: CycleSource::Frames,
            min: docid(1),
        },
    };
    let level = |causes: Vec<Cause>| match Freshness::from_causes(causes) {
        Freshness::Degraded(d) => d.level(),
        Freshness::Fresh => panic!("causes make a degradation"),
    };

    assert_eq!(level(vec![expired.clone()]), Severity::Expired);
    assert_eq!(level(vec![expired.clone(), stale.clone()]), Severity::Stale);
    assert_eq!(level(vec![stale.clone(), expired.clone()]), Severity::Stale);
    assert_eq!(
        level(vec![expired.clone(), broken.clone(), stale.clone()]),
        Severity::Broken
    );
    assert_eq!(level(vec![broken, expired, stale]), Severity::Broken);
    assert_eq!(Freshness::from_causes(vec![]), Freshness::Fresh);
}

#[test]
fn documents_in_a_cycle_of_frames_are_in_cycle_with_no_via_between_them() {
    // Two worktrees: in one FR-1 stands on FR-2, in the other FR-2 on FR-1, and each
    // approved its own on a ledger that did not have the other's record. Together
    // the frames make a cycle, though no shelf had a cycle of links.
    let one = shelf(vec![fr(1, &["FR-2"]), fr(2, &[])]);
    let other = shelf(vec![fr(1, &[]), fr(2, &["FR-1"])]);
    let ledger = ledger(vec![
        approve(&one, &ledger(vec![]), 1, 1001, 10, None),
        approve(&other, &ledger(vec![]), 2, 1002, 10, None),
    ]);
    let assessment = assess(&one, &ledger, JudgedAt::new(100));

    assert!(
        assessment
            .cycles()
            .iter()
            .any(|c| c.id().source == CycleSource::Frames
                && c.contains(docid(1))
                && c.contains(docid(2))),
        "{:?}",
        assessment.cycles()
    );
    for n in [1, 2] {
        let d = degraded(assessment.standing_of(docid(n)));
        assert_eq!(d.level(), Severity::Broken, "FR-{n}");
        assert!(
            d.causes().iter().any(|c| matches!(
                c,
                Cause::InCycle { cycle } if cycle.source == CycleSource::Frames
            )),
            "FR-{n}: {:?}",
            d.causes()
        );
        assert!(
            !d.causes().iter().any(|c| matches!(c, Cause::Via { .. })),
            "FR-{n}: {:?}",
            d.causes()
        );
    }
}
