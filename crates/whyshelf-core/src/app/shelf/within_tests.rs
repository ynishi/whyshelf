//! Tests of the gate's use case ([`ShelfApp::impact_within`]): the one reading of
//! both ends that `impact` makes, with the gate beside it, and how a reference is
//! resolved across the two ends (at the end before; at the end after only when it
//! names nothing before, and that end's answer then).

use crate::domain::nonempty::NonEmptyVec;
use crate::domain::shelf::build::DocReading;
use crate::domain::shelf::lookup::Reference;
use crate::domain::shelf::schema::ShelfDecl;
use crate::domain::shelf::within::{InsideBy, VerdictKind};

use super::error::{ErrorClass, ShelfError};
use super::fake_tests::{FakeBefore, FakeShelf, block_on, decls, docid, file, fr, sp};
use super::{ImpactReport, ShelfApp};

/// A fake whose end before holds `before` and whose end after holds `after`, with no
/// record at either.
fn change(before: Vec<DocReading>, after: Vec<DocReading>) -> ShelfApp<FakeShelf> {
    let ports = FakeShelf::new().with_files(after);
    *ports.before.lock().unwrap() = Some(FakeBefore {
        readings: before,
        ..FakeBefore::default()
    });
    ShelfApp::new(ports)
}

fn refs(texts: &[&str]) -> NonEmptyVec<Reference> {
    NonEmptyVec::from_vec(texts.iter().map(|t| t.parse().unwrap()).collect()).unwrap()
}

// The error is read once and ends the test; it is not a hot path.
#[allow(clippy::result_large_err)]
fn within(app: &ShelfApp<FakeShelf>, texts: &[&str]) -> Result<ImpactReport, ShelfError> {
    block_on(app.impact_within(&refs(texts)))
}

#[test]
fn impact_answers_no_gate_and_impact_within_answers_the_gate_beside_the_same_rows() {
    let app = change(
        vec![fr(1, "slow", &[]), sp(2, "cache", &["FR-1"])],
        vec![fr(1, "slow", &[]), sp(2, "cached", &["FR-1"])],
    );
    let plain = block_on(app.impact()).unwrap();
    assert_eq!(plain.within, None);
    let gated = within(&app, &["FR-1"]).unwrap();
    assert_eq!(gated.entries, plain.entries, "the same reading");
    let gate = gated.within.expect("the gate");
    assert_eq!(gate.named().iter().copied().collect::<Vec<_>>(), [docid(1)]);
    assert!(gate.passes(), "SP-2 depends on FR-1");
    let gate = within(&app, &["SP-2"]).unwrap().within.unwrap();
    assert!(gate.passes());
}

#[test]
fn a_document_the_change_adds_is_named_at_the_end_after() {
    let app = change(
        vec![fr(1, "slow", &[])],
        vec![fr(1, "slow", &[]), sp(3, "added", &["FR-1"])],
    );
    let gate = within(&app, &["FR-1", "SP-3"]).unwrap().within.unwrap();
    assert!(gate.named().contains(&docid(3)));
    let three = gate
        .entries()
        .iter()
        .find(|entry| entry.id() == docid(3))
        .unwrap();
    assert!(matches!(
        three.verdict().kind(),
        VerdictKind::Inside {
            by: InsideBy::Named,
            ..
        }
    ));
}

/// A key the change moved to another document names the document before the change.
#[test]
fn a_key_the_change_gave_another_document_names_the_one_before() {
    let app = change(
        vec![fr(1, "slow", &[])],
        vec![
            file("findings/fr-1.md", 1, "FR-7", "slow", &[]),
            file("findings/fr-3.md", 3, "FR-1", "new", &[]),
        ],
    );
    let gate = within(&app, &["FR-1"]).unwrap().within.unwrap();
    assert_eq!(gate.named().iter().copied().collect::<Vec<_>>(), [docid(1)]);
}

/// N1: a reference that names nothing at either end is the error it is for `get`,
/// and so is the answer of the end after when it names nothing before; any other
/// error at the end before (several documents, a file with no identity that could be
/// read) is that error, whatever the end after holds. Each is an error of the
/// request.
#[test]
fn a_reference_that_names_no_document_is_the_error_of_the_end_that_answered() {
    let two_writers = || {
        vec![
            fr(1, "slow", &[]),
            file("findings/a.md", 8, "FR-9", "one", &[]),
            file("findings/b.md", 9, "FR-9", "two", &[]),
        ]
    };
    let app = change(vec![fr(1, "slow", &[])], vec![fr(1, "slow", &[])]);
    let error = within(&app, &["FR-1", "FR-9"]).unwrap_err();
    assert!(matches!(error, ShelfError::NotFound { .. }), "{error:?}");
    assert_eq!(error.class(), ErrorClass::Request);
    // Nothing before, two documents after: the end after's answer.
    let app = change(vec![fr(1, "slow", &[])], two_writers());
    let error = within(&app, &["FR-9"]).unwrap_err();
    assert!(matches!(error, ShelfError::Ambiguous { .. }), "{error:?}");
    assert_eq!(error.class(), ErrorClass::Request);
    // Two documents before, one after: the end before's answer.
    let app = change(
        two_writers(),
        vec![
            fr(1, "slow", &[]),
            file("findings/a.md", 8, "FR-9", "one", &[]),
        ],
    );
    let error = within(&app, &["FR-9"]).unwrap_err();
    assert!(matches!(error, ShelfError::Ambiguous { .. }), "{error:?}");
}

/// A reference the declaration before cannot read names nothing before: a document of
/// a book the change adds is named at the end after, by `<book>:<key>` and by a key
/// alone.
#[test]
fn a_document_of_a_book_the_change_adds_is_named_at_the_end_after() {
    let ports = FakeShelf::new().with_files(vec![fr(1, "slow", &[]), sp(3, "added", &["FR-1"])]);
    let finding_only: Vec<_> = decls()
        .into_iter()
        .filter(|book| book.name == "finding")
        .collect();
    *ports.before.lock().unwrap() = Some(FakeBefore {
        decl: Some(ShelfDecl::of_books(finding_only)),
        readings: vec![fr(1, "slow", &[])],
        ..FakeBefore::default()
    });
    let app = ShelfApp::new(ports);
    for reference in ["spec:SP-3", "SP-3"] {
        let gate = within(&app, &[reference]).unwrap().within.unwrap();
        assert_eq!(
            gate.named().iter().copied().collect::<Vec<_>>(),
            [docid(3)],
            "{reference}"
        );
    }
}
