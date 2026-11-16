//! Tests of `new_docs`, a run of new documents, over the in-memory fake.

use crate::domain::shelf::build::{DocReading, PlanProblem, RawValue};
use crate::domain::shelf::schema::{
    BookName, BookSchemaDecl, FieldEntryDecl, FieldPresence, FieldTypeDecl, KeyPatternDecl,
};

use super::fake_tests::{FakeShelf, block_on, fr, path};
use super::{EntryInput, ErrorClass, ItemInput, NewDocRequest, ShelfApp, ShelfError};

fn text(value: &str) -> EntryInput {
    EntryInput::Text(value.to_owned())
}

/// The names of the links of a row: keys and names (`@name`).
fn items(items: &[&str]) -> EntryInput {
    EntryInput::Items(
        items
            .iter()
            .map(|item| match item.strip_prefix('@') {
                Some(named) => ItemInput::Named(named.to_owned()),
                None => ItemInput::Text((*item).to_owned()),
            })
            .collect(),
    )
}

/// A row, numbered `row`, of `book`.
fn row(
    row: usize,
    book: &str,
    key: Option<&str>,
    name: Option<&str>,
    entries: Vec<(&str, EntryInput)>,
) -> NewDocRequest {
    NewDocRequest {
        row,
        book: BookName::new(book).unwrap(),
        key: key.map(str::to_owned),
        name: name.map(str::to_owned),
        entries: entries
            .into_iter()
            .map(|(entry, input)| (entry.to_owned(), input))
            .collect(),
        items: Vec::new(),
        body: "Body.\n".to_owned(),
    }
}

/// A finding of the fake's books, with a title.
fn finding(n: usize, key: Option<&str>, name: Option<&str>, links: &[&str]) -> NewDocRequest {
    let mut entries = vec![("title", text(&format!("finding {n}")))];
    if !links.is_empty() {
        entries.push(("base", items(links)));
    }
    row(n, "finding", key, name, entries)
}

/// A spec of the fake's books, with a title.
fn spec(n: usize, key: Option<&str>, name: Option<&str>, grounds: &[&str]) -> NewDocRequest {
    let mut entries = vec![("title", text(&format!("spec {n}")))];
    if !grounds.is_empty() {
        entries.push(("grounds", items(grounds)));
    }
    row(n, "spec", key, name, entries)
}

#[test]
fn a_typed_item_is_trimmed_and_an_empty_one_is_refused() {
    let mut app = ShelfApp::new(FakeShelf::new().with_files(vec![fr(1, "slow", &[])]));
    let many = block_on(app.new_docs(vec![spec(1, None, None, &[" FR-1 "])])).unwrap();
    assert!(many.docs[0].unresolved.is_empty(), "{:?}", many.docs);
    let error = block_on(app.new_docs(vec![spec(2, None, None, &["FR-1", "  "])])).unwrap_err();
    let ShelfError::InRow { row: 2, cause } = &error else {
        panic!("{error:?}");
    };
    assert!(matches!(**cause, ShelfError::EmptyItem { .. }), "{error:?}");
}

fn keys_of(many: &super::NewMany) -> Vec<String> {
    many.docs.iter().map(|d| d.key.to_string()).collect()
}

/// The entries of the file written at `at`.
fn written_entries(
    app: &ShelfApp<FakeShelf>,
    at: &str,
) -> std::collections::BTreeMap<String, RawValue> {
    let readings = app.ports().readings.lock().unwrap().clone();
    readings
        .into_iter()
        .find_map(|reading| match reading {
            DocReading::Parsed {
                origin, entries, ..
            } if origin == path(at) => Some(entries),
            _ => None,
        })
        .unwrap_or_else(|| panic!("{at} was written"))
}

#[test]
fn twenty_five_rows_read_the_shelf_and_ask_the_artifacts_once_and_are_written_in_order() {
    let mut app = ShelfApp::new(FakeShelf::new().with_files(vec![fr(1, "slow", &[])]));
    let rows: Vec<NewDocRequest> = (1..=25)
        .map(|n| {
            row(
                n,
                "spec",
                None,
                None,
                vec![
                    ("title", text(&format!("spec {n}"))),
                    ("grounds", text("FR-1")),
                    ("tests", text(&format!("tests/t{n}.rs, tests/shared.rs"))),
                ],
            )
        })
        .collect();
    let ids_before = *app.ports().next_id.lock().unwrap();
    let many = block_on(app.new_docs(rows)).unwrap();
    assert_eq!(many.docs.len(), 25);
    assert_eq!(app.ports().created_count(), 25);
    // The shelf was read once, and the adapter of the artifacts asked once for every
    // name of every row (a name shared by rows once).
    assert_eq!(app.ports().doc_read_count(), 1);
    assert_eq!(app.ports().artifact_read_count(), 1);
    let asked = app.ports().asked();
    assert_eq!(asked.len(), 1);
    assert_eq!(asked[0].iter().count(), 26);
    // One identity each.
    assert_eq!(*app.ports().next_id.lock().unwrap(), ids_before + 25);
    let ids: std::collections::BTreeSet<_> = many.docs.iter().map(|d| d.id).collect();
    assert_eq!(ids.len(), 25);
    // Keys run on from the first free number, in the order of the rows.
    let expected: Vec<String> = (1..=25).map(|n| format!("spec:SP-{n}")).collect();
    assert_eq!(keys_of(&many), expected);
    // Every spec is a head: nothing of the run stands on another of them.
    assert_eq!(many.heads.len(), 25);
    // The shelf reads back clean.
    let check = block_on(app.books_check()).unwrap();
    assert!(check.is_clean(), "{check:?}");
}

#[test]
fn one_row_refused_writes_nothing_and_tells_every_row_refused_with_its_number() {
    let mut app = ShelfApp::new(FakeShelf::new());
    let mut rows: Vec<NewDocRequest> = (1..=25).map(|n| finding(n, None, None, &[])).collect();
    // Row 13 has no title, which its book requires; row 20 is a key that is no key.
    rows[12].entries.retain(|(name, _)| name != "title");
    rows[19].key = Some("not a key".to_owned());
    let error = block_on(app.new_docs(rows)).unwrap_err();
    let ShelfError::Plan(problems) = &error else {
        panic!("{error:?}");
    };
    let told: Vec<String> = problems.iter().map(|p| p.to_string()).collect();
    assert_eq!(told.len(), 2, "{told:?}");
    assert!(told[0].starts_with("row 13: the new document would be invalid: "));
    assert!(told[1].starts_with("row 20: "), "{told:?}");
    assert_eq!(error.class(), ErrorClass::Request);
    assert_eq!(app.ports().created_count(), 0);
}

#[test]
fn a_name_that_stands_for_no_row_refuses_the_run() {
    let mut app = ShelfApp::new(FakeShelf::new());
    let error = block_on(app.new_docs(vec![
        finding(1, None, Some("a"), &[]),
        spec(2, None, None, &["@b"]),
    ]))
    .unwrap_err();
    let ShelfError::Plan(problems) = &error else {
        panic!("{error:?}");
    };
    assert!(matches!(
        problems.first(),
        PlanProblem::UnknownName { row: 2, .. }
    ));
    assert_eq!(app.ports().created_count(), 0);
}

#[test]
fn names_are_written_as_keys_and_the_row_named_is_written_first() {
    let mut app = ShelfApp::new(FakeShelf::new());
    let many = block_on(app.new_docs(vec![
        spec(1, None, None, &["@f", "FR-77"]),
        finding(2, None, Some("f"), &[]),
    ]))
    .unwrap();
    // The finding is written first, so its file is there when the spec's is.
    assert_eq!(keys_of(&many), ["finding:FR-1", "spec:SP-1"]);
    assert_eq!(
        written_entries(&app, "specs/SP-1.md")["grounds"],
        RawValue::List(vec!["FR-1".to_owned(), "FR-77".to_owned()])
    );
    assert!(!written_entries(&app, "specs/SP-1.md").contains_key("as"));
    // The link to the row is resolved; the one to a key nothing answers to is told.
    let unresolved: Vec<_> = many.docs[1]
        .unresolved
        .iter()
        .map(|(link, u)| (link.to_string(), u.written().to_owned()))
        .collect();
    assert_eq!(unresolved, [("grounds".to_owned(), "FR-77".to_owned())]);
    assert!(many.docs[0].unresolved.is_empty());
    // Only the spec stands on nothing.
    let heads: Vec<String> = many.heads.iter().map(ToString::to_string).collect();
    assert_eq!(heads, ["spec:SP-1"]);
    let check = block_on(app.books_check()).unwrap();
    assert!(check.is_clean(), "{check:?}");
}

#[test]
fn a_file_that_appears_mid_run_moves_only_the_row_it_was_taken_from() {
    let fake = FakeShelf::new();
    fake.appear_before_create
        .lock()
        .unwrap()
        .insert(path("findings/FR-2.md"));
    let ids_before = *fake.next_id.lock().unwrap();
    let mut app = ShelfApp::new(fake);
    let many = block_on(app.new_docs(vec![
        finding(1, None, None, &[]),
        finding(2, None, Some("y"), &[]),
        finding(3, None, None, &["@y"]),
    ]))
    .unwrap();
    // FR-2 was taken: that row is FR-4, the number after the highest of the run; the
    // others keep theirs, and the row that names it follows.
    assert_eq!(
        keys_of(&many),
        ["finding:FR-1", "finding:FR-4", "finding:FR-3"]
    );
    assert_eq!(
        written_entries(&app, "findings/FR-3.md")["base"],
        RawValue::List(vec!["FR-4".to_owned()])
    );
    assert!(many.docs.iter().all(|d| d.unresolved.is_empty()));
    assert_eq!(app.ports().created_count(), 3);
    // Neither the shelf nor the artifacts nor the identities were taken again.
    assert_eq!(app.ports().doc_read_count(), 1);
    assert_eq!(*app.ports().next_id.lock().unwrap(), ids_before + 3);
}

#[test]
fn a_ring_of_names_goes_on_when_the_first_written_is_taken_and_stops_when_the_second_is() {
    // The first is taken before anything is written: only it moves.
    let fake = FakeShelf::new();
    fake.appear_before_create
        .lock()
        .unwrap()
        .insert(path("findings/FR-1.md"));
    let mut app = ShelfApp::new(fake);
    let many = block_on(app.new_docs(vec![
        finding(1, None, Some("a"), &["@b"]),
        finding(2, None, Some("b"), &["@a"]),
    ]))
    .unwrap();
    assert_eq!(keys_of(&many), ["finding:FR-3", "finding:FR-2"]);
    assert!(many.docs.iter().all(|d| d.unresolved.is_empty()));

    // The second is taken after the first was written with its key in a link: the
    // file written says a key that cannot be put right.
    let fake = FakeShelf::new();
    fake.appear_before_create
        .lock()
        .unwrap()
        .insert(path("findings/FR-2.md"));
    let mut app = ShelfApp::new(fake);
    let error = block_on(app.new_docs(vec![
        finding(4, None, Some("a"), &["@b"]),
        finding(7, None, Some("b"), &["@a"]),
    ]))
    .unwrap_err();
    let ShelfError::PartWay {
        written,
        failed,
        cause,
    } = &error
    else {
        panic!("{error:?}");
    };
    assert_eq!(written.clone().into_vec(), [path("findings/FR-1.md")]);
    assert_eq!(*failed, path("findings/FR-2.md"));
    assert!(matches!(
        **cause,
        ShelfError::Stuck {
            row: 7,
            written_row: 4
        }
    ));
    assert_eq!(
        cause.to_string(),
        "the key of row 7 was taken while the run wrote, and row 4, already written, links to it by name"
    );
    assert_eq!(error.class(), ErrorClass::Request);
    assert_eq!(app.ports().created_count(), 1);
}

#[test]
fn a_key_asked_for_that_is_taken_after_two_were_written_is_part_way() {
    let fake = FakeShelf::new();
    fake.appear_before_create
        .lock()
        .unwrap()
        .insert(path("findings/FR-9.md"));
    let mut app = ShelfApp::new(fake);
    let error = block_on(app.new_docs(vec![
        finding(1, None, None, &[]),
        finding(2, None, None, &[]),
        finding(3, Some("FR-9"), None, &[]),
    ]))
    .unwrap_err();
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
        [path("findings/FR-1.md"), path("findings/FR-2.md")]
    );
    assert_eq!(*failed, path("findings/FR-9.md"));
    assert!(matches!(**cause, ShelfError::FileTaken { .. }));
    assert_eq!(error.class(), ErrorClass::Request);
    assert_eq!(
        error.to_string(),
        "findings/FR-1.md and findings/FR-2.md were written, then findings/FR-9.md was not"
    );
}

#[test]
fn a_file_at_every_key_tried_after_two_were_written_is_part_way() {
    let fake = FakeShelf::new();
    // FR-1 and FR-2 are the first two rows'; the third is at FR-3, and every key
    // from there to FR-66 (64 keys) has a file at the write.
    for n in 3..=66 {
        fake.appear_before_create
            .lock()
            .unwrap()
            .insert(path(&format!("findings/FR-{n}.md")));
    }
    let mut app = ShelfApp::new(fake);
    let error = block_on(app.new_docs(vec![
        finding(1, None, None, &[]),
        finding(2, None, None, &[]),
        finding(3, None, None, &[]),
    ]))
    .unwrap_err();
    let ShelfError::PartWay { written, cause, .. } = &error else {
        panic!("{error:?}");
    };
    assert_eq!(written.len(), 2);
    let ShelfError::NoFreeKey(problem) = &**cause else {
        panic!("{error:?}");
    };
    assert_eq!(problem.first.to_string(), "finding:FR-3");
    assert_eq!(problem.last.to_string(), "finding:FR-66");
    assert_eq!(error.class(), ErrorClass::Request);
    assert_eq!(app.ports().created_count(), 2);
}

#[test]
fn a_write_that_fails_after_the_first_is_part_way_with_what_was_written() {
    let fake = FakeShelf::new();
    *fake.creates_before_failing.lock().unwrap() = Some(1);
    let mut app = ShelfApp::new(fake);
    let error = block_on(app.new_docs(vec![
        finding(1, None, None, &[]),
        finding(2, None, None, &[]),
    ]))
    .unwrap_err();
    let ShelfError::PartWay {
        written,
        failed,
        cause,
    } = &error
    else {
        panic!("{error:?}");
    };
    assert_eq!(written.clone().into_vec(), [path("findings/FR-1.md")]);
    assert_eq!(*failed, path("findings/FR-2.md"));
    assert!(matches!(**cause, ShelfError::Port { .. }));
    assert_eq!(error.class(), ErrorClass::Failed);

    // A write that fails before any is the failure itself.
    let fake = FakeShelf::new();
    *fake.creates_before_failing.lock().unwrap() = Some(0);
    let mut app = ShelfApp::new(fake);
    let error = block_on(app.new_docs(vec![finding(1, None, None, &[])])).unwrap_err();
    assert!(matches!(error, ShelfError::Port { .. }), "{error:?}");
}

/// A book with a list field `tags` and a link-free shape.
fn measures() -> BookSchemaDecl {
    BookSchemaDecl {
        name: "measure".to_owned(),
        dir: "measures".to_owned(),
        scope: Default::default(),
        key: KeyPatternDecl::Prefixed {
            prefix: "M".to_owned(),
        },
        description: None,
        fields: vec![FieldEntryDecl {
            name: "tags".to_owned(),
            ty: FieldTypeDecl::Word("list".to_owned()),
            presence: FieldPresence::Optional,
        }],
        links: Vec::new(),
    }
}

#[test]
fn the_name_of_a_row_in_a_text_list_is_refused_where_it_is_given() {
    let fake = FakeShelf::new();
    fake.decls.lock().unwrap().push(measures());
    let mut app = ShelfApp::new(fake);
    let error = block_on(app.new_docs(vec![
        finding(1, None, Some("a"), &[]),
        finding(2, None, None, &[]),
        row(
            3,
            "measure",
            None,
            None,
            vec![("tags", items(&["x", "@a"]))],
        ),
    ]))
    .unwrap_err();
    assert!(
        matches!(&error, ShelfError::NameOutsideLink { row: 3, entry } if entry == "tags"),
        "{error:?}"
    );
    assert_eq!(
        error.to_string(),
        "row 3: `tags` is no link, so it cannot name a row"
    );
    assert_eq!(error.class(), ErrorClass::Request);
    assert_eq!(app.ports().created_count(), 0);

    // Its items are texts, as they are given.
    let many = block_on(app.new_docs(vec![row(
        1,
        "measure",
        None,
        None,
        vec![("tags", items(&["a, b", "c"]))],
    )]))
    .unwrap();
    assert_eq!(many.docs.len(), 1);
    assert_eq!(
        written_entries(&app, "measures/M-1.md")["tags"],
        RawValue::List(vec!["a, b".to_owned(), "c".to_owned()])
    );
}

#[test]
fn a_truth_value_and_a_whole_number_are_kept_as_they_are() {
    let fake = FakeShelf::new();
    let mut declared = measures();
    for (name, word) in [("count", "integer"), ("flag", "bool")] {
        declared.fields.push(FieldEntryDecl {
            name: name.to_owned(),
            ty: FieldTypeDecl::Word(word.to_owned()),
            presence: FieldPresence::Optional,
        });
    }
    fake.decls.lock().unwrap().push(declared);
    let mut app = ShelfApp::new(fake);
    block_on(app.new_docs(vec![row(
        1,
        "measure",
        None,
        None,
        vec![
            ("count", EntryInput::Integer(-3)),
            ("flag", EntryInput::Bool(true)),
        ],
    )]))
    .unwrap();
    let entries = written_entries(&app, "measures/M-1.md");
    assert_eq!(entries["count"], RawValue::Integer(-3));
    assert_eq!(entries["flag"], RawValue::Bool(true));
}

#[test]
fn an_entry_twice_in_a_row_is_refused_with_the_row() {
    let mut app = ShelfApp::new(FakeShelf::new());
    let error = block_on(app.new_docs(vec![
        finding(1, None, None, &[]),
        row(
            2,
            "finding",
            None,
            None,
            vec![("title", text("a")), ("title", text("b"))],
        ),
    ]))
    .unwrap_err();
    let ShelfError::InRow { row, cause } = &error else {
        panic!("{error:?}");
    };
    assert_eq!(*row, 2);
    assert!(matches!(**cause, ShelfError::EntryTwice { .. }));
    assert_eq!(error.to_string(), "row 2: the entry `title` is given twice");
    assert_eq!(error.class(), ErrorClass::Request);
    assert_eq!(app.ports().created_count(), 0);
}

#[test]
fn an_empty_name_is_refused_with_the_row() {
    let mut app = ShelfApp::new(FakeShelf::new());
    let error = block_on(app.new_docs(vec![finding(4, None, Some("  "), &[])])).unwrap_err();
    assert!(
        matches!(&error, ShelfError::InRow { row: 4, cause } if matches!(**cause, ShelfError::EmptyName)),
        "{error:?}"
    );
    assert_eq!(app.ports().created_count(), 0);
}

#[test]
fn every_row_with_a_fault_of_the_request_is_told_and_the_run_is_not_planned() {
    let fake = FakeShelf::new();
    fake.decls.lock().unwrap().push(measures());
    let mut app = ShelfApp::new(fake);
    let mut item_of_no_list = finding(5, None, None, &[]);
    item_of_no_list.items = vec![("title".to_owned(), "x".to_owned())];
    let ids_before = *app.ports().next_id.lock().unwrap();
    let error = block_on(app.new_docs(vec![
        finding(1, Some("FR-9"), None, &[]),
        // An entry twice, an empty name, an item for no list and a name where no
        // document is linked: four rows, four faults. Rows 1 and 3 are whole, but
        // ask for one key, which the plan would tell and a request with faults
        // waits for.
        row(
            2,
            "finding",
            None,
            None,
            vec![("title", text("a")), ("title", text("b"))],
        ),
        finding(3, Some("FR-9"), None, &[]),
        finding(4, None, Some("  "), &[]),
        item_of_no_list,
        row(
            6,
            "measure",
            None,
            None,
            vec![("tags", items(&["x", "@a"]))],
        ),
    ]))
    .unwrap_err();
    let ShelfError::Rows(faults) = &error else {
        panic!("{error:?}");
    };
    let rows: Vec<usize> = faults
        .iter()
        .map(|fault| match fault {
            ShelfError::InRow { row, .. } | ShelfError::NameOutsideLink { row, .. } => *row,
            other => panic!("{other:?}"),
        })
        .collect();
    assert_eq!(rows, [2, 4, 5, 6]);
    assert_eq!(
        error.to_string(),
        "row 2: the entry `title` is given twice; \
         row 4: the name of the document is empty; \
         row 5: `title` is no list field or link of the book, so it takes no item; \
         row 6: `tags` is no link, so it cannot name a row"
    );
    assert_eq!(error.class(), ErrorClass::Request);
    assert_eq!(app.ports().created_count(), 0);
    // Nothing was planned: no identity was taken for a run that was not.
    assert_eq!(*app.ports().next_id.lock().unwrap(), ids_before);
}

#[test]
fn an_entry_given_as_a_value_and_as_items_is_given_twice_and_neither_replaces_the_other() {
    let fake = FakeShelf::new();
    fake.decls.lock().unwrap().push(measures());
    let mut app = ShelfApp::new(fake);
    let mut request = row(
        3,
        "measure",
        None,
        None,
        vec![("tags", EntryInput::Bool(true))],
    );
    request.items = vec![("tags".to_owned(), "x".to_owned())];
    let error = block_on(app.new_docs(vec![request.clone()])).unwrap_err();
    assert!(
        matches!(&error, ShelfError::InRow { row: 3, cause } if matches!(&**cause, ShelfError::EntryTwice { name } if name == "tags")),
        "{error:?}"
    );
    assert_eq!(app.ports().created_count(), 0);
    // `new` of one tells it with no row, as it tells an entry given twice.
    let error = block_on(app.new_doc(request)).unwrap_err();
    assert!(
        matches!(&error, ShelfError::EntryTwice { name } if name == "tags"),
        "{error:?}"
    );
}

#[test]
fn one_row_is_told_as_new_always_told_it() {
    // `new_doc` is the run of one row: the refusals are the ones of before, with no row.
    let mut app = ShelfApp::new(FakeShelf::new());
    let error = block_on(app.new_doc(finding(1, Some("not a key"), None, &[]))).unwrap_err();
    assert!(matches!(error, ShelfError::ClaimKey(_)), "{error:?}");
    let error = block_on(app.new_doc(row(
        1,
        "finding",
        None,
        None,
        vec![("title", text("a")), ("title", text("b"))],
    )))
    .unwrap_err();
    assert!(matches!(error, ShelfError::EntryTwice { .. }), "{error:?}");
    // A name no row has is a problem of a run, told as one.
    let error = block_on(app.new_doc(finding(1, None, None, &["@a"]))).unwrap_err();
    assert!(matches!(error, ShelfError::Plan(_)), "{error:?}");
    assert_eq!(app.ports().created_count(), 0);
}

#[test]
fn an_empty_run_writes_nothing_and_reads_the_shelf() {
    let mut app = ShelfApp::new(FakeShelf::new());
    let many = block_on(app.new_docs(Vec::new())).unwrap();
    assert!(many.docs.is_empty());
    assert!(many.heads.is_empty());
    assert_eq!(app.ports().created_count(), 0);
}
