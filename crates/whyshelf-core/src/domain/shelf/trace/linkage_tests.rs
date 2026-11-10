//! Tests of [`linkage`]: which links count each way, which holders reach, what the
//! declaration makes inevitable, and that the answers are disjoint.

use std::collections::BTreeMap;

use crate::domain::id::DocId;
use crate::domain::shelf::build::{DocReading, Shelf};
use crate::domain::shelf::fixtures_tests::{
    decls, docid, fr, ledger, list, reading, retire, shelf, shelf_reading, sp_testing, spaces, text,
};
use crate::domain::shelf::freshness::assess;
use crate::domain::shelf::ledger::{JudgedAt, Record};
use crate::domain::shelf::schema::{LinkTargetDecl, Propagation, Schema};

use super::{Linkage, linkage};

/// The linkage of a shelf whose ledger holds these records.
fn linked(s: &Shelf, records: Vec<Record>) -> BTreeMap<DocId, Linkage> {
    linkage(s, &assess(s, &ledger(records), JudgedAt::new(100)))
}

/// A finding `FR-<n>` holding these keys in the link `link`.
fn finding(n: u128, link: &str, keys: &[&str]) -> DocReading {
    reading(
        &format!("findings/fr-{n}.md"),
        n,
        &format!("FR-{n}"),
        &[("title", text("t")), (link, list(keys))],
    )
}

/// A design (key `d<n>`) naming these specs.
fn design(n: u128, specs: &[&str]) -> DocReading {
    let mut entries = vec![("title", text("t"))];
    if !specs.is_empty() {
        entries.push(("specs", list(specs)));
    }
    reading(&format!("designs/d{n}.md"), n, &format!("d{n}"), &entries)
}

/// `(isolated, unreached, holds_none)` of a document.
fn answers(all: &BTreeMap<DocId, Linkage>, n: u128) -> (bool, bool, bool) {
    let l = all[&docid(n)];
    (l.isolated(), l.unreached(), l.holds_none())
}

#[test]
fn every_meaning_counts_and_no_link_to_itself() {
    let s = shelf(vec![
        fr(1, &[]),
        // depends, informs, replaces: 2 -> 1, 3 -> 2, 4 -> 3
        finding(2, "base", &["FR-1"]),
        finding(3, "related", &["FR-2"]),
        finding(4, "supersedes", &["FR-3"]),
        // a link to itself, a key that names nothing, a link to an artifact
        finding(5, "base", &["FR-5"]),
        finding(6, "base", &["FR-99"]),
        sp_testing(7, &[], &["tests/a.rs"]),
        fr(8, &[]),
    ]);
    let all = linked(&s, vec![]);
    let got: Vec<(u128, bool, bool)> = (1..=8)
        .map(|n| (n, all[&docid(n)].incoming, all[&docid(n)].outgoing))
        .collect();
    assert_eq!(
        got,
        vec![
            (1, true, false),
            (2, true, true),
            (3, true, true),
            (4, false, true),
            (5, false, true),
            (6, false, true),
            (7, false, true),
            (8, false, false),
        ]
    );
}

#[test]
fn a_holder_decided_against_reaches_nothing() {
    let s = shelf(vec![fr(1, &[]), sp_testing(2, &["FR-1"], &[])]);
    assert!(linked(&s, vec![])[&docid(1)].incoming);
    let all = linked(&s, vec![retire(&s, 2, 100, 10)]);
    assert!(!all[&docid(1)].incoming);
    // Its own link still counts as held.
    assert!(all[&docid(2)].outgoing);
}

#[test]
fn links_into_the_own_book_count_for_what_may_be() {
    // No other book points into the finding, whose links all point into itself:
    // those links alone let a finding be reached and hold one.
    let mut decls = decls();
    decls[1].links.retain(|l| l.name != "grounds");
    let schema = Schema::validate(decls, &spaces()).unwrap();
    let s = shelf_reading(&schema, vec![fr(1, &[]), fr(2, &["FR-1"]), fr(3, &[])], &[]);
    let all = linked(&s, vec![]);
    assert!(all[&docid(1)].may_hold && all[&docid(1)].reachable);
    assert_eq!(answers(&all, 1), (false, false, true));
    assert_eq!(answers(&all, 2), (false, true, false));
    // The lone finding is isolated.
    assert_eq!(answers(&all, 3), (true, false, false));
}

#[test]
fn a_replaces_link_says_nothing_of_what_may_be_and_a_book_cut_off_is_left_out() {
    // The design's only link replaces: the design may hold no link, and no other
    // link points into it, so it is cut off; and the spec, named by no other link,
    // may not be reached.
    let mut decls = decls();
    let mut supersedes = decls[0].links[2].clone();
    assert_eq!(supersedes.propagation, Propagation::Replaces);
    supersedes.target = LinkTargetDecl::Book("design".to_owned());
    decls[2].links = vec![supersedes];
    let schema = Schema::validate(decls, &spaces()).unwrap();
    let s = shelf_reading(
        &schema,
        vec![
            design(1, &[]),
            sp_testing(2, &[], &["tests/a.rs"]),
            reading(
                "designs/d3.md",
                3,
                "d3",
                &[("title", text("t")), ("supersedes", list(&["d1"]))],
            ),
            design(4, &[]),
        ],
        &[],
    );
    let all = linked(&s, vec![]);
    let d = all[&docid(1)];
    assert!(!d.reachable && !d.may_hold);
    // d1 is replaced by d3: a link it is reached by all the same.
    assert!(d.incoming && all[&docid(3)].outgoing);
    assert_eq!(answers(&all, 1), (false, false, false));
    assert_eq!(answers(&all, 3), (false, false, false));
    // d4 is linked neither way, and its declaration lets it be nothing else.
    assert!(!all[&docid(4)].incoming && !all[&docid(4)].outgoing);
    assert_eq!(answers(&all, 4), (false, false, false), "cut off");
    let spec = all[&docid(2)];
    assert!(!spec.reachable && spec.may_hold);
    assert_eq!(answers(&all, 2), (false, false, false));
}

#[test]
fn the_answers_are_disjoint() {
    let s = shelf(vec![
        // isolated: a finding linked neither way
        fr(1, &[]),
        // unreached: a spec that grounds FR-3 and that no design names; FR-3 holds
        // no link, though a finding may
        sp_testing(2, &["FR-3"], &[]),
        fr(3, &[]),
        // holds_none: a spec a design names, that holds nothing
        sp_testing(4, &[], &[]),
        design(5, &["SP-4"]),
    ]);
    let all = linked(&s, vec![]);
    assert_eq!(answers(&all, 1), (true, false, false));
    assert_eq!(answers(&all, 2), (false, true, false));
    assert_eq!(answers(&all, 3), (false, false, true));
    assert_eq!(answers(&all, 4), (false, false, true));
    // Nothing points into the design, and it holds a link.
    assert_eq!(answers(&all, 5), (false, false, false));
    for (id, l) in &all {
        let count = [l.isolated(), l.unreached(), l.holds_none()]
            .iter()
            .filter(|b| **b)
            .count();
        assert!(count <= 1, "{id:?}: {l:?}");
    }
}

#[test]
fn only_valid_documents_are_asked_about() {
    // FR-2 has no title, so it is no document: it neither reaches FR-1 nor is
    // listed.
    let invalid = reading("findings/fr-2.md", 2, "FR-2", &[("base", list(&["FR-1"]))]);
    let s = shelf(vec![fr(1, &[]), invalid]);
    let all = linked(&s, vec![]);
    assert_eq!(all.len(), 1);
    assert!(!all[&docid(1)].incoming && !all[&docid(1)].outgoing);
}
