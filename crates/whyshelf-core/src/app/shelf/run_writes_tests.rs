//! Tests of the run of writes `new` and `init` make their files in: when the files
//! are put on disk, and what is told when that or a write fails, over the in-memory
//! fake, which counts each call to put files on disk with its files.

use crate::domain::shelf::schema::{BookName, ShelfDecl};

use super::fake_tests::{FakeShelf, block_on, decls, fr, path};
use super::{
    DeclarationText, EntryInput, ErrorClass, InitFile, InitRequest, NewDocRequest, ShelfApp,
    ShelfError,
};

/// A finding of the fake's books, with a title: `key` as asked for, or the next.
fn finding(n: usize, key: Option<&str>) -> NewDocRequest {
    NewDocRequest {
        row: n,
        book: BookName::new("finding").unwrap(),
        key: key.map(str::to_owned),
        name: None,
        entries: vec![("title".to_owned(), EntryInput::Text(format!("finding {n}")))],
        items: Vec::new(),
        body: "Body.\n".to_owned(),
    }
}

/// The text the fake reads as the two books of [`decls`].
const TEXT: &str = "the two books";

/// An empty project whose declaration text reads as the two books.
fn fake() -> FakeShelf {
    let fake = FakeShelf::new();
    fake.read_text_as(TEXT, ShelfDecl::of_books(decls()));
    fake
}

/// The declaration, a table and two findings.
fn init_request() -> InitRequest {
    InitRequest {
        declaration: DeclarationText::new(TEXT),
        files: vec![InitFile {
            path: path("data/table.csv"),
            bytes: b"a,b\n".to_vec(),
        }],
        docs: vec![finding(1, Some("FR-1")), finding(2, Some("FR-2"))],
    }
}

fn all_four() -> Vec<crate::domain::location::RelPath> {
    vec![
        path(".whyshelf/books.toml"),
        path("data/table.csv"),
        path("findings/FR-1.md"),
        path("findings/FR-2.md"),
    ]
}

#[test]
fn new_docs_puts_every_file_on_disk_once_after_the_last_write() {
    let mut app = ShelfApp::new(FakeShelf::new());
    let many =
        block_on(app.new_docs(vec![finding(1, None), finding(2, None), finding(3, None)])).unwrap();
    assert_eq!(many.docs.len(), 3);
    assert_eq!(app.ports().created_count(), 3);
    assert_eq!(
        app.ports().persist_calls(),
        [vec![
            path("findings/FR-1.md"),
            path("findings/FR-2.md"),
            path("findings/FR-3.md")
        ]]
    );
}

#[test]
fn new_doc_puts_its_one_file_on_disk() {
    let mut app = ShelfApp::new(FakeShelf::new());
    let created = block_on(app.new_doc(finding(1, None))).unwrap();
    assert_eq!(created.file, path("findings/FR-1.md"));
    assert_eq!(
        app.ports().persist_calls(),
        [vec![path("findings/FR-1.md")]]
    );
}

#[test]
fn a_file_replanned_to_another_key_is_put_on_disk_and_the_file_that_was_taken_is_not() {
    let fake = FakeShelf::new();
    fake.appear_before_create
        .lock()
        .unwrap()
        .insert(path("findings/FR-1.md"));
    let mut app = ShelfApp::new(fake);
    let many = block_on(app.new_docs(vec![finding(1, None), finding(2, None)])).unwrap();
    // The taken key's file is the other run's, not this one's: it is no part of the
    // run, and the files that are are put on disk in the order they were written.
    let written: Vec<_> = many.docs.iter().map(|d| d.file.clone()).collect();
    assert_eq!(written.len(), 2);
    assert!(!written.contains(&path("findings/FR-1.md")));
    assert_eq!(app.ports().persist_calls(), [written]);
}

#[test]
fn a_run_that_writes_nothing_puts_nothing_on_disk() {
    // Refused before it writes: a key asked for that is taken.
    let fake = FakeShelf::new().with_files(vec![fr(9, "somebody's", &[])]);
    let mut app = ShelfApp::new(fake);
    let error = block_on(app.new_docs(vec![finding(1, Some("FR-9"))])).unwrap_err();
    assert!(!matches!(error, ShelfError::PartWay { .. }), "{error:?}");
    assert!(app.ports().persist_calls().is_empty());

    // Stopped at the first write: the file is taken, and a key that was asked for is
    // not chosen again.
    let fake = FakeShelf::new();
    fake.appear_before_create
        .lock()
        .unwrap()
        .insert(path("findings/FR-9.md"));
    let mut app = ShelfApp::new(fake);
    let error = block_on(app.new_docs(vec![finding(1, Some("FR-9"))])).unwrap_err();
    assert!(matches!(error, ShelfError::FileTaken { .. }), "{error:?}");
    assert!(app.ports().persist_calls().is_empty());

    // A run with no row to write: it answers, and has nothing to put on disk.
    let mut app = ShelfApp::new(FakeShelf::new());
    assert!(block_on(app.new_docs(Vec::new())).unwrap().docs.is_empty());
    assert!(app.ports().persist_calls().is_empty());
}

#[test]
fn a_run_that_stops_part_way_and_cannot_put_what_it_wrote_on_disk_says_both() {
    let fake = FakeShelf::new();
    *fake.creates_before_failing.lock().unwrap() = Some(1);
    fake.fail_persist("the drive went away");
    let mut app = ShelfApp::new(fake);
    let error = block_on(app.new_docs(vec![finding(1, None), finding(2, None)])).unwrap_err();
    let ShelfError::PartWay {
        written,
        failed,
        cause,
        not_durable,
        undo_failed: false,
    } = &error
    else {
        panic!("{error:?}");
    };
    assert_eq!(written.clone().into_vec(), [path("findings/FR-1.md")]);
    assert_eq!(*failed, path("findings/FR-2.md"));
    assert!(matches!(**cause, ShelfError::Port { .. }));
    let Some(problem) = not_durable else {
        panic!("{error:?}");
    };
    assert_eq!(problem.to_string(), "the file sync port failed");
    assert_eq!(problem.reason(), "fake failure: the drive went away");
    assert_eq!(
        error.to_string(),
        "findings/FR-1.md was written, then findings/FR-2.md was not; putting what was written on disk failed too: fake failure: the drive went away"
    );
    assert_eq!(error.class(), ErrorClass::Failed);
    // It was asked once, for the one file that was written.
    assert_eq!(
        app.ports().persist_calls(),
        [vec![path("findings/FR-1.md")]]
    );

    // What was written could not be put on disk, so the run failed, even where what
    // stopped it is a request's trouble (a file that was taken).
    let fake = FakeShelf::new();
    fake.appear_before_create
        .lock()
        .unwrap()
        .insert(path("findings/FR-9.md"));
    fake.fail_persist("the drive went away");
    let mut app = ShelfApp::new(fake);
    let error =
        block_on(app.new_docs(vec![finding(1, None), finding(2, Some("FR-9"))])).unwrap_err();
    assert!(
        matches!(
            &error,
            ShelfError::PartWay {
                not_durable: Some(_),
                ..
            }
        ),
        "{error:?}"
    );
    assert_eq!(error.class(), ErrorClass::Failed);
}

#[test]
fn files_all_written_and_not_put_on_disk_are_not_durable_and_are_kept_and_named() {
    let fake = FakeShelf::new();
    fake.fail_persist("the drive went away");
    let mut app = ShelfApp::new(fake);
    let error = block_on(app.new_docs(vec![finding(1, None), finding(2, None)])).unwrap_err();
    let ShelfError::NotDurable { files, cause } = &error else {
        panic!("{error:?}");
    };
    assert_eq!(
        files.clone().into_vec(),
        [path("findings/FR-1.md"), path("findings/FR-2.md")]
    );
    assert_eq!(cause.reason(), "fake failure: the drive went away");
    assert_eq!(
        error.to_string(),
        "findings/FR-1.md and findings/FR-2.md were written whole and left in place, but could not be put on disk: a power loss may lose what was written"
    );
    assert_eq!(error.class(), ErrorClass::Failed);
    // The files are not removed.
    assert_eq!(app.ports().created_count(), 2);
    assert_eq!(app.ports().persist_calls().len(), 1);
}

#[test]
fn init_puts_the_declaration_the_file_and_the_documents_on_disk_once() {
    let mut app = ShelfApp::new(fake());
    let done = block_on(app.init(init_request())).unwrap();
    assert_eq!(done.docs.len(), 2);
    assert_eq!(app.ports().persist_calls(), [all_four()]);
}

#[test]
fn init_that_cannot_put_what_it_wrote_on_disk_is_not_durable_and_keeps_everything() {
    let fake = fake();
    fake.fail_persist("the drive went away");
    let mut app = ShelfApp::new(fake);
    let error = block_on(app.init(init_request())).unwrap_err();
    let ShelfError::NotDurable { files, .. } = &error else {
        panic!("{error:?}");
    };
    assert_eq!(files.clone().into_vec(), all_four());
    assert_eq!(error.class(), ErrorClass::Failed);
    assert_eq!(app.ports().declaration_file().as_deref(), Some(TEXT));
    assert_eq!(app.ports().created_count(), 2);
}

#[test]
fn init_that_stops_part_way_and_cannot_put_what_it_wrote_on_disk_says_both() {
    let fake = fake();
    // The declaration, the table and FR-1 are written; FR-2 fails.
    *fake.creates_before_failing.lock().unwrap() = Some(1);
    fake.fail_persist("the drive went away");
    let mut app = ShelfApp::new(fake);
    let error = block_on(app.init(init_request())).unwrap_err();
    let ShelfError::PartWay {
        written,
        failed,
        not_durable,
        ..
    } = &error
    else {
        panic!("{error:?}");
    };
    assert_eq!(
        written.clone().into_vec(),
        [
            path(".whyshelf/books.toml"),
            path("data/table.csv"),
            path("findings/FR-1.md")
        ]
    );
    assert_eq!(*failed, path("findings/FR-2.md"));
    assert!(not_durable.is_some());
    assert_eq!(app.ports().persist_calls().len(), 1);
}

#[test]
fn init_that_stops_at_a_file_in_the_way_puts_the_declaration_on_disk() {
    let fake = fake();
    // Not there when looked for, there when written.
    fake.appear_before_create
        .lock()
        .unwrap()
        .insert(path("data/table.csv"));
    let mut app = ShelfApp::new(fake);
    let error = block_on(app.init(init_request())).unwrap_err();
    assert!(
        matches!(
            &error,
            ShelfError::PartWay {
                not_durable: None,
                ..
            }
        ),
        "{error:?}"
    );
    assert_eq!(
        app.ports().persist_calls(),
        [vec![path(".whyshelf/books.toml")]]
    );
}

#[test]
fn init_that_is_refused_before_it_writes_puts_nothing_on_disk() {
    let fake = fake();
    *fake.declaration_file.lock().unwrap() = Some("what was there".to_owned());
    let mut app = ShelfApp::new(fake);
    let error = block_on(app.init(init_request())).unwrap_err();
    assert!(matches!(error, ShelfError::InTheWay { .. }), "{error:?}");
    assert!(app.ports().persist_calls().is_empty());
}

/// The futures of the use cases that write are `Send` when the ports are: nothing in
/// the run makes them otherwise, so a caller may spawn one on a runtime that moves
/// its tasks.
#[test]
fn the_futures_of_the_use_cases_that_write_are_send() {
    fn is_send<T: Send>(_: &T) {}
    let mut app = ShelfApp::new(fake());
    let future = app.new_docs(vec![finding(1, None)]);
    is_send(&future);
    drop(future);
    let future = app.init(init_request());
    is_send(&future);
}

/// A create port, and the ports that put a file back, remove one or put a removal on
/// disk, are called in `run_writes.rs` and nowhere else outside the tests and the
/// ports' own page: the one way left to write a file the run does not know. (`fmt`
/// calls `replace_read` itself, one file at a time.) A module added to the use cases is
/// added to this list.
#[test]
fn no_module_but_run_writes_calls_a_create_port() {
    let modules = [
        ("approve_chosen", include_str!("approve_chosen.rs")),
        ("approved", include_str!("approved.rs")),
        ("error", include_str!("error.rs")),
        ("exempt", include_str!("exempt.rs")),
        ("fmt", include_str!("fmt.rs")),
        ("init", include_str!("init.rs")),
        ("judge", include_str!("judge.rs")),
        ("load", include_str!("load.rs")),
        ("mod", include_str!("mod.rs")),
        ("moved", include_str!("moved.rs")),
        ("moves", include_str!("moves.rs")),
        ("new_doc", include_str!("new_doc.rs")),
        ("read", include_str!("read.rs")),
        ("rekey", include_str!("rekey.rs")),
        ("report", include_str!("report.rs")),
        ("same_meaning", include_str!("same_meaning.rs")),
        ("unlinked", include_str!("unlinked.rs")),
        ("withdraw", include_str!("withdraw.rs")),
    ];
    let calls = [
        ("doc_writer()", ".create("),
        ("declaration_writer()", ".create("),
        ("file_writer()", ".create("),
        ("doc_replacer()", ".restore("),
        ("doc_remover()", ".remove_read("),
        ("file_syncer()", ".persist_gone("),
    ];
    for (name, source) in modules {
        for (writer, method) in calls {
            let call = format!("{writer}{method}");
            let multi_line = format!("{writer}\n");
            assert!(
                !source.contains(&call),
                "{name}.rs calls a write port ({call}): write inside RunWrites"
            );
            // A call split over lines puts the method on the line after the accessor.
            for (at, _) in source.match_indices(&multi_line) {
                let rest = source[at + multi_line.len()..].trim_start();
                assert!(
                    !rest.starts_with(method),
                    "{name}.rs calls a write port ({writer}{method}): write inside RunWrites"
                );
            }
        }
    }
    let run_writes = include_str!("run_writes.rs");
    // `create`: the declaration, another file, a document, and the document of an edit.
    assert_eq!(run_writes.matches(".create(").count(), 4);
    assert_eq!(run_writes.matches(".restore(").count(), 1);
    // The removal of an edit, and the removal that undoes its new file.
    assert_eq!(run_writes.matches(".remove_read(").count(), 2);
    assert_eq!(run_writes.matches(".persist_gone(").count(), 2);
}
