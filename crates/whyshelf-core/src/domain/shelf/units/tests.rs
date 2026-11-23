//! Tests of the division of what does not hold into units: each rule of the module
//! page on a small shelf, and the same answer for the same shelf read in another order.

use std::collections::{BTreeMap, BTreeSet};

use crate::domain::id::DocId;
use crate::domain::nonempty::{NonBlank, NonEmptySet};
use crate::domain::shelf::artifact::ArtifactKey;
use crate::domain::shelf::build::{DocReading, Shelf};
use crate::domain::shelf::fixtures_tests::{
    approved, docid, file_artifact, fr, fr_titled, list, path, reading, schema_of_two_links, shelf,
    shelf_of, sp_testing, text,
};
use crate::domain::shelf::freshness::{FreshnessLevel, StandingKind, assess_unasked};
use crate::domain::shelf::ledger::{JudgedAt, Ledger};
use crate::domain::shelf::order::{Hold, upstream_first};
use crate::domain::shelf::placement::Placement;

use super::*;

/// A ledger with no record: every valid document is pending.
fn no_records() -> Ledger {
    crate::domain::shelf::fixtures_tests::ledger(Vec::new())
}

fn ids(ns: &[u128]) -> BTreeSet<DocId> {
    ns.iter().map(|n| docid(*n)).collect()
}

/// The finding `FR-<n>` with identity `n`, not valid: it has no title.
fn invalid(n: u128) -> DocReading {
    reading(&format!("findings/fr-{n}.md"), n, &format!("FR-{n}"), &[])
}

/// The artifacts placed whole in these files.
fn only(paths: &[&str]) -> Placement {
    let mut files = paths.iter().map(|p| path(p));
    let first = files.next().unwrap();
    Placement::Only(NonEmptySet::new(first, files))
}

/// Every name asked placed whole in the file of its own name.
fn placed_as_named(asks: &BTreeSet<ArtifactKey>) -> BTreeMap<ArtifactKey, Placement> {
    asks.iter()
        .map(|key| (key.clone(), only(&[key.name().as_str()])))
        .collect()
}

/// The units of `now` and `ledger`, of `of`, the artifacts placed by `placements` (or,
/// when none is given, each in the file of its own name).
fn divided(
    now: &Shelf,
    ledger: &Ledger,
    of: UnitsOf,
    placements: Option<BTreeMap<ArtifactKey, Placement>>,
) -> Units {
    let assessment = assess_unasked(now, ledger, JudgedAt::new(100));
    let plan = plan(now, ledger, &assessment, of);
    let placements = placements.unwrap_or_else(|| placed_as_named(&plan.asks()));
    divide(plan, &placements)
}

/// The documents of each unit, in their order.
fn docs_of(units: &Units) -> Vec<Vec<DocId>> {
    units
        .units()
        .iter()
        .map(|unit| unit.documents().iter().map(UnitDoc::id).collect())
        .collect()
}

fn order(ns: &[u128]) -> Vec<DocId> {
    ns.iter().map(|n| docid(*n)).collect()
}

/// FR-1 → SP-2 (testing `a`) and FR-3 → SP-4 (testing `b`), all approved; then FR-1 and
/// FR-3 edited, so all four are stale.
fn two_chains(a: &str, b: &str) -> (Shelf, Ledger) {
    let then = shelf(vec![
        fr(1, &[]),
        sp_testing(2, &["FR-1"], &[a]),
        fr(3, &[]),
        sp_testing(4, &["FR-3"], &[b]),
    ]);
    let ledger = approved(&then, &[1, 2, 3, 4]);
    let now = shelf(vec![
        fr_titled(1, "edited", &[]),
        sp_testing(2, &["FR-1"], &[a]),
        fr_titled(3, "edited", &[]),
        sp_testing(4, &["FR-3"], &[b]),
    ]);
    (now, ledger)
}

#[test]
fn two_chains_that_share_nothing_are_two_units_each_upstream_first() {
    let (now, ledger) = two_chains("t/a.rs", "t/b.rs");
    let assessment = assess_unasked(&now, &ledger, JudgedAt::new(100));
    // What is divided is the set of degraded approvals, and every origin is of it.
    let degraded: BTreeSet<DocId> = assessment.degraded().map(|(id, _)| id).collect();
    assert_eq!(degraded, ids(&[1, 2, 3, 4]));
    let undone: BTreeSet<DocId> = plan(&now, &ledger, &assessment, UnitsOf::Degraded)
        .undone
        .keys()
        .copied()
        .collect();
    assert_eq!(undone, degraded);
    for group in assessment.origins() {
        assert!(group.degraded.is_subset(&degraded));
    }

    let units = divided(&now, &ledger, UnitsOf::Degraded, None);
    assert_eq!(docs_of(&units), [order(&[1, 2]), order(&[3, 4])]);
    let first = &units.units()[0];
    assert_eq!(first.files(), &BTreeSet::from([path("t/a.rs")]));
    let artifacts: Vec<(&ArtifactKey, &BTreeSet<DocId>)> = first
        .artifacts()
        .iter()
        .map(|a| (a.artifact(), a.documents()))
        .collect();
    assert_eq!(artifacts, [(&file_artifact("t/a.rs"), &ids(&[2]))]);
    assert!(first.waits_on().is_empty());
    assert!(units.unplaced().is_empty());
    let stale = StandingKind::Approved(FreshnessLevel::Stale);
    assert!(first.documents().iter().all(|doc| doc.standing() == stale));
}

#[test]
fn documents_that_stand_on_one_file_are_one_unit() {
    // The same name in both chains.
    let (now, ledger) = two_chains("t/a.rs", "t/a.rs");
    let units = divided(&now, &ledger, UnitsOf::Degraded, None);
    assert_eq!(docs_of(&units), [order(&[1, 2, 3, 4])]);
    // Two names that live in one file, as two tests of one file do.
    let (now, ledger) = two_chains("t/one", "t/two");
    let placements = BTreeMap::from([
        (file_artifact("t/one"), only(&["t/a.rs"])),
        (
            file_artifact("t/two"),
            Placement::Partly(NonEmptySet::new(path("t/a.rs"), [])),
        ),
    ]);
    let units = divided(&now, &ledger, UnitsOf::Degraded, Some(placements));
    assert_eq!(units.units().len(), 1);
    assert_eq!(units.units()[0].files(), &BTreeSet::from([path("t/a.rs")]));
    assert_eq!(units.units()[0].artifacts().len(), 2);
}

/// FR-2 and FR-3 approved on FR-1, then linked to FR-5 too (`with` of them), which is
/// pending, or else edited: both are stale, and FR-1 holds.
fn on_a_pending_upstream(with: &[u128]) -> (Shelf, Ledger) {
    let then = shelf(vec![
        fr(1, &[]),
        fr(2, &["FR-1"]),
        fr(3, &["FR-1"]),
        fr(5, &[]),
    ]);
    let ledger = approved(&then, &[1, 2, 3]);
    let base = |n: u128| match with.contains(&n) {
        true => fr(n, &["FR-1", "FR-5"]),
        false => fr_titled(n, "edited", &["FR-1"]),
    };
    let now = shelf(vec![fr(1, &[]), base(2), base(3), fr(5, &[])]);
    (now, ledger)
}

#[test]
fn the_same_upstream_outside_that_holds_both_back_joins_them_and_is_waited_on() {
    let (now, ledger) = on_a_pending_upstream(&[2, 3]);
    let units = divided(&now, &ledger, UnitsOf::Degraded, None);
    assert_eq!(docs_of(&units), [order(&[2, 3])]);
    assert_eq!(
        units.units()[0].waits_on(),
        &BTreeMap::from([(docid(5), Hold::Pending)])
    );
    // Only one of them stands on it: two units, one waiting.
    let (now, ledger) = on_a_pending_upstream(&[2]);
    let units = divided(&now, &ledger, UnitsOf::Degraded, None);
    assert_eq!(docs_of(&units), [order(&[2]), order(&[3])]);
    assert_eq!(units.units()[0].waits_on().len(), 1);
    assert!(units.units()[1].waits_on().is_empty());

    // With the pending divided too, the upstream is a document of the unit, first, and
    // no key: nothing waits on it. Each kind is the standing's, read once.
    let (now, ledger) = on_a_pending_upstream(&[2, 3]);
    let assessment = assess_unasked(&now, &ledger, JudgedAt::new(100));
    let units = divided(&now, &ledger, UnitsOf::DegradedAndPending, None);
    assert_eq!(docs_of(&units), [order(&[5, 2, 3])]);
    let unit = &units.units()[0];
    assert!(unit.waits_on().is_empty());
    for doc in unit.documents() {
        assert_eq!(doc.standing(), assessment.standing_of(doc.id()).kind());
    }
    let kinds: Vec<StandingKind> = unit.documents().iter().map(UnitDoc::standing).collect();
    assert_eq!(
        kinds,
        [
            StandingKind::Pending,
            StandingKind::Approved(FreshnessLevel::Stale),
            StandingKind::Approved(FreshnessLevel::Stale),
        ]
    );
}

#[test]
fn a_hold_on_a_second_link_or_on_the_second_target_of_a_link_joins_too() {
    let schema = schema_of_two_links();
    let on = |n: u128, base: &[&str], also: &[&str]| {
        let mut extra = vec![("title", text("t")), ("base", list(base))];
        if !also.is_empty() {
            extra.push(("also", list(also)));
        }
        reading(
            &format!("findings/fr-{n}.md"),
            n,
            &format!("FR-{n}"),
            &extra,
        )
    };
    let then = shelf_of(
        &schema,
        vec![
            fr(1, &[]),
            fr(5, &[]),
            on(6, &["FR-1"], &[]),
            on(7, &["FR-1"], &[]),
        ],
    );
    let ledger = approved(&then, &[1, 6, 7]);
    // FR-6: the second target of `base` is FR-5. FR-7: `also` (read first) names FR-1,
    // and `base`, the second link, FR-5.
    let now = shelf_of(
        &schema,
        vec![
            fr(1, &[]),
            fr(5, &[]),
            on(6, &["FR-1", "FR-5"], &[]),
            on(7, &["FR-5"], &["FR-1"]),
        ],
    );
    let units = divided(&now, &ledger, UnitsOf::Degraded, None);
    assert_eq!(docs_of(&units), [order(&[6, 7])]);
    assert_eq!(
        units.units()[0].waits_on(),
        &BTreeMap::from([(docid(5), Hold::Pending)])
    );
}

#[test]
fn a_link_to_an_undone_document_it_cannot_stand_on_joins_the_two() {
    // B is approved, then not valid (broken); A, pending, links to it, which it cannot
    // stand on, and stands on nothing. They are one unit, by identity in either order.
    for (a, b) in [(1, 2), (4, 3)] {
        let then = shelf(vec![fr(b, &[])]);
        let ledger = approved(&then, &[b]);
        let key_b = format!("FR-{b}");
        let now = shelf(vec![invalid(b), fr(a, &[key_b.as_str()])]);
        let units = divided(&now, &ledger, UnitsOf::DegradedAndPending, None);
        let mut both = order(&[a, b]);
        both.sort();
        assert_eq!(docs_of(&units), [both.clone()]);
        assert_eq!(upstream_first(&now, &ledger, &ids(&[a, b])), both);
        // Without the pending divided, B is alone.
        let units = divided(&now, &ledger, UnitsOf::Degraded, None);
        assert_eq!(docs_of(&units), [order(&[b])]);
    }
}

#[test]
fn a_link_to_a_key_an_undone_document_writes_with_another_file_joins_the_two() {
    // FR-5 approved; then a second file writes its key, so both are not valid: FR-5 is
    // broken, the other (6) pending and not valid, so not divided. FR-1 links to the key.
    let then = shelf(vec![fr(5, &[])]);
    let ledger = approved(&then, &[5]);
    let twin = reading("findings/twin.md", 6, "FR-5", &[("title", text("t"))]);
    let now = shelf(vec![fr(5, &[]), twin, fr(1, &["FR-5"])]);
    assert_eq!(now.collisions().len(), 1);
    let units = divided(&now, &ledger, UnitsOf::DegradedAndPending, None);
    assert_eq!(docs_of(&units), [order(&[1, 5])]);
}

#[test]
fn a_document_a_link_names_outside_the_undone_joins_nothing() {
    // FR-9 is not valid and was never approved: not divided. FR-1 and FR-2, pending,
    // both link to it, and are two units; FR-9 is in neither.
    let now = shelf(vec![invalid(9), fr(1, &["FR-9"]), fr(2, &["FR-9"])]);
    let units = divided(&now, &no_records(), UnitsOf::DegradedAndPending, None);
    assert_eq!(docs_of(&units), [order(&[1]), order(&[2])]);
    assert!(units.units().iter().all(|unit| unit.waits_on().is_empty()));
}

#[test]
fn the_order_of_a_unit_joined_by_a_name_alone_is_upstream_firsts() {
    // E (1) stands on A (4); A links to B (3), not valid, which it cannot stand on.
    let then = shelf(vec![fr(3, &[])]);
    let ledger = approved(&then, &[3]);
    let now = shelf(vec![fr(1, &["FR-4"]), invalid(3), fr(4, &["FR-3"])]);
    let units = divided(&now, &ledger, UnitsOf::DegradedAndPending, None);
    assert_eq!(units.units().len(), 1);
    let documents: Vec<DocId> = units.units()[0]
        .documents()
        .iter()
        .map(UnitDoc::id)
        .collect();
    assert_eq!(documents, upstream_first(&now, &ledger, &ids(&[1, 3, 4])));
}

#[test]
fn a_name_whose_place_cannot_be_told_joins_only_the_same_name() {
    let why = NonBlank::new("cannot say").unwrap();
    let unplaced = |names: &[&str]| -> BTreeMap<ArtifactKey, Placement> {
        names
            .iter()
            .map(|n| (file_artifact(n), Placement::Unplaced(why.clone())))
            .collect()
    };
    let (now, ledger) = two_chains("t/x", "t/x");
    let units = divided(&now, &ledger, UnitsOf::Degraded, Some(unplaced(&["t/x"])));
    assert_eq!(units.units().len(), 1);
    assert!(units.units()[0].files().is_empty());
    let listed: Vec<(&ArtifactKey, &NonBlank, &BTreeSet<DocId>)> = units
        .unplaced()
        .iter()
        .map(|u| (u.artifact(), u.why(), u.documents()))
        .collect();
    assert_eq!(listed, [(&file_artifact("t/x"), &why, &ids(&[2, 4]))]);

    let (now, ledger) = two_chains("t/x", "t/y");
    let units = divided(
        &now,
        &ledger,
        UnitsOf::Degraded,
        Some(unplaced(&["t/x", "t/y"])),
    );
    assert_eq!(units.units().len(), 2);
    assert_eq!(units.unplaced().len(), 2);
}

#[test]
fn the_names_asked_are_the_names_divided_and_one_not_answered_has_no_place() {
    let (now, ledger) = two_chains("t/a.rs", "t/b.rs");
    let assessment = assess_unasked(&now, &ledger, JudgedAt::new(100));
    let plan = plan(&now, &ledger, &assessment, UnitsOf::Degraded);
    let asks = plan.asks();
    assert_eq!(
        asks,
        BTreeSet::from([file_artifact("t/a.rs"), file_artifact("t/b.rs")])
    );
    let mut placements = placed_as_named(&asks);
    placements.remove(&file_artifact("t/b.rs"));
    let units = divide(plan, &placements);
    let listed: Vec<(&ArtifactKey, Placement)> = units
        .unplaced()
        .iter()
        .map(|u| (u.artifact(), Placement::Unplaced(u.why().clone())))
        .collect();
    assert_eq!(
        listed,
        [(&file_artifact("t/b.rs"), Placement::unanswered())]
    );
}

#[test]
fn a_cycle_of_pending_links_and_a_broken_cycle_are_each_one_unit_by_identity() {
    let pending = shelf(vec![fr(1, &["FR-2"]), fr(2, &["FR-1"]), fr(3, &[])]);
    let units = divided(&pending, &no_records(), UnitsOf::DegradedAndPending, None);
    assert_eq!(docs_of(&units), [order(&[1, 2]), order(&[3])]);

    let then = shelf(vec![fr(1, &[]), fr(2, &[])]);
    let ledger = approved(&then, &[1, 2]);
    let units = divided(&pending, &ledger, UnitsOf::Degraded, None);
    assert_eq!(docs_of(&units), [order(&[1, 2])]);
    let broken = StandingKind::Approved(FreshnessLevel::Broken);
    assert!(
        units.units()[0]
            .documents()
            .iter()
            .all(|doc| doc.standing() == broken)
    );
}

#[test]
fn the_same_shelf_read_in_another_order_is_divided_the_same() {
    let (now, ledger) = two_chains("t/a.rs", "t/a.rs");
    let readings = vec![
        fr_titled(1, "edited", &[]),
        sp_testing(2, &["FR-1"], &["t/a.rs"]),
        fr_titled(3, "edited", &[]),
        sp_testing(4, &["FR-3"], &["t/a.rs"]),
    ];
    let reversed = shelf(readings.into_iter().rev().collect());
    assert_eq!(
        divided(&now, &ledger, UnitsOf::DegradedAndPending, None),
        divided(&reversed, &ledger, UnitsOf::DegradedAndPending, None)
    );
}

/// The design `key` with identity `n`, titled `title`, on the specs `specs`.
fn design(n: u128, key: &str, title: &str, specs: &[&str]) -> DocReading {
    let mut extra = vec![("title", text(title))];
    if !specs.is_empty() {
        extra.push(("specs", list(specs)));
    }
    reading(&format!("designs/{key}.md"), n, key, &extra)
}

/// FR-1 and FR-2 approved on nothing, then linked to FR-11 and FR-12, pending; FR-12
/// stands on FR-11 when `p2_on_p1`.
fn held_by_two_pending(p2_on_p1: bool) -> (Shelf, Ledger) {
    let then = shelf(vec![fr(1, &[]), fr(2, &[])]);
    let ledger = approved(&then, &[1, 2]);
    let p2: &[&str] = if p2_on_p1 { &["FR-11"] } else { &[] };
    let now = shelf(vec![
        fr(1, &["FR-11"]),
        fr(2, &["FR-12"]),
        fr(11, &[]),
        fr(12, p2),
    ]);
    (now, ledger)
}

#[test]
fn a_chain_outside_joins_what_it_holds_back_and_only_its_head_is_waited_on() {
    // (i) FR-12, which holds FR-2 back, stands on FR-11, which holds FR-1 back.
    let (now, ledger) = held_by_two_pending(true);
    let units = divided(&now, &ledger, UnitsOf::Degraded, None);
    assert_eq!(docs_of(&units), [order(&[1, 2])]);
    assert_eq!(
        units.units()[0].waits_on(),
        &BTreeMap::from([(docid(11), Hold::Pending), (docid(12), Hold::Pending)])
    );
    let (now, ledger) = held_by_two_pending(false);
    let units = divided(&now, &ledger, UnitsOf::Degraded, None);
    assert_eq!(docs_of(&units), [order(&[1]), order(&[2])]);
    // (vi) With the pending divided, the chain is empty and R3 joins the same.
    let (now, ledger) = held_by_two_pending(true);
    let units = divided(&now, &ledger, UnitsOf::DegradedAndPending, None);
    assert_eq!(units.units().len(), 1);
    let unit = &units.units()[0];
    assert!(unit.waits_on().is_empty());
    let documents: Vec<DocId> = unit.documents().iter().map(UnitDoc::id).collect();
    assert_eq!(
        documents,
        upstream_first(&now, &ledger, &ids(&[1, 2, 11, 12]))
    );
}

/// FR-5 and SP-1 (testing `t/a.rs`) stale through an edit of FR-5; the design `b`
/// approved on nothing, then on SP-12, pending, which tests `p2_tests`.
fn a_chain_with_tests(a_tests: &[&str], p2_tests: &[&str]) -> (Shelf, Ledger) {
    let then = shelf(vec![
        fr(5, &[]),
        sp_testing(1, &["FR-5"], a_tests),
        design(2, "b", "t", &[]),
    ]);
    let ledger = approved(&then, &[5, 1, 2]);
    let now = shelf(vec![
        fr_titled(5, "edited", &[]),
        sp_testing(1, &["FR-5"], a_tests),
        design(2, "b", "t", &["SP-12"]),
        sp_testing(12, &[], p2_tests),
    ]);
    (now, ledger)
}

#[test]
fn an_artifact_of_the_chain_joins_by_its_file_and_is_not_listed_in_a_unit() {
    // (ii) SP-12, which holds `b` back, tests the file SP-1 tests.
    let (now, ledger) = a_chain_with_tests(&["t/a.rs"], &["t/a.rs"]);
    let units = divided(&now, &ledger, UnitsOf::Degraded, None);
    assert_eq!(units.units().len(), 1);
    let unit = &units.units()[0];
    let documents: BTreeSet<DocId> = unit.documents().iter().map(UnitDoc::id).collect();
    assert_eq!(documents, ids(&[1, 2, 5]));
    let artifacts: Vec<(&ArtifactKey, &BTreeSet<DocId>)> = unit
        .artifacts()
        .iter()
        .map(|a| (a.artifact(), a.documents()))
        .collect();
    assert_eq!(artifacts, [(&file_artifact("t/a.rs"), &ids(&[1]))]);
    // Another file: two units, and the chain's file is in neither.
    let (now, ledger) = a_chain_with_tests(&["t/a.rs"], &["t/b.rs"]);
    let units = divided(&now, &ledger, UnitsOf::Degraded, None);
    assert_eq!(docs_of(&units), [order(&[5, 1]), order(&[2])]);
    assert!(units.units()[1].files().is_empty());
    assert!(units.units()[1].artifacts().is_empty());
    assert_eq!(
        units.units()[1].waits_on(),
        &BTreeMap::from([(docid(12), Hold::Pending)])
    );
}

#[test]
fn a_name_of_the_chain_is_asked_and_one_unanswered_joins_by_name_and_is_listed() {
    // (vii) SP-1 tests `t/x`; SP-12 tests `t/x` and `t/q`; no answer for either.
    let (now, ledger) = a_chain_with_tests(&["t/x"], &["t/x", "t/q"]);
    let assessment = assess_unasked(&now, &ledger, JudgedAt::new(100));
    let plan = plan(&now, &ledger, &assessment, UnitsOf::Degraded);
    assert_eq!(
        plan.asks(),
        BTreeSet::from([file_artifact("t/q"), file_artifact("t/x")])
    );
    let units = divide(plan, &BTreeMap::new());
    assert_eq!(units.units().len(), 1);
    let listed: Vec<(&ArtifactKey, &BTreeSet<DocId>)> = units
        .unplaced()
        .iter()
        .map(|u| (u.artifact(), u.documents()))
        .collect();
    assert_eq!(
        listed,
        [
            (&file_artifact("t/q"), &ids(&[12])),
            (&file_artifact("t/x"), &ids(&[1, 12])),
        ]
    );
    let names: Vec<&ArtifactKey> = units.units()[0]
        .artifacts()
        .iter()
        .map(UnitArtifact::artifact)
        .collect();
    assert_eq!(names, [&file_artifact("t/x")]);
}

#[test]
fn a_chain_joins_what_it_stands_on_among_the_undone() {
    // (iii) FR-12, which holds FR-1 back, stands on FR-3, stale, of another unit.
    let chain = |on: &[&str]| {
        let then = shelf(vec![fr(1, &[]), fr(3, &[])]);
        let ledger = approved(&then, &[1, 3]);
        let now = shelf(vec![
            fr(1, &["FR-12"]),
            fr_titled(3, "edited", &[]),
            fr(12, on),
        ]);
        divided(&now, &ledger, UnitsOf::Degraded, None)
    };
    let units = chain(&["FR-3"]);
    assert_eq!(docs_of(&units), [order(&[1, 3])]);
    assert_eq!(
        units.units()[0].waits_on(),
        &BTreeMap::from([(docid(12), Hold::Pending)])
    );
    assert_eq!(docs_of(&chain(&[])), [order(&[1]), order(&[3])]);
}

#[test]
fn the_same_awaiting_upstream_joins_and_holds_nothing_back_in_turn() {
    use crate::domain::shelf::fixtures_tests::{admitted, approve_as, asking, ruled_shelf};
    // (iv) FR-9 is approved by one name of the two its rule asks for: awaiting.
    let then = ruled_shelf(asking(2, None), vec![fr(1, &[]), fr(2, &[]), fr(9, &[])]);
    let mut records = Vec::new();
    for n in [9, 1, 2] {
        let so_far = admitted(&then, records.clone());
        records.push(approve_as(&then, &so_far, n, 1000 + n, ("ana", 10, None)));
    }
    let now = ruled_shelf(
        asking(2, None),
        vec![fr(1, &["FR-9"]), fr(2, &["FR-9"]), fr(9, &[])],
    );
    let ledger = admitted(&now, records);
    let units = divided(&now, &ledger, UnitsOf::Degraded, None);
    assert_eq!(docs_of(&units), [order(&[1, 2])]);
    assert_eq!(
        units.units()[0].waits_on(),
        &BTreeMap::from([(docid(9), Hold::Awaiting)])
    );
}

#[test]
fn a_chain_stops_at_an_approval_that_holds() {
    // (v) FR-11 and FR-12, which hold FR-1 and FR-2 back, both stand on FR-7, fresh.
    let then = shelf(vec![fr(1, &[]), fr(2, &[]), fr(7, &[])]);
    let ledger = approved(&then, &[1, 2, 7]);
    let now = shelf(vec![
        fr(1, &["FR-11"]),
        fr(2, &["FR-12"]),
        fr(7, &[]),
        fr(11, &["FR-7"]),
        fr(12, &["FR-7"]),
    ]);
    let units = divided(&now, &ledger, UnitsOf::Degraded, None);
    assert_eq!(docs_of(&units), [order(&[1]), order(&[2])]);

    // (viii) `b`, stale, stands on SP-9, fresh, which tests the file SP-1 tests: a fresh
    // document is no key, so the two are apart.
    let then = shelf(vec![
        fr(5, &[]),
        sp_testing(1, &["FR-5"], &["t/a.rs"]),
        sp_testing(9, &[], &["t/a.rs"]),
        design(2, "b", "t", &["SP-9"]),
    ]);
    let ledger = approved(&then, &[5, 1, 9, 2]);
    let now = shelf(vec![
        fr_titled(5, "edited", &[]),
        sp_testing(1, &["FR-5"], &["t/a.rs"]),
        sp_testing(9, &[], &["t/a.rs"]),
        design(2, "b", "edited", &["SP-9"]),
    ]);
    let units = divided(&now, &ledger, UnitsOf::Degraded, None);
    assert_eq!(docs_of(&units), [order(&[5, 1]), order(&[2])]);
}

#[test]
fn what_could_join_units_unseen_is_counted() {
    use crate::domain::shelf::build::{RawProblem, ReadId};
    let told = |units: &Units| units.untold().map(|u| (u.invalid(), u.unreadable()));
    // A clean shelf: nothing.
    let (now, ledger) = two_chains("t/a.rs", "t/b.rs");
    assert_eq!(told(&divided(&now, &ledger, UnitsOf::Degraded, None)), None);
    // An undone document not valid (approved, then broken) is counted.
    let then = shelf(vec![fr(2, &[])]);
    let ledger = approved(&then, &[2]);
    let now = shelf(vec![invalid(2), fr(1, &[])]);
    let units = divided(&now, &ledger, UnitsOf::Degraded, None);
    assert_eq!(told(&units), Some((1, 0)));
    // One outside that an undone document's link names is counted; one nothing
    // divided names is not.
    let now = shelf(vec![invalid(9), invalid(8), fr(1, &["FR-9"])]);
    let units = divided(&now, &no_records(), UnitsOf::DegradedAndPending, None);
    assert_eq!(told(&units), Some((1, 0)));
    let now = shelf(vec![invalid(8), fr(1, &[])]);
    let units = divided(&now, &no_records(), UnitsOf::DegradedAndPending, None);
    assert_eq!(told(&units), None);
    // A file whose identity cannot be read is counted.
    let unreadable = DocReading::Unparsed {
        origin: path("findings/unreadable.md"),
        id: ReadId::Unreadable,
        why: RawProblem::NotAMapping,
    };
    let now = shelf(vec![fr(1, &[]), unreadable]);
    let units = divided(&now, &no_records(), UnitsOf::DegradedAndPending, None);
    assert_eq!(told(&units), Some((0, 1)));
}

#[test]
fn a_document_not_valid_that_only_the_chain_names_is_counted() {
    // FR-1, stale, waits on FR-11, pending, whose link names FR-9, not valid and never
    // approved: only the chain names it.
    let told = |units: &Units| units.untold().map(|u| (u.invalid(), u.unreadable()));
    let then = shelf(vec![fr(1, &[])]);
    let ledger = approved(&then, &[1]);
    let now = shelf(vec![fr(1, &["FR-11"]), fr(11, &["FR-9"]), invalid(9)]);
    let units = divided(&now, &ledger, UnitsOf::Degraded, None);
    assert_eq!(docs_of(&units), [order(&[1])]);
    assert_eq!(told(&units), Some((1, 0)));
    // When FR-11 names nothing, nothing divided or of the chain names FR-9.
    let now = shelf(vec![fr(1, &["FR-11"]), fr(11, &[]), invalid(9)]);
    assert_eq!(told(&divided(&now, &ledger, UnitsOf::Degraded, None)), None);
}

#[test]
fn no_order_passes_through_the_chain_a_unit_waits_on() {
    // FR-1 waits on FR-11, pending, which stands on FR-2, stale: one unit, in the order
    // of upstream_first over FR-1 and FR-2 alone, which puts FR-1 first.
    let then = shelf(vec![fr(1, &[]), fr(2, &[])]);
    let ledger = approved(&then, &[1, 2]);
    let now = shelf(vec![
        fr(1, &["FR-11"]),
        fr_titled(2, "edited", &[]),
        fr(11, &["FR-2"]),
    ]);
    let units = divided(&now, &ledger, UnitsOf::Degraded, None);
    assert_eq!(units.units().len(), 1);
    let documents: Vec<DocId> = units.units()[0]
        .documents()
        .iter()
        .map(UnitDoc::id)
        .collect();
    assert_eq!(documents, upstream_first(&now, &ledger, &ids(&[1, 2])));
    assert_eq!(documents, order(&[1, 2]));
}
