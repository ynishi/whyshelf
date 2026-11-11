//! Tests of the measures of how the work went, on a small ledger.
//!
//! ```text
//!   at    actor   record
//!   10    tester  approve FR-1, approve SP-2 (grounds FR-1, tests t.rs "one")
//!   20    tester  FR-1 edited, t.rs "two": approve FR-1 again, SP-2 again
//!   30    tester  t.rs "three": approve SP-2 again (revoked at 40)
//!   5000  other   approve FR-3 (its identity made at 1000)
//!   5000  tester  approve FR-4
//! ```

use std::collections::BTreeSet;

use uuid::Uuid;

use crate::domain::actor::Actor;
use crate::domain::id::{DocId, EntityId};
use crate::domain::nonempty::{NonBlank, NonEmptySet};
use crate::domain::shelf::build::{DocReading, Shelf};
use crate::domain::shelf::fixtures_tests::{
    actor, docid, entries, file_artifact, fr_titled, jid, ledger, path, schema, schema_of_lines,
    shelf_reading, sp_testing, text,
};
use crate::domain::shelf::ledger::{Decision, JudgedAt, Ledger, Record, Revocation, frame_approve};
use crate::domain::shelf::schema::Schema;

use super::*;

/// A document identity made at `millis`, ending in `n`.
fn made_at(millis: u128, n: u128) -> DocId {
    let bits = (millis << 80) | (0x7u128 << 76) | (0b10u128 << 62) | n;
    DocId::new(EntityId::new(Uuid::from_u128(bits)).unwrap())
}

/// A finding with an identity of its own.
fn finding(id: DocId, n: u128) -> DocReading {
    DocReading::Parsed {
        origin: path(&format!("findings/fr-{n}.md")),
        entries: entries(&[
            ("id", text(&id.to_string())),
            ("key", text(&format!("FR-{n}"))),
            ("title", text("title")),
        ]),
        body: "body\n".to_owned(),
    }
}

/// The shelf at one moment: FR-1 with this title, SP-2 on FR-1 and `t.rs` with these
/// bytes, FR-3 (made at 1000) and FR-4.
fn shelf_at(title: &str, tests: &[u8]) -> Shelf {
    shelf_reading(
        &schema(),
        vec![
            fr_titled(1, title, &[]),
            sp_testing(2, &["FR-1"], &["t.rs"]),
            finding(made_at(1000, 3), 3),
            fr_titled(4, "title", &[]),
        ],
        &[("t.rs", tests)],
    )
}

fn approval(shelf: &Shelf, subject: DocId, judgement: u128, at: i64, by: &Actor) -> Record {
    let frame = frame_approve(shelf, &ledger(vec![]), subject).unwrap();
    Record::Decision(
        Decision::approve(
            jid(judgement),
            frame,
            None,
            by.clone(),
            JudgedAt::new(at),
            None,
        )
        .unwrap(),
    )
}

fn revocation(judgement: u128, target: u128, at: i64) -> Record {
    Record::Revocation(Revocation::new(
        jid(judgement),
        NonEmptySet::from_set(BTreeSet::from([jid(target)])).unwrap(),
        actor(),
        JudgedAt::new(at),
        NonBlank::new("taken back").unwrap(),
    ))
}

/// The records of the module page, and the shelf as it is at the end.
fn story() -> (Vec<Record>, Shelf) {
    let tester = actor();
    let other = Actor::new("other").unwrap();
    let first = shelf_at("a", b"one");
    let second = shelf_at("b", b"two");
    let third = shelf_at("b", b"three");
    let records = vec![
        approval(&first, docid(1), 101, 10, &tester),
        approval(&first, docid(2), 102, 10, &tester),
        approval(&second, docid(1), 103, 20, &tester),
        approval(&second, docid(2), 104, 20, &tester),
        approval(&third, docid(2), 105, 30, &tester),
        revocation(106, 105, 40),
        approval(&third, made_at(1000, 3), 107, 5000, &other),
        approval(&third, docid(4), 108, 5000, &tester),
    ];
    (records, third)
}

fn key(text: &str) -> Option<String> {
    Some(text.to_owned())
}

fn keys(stats: &Stats) -> Vec<Option<String>> {
    stats
        .approvals
        .docs
        .iter()
        .map(|d| d.key.as_ref().map(ToString::to_string))
        .collect()
}

fn book(name: &str) -> Option<BookName> {
    Some(BookName::new(name).unwrap())
}

#[test]
fn approvals_are_counted_with_those_made_again_and_a_revoked_one_apart() {
    let (records, shelf) = story();
    let stats = of(&Ledger::build(records), &shelf, None);
    let counts: Vec<(usize, usize, usize)> = stats
        .approvals
        .docs
        .iter()
        .map(|d| (d.approvals, d.again, d.revoked))
        .collect();
    // The revoked approval of SP-2 is still one of its approvals, and one again.
    assert_eq!(
        keys(&stats),
        [
            key("spec:SP-2"),
            key("finding:FR-1"),
            key("finding:FR-3"),
            key("finding:FR-4")
        ]
    );
    assert_eq!(counts, [(3, 2, 1), (2, 1, 0), (1, 0, 0), (1, 0, 0)]);

    let books: Vec<(Option<BookName>, usize, usize, usize, usize)> = stats
        .approvals
        .books
        .iter()
        .map(|b| (b.book.clone(), b.docs, b.approvals, b.again, b.revoked))
        .collect();
    // The books in the order of the declaration, one with no approval included.
    assert_eq!(
        books,
        [
            (book("finding"), 3, 4, 1, 0),
            (book("spec"), 1, 3, 2, 1),
            (book("design"), 0, 0, 0, 0),
        ]
    );
}

#[test]
fn the_pending_time_runs_from_the_identity_to_the_first_approval() {
    let (records, shelf) = story();
    let stats = of(&Ledger::build(records), &shelf, None);
    let pending: Vec<(Option<BookName>, Spread)> = stats
        .pending
        .iter()
        .map(|p| (p.book.clone(), p.millis))
        .collect();
    // FR-1 and FR-4 were made at 0 and FR-3 at 1000: 10, 4000 and 5000 ms.
    assert_eq!(
        pending,
        [
            (
                book("finding"),
                Spread {
                    count: 3,
                    median: Some(4000),
                    p90: Some(5000),
                    max: Some(5000)
                }
            ),
            (
                book("spec"),
                Spread {
                    count: 1,
                    median: Some(10),
                    p90: Some(10),
                    max: Some(10)
                }
            ),
            (
                book("design"),
                Spread {
                    count: 0,
                    median: None,
                    p90: None,
                    max: None
                }
            ),
        ]
    );
}

#[test]
fn a_spread_is_nearest_rank() {
    let spread = Spread::of(vec![4, 1, 3, 2]);
    // ceil(0.5 * 4) = 2 and ceil(0.9 * 4) = 4.
    assert_eq!(
        spread,
        Spread {
            count: 4,
            median: Some(2),
            p90: Some(4),
            max: Some(4)
        }
    );
    let ten = Spread::of((1..=10).collect());
    assert_eq!((ten.median, ten.p90, ten.max), (Some(5), Some(9), Some(10)));
}

#[test]
fn runs_are_the_decisions_of_one_time_and_one_actor() {
    let (records, shelf) = story();
    let stats = of(&Ledger::build(records), &shelf, None);
    // (10, tester) 2, (20, tester) 2, (30, tester) 1, (5000, other) 1, (5000, tester) 1;
    // the revocation is no decision.
    assert_eq!(
        stats.runs.sizes,
        Spread {
            count: 5,
            median: Some(1),
            p90: Some(2),
            max: Some(2)
        }
    );
    let largest: Vec<(i64, String, usize)> = stats
        .runs
        .largest
        .iter()
        .map(|r| (r.at.millis(), r.actor.to_string(), r.size))
        .collect();
    assert_eq!(
        largest,
        [
            (10, "tester".to_owned(), 2),
            (20, "tester".to_owned(), 2),
            (30, "tester".to_owned(), 1),
            (5000, "other".to_owned(), 1),
            (5000, "tester".to_owned(), 1),
        ]
    );
}

#[test]
fn origins_are_the_pins_whose_hash_changed_under_an_approval_made_again() {
    let (records, shelf) = story();
    let stats = of(&Ledger::build(records.clone()), &shelf, None);
    // t.rs changed under both approvals of SP-2 made again, FR-1 under the first;
    // FR-1's own approval made again stood on nothing.
    assert_eq!(
        stats.origins,
        [
            OriginCount {
                origin: Origin::Artifact(file_artifact("t.rs")),
                count: 2
            },
            OriginCount {
                origin: Origin::Doc {
                    doc: docid(1),
                    key: shelf.doc(docid(1)).map(|d| d.key().clone()),
                },
                count: 1
            },
        ]
    );

    // Counted alike, a document comes before an artifact.
    let stats = of(&Ledger::build(records[..4].to_vec()), &shelf, None);
    let order: Vec<bool> = stats
        .origins
        .iter()
        .map(|o| matches!(o.origin, Origin::Doc { .. }))
        .collect();
    assert_eq!(order, [true, false]);
    assert!(stats.origins.iter().all(|o| o.count == 1));
}

#[test]
fn since_keeps_the_records_at_or_after_it() {
    let (records, shelf) = story();
    let stats = of(&Ledger::build(records), &shelf, Some(JudgedAt::new(20)));
    let counts: Vec<(Option<String>, usize, usize)> = stats
        .approvals
        .docs
        .iter()
        .map(|d| {
            (
                d.key.as_ref().map(ToString::to_string),
                d.approvals,
                d.again,
            )
        })
        .collect();
    // An approval after `since` of a document first approved before it is again.
    assert_eq!(
        counts,
        [
            (key("spec:SP-2"), 2, 2),
            (key("finding:FR-1"), 1, 1),
            (key("finding:FR-3"), 1, 0),
            (key("finding:FR-4"), 1, 0),
        ]
    );
    // Only the documents first approved at or after 20 have a pending time.
    assert_eq!(stats.pending[0].millis.count, 2);
    assert_eq!(stats.pending[1].millis.count, 0);
    assert_eq!(stats.runs.sizes.count, 4);
    assert_eq!(stats.origins[0].count, 2);

    let late = of(&Ledger::build(story().0), &shelf, Some(JudgedAt::new(6000)));
    assert!(late.approvals.docs.is_empty());
    assert!(late.origins.is_empty());
    assert_eq!(late.runs.sizes.count, 0);
}

#[test]
fn a_document_that_is_no_valid_document_now_is_of_no_book() {
    let (records, _) = story();
    // FR-4's file is gone.
    let shelf = shelf_reading(
        &schema(),
        vec![
            fr_titled(1, "b", &[]),
            sp_testing(2, &["FR-1"], &["t.rs"]),
            finding(made_at(1000, 3), 3),
        ],
        &[("t.rs", b"three")],
    );
    let stats = of(&Ledger::build(records), &shelf, None);
    let last = stats.approvals.docs.last().unwrap();
    assert_eq!((last.doc, &last.key), (docid(4), &None));
    let none = stats.approvals.books.last().unwrap();
    assert_eq!((&none.book, none.docs), (&None, 1));
    assert_eq!(stats.pending.last().unwrap().book, None);
}

#[test]
fn the_same_ledger_gives_the_same_answer_in_any_order() {
    let (mut records, shelf) = story();
    let once = of(&Ledger::build(records.clone()), &shelf, None);
    records.reverse();
    assert_eq!(of(&Ledger::build(records), &shelf, None), once);
}

/// A shelf of `schema` with SP-2 grounded on `grounds` and tested by `tests`, the
/// findings FR-1 and FR-4, and these files.
fn spec_shelf(schema: &Schema, grounds: &[&str], tests: &[&str], files: &[(&str, &[u8])]) -> Shelf {
    shelf_reading(
        schema,
        vec![
            fr_titled(1, "title", &[]),
            fr_titled(4, "title", &[]),
            sp_testing(2, grounds, tests),
        ],
        files,
    )
}

fn origin_names(stats: &Stats) -> Vec<(String, usize)> {
    stats
        .origins
        .iter()
        .map(|o| {
            let name = match &o.origin {
                Origin::Doc { doc, .. } => doc.to_string(),
                Origin::Artifact(artifact) => artifact.name().to_string(),
            };
            (name, o.count)
        })
        .collect()
}

#[test]
fn a_pin_whose_comparison_changed_is_no_origin() {
    let tester = actor();
    let bytes = spec_shelf(&schema(), &[], &["t.rs"], &[("t.rs", b"one")]);
    let lines = schema_of_lines(&[]);
    let as_lines = spec_shelf(&lines, &[], &["t.rs"], &[("t.rs", b"two")]);
    let edited = spec_shelf(&lines, &[], &["t.rs"], &[("t.rs", b"three")]);
    let records = vec![
        approval(&bytes, docid(2), 101, 10, &tester),
        approval(&as_lines, docid(2), 102, 20, &tester),
        approval(&edited, docid(2), 103, 30, &tester),
    ];
    let stats = of(&Ledger::build(records), &edited, None);
    // 20 against 10: bytes and lines, not compared; 30 against 20: lines both.
    assert_eq!(stats.approvals.docs[0].again, 2);
    assert_eq!(origin_names(&stats), [("t.rs".to_owned(), 1)]);
}

#[test]
fn a_pin_only_one_frame_holds_is_no_origin() {
    let tester = actor();
    let before = spec_shelf(&schema(), &["FR-1"], &["t.rs"], &[("t.rs", b"one")]);
    // FR-1 and t.rs taken out, FR-4 and u.rs put in.
    let after = spec_shelf(
        &schema(),
        &["FR-4"],
        &["u.rs"],
        &[("t.rs", b"two"), ("u.rs", b"new")],
    );
    let records = vec![
        approval(&before, docid(2), 101, 10, &tester),
        approval(&after, docid(2), 102, 20, &tester),
    ];
    let stats = of(&Ledger::build(records), &after, None);
    assert_eq!(stats.approvals.docs[0].again, 1);
    assert!(stats.origins.is_empty(), "{:?}", stats.origins);
}

#[test]
fn a_revoked_approval_is_not_one_made_before() {
    let tester = actor();
    let one = spec_shelf(&schema(), &[], &["t.rs"], &[("t.rs", b"one")]);
    let two = spec_shelf(&schema(), &[], &["t.rs"], &[("t.rs", b"two")]);
    let records = vec![
        // FR-1: approved at 10, revoked, approved at 20.
        approval(&one, docid(1), 101, 10, &tester),
        revocation(102, 101, 15),
        approval(&one, docid(1), 103, 20, &tester),
        // SP-2: t.rs "one" at 10, "two" at 20 (revoked), "one" again at 30.
        approval(&one, docid(2), 104, 10, &tester),
        approval(&two, docid(2), 105, 20, &tester),
        revocation(106, 105, 25),
        approval(&one, docid(2), 107, 30, &tester),
    ];
    let stats = of(&Ledger::build(records), &one, None);
    let counts: Vec<(Option<String>, usize, usize, usize)> = stats
        .approvals
        .docs
        .iter()
        .map(|d| {
            (
                d.key.as_ref().map(ToString::to_string),
                d.approvals,
                d.again,
                d.revoked,
            )
        })
        .collect();
    // FR-1's approval at 20 follows only a revoked one: it is no approval again.
    assert_eq!(
        counts,
        [(key("spec:SP-2"), 3, 2, 1), (key("finding:FR-1"), 2, 0, 1)]
    );
    // The approval at 30 is set beside the one at 10, not the revoked one at 20: only
    // the revoked one saw t.rs changed.
    assert_eq!(origin_names(&stats), [("t.rs".to_owned(), 1)]);
    // FR-1 was first approved, not revoked, at 20, and made at 0.
    assert_eq!(stats.pending[0].millis.max, Some(20));
}
