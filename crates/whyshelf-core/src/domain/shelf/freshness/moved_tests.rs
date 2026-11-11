//! Tests of freshness when people declared that an artifact moved: a pin of the old
//! name, made before, is read as a pin of the new one, with the hash it pinned.

use std::collections::BTreeSet;

use crate::domain::nonempty::{NonBlank, NonEmptySet};
use crate::domain::shelf::artifact::{ArtifactHash, Space};
use crate::domain::shelf::fixtures_tests::{
    actor, approve, docid, file_artifact, file_space, fr, jid, ledger, name, shelf_with, sp_testing,
};
use crate::domain::shelf::ledger::{
    JudgedAt, Moved, MovedOf, Record, SameMeaning, SameMeaningOf, SameMeaningTarget,
    frame_same_meaning,
};

use super::*;

fn h(text: &str) -> ArtifactHash {
    ArtifactHash::of_content(text.as_bytes())
}

/// FR-1 and SP-2 on it, its `tests` link writing `tests`, with these files there.
fn spec(tests: &[&str], files: &[(&str, &str)]) -> Shelf {
    let files: Vec<(&str, Option<ArtifactHash>)> = files
        .iter()
        .map(|(at, text)| (*at, Some(h(text))))
        .collect();
    shelf_with(vec![fr(1, &[]), sp_testing(2, &["FR-1"], tests)], &files)
}

/// The approvals of FR-1 and SP-2 on `shelf`, at `at`.
fn approvals(shelf: &Shelf, at: i64) -> Vec<Record> {
    let first = approve(shelf, &ledger(vec![]), 1, 1001, at, None);
    let second = approve(shelf, &ledger(vec![first.clone()]), 2, 1002, at, None);
    vec![first, second]
}

fn moved_in(space: Space, judgement: u128, from: &str, to: &str, at: i64) -> Record {
    Record::Moved(Moved::new(
        jid(judgement),
        MovedOf::new(space, name(from), name(to)).unwrap(),
        actor(),
        JudgedAt::new(at),
        NonBlank::new("the module was split").unwrap(),
    ))
}

fn moved(judgement: u128, from: &str, to: &str, at: i64) -> Record {
    moved_in(file_space(), judgement, from, to, at)
}

fn causes_of(assessment: &Assessment, n: u128) -> Vec<Cause> {
    match assessment.standing_of(docid(n)) {
        Standing::Approved(Freshness::Fresh) => Vec::new(),
        Standing::Approved(Freshness::Degraded(d)) => d.causes().iter().cloned().collect(),
        other => panic!("not an approval: {other:?}"),
    }
}

fn assessed(now: &Shelf, records: Vec<Record>) -> Assessment {
    assess(now, &ledger(records), JudgedAt::new(100))
}

#[test]
fn an_unchanged_move_leaves_the_approval_as_it_was() {
    let then = spec(&["tests/a.rs"], &[("tests/a.rs", "a")]);
    let now = spec(&["tests/b.rs"], &[("tests/b.rs", "a")]);
    let mut records = approvals(&then, 10);
    // Before the declaration: the link names a file the approval did not pin.
    assert!(matches!(
        causes_of(&assessed(&now, records.clone()), 2).as_slice(),
        [Cause::ArtifactLinksChanged { .. }]
    ));

    records.push(moved(2001, "tests/a.rs", "tests/b.rs", 50));
    let assessment = assessed(&now, records);
    assert_eq!(
        assessment.standing_of(docid(2)),
        Standing::Approved(Freshness::Fresh)
    );
    assert!(assessment.notices_of(docid(2)).is_empty());
}

#[test]
fn a_chain_of_moves_is_read_to_its_end() {
    let then = spec(&["tests/a.rs"], &[("tests/a.rs", "a")]);
    let now = spec(&["tests/c.rs"], &[("tests/c.rs", "a")]);
    let mut records = approvals(&then, 10);
    records.push(moved(2001, "tests/a.rs", "tests/b.rs", 50));
    records.push(moved(2002, "tests/b.rs", "tests/c.rs", 60));
    assert!(causes_of(&assessed(&now, records.clone()), 2).is_empty());

    // Without the last, the pin reads as b, which the link does not write: the link
    // dropped what it wrote then, a, and writes c.
    records.pop();
    assert!(matches!(
        causes_of(&assessed(&now, records), 2).as_slice(),
        [Cause::ArtifactLinksChanged { added, removed, .. }]
            if added == &[file_artifact("tests/c.rs")] && removed == &[file_artifact("tests/a.rs")]
    ));
}

#[test]
fn a_link_that_drops_a_moved_pin_names_what_it_wrote() {
    // The link [a] becomes [x] after a -> b.
    let then = spec(&["tests/a.rs"], &[("tests/a.rs", "a")]);
    let mut records = approvals(&then, 10);
    records.push(moved(2001, "tests/a.rs", "tests/b.rs", 50));
    let now = spec(&["tests/x.rs"], &[("tests/b.rs", "a"), ("tests/x.rs", "x")]);
    let causes = causes_of(&assessed(&now, records), 2);
    assert!(
        matches!(causes.as_slice(), [Cause::ArtifactLinksChanged { added, removed, .. }]
            if added == &[file_artifact("tests/x.rs")] && removed == &[file_artifact("tests/a.rs")]),
        "{causes:?}"
    );
}

#[test]
fn a_pin_of_a_name_as_absent_is_read_as_written() {
    // Approved with nothing at a.rs; a move of a.rs declared after (of some other
    // approval's pin) moves nothing of it.
    let then = spec(&["tests/a.rs"], &[]);
    let mut records = approvals(&then, 10);
    records.push(moved(2001, "tests/a.rs", "tests/b.rs", 50));
    let l = ledger(records.clone());
    let read = l.read_pins(docid(2));
    let [pin] = read.as_slice() else {
        panic!("one pin: {read:?}");
    };
    assert_eq!(pin.moved(), None);
    assert_eq!(pin.read_as(), &file_artifact("tests/a.rs"));
    // Still nothing at a.rs: nothing changed for it, and b.rs is not its.
    let now = spec(&["tests/a.rs"], &[("tests/b.rs", "b")]);
    assert!(causes_of(&assessed(&now, records), 2).is_empty());
}

#[test]
fn a_move_with_an_edit_is_a_change_at_the_new_name() {
    let then = spec(&["tests/a.rs"], &[("tests/a.rs", "a")]);
    let now = spec(&["tests/b.rs"], &[("tests/b.rs", "edited")]);
    let mut records = approvals(&then, 10);
    records.push(moved(2001, "tests/a.rs", "tests/b.rs", 50));
    let assessment = assessed(&now, records);
    let causes = causes_of(&assessment, 2);
    let [Cause::ArtifactChanged { pin, now: hash }] = causes.as_slice() else {
        panic!("one change: {causes:?}");
    };
    assert_eq!(*hash, h("edited"));
    assert_eq!(pin.pin().artifact(), &file_artifact("tests/a.rs"));
    assert_eq!(pin.read_as(), &file_artifact("tests/b.rs"));
    assert_eq!(
        pin.moved().map(|m| m.declared().into_vec()),
        Some(vec![jid(2001)])
    );
    // Stale, not broken; and its origin is the new name.
    assert_eq!(
        assessment.standing_of(docid(2)).kind(),
        StandingKind::Approved(FreshnessLevel::Stale)
    );
    assert_eq!(
        causes[0].origin(docid(2)),
        Some(Origin::Artifact(file_artifact("tests/b.rs")))
    );
    let origins = assessment.origins();
    assert_eq!(origins.len(), 1);
    assert_eq!(
        origins[0].origin,
        Origin::Artifact(file_artifact("tests/b.rs"))
    );
}

#[test]
fn an_edit_with_a_move_can_be_declared_to_keep_the_meaning_at_the_new_name() {
    let then = spec(&["tests/a.rs"], &[("tests/a.rs", "a")]);
    let now = spec(&["tests/b.rs"], &[("tests/b.rs", "a ")]);
    let mut records = approvals(&then, 10);
    records.push(moved(2001, "tests/a.rs", "tests/b.rs", 50));
    let of = frame_same_meaning(
        &now,
        &ledger(records.clone()),
        SameMeaningTarget::Artifact(name("tests/b.rs")),
        None,
    )
    .unwrap();
    assert!(
        matches!(&of, SameMeaningOf::Artifact { artifact, .. } if *artifact == file_artifact("tests/b.rs"))
    );
    records.push(Record::SameMeaning(
        SameMeaning::new(
            jid(2002),
            of,
            actor(),
            JudgedAt::new(60),
            NonBlank::new("formatting").unwrap(),
        )
        .unwrap(),
    ));
    let assessment = assessed(&now, records);
    assert!(causes_of(&assessment, 2).is_empty());
    assert!(matches!(
        assessment.notices_of(docid(2)),
        [Notice::ArtifactSameMeaning { .. }]
    ));
}

#[test]
fn an_edit_declared_before_the_move_still_keeps_the_meaning_after_it() {
    let then = spec(&["tests/a.rs"], &[("tests/a.rs", "a")]);
    let now = spec(&["tests/b.rs"], &[("tests/b.rs", "a ")]);
    let mut records = approvals(&then, 10);
    records.push(Record::SameMeaning(
        SameMeaning::new(
            jid(2001),
            SameMeaningOf::Artifact {
                artifact: file_artifact("tests/a.rs"),
                from: NonEmptySet::from_set(BTreeSet::from([h("a")])).unwrap(),
                to: h("a "),
            },
            actor(),
            JudgedAt::new(40),
            NonBlank::new("formatting").unwrap(),
        )
        .unwrap(),
    ));
    records.push(moved(2002, "tests/a.rs", "tests/b.rs", 50));
    let assessment = assessed(&now, records);
    assert!(causes_of(&assessment, 2).is_empty());
}

#[test]
fn a_pin_made_after_the_declaration_is_read_as_written() {
    // Approved after the move was declared, on a link that still wrote a.rs, with
    // nothing there: the pin of a.rs is of a.rs.
    let then = spec(&["tests/a.rs"], &[]);
    let mut records = vec![moved(2001, "tests/a.rs", "tests/b.rs", 5)];
    records.extend(approvals(&then, 10));
    let now = spec(&["tests/a.rs"], &[("tests/b.rs", "a")]);
    let assessment = assessed(&now, records);
    assert!(causes_of(&assessment, 2).is_empty());
}

#[test]
fn a_move_in_another_space_reads_no_pin_of_this_one() {
    let then = spec(&["tests/a.rs"], &[("tests/a.rs", "a")]);
    let now = spec(&["tests/b.rs"], &[("tests/b.rs", "a")]);
    let mut records = approvals(&then, 10);
    records.push(moved_in(
        Space::new("other").unwrap(),
        2001,
        "tests/a.rs",
        "tests/b.rs",
        50,
    ));
    assert!(matches!(
        causes_of(&assessed(&now, records), 2).as_slice(),
        [Cause::ArtifactLinksChanged { .. }]
    ));
}

#[test]
fn a_link_still_on_the_old_name_is_one_cause_and_no_change_of_the_link() {
    let then = spec(&["tests/a.rs"], &[("tests/a.rs", "a")]);
    let now = spec(&["tests/a.rs"], &[("tests/b.rs", "a")]);
    let mut records = approvals(&then, 10);
    // Before the declaration: gone, and broken.
    assert!(matches!(
        causes_of(&assessed(&now, records.clone()), 2).as_slice(),
        [Cause::ArtifactGone { .. }]
    ));

    records.push(moved(2001, "tests/a.rs", "tests/b.rs", 50));
    let assessment = assessed(&now, records);
    let causes = causes_of(&assessment, 2);
    let [Cause::ArtifactMoved { link, written, pin }] = causes.as_slice() else {
        panic!("one cause, and no links_changed: {causes:?}");
    };
    assert_eq!(link, &EntryName::new("tests").unwrap());
    assert_eq!(written, &file_artifact("tests/a.rs"));
    assert_eq!(pin.read_as(), &file_artifact("tests/b.rs"));
    assert_eq!(
        pin.moved().map(|m| m.declared().into_vec()),
        Some(vec![jid(2001)])
    );
    assert_eq!(
        assessment.standing_of(docid(2)).kind(),
        StandingKind::Approved(FreshnessLevel::Stale)
    );
    assert_eq!(causes[0].origin(docid(2)), Some(Origin::Doc(docid(2))));
    // The old name has nothing behind it, which is its own notice, and the only one.
    assert!(matches!(
        assessment.notices_of(docid(2)),
        [Notice::ArtifactAbsent { .. }]
    ));
}

#[test]
fn a_link_on_a_middle_name_of_a_chain_is_one_cause() {
    // a -> b; the link rewritten to b; then b -> c.
    let then = spec(&["tests/a.rs"], &[("tests/a.rs", "a")]);
    let mut records = approvals(&then, 10);
    records.push(moved(2001, "tests/a.rs", "tests/b.rs", 50));
    records.push(moved(2002, "tests/b.rs", "tests/c.rs", 60));
    let now = spec(&["tests/b.rs"], &[("tests/c.rs", "a")]);
    let causes = causes_of(&assessed(&now, records), 2);
    assert!(
        matches!(causes.as_slice(), [Cause::ArtifactMoved { written, pin, .. }]
            if written == &file_artifact("tests/b.rs")
                && pin.read_as() == &file_artifact("tests/c.rs")),
        "{causes:?}"
    );
}

#[test]
fn a_link_on_the_middle_name_of_a_way_back_is_one_cause() {
    // a -> b, then b -> a: the pin is read as a, and a link on b is behind it.
    let then = spec(&["tests/a.rs"], &[("tests/a.rs", "a")]);
    let mut records = approvals(&then, 10);
    records.push(moved(2001, "tests/a.rs", "tests/b.rs", 50));
    records.push(moved(2002, "tests/b.rs", "tests/a.rs", 60));
    let now = spec(&["tests/b.rs"], &[("tests/a.rs", "a")]);
    let causes = causes_of(&assessed(&now, records.clone()), 2);
    assert!(
        matches!(causes.as_slice(), [Cause::ArtifactMoved { written, pin, .. }]
            if written == &file_artifact("tests/b.rs")
                && pin.read_as() == &file_artifact("tests/a.rs")),
        "{causes:?}"
    );
    // On a, as pinned, it holds.
    let now = spec(&["tests/a.rs"], &[("tests/a.rs", "a")]);
    assert!(causes_of(&assessed(&now, records), 2).is_empty());
}

#[test]
fn of_two_moves_of_one_name_at_one_time_the_larger_identity_is_followed() {
    let then = spec(&["tests/a.rs"], &[("tests/a.rs", "a")]);
    let mut records = approvals(&then, 10);
    records.push(moved(2002, "tests/a.rs", "tests/c.rs", 50));
    records.push(moved(2001, "tests/a.rs", "tests/b.rs", 50));
    let now = spec(&["tests/c.rs"], &[("tests/c.rs", "a")]);
    assert!(causes_of(&assessed(&now, records), 2).is_empty());
}

#[test]
fn the_old_name_there_again_is_another_thing_and_the_link_is_still_told() {
    let then = spec(&["tests/a.rs"], &[("tests/a.rs", "a")]);
    let mut records = approvals(&then, 10);
    records.push(moved(2001, "tests/a.rs", "tests/b.rs", 50));
    // A new a.rs, and the link left on it.
    let now = spec(
        &["tests/a.rs"],
        &[("tests/a.rs", "new"), ("tests/b.rs", "a")],
    );
    let assessment = assessed(&now, records.clone());
    assert!(matches!(
        causes_of(&assessment, 2).as_slice(),
        [Cause::ArtifactMoved { .. }]
    ));
    // Approved again on it, the new pin is read as written.
    let mut again = records;
    let second = approve(&now, &ledger(again.clone()), 2, 1003, 60, None);
    again.push(second);
    let assessment = assessed(&now, again);
    assert!(causes_of(&assessment, 2).is_empty());
    assert!(assessment.notices_of(docid(2)).is_empty());
}

#[test]
fn a_move_onto_a_name_another_approval_pins_is_a_change_of_that_one() {
    // SP-2 pinned a.rs and b.rs; a.rs was then moved over b.rs.
    let then = spec(
        &["tests/a.rs", "tests/b.rs"],
        &[("tests/a.rs", "a"), ("tests/b.rs", "b")],
    );
    let mut records = approvals(&then, 10);
    records.push(moved(2001, "tests/a.rs", "tests/b.rs", 50));
    let now = spec(&["tests/b.rs"], &[("tests/b.rs", "a")]);
    let assessment = assessed(&now, records);
    // The pin of a.rs reads as b.rs and holds; the pin of b.rs sees another content.
    let causes = causes_of(&assessment, 2);
    assert!(
        matches!(causes.as_slice(), [Cause::ArtifactChanged { pin, .. }]
            if pin.pin().artifact() == &file_artifact("tests/b.rs") && pin.moved().is_none()),
        "{causes:?}"
    );
}
