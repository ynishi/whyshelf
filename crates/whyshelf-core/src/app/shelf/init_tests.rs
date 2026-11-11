//! Tests of `init`, over the in-memory fake.

use crate::domain::shelf::schema::{BookName, LinkTargetDecl, ShelfDecl};

use super::fake_tests::{FakeShelf, block_on, decls, file, path};
use super::{
    DeclarationText, ErrorClass, InitRequest, NewDocRequest, Obstacle, ShelfApp, ShelfError,
    ShelfPort,
};

/// The text the fake reads as the two books of [`decls`].
const TEXT: &str = "the two books";

fn doc(book: &str, key: &str, entries: &[(&str, &str)]) -> NewDocRequest {
    NewDocRequest {
        book: BookName::new(book).unwrap(),
        key: Some(key.to_owned()),
        entries: entries
            .iter()
            .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
            .collect(),
        items: Vec::new(),
        body: "Body.\n".to_owned(),
    }
}

/// A finding and a spec grounded in it, the second linking to the first.
fn request(text: &str) -> InitRequest {
    InitRequest {
        declaration: DeclarationText::new(text),
        docs: vec![
            doc("finding", "FR-1", &[("title", "a finding")]),
            doc("spec", "SP-1", &[("title", "a spec"), ("grounds", "FR-1")]),
        ],
    }
}

/// An empty project whose declaration text reads as the two books.
fn fake() -> FakeShelf {
    let fake = FakeShelf::new();
    fake.read_text_as(TEXT, ShelfDecl::of_books(decls()));
    fake
}

/// Nothing was written: no declaration (or the one that was there) and no document.
fn assert_nothing_written(app: &ShelfApp<FakeShelf>, declaration: Option<&str>) {
    assert_eq!(app.ports().declaration_file().as_deref(), declaration);
    assert_eq!(app.ports().created_count(), 0);
}

#[test]
fn init_writes_the_declaration_and_then_each_document_linked_to_the_one_before() {
    let mut app = ShelfApp::new(fake());
    let done = block_on(app.init(request(TEXT))).unwrap();
    assert_eq!(done.declaration, path(".whyshelf/books.toml"));
    assert_eq!(app.ports().declaration_file().as_deref(), Some(TEXT));
    assert_eq!(app.ports().created_count(), 2);
    let keys: Vec<String> = done.docs.iter().map(|d| d.key.to_string()).collect();
    assert_eq!(keys, ["finding:FR-1", "spec:SP-1"]);
    let files: Vec<String> = done.docs.iter().map(|d| d.file.to_string()).collect();
    assert_eq!(files, ["findings/FR-1.md", "specs/SP-1.md"]);
    // The spec's link to the finding made before it resolves.
    assert!(done.docs.iter().all(|d| d.unresolved.is_empty()));
    assert_ne!(done.docs[0].id, done.docs[1].id);

    // The project reads as the declaration says, and both documents are valid.
    let check = block_on(app.books_check()).unwrap();
    assert!(check.is_clean(), "{check:?}");
}

#[test]
fn init_with_no_document_writes_the_declaration_alone() {
    let mut app = ShelfApp::new(fake());
    let done = block_on(app.init(InitRequest {
        declaration: DeclarationText::new(TEXT),
        docs: Vec::new(),
    }))
    .unwrap();
    assert!(done.docs.is_empty());
    assert_eq!(app.ports().declaration_file().as_deref(), Some(TEXT));
    assert_eq!(app.ports().created_count(), 0);
}

/// What is in the way, from a refusal of `init`.
fn obstacles(error: &ShelfError) -> Vec<Obstacle> {
    let ShelfError::InTheWay { obstacles } = error else {
        panic!("{error:?}");
    };
    assert_eq!(error.class(), ErrorClass::Request);
    obstacles.clone().into_vec()
}

#[test]
fn a_declaration_in_the_way_refuses_and_writes_nothing() {
    let fake = fake();
    *fake.declaration_file.lock().unwrap() = Some("what was there".to_owned());
    let mut app = ShelfApp::new(fake);
    let error = block_on(app.init(request(TEXT))).unwrap_err();
    assert_eq!(
        obstacles(&error),
        [Obstacle::There(path(".whyshelf/books.toml"))]
    );
    assert_eq!(
        error.to_string(),
        ".whyshelf/books.toml is already there; nothing was written"
    );
    assert_nothing_written(&app, Some("what was there"));
}

#[test]
fn a_document_path_in_the_way_refuses_and_writes_nothing() {
    let fake = fake();
    fake.put_file("specs/SP-1.md", b"a file of the user's");
    let mut app = ShelfApp::new(fake);
    let error = block_on(app.init(request(TEXT))).unwrap_err();
    assert_eq!(obstacles(&error), [Obstacle::There(path("specs/SP-1.md"))]);
    assert_nothing_written(&app, None);
}

#[test]
fn a_parent_that_is_no_directory_is_in_the_way_and_named() {
    let fake = fake();
    fake.put_file("findings", b"a file where a book's directory goes");
    let mut app = ShelfApp::new(fake);
    let error = block_on(app.init(request(TEXT))).unwrap_err();
    assert_eq!(obstacles(&error), [Obstacle::There(path("findings"))]);
    assert_nothing_written(&app, None);
}

#[test]
fn everything_in_the_way_is_named_the_declaration_first() {
    let fake = fake();
    *fake.declaration_file.lock().unwrap() = Some("old".to_owned());
    fake.put_file("findings/FR-1.md", b"old");
    fake.put_file("specs/SP-1.md", b"old");
    let mut app = ShelfApp::new(fake);
    let error = block_on(app.init(request(TEXT))).unwrap_err();
    assert_eq!(
        obstacles(&error),
        [
            Obstacle::There(path(".whyshelf/books.toml")),
            Obstacle::There(path("findings/FR-1.md")),
            Obstacle::There(path("specs/SP-1.md"))
        ]
    );
    assert_eq!(
        error.to_string(),
        ".whyshelf/books.toml, findings/FR-1.md and specs/SP-1.md are already there; nothing was written"
    );
}

#[test]
fn every_file_elsewhere_in_a_book_that_writes_a_key_is_named_and_nothing_is_written() {
    let fake = fake().with_files(vec![
        file("findings/old.md", 1, "FR-1", "old", &[]),
        file("findings/older.md", 2, "FR-1", "older", &[]),
        file("specs/old.md", 3, "SP-1", "old", &[]),
    ]);
    let mut app = ShelfApp::new(fake);
    let error = block_on(app.init(request(TEXT))).unwrap_err();
    let found: Vec<(String, String)> = obstacles(&error)
        .iter()
        .map(|o| match o {
            Obstacle::KeyTaken { key, file } => (key.to_string(), file.to_string()),
            Obstacle::There(path) => panic!("{path} is no key"),
        })
        .collect();
    let pair = |k: &str, f: &str| (k.to_owned(), f.to_owned());
    assert_eq!(
        found,
        [
            pair("finding:FR-1", "findings/old.md"),
            pair("finding:FR-1", "findings/older.md"),
            pair("spec:SP-1", "specs/old.md")
        ]
    );
    assert_eq!(
        error.to_string(),
        "findings/old.md already writes the key finding:FR-1; findings/older.md already writes the key finding:FR-1; specs/old.md already writes the key spec:SP-1; nothing was written"
    );
    assert_nothing_written(&app, None);
}

#[test]
fn a_write_that_fails_part_way_says_what_was_written_and_what_was_not() {
    let fake = fake();
    // The declaration and FR-1 are written; the third write, SP-1, fails.
    *fake.creates_before_failing.lock().unwrap() = Some(1);
    let mut app = ShelfApp::new(fake);
    let error = block_on(app.init(request(TEXT))).unwrap_err();
    let ShelfError::PartWay {
        written,
        failed,
        cause,
    } = &error
    else {
        panic!("{error:?}");
    };
    assert_eq!(
        written.clone().into_vec(),
        [path(".whyshelf/books.toml"), path("findings/FR-1.md")]
    );
    assert_eq!(*failed, path("specs/SP-1.md"));
    assert!(matches!(
        **cause,
        ShelfError::Port {
            port: ShelfPort::DocWrite,
            ..
        }
    ));
    assert_eq!(error.class(), ErrorClass::Failed);
    assert_eq!(
        error.to_string(),
        ".whyshelf/books.toml and findings/FR-1.md were written, then specs/SP-1.md was not"
    );
    assert_eq!(app.ports().declaration_file().as_deref(), Some(TEXT));
    assert_eq!(app.ports().created_count(), 1);
}

#[test]
fn a_file_that_appears_before_its_write_is_a_request_error_part_way() {
    let fake = fake();
    fake.appear_before_create
        .lock()
        .unwrap()
        .insert(path("findings/FR-1.md"));
    let mut app = ShelfApp::new(fake);
    let error = block_on(app.init(request(TEXT))).unwrap_err();
    let ShelfError::PartWay { written, cause, .. } = &error else {
        panic!("{error:?}");
    };
    assert_eq!(written.clone().into_vec(), [path(".whyshelf/books.toml")]);
    assert!(matches!(**cause, ShelfError::FileTaken { .. }));
    assert_eq!(error.class(), ErrorClass::Request);
    assert_eq!(
        error.to_string(),
        ".whyshelf/books.toml was written, then findings/FR-1.md was not"
    );
}

#[test]
fn a_text_that_is_no_declaration_is_refused_before_anything_is_written() {
    let mut app = ShelfApp::new(fake());
    let error = block_on(app.init(request("no declaration"))).unwrap_err();
    assert!(
        matches!(
            error,
            ShelfError::Port {
                port: ShelfPort::Schema,
                ..
            }
        ),
        "{error:?}"
    );
    assert_eq!(error.class(), ErrorClass::Failed);
    assert_nothing_written(&app, None);
}

#[test]
fn a_declaration_that_is_not_sound_is_refused_before_anything_is_written() {
    let fake = fake();
    let mut unsound = decls();
    unsound[1].links[0].target = LinkTargetDecl::Book("nowhere".to_owned());
    fake.read_text_as("unsound", ShelfDecl::of_books(unsound));
    let mut app = ShelfApp::new(fake);
    let error = block_on(app.init(request("unsound"))).unwrap_err();
    assert!(matches!(error, ShelfError::Schema(_)), "{error:?}");
    assert_eq!(error.class(), ErrorClass::Request);
    assert_nothing_written(&app, None);
}

#[test]
fn a_document_its_book_refuses_is_refused_before_anything_is_written() {
    let mut app = ShelfApp::new(fake());
    let mut req = request(TEXT);
    // A spec with no title, which its book requires.
    req.docs[1].entries.retain(|(name, _)| name != "title");
    let error = block_on(app.init(req)).unwrap_err();
    assert!(matches!(error, ShelfError::NewDoc(_)), "{error:?}");
    assert_nothing_written(&app, None);
}
