//! Tests of `book_links` over the in-memory fake: what each valid document of one book
//! names in its links to artifacts, in the order written and by space and name; the
//! files of the book that are no valid document, one with no `id:` among them; and a
//! book the declaration does not declare.

use std::collections::BTreeMap;

use crate::domain::shelf::build::{DocReading, RawValue};
use crate::domain::shelf::schema::BookName;

use super::fake_tests::{FakeShelf, artifact, block_on, docid, file, file_space, fr, path, sp};
use super::{BookLinksReport, ShelfApp, ShelfError};

fn app(files: Vec<DocReading>) -> ShelfApp<FakeShelf> {
    ShelfApp::new(FakeShelf::new().with_files(files))
}

fn spec() -> BookName {
    BookName::new("spec").unwrap()
}

fn links_of(app: &ShelfApp<FakeShelf>, book: &BookName) -> BookLinksReport {
    block_on(app.book_links(book)).unwrap()
}

/// A spec `SP-<n>` whose `tests` entry is `tests`, as written.
fn sp_with_tests(n: u128, tests: RawValue) -> DocReading {
    let mut entries = BTreeMap::new();
    entries.insert("id".to_owned(), RawValue::Text(docid(n).to_string()));
    entries.insert("key".to_owned(), RawValue::Text(format!("SP-{n}")));
    entries.insert("title".to_owned(), RawValue::Text("t".to_owned()));
    entries.insert("tests".to_owned(), tests);
    DocReading::Parsed {
        origin: path(&format!("specs/sp-{n}.md")),
        entries,
        body: "t\n".to_owned(),
    }
}

#[test]
fn each_document_of_the_book_names_its_artifacts_in_the_order_written_and_no_document() {
    let app = app(vec![
        fr(1, "f", &[]),
        file(
            "specs/sp-3.md",
            3,
            "SP-3",
            "t",
            &[("grounds", &["FR-1"]), ("tests", &["src/b.rs", "src/a.rs"])],
        ),
    ]);
    let report = links_of(&app, &spec());
    assert!(report.not_valid.is_empty(), "{report:?}");
    let [doc] = report.docs.as_slice() else {
        panic!("one document of the book: {report:?}");
    };
    assert_eq!(doc.key.to_string(), "spec:SP-3");
    assert_eq!(doc.file, path("specs/sp-3.md"));
    // The link to a document is no link to artifacts; the artifacts keep their order
    // and their space.
    assert_eq!(
        doc.links.keys().map(|l| l.as_str()).collect::<Vec<_>>(),
        ["tests"]
    );
    assert_eq!(
        doc.links.values().next().unwrap(),
        &[
            artifact(&file_space(), "src/b.rs"),
            artifact(&file_space(), "src/a.rs")
        ]
    );
    // The documents of another book are not of it.
    let findings = links_of(&app, &BookName::new("finding").unwrap());
    assert_eq!(
        findings
            .docs
            .iter()
            .map(|d| d.key.to_string())
            .collect::<Vec<_>>(),
        ["finding:FR-1"]
    );
    assert!(findings.docs[0].links.is_empty(), "{findings:?}");
}

#[test]
fn a_null_an_empty_list_and_a_blank_text_name_nothing_and_are_valid() {
    let app = app(vec![
        sp_with_tests(3, RawValue::Null),
        sp_with_tests(4, RawValue::List(Vec::new())),
        sp_with_tests(5, RawValue::Text("  ".to_owned())),
        sp(6, "no tests written", &[]),
    ]);
    let report = links_of(&app, &spec());
    assert!(report.not_valid.is_empty(), "{report:?}");
    assert_eq!(report.docs.len(), 4, "{report:?}");
    assert!(
        report
            .docs
            .iter()
            .all(|doc| doc.links.values().all(Vec::is_empty)),
        "{report:?}"
    );
}

#[test]
fn a_file_of_the_book_that_is_no_valid_document_is_named_apart_with_no_id_among_them() {
    let no_id = DocReading::Parsed {
        origin: path("specs/no-id.md"),
        entries: BTreeMap::from([
            ("key".to_owned(), RawValue::Text("SP-7".to_owned())),
            ("title".to_owned(), RawValue::Text("t".to_owned())),
        ]),
        body: "t\n".to_owned(),
    };
    let app = app(vec![
        // A blank item is no name: the document is not valid.
        sp_with_tests(
            3,
            RawValue::List(vec!["src/a.rs".to_owned(), String::new()]),
        ),
        // A text that says something is no list: not valid.
        sp_with_tests(4, RawValue::Text("src/a.rs".to_owned())),
        no_id,
        sp(5, "kept", &[]),
        // Not valid, of another book.
        DocReading::Parsed {
            origin: path("findings/no-id.md"),
            entries: BTreeMap::new(),
            body: String::new(),
        },
    ]);
    let report = links_of(&app, &spec());
    let mut not_valid: Vec<&str> = report.not_valid.iter().map(|p| p.as_str()).collect();
    not_valid.sort_unstable();
    assert_eq!(
        not_valid,
        ["specs/no-id.md", "specs/sp-3.md", "specs/sp-4.md"]
    );
    assert_eq!(
        report
            .docs
            .iter()
            .map(|d| d.key.to_string())
            .collect::<Vec<_>>(),
        ["spec:SP-5"]
    );
}

#[test]
fn a_declared_book_with_no_document_is_empty_and_a_book_not_declared_is_an_error() {
    let app = app(vec![fr(1, "f", &[])]);
    let report = links_of(&app, &spec());
    assert!(
        report.docs.is_empty() && report.not_valid.is_empty(),
        "{report:?}"
    );

    let err = block_on(app.book_links(&BookName::new("design").unwrap())).unwrap_err();
    assert!(
        matches!(
            &err,
            ShelfError::UnknownBook { reference: None, book } if book.as_str() == "design"
        ),
        "{err:?}"
    );
}
