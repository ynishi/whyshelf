//! Tests of a run of new documents: `Shelf::plan_new` and
//! `Shelf::replan_after_taken`.

use super::*;
use crate::domain::shelf::fixtures_tests::{docid, fr, list, reading, schema, shelf, spaces, text};
use crate::domain::shelf::schema::{Schema, ScopeDecl};

fn book(name: &str) -> BookName {
    BookName::new(name).unwrap()
}

fn name(text: &str) -> RunName {
    RunName::new(text).unwrap()
}

fn raw(value: RawValue) -> PlannedValue {
    PlannedValue::Raw(value)
}

/// A link to documents: each item a key (`Ok`-less text) or a name (`@name`).
fn links(items: &[&str]) -> PlannedValue {
    PlannedValue::Link(
        items
            .iter()
            .map(|item| match item.strip_prefix('@') {
                Some(named) => LinkItem::Named(name(named)),
                None => LinkItem::Key((*item).to_owned()),
            })
            .collect(),
    )
}

/// A request of row `row` of `book`, with the entries given; `n` is its identity.
fn request(
    row: usize,
    book_name: &str,
    key: Option<&str>,
    run_name: Option<&str>,
    entries: Vec<(&str, PlannedValue)>,
) -> (NewRequest, DocId) {
    (
        NewRequest {
            row,
            book: book(book_name),
            key: key.map(str::to_owned),
            name: run_name.map(name),
            entries: entries
                .into_iter()
                .map(|(entry, value)| (entry.to_owned(), value))
                .collect(),
            body: "body\n".to_owned(),
        },
        docid(1000 + row as u128),
    )
}

/// A finding: a title, and the entries given.
fn finding(
    row: usize,
    key: Option<&str>,
    run_name: Option<&str>,
    mut entries: Vec<(&str, PlannedValue)>,
) -> (NewRequest, DocId) {
    entries.push(("title", raw(text("t"))));
    request(row, "finding", key, run_name, entries)
}

fn plan(s: &Shelf, requests: Vec<(NewRequest, DocId)>) -> NewPlan {
    s.plan_new(requests, &ArtifactReadings::new())
        .unwrap_or_else(|problems| panic!("planned: {problems:?}"))
}

fn problems(s: &Shelf, requests: Vec<(NewRequest, DocId)>) -> Vec<PlanProblem> {
    s.plan_new(requests, &ArtifactReadings::new())
        .expect_err("refused")
        .into_vec()
}

/// The keys of the documents, in the order they are written.
fn keys(plan: &NewPlan) -> Vec<String> {
    plan.docs()
        .map(|new| new.doc().key().form.to_string())
        .collect()
}

/// What each key written in the link `link` of the document comes to: the key it
/// resolved to, or `?<written>` when it does not.
fn states(new: &NewDoc, link: &str) -> Vec<String> {
    let (_, link) = new
        .doc()
        .links()
        .iter()
        .find(|(name, _)| name.as_str() == link)
        .expect("the book declares the link");
    link.states()
        .iter()
        .map(|state| match state {
            LinkState::Resolved(target) => target.key().form.to_string(),
            LinkState::Unresolved(unresolved) => format!("?{}", unresolved.written()),
        })
        .collect()
}

fn doc_of<'a>(plan: &'a NewPlan, key: &str) -> &'a NewDoc {
    plan.docs()
        .find(|new| new.doc().key().form.to_string() == key)
        .expect("a document of the run")
}

/// The plan moved on after the file at the `at`th place was there.
fn replanned(s: &Shelf, plan: NewPlan, at: usize) -> NewPlan {
    match s.replan_after_taken(plan, at) {
        PlanAfterTaken::Replanned(next) => next,
        other => panic!("expected the plan to go on: {other:?}"),
    }
}

#[test]
fn keys_asked_for_by_two_rows_are_one_problem_naming_both_rows() {
    let s = shelf(vec![]);
    let problems = problems(
        &s,
        vec![
            finding(2, Some("FR-7"), None, vec![]),
            finding(3, None, None, vec![]),
            finding(5, Some("FR-7"), None, vec![]),
        ],
    );
    assert_eq!(problems.len(), 1);
    let PlanProblem::KeyTwice { rows, key } = &problems[0] else {
        panic!("{problems:?}");
    };
    assert_eq!(rows.iter().copied().collect::<Vec<_>>(), [2, 5]);
    assert_eq!(key.to_string(), "finding:FR-7");
    assert_eq!(
        problems[0].to_string(),
        "rows 2 and 5 ask for the key `FR-7`"
    );
}

#[test]
fn a_key_asked_for_that_a_file_writes_is_refused_with_its_row() {
    let s = shelf(vec![fr(1, &[])]);
    let problems = problems(&s, vec![finding(4, Some("FR-1"), None, vec![])]);
    assert!(matches!(
        problems.as_slice(),
        [PlanProblem::ClaimKey {
            row: 4,
            problem: ClaimKeyProblem::Taken { .. }
        }]
    ));
    assert_eq!(
        problems[0].to_string(),
        "row 4: the key `finding:FR-1` is taken"
    );
}

#[test]
fn chosen_keys_pass_over_the_keys_asked_for_and_the_keys_written() {
    // The files write FR-1 and FR-4; the run asks for FR-6, and chooses four more.
    let s = shelf(vec![fr(1, &[]), fr(4, &[])]);
    let planned = plan(
        &s,
        vec![
            finding(1, None, None, vec![]),
            finding(2, Some("FR-6"), None, vec![]),
            finding(3, None, None, vec![]),
            finding(4, None, None, vec![]),
            finding(5, None, None, vec![]),
        ],
    );
    assert_eq!(keys(&planned), ["FR-5", "FR-6", "FR-7", "FR-8", "FR-9"]);
}

#[test]
fn chosen_keys_are_counted_for_each_book_on_its_own() {
    let s = shelf(vec![fr(2, &[])]);
    let planned = plan(
        &s,
        vec![
            finding(1, None, None, vec![]),
            request(2, "spec", None, None, vec![("title", raw(text("s")))]),
            finding(3, None, None, vec![]),
            request(4, "spec", None, None, vec![("title", raw(text("s")))]),
        ],
    );
    assert_eq!(keys(&planned), ["FR-3", "SP-1", "FR-4", "SP-2"]);
}

#[test]
fn a_row_with_no_key_in_a_book_of_slugs_or_an_unknown_book_is_refused_with_its_row() {
    let s = shelf(vec![]);
    let problems = problems(
        &s,
        vec![
            request(1, "design", None, None, vec![("title", raw(text("d")))]),
            request(2, "nope", None, None, vec![]),
            request(3, "nope", Some("X-1"), None, vec![]),
        ],
    );
    assert!(matches!(
        problems.as_slice(),
        [
            PlanProblem::NextKey {
                row: 1,
                problem: NextKeyProblem::SlugBook { .. }
            },
            PlanProblem::NextKey {
                row: 2,
                problem: NextKeyProblem::UnknownBook { .. }
            },
            PlanProblem::ClaimKey {
                row: 3,
                problem: ClaimKeyProblem::UnknownBook { .. }
            },
        ]
    ));
}

#[test]
fn a_name_stands_for_the_key_of_its_row_and_the_link_is_resolved_whichever_comes_first() {
    let s = shelf(vec![]);
    // A link forward (row 1 to row 2), and one backward (row 3 to row 2).
    let planned = plan(
        &s,
        vec![
            finding(1, None, Some("a"), vec![("base", links(&["@b"]))]),
            finding(2, None, Some("b"), vec![]),
            finding(3, None, Some("c"), vec![("base", links(&["@b", "FR-77"]))]),
        ],
    );
    // The row linked to is written first.
    assert_eq!(keys(&planned), ["FR-2", "FR-1", "FR-3"]);
    assert_eq!(states(doc_of(&planned, "FR-1"), "base"), ["FR-2"]);
    assert_eq!(states(doc_of(&planned, "FR-3"), "base"), ["FR-2", "?FR-77"]);
    // The file holds the key, not the name.
    assert_eq!(
        doc_of(&planned, "FR-1").entries().as_map()["base"],
        list(&["FR-2"])
    );
    assert!(doc_of(&planned, "FR-1").entries().get("as").is_none());
    // Only what no document answers to is unresolved.
    assert_eq!(doc_of(&planned, "FR-1").unresolved().len(), 0);
    assert_eq!(doc_of(&planned, "FR-3").unresolved().len(), 1);
}

#[test]
fn names_in_a_ring_are_resolved_and_the_ring_stays_in_the_order_of_the_run() {
    let s = shelf(vec![]);
    let planned = plan(
        &s,
        vec![
            finding(1, None, Some("a"), vec![("base", links(&["@b"]))]),
            finding(2, None, Some("b"), vec![("base", links(&["@a"]))]),
        ],
    );
    assert_eq!(keys(&planned), ["FR-1", "FR-2"]);
    assert_eq!(states(doc_of(&planned, "FR-1"), "base"), ["FR-2"]);
    assert_eq!(states(doc_of(&planned, "FR-2"), "base"), ["FR-1"]);
}

#[test]
fn a_row_that_names_itself_resolves_to_itself() {
    let s = shelf(vec![]);
    let planned = plan(
        &s,
        vec![finding(
            1,
            None,
            Some("a"),
            vec![("related", links(&["@a"]))],
        )],
    );
    assert_eq!(states(doc_of(&planned, "FR-1"), "related"), ["FR-1"]);
}

#[test]
fn a_name_no_row_has_is_refused_with_the_row_and_the_link() {
    let s = shelf(vec![]);
    let problems = problems(
        &s,
        vec![
            finding(1, None, Some("a"), vec![]),
            finding(2, None, None, vec![("base", links(&["@b"]))]),
        ],
    );
    assert_eq!(
        problems,
        [PlanProblem::UnknownName {
            row: 2,
            link: "base".to_owned(),
            name: name("b")
        }]
    );
    assert_eq!(
        problems[0].to_string(),
        "row 2 links `base` to `b`, which no row of the run is named"
    );
}

#[test]
fn a_name_given_to_two_rows_is_one_problem_naming_them_and_no_second_one_for_a_link() {
    let s = shelf(vec![]);
    let problems = problems(
        &s,
        vec![
            finding(1, None, Some("a"), vec![]),
            finding(2, None, None, vec![("base", links(&["@a"]))]),
            finding(4, None, Some("a"), vec![]),
        ],
    );
    assert_eq!(problems.len(), 1);
    assert_eq!(problems[0].to_string(), "rows 1 and 4 are both named `a`");
}

#[test]
fn a_name_of_a_row_of_another_book_than_the_link_points_into_is_refused() {
    let s = shelf(vec![]);
    let problems = problems(
        &s,
        vec![
            request(1, "spec", None, Some("a"), vec![("title", raw(text("s")))]),
            request(
                2,
                "spec",
                None,
                None,
                vec![("title", raw(text("s"))), ("grounds", links(&["@a"]))],
            ),
        ],
    );
    assert_eq!(
        problems,
        [PlanProblem::NameOtherBook {
            row: 2,
            link: "grounds".to_owned(),
            name: name("a"),
            book: book("spec"),
            target: book("finding"),
        }]
    );
    assert_eq!(
        problems[0].to_string(),
        "row 2 links `grounds` to `a`, a document of the book `spec`, not of `finding`"
    );
}

#[test]
fn a_name_of_a_row_of_the_book_the_link_points_into_is_taken_across_books() {
    let s = shelf(vec![]);
    let planned = plan(
        &s,
        vec![
            request(
                1,
                "spec",
                None,
                None,
                vec![("title", raw(text("s"))), ("grounds", links(&["@f"]))],
            ),
            finding(2, None, Some("f"), vec![]),
        ],
    );
    assert_eq!(keys(&planned), ["FR-1", "SP-1"]);
    assert_eq!(states(doc_of(&planned, "SP-1"), "grounds"), ["FR-1"]);
}

#[test]
fn one_row_refused_refuses_the_run_and_every_row_refused_is_told_with_its_number() {
    let s = shelf(vec![]);
    // Rows are numbered by the caller (lines of a file, with blank ones between).
    let problems = problems(
        &s,
        vec![
            finding(3, None, None, vec![]),
            request(5, "finding", None, None, vec![]),
            finding(6, None, None, vec![("colour", raw(text("red")))]),
        ],
    );
    let rows: Vec<usize> = problems
        .iter()
        .map(|problem| match problem {
            PlanProblem::Doc { row, .. } => *row,
            other => panic!("{other:?}"),
        })
        .collect();
    assert_eq!(rows, [5, 6]);
    assert!(
        problems[0]
            .to_string()
            .starts_with("row 5: the new document would be invalid: ")
    );
}

#[test]
fn a_run_judges_a_row_as_the_shelf_judges_one() {
    let s = shelf(vec![]);
    let (alone, id) = finding(1, None, None, vec![("count", raw(RawValue::Integer(3)))]);
    let planned = plan(&s, vec![(alone, id)]);
    let key = s.next_key(&book("finding")).unwrap();
    let judged = s
        .check_new(
            id,
            &key,
            [
                ("title".to_owned(), text("t")),
                ("count".to_owned(), RawValue::Integer(3)),
            ]
            .into_iter()
            .collect(),
            "body\n".to_owned(),
            &ArtifactReadings::new(),
        )
        .unwrap();
    assert_eq!(planned.doc_at(0), Some(&judged));
}

#[test]
fn the_heads_are_the_documents_of_the_run_no_other_stands_on() {
    let s = shelf(vec![]);
    let planned = plan(
        &s,
        vec![
            request(
                1,
                "spec",
                None,
                None,
                vec![("title", raw(text("s"))), ("grounds", links(&["@f"]))],
            ),
            finding(2, None, Some("f"), vec![]),
            // Only informs: it stands on nothing, and nothing stands on it.
            finding(3, None, None, vec![("related", links(&["@f"]))]),
        ],
    );
    let heads: Vec<String> = planned.heads().iter().map(ToString::to_string).collect();
    // The finding is stood on by the spec; the spec and the third finding are not
    // (heads come in the order of writing, the finding first).
    assert_eq!(keys(&planned), ["FR-1", "SP-1", "FR-2"]);
    assert_eq!(heads, ["spec:SP-1", "finding:FR-2"].map(String::from));
}

#[test]
fn heads_follow_the_depends_rule_order_uses() {
    // A finding that depends on one of the run, and one that depends on a file's.
    let s = shelf(vec![fr(1, &[])]);
    let planned = plan(
        &s,
        vec![
            finding(1, None, Some("a"), vec![("base", links(&["FR-1"]))]),
            finding(2, None, None, vec![("base", links(&["@a"]))]),
        ],
    );
    let heads: Vec<String> = planned.heads().iter().map(ToString::to_string).collect();
    assert_eq!(heads, ["finding:FR-3"]);
}

#[test]
fn only_the_row_taken_is_given_another_key_and_the_rows_that_name_it_follow() {
    let s = shelf(vec![]);
    let planned = plan(
        &s,
        vec![
            finding(1, None, Some("x"), vec![]),
            finding(2, None, Some("y"), vec![]),
            finding(3, None, None, vec![("base", links(&["@y"]))]),
        ],
    );
    assert_eq!(keys(&planned), ["FR-1", "FR-2", "FR-3"]);
    let after = replanned(&s, planned, 1);
    // The number after the highest the run holds; the others keep theirs.
    assert_eq!(keys(&after), ["FR-1", "FR-4", "FR-3"]);
    assert_eq!(states(doc_of(&after, "FR-3"), "base"), ["FR-4"]);
    assert_eq!(
        doc_of(&after, "FR-3").entries().as_map()["base"],
        list(&["FR-4"])
    );
    // The file of FR-3 is the one that was planned, not one that is now taken.
    assert_eq!(
        doc_of(&after, "FR-4").doc().origin().as_str(),
        "findings/FR-4.md"
    );
}

#[test]
fn a_row_taken_after_another_was_taken_is_followed_by_the_highest_number_of_the_run() {
    let s = shelf(vec![]);
    let planned = plan(
        &s,
        vec![
            finding(1, None, None, vec![]),
            finding(2, None, None, vec![]),
            finding(3, None, None, vec![]),
        ],
    );
    let after = replanned(&s, planned, 0);
    assert_eq!(keys(&after), ["FR-4", "FR-2", "FR-3"]);
    let after = replanned(&s, after, 0);
    assert_eq!(keys(&after), ["FR-5", "FR-2", "FR-3"]);
}

#[test]
fn the_other_row_of_a_ring_is_not_given_another_key_when_the_first_is_taken() {
    let s = shelf(vec![]);
    let planned = plan(
        &s,
        vec![
            finding(1, None, Some("a"), vec![("base", links(&["@b"]))]),
            finding(2, None, Some("b"), vec![("base", links(&["@a"]))]),
        ],
    );
    // Nothing is written yet when the key of the first is found taken.
    let after = replanned(&s, planned, 0);
    assert_eq!(keys(&after), ["FR-3", "FR-2"]);
    assert_eq!(states(doc_of(&after, "FR-3"), "base"), ["FR-2"]);
    assert_eq!(states(doc_of(&after, "FR-2"), "base"), ["FR-3"]);
}

#[test]
fn a_row_already_written_that_names_the_row_taken_leaves_the_run_stuck() {
    let s = shelf(vec![]);
    let planned = plan(
        &s,
        vec![
            finding(4, None, Some("a"), vec![("base", links(&["@b"]))]),
            finding(7, None, Some("b"), vec![("base", links(&["@a"]))]),
        ],
    );
    // The first (row 4) is written, and links to the second (row 7) by name.
    let stuck = s.replan_after_taken(planned, 1);
    assert_eq!(
        stuck,
        PlanAfterTaken::Stuck {
            row: 7,
            written_row: 4
        }
    );
}

#[test]
fn a_row_already_written_that_does_not_name_the_row_taken_is_no_trouble() {
    let s = shelf(vec![]);
    let planned = plan(
        &s,
        vec![
            finding(1, None, Some("a"), vec![("base", links(&["FR-2"]))]),
            finding(2, None, Some("b"), vec![]),
        ],
    );
    // A written row links to FR-2 by its key as typed, not by name: it is not stuck
    // (what its text said is the person's).
    let after = replanned(&s, planned, 1);
    assert_eq!(keys(&after), ["FR-1", "FR-3"]);
}

#[test]
fn a_key_that_was_asked_for_is_not_chosen_again() {
    let s = shelf(vec![]);
    let planned = plan(
        &s,
        vec![
            finding(1, None, None, vec![]),
            finding(2, Some("FR-9"), Some("n"), vec![]),
        ],
    );
    assert_eq!(s.replan_after_taken(planned, 1), PlanAfterTaken::Asked);
}

#[test]
fn a_place_that_is_not_in_the_order_is_answered_as_asked() {
    let s = shelf(vec![]);
    let planned = plan(&s, vec![finding(1, None, None, vec![])]);
    assert_eq!(s.replan_after_taken(planned, 3), PlanAfterTaken::Asked);
}

#[test]
fn the_run_gives_up_after_sixty_four_keys_with_the_first_and_the_last_named() {
    // Every key from FR-5 is taken: FR-5 ..= FR-68 are the 64 keys tried.
    let s = shelf(vec![fr(4, &[])]);
    let mut planned = plan(&s, vec![finding(1, None, None, vec![])]);
    assert_eq!(keys(&planned), ["FR-5"]);
    for tried in 1..NEXT_KEY_TRIES {
        planned = replanned(&s, planned, 0);
        assert_eq!(keys(&planned), [format!("FR-{}", 5 + tried)]);
    }
    let PlanAfterTaken::NoFreeKey(problem) = s.replan_after_taken(planned, 0) else {
        panic!("gave up");
    };
    assert_eq!(problem.book, book("finding"));
    assert_eq!(problem.first.to_string(), "finding:FR-5");
    assert_eq!(problem.last.to_string(), "finding:FR-68");
    assert_eq!(
        problem.to_string(),
        "the book `finding` had a file at every key from `FR-5` to `FR-68`"
    );
}

#[test]
fn a_run_whose_every_row_is_moved_once_is_written_whatever_the_number_of_rows() {
    // More rows than the keys one row may try: each is found taken once, so each tries
    // two keys, and the run is never stopped for how many rows met another run.
    let rows = NEXT_KEY_TRIES as usize + 6;
    let s = shelf(vec![]);
    let mut planned = plan(
        &s,
        (1..=rows)
            .map(|row| finding(row, None, None, vec![]))
            .collect(),
    );
    for at in 0..rows {
        planned = replanned(&s, planned, at);
    }
    // Each row went on to the number after the highest the run held, so no two rows
    // share a key, and all of them are written.
    let expected: Vec<String> = (rows + 1..=2 * rows).map(|n| format!("FR-{n}")).collect();
    assert_eq!(keys(&planned), expected);
}

#[test]
fn a_row_gives_up_after_its_own_sixty_four_keys_naming_its_first_and_last() {
    let s = shelf(vec![]);
    let mut planned = plan(
        &s,
        vec![
            finding(1, None, None, vec![]),
            finding(2, None, None, vec![]),
        ],
    );
    assert_eq!(keys(&planned), ["FR-1", "FR-2"]);
    // The first row is moved once: its try does not count for the second.
    planned = replanned(&s, planned, 0);
    assert_eq!(keys(&planned), ["FR-3", "FR-2"]);
    // The second row has tried FR-2; 63 more keys are 64 tried, which is as many as
    // it may.
    for _ in 1..NEXT_KEY_TRIES {
        planned = replanned(&s, planned, 1);
    }
    let PlanAfterTaken::NoFreeKey(problem) = s.replan_after_taken(planned, 1) else {
        panic!("gave up");
    };
    assert_eq!(problem.first.to_string(), "finding:FR-2");
    assert_eq!(problem.last.to_string(), "finding:FR-66");
    assert_eq!(
        problem.to_string(),
        "the book `finding` had a file at every key from `FR-2` to `FR-66`"
    );
}

#[test]
fn a_number_after_the_highest_that_the_grammar_has_no_room_for_is_no_next_key() {
    // The highest key a file writes has 19 digits less one; the key chosen is the
    // last number that fits, and the one after it is too long.
    let s = shelf(vec![reading(
        "findings/x.md",
        1,
        "FR-9999999999999999998",
        &[("title", text("t"))],
    )]);
    let planned = plan(&s, vec![finding(1, None, None, vec![])]);
    assert_eq!(keys(&planned), ["FR-9999999999999999999"]);
    assert!(matches!(
        s.replan_after_taken(planned, 0),
        PlanAfterTaken::NoNextKey(NextKeyProblem::Exhausted {
            problem: KeyProblem::NumberTooLong { .. },
            ..
        })
    ));
}

#[test]
fn a_next_number_the_range_does_not_take_is_no_next_key() {
    let mut declared = crate::domain::shelf::fixtures_tests::decls();
    declared[0].scope = ScopeDecl {
        include: Some(vec!["FR-1.md".to_owned(), "FR-2.md".to_owned()]),
        exclude: None,
        without_front_matter: None,
    };
    let schema = Schema::validate(declared, &spaces()).unwrap();
    let s = Shelf::build(&schema, Vec::new(), &ArtifactReadings::new());
    let planned = plan(&s, vec![finding(1, None, None, vec![])]);
    assert_eq!(keys(&planned), ["FR-1"]);
    let planned = replanned(&s, planned, 0);
    assert_eq!(keys(&planned), ["FR-2"]);
    let PlanAfterTaken::NoNextKey(NextKeyProblem::OutOfRange(out)) =
        s.replan_after_taken(planned, 0)
    else {
        panic!("FR-3 is out of the range");
    };
    assert_eq!(out.key.to_string(), "finding:FR-3");
}

#[test]
fn a_run_on_a_shelf_with_a_schema_reads_nothing_else_of_the_shelf() {
    // The schema alone is enough to plan: the shelf is only asked its index.
    let s = Shelf::build(&schema(), Vec::new(), &ArtifactReadings::new());
    let planned = plan(&s, vec![finding(1, None, None, vec![])]);
    assert_eq!(planned.len(), 1);
    assert!(!planned.is_empty());
}
