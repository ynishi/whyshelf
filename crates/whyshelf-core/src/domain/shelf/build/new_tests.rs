//! Tests of a new document: its key and whether it would be valid.

use super::*;
use crate::domain::shelf::fixtures_tests::{docid, entries, fr, list, path, reading, shelf, text};

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
