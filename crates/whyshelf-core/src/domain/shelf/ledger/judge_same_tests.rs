//! Tests of what `judge same` offers to declare and what `check` tells as short of
//! names, beside what stale tells of the same pins: both read the pins stale compares
//! now, join two hashes along the moves of each pin that holds them, and refuse, with
//! why, what stale does not compare.

use std::collections::BTreeSet;

use crate::domain::actor::{Actor, Attribution};
use crate::domain::hash::{ContentHash, HashKind};
use crate::domain::shelf::artifact::{ArtifactHash, ArtifactReading};
use crate::domain::shelf::build::DocReading;
use crate::domain::shelf::fixtures_tests::{
    admitted, approve, approve_as, asking, by, docid, file_artifact, file_space, fr, fr_titled,
    jid, ledger, list, name, reading, ruled_schema_of_lines, ruled_shelf, ruled_shelf_answered,
    ruled_shelf_with, shelf_answered, shelf_reading, shelf_with, sp_testing, text,
};
use crate::domain::shelf::freshness::{Cause, Freshness, Notice, Standing, assess_unasked};

use super::compared::{Compared, ComparedNow, Content, DocPinRole, NotCompared, PinRef, PinsNow};
use super::*;

fn h(text: &str) -> ArtifactHash {
    ArtifactHash::of_content(text.as_bytes())
}

fn because() -> NonBlank {
    NonBlank::new("formatting").unwrap()
}

fn moved(judgement: u128, from: &str, to: &str, at: i64) -> Record {
    moved_by(by(), judgement, from, to, at)
}

/// A declaration by `who` that the file at `from` moved to `to`.
fn moved_by(who: Attribution, judgement: u128, from: &str, to: &str, at: i64) -> Record {
    Record::Moved(Moved::new(
        jid(judgement),
        MovedOf::new(file_space(), name(from), name(to)).unwrap(),
        who,
        JudgedAt::new(at),
        because(),
    ))
}

/// The edit of the artifact at `at` from `from` to `to`.
fn edit_of(at: &str, from: &[ArtifactHash], to: ArtifactHash) -> SameMeaningOf {
    SameMeaningOf::Artifact {
        artifact: file_artifact(at),
        from: NonEmptySet::from_set(from.iter().copied().collect()).unwrap(),
        to,
    }
}

/// A declaration of [`edit_of`] by `who`, at 50.
fn declared_by(judgement: u128, who: &str, edit: SameMeaningOf) -> Record {
    Record::SameMeaning(
        SameMeaning::new(
            jid(judgement),
            edit,
            Attribution::one(Actor::new(who).unwrap()),
            JudgedAt::new(50),
            because(),
        )
        .unwrap(),
    )
}

/// A declaration of [`edit_of`] by the fixtures' actor.
fn declared(judgement: u128, at: &str, from: &[ArtifactHash], to: ArtifactHash) -> Record {
    Record::SameMeaning(
        SameMeaning::new(
            jid(judgement),
            edit_of(at, from, to),
            by(),
            JudgedAt::new(50),
            because(),
        )
        .unwrap(),
    )
}

/// What `judge same` offers of the artifact `at`, for the hash `named` or none.
fn offered(
    shelf: &Shelf,
    ledger: &Ledger,
    at: &str,
    named: Option<ArtifactHash>,
) -> Result<SameMeaningOf, SameMeaningRefusal> {
    frame_same_meaning(
        shelf,
        ledger,
        SameMeaningTarget::Artifact(name(at)),
        named.map(|hash| *hash.hash()),
    )
    .map(|framed| framed.of())
}

/// What `judge same` offers of the document `n`, for the hash `named` or none.
fn offered_doc(
    shelf: &Shelf,
    ledger: &Ledger,
    n: u128,
    named: Option<DocHash>,
) -> Result<SameMeaningOf, SameMeaningRefusal> {
    frame_same_meaning(
        shelf,
        ledger,
        SameMeaningTarget::Doc(docid(n)),
        named.map(|hash| *hash.hash()),
    )
    .map(|framed| framed.of())
}

/// The causes stale tells of `n`, none when it holds.
fn causes_of(now: &Shelf, l: &Ledger, n: u128) -> Vec<Cause> {
    match assess_unasked(now, l, JudgedAt::new(100)).standing_of(docid(n)) {
        Standing::Approved {
            freshness: Freshness::Degraded(degraded),
            ..
        } => degraded.causes().iter().cloned().collect(),
        Standing::Approved { .. } => Vec::new(),
        other => panic!("an approval stands about {n}, got {other:?}"),
    }
}

/// The approvals, in turn and at 10, of each of `subjects` on `shelf`, ids from `first`.
fn approvals(shelf: &Shelf, subjects: &[u128], first: u128) -> Vec<Record> {
    let mut records = Vec::new();
    for (i, subject) in subjects.iter().enumerate() {
        let so_far = ledger(records.clone());
        records.push(approve(
            shelf,
            &so_far,
            *subject,
            first + i as u128,
            10,
            None,
        ));
    }
    records
}

/// FR-1 and, for each name, a spec SP-n (n from 2, in turn) on it testing that name.
fn specs_testing(names: &[&str]) -> Vec<DocReading> {
    let mut docs = vec![fr(1, &[])];
    docs.extend(
        names
            .iter()
            .enumerate()
            .map(|(i, at)| sp_testing(2 + i as u128, &["FR-1"], &[at])),
    );
    docs
}

/// For each `(name, held)`, a spec testing `name` while it held `held`, approved with
/// FR-1 at 10; then each name declared moved to `tests/b.rs`, at 60. Each pin is read
/// as `tests/b.rs` along a way of its own.
fn ways(pins: &[(&str, &str)]) -> Vec<Record> {
    let names: Vec<&str> = pins.iter().map(|(at, _)| *at).collect();
    let files: Vec<(&str, Option<ArtifactHash>)> =
        pins.iter().map(|(at, held)| (*at, Some(h(held)))).collect();
    let then = shelf_with(specs_testing(&names), &files);
    let subjects: Vec<u128> = (1..=pins.len() as u128 + 1).collect();
    let mut records = approvals(&then, &subjects, 1001);
    for (i, at) in names.iter().enumerate() {
        records.push(moved(3001 + i as u128, at, "tests/b.rs", 60));
    }
    records
}

/// The shelf after [`ways`] of `n` pins: each spec tests `tests/b.rs`, which holds `now`.
fn ways_now(n: usize, now: &str) -> Shelf {
    shelf_with(
        specs_testing(&vec!["tests/b.rs"; n]),
        &[("tests/b.rs", Some(h(now)))],
    )
}

// The declarations of a pin's way, as stale reads them.

#[test]
fn two_hashes_joined_under_the_old_name_are_offered_together_at_the_new_one() {
    // SP-2 pinned `tests/a.rs` at h0, SP-3 at h1; h0 -> h1 was declared there, then
    // the file moved to `tests/b.rs`, which holds h2.
    let at = |held| {
        shelf_with(
            specs_testing(&["tests/a.rs", "tests/a.rs"]),
            &[("tests/a.rs", Some(h(held)))],
        )
    };
    let mut records = approvals(&at("h0"), &[1, 2], 1001);
    records.push(approve(
        &at("h1"),
        &ledger(records.clone()),
        3,
        1003,
        20,
        None,
    ));
    records.push(declared(2001, "tests/a.rs", &[h("h0")], h("h1")));
    records.push(moved(3001, "tests/a.rs", "tests/b.rs", 60));
    let l = ledger(records);
    let now = ways_now(2, "h2");
    for n in [2, 3] {
        assert!(
            matches!(
                causes_of(&now, &l, n).as_slice(),
                [Cause::ArtifactChanged { .. }]
            ),
            "{n}"
        );
    }
    assert_eq!(
        offered(&now, &l, "tests/b.rs", None),
        Ok(edit_of("tests/b.rs", &[h("h0"), h("h1")], h("h2")))
    );
}

// An edit is offered for a pin exactly when stale tells it as changed.

#[test]
fn a_declaration_along_another_pin_s_way_keeps_only_that_pin() {
    // P (`tests/a.rs` -> `tests/b.rs`) and Q (`tests/c.rs` -> `tests/b.rs`) both pinned
    // "h"; the edit is declared under `tests/c.rs` alone, Q's way.
    let mut records = ways(&[("tests/a.rs", "h"), ("tests/c.rs", "h")]);
    records.push(declared(2001, "tests/c.rs", &[h("h")], h("now")));
    let l = ledger(records);
    let now = ways_now(2, "now");
    assert!(matches!(
        causes_of(&now, &l, 2).as_slice(),
        [Cause::ArtifactChanged { .. }]
    ));
    assert!(causes_of(&now, &l, 3).is_empty());
    assert!(
        assess_unasked(&now, &l, JudgedAt::new(100))
            .notices_of(docid(3))
            .iter()
            .any(|n| matches!(n, Notice::ArtifactSameMeaning { .. }))
    );
    assert_eq!(
        offered(&now, &l, "tests/b.rs", None),
        Ok(edit_of("tests/b.rs", &[h("h")], h("now")))
    );
}

#[test]
fn the_second_step_leaves_a_hash_in_while_a_pin_that_holds_it_lacks_the_declaration() {
    // As above, under a rule that asks for two names: ana's declaration under Q's way
    // is short, so stale tells both pins as changed.
    let rule = || asking(2, None);
    let docs = |names: &[&str]| specs_testing(names);
    let then = ruled_shelf_with(
        rule(),
        docs(&["tests/a.rs", "tests/c.rs"]),
        &[("tests/a.rs", h("h")), ("tests/c.rs", h("h"))],
    );
    let mut records = Vec::new();
    for (i, subject) in [1, 2, 3].into_iter().enumerate() {
        let so_far = admitted(&then, records.clone());
        records.push(approve_as(
            &then,
            &so_far,
            subject,
            1001 + i as u128,
            ("ana", 10, None),
        ));
    }
    let ana_moved = |judgement, from| {
        moved_by(
            Attribution::one(Actor::new("ana").unwrap()),
            judgement,
            from,
            "tests/b.rs",
            60,
        )
    };
    records.push(ana_moved(3001, "tests/a.rs"));
    records.push(ana_moved(3002, "tests/c.rs"));
    let now = ruled_shelf_with(
        rule(),
        docs(&["tests/b.rs", "tests/b.rs"]),
        &[("tests/b.rs", h("now"))],
    );
    let ana = Actor::new("ana").unwrap();
    let edit = edit_of("tests/b.rs", &[h("h")], h("now"));
    let second = |declared_at: &str| {
        let mut records = records.clone();
        records.push(declared_by(
            2001,
            "ana",
            edit_of(declared_at, &[h("h")], h("now")),
        ));
        let l = admitted(&now, records);
        let framed = frame_same_meaning(
            &now,
            &l,
            SameMeaningTarget::Artifact(name("tests/b.rs")),
            None,
        )
        .unwrap();
        (framed.of(), framed.not_declared_by(&ana))
    };
    // Under Q's way alone, P lacks it: the whole command declares the edit for P.
    assert_eq!(second("tests/c.rs"), (edit.clone(), Ok(edit.clone())));
    // Under the name both ways share, ana declared it for both pins already.
    assert_eq!(
        second("tests/b.rs"),
        (
            edit,
            Err(SameMeaningRefusal::AlreadyDeclaredBy { actor: ana.clone() })
        )
    );
}

#[test]
fn another_hash_joins_the_earlier_one_only_along_the_way_of_each_pin_that_holds_it() {
    // P (`tests/a.rs`) pinned h1, Q (`tests/c.rs`) pinned h2, both read as `tests/b.rs`.
    let records = ways(&[("tests/a.rs", "h1"), ("tests/c.rs", "h2")]);
    let now = ways_now(2, "now");
    let with = |at: &str| {
        let mut records = records.clone();
        records.push(declared(2001, at, &[h("h2")], h("h1")));
        ledger(records)
    };
    let both = Ok(edit_of("tests/b.rs", &[h("h1"), h("h2")], h("now")));

    // Declared along P's way only: Q, which holds h2, lacks it, so h2 does not join h1.
    let on_p = with("tests/a.rs");
    assert_eq!(
        offered(&now, &on_p, "tests/b.rs", Some(h("h1"))),
        Ok(edit_of("tests/b.rs", &[h("h1")], h("now")))
    );
    // P, which holds h1, has it, so h1 joins h2.
    assert_eq!(offered(&now, &on_p, "tests/b.rs", Some(h("h2"))), both);
    assert_eq!(offered(&now, &on_p, "tests/b.rs", None), both);

    // Declared along Q's way: h2 joins h1.
    let on_q = with("tests/c.rs");
    assert_eq!(offered(&now, &on_q, "tests/b.rs", Some(h("h1"))), both);
    assert_eq!(offered(&now, &on_q, "tests/b.rs", None), both);
}

#[test]
fn several_earlier_hashes_are_told_whatever_their_order() {
    let now = ways_now(2, "now");
    let several = |l: &Ledger| {
        matches!(
            offered(&now, l, "tests/b.rs", None),
            Err(SameMeaningRefusal::SeveralEarlier { ref hashes }) if hashes.len() == 2
        )
    };
    // The contents swapped, so that each of the two hashes is the smaller once.
    for (one, two) in [("h1", "h2"), ("h2", "h1")] {
        let records = ways(&[("tests/a.rs", one), ("tests/c.rs", two)]);
        let both = Ok(edit_of("tests/b.rs", &[h(one), h(two)], h("now")));
        let with = |at: &str, from: &str, to: &str| {
            let mut records = records.clone();
            records.push(declared(2001, at, &[h(from)], h(to)));
            ledger(records)
        };
        // Joined along Q's way, or the other way round along P's: both.
        assert_eq!(
            offered(&now, &with("tests/c.rs", two, one), "tests/b.rs", None),
            both
        );
        assert_eq!(
            offered(&now, &with("tests/a.rs", one, two), "tests/b.rs", None),
            both
        );
        // Joined by nothing: several.
        assert!(several(&ledger(records.clone())), "{one} {two}");
    }

    // Three: h2 and h3 each joined to h1 along their own way are one edit; h3 joined to
    // nothing leaves several.
    let records = ways(&[
        ("tests/a.rs", "h1"),
        ("tests/c.rs", "h2"),
        ("tests/d.rs", "h3"),
    ]);
    let now = ways_now(3, "now");
    let mut joined = records.clone();
    joined.push(declared(2001, "tests/c.rs", &[h("h2")], h("h1")));
    let only_h2 = joined.clone();
    joined.push(declared(2002, "tests/d.rs", &[h("h3")], h("h1")));
    assert_eq!(
        offered(&now, &ledger(joined), "tests/b.rs", None),
        Ok(edit_of(
            "tests/b.rs",
            &[h("h1"), h("h2"), h("h3")],
            h("now")
        ))
    );
    assert!(matches!(
        offered(&now, &ledger(only_h2), "tests/b.rs", None),
        Err(SameMeaningRefusal::SeveralEarlier { ref hashes }) if hashes.len() == 3
    ));
}

/// What a pin of a document's group is, for the test of which pins it gathers.
fn kind(pin: &Compared<'_>) -> &'static str {
    match pin {
        Compared::Content(Content::Doc {
            role: DocPinRole::Own,
            ..
        }) => "own",
        Compared::Content(Content::Doc {
            role: DocPinRole::Upstream,
            ..
        }) => "upstream",
        Compared::NotCompared(NotCompared::SubjectNotValid {
            pin: PinRef::Upstream(..),
        }) => "upstream, of a document not valid",
        Compared::Content(Content::Artifact { .. })
        | Compared::Rest { .. }
        | Compared::NotCompared(..) => "another",
    }
}

#[test]
fn a_document_gathers_its_own_pin_and_every_upstream_pin_of_it_and_no_other() {
    // FR-3 pinned FR-1 at x, FR-1's own approval and FR-2 at y; FR-1 is z now. FR-3 and
    // FR-4 are not valid now, and FR-5 pinned FR-4.
    let invalid = |n: u128, base: &[&str]| {
        let mut extra = Vec::new();
        if !base.is_empty() {
            extra.push(("base", list(base)));
        }
        reading(
            &format!("findings/fr-{n}.md"),
            n,
            &format!("FR-{n}"),
            &extra,
        )
    };
    let rest = |valid: bool| {
        let (fr_3, fr_4) = if valid {
            (fr(3, &["FR-1"]), fr(4, &[]))
        } else {
            (invalid(3, &["FR-1"]), invalid(4, &[]))
        };
        vec![fr_3, fr_4, fr(5, &["FR-4"])]
    };
    let at = |title: &str, valid: bool| {
        let mut docs = vec![fr_titled(1, title, &[]), fr(2, &["FR-1"])];
        docs.extend(rest(valid));
        crate::domain::shelf::fixtures_tests::shelf(docs)
    };
    let at_x = at("x", true);
    let mut records = approvals(&at_x, &[1, 3, 4, 5], 1001);
    let at_y = at("y", true);
    for (subject, judgement) in [(1, 1011), (2, 1012)] {
        records.push(approve(
            &at_y,
            &ledger(records.clone()),
            subject,
            judgement,
            20,
            None,
        ));
    }
    let l = ledger(records);
    let now = at("z", false);
    let hash = |shelf: &Shelf, n: u128| *shelf.doc(docid(n)).unwrap().hash();

    let pins = PinsNow::of(&l, &now);
    let compared = ComparedNow::of(&pins);
    let group = compared.doc(docid(1)).map_err(|_| "FR-1 is valid").unwrap();
    let kinds: Vec<&str> = group.pins().iter().map(|pin| kind(pin)).collect();
    assert_eq!(
        kinds,
        ["own", "upstream", "upstream, of a document not valid"]
    );
    assert!(compared.doc(docid(4)).is_err());

    // Stale tells the hash offered as changed, in the own pin of FR-1 and in the
    // upstream pin of FR-2.
    assert!(causes_of(&now, &l, 1).iter().any(|cause| matches!(
        cause,
        Cause::SelfChanged { approved, now: seen_now }
            if *approved == hash(&at_y, 1) && *seen_now == hash(&now, 1)
    )));
    assert!(causes_of(&now, &l, 2).iter().any(|cause| matches!(
        cause,
        Cause::UpstreamChanged { pin, now: seen_now }
            if *pin.hash() == hash(&at_y, 1) && *seen_now == hash(&now, 1)
    )));

    // The own approval's y, and not FR-3's x, which stale does not compare.
    assert_eq!(
        offered_doc(&now, &l, 1, None),
        Ok(SameMeaningOf::Doc {
            id: docid(1),
            from: NonEmptySet::new(hash(&at_y, 1), []),
            to: hash(&now, 1),
        })
    );
    assert_eq!(
        offered_doc(&now, &l, 1, Some(hash(&at_x, 1))),
        Err(SameMeaningRefusal::NotComparedNow {
            hash: Some(hash(&at_x, 1).to_string()),
            why: NotComparedWhy::DocumentNotValid,
        })
    );
    // FR-4's own hash and FR-5's pin of it are no pins of FR-1.
    assert_eq!(
        offered_doc(&now, &l, 1, Some(hash(&at_x, 4))),
        Err(SameMeaningRefusal::NotPinned {
            hash: hash(&at_x, 4).to_string()
        })
    );
    assert_eq!(
        offered_doc(&now, &l, 4, None),
        Err(SameMeaningRefusal::DocNotValid {
            id: docid(4),
            tag: FailureTag::Invalid,
        })
    );
}

#[test]
fn a_pin_whose_name_only_an_informs_link_writes_now_is_not_compared() {
    let then = shelf_with(
        specs_testing(&["tests/a.rs"]),
        &[("tests/a.rs", Some(h("a")))],
    );
    let l = ledger(approvals(&then, &[1, 2], 1001));
    let coded = reading(
        "specs/sp-2.md",
        2,
        "SP-2",
        &[
            ("title", text("spec")),
            ("grounds", list(&["FR-1"])),
            ("tests", list(&["tests/c.rs"])),
            ("code", list(&["tests/a.rs"])),
        ],
    );
    let now = shelf_with(
        vec![fr(1, &[]), coded],
        &[("tests/a.rs", Some(h("b"))), ("tests/c.rs", Some(h("c")))],
    );
    assert!(
        causes_of(&now, &l, 2)
            .iter()
            .any(|c| matches!(c, Cause::ArtifactLinksChanged { .. }))
    );
    assert_eq!(
        offered(&now, &l, "tests/a.rs", None),
        Err(SameMeaningRefusal::NotComparedNow {
            hash: None,
            why: NotComparedWhy::LinkDropped,
        })
    );
}

// What changed: nothing there now, or not told now, is told by the pins that hold a
// hash, and a name no pin holds a hash of has nothing to declare.

#[test]
fn a_name_no_pin_holds_a_hash_of_was_absent_or_unknown_and_has_nothing_to_declare_now() {
    // SP-2 tests `tests/a.rs`, and only FR-1 is approved.
    let only_fr_1 = |files: &[(&str, Option<ArtifactHash>)]| {
        ledger(approvals(
            &shelf_with(specs_testing(&["tests/a.rs"]), files),
            &[1],
            1001,
        ))
    };
    let l = only_fr_1(&[]);
    // Nothing there: it was `ArtifactAbsent`.
    let absent = shelf_with(specs_testing(&["tests/a.rs"]), &[]);
    assert_eq!(
        offered(&absent, &l, "tests/a.rs", None),
        Err(SameMeaningRefusal::NothingToDeclare)
    );
    // Not told: it was `ArtifactUnknown`.
    let untold = shelf_answered(
        specs_testing(&["tests/a.rs"]),
        &[(
            "tests/a.rs",
            ArtifactReading::cannot_tell(NonBlank::new("offline").unwrap()),
        )],
    );
    assert_eq!(
        offered(&untold, &l, "tests/a.rs", None),
        Err(SameMeaningRefusal::NothingToDeclare)
    );
    // Pinned as nothing there, and nothing there still: it was `ArtifactAbsent`.
    let pinned_absent = ledger(approvals(&absent, &[1, 2], 1001));
    assert_eq!(
        offered(&absent, &pinned_absent, "tests/a.rs", None),
        Err(SameMeaningRefusal::NothingToDeclare)
    );
}

#[test]
fn a_hash_pinned_only_under_a_link_that_dropped_the_name_was_absent_and_is_not_compared_now() {
    let then = shelf_with(
        specs_testing(&["tests/a.rs"]),
        &[("tests/a.rs", Some(h("a")))],
    );
    let l = ledger(approvals(&then, &[1, 2], 1001));
    // SP-2 tests `tests/c.rs` now; SP-3, not approved, writes `tests/a.rs`, with nothing
    // there: it was `ArtifactAbsent`.
    let mut docs = specs_testing(&["tests/c.rs"]);
    docs.push(sp_testing(3, &["FR-1"], &["tests/a.rs"]));
    let dropped = shelf_with(docs, &[("tests/c.rs", Some(h("c")))]);
    assert_eq!(
        offered(&dropped, &l, "tests/a.rs", None),
        Err(SameMeaningRefusal::NotComparedNow {
            hash: None,
            why: NotComparedWhy::LinkDropped,
        })
    );
    // The pin of the hash gone: nothing there, as before.
    let gone = shelf_with(specs_testing(&["tests/a.rs"]), &[]);
    assert_eq!(
        offered(&gone, &l, "tests/a.rs", None),
        Err(SameMeaningRefusal::ArtifactAbsent {
            artifact: file_artifact("tests/a.rs")
        })
    );
}

#[test]
fn a_name_only_a_document_not_valid_links_is_not_linked_before_the_pins_are_read() {
    // SP-2, the only document testing `tests/a.rs`, was approved; it is not valid now
    // (no title), so no valid document links the name, and the pin it holds is not read.
    let then = shelf_with(
        specs_testing(&["tests/a.rs"]),
        &[("tests/a.rs", Some(h("a")))],
    );
    let l = ledger(approvals(&then, &[1, 2], 1001));
    let not_valid = reading(
        "specs/sp-2.md",
        2,
        "SP-2",
        &[
            ("grounds", list(&["FR-1"])),
            ("tests", list(&["tests/a.rs"])),
        ],
    );
    let now = shelf_with(vec![fr(1, &[]), not_valid], &[("tests/a.rs", Some(h("b")))]);
    assert_eq!(
        offered(&now, &l, "tests/a.rs", None),
        Err(SameMeaningRefusal::ArtifactNotLinked {
            name: name("tests/a.rs")
        })
    );
}

// `check` tells as short only what a pin stale reads now meets.

/// A hash of a document, from a word.
fn dh(word: &str) -> DocHash {
    DocHash::restore(ContentHash::of_bytes(HashKind::ShelfDoc, word.as_bytes()))
}

/// The edge each short declaration is about, in the order `check` tells them.
fn short_edges(l: &Ledger, now: &Shelf) -> Vec<SameMeaningEdge> {
    l.same_meaning_short(now)
        .into_iter()
        .map(|short| short.of)
        .collect()
}

fn doc_edge(n: u128, x: DocHash, y: DocHash) -> SameMeaningEdge {
    let (a, b) = if x <= y { (x, y) } else { (y, x) };
    SameMeaningEdge::Doc { id: docid(n), a, b }
}

fn artifact_edge(at: &str, x: ArtifactHash, y: ArtifactHash) -> SameMeaningEdge {
    let (a, b) = if x <= y { (x, y) } else { (y, x) };
    SameMeaningEdge::Artifact {
        artifact: file_artifact(at),
        a,
        b,
    }
}

/// ana's approvals, in turn and at 10, of each of `subjects` on `shelf`.
fn anas(shelf: &Shelf, subjects: &[u128]) -> Vec<Record> {
    let mut records = Vec::new();
    for (i, subject) in subjects.iter().enumerate() {
        let so_far = admitted(shelf, records.clone());
        records.push(approve_as(
            shelf,
            &so_far,
            *subject,
            1001 + i as u128,
            ("ana", 10, None),
        ));
    }
    records
}

#[test]
fn check_tells_no_edge_that_only_a_pin_stale_does_not_compare_meets() {
    let rule = || asking(2, None);
    let then = ruled_shelf_with(
        rule(),
        specs_testing(&["tests/a.rs"]),
        &[("tests/a.rs", h("one"))],
    );
    let mut records = anas(&then, &[1, 2]);
    records.push(declared_by(
        2001,
        "ana",
        edit_of("tests/a.rs", &[h("one")], h("two")),
    ));
    let l = admitted(&then, records);
    let edge = || vec![artifact_edge("tests/a.rs", h("one"), h("two"))];
    let spec = |tests: &str, valid: bool| {
        let mut extra = vec![("grounds", list(&["FR-1"])), ("tests", list(&[tests]))];
        if valid {
            extra.push(("title", text("spec")));
        }
        reading("specs/sp-2.md", 2, "SP-2", &extra)
    };
    let shelf_of = |sp_2: DocReading, files: &[(&str, ArtifactReading)]| {
        ruled_shelf_answered(rule(), vec![fr(1, &[]), sp_2], files)
    };
    let held = |text: &str| {
        ArtifactReading::of_hashes(BTreeMap::from([(Comparison::Bytes, h(text))]), None)
    };

    // Read now: edited, gone, or not told; and not told, the list is the same.
    let edited = shelf_of(spec("tests/a.rs", true), &[("tests/a.rs", held("two"))]);
    assert_eq!(short_edges(&l, &edited), edge());
    let gone = shelf_of(spec("tests/a.rs", true), &[]);
    assert_eq!(short_edges(&l, &gone), edge());
    let untold = shelf_of(
        spec("tests/a.rs", true),
        &[(
            "tests/a.rs",
            ArtifactReading::cannot_tell(NonBlank::new("offline").unwrap()),
        )],
    );
    assert_eq!(short_edges(&l, &untold), edge());

    // Not compared now: the document is not valid, the link no longer writes the name,
    // or the comparison changed.
    let invalid = shelf_of(spec("tests/a.rs", false), &[("tests/a.rs", held("two"))]);
    assert_eq!(short_edges(&l, &invalid), []);
    let dropped = shelf_of(spec("tests/c.rs", true), &[("tests/c.rs", held("two"))]);
    assert_eq!(short_edges(&l, &dropped), []);
    let remade = shelf_reading(
        &ruled_schema_of_lines(rule(), &["//"]),
        vec![fr(1, &[]), spec("tests/a.rs", true)],
        &[("tests/a.rs", b"two")],
    );
    assert_eq!(short_edges(&l, &remade), []);
}

#[test]
fn check_tells_an_edge_of_an_upstream_not_valid_and_none_of_a_document_not_valid_itself() {
    let rule = || asking(2, None);
    let at = |fr_1: DocReading| ruled_shelf(rule(), vec![fr_1, fr(2, &["FR-1"])]);
    let then = at(fr_titled(1, "a", &[]));
    let a = *then.doc(docid(1)).unwrap().hash();
    let declaration = Record::SameMeaning(
        SameMeaning::new(
            jid(2001),
            SameMeaningOf::Doc {
                id: docid(1),
                from: NonEmptySet::new(a, []),
                to: dh("b"),
            },
            Attribution::one(Actor::new("ana").unwrap()),
            JudgedAt::new(50),
            because(),
        )
        .unwrap(),
    );
    let not_valid = || reading("findings/fr-1.md", 1, "FR-1", &[]);

    // FR-1's own pin of `a`, of a document not valid now, meets the edge: told before,
    // not now.
    let alone = admitted(
        &then,
        [anas(&then, &[1]), vec![declaration.clone()]].concat(),
    );
    assert_eq!(
        short_edges(&alone, &at(fr_titled(1, "a", &[]))),
        [doc_edge(1, a, dh("b"))]
    );
    assert_eq!(short_edges(&alone, &at(not_valid())), []);

    // FR-2's pin of FR-1, an upstream not valid now, is read: told, as before.
    let both = admitted(&then, [anas(&then, &[1, 2]), vec![declaration]].concat());
    assert_eq!(
        short_edges(&both, &at(not_valid())),
        [doc_edge(1, a, dh("b"))]
    );
}

#[test]
fn check_tells_the_edges_in_the_order_of_the_approvals_and_their_pins_as_before() {
    // FR-1; SP-2 on it, testing `tests/a.rs`; FR-3 on FR-1. An edge of each, so that an
    // artifact's edge of an approval comes before a document's edge of a later one.
    let rule = || asking(2, None);
    let mut docs = specs_testing(&["tests/a.rs"]);
    docs.push(fr(3, &["FR-1"]));
    let then = ruled_shelf_with(rule(), docs, &[("tests/a.rs", h("one"))]);
    let (fr_1, fr_3) = (
        *then.doc(docid(1)).unwrap().hash(),
        *then.doc(docid(3)).unwrap().hash(),
    );
    let ana = || Attribution::one(Actor::new("ana").unwrap());
    let doc = |judgement: u128, n: u128, from: DocHash| {
        Record::SameMeaning(
            SameMeaning::new(
                jid(judgement),
                SameMeaningOf::Doc {
                    id: docid(n),
                    from: NonEmptySet::new(from, []),
                    to: dh("later"),
                },
                ana(),
                JudgedAt::new(50),
                because(),
            )
            .unwrap(),
        )
    };
    // Declared in another order than the pins'.
    let mut records = anas(&then, &[1, 2, 3]);
    records.push(doc(2001, 3, fr_3));
    records.push(declared_by(
        2002,
        "ana",
        edit_of("tests/a.rs", &[h("one")], h("two")),
    ));
    records.push(doc(2003, 1, fr_1));
    let l = admitted(&then, records);
    // FR-1's own pin; then SP-2's upstream (FR-1's edge, told once) and its artifact;
    // then FR-3's own pin.
    assert_eq!(
        short_edges(&l, &then),
        [
            doc_edge(1, fr_1, dh("later")),
            artifact_edge("tests/a.rs", h("one"), h("two")),
            doc_edge(3, fr_3, dh("later")),
        ]
    );
}

#[test]
fn what_is_offered_is_what_stale_tells_as_changed_pin_by_pin() {
    // SP-2 and SP-3 test `tests/a.rs`, approved while it held "a"; the edit a -> b was
    // declared.
    let at = |held: &str| {
        shelf_with(
            specs_testing(&["tests/a.rs", "tests/a.rs"]),
            &[("tests/a.rs", Some(h(held)))],
        )
    };
    let mut records = approvals(&at("a"), &[1, 2, 3], 1001);
    records.push(declared(2001, "tests/a.rs", &[h("a")], h("b")));
    let l = ledger(records);
    // Kept by the declaration, both pins hold: stale tells nothing, nothing is offered.
    let now = at("b");
    for n in [2, 3] {
        assert!(causes_of(&now, &l, n).is_empty(), "{n}");
    }
    assert_eq!(
        offered(&now, &l, "tests/a.rs", None),
        Err(SameMeaningRefusal::NothingToDeclare)
    );
    // Edited again, both are changes, and the edit of each is offered.
    let now = at("c");
    let changed: BTreeSet<u128> = [2, 3]
        .into_iter()
        .filter(|n| !causes_of(&now, &l, *n).is_empty())
        .collect();
    assert_eq!(changed, BTreeSet::from([2, 3]));
    assert_eq!(
        offered(&now, &l, "tests/a.rs", None),
        Ok(edit_of("tests/a.rs", &[h("a")], h("c")))
    );

    // One shelf with a pin changed and another kept: SP-2 pinned "a", and the edit
    // a -> c is declared; SP-3 pinned "b", which nothing joins to "c". What is offered
    // is the hash of the pins whose cause is `ArtifactChanged`, and no other.
    let mut records = approvals(&at("a"), &[1, 2], 1001);
    records.push(approve(
        &at("b"),
        &ledger(records.clone()),
        3,
        1003,
        20,
        None,
    ));
    records.push(declared(2001, "tests/a.rs", &[h("a")], h("c")));
    let l = ledger(records);
    let now = at("c");
    assert!(causes_of(&now, &l, 2).is_empty());
    let mut pinned_and_changed = BTreeSet::new();
    for n in [2, 3] {
        for cause in causes_of(&now, &l, n) {
            let Cause::ArtifactChanged { pin, .. } = cause else {
                panic!("{n} is changed as an artifact, got {cause:?}");
            };
            let PinnedArtifact::Present { hash, .. } = pin.pin().seen() else {
                panic!("{n} pinned a hash");
            };
            pinned_and_changed.insert(*hash);
        }
    }
    assert_eq!(pinned_and_changed, BTreeSet::from([h("b")]));
    assert_eq!(
        offered(&now, &l, "tests/a.rs", None),
        Ok(edit_of(
            "tests/a.rs",
            &pinned_and_changed.into_iter().collect::<Vec<_>>(),
            h("c")
        ))
    );
}
