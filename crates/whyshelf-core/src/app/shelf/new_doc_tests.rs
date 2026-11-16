//! Tests of `new_doc`, over the in-memory fake.

use crate::domain::shelf::build::{
    ClaimKeyProblem, DocProblem, DocReading, NewDocProblem, NextKeyProblem, RawProblem, RawValue,
    ReadId,
};
use crate::domain::shelf::schema::{
    BookName, BookSchemaDecl, FieldEntryDecl, FieldPresence, FieldTypeDecl, KeyPatternDecl,
    ScopeDecl,
};

use super::fake_tests::{FakeShelf, block_on, file, fr, path};
use super::{EntryInput, ErrorClass, NewDocRequest, ShelfApp, ShelfError};

fn request(book: &str, key: Option<&str>, entries: &[(&str, &str)]) -> NewDocRequest {
    NewDocRequest {
        row: 1,
        book: BookName::new(book).unwrap(),
        key: key.map(str::to_owned),
        name: None,
        entries: entries
            .iter()
            .map(|(k, v)| ((*k).to_owned(), EntryInput::Text((*v).to_owned())))
            .collect(),
        items: Vec::new(),
        body: "Body.\n".to_owned(),
    }
}

fn with_items(mut request: NewDocRequest, items: &[(&str, &str)]) -> NewDocRequest {
    request.items = items
        .iter()
        .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
        .collect();
    request
}

#[test]
fn an_item_keeps_its_commas_and_follows_the_split_items_in_order() {
    let fake = FakeShelf::new();
    fake.decls.lock().unwrap().push(measures());
    let mut app = ShelfApp::new(fake);
    let req = with_items(
        request("measure", None, &[("tags", "a, b")]),
        &[("tags", "x, y: and z"), ("tags", "w")],
    );
    block_on(app.new_doc(req)).unwrap();
    let readings = app.ports().readings.lock().unwrap().clone();
    let crate::domain::shelf::build::DocReading::Parsed { entries, .. } = &readings[0] else {
        panic!("parsed");
    };
    assert_eq!(
        entries["tags"],
        RawValue::List(vec![
            "a".to_owned(),
            "b".to_owned(),
            "x, y: and z".to_owned(),
            "w".to_owned()
        ])
    );
}

#[test]
fn items_alone_make_the_list() {
    let fake = FakeShelf::new();
    fake.decls.lock().unwrap().push(measures());
    let mut app = ShelfApp::new(fake);
    block_on(app.new_doc(with_items(
        request("measure", None, &[]),
        &[("tags", "one, with a comma")],
    )))
    .unwrap();
    let readings = app.ports().readings.lock().unwrap().clone();
    let crate::domain::shelf::build::DocReading::Parsed { entries, .. } = &readings[0] else {
        panic!("parsed");
    };
    assert_eq!(
        entries["tags"],
        RawValue::List(vec!["one, with a comma".to_owned()])
    );
}

#[test]
fn an_item_for_a_link_is_a_key_trimmed_and_an_empty_item_is_an_item_blank() {
    let fake = FakeShelf::new().with_files(vec![fr(1, "a finding", &[])]);
    let mut app = ShelfApp::new(fake);
    block_on(app.new_doc(with_items(
        request("spec", None, &[("title", "a spec")]),
        &[("grounds", "  FR-1 ")],
    )))
    .unwrap();
    let readings = app.ports().readings.lock().unwrap().clone();
    let crate::domain::shelf::build::DocReading::Parsed { entries, .. } = readings.last().unwrap()
    else {
        panic!("parsed");
    };
    assert_eq!(entries["grounds"], RawValue::List(vec!["FR-1".to_owned()]));

    let error = block_on(app.new_doc(with_items(
        request("spec", None, &[("title", "another")]),
        &[("grounds", "   ")],
    )))
    .unwrap_err();
    // The app does not judge an item of nothing: the domain tells it as the item of the
    // document it is, which `check` tells too.
    let ShelfError::NewDoc(NewDocProblem::Invalid(problems)) = &error else {
        panic!("{error:?}");
    };
    assert_eq!(
        problems.iter().collect::<Vec<_>>(),
        [&DocProblem::ItemBlank {
            entry: crate::domain::shelf::schema::EntryName::new("grounds").unwrap(),
            at: 1,
        }]
    );
    assert_eq!(error.class(), ErrorClass::Request);
}

#[test]
fn an_item_of_nothing_for_a_list_field_is_an_item_blank_after_the_items_before_it() {
    let fake = FakeShelf::new();
    fake.decls.lock().unwrap().push(measures());
    let mut app = ShelfApp::new(fake);
    let error = block_on(app.new_doc(with_items(
        request("measure", None, &[("tags", "a, b")]),
        &[("tags", "")],
    )))
    .unwrap_err();
    let ShelfError::NewDoc(NewDocProblem::Invalid(problems)) = &error else {
        panic!("{error:?}");
    };
    assert_eq!(
        problems.iter().collect::<Vec<_>>(),
        [&DocProblem::ItemBlank {
            entry: crate::domain::shelf::schema::EntryName::new("tags").unwrap(),
            at: 3,
        }]
    );
    assert_eq!(error.class(), ErrorClass::Request);
    assert_eq!(app.ports().created_count(), 0);
}

#[test]
fn a_field_given_with_nothing_is_not_written() {
    let fake = FakeShelf::new();
    fake.decls.lock().unwrap().push(measures());
    let mut app = ShelfApp::new(fake);
    block_on(app.new_doc(request(
        "measure",
        None,
        &[("note", ""), ("count", "  "), ("tags", "")],
    )))
    .unwrap();
    let readings = app.ports().readings.lock().unwrap().clone();
    let crate::domain::shelf::build::DocReading::Parsed { entries, .. } = &readings[0] else {
        panic!("parsed");
    };
    for left_out in ["note", "count", "tags"] {
        assert!(!entries.contains_key(left_out), "{left_out}: {entries:?}");
    }
}

#[test]
fn a_null_entry_is_given_to_the_domain_and_not_written() {
    let fake = FakeShelf::new();
    fake.decls.lock().unwrap().push(measures());
    let mut app = ShelfApp::new(fake);
    let mut req = request("measure", None, &[]);
    req.entries.push(("note".to_owned(), EntryInput::Null));
    block_on(app.new_doc(req)).unwrap();
    let readings = app.ports().readings.lock().unwrap().clone();
    let crate::domain::shelf::build::DocReading::Parsed { entries, .. } = &readings[0] else {
        panic!("parsed");
    };
    assert!(!entries.contains_key("note"), "{entries:?}");
}

#[test]
fn an_item_for_a_field_that_is_no_list_is_a_request_error_and_writes_nothing() {
    let fake = FakeShelf::new();
    fake.decls.lock().unwrap().push(measures());
    let mut app = ShelfApp::new(fake);
    let error = block_on(app.new_doc(with_items(
        request("measure", None, &[]),
        &[("note", "a, b")],
    )))
    .unwrap_err();
    assert!(matches!(&error, ShelfError::ItemOfNoList { name } if name == "note"));
    assert_eq!(error.class(), ErrorClass::Request);
    assert_eq!(app.ports().created_count(), 0);
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
        scope: Default::default(),
        key: KeyPatternDecl::Prefixed {
            prefix: "M".to_owned(),
        },
        description: None,
        fields: vec![
            field("count", FieldTypeDecl::Word("integer".to_owned())),
            field("flag", FieldTypeDecl::Word("bool".to_owned())),
            field("tags", FieldTypeDecl::Word("list".to_owned())),
            field("note", FieldTypeDecl::Word("text".to_owned())),
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

/// A fake in which a file is at each of the first `k` numbered paths of `dir` (`FR-1.md`
/// to `FR-<k>.md`) when the write is made, and not when the shelf is read: the work of
/// runs beside this one.
fn with_files_appearing(dir: &str, prefix: &str, k: usize) -> FakeShelf {
    let fake = FakeShelf::new();
    for n in 1..=k {
        fake.appear_before_create
            .lock()
            .unwrap()
            .insert(path(&format!("{dir}/{prefix}-{n}.md")));
    }
    fake
}

#[test]
fn a_chosen_key_whose_file_appears_before_the_write_is_followed_by_the_next_one() {
    for k in [1, 3, 10] {
        let fake = with_files_appearing("findings", "FR", k);
        let ids_before = *fake.next_id.lock().unwrap();
        let mut app = ShelfApp::new(fake);
        let created = block_on(app.new_doc(request("finding", None, &[("title", "t")]))).unwrap();
        assert_eq!(created.key.to_string(), format!("finding:FR-{}", k + 1));
        assert_eq!(created.file.as_str(), format!("findings/FR-{}.md", k + 1));
        assert_eq!(app.ports().created_count(), 1);
        // One identity, taken once, whatever the number of tries.
        assert_eq!(*app.ports().next_id.lock().unwrap(), ids_before + 1);
        // The file holds the key it was written at.
        let readings = app.ports().readings.lock().unwrap().clone();
        let DocReading::Parsed { entries, .. } = readings.last().unwrap() else {
            panic!("parsed");
        };
        assert_eq!(entries["key"], RawValue::Text(format!("FR-{}", k + 1)));
    }
}

#[test]
fn a_chosen_key_chosen_again_asks_the_artifact_source_once() {
    let fake = with_files_appearing("specs", "SP", 2);
    let mut app = ShelfApp::new(fake);
    let created = block_on(app.new_doc(request(
        "spec",
        None,
        &[("title", "cache"), ("tests", "tests/a.rs")],
    )))
    .unwrap();
    assert_eq!(created.key.to_string(), "spec:SP-3");
    assert_eq!(app.ports().artifact_read_count(), 1);
    assert_eq!(app.ports().asked().len(), 1);
}

#[test]
fn a_given_key_whose_file_appears_before_the_write_is_refused_and_not_moved() {
    let fake = with_files_appearing("findings", "FR", 2);
    let mut app = ShelfApp::new(fake);
    let error =
        block_on(app.new_doc(request("finding", Some("FR-1"), &[("title", "t")]))).unwrap_err();
    let ShelfError::FileTaken { path } = &error else {
        panic!("{error:?}");
    };
    assert_eq!(path.as_str(), "findings/FR-1.md");
    assert_eq!(
        error.to_string(),
        "findings/FR-1.md is already there; nothing was written"
    );
    assert_eq!(error.class(), ErrorClass::Request);
    assert_eq!(app.ports().created_count(), 0);
}

#[test]
fn after_sixty_four_keys_with_a_file_each_nothing_is_written() {
    let fake = with_files_appearing("findings", "FR", 64);
    let mut app = ShelfApp::new(fake);
    let error = block_on(app.new_doc(request("finding", None, &[("title", "t")]))).unwrap_err();
    let ShelfError::NoFreeKey(problem) = &error else {
        panic!("{error:?}");
    };
    assert_eq!(problem.first.to_string(), "finding:FR-1");
    assert_eq!(problem.last.to_string(), "finding:FR-64");
    assert_eq!(
        error.to_string(),
        "the book `finding` had a file at every key from `FR-1` to `FR-64`; nothing was written: \
         other runs may be writing at the same time, or files that are no valid document are in \
         the way (`check` lists them); run `new` again once they are done or put right"
    );
    assert_eq!(error.class(), ErrorClass::Request);
    assert_eq!(app.ports().created_count(), 0);

    // One file fewer, and the 64th key is written.
    let fake = with_files_appearing("findings", "FR", 63);
    let mut app = ShelfApp::new(fake);
    let created = block_on(app.new_doc(request("finding", None, &[("title", "t")]))).unwrap();
    assert_eq!(created.key.to_string(), "finding:FR-64");
}

#[test]
fn a_key_chosen_again_that_has_no_next_is_the_error_of_the_next_key() {
    let fake = FakeShelf::new();
    fake.decls.lock().unwrap().push(BookSchemaDecl {
        scope: ScopeDecl {
            include: Some(vec!["M-1.md".to_owned(), "M-2.md".to_owned()]),
            exclude: None,
            without_front_matter: None,
        },
        ..measures()
    });
    for n in 1..=2 {
        fake.appear_before_create
            .lock()
            .unwrap()
            .insert(path(&format!("measures/M-{n}.md")));
    }
    let mut app = ShelfApp::new(fake);
    let error = block_on(app.new_doc(request("measure", None, &[]))).unwrap_err();
    assert!(matches!(
        error,
        ShelfError::NextKey(NextKeyProblem::OutOfRange(_))
    ));
    assert_eq!(error.class(), ErrorClass::Request);
    assert_eq!(app.ports().created_count(), 0);
}

#[test]
fn a_file_whose_front_matter_cannot_be_read_does_not_stop_a_new_document() {
    // `findings/FR-1.md` shows no key, so the shelf offers FR-1; the write finds the
    // file there, and the next key is written.
    let unreadable = DocReading::Unparsed {
        origin: path("findings/FR-1.md"),
        id: ReadId::Unreadable,
        why: RawProblem::NoFrontMatter,
    };
    let mut app = ShelfApp::new(FakeShelf::new().with_files(vec![unreadable]));
    let created = block_on(app.new_doc(request("finding", None, &[("title", "t")]))).unwrap();
    assert_eq!(created.key.to_string(), "finding:FR-2");
    assert_eq!(app.ports().created_count(), 1);
}

/// The book `measure`, with a range that takes only `notes/**` below its directory.
fn measures_in_notes() -> BookSchemaDecl {
    BookSchemaDecl {
        scope: ScopeDecl {
            include: Some(vec!["notes/**".to_owned()]),
            exclude: None,
            without_front_matter: None,
        },
        ..measures()
    }
}

#[test]
fn a_new_document_outside_the_range_is_a_request_error_that_writes_nothing() {
    for key in [None, Some("M-3")] {
        let fake = FakeShelf::new();
        fake.decls.lock().unwrap().push(measures_in_notes());
        let ids_before = *fake.next_id.lock().unwrap();
        let mut app = ShelfApp::new(fake);
        let error = block_on(app.new_doc(request("measure", key, &[]))).unwrap_err();
        let (ShelfError::NextKey(NextKeyProblem::OutOfRange(out))
        | ShelfError::ClaimKey(ClaimKeyProblem::OutOfRange(out))) = &error
        else {
            panic!("{key:?}: {error:?}");
        };
        let said = out.to_string();
        assert!(said.contains("include `notes/**`"), "{said}");
        assert_eq!(error.class(), ErrorClass::Request);
        assert_eq!(app.ports().created_count(), 0);
        // The identity is taken before the plan, because a key is decided in it; a
        // run that is refused shows none (nothing was written).
        assert_eq!(*app.ports().next_id.lock().unwrap(), ids_before + 1);
    }
}
