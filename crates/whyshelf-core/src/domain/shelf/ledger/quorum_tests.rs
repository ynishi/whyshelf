//! Tests of how many names an approval and a declaration of the same meaning count
//! ([`Ledger::quorum`], [`Ledger::doc_same_meaning`], [`Ledger::same_meaning_short`]):
//! under a rule that asks for two names, one approval is short and another name's
//! approval of the same content meets it; a name counts once, and only while its own
//! latest reading has not lapsed; an approval no rule reads needs one name; an edit
//! is joined once two names declare it, counted per edge over every name freshness
//! reads for a pin, with the least need of its declarations; and an edge short of
//! names is told only where a pin of an approval that stands meets it.

use std::slice::from_ref;

use crate::domain::actor::Actor;
use crate::domain::shelf::artifact::ArtifactHash;
use crate::domain::shelf::fixtures_tests::{
    admitted, approve_as, asking, docid, file_artifact, file_space, fr, fr_titled, jid, name,
    ruled_shelf, ruled_shelf_with, shelf, sp_testing,
};
use crate::domain::shelf::schema::{Approvals, DecideDecl};

use super::*;

/// When the quorum is asked, after every record of the tests.
fn now() -> JudgedAt {
    JudgedAt::new(100)
}

/// An approval of FR-1 on `shelf` (who, when, until when), made after `before`.
fn approval(
    shelf: &Shelf,
    before: &[Record],
    judgement: u128,
    read: (&str, i64, Option<i64>),
) -> Record {
    approve_as(shelf, &admitted(shelf, before.to_vec()), 1, judgement, read)
}

/// A quorum short of `need`, with these names counting.
fn short(readers: &[&str], need: i64) -> Quorum {
    let mut names = readers.iter().map(|name| Actor::new(name).unwrap());
    Quorum::Short {
        readers: NonEmptySet::new(names.next().unwrap(), names),
        need: Approvals::new(need).unwrap(),
    }
}

/// A declaration by `by`, at 30, that the hash `from` of FR-1 means what `to` means.
fn same(judgement: u128, by: &str, from: DocHash, to: DocHash) -> Record {
    same_at(judgement, by, (from, to), 30)
}

/// As [`same`], made at `at`.
fn same_at(judgement: u128, by: &str, (from, to): (DocHash, DocHash), at: i64) -> Record {
    Record::SameMeaning(
        SameMeaning::new(
            jid(judgement),
            SameMeaningOf::Doc {
                id: docid(1),
                from: NonEmptySet::new(from, []),
                to,
            },
            Actor::new(by).unwrap(),
            JudgedAt::new(at),
            NonBlank::new("the same").unwrap(),
        )
        .unwrap(),
    )
}

/// The hash FR-1 has with this title, on a shelf that asks for two names.
fn hash_titled(title: &str) -> DocHash {
    let shelf = ruled_shelf(asking(2, None), vec![fr_titled(1, title, &[])]);
    *shelf.doc(docid(1)).unwrap().hash()
}

#[test]
fn an_approval_is_short_until_another_name_approves_the_same_content() {
    let shelf = ruled_shelf(asking(2, None), vec![fr(1, &[])]);
    let nothing = admitted(&shelf, Vec::new());
    assert_eq!(nothing.quorum(docid(1), now()), Quorum::Met);

    let first = approval(&shelf, &[], 1001, ("ana", 10, None));
    let ledger = admitted(&shelf, vec![first.clone()]);
    assert_eq!(ledger.quorum(docid(1), now()), short(&["ana"], 2));

    // The same name again, with another expiry, renews its reading: one name still.
    let again = approval(&shelf, from_ref(&first), 1002, ("ana", 20, Some(500)));
    let records = vec![first, again];
    let ledger = admitted(&shelf, records.clone());
    assert_eq!(ledger.quorum(docid(1), now()), short(&["ana"], 2));

    // Another name's reading of the same content meets it.
    let second = approval(&shelf, &records, 1003, ("bo", 30, Some(500)));
    let ledger = admitted(&shelf, [records, vec![second]].concat());
    assert_eq!(ledger.quorum(docid(1), now()), Quorum::Met);
}

#[test]
fn a_reader_whose_own_latest_reading_lapsed_counts_no_more_whoever_approved_last() {
    let shelf = ruled_shelf(asking(2, None), vec![fr(1, &[])]);
    // ana until 50, then bo with none, whose approval stands.
    let ana = approval(&shelf, &[], 1001, ("ana", 10, Some(50)));
    let bo = approval(&shelf, from_ref(&ana), 1002, ("bo", 20, None));
    let ledger = admitted(&shelf, vec![ana, bo]);
    assert_eq!(ledger.quorum(docid(1), JudgedAt::new(40)), Quorum::Met);
    let lapsed = ledger.quorum(docid(1), JudgedAt::new(50));
    assert_eq!(lapsed, short(&["bo"], 2));

    // The other way round: ana with none, then bo until 50, whose approval stands
    // and lapses (freshness tells it): ana alone reads it, short as well.
    let ana = approval(&shelf, &[], 1001, ("ana", 10, None));
    let bo = approval(&shelf, from_ref(&ana), 1002, ("bo", 20, Some(50)));
    let ledger = admitted(&shelf, vec![ana, bo]);
    assert_eq!(ledger.quorum(docid(1), JudgedAt::new(40)), Quorum::Met);
    let lapsed = ledger.quorum(docid(1), JudgedAt::new(60));
    assert_eq!(lapsed, short(&["ana"], 2));

    // Both lapsed: the reader of the approval that stands is told alone.
    let ana = approval(&shelf, &[], 1001, ("ana", 10, Some(50)));
    let bo = approval(&shelf, from_ref(&ana), 1002, ("bo", 20, Some(50)));
    let ledger = admitted(&shelf, vec![ana, bo]);
    let lapsed = ledger.quorum(docid(1), JudgedAt::new(60));
    assert_eq!(lapsed, short(&["bo"], 2));
}

#[test]
fn an_approval_no_rule_reads_needs_one_name_lapsed_or_not() {
    // A project that declares no rule.
    let plain = shelf(vec![fr(1, &[])]);
    let lapsing = approval(&plain, &[], 1001, ("ana", 10, Some(50)));
    let ledger = admitted(&plain, vec![lapsing]);
    assert_eq!(ledger.quorum(docid(1), now()), Quorum::Met);

    // A record made before the rule's `since` is unruled; one made from it is not.
    let ruled = ruled_shelf(asking(2, Some(100)), vec![fr(1, &[])]);
    let later = JudgedAt::new(200);
    let before = admitted(&ruled, vec![approval(&ruled, &[], 1001, ("ana", 99, None))]);
    assert_eq!(before.quorum(docid(1), later), Quorum::Met);
    let from = approval(&ruled, &[], 1001, ("ana", 100, None));
    let from = admitted(&ruled, vec![from]);
    assert_eq!(from.quorum(docid(1), later), short(&["ana"], 2));
}

/// ana's approval of FR-1 with this title, under `decide`, at 10: the pin of its hash.
fn approved_titled(decide: DecideDecl, title: &str) -> Record {
    let shelf = ruled_shelf(decide, vec![fr_titled(1, title, &[])]);
    approval(&shelf, &[], 1001, ("ana", 10, None))
}

/// The short edge of FR-1 between `x` and `y`, by these declarations and names.
fn short_edge(x: DocHash, y: DocHash, declared: &[u128], names: &[&str]) -> ShortSameMeaning {
    let (a, b) = if x <= y { (x, y) } else { (y, x) };
    let mut ids = declared.iter().map(|n| jid(*n));
    let mut names = names.iter().map(|name| Actor::new(name).unwrap());
    ShortSameMeaning {
        of: SameMeaningEdge::Doc { id: docid(1), a, b },
        declared: NonEmptyVec::new(ids.next().unwrap(), ids.collect()),
        names: NonEmptySet::new(names.next().unwrap(), names),
        need: Approvals::new(2).unwrap(),
    }
}

#[test]
fn an_edit_is_joined_once_as_many_names_declare_it_as_its_rule_asks_for() {
    let now = ruled_shelf(asking(2, None), vec![fr_titled(1, "edited", &[])]);
    let (a, b) = (hash_titled("title"), hash_titled("edited"));
    let pinned = approved_titled(asking(2, None), "title");
    let ana = same(2001, "ana", a, b);
    let ledger = admitted(&now, vec![pinned.clone(), ana.clone()]);
    assert_eq!(ledger.doc_same_meaning(docid(1), &a, &b), None);
    // The approval that stands pins `a`, and meets the edge.
    assert_eq!(
        ledger.same_meaning_short(),
        [short_edge(a, b, &[2001], &["ana"])]
    );

    // A second declaration by ana is still one name; bo's is the second.
    let more = [same(2002, "ana", a, b), same(2003, "bo", a, b)];
    let ledger = admitted(&now, [vec![pinned, ana], more.to_vec()].concat());
    assert_eq!(
        ledger.doc_same_meaning(docid(1), &a, &b),
        Some(NonEmptyVec::new(jid(2001), vec![jid(2003)]))
    );
    assert!(ledger.same_meaning_short().is_empty());

    // With no rule, one name joins it, as before, and nothing is counted.
    let plain = shelf(vec![fr_titled(1, "edited", &[])]);
    let ledger = admitted(&plain, vec![same(2001, "ana", a, b)]);
    assert_eq!(
        ledger.doc_same_meaning(docid(1), &a, &b),
        Some(NonEmptyVec::new(jid(2001), Vec::new()))
    );
    assert!(ledger.same_meaning_short().is_empty());
}

#[test]
fn the_names_are_counted_per_edge_not_per_way() {
    let (a, b, c) = (hash_titled("a"), hash_titled("b"), hash_titled("c"));
    let now = ruled_shelf(asking(2, None), vec![fr_titled(1, "c", &[])]);
    let pinned = approved_titled(asking(2, None), "a");
    // Two names along the way, one on each edge: no edge counts, and the pin of `a`
    // meets the first only, as the second is reached through no edge that counts.
    let mut records = vec![pinned, same(2001, "ana", a, b), same(2002, "bo", b, c)];
    let ledger = admitted(&now, records.clone());
    assert_eq!(ledger.doc_same_meaning(docid(1), &a, &c), None);
    assert_eq!(
        ledger.same_meaning_short(),
        [short_edge(a, b, &[2001], &["ana"])]
    );

    // bo declares the first edge too: it counts, and the pin meets the second.
    records.push(same(2003, "bo", a, b));
    let ledger = admitted(&now, records.clone());
    assert!(ledger.doc_same_meaning(docid(1), &a, &b).is_some());
    assert_eq!(ledger.doc_same_meaning(docid(1), &a, &c), None);
    assert_eq!(
        ledger.same_meaning_short(),
        [short_edge(b, c, &[2002], &["bo"])]
    );

    // ana the second: the way counts, each edge with its two names.
    records.push(same(2004, "ana", b, c));
    let ledger = admitted(&now, records);
    let way = NonEmptyVec::new(jid(2001), vec![jid(2003), jid(2002), jid(2004)]);
    assert_eq!(ledger.doc_same_meaning(docid(1), &a, &c), Some(way));
    assert!(ledger.same_meaning_short().is_empty());
}

#[test]
fn an_edge_needs_the_least_its_declarations_need_so_a_declaration_added_takes_nothing_back() {
    // The rule reads the records from 100: ana's declaration at 30 needs one name.
    let rule = || asking(2, Some(100));
    let now = ruled_shelf(rule(), vec![fr_titled(1, "edited", &[])]);
    let (a, b) = (hash_titled("title"), hash_titled("edited"));
    let pinned = approved_titled(rule(), "title");
    let before = same(2001, "ana", a, b);
    let ledger = admitted(&now, vec![pinned.clone(), before.clone()]);
    let joined = Some(NonEmptyVec::new(jid(2001), Vec::new()));
    assert_eq!(ledger.doc_same_meaning(docid(1), &a, &b), joined);

    // ana declares it again under the rule, which asks for two: it stays joined.
    let again = same_at(2002, "ana", (a, b), 200);
    let ledger = admitted(&now, vec![pinned, before, again]);
    assert_eq!(ledger.doc_same_meaning(docid(1), &a, &b), joined);
    assert!(ledger.same_meaning_short().is_empty());
}

#[test]
fn a_name_that_declared_an_edge_adds_nothing_by_declaring_it_again() {
    let now = ruled_shelf(asking(2, None), vec![fr_titled(1, "edited", &[])]);
    let (a, b) = (hash_titled("title"), hash_titled("edited"));
    let c = hash_titled("c");
    let ledger = admitted(&now, vec![same(2001, "ana", a, b)]);
    let edit = |from: &[DocHash]| SameMeaningOf::Doc {
        id: docid(1),
        from: NonEmptySet::from_set(from.iter().copied().collect()).unwrap(),
        to: b,
    };
    let ana = Actor::new("ana").unwrap();
    // The edge does not count yet, and ana declared it: it awaits another name.
    assert_eq!(
        ledger.not_declared_by(edit(&[a]), &ana),
        Err(SameMeaningRefusal::AlreadyDeclaredBy { actor: ana.clone() })
    );
    // Beside an edge ana has not declared, only that one is left.
    assert_eq!(ledger.not_declared_by(edit(&[a, c]), &ana), Ok(edit(&[c])));
    // Another name's declaration is its own.
    let bo = Actor::new("bo").unwrap();
    assert_eq!(ledger.not_declared_by(edit(&[a]), &bo), Ok(edit(&[a])));
}

#[test]
fn an_edge_no_approval_that_stands_meets_is_not_told() {
    let (a, b) = (hash_titled("a"), hash_titled("b"));
    let at_c = ruled_shelf(asking(2, None), vec![fr_titled(1, "c", &[])]);
    let pinned = approved_titled(asking(2, None), "a");
    // FR-1 is approved again on other content: no approval stands on `a` now.
    let again = approval(&at_c, from_ref(&pinned), 1002, ("ana", 20, None));
    let records = vec![pinned, again, same(2001, "ana", a, b)];
    let ledger = admitted(&at_c, records);
    assert_eq!(ledger.doc_same_meaning(docid(1), &a, &b), None);
    assert!(ledger.same_meaning_short().is_empty());
}

/// The hashes of `tests/a.rs` an approval pinned, and the one after the edit.
fn one_two() -> (ArtifactHash, ArtifactHash) {
    (
        ArtifactHash::of_content(b"one"),
        ArtifactHash::of_content(b"two"),
    )
}

/// FR-1, and SP-2 tested by `tests/a.rs`, both approved by ana at 10 with the file
/// at `one`; then the file declared moved to `tests/b.rs`, at 20.
fn moved_spec() -> (Shelf, Vec<Record>) {
    let then = ruled_shelf_with(
        asking(2, None),
        vec![fr(1, &[]), sp_testing(2, &["FR-1"], &["tests/a.rs"])],
        &[("tests/a.rs", one_two().0)],
    );
    let nothing = admitted(&then, Vec::new());
    let finding = approve_as(&then, &nothing, 1, 1001, ("ana", 10, None));
    let so_far = admitted(&then, vec![finding.clone()]);
    let spec = approve_as(&then, &so_far, 2, 1002, ("ana", 10, None));
    let moved = Record::Moved(Moved::new(
        jid(3001),
        MovedOf::new(file_space(), name("tests/a.rs"), name("tests/b.rs")).unwrap(),
        Actor::new("ana").unwrap(),
        JudgedAt::new(20),
        NonBlank::new("renamed").unwrap(),
    ));
    (then, vec![finding, spec, moved])
}

/// The edit of the artifact at the name `at` from `one` to `two`.
fn artifact_edit(at: &str) -> SameMeaningOf {
    let (one, two) = one_two();
    SameMeaningOf::Artifact {
        artifact: file_artifact(at),
        from: NonEmptySet::new(one, []),
        to: two,
    }
}

/// A declaration by `by`, at 30, of [`artifact_edit`] at the name `at`.
fn artifact_same(judgement: u128, by: &str, at: &str) -> Record {
    Record::SameMeaning(
        SameMeaning::new(
            jid(judgement),
            artifact_edit(at),
            Actor::new(by).unwrap(),
            JudgedAt::new(30),
            NonBlank::new("the same").unwrap(),
        )
        .unwrap(),
    )
}

#[test]
fn a_name_that_declared_an_artifact_edit_under_its_old_name_adds_nothing_under_the_new() {
    let (then, mut records) = moved_spec();
    records.push(artifact_same(2001, "ana", "tests/a.rs"));
    let ledger = admitted(&then, records);
    // ana declares the same edit under the name the file moved to: the names of the
    // way are one thing, as counting reads them.
    let ana = Actor::new("ana").unwrap();
    assert_eq!(
        ledger.not_declared_by(artifact_edit("tests/b.rs"), &ana),
        Err(SameMeaningRefusal::AlreadyDeclaredBy { actor: ana })
    );
    let bo = Actor::new("bo").unwrap();
    let edit = artifact_edit("tests/b.rs");
    assert_eq!(ledger.not_declared_by(edit.clone(), &bo), Ok(edit));
}

#[test]
fn an_artifact_edge_is_counted_over_every_name_of_the_pins_way_as_freshness_reads_it() {
    let (one, two) = one_two();
    let (then, mut records) = moved_spec();
    // ana declares the edit under the old name, bo under the new one.
    let of = artifact_same;
    records.push(of(2001, "ana", "tests/a.rs"));

    // ana's alone is short, and the pin meets it.
    let ledger = admitted(&then, records.clone());
    let [found] = ledger.same_meaning_short().try_into().unwrap();
    assert_eq!(found.declared, NonEmptyVec::new(jid(2001), Vec::new()));

    // With bo's under the new name, the two are one edge of two names, as freshness
    // reads the pin: it is joined, and not told.
    let both = [records, vec![of(2002, "bo", "tests/b.rs")]].concat();
    let ledger = admitted(&then, both);
    let pin = ledger.read_pins(docid(2)).remove(0);
    assert_eq!(
        ledger.pin_same_meaning(&pin, &one, &two),
        Some(NonEmptyVec::new(jid(2001), vec![jid(2002)]))
    );
    assert!(ledger.same_meaning_short().is_empty());
}
