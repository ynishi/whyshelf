//! Tests of `reach` over the in-memory fake: the use case reads the side after, asks
//! the adapters where the artifacts live, and carries the domain's answer.

use std::collections::BTreeSet;

use crate::domain::location::RelPath;
use crate::domain::nonempty::{NonEmptySet, NonEmptyVec};
use crate::domain::shelf::lookup::Reference;
use crate::domain::shelf::placement::Placement;

use super::fake_tests::{FakeShelf, artifact, block_on, docid, file_space, fr, path, sp_tested};
use super::{ShelfApp, ShelfError};

fn refs(texts: &[&str]) -> NonEmptyVec<Reference> {
    NonEmptyVec::from_vec(texts.iter().map(|t| t.parse().unwrap()).collect()).unwrap()
}

fn files(texts: &[&str]) -> BTreeSet<RelPath> {
    texts.iter().map(|t| path(t)).collect()
}

#[test]
fn a_reach_is_what_depends_on_the_named_what_they_stand_on_and_its_files() {
    let app = ShelfApp::new(FakeShelf::new().with_files(vec![
        fr(1, "one", &[]),
        sp_tested(2, "two", &["FR-1"], &["t/a.rs"]),
        fr(3, "three", &[]),
        sp_tested(4, "four", &["FR-3"], &["t/b.rs"]),
    ]));
    let report = block_on(app.reach(&refs(&["FR-1"]))).unwrap();
    let reached: Vec<_> = report.docs.iter().map(|d| (d.row.id, d.named)).collect();
    assert_eq!(reached, [(docid(1), true), (docid(2), false)]);
    assert_eq!(report.docs[1].files, files(&["specs/sp-2.md"]));
    assert_eq!(report.artifacts.len(), 1);
    let only = &report.artifacts[0];
    assert_eq!(only.artifact, artifact(&file_space(), "t/a.rs"));
    assert_eq!(only.by, BTreeSet::from([docid(2)]));
    assert_eq!(
        only.placement,
        Placement::Only(NonEmptySet::new(path("t/a.rs"), []))
    );
    assert_eq!(
        report.files,
        files(&["findings/fr-1.md", "specs/sp-2.md", "t/a.rs"])
    );
    assert!(report.gone.is_empty());
    assert_eq!(report.unplaced().count(), 0);
    assert_eq!(report.untold, None);
    assert!(report.health.is_clean());
    // Where the artifacts live is asked once, for all of them.
    assert_eq!(app.ports().places_count(), 1);
}

#[test]
fn every_reference_is_named_and_one_that_names_nothing_is_an_error() {
    let app =
        ShelfApp::new(FakeShelf::new().with_files(vec![fr(1, "one", &[]), fr(3, "three", &[])]));
    let report = block_on(app.reach(&refs(&["FR-1", "FR-3"]))).unwrap();
    assert!(report.docs.iter().all(|d| d.named));
    assert_eq!(report.docs.len(), 2);
    // No document stands on an artifact: no adapter is asked where one lives.
    assert!(report.artifacts.is_empty());
    assert_eq!(app.ports().places_count(), 0);
    assert!(matches!(
        block_on(app.reach(&refs(&["FR-1", "FR-9"]))),
        Err(ShelfError::NotFound { .. })
    ));
}
