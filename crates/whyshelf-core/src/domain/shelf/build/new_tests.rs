//! Tests of a new document: its key and whether it would be valid.

use super::*;
use crate::domain::shelf::fixtures_tests::{
    decls, docid, entries, fr, list, path, reading, shelf, spaces, text,
};
use crate::domain::shelf::schema::{Schema, ScopeDecl};

fn book(name: &str) -> BookName {
    BookName::new(name).unwrap()
}

fn next(s: &Shelf, name: &str) -> String {
    s.next_key(&book(name)).unwrap().key().form.to_string()
}

#[test]
fn the_next_key_of_an_empty_book_is_the_first_number() {
    assert_eq!(next(&shelf(vec![]), "finding"), "FR-1");
}

#[test]
fn the_next_key_counts_invalid_files_and_ignores_name_tails() {
    let s = shelf(vec![
        fr(1, &[]),
        // Invalid (no title), and still holds its key.
        reading("findings/x.md", 2, "FR-7", &[]),
        // An identity that cannot be read: the key is still taken.
        DocReading::Parsed {
            origin: path("findings/y.md"),
            entries: entries(&[("id", text("not-an-id")), ("key", text("FR-9"))]),
            body: String::new(),
        },
        reading("findings/z.md", 3, "FR-LATENCY", &[("title", text("t"))]),
        // A front matter that cannot be read writes no key that is seen, whatever
        // its file is named.
        DocReading::Unparsed {
            origin: path("findings/FR-20.md"),
            id: ReadId::Unreadable,
            why: RawProblem::NoFrontMatter,
        },
        // A key of another book does not count.
        reading("specs/s.md", 4, "SP-40", &[("title", text("t"))]),
    ]);
    assert_eq!(next(&s, "finding"), "FR-10");
    assert_eq!(next(&s, "spec"), "SP-41");
}

#[test]
fn a_book_of_slugs_has_no_next_key_and_an_unknown_book_none_either() {
    let s = shelf(vec![]);
    assert!(matches!(
        s.next_key(&book("design")),
        Err(NextKeyProblem::SlugBook { .. })
    ));
    assert!(matches!(
        s.next_key(&book("nope")),
        Err(NextKeyProblem::UnknownBook { .. })
    ));
}

#[test]
fn the_number_after_the_longest_is_refused_by_the_grammar() {
    let s = shelf(vec![reading(
        "findings/x.md",
        1,
        "FR-9999999999999999999",
        &[("title", text("t"))],
    )]);
    assert!(matches!(
        s.next_key(&book("finding")),
        Err(NextKeyProblem::Exhausted {
            problem: KeyProblem::NumberTooLong { .. },
            ..
        })
    ));
}

#[test]
fn a_given_key_must_fit_the_grammar_and_be_free() {
    let s = shelf(vec![
        fr(1, &[]),
        reading("designs/a.md", 2, "cache/timeout", &[]),
    ]);
    assert!(matches!(
        s.claim_key(&book("finding"), "FR-1"),
        Err(ClaimKeyProblem::Taken { .. })
    ));
    assert!(matches!(
        s.claim_key(&book("finding"), "SP-2"),
        Err(ClaimKeyProblem::Malformed { .. })
    ));
    // The invalid design (no title) still holds its key.
    assert!(matches!(
        s.claim_key(&book("design"), "cache/timeout"),
        Err(ClaimKeyProblem::Taken { .. })
    ));
    let key = s.claim_key(&book("design"), "cache/expiry").unwrap();
    assert_eq!(key.key().to_string(), "design:cache/expiry");
}

#[test]
fn a_valid_new_document_is_made_as_the_build_makes_it_with_its_unresolved_links() {
    let s = shelf(vec![fr(1, &[])]);
    let key = s.next_key(&book("finding")).unwrap();
    let new = s
        .check_new(
            docid(5),
            &key,
            entries(&[
                ("title", text("t")),
                ("count", RawValue::Integer(2)),
                ("base", list(&["FR-1", "FR-8"])),
            ]),
            "body\n".to_owned(),
            &ArtifactReadings::new(),
        )
        .unwrap();
    assert_eq!(new.doc().origin().as_str(), "findings/FR-2.md");
    assert_eq!(new.doc().key().to_string(), "finding:FR-2");
    assert_eq!(new.entries()["key"], text("FR-2"));
    assert_eq!(new.entries()["id"], text(&docid(5).to_string()));
    let unresolved: Vec<_> = new
        .unresolved()
        .into_iter()
        .map(|(link, u)| (link.to_string(), u.written().to_owned()))
        .collect();
    assert_eq!(unresolved, vec![("base".to_owned(), "FR-8".to_owned())]);

    // Read back as a file, the same entries make the same document.
    let built = shelf(vec![
        fr(1, &[]),
        DocReading::Parsed {
            origin: new.doc().origin().clone(),
            entries: new.entries().clone(),
            body: "body\n".to_owned(),
        },
    ]);
    assert_eq!(built.doc(docid(5)), Some(new.doc()));
}

#[test]
fn an_invalid_new_document_has_every_problem_the_build_finds() {
    let s = shelf(vec![]);
    let key = s.next_key(&book("finding")).unwrap();
    let Err(NewDocProblem::Invalid(problems)) = s.check_new(
        docid(5),
        &key,
        entries(&[
            ("count", text("two")),
            ("level", text("mid")),
            ("supersedes", text("FR-1")),
            ("colour", text("red")),
        ]),
        String::new(),
        &ArtifactReadings::new(),
    ) else {
        panic!("invalid");
    };
    let problems = problems.into_vec();
    assert!(problems.contains(&DocProblem::FieldMissing {
        field: EntryName::new("title").unwrap()
    }));
    assert!(
        problems
            .iter()
            .any(|p| matches!(p, DocProblem::FieldMismatch { .. }))
    );
    assert!(
        problems
            .iter()
            .any(|p| matches!(p, DocProblem::FieldNotInEnum { .. }))
    );
    assert!(
        problems
            .iter()
            .any(|p| matches!(p, DocProblem::LinkNotList { .. }))
    );
    assert!(
        problems
            .iter()
            .any(|p| matches!(p, DocProblem::UnknownEntry { .. }))
    );
}

#[test]
fn a_new_document_with_a_blank_required_title_is_refused() {
    let s = shelf(vec![]);
    let key = s.next_key(&book("finding")).unwrap();
    let Err(NewDocProblem::Invalid(problems)) = s.check_new(
        docid(5),
        &key,
        entries(&[("title", text("  "))]),
        String::new(),
        &ArtifactReadings::new(),
    ) else {
        panic!("invalid");
    };
    assert_eq!(
        problems.into_vec(),
        vec![DocProblem::FieldEmpty {
            field: EntryName::new("title").unwrap()
        }]
    );
}

#[test]
fn the_identity_and_the_key_cannot_be_given_as_entries() {
    let s = shelf(vec![]);
    let key = s.next_key(&book("finding")).unwrap();
    assert_eq!(
        s.check_new(
            docid(5),
            &key,
            entries(&[("title", text("t")), ("key", text("FR-3"))]),
            String::new(),
            &ArtifactReadings::new(),
        ),
        Err(NewDocProblem::ReservedEntry { name: "key" })
    );
}

/// The key of a `NewKey`, as text.
fn text_of(key: &NewKey) -> String {
    key.key().form.to_string()
}

/// The search moved on to the next key; anything else is a failure.
fn moved_on(s: &Shelf, search: KeySearch) -> KeySearch {
    match s.key_after_taken(search) {
        AfterTaken::Next(next) => next,
        other => panic!("expected the next key: {other:?}"),
    }
}

#[test]
fn a_chosen_key_whose_file_was_there_is_followed_by_the_next_number() {
    let s = shelf(vec![fr(1, &[]), fr(4, &[])]);
    let first = s.next_key(&book("finding")).unwrap();
    assert_eq!(text_of(&first), "FR-5");
    let second = moved_on(&s, KeySearch::start(first));
    assert_eq!(text_of(second.key()), "FR-6");
    // The one after is counted from the one chosen, not read from the shelf again.
    let third = moved_on(&s, second);
    assert_eq!(text_of(third.key()), "FR-7");
}

#[test]
fn a_given_key_whose_file_was_there_is_not_chosen_again() {
    let s = shelf(vec![fr(1, &[])]);
    let claimed = s.claim_key(&book("finding"), "FR-5").unwrap();
    assert_eq!(
        s.key_after_taken(KeySearch::start(claimed)),
        AfterTaken::Asked
    );
    let design = s.claim_key(&book("design"), "cache/expiry").unwrap();
    assert_eq!(
        s.key_after_taken(KeySearch::start(design)),
        AfterTaken::Asked
    );
}

#[test]
fn the_search_gives_up_after_sixty_four_keys_with_the_first_and_the_last_named() {
    // Every key from FR-5 is taken: FR-5 ..= FR-68 are the 64 keys tried.
    let s = shelf(vec![fr(4, &[])]);
    let first = s.next_key(&book("finding")).unwrap();
    let mut search = KeySearch::start(first);
    let mut tried = 1;
    let problem = loop {
        match s.key_after_taken(search) {
            AfterTaken::Next(next) => {
                tried += 1;
                search = next;
            }
            AfterTaken::NoFreeKey(problem) => break problem,
            other => panic!("expected the search to go on or to give up: {other:?}"),
        }
    };
    assert_eq!(tried, 64);
    assert_eq!(problem.book, book("finding"));
    assert_eq!(problem.first.to_string(), "finding:FR-5");
    assert_eq!(problem.last.to_string(), "finding:FR-68");
    assert_eq!(
        problem.to_string(),
        "the book `finding` had a file at every key from `FR-5` to `FR-68`"
    );
}

#[test]
fn a_search_goes_on_from_the_key_it_is_at() {
    let s = shelf(vec![]);
    let mut search = KeySearch::start(s.next_key(&book("finding")).unwrap());
    for _ in 0..10 {
        search = moved_on(&s, search);
    }
    assert_eq!(text_of(search.key()), "FR-11");
    let next = moved_on(&s, search);
    assert_eq!(text_of(next.key()), "FR-12");
}

#[test]
fn no_next_number_in_the_grammar_is_no_next_key() {
    // The highest key a file writes has 19 digits less one; the key chosen is the
    // last number that fits, and the one after it is too long.
    let s = shelf(vec![reading(
        "findings/x.md",
        1,
        "FR-9999999999999999998",
        &[("title", text("t"))],
    )]);
    let chosen = s.next_key(&book("finding")).unwrap();
    assert_eq!(text_of(&chosen), "FR-9999999999999999999");
    assert!(matches!(
        s.key_after_taken(KeySearch::start(chosen)),
        AfterTaken::NoNextKey(NextKeyProblem::Exhausted {
            highest: 9_999_999_999_999_999_999,
            problem: KeyProblem::NumberTooLong { .. },
            ..
        })
    ));
}

#[test]
fn a_next_number_the_range_does_not_take_is_no_next_key() {
    let mut declared = decls();
    declared[0].scope = ScopeDecl {
        include: Some(vec!["FR-1.md".to_owned(), "FR-2.md".to_owned()]),
        exclude: None,
        without_front_matter: None,
    };
    let schema = Schema::validate(declared, &spaces()).unwrap();
    let s = Shelf::build(&schema, Vec::new(), &ArtifactReadings::new());
    let first = s.next_key(&book("finding")).unwrap();
    assert_eq!(text_of(&first), "FR-1");
    let second = moved_on(&s, KeySearch::start(first));
    assert_eq!(text_of(second.key()), "FR-2");
    let AfterTaken::NoNextKey(NextKeyProblem::OutOfRange(out)) = s.key_after_taken(second) else {
        panic!("FR-3 is out of the range");
    };
    assert_eq!(out.key.to_string(), "finding:FR-3");
}
