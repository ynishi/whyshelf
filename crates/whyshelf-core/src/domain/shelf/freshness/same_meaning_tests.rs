//! Tests of freshness when people declared that an edit keeps the meaning: the edit
//! is a notice and not a cause, until it is edited again or the declaration is
//! withdrawn.

use std::collections::BTreeSet;

use crate::domain::nonempty::{NonBlank, NonEmptySet};
use crate::domain::shelf::artifact::ArtifactHash;
use crate::domain::shelf::fixtures_tests::{
    actor, approve, docid, fr, fr_titled, jid, ledger, path, shelf, shelf_with, sp_testing,
};
use crate::domain::shelf::ledger::{
    JudgedAt, Record, Revocation, SameMeaning, SameMeaningOf, SameMeaningTarget, frame_same_meaning,
};

use super::*;

fn h(text: &str) -> ArtifactHash {
    ArtifactHash::of_content(text.as_bytes())
}

/// The approvals of `subjects` in order on `shelf`, with judgement ids
/// `1000 + subject`.
fn approvals(shelf: &Shelf, subjects: &[u128]) -> Vec<Record> {
    let mut records = Vec::new();
    for subject in subjects {
        let so_far = ledger(records.clone());
        records.push(approve(shelf, &so_far, *subject, 1000 + subject, 10, None));
    }
    records
}

fn declaration(judgement: u128, of: SameMeaningOf) -> Record {
    Record::SameMeaning(
        SameMeaning::new(
            jid(judgement),
            of,
            actor(),
            JudgedAt::new(50),
            NonBlank::new("a typo fixed").unwrap(),
        )
        .unwrap(),
    )
}

/// `records` and the declaration about `target` that `frame_same_meaning` frames on
/// `now`.
fn declared(
    now: &Shelf,
    mut records: Vec<Record>,
    target: SameMeaningTarget,
    judgement: u128,
) -> Vec<Record> {
    let of = frame_same_meaning(now, &ledger(records.clone()), target, None).unwrap();
    records.push(declaration(judgement, of));
    records
}

fn revoke(judgement: u128, target: u128) -> Record {
    Record::Revocation(Revocation::new(
        jid(judgement),
        NonEmptySet::from_set(BTreeSet::from([jid(target)])).unwrap(),
        actor(),
        JudgedAt::new(60),
        NonBlank::new("not the same after all").unwrap(),
    ))
}

fn causes_of(assessment: &Assessment, n: u128) -> Vec<Cause> {
    match assessment.standing_of(docid(n)) {
        Standing::Approved(Freshness::Fresh) => Vec::new(),
        Standing::Approved(Freshness::Degraded(d)) => d.causes().iter().cloned().collect(),
        other => panic!("not an approval: {other:?}"),
    }
}

fn hash_of(shelf: &Shelf, n: u128) -> DocHash {
    *shelf.doc(docid(n)).unwrap().hash()
}

/// FR-1, FR-2 on FR-1 and FR-3 on FR-2, with FR-1 titled `title`.
fn chain(title: &str) -> Shelf {
    shelf(vec![
        fr_titled(1, title, &[]),
        fr(2, &["FR-1"]),
        fr(3, &["FR-2"]),
    ])
}

#[test]
fn a_declared_edit_keeps_the_document_and_what_stands_on_it_fresh_with_notices() {
    let then = chain("a");
    let now = chain("b");
    let records = declared(
        &now,
        approvals(&then, &[1, 2, 3]),
        SameMeaningTarget::Doc(docid(1)),
        2001,
    );
    let assessment = assess(&now, &ledger(records), JudgedAt::new(100));

    for n in [1, 2, 3] {
        assert_eq!(
            assessment.standing_of(docid(n)),
            Standing::Approved(Freshness::Fresh),
            "FR-{n}"
        );
        assert!(assessment.roots_of(docid(n)).is_empty(), "FR-{n}");
    }
    let expected = NonEmptyVec::new(jid(2001), Vec::new());
    assert_eq!(
        assessment.notices_of(docid(1)),
        [Notice::SelfSameMeaning {
            approved: hash_of(&then, 1),
            now: hash_of(&now, 1),
            declared: expected.clone(),
        }]
    );
    assert!(
        matches!(assessment.notices_of(docid(2)), [Notice::UpstreamSameMeaning { pin, now: at, declared }]
            if pin.target() == docid(1)
                && *pin.hash() == hash_of(&then, 1)
                && *at == hash_of(&now, 1)
                && *declared == expected),
        "{:?}",
        assessment.notices_of(docid(2))
    );
    // FR-3 pinned FR-2, which is unchanged: nothing to tell.
    assert!(assessment.notices_of(docid(3)).is_empty());
}

#[test]
fn when_both_comparisons_are_kept_there_is_no_root_anywhere() {
    // FR-1's own approval and FR-2's pin both saw "a"; "a" -> "b" is declared.
    let then = chain("a");
    let now = chain("b");
    let records = declared(
        &now,
        approvals(&then, &[1, 2]),
        SameMeaningTarget::Doc(docid(1)),
        2001,
    );
    let assessment = assess(&now, &ledger(records), JudgedAt::new(100));
    for n in [1, 2] {
        assert_eq!(assessment.roots_of(docid(n)), Vec::new(), "FR-{n}");
    }
}

#[test]
fn a_rewrite_approved_again_then_a_declared_typo_fix_leaves_the_older_pin_a_root() {
    // FR-2 pinned FR-1 at "a"; FR-1 was rewritten and approved again at "b"; then a
    // typo fix to "c" is declared. Only b -> c is declared: FR-2's a -> c is a change.
    let at_a = chain("a");
    let at_b = chain("b");
    let at_c = chain("c");
    let mut records = approvals(&at_a, &[1, 2]);
    records.push(approve(&at_b, &ledger(records.clone()), 1, 1003, 20, None));
    let records = declared(&at_c, records, SameMeaningTarget::Doc(docid(1)), 2001);
    let assessment = assess(&at_c, &ledger(records), JudgedAt::new(100));

    assert_eq!(
        assessment.standing_of(docid(1)),
        Standing::Approved(Freshness::Fresh)
    );
    assert!(matches!(
        assessment.notices_of(docid(1)),
        [Notice::SelfSameMeaning { approved, .. }] if *approved == hash_of(&at_b, 1)
    ));
    let causes = causes_of(&assessment, 2);
    assert!(
        matches!(causes.as_slice(), [Cause::UpstreamChanged { pin, now }]
            if pin.target() == docid(1)
                && *pin.hash() == hash_of(&at_a, 1)
                && *now == hash_of(&at_c, 1)),
        "{causes:?}"
    );
    assert!(assessment.notices_of(docid(2)).is_empty());

    // The root is at FR-2 itself: FR-1's own edit is no cause.
    let roots = assessment.roots_of(docid(2));
    assert_eq!(roots.len(), 1, "{roots:?}");
    assert_eq!(roots[0].path, vec![docid(2)]);
    assert!(matches!(
        roots[0].cause,
        Cause::UpstreamChanged { ref pin, .. } if pin.target() == docid(1)
    ));
    assert!(assessment.roots_of(docid(1)).is_empty());
}

#[test]
fn an_edit_after_the_declaration_is_a_cause_again() {
    let then = chain("a");
    let records = declared(
        &chain("b"),
        approvals(&then, &[1, 2, 3]),
        SameMeaningTarget::Doc(docid(1)),
        2001,
    );
    let again = chain("c");
    let assessment = assess(&again, &ledger(records), JudgedAt::new(100));

    assert!(
        causes_of(&assessment, 1)
            .iter()
            .any(|c| matches!(c, Cause::SelfChanged { .. }))
    );
    assert!(
        causes_of(&assessment, 2)
            .iter()
            .any(|c| matches!(c, Cause::UpstreamChanged { pin, .. } if pin.target() == docid(1)))
    );
    for n in [1, 2] {
        assert!(assessment.notices_of(docid(n)).is_empty(), "FR-{n}");
    }
    let roots = assessment.roots_of(docid(2));
    assert_eq!(roots.len(), 1, "{roots:?}");
    assert!(matches!(roots[0].cause, Cause::SelfChanged { .. }));
}

#[test]
fn a_withdrawn_declaration_makes_the_edit_a_cause_again() {
    let then = chain("a");
    let now = chain("b");
    let mut records = declared(
        &now,
        approvals(&then, &[1, 2]),
        SameMeaningTarget::Doc(docid(1)),
        2001,
    );
    records.push(revoke(3001, 2001));
    let assessment = assess(&now, &ledger(records), JudgedAt::new(100));
    assert!(
        causes_of(&assessment, 1)
            .iter()
            .any(|c| matches!(c, Cause::SelfChanged { .. }))
    );
    assert!(
        causes_of(&assessment, 2)
            .iter()
            .any(|c| matches!(c, Cause::UpstreamChanged { .. }))
    );
    assert!(assessment.notices_of(docid(2)).is_empty());
}

#[test]
fn a_way_of_two_declarations_is_named_in_order() {
    let then = chain("a");
    let between = chain("b");
    let now = chain("c");
    let mut records = approvals(&then, &[1, 2]);
    // A -> B, then B -> C, written by hand: `frame_same_meaning` would join A to C
    // (FR-2's pin, the one earlier hash).
    for (judgement, from, to) in [(2001, &then, &between), (2002, &between, &now)] {
        records.push(declaration(
            judgement,
            SameMeaningOf::Doc {
                id: docid(1),
                from: NonEmptySet::from_set(BTreeSet::from([hash_of(from, 1)])).unwrap(),
                to: hash_of(to, 1),
            },
        ));
    }
    let assessment = assess(&now, &ledger(records), JudgedAt::new(100));
    assert_eq!(
        assessment.standing_of(docid(2)),
        Standing::Approved(Freshness::Fresh)
    );
    let expected = NonEmptyVec::new(jid(2001), vec![jid(2002)]);
    assert!(
        matches!(assessment.notices_of(docid(2)), [Notice::UpstreamSameMeaning { declared, .. }] if *declared == expected),
        "{:?}",
        assessment.notices_of(docid(2))
    );
}

#[test]
fn a_declaration_about_another_document_keeps_no_edit() {
    let then = chain("a");
    let now = chain("b");
    let mut records = approvals(&then, &[1, 2]);
    // The hashes of FR-1, said of FR-2.
    records.push(declaration(
        2001,
        SameMeaningOf::Doc {
            id: docid(2),
            from: NonEmptySet::from_set(BTreeSet::from([hash_of(&then, 1)])).unwrap(),
            to: hash_of(&now, 1),
        },
    ));
    let assessment = assess(&now, &ledger(records), JudgedAt::new(100));
    assert!(
        causes_of(&assessment, 1)
            .iter()
            .any(|c| matches!(c, Cause::SelfChanged { .. }))
    );
    assert!(assessment.notices_of(docid(1)).is_empty());
}

/// FR-1 and SP-2 on it, tested by `tests/a.rs` with this content.
fn spec_shelf(content: &str) -> Shelf {
    shelf_with(
        vec![fr(1, &[]), sp_testing(2, &["FR-1"], &["tests/a.rs"])],
        &[("tests/a.rs", Some(h(content)))],
    )
}

#[test]
fn a_declared_change_of_a_file_is_a_notice_and_not_a_cause() {
    let then = spec_shelf("a");
    let now = spec_shelf("b");
    let records = declared(
        &now,
        approvals(&then, &[1, 2]),
        SameMeaningTarget::Artifact(path("tests/a.rs")),
        2001,
    );
    let assessment = assess(&now, &ledger(records.clone()), JudgedAt::new(100));
    assert!(causes_of(&assessment, 2).is_empty());
    assert!(assessment.roots_of(docid(2)).is_empty());
    assert!(
        matches!(assessment.notices_of(docid(2)), [Notice::ArtifactSameMeaning { pin, now, declared }]
            if pin.path() == &path("tests/a.rs")
                && *now == h("b")
                && declared.clone().into_vec() == [jid(2001)]),
        "{:?}",
        assessment.notices_of(docid(2))
    );

    // Changed again, it is a cause.
    let again = spec_shelf("c");
    let assessment = assess(&again, &ledger(records), JudgedAt::new(100));
    let causes = causes_of(&assessment, 2);
    assert!(
        matches!(causes.as_slice(), [Cause::ArtifactChanged { now, .. }] if *now == h("c")),
        "{causes:?}"
    );
    assert!(assessment.notices_of(docid(2)).is_empty());
}

#[test]
fn a_declaration_about_another_path_keeps_no_change_of_a_file() {
    let then = spec_shelf("a");
    let now = spec_shelf("b");
    let mut records = approvals(&then, &[1, 2]);
    records.push(declaration(
        2001,
        SameMeaningOf::Artifact {
            path: path("tests/other.rs"),
            from: NonEmptySet::from_set(BTreeSet::from([h("a")])).unwrap(),
            to: h("b"),
        },
    ));
    let assessment = assess(&now, &ledger(records), JudgedAt::new(100));
    assert!(
        matches!(
            causes_of(&assessment, 2).as_slice(),
            [Cause::ArtifactChanged { .. }]
        ),
        "{:?}",
        causes_of(&assessment, 2)
    );
}
