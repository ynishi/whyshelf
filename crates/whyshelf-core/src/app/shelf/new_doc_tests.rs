//! Tests of `new_doc`, over the in-memory fake.

use crate::domain::shelf::build::{ClaimKeyProblem, DocProblem, NewDocProblem, RawValue};
use crate::domain::shelf::schema::{
    BookName, BookSchemaDecl, FieldEntryDecl, FieldPresence, FieldTypeDecl, KeyPatternDecl,
};

use super::fake_tests::{FakeShelf, block_on, file, fr};
use super::{ErrorClass, NewDocRequest, ShelfApp, ShelfError};

fn request(book: &str, key: Option<&str>, entries: &[(&str, &str)]) -> NewDocRequest {
    NewDocRequest {
        book: BookName::new(book).unwrap(),
        key: key.map(str::to_owned),
        entries: entries
            .iter()
            .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
            .collect(),
        body: "Body.\n".to_owned(),
    }
}

/// A book with a field of each type, keyed `M-<n>`.
fn measures() -> BookSchemaDecl {
    let field = |name: &str, ty| FieldEntryDecl {
        name: name.to_owned(),
        ty,
        presence: FieldPresence::Optional,
    };
    BookSchemaDecl {
        name: "measure".to_owned(),
        dir: "measures".to_owned(),
        key: KeyPatternDecl::Prefixed {
            prefix: "M".to_owned(),
        },
        fields: vec![
            field("count", FieldTypeDecl::Integer),
            field("flag", FieldTypeDecl::Bool),
            field("tags", FieldTypeDecl::TextList),
            field("note", FieldTypeDecl::Text),
        ],
        links: Vec::new(),
    }
}

#[test]
fn a_new_document_takes_the_next_key_is_written_and_reads_back_valid() {
    let mut app = ShelfApp::new(FakeShelf::new().with_files(vec![fr(1, "slow", &[])]));
    let created = block_on(app.new_doc(request(
        "spec",
        None,
        &[("title", "cache"), ("grounds", "FR-1, FR-9")],
    )))
    .unwrap();
    assert_eq!(created.key.to_string(), "spec:SP-1");
    assert_eq!(created.file.as_str(), "specs/SP-1.md");
    let unresolved: Vec<_> = created
        .unresolved
        .iter()
        .map(|(link, u)| (link.to_string(), u.written().to_owned()))
        .collect();
    assert_eq!(unresolved, vec![("grounds".to_owned(), "FR-9".to_owned())]);
    assert_eq!(app.ports().created_count(), 1);

    // The shelf read again has it, valid, and the next key moved on.
    let status = block_on(app.status()).unwrap();
    assert!(status.rows.iter().any(|row| row.id == created.id
        && matches!(
            row.presence,
            crate::domain::shelf::build::DocPresence::Valid { .. }
        )));
    let again = block_on(app.new_doc(request("spec", None, &[("title", "two")]))).unwrap();
    assert_eq!(again.key.to_string(), "spec:SP-2");
    assert!(again.unresolved.is_empty());
}

#[test]
fn a_taken_key_is_a_request_error_and_writes_nothing() {
    let mut app = ShelfApp::new(FakeShelf::new().with_files(vec![fr(1, "slow", &[])]));
    let error =
        block_on(app.new_doc(request("finding", Some("FR-1"), &[("title", "t")]))).unwrap_err();
    assert!(matches!(
        error,
        ShelfError::ClaimKey(ClaimKeyProblem::Taken { .. })
    ));
    assert_eq!(error.class(), ErrorClass::Request);
    assert_eq!(app.ports().created_count(), 0);
}

#[test]
fn entries_the_book_refuses_are_listed_and_nothing_is_written() {
    let mut app = ShelfApp::new(FakeShelf::new());
    let error = block_on(app.new_doc(request("finding", None, &[("colour", "red")]))).unwrap_err();
    let ShelfError::NewDoc(NewDocProblem::Invalid(problems)) = &error else {
        panic!("invalid: {error:?}");
    };
    assert!(
        problems
            .iter()
            .any(|p| matches!(p, DocProblem::FieldMissing { .. }))
    );
    assert!(
        problems
            .iter()
            .any(|p| matches!(p, DocProblem::UnknownEntry { .. }))
    );
    assert_eq!(error.class(), ErrorClass::Request);
    assert_eq!(app.ports().created_count(), 0);
}

#[test]
fn a_title_for_a_book_with_no_title_field_is_an_unknown_entry() {
    let fake = FakeShelf::new();
    fake.decls.lock().unwrap().push(measures());
    let mut app = ShelfApp::new(fake);
    let error =
        block_on(app.new_doc(request("measure", None, &[("title", "a title")]))).unwrap_err();
    let ShelfError::NewDoc(NewDocProblem::Invalid(problems)) = &error else {
        panic!("invalid: {error:?}");
    };
    assert!(
        problems.iter().any(
            |p| matches!(p, DocProblem::UnknownEntry { name, .. } if name.as_str() == "title")
        )
    );
    assert_eq!(app.ports().created_count(), 0);
    // And with no title, the same book takes a document.
    block_on(app.new_doc(request("measure", None, &[("count", "1")]))).unwrap();
    assert_eq!(app.ports().created_count(), 1);
}

#[test]
fn texts_become_values_by_the_declared_types() {
    let fake = FakeShelf::new();
    fake.decls.lock().unwrap().push(measures());
    let mut app = ShelfApp::new(fake);
    block_on(app.new_doc(request(
        "measure",
        None,
        &[
            ("count", "-3"),
            ("flag", "true"),
            ("tags", "a, b,"),
            ("note", "12"),
        ],
    )))
    .unwrap();
    let readings = app.ports().readings.lock().unwrap().clone();
    let crate::domain::shelf::build::DocReading::Parsed { entries, .. } = &readings[0] else {
        panic!("parsed");
    };
    assert_eq!(entries["count"], RawValue::Integer(-3));
    assert_eq!(entries["flag"], RawValue::Bool(true));
    assert_eq!(
        entries["tags"],
        RawValue::List(vec!["a".to_owned(), "b".to_owned()])
    );
    assert_eq!(entries["note"], RawValue::Text("12".to_owned()));

    // A text that does not fit its type stays a text, and the domain says so.
    let error = block_on(app.new_doc(request("measure", None, &[("flag", "yes")]))).unwrap_err();
    let ShelfError::NewDoc(NewDocProblem::Invalid(problems)) = error else {
        panic!("invalid");
    };
    assert!(matches!(problems.first(), DocProblem::FieldMismatch { .. }));
}

#[test]
fn an_entry_given_twice_is_refused() {
    let mut app = ShelfApp::new(FakeShelf::new());
    let error = block_on(app.new_doc(request("finding", None, &[("title", "a"), ("title", "b")])))
        .unwrap_err();
    assert!(matches!(error, ShelfError::EntryTwice { .. }));
}

#[test]
fn a_file_already_at_the_path_is_never_replaced() {
    // `findings/FR-1.md` holds a document whose key is FR-5 (renamed, file kept).
    let app_files = vec![file("findings/FR-1.md", 1, "FR-5", "t", &[])];
    let mut app = ShelfApp::new(FakeShelf::new().with_files(app_files));
    let error =
        block_on(app.new_doc(request("finding", Some("FR-1"), &[("title", "t")]))).unwrap_err();
    assert!(matches!(error, ShelfError::FileTaken { .. }));
    assert_eq!(error.class(), ErrorClass::Request);
    assert_eq!(app.ports().created_count(), 0);
}
