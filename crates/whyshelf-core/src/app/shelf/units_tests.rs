//! Tests of `units` over the in-memory fake: the use case reads the side after, asks
//! the adapters where the artifacts live once (or not at all), and carries the
//! domain's division.

use std::collections::BTreeSet;

use crate::domain::id::DocId;
use crate::domain::shelf::units::{UnitDoc, UnitsOf};

use super::ShelfApp;
use super::fake_tests::{FakeShelf, block_on, docid, fr, path, sp_tested};

fn docs(report: &super::UnitsReport) -> Vec<Vec<DocId>> {
    report
        .units
        .units()
        .iter()
        .map(|unit| unit.documents().iter().map(UnitDoc::id).collect())
        .collect()
}

#[test]
fn the_pending_are_divided_when_asked_by_the_files_their_artifacts_live_in() {
    // Two chains, both pending: apart on two files, one unit on one.
    let app = ShelfApp::new(FakeShelf::new().with_files(vec![
        fr(1, "one", &[]),
        sp_tested(2, "two", &["FR-1"], &["t/a.rs"]),
        fr(3, "three", &[]),
        sp_tested(4, "four", &["FR-3"], &["t/b.rs"]),
    ]));
    let report = block_on(app.units(UnitsOf::DegradedAndPending)).unwrap();
    assert_eq!(
        docs(&report),
        [vec![docid(1), docid(2)], vec![docid(3), docid(4)]]
    );
    assert_eq!(
        report.units.units()[0].files(),
        &BTreeSet::from([path("t/a.rs")])
    );
    assert!(report.units.unplaced().is_empty());
    assert!(report.health.is_clean());
    assert_eq!(app.ports().places_count(), 1);

    let app = ShelfApp::new(FakeShelf::new().with_files(vec![
        fr(1, "one", &[]),
        sp_tested(2, "two", &["FR-1"], &["t/a.rs"]),
        fr(3, "three", &[]),
        sp_tested(4, "four", &["FR-3"], &["t/a.rs"]),
    ]));
    let report = block_on(app.units(UnitsOf::DegradedAndPending)).unwrap();
    assert_eq!(report.units.units().len(), 1);

    // Nothing approved, so nothing no longer holds: no unit, and no adapter asked.
    let report = block_on(app.units(UnitsOf::Degraded)).unwrap();
    assert!(report.units.units().is_empty());
    assert_eq!(app.ports().places_count(), 1);
}
