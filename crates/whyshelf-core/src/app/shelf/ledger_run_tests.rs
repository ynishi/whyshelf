//! Tests of the use cases that write records to the ledger (`judge`, `approve_chosen`,
//! `same_meaning`, `exempt`, `moved`, `moved_prefix`, `withdraw`) as runs of writes: every record is
//! written, then put on disk once, with the path of each record's file; a write that
//! fails part way is told as the records written; records all written and not put on
//! disk are not durable. Over the in-memory fake, which counts each call to put files
//! on disk with its files.

// The error is the use case's own, read once by each test.
#![allow(clippy::result_large_err)]

use crate::domain::canon::Canon;
use crate::domain::location::RelPath;
use crate::domain::nonempty::NonBlank;
use crate::domain::shelf::artifact::{NamePrefix, PrefixMove};
use crate::domain::shelf::build::DocReading;
use crate::domain::shelf::ledger::Record;
use crate::domain::shelf::lookup::Reference;
use crate::domain::shelf::schema::{BookName, CoverDecl, EntryName};

use super::fake_tests::{FakeShelf, aname, block_on, decls, file_space, fr, reason, sp, sp_tested};
use super::{
    ApproveChosenRequest, Choose, ErrorClass, ExemptRequest, JudgeRequest, Judgement, LedgerAppend,
    MovedRequest, PendingScope, PrefixMovedRequest, SameMeaningAbout, SameMeaningRequest, ShelfApp,
    ShelfError, ShelfPort, WithdrawRequest,
};

fn r(text: &str) -> Reference {
    text.parse().unwrap()
}

fn approve(reference: &str) -> JudgeRequest {
    JudgeRequest {
        judgement: Judgement::Approve {
            until: None,
            reason: None,
            again: false,
        },
        reference: r(reference),
        by: Default::default(),
    }
}

/// One use case that writes records: an app that is ready to run it, and the run.
struct Case {
    name: &'static str,
    /// The app, with what the run needs already written (and put on disk).
    make: fn() -> ShelfApp<FakeShelf>,
    run: fn(&mut ShelfApp<FakeShelf>) -> Result<(), ShelfError>,
    /// How many records the run writes.
    records: usize,
}

/// FR-1 and SP-2 on it, nothing judged.
fn pending() -> ShelfApp<FakeShelf> {
    ShelfApp::new(FakeShelf::new().with_files(vec![fr(1, "slow", &[]), sp(2, "cache", &["FR-1"])]))
}

/// FR-1 and SP-2 on it, both approved.
fn approved() -> ShelfApp<FakeShelf> {
    let mut app = pending();
    block_on(app.judge_all(vec![approve("FR-1"), approve("SP-2")])).unwrap();
    app
}

/// FR-1 and SP-2 on it, both approved, then both edited.
fn edited() -> ShelfApp<FakeShelf> {
    let app = approved();
    app.ports().edit(fr(1, "slower", &[]), 1);
    app.ports().edit(sp(2, "cache, better", &["FR-1"]), 2);
    app
}

/// FR-1 and SP-2 on it, tested by two files, both approved.
fn tested() -> ShelfApp<FakeShelf> {
    let tests = ["tests/a.rs", "tests/c.rs"];
    let fake = FakeShelf::new().with_files(vec![
        fr(1, "slow", &[]),
        sp_tested(2, "cache", &["FR-1"], &tests),
    ]);
    for at in tests {
        fake.put_file(at, at.as_bytes());
    }
    let mut app = ShelfApp::new(fake);
    block_on(app.judge_all(vec![approve("FR-1"), approve("SP-2")])).unwrap();
    app
}

/// The fake's books with `cover = true` on the spec's `grounds`: FR-2 and FR-4 are
/// alone, and may be exempt.
fn covering() -> ShelfApp<FakeShelf> {
    let mut books = decls();
    books
        .iter_mut()
        .find(|b| b.name == "spec")
        .and_then(|b| b.links.iter_mut().find(|l| l.name == "grounds"))
        .expect("the fake declares it")
        .cover = Some(CoverDecl::Truth(true));
    let files: Vec<DocReading> = vec![
        fr(1, "grounded", &[]),
        fr(2, "kept", &[]),
        fr(4, "also kept", &[]),
        sp(3, "cache", &["FR-1"]),
    ];
    let fake = FakeShelf::new().with_files(files);
    *fake.decls.lock().unwrap() = books;
    ShelfApp::new(fake)
}

fn exempt(reference: &str) -> ExemptRequest {
    ExemptRequest {
        reference: r(reference),
        book: BookName::new("spec").unwrap(),
        link: EntryName::new("grounds").unwrap(),
        reason: NonBlank::new("kept on purpose").unwrap(),
        by: Default::default(),
    }
}

fn same(reference: &str) -> SameMeaningRequest {
    SameMeaningRequest {
        about: SameMeaningAbout::Doc(r(reference)),
        reason: NonBlank::new("a typo fixed").unwrap(),
        by: Default::default(),
        from: None,
    }
}

fn moved(from: &str, to: &str) -> MovedRequest {
    MovedRequest {
        from: aname(from),
        to: aname(to),
        reason: NonBlank::new("the module was split").unwrap(),
        by: Default::default(),
    }
}

fn cases() -> Vec<Case> {
    vec![
        Case {
            name: "judge",
            make: pending,
            run: |app| block_on(app.judge_all(vec![approve("FR-1"), approve("SP-2")])).map(drop),
            records: 2,
        },
        Case {
            name: "approve_chosen",
            make: pending,
            run: |app| {
                block_on(app.approve_chosen(ApproveChosenRequest {
                    choose: Choose::Pending(PendingScope::All),
                    reason: reason("reviewed"),
                    by: Default::default(),
                    until: None,
                }))
                .map(drop)
            },
            records: 2,
        },
        Case {
            name: "same_meaning",
            make: edited,
            run: |app| block_on(app.same_meaning(vec![same("FR-1"), same("SP-2")])).map(drop),
            records: 2,
        },
        Case {
            name: "exempt",
            make: covering,
            run: |app| block_on(app.exempt(vec![exempt("FR-2"), exempt("FR-4")])).map(drop),
            records: 2,
        },
        Case {
            name: "moved",
            make: tested,
            run: |app| {
                app.ports().remove_file("tests/a.rs");
                app.ports().put_file("tests/b.rs", b"tests/a.rs");
                app.ports().remove_file("tests/c.rs");
                app.ports().put_file("tests/d.rs", b"tests/c.rs");
                block_on(app.moved(vec![
                    moved("tests/a.rs", "tests/b.rs"),
                    moved("tests/c.rs", "tests/d.rs"),
                ]))
                .map(drop)
            },
            records: 2,
        },
        Case {
            name: "moved_prefix",
            make: tested,
            run: |app| {
                app.ports().set_boundaries(&file_space(), &["/"]);
                for (old, new) in [("tests/a.rs", "spec/a.rs"), ("tests/c.rs", "spec/c.rs")] {
                    app.ports().remove_file(old);
                    app.ports().put_file(new, old.as_bytes());
                }
                block_on(
                    app.moved_prefix(PrefixMovedRequest {
                        of: PrefixMove::new(
                            NamePrefix::new("tests").unwrap(),
                            NamePrefix::new("spec").unwrap(),
                        )
                        .unwrap(),
                        reason: NonBlank::new("the directory was renamed").unwrap(),
                        by: Default::default(),
                    }),
                )
                .map(drop)
            },
            records: 2,
        },
        Case {
            name: "withdraw",
            make: approved,
            run: |app| {
                let first = app.ports().records()[0].id();
                block_on(app.withdraw(WithdrawRequest {
                    records: vec![first],
                    reason: NonBlank::new("not the same after all").unwrap(),
                    by: Default::default(),
                }))
                .map(drop)
            },
            records: 1,
        },
    ]
}

/// The paths of the files of the records the ledger holds from the `from`th on.
fn files_from(app: &ShelfApp<FakeShelf>, from: usize) -> Vec<RelPath> {
    app.ports()
        .records()
        .iter()
        .skip(from)
        .map(|record| app.ports().path_of(record))
        .collect()
}

#[test]
fn every_use_case_puts_its_records_on_disk_once_with_the_path_of_each() {
    for case in cases() {
        let mut app = (case.make)();
        let (records, calls) = (
            app.ports().record_count(),
            app.ports().persist_calls().len(),
        );
        (case.run)(&mut app).unwrap_or_else(|e| panic!("{}: {e:?}", case.name));
        let written = files_from(&app, records);
        assert_eq!(written.len(), case.records, "{}", case.name);
        for file in &written {
            assert!(file.as_str().starts_with(".whyshelf/ledger/"), "{file}");
        }
        let all = app.ports().persist_calls();
        assert_eq!(all.len(), calls + 1, "{}: once for the run", case.name);
        assert_eq!(all[calls], written, "{}", case.name);
    }
}

#[test]
fn a_write_that_fails_part_way_is_told_as_the_records_written() {
    for case in cases().into_iter().filter(|c| c.records > 1) {
        // The paths the run writes, from a run that does not fail: the fake gives the
        // same identities to the same run.
        let mut whole = (case.make)();
        let records = whole.ports().record_count();
        (case.run)(&mut whole).unwrap();
        let paths = files_from(&whole, records);

        let mut app = (case.make)();
        let calls = app.ports().persist_calls().len();
        *app.ports().appends_before_failing.lock().unwrap() = Some(1);
        let error = (case.run)(&mut app).unwrap_err();
        let ShelfError::PartWay {
            written,
            failed,
            cause,
            not_durable,
        } = &error
        else {
            panic!("{}: {error:?}", case.name);
        };
        assert_eq!(written.clone().into_vec(), paths[..1], "{}", case.name);
        assert_eq!(*failed, paths[1], "{}", case.name);
        assert!(
            matches!(
                **cause,
                ShelfError::Port {
                    port: ShelfPort::LedgerAppend,
                    ..
                }
            ),
            "{}: {cause:?}",
            case.name
        );
        assert!(not_durable.is_none(), "{}", case.name);
        assert_eq!(error.class(), ErrorClass::Failed, "{}", case.name);
        // What was written was put on disk, once, before it was told.
        let all = app.ports().persist_calls();
        assert_eq!(all.len(), calls + 1, "{}", case.name);
        assert_eq!(all[calls], paths[..1], "{}", case.name);
    }
}

#[test]
fn a_write_that_fails_before_any_record_was_written_is_its_own_error() {
    for case in cases() {
        let mut app = (case.make)();
        let calls = app.ports().persist_calls().len();
        *app.ports().appends_before_failing.lock().unwrap() = Some(0);
        let error = (case.run)(&mut app).unwrap_err();
        assert!(
            matches!(
                error,
                ShelfError::Port {
                    port: ShelfPort::LedgerAppend,
                    ..
                }
            ),
            "{}: {error:?}",
            case.name
        );
        assert_eq!(app.ports().persist_calls().len(), calls, "{}", case.name);
    }
}

#[test]
fn records_all_written_and_not_put_on_disk_are_not_durable_and_are_named() {
    for case in cases() {
        let mut app = (case.make)();
        let records = app.ports().record_count();
        app.ports().fail_persist("the drive went away");
        let error = (case.run)(&mut app).unwrap_err();
        let ShelfError::NotDurable { files, cause } = &error else {
            panic!("{}: {error:?}", case.name);
        };
        assert_eq!(
            files.clone().into_vec(),
            files_from(&app, records),
            "{}",
            case.name
        );
        assert_eq!(cause.reason(), "fake failure: the drive went away");
        assert_eq!(error.class(), ErrorClass::Failed, "{}", case.name);
        // The records are not removed.
        assert_eq!(app.ports().record_count(), records + case.records);
    }
}

/// Who ran the command, as a record's canonical form says it: the one `ran_by` key.
fn ran_by_of(record: &Record) -> Option<String> {
    match record.to_canon() {
        Canon::Map(map) => match map.get("ran_by") {
            None => None,
            Some(Canon::Str(name)) => Some(name.clone()),
            Some(other) => panic!("a ran_by that is no text: {other:?}"),
        },
        other => panic!("a record is a map: {other:?}"),
    }
}

#[test]
fn every_use_case_writes_who_ran_the_command_as_the_port_names_one() {
    for case in cases() {
        let mut app = (case.make)();
        let records = app.ports().record_count();
        // Only now: what the case wrote to be ready is not about it.
        app.ports().run_by("bot");
        (case.run)(&mut app).unwrap_or_else(|e| panic!("{}: {e:?}", case.name));
        let written: Vec<Record> = app.ports().records().into_iter().skip(records).collect();
        assert_eq!(written.len(), case.records, "{}", case.name);
        for record in &written {
            assert_eq!(ran_by_of(record).as_deref(), Some("bot"), "{}", case.name);
        }
    }
}

#[test]
fn a_record_names_no_one_who_ran_it_when_the_port_names_none() {
    for case in cases() {
        let mut app = (case.make)();
        let records = app.ports().record_count();
        (case.run)(&mut app).unwrap_or_else(|e| panic!("{}: {e:?}", case.name));
        for record in app.ports().records().iter().skip(records) {
            assert_eq!(ran_by_of(record), None, "{}", case.name);
        }
    }
}

/// `append` is called in `run_writes.rs` and nowhere else outside the tests and the
/// ports' own page: the one way left to write a record the run does not know. A
/// module added to the use cases is added to the list of
/// `run_writes_tests::no_module_but_run_writes_calls_a_create_port`.
#[test]
fn no_module_but_run_writes_appends_a_record() {
    let modules = [
        ("approve_chosen", include_str!("approve_chosen.rs")),
        ("exempt", include_str!("exempt.rs")),
        ("judge", include_str!("judge.rs")),
        ("moved", include_str!("moved.rs")),
        ("same_meaning", include_str!("same_meaning.rs")),
        ("withdraw", include_str!("withdraw.rs")),
    ];
    for (name, source) in modules {
        assert!(
            !source.contains(".append(") && !source.contains(".append()"),
            "{name}.rs appends to the ledger: write inside RunWrites::run"
        );
    }
    let run_writes = include_str!("run_writes.rs");
    assert_eq!(run_writes.matches(".append(record)").count(), 1);
}
