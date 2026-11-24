//! Tests of [`impact`]: the table of I2–I4 on holdings made of real causes, and one
//! change read at its two ends (I1, I5).

use crate::domain::shelf::artifact::{ArtifactHash, Untold};
use crate::domain::shelf::fixtures_tests::{
    approved, docid, file_artifact, fr, fr_titled, shelf, shelf_with, sp_testing,
};
use crate::domain::shelf::freshness::{HoldsAs, Severity, assess_unasked as assess};
use crate::domain::shelf::ledger::JudgedAt;
use crate::domain::shelf::schema::EntryName;

use super::*;

fn h(text: &str) -> ArtifactHash {
    crate::domain::shelf::artifact::Comparison::Bytes.fingerprint(text.as_bytes())
}

/// A cause that starts at `tests/a.rs` (an edit of it), held by SP-2, from a real
/// assessment.
fn artifact_cause() -> RootCause {
    let docs = || vec![fr(1, &[]), sp_testing(2, &["FR-1"], &["tests/a.rs"])];
    let then = shelf_with(docs(), &[("tests/a.rs", Some(h("a")))]);
    let ledger = approved(&then, &[1, 2]);
    let now = shelf_with(docs(), &[("tests/a.rs", Some(h("edited")))]);
    assess(&now, &ledger, JudgedAt::new(100))
        .roots_of(docid(2))
        .into_iter()
        .find(|root| matches!(root.cause, Cause::ArtifactChanged { .. }))
        .unwrap()
}

/// A cause of SP-2's own text (an edit of it), from a real assessment.
fn own_cause() -> RootCause {
    let then = shelf(vec![fr_titled(2, "one", &[])]);
    let ledger = approved(&then, &[2]);
    let now = shelf(vec![fr_titled(2, "two", &[])]);
    assess(&now, &ledger, JudgedAt::new(100))
        .roots_of(docid(2))
        .into_iter()
        .next()
        .unwrap()
}

fn pin(name: &str) -> UntoldPin {
    UntoldPin {
        link: EntryName::new("tests").unwrap(),
        artifact: file_artifact(name),
        why: Untold::NoAnswer,
    }
}

fn not_holding(roots: Vec<RootCause>, untold: Vec<UntoldPin>) -> Holding {
    Holding::DoesNotHold {
        level: Severity::Stale,
        roots,
        untold,
    }
}

fn cannot_tell(untold: Vec<UntoldPin>) -> Holding {
    Holding::CannotTell {
        holds_as: HoldsAs::Fresh,
        untold: NonEmptyVec::from_vec(untold).unwrap(),
    }
}

/// `made`, the side before of every holder being `before`.
fn made_of(was: &Holding, is: &Holding, before: &Holding) -> Made {
    made(was, is, |_| before.clone())
}

#[test]
fn a_certain_cause_beside_one_on_a_name_the_side_before_could_not_tell_is_yes_with_both() {
    let was = cannot_tell(vec![pin("tests/a.rs")]);
    let is = not_holding(vec![own_cause(), artifact_cause()], Vec::new());
    let Made::Yes(yes) = made_of(&was, &is, &was) else {
        panic!("yes");
    };
    assert_eq!(
        yes.certain().iter().cloned().collect::<Vec<_>>(),
        [own_cause()]
    );
    assert_eq!(yes.uncertain(), [artifact_cause()]);
}

#[test]
fn only_a_cause_on_a_name_the_side_before_could_not_tell_cannot_be_told_from_before() {
    let was = cannot_tell(vec![pin("tests/a.rs")]);
    let is = not_holding(vec![artifact_cause()], Vec::new());
    let Made::CannotTell(why) = made_of(&was, &is, &was) else {
        panic!("cannot tell");
    };
    assert_eq!(why.side(), Sides::Before);
    assert_eq!(
        why.untold().iter().cloned().collect::<Vec<_>>(),
        [pin("tests/a.rs")]
    );
    assert_eq!(why.uncertain(), [artifact_cause()]);
    // With a name the side after cannot tell too, both sides could not.
    let is = not_holding(vec![artifact_cause()], vec![pin("tests/b.rs")]);
    let Made::CannotTell(why) = made_of(&was, &is, &was) else {
        panic!("cannot tell");
    };
    assert_eq!(why.side(), Sides::Both);
    assert_eq!(why.untold().iter().count(), 2);
}

#[test]
fn a_name_neither_side_could_tell_is_listed_once() {
    let was = cannot_tell(vec![pin("tests/a.rs")]);
    let is = not_holding(vec![artifact_cause()], vec![pin("tests/a.rs")]);
    let Made::CannotTell(why) = made_of(&was, &is, &was) else {
        panic!("cannot tell");
    };
    assert_eq!(why.side(), Sides::Both);
    assert_eq!(
        why.untold().iter().cloned().collect::<Vec<_>>(),
        [pin("tests/a.rs")]
    );
}

#[test]
fn a_name_the_side_after_cannot_tell_may_hide_a_cause() {
    let was = Holding::Holds(HoldsAs::Fresh);
    let is = cannot_tell(vec![pin("tests/b.rs")]);
    let Made::CannotTell(why) = made_of(&was, &is, &was) else {
        panic!("cannot tell");
    };
    assert_eq!(why.side(), Sides::After);
    assert!(why.uncertain().is_empty());
}

/// I4: a name neither end could tell, with no cause added, cannot be told from both
/// ends, listed once; and the two holdings, alike, are no change (I5).
#[test]
fn the_same_name_untold_at_both_ends_is_of_both_listed_once_and_no_change() {
    let was = cannot_tell(vec![pin("tests/a.rs")]);
    let is = cannot_tell(vec![pin("tests/a.rs")]);
    let Made::CannotTell(why) = made_of(&was, &is, &was) else {
        panic!("cannot tell");
    };
    assert_eq!(why.side(), Sides::Both);
    assert_eq!(
        why.untold().iter().cloned().collect::<Vec<_>>(),
        [pin("tests/a.rs")]
    );
    assert_eq!(was, is, "no change");
}

/// I3: a move the change itself declares leaves the name the end before could not tell
/// at the old name, and the cause at the new one: the cause is matched through the
/// move, and is uncertain, never certain.
#[test]
fn a_cause_on_a_name_moved_by_the_change_from_one_that_could_not_be_told_is_uncertain() {
    use crate::domain::nonempty::NonBlank;
    use crate::domain::shelf::fixtures_tests::{approve, by, file_space, jid, ledger, name};
    use crate::domain::shelf::ledger::{Moved, MovedOf, Record};

    let docs = |tests: &str| vec![fr(1, &[]), sp_testing(2, &["FR-1"], &[tests])];
    let then = shelf_with(docs("tests/a.rs"), &[("tests/a.rs", Some(h("a")))]);
    let first = approve(&then, &ledger(vec![]), 1, 1001, 10, None);
    let second = approve(&then, &ledger(vec![first.clone()]), 2, 1002, 10, None);
    let moved = Record::Moved(Moved::new(
        jid(2001),
        MovedOf::new(file_space(), name("tests/a.rs"), name("tests/b.rs")).unwrap(),
        by(),
        JudgedAt::new(50),
        NonBlank::new("the file was renamed").unwrap(),
    ));
    let now = shelf_with(docs("tests/b.rs"), &[("tests/b.rs", Some(h("edited")))]);
    let roots = assess(
        &now,
        &ledger(vec![first, second, moved]),
        JudgedAt::new(100),
    )
    .roots_of(docid(2));
    let cause = roots
        .into_iter()
        .find(|root| matches!(root.cause, Cause::ArtifactChanged { .. }))
        .expect("the edit of the file it moved to");
    // The end before could not tell the name as it was written then.
    let was = cannot_tell(vec![pin("tests/a.rs")]);
    let is = not_holding(vec![cause.clone()], Vec::new());
    let Made::CannotTell(why) = made_of(&was, &is, &was) else {
        panic!("cannot tell, not yes");
    };
    assert_eq!(why.side(), Sides::Before);
    assert_eq!(why.uncertain(), [cause]);
}

#[test]
fn a_cause_reached_another_way_is_no_new_cause() {
    let cause = artifact_cause();
    let was = not_holding(vec![cause.clone()], Vec::new());
    let mut other_way = cause.clone();
    other_way.path.insert(0, docid(9));
    let is = not_holding(vec![other_way], Vec::new());
    assert_eq!(made_of(&was, &is, &was), Made::No);
}

#[test]
fn a_cause_of_the_documents_own_text_is_certain() {
    let was = Holding::Holds(HoldsAs::Fresh);
    let is = not_holding(vec![own_cause()], Vec::new());
    let Made::Yes(yes) = made_of(&was, &is, &was) else {
        panic!("yes");
    };
    assert!(yes.uncertain().is_empty());
    // An artifact cause on a name the side before could tell is certain too.
    let is = not_holding(vec![artifact_cause()], Vec::new());
    assert!(matches!(made_of(&was, &is, &was), Made::Yes(_)));
}

#[test]
fn a_change_read_at_its_two_ends_answers_every_document() {
    let docs = || vec![fr(1, &[]), sp_testing(2, &["FR-1"], &["tests/a.rs"])];
    let then = shelf_with(docs(), &[("tests/a.rs", Some(h("a")))]);
    let ledger = approved(&then, &[1, 2]);
    let now = shelf_with(docs(), &[("tests/a.rs", Some(h("edited")))]);
    let at = JudgedAt::new(100);
    let (before, after) = (assess(&then, &ledger, at), assess(&now, &ledger, at));
    let entries = impact((&then, &before), (&now, &after));
    assert_eq!(
        entries.iter().map(ImpactEntry::id).collect::<Vec<_>>(),
        [docid(1), docid(2)]
    );
    assert_eq!(entries[0].made(), &Made::No);
    assert!(!entries[0].changed());
    assert!(matches!(entries[1].made(), Made::Yes(_)));
    assert!(entries[1].changed());
    assert_eq!(entries[1].before(), &Holding::Holds(HoldsAs::Fresh));
}
