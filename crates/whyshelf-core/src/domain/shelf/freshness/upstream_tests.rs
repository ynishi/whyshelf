//! Tests of what a decision about an upstream means for what stands on it: one row
//! for each way an upstream stands (no decision, an approval at each freshness and
//! quorum, and each decision against it), and for each row every reading of it, so that
//! a rule that moves moves the same row of every column and a column that disagrees
//! with the others fails here.
//!
//! The columns: whether `frame_approve` refuses a new approval on it
//! ([`NotStanding`]), and `Ledger::may_stand_on` with it, whether it counts, why an
//! exemption of it is unneeded, what it is to what stands on it ([`UpstreamReading`]),
//! the causes that come of it with their strength, the events at the origins
//! ([`OriginEvent`]), whether a run holds what stands on it back ([`held_back`], with
//! the upstream in the run and out of it, and left out of it), whether what `--stale`
//! is foreseen to approve again takes what stands on it ([`stale_taken`]), and whether
//! approving again settles what it makes stale ([`Settlement::ApproveStale`]).

use std::collections::BTreeSet;

use crate::domain::shelf::artifact::{ArtifactReadings, ListedNames, Listings};
use crate::domain::shelf::build::DocReading;
use crate::domain::shelf::cover::{counts, decided_of};
use crate::domain::shelf::exemption::{ExemptionReading, Unneeded, exemption_reading};
use crate::domain::shelf::fixtures_tests::{
    admitted, approve, approve_as, approved, asking, decls, deny, deprecate, docid, fr, fr_titled,
    ledger, list, reading, retire, ruled_shelf, shelf, spaces, text,
};
use crate::domain::shelf::ledger::{ExemptOf, FrameRefusal, Record, frame_approve};
use crate::domain::shelf::order::{Hold, held_back, stale_taken};
use crate::domain::shelf::schema::{CoverDecl, Schema};
use crate::domain::shelf::settle::{Settlement, settlements};

use super::assess_unasked as assess;
use super::*;

/// A cause that comes of the upstream, without the pin.
#[derive(Debug, PartialEq)]
enum Told {
    Declared(UpstreamStatus),
    Via(Carried),
}

/// What the readings of the upstream FR-1 are, for what stands on it (FR-2).
struct Expect {
    /// What `frame_approve` of FR-2 refuses on FR-1, if it does.
    refused: Option<NotStanding>,
    /// Whether FR-1 counts.
    counts: bool,
    /// What the standing of FR-1 is to FR-2.
    reading: UpstreamReading,
    /// The causes of FR-2 that come of FR-1, and how strong each is.
    told: Vec<(Told, Severity)>,
    /// The events of every origin.
    events: Vec<OriginEvent>,
    /// What holds a run back, with FR-1 out of the run and in it.
    held: (Option<Hold>, Option<Hold>),
    /// Whether `--stale` is foreseen to approve FR-2 again: only where it is stale,
    /// nothing holds it back and the frame does not refuse it.
    taken: bool,
    /// Whether approving again settles what the origins hold.
    approving_again_settles: bool,
}

/// One way an upstream stands, as the shelf is now and the ledger of the decisions.
struct Case {
    what: &'static str,
    now: Vec<DocReading>,
    shelf: Shelf,
    ledger: Ledger,
}

fn unapproved(unapproved: Unapproved) -> UpstreamReading {
    UpstreamReading::Unapproved(unapproved)
}

fn refused(refused: Refused) -> Unapproved {
    Unapproved::Refused(refused)
}

fn declared(unapproved: Unapproved) -> Told {
    Told::Declared(UpstreamStatus::Unapproved(unapproved))
}

fn approval(awaiting: bool, carried: Option<Carried>) -> UpstreamReading {
    UpstreamReading::Approval { awaiting, carried }
}

/// FR-2 stands on FR-1, with nothing else on the shelf.
fn plain() -> Vec<DocReading> {
    vec![fr(1, &[]), fr(2, &["FR-1"])]
}

/// The approvals of FR-1 and then FR-2 at time 10, upstream first.
fn both_approved(shelf: &Shelf) -> Vec<Record> {
    let first = approve(shelf, &ledger(vec![]), 1, 1001, 10, None);
    let second = approve(shelf, &ledger(vec![first.clone()]), 2, 1002, 10, None);
    vec![first, second]
}

/// A shelf of `readings`, with `decision` made about FR-1 after both approvals.
fn decided_about_fr1(
    what: &'static str,
    readings: Vec<DocReading>,
    decision: impl Fn(&Shelf) -> Record,
) -> Case {
    let shelf = shelf(readings.clone());
    let mut records = both_approved(&shelf);
    records.push(decision(&shelf));
    Case {
        what,
        now: readings,
        ledger: ledger(records),
        shelf,
    }
}

/// FR-3 replaces FR-1.
fn replacing_fr1() -> Vec<DocReading> {
    let mut readings = plain();
    readings.push(reading(
        "findings/fr-3.md",
        3,
        "FR-3",
        &[("title", text("title")), ("supersedes", list(&["FR-1"]))],
    ));
    readings
}

/// FR-2 stands on FR-1, which needs two names: FR-1 is approved by `ana` alone, and
/// FR-2 by two names.
fn short_upstream(now: Vec<DocReading>, then: Vec<DocReading>) -> Case {
    let decide = || asking(2, None);
    let before = ruled_shelf(decide(), then);
    let mut records = vec![approve_as(
        &before,
        &admitted(&before, vec![]),
        1,
        1001,
        ("ana", 10, None),
    )];
    for (judgement, by) in [(1002, "ana"), (1003, "bo")] {
        let so_far = admitted(&before, records.clone());
        records.push(approve_as(&before, &so_far, 2, judgement, (by, 11, None)));
    }
    let shelf = ruled_shelf(decide(), now.clone());
    Case {
        what: "",
        now,
        ledger: admitted(&shelf, records),
        shelf,
    }
}

fn cases() -> Vec<(Case, Expect)> {
    let edited = vec![fr_titled(1, "edited", &[]), fr(2, &["FR-1"])];

    let pending = {
        let shelf = shelf(plain());
        Case {
            what: "no decision stands about the upstream",
            now: plain(),
            ledger: approved(&shelf, &[2]),
            shelf,
        }
    };
    let fresh = {
        let shelf = shelf(plain());
        Case {
            what: "approved, fresh and its quorum met",
            now: plain(),
            ledger: approved(&shelf, &[1, 2]),
            shelf,
        }
    };
    let short = Case {
        what: "approved and short of names",
        ..short_upstream(plain(), plain())
    };
    let short_and_stale = Case {
        what: "approved, short of names and stale",
        ..short_upstream(edited.clone(), plain())
    };
    let stale = {
        let then = shelf(plain());
        let records = both_approved(&then);
        Case {
            what: "approved and stale",
            now: edited.clone(),
            shelf: shelf(edited),
            ledger: ledger(records),
        }
    };
    let broken = {
        let readings = vec![fr(3, &[]), fr(1, &["FR-3"]), fr(2, &["FR-1"])];
        let shelf = shelf(readings.clone());
        let mut records = Vec::new();
        for (subject, judgement) in [(3, 1003), (1, 1001), (2, 1002)] {
            let next = approve(
                &shelf,
                &ledger(records.clone()),
                subject,
                judgement,
                10,
                None,
            );
            records.push(next);
        }
        records.push(deny(&shelf, 3, 1004, 20));
        Case {
            what: "approved and broken",
            now: readings,
            ledger: ledger(records),
            shelf,
        }
    };
    let expired = {
        let shelf = shelf(plain());
        let first = approve(&shelf, &ledger(vec![]), 1, 1001, 10, Some(50));
        let second = approve(&shelf, &ledger(vec![first.clone()]), 2, 1002, 10, None);
        Case {
            what: "approved and expired, nothing else",
            now: plain(),
            ledger: ledger(vec![first, second]),
            shelf,
        }
    };
    let denied = decided_about_fr1("denied", plain(), |s| deny(s, 1, 1003, 20));
    let superseded = decided_about_fr1("deprecated, with a successor", replacing_fr1(), |s| {
        deprecate(s, 1, 1003, 20)
    });
    let unreplaced = decided_about_fr1("deprecated, with no successor", plain(), |s| {
        deprecate(s, 1, 1003, 20)
    });
    let retired = decided_about_fr1("retired", plain(), |s| retire(s, 1, 1003, 20));

    let nothing = |reading| Expect {
        refused: None,
        counts: true,
        reading,
        told: Vec::new(),
        events: Vec::new(),
        held: (None, None),
        taken: false,
        approving_again_settles: false,
    };
    let against = |status, reading, told: Told, severity, event| Expect {
        refused: Some(status),
        counts: false,
        reading,
        told: vec![(told, severity)],
        events: vec![event],
        held: (None, None),
        taken: false,
        approving_again_settles: false,
    };

    vec![
        (
            pending,
            Expect {
                told: vec![(declared(Unapproved::Pending), Severity::Stale)],
                events: vec![OriginEvent::UpstreamPending],
                held: (Some(Hold::Pending), None),
                ..nothing(unapproved(Unapproved::Pending))
            },
        ),
        (fresh, nothing(approval(false, None))),
        (
            short,
            Expect {
                told: vec![(Told::Declared(UpstreamStatus::Unseconded), Severity::Stale)],
                events: vec![OriginEvent::UpstreamAwaiting],
                held: (Some(Hold::Awaiting), Some(Hold::Awaiting)),
                ..nothing(approval(true, None))
            },
        ),
        (
            short_and_stale,
            Expect {
                told: vec![
                    (Told::Declared(UpstreamStatus::Unseconded), Severity::Stale),
                    (Told::Via(Carried::Stale), Severity::Stale),
                ],
                events: vec![OriginEvent::Edited, OriginEvent::UpstreamAwaiting],
                // Stale and out of the run holds it back first; in the run, the
                // names it still awaits do.
                held: (Some(Hold::Stale), Some(Hold::Awaiting)),
                approving_again_settles: true,
                ..nothing(approval(true, Some(Carried::Stale)))
            },
        ),
        (
            stale,
            Expect {
                told: vec![(Told::Via(Carried::Stale), Severity::Stale)],
                events: vec![OriginEvent::Edited],
                held: (Some(Hold::Stale), None),
                taken: true,
                approving_again_settles: true,
                ..nothing(approval(false, Some(Carried::Stale)))
            },
        ),
        (
            broken,
            Expect {
                told: vec![(Told::Via(Carried::Broken), Severity::Broken)],
                events: vec![OriginEvent::UpstreamRefused],
                held: (Some(Hold::Broken), Some(Hold::Broken)),
                ..nothing(approval(false, Some(Carried::Broken)))
            },
        ),
        (
            expired,
            Expect {
                events: vec![OriginEvent::Expired],
                ..nothing(approval(false, None))
            },
        ),
        (
            denied,
            against(
                NotStanding::Denied,
                unapproved(refused(Refused::Denied)),
                declared(refused(Refused::Denied)),
                Severity::Broken,
                OriginEvent::UpstreamRefused,
            ),
        ),
        (
            superseded,
            against(
                NotStanding::Deprecated,
                unapproved(Unapproved::Superseded {
                    successors: NonEmptyVec::new(docid(3), vec![]),
                }),
                declared(Unapproved::Superseded {
                    successors: NonEmptyVec::new(docid(3), vec![]),
                }),
                Severity::Stale,
                OriginEvent::UpstreamSuperseded,
            ),
        ),
        (
            unreplaced,
            against(
                NotStanding::Deprecated,
                unapproved(refused(Refused::Unreplaced)),
                declared(refused(Refused::Unreplaced)),
                Severity::Broken,
                OriginEvent::UpstreamRefused,
            ),
        ),
        (
            retired,
            against(
                NotStanding::Retired,
                unapproved(refused(Refused::Retired)),
                declared(refused(Refused::Retired)),
                Severity::Broken,
                OriginEvent::UpstreamRefused,
            ),
        ),
    ]
}

/// The fixtures' schema with `cover = true` on the spec's `grounds`, which covers the
/// findings: FR-1 is expected to be grounded by a spec.
fn covering() -> Schema {
    let mut decls = decls();
    decls
        .iter_mut()
        .find(|d| d.name == "spec")
        .and_then(|d| d.links.iter_mut().find(|k| k.name == "grounds"))
        .expect("the fixtures declare it")
        .cover = Some(CoverDecl::Truth(true));
    Schema::validate(decls, &spaces()).unwrap()
}

#[test]
fn what_a_decision_about_an_upstream_is_to_what_stands_on_it_is_read_once() {
    for (case, expect) in cases() {
        let Case {
            what,
            now,
            shelf,
            ledger,
        } = case;
        let assessment = assess(&shelf, &ledger, JudgedAt::new(100));
        let upstream = assessment.standing_of(docid(1));
        let downstream = assessment.standing_of(docid(2));

        // The refusal of a new approval on it.
        let refusal = match frame_approve(&shelf, &ledger, docid(2)) {
            Ok(_) => None,
            Err(FrameRefusal::UpstreamNotStanding { status, .. }) => Some(status),
            Err(other) => panic!("{what}: {other:?}"),
        };
        assert_eq!(refusal, expect.refused, "{what}: frame_approve");
        assert_eq!(
            ledger.may_stand_on(docid(1)).err(),
            expect.refused,
            "{what}: may_stand_on"
        );

        // Whether it counts, as the ledger declares it and as it stands.
        assert_eq!(
            counts(decided_of(&ledger, docid(1))),
            expect.counts,
            "{what}: counts"
        );
        assert_eq!(upstream.stands(), expect.counts, "{what}: stands");

        // Why an exemption of it from a link that covers it is unneeded.
        let covered = Shelf::build(&covering(), now, &ArtifactReadings::new());
        let of = ExemptOf::new(
            docid(1),
            BookName::new("spec").unwrap(),
            EntryName::new("grounds").unwrap(),
        );
        let exemption = match exemption_reading(
            &covered,
            &ledger,
            &of,
            &ListedNames::of(&Listings::NotAsked),
        ) {
            ExemptionReading::Needed(_) => None,
            ExemptionReading::Unneeded(Unneeded::DecidedAgainst(status)) => Some(status),
            other => panic!("{what}: {other:?}"),
        };
        assert_eq!(exemption, expect.refused, "{what}: exemption");

        // What it is to what stands on it, and the causes that come of that.
        assert_eq!(
            UpstreamReading::of(&upstream),
            expect.reading,
            "{what}: reading"
        );
        let told: Vec<(Told, Severity)> = match downstream.freshness() {
            Some(Freshness::Degraded(d)) => d
                .causes()
                .iter()
                .filter_map(|cause| match cause {
                    Cause::UpstreamDeclared { pin, status } if pin.target() == docid(1) => {
                        Some((Told::Declared(status.clone()), cause.severity()))
                    }
                    Cause::Via { pin, carried } if pin.target() == docid(1) => {
                        Some((Told::Via(*carried), cause.severity()))
                    }
                    _ => None,
                })
                .collect(),
            _ => Vec::new(),
        };
        assert_eq!(told, expect.told, "{what}: causes");

        // The events at the origins, and whether approving again settles any.
        let groups = assessment.origins();
        let events: Vec<OriginEvent> = groups
            .iter()
            .flat_map(OriginGroup::events)
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect();
        assert_eq!(events, expect.events, "{what}: events");
        let degraded: BTreeSet<DocId> = groups
            .iter()
            .flat_map(|group| group.degraded.iter().copied())
            .collect();
        let settles = groups
            .iter()
            .any(|group| settlements(group, &degraded).contains(&Settlement::ApproveStale));
        assert_eq!(
            settles, expect.approving_again_settles,
            "{what}: approving again"
        );

        // What holds a run back, with FR-1 out of the run and in it, and with it left
        // out of the run: after every reading that holds nothing back, that is all.
        let held = |run: &[u128], left_out: &[u128]| {
            let ids = |ns: &[u128]| -> BTreeSet<DocId> { ns.iter().map(|n| docid(*n)).collect() };
            held_back(&shelf, &assessment, docid(2), &ids(run), &ids(left_out)).map(|held| {
                assert_eq!(held.upstream, docid(1), "{what}");
                held.hold
            })
        };
        assert_eq!(held(&[2], &[]), expect.held.0, "{what}: out of the run");
        assert_eq!(held(&[1, 2], &[]), expect.held.1, "{what}: in the run");
        assert_eq!(
            held(&[2], &[1]),
            expect.held.0.or(Some(Hold::LeftOut)),
            "{what}: left out"
        );

        // What `--stale` is foreseen to approve again: FR-2 stale on an upstream the
        // frame refuses is not taken, as the run refuses it.
        assert_eq!(
            stale_taken(&shelf, &ledger, &assessment).contains(&docid(2)),
            expect.taken,
            "{what}: taken by --stale"
        );
    }
}
