//! Tests of `rekey`, and of the run of writes it makes its files in (all or none, and the
//! undo), over the in-memory fake.
//!
//! The merge: two findings write `FR-1` (the identities 1 and 2, in `findings/fr-1.md`
//! and `findings/other.md`) and two specs (11 and 12) link to it. The finding 2 is the
//! one to take another key.

use crate::domain::id::DocId;
use crate::domain::shelf::build::{DocReading, RawValue};
use crate::domain::shelf::lookup::Reference;
use crate::domain::shelf::rekey::RekeyProblem;

use super::fake_tests::{FakeShelf, block_on, docid, file, fr, path, sp};
use super::{Edited, ErrorClass, RekeyRequest, ShelfApp, ShelfError};

fn merged() -> ShelfApp<FakeShelf> {
    ShelfApp::new(FakeShelf::new().with_files(vec![
        fr(1, "one", &[]),
        file("findings/other.md", 2, "FR-1", "other", &[("base", &[])]),
        sp(11, "eleven", &["FR-1"]),
        sp(12, "twelve", &["FR-1"]),
    ]))
}

fn r(n: u128) -> Reference {
    docid(n).to_string().parse().unwrap()
}

fn ask(follow: &[u128], stay: &[u128], both: &[u128], dry_run: bool) -> RekeyRequest {
    RekeyRequest {
        target: r(2),
        key: None,
        follow: follow.iter().map(|n| r(*n)).collect(),
        stay: stay.iter().map(|n| r(*n)).collect(),
        both: both.iter().map(|n| r(*n)).collect(),
        dry_run,
    }
}

fn readings(app: &ShelfApp<FakeShelf>) -> Vec<DocReading> {
    app.ports().readings.lock().unwrap().clone()
}

fn files(app: &ShelfApp<FakeShelf>) -> Vec<String> {
    let mut files: Vec<String> = readings(app)
        .iter()
        .map(|reading| reading.origin().to_string())
        .collect();
    files.sort();
    files
}

/// The keys written in the link `grounds` of the spec in `file`.
fn grounds(app: &ShelfApp<FakeShelf>, file: &str) -> Vec<String> {
    readings(app)
        .into_iter()
        .find_map(|reading| match reading {
            DocReading::Parsed {
                origin, entries, ..
            } if origin.as_str() == file => match entries.get("grounds") {
                Some(RawValue::List(keys)) => Some(keys.clone()),
                _ => None,
            },
            _ => None,
        })
        .unwrap()
}

fn all_the_files() -> Vec<String> {
    [
        "findings/fr-1.md",
        "findings/other.md",
        "specs/sp-11.md",
        "specs/sp-12.md",
    ]
    .map(str::to_owned)
    .to_vec()
}

#[test]
fn a_document_moves_to_the_file_of_its_new_key_and_the_links_that_mean_it_follow() {
    let mut app = merged();
    let report = block_on(app.rekey(ask(&[11], &[12], &[], false))).unwrap();

    assert_eq!(report.moved.id, docid(2));
    assert_eq!(report.moved.from_key.to_string(), "finding:FR-1");
    assert_eq!(report.moved.to_key.to_string(), "finding:FR-2");
    assert_eq!(report.moved.from_file.as_str(), "findings/other.md");
    assert_eq!(report.moved.to_file.as_str(), "findings/FR-2.md");
    let files_of = |writers: &[crate::domain::shelf::rekey::Rewriting]| -> Vec<String> {
        writers.iter().map(|w| w.file().to_string()).collect()
    };
    assert_eq!(files_of(&report.rewritten), ["specs/sp-11.md"]);
    assert_eq!(files_of(&report.stayed), ["specs/sp-12.md"]);
    assert!(report.both.is_empty());
    assert_eq!(
        report.written,
        Some(Edited {
            created: path("findings/FR-2.md"),
            replaced: vec![path("specs/sp-11.md")],
            removed: path("findings/other.md"),
        })
    );
    // The target and each document that wrote the key in a link: asked of the files as
    // they are after the write.
    let ids: Vec<DocId> = report.approvals.iter().map(|row| row.id).collect();
    assert_eq!(ids, [docid(2), docid(11), docid(12)]);
    assert_eq!(
        report.names.key_of(docid(2)).unwrap().to_string(),
        "finding:FR-2"
    );

    // The files: the finding in a file of its key, its old file gone, the link that
    // meant it rewritten, the other left.
    assert_eq!(
        files(&app),
        [
            "findings/FR-2.md",
            "findings/fr-1.md",
            "specs/sp-11.md",
            "specs/sp-12.md"
        ]
    );
    assert_eq!(grounds(&app, "specs/sp-11.md"), ["FR-2"]);
    assert_eq!(grounds(&app, "specs/sp-12.md"), ["FR-1"]);
    // Written, put on disk once, then the removal put on disk.
    let fake = app.ports();
    assert_eq!(
        fake.persist_calls(),
        [vec![path("findings/FR-2.md"), path("specs/sp-11.md")]]
    );
    assert_eq!(fake.persist_gone_calls(), [vec![path("findings/other.md")]]);
}

#[test]
fn the_body_of_the_document_renumbered_is_listed_when_it_names_the_old_key() {
    // The finding 2 is invalid while `FR-1` is written twice; its body names the key.
    let merged_with_a_mention = || {
        ShelfApp::new(FakeShelf::new().with_files(vec![
            fr(1, "one", &[]),
            file(
                "findings/other.md",
                2,
                "FR-1",
                "a copy of FR-1",
                &[("base", &[])],
            ),
            sp(11, "eleven", &["FR-1"]),
        ]))
    };
    for dry_run in [true, false] {
        let mut app = merged_with_a_mention();
        let report = block_on(app.rekey(ask(&[11], &[], &[], dry_run))).unwrap();
        assert_eq!(report.mentions, [docid(2)], "dry run: {dry_run}");
    }
    // A document that does not name it lists none.
    let mut app = merged();
    let report = block_on(app.rekey(ask(&[11], &[12], &[], false))).unwrap();
    assert!(report.mentions.is_empty());
}

#[test]
fn both_keeps_the_old_key_and_adds_the_new_one() {
    let mut app = merged();
    let report = block_on(app.rekey(ask(&[], &[12], &[11], false))).unwrap();
    assert_eq!(report.both.len(), 1);
    assert_eq!(grounds(&app, "specs/sp-11.md"), ["FR-1", "FR-2"]);
    assert_eq!(grounds(&app, "specs/sp-12.md"), ["FR-1"]);
}

#[test]
fn a_dry_run_is_the_same_plan_and_writes_nothing() {
    let mut app = merged();
    let before = readings(&app);
    let report = block_on(app.rekey(ask(&[11], &[12], &[], true))).unwrap();
    assert_eq!(report.moved.to_key.to_string(), "finding:FR-2");
    assert_eq!(report.rewritten.len(), 1);
    assert_eq!(report.written, None);
    assert!(report.approvals.is_empty());
    assert_eq!(readings(&app), before);
    assert_eq!(app.ports().created_count(), 0);
    assert!(app.ports().persist_calls().is_empty());
    assert!(app.ports().persist_gone_calls().is_empty());
}

#[test]
fn a_document_that_is_not_decided_is_refused_with_every_such_document_and_nothing_is_written() {
    let mut app = merged();
    let before = readings(&app);
    let refused = block_on(app.rekey(ask(&[], &[], &[], false))).unwrap_err();
    assert_eq!(refused.class(), ErrorClass::Request);
    match refused {
        ShelfError::Rekey(RekeyProblem::Unsettled {
            undecided,
            contradicted,
        }) => {
            assert!(contradicted.is_empty());
            let files: Vec<String> = undecided.iter().map(|u| u.file.to_string()).collect();
            assert_eq!(files, ["specs/sp-11.md", "specs/sp-12.md"]);
        }
        other => panic!("{other:?}"),
    }
    assert_eq!(readings(&app), before);
    assert!(app.ports().persist_calls().is_empty());
}

#[test]
fn a_document_named_twice_is_refused() {
    let mut app = merged();
    let refused = block_on(app.rekey(ask(&[11], &[11], &[], false))).unwrap_err();
    assert!(matches!(refused, ShelfError::SaidTwice { doc } if doc == docid(11)));
}

#[test]
fn something_at_the_new_file_refuses_the_rekey_before_anything_is_written() {
    let mut app = merged();
    app.ports()
        .put_file("findings/FR-2.md", b"a file of a person's");
    let before = readings(&app);
    let refused = block_on(app.rekey(ask(&[11], &[12], &[], false))).unwrap_err();
    assert!(
        matches!(&refused, ShelfError::FileTaken { path } if path.as_str() == "findings/FR-2.md")
    );
    assert_eq!(readings(&app), before);
    assert!(app.ports().persist_calls().is_empty());
}

#[test]
fn a_stop_before_the_removal_puts_every_file_back_and_says_why() {
    let mut app = merged();
    let before = readings(&app);
    app.ports().fail_remove("the disk is full");
    let stopped = block_on(app.rekey(ask(&[11], &[12], &[], false))).unwrap_err();
    // Every file is back as it was and on disk, so the cause is told as it is.
    assert!(
        matches!(&stopped, ShelfError::Port { port, .. } if port.to_string() == "document remove"),
        "{stopped:?}"
    );
    assert_eq!(readings(&app), before);
    assert_eq!(files(&app), all_the_files());
    let fake = app.ports();
    // The files written were put on disk before the removal; the file put back, and the
    // file made and removed again, afterwards.
    assert_eq!(
        fake.persist_calls(),
        [
            vec![path("findings/FR-2.md"), path("specs/sp-11.md")],
            vec![path("specs/sp-11.md")]
        ]
    );
    assert_eq!(fake.persist_gone_calls(), [vec![path("findings/FR-2.md")]]);
}

#[test]
fn a_stop_that_cannot_be_undone_names_what_is_not_back() {
    let mut app = merged();
    app.ports().fail_remove("the disk is full");
    app.ports().fail_restore("the disk is full again");
    let stopped = block_on(app.rekey(ask(&[11], &[12], &[], false))).unwrap_err();
    match &stopped {
        ShelfError::PartWay {
            written,
            failed,
            not_durable,
            ..
        } => {
            // The link that was rewritten is not back; the file made is removed.
            let written: Vec<String> = written.iter().map(ToString::to_string).collect();
            assert_eq!(written, ["specs/sp-11.md"]);
            assert_eq!(failed.as_str(), "findings/other.md");
            assert!(not_durable.is_none());
        }
        other => panic!("{other:?}"),
    }
    assert_eq!(stopped.class(), ErrorClass::Failed);
    assert_eq!(grounds(&app, "specs/sp-11.md"), ["FR-2"]);
    assert!(!files(&app).contains(&"findings/FR-2.md".to_owned()));
    assert!(files(&app).contains(&"findings/other.md".to_owned()));
}

#[test]
fn a_file_that_changed_before_it_was_written_over_stops_the_run_and_nothing_is_left() {
    let mut app = merged();
    let before = readings(&app);
    app.ports()
        .edited_before_replace
        .lock()
        .unwrap()
        .insert(path("specs/sp-11.md"));
    let stopped = block_on(app.rekey(ask(&[11], &[12], &[], false))).unwrap_err();
    assert!(
        matches!(&stopped, ShelfError::Changed { file } if file.as_str() == "specs/sp-11.md"),
        "{stopped:?}"
    );
    assert_eq!(stopped.class(), ErrorClass::Request);
    assert_eq!(readings(&app), before);
    assert_eq!(
        app.ports().persist_gone_calls(),
        [vec![path("findings/FR-2.md")]]
    );
}

#[test]
fn a_removal_that_cannot_be_put_on_disk_leaves_what_was_written_and_names_it() {
    let mut app = merged();
    app.ports().fail_persist_gone("the drive is gone");
    let stopped = block_on(app.rekey(ask(&[11], &[12], &[], false))).unwrap_err();
    match &stopped {
        ShelfError::NotDurable { files, .. } => {
            let files: Vec<String> = files.iter().map(ToString::to_string).collect();
            // The file removed is named too: it is the path that may come back.
            assert_eq!(
                files,
                ["findings/FR-2.md", "specs/sp-11.md", "findings/other.md"]
            );
        }
        other => panic!("{other:?}"),
    }
    assert_eq!(stopped.class(), ErrorClass::Failed);
    // Whole, and left.
    assert!(files(&app).contains(&"findings/FR-2.md".to_owned()));
    assert!(!files(&app).contains(&"findings/other.md".to_owned()));
    assert_eq!(grounds(&app, "specs/sp-11.md"), ["FR-2"]);
}

#[test]
fn a_stop_that_cannot_be_undone_is_a_failure_whatever_stopped_it() {
    let mut app = merged();
    // A request-class stop (a file that changed), then the file made cannot be removed.
    app.ports()
        .edited_before_replace
        .lock()
        .unwrap()
        .insert(path("specs/sp-11.md"));
    app.ports().fail_remove("the disk is full");
    let stopped = block_on(app.rekey(ask(&[11], &[12], &[], false))).unwrap_err();
    match &stopped {
        ShelfError::PartWay {
            written,
            cause,
            not_durable,
            undo_failed,
            ..
        } => {
            let written: Vec<String> = written.iter().map(ToString::to_string).collect();
            assert_eq!(written, ["findings/FR-2.md"]);
            assert!(matches!(&**cause, ShelfError::Changed { .. }), "{cause:?}");
            assert_eq!(cause.class(), ErrorClass::Request);
            assert!(not_durable.is_none());
            assert!(*undo_failed);
        }
        other => panic!("{other:?}"),
    }
    // The manual says 3: the run failed, as the cause is only a request.
    assert_eq!(stopped.class(), ErrorClass::Failed);
}

#[test]
fn a_file_that_is_gone_when_it_is_removed_is_removed_and_the_document_stays_in_the_new_file() {
    let mut app = merged();
    app.ports()
        .gone_before_remove
        .lock()
        .unwrap()
        .insert(path("findings/other.md"));
    let report = block_on(app.rekey(ask(&[11], &[12], &[], false))).unwrap();
    // As written: no undo, and the file made is the document's.
    assert_eq!(
        report.written,
        Some(Edited {
            created: path("findings/FR-2.md"),
            replaced: vec![path("specs/sp-11.md")],
            removed: path("findings/other.md"),
        })
    );
    assert_eq!(
        files(&app),
        [
            "findings/FR-2.md",
            "findings/fr-1.md",
            "specs/sp-11.md",
            "specs/sp-12.md"
        ]
    );
    assert_eq!(grounds(&app, "specs/sp-11.md"), ["FR-2"]);
    let fake = app.ports();
    assert_eq!(
        fake.persist_calls(),
        [vec![path("findings/FR-2.md"), path("specs/sp-11.md")]]
    );
    assert_eq!(fake.persist_gone_calls(), [vec![path("findings/other.md")]]);
    assert_eq!(
        report.names.key_of(docid(2)).unwrap().to_string(),
        "finding:FR-2"
    );
}
