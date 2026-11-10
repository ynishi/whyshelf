//! Tests of `fmt` and `fmt --check`, over the in-memory fake.

use crate::domain::id::DocId;
use crate::domain::location::RelPath;
use crate::domain::shelf::build::{DocReading, RawProblem, ReadId};
use crate::domain::shelf::doc::DocHash;
use crate::domain::shelf::freshness::{Freshness, Standing};
use crate::domain::shelf::lookup::Reference;

use super::fake_tests::{FakeShelf, block_on, docid, fr, path, sp};
use super::ports::Form;
use super::{FmtReport, JudgeRequest, Judgement, LeftAlone, LeftAloneWhy, ShelfApp};

fn approve(reference: &str) -> JudgeRequest {
    JudgeRequest {
        judgement: Judgement::Approve {
            until: None,
            reason: None,
        },
        reference: reference.parse::<Reference>().unwrap(),
        actor: None,
    }
}

/// A text in the form that reads as `reading`.
fn other(reading: &DocReading) -> Form {
    Form::Other {
        text: b"in the form".to_vec(),
        reads_as: reading.clone(),
    }
}

/// Five files: FR-1 not in the form, SP-2 in it, SP-3 with a comment, SP-4 whose
/// text in the form would read otherwise, and one that cannot be read. FR-1 and SP-2
/// are approved, SP-2 upon FR-1.
fn world() -> ShelfApp<FakeShelf> {
    let unparsed = DocReading::Unparsed {
        origin: path("specs/broken.md"),
        id: ReadId::Unreadable,
        why: RawProblem::Unclosed,
    };
    let fr1 = fr(1, "slow", &[]);
    let sp4 = sp(4, "four", &["FR-1"]);
    let fake = FakeShelf::new().with_files(vec![
        fr1.clone(),
        sp(2, "cache", &["FR-1"]),
        sp(3, "three", &["FR-1"]),
        sp4,
        unparsed,
    ]);
    for (at, bytes) in [
        ("findings/fr-1.md", "id: unquoted"),
        ("specs/sp-2.md", "in the form"),
        ("specs/sp-3.md", "a # comment"),
        ("specs/sp-4.md", "reads otherwise"),
        ("specs/broken.md", "---"),
    ] {
        fake.put_file(at, bytes.as_bytes());
    }
    fake.set_form("findings/fr-1.md", other(&fr1));
    fake.set_form("specs/sp-3.md", Form::Comment { line: 4 });
    fake.set_form("specs/sp-4.md", other(&sp(4, "another title", &["FR-1"])));
    let mut app = ShelfApp::new(fake);
    block_on(app.judge(approve("FR-1"))).unwrap();
    block_on(app.judge(approve("SP-2"))).unwrap();
    app
}

fn paths(texts: &[&str]) -> Vec<RelPath> {
    texts.iter().map(|t| path(t)).collect()
}

fn left_alone() -> Vec<LeftAlone> {
    vec![
        LeftAlone {
            file: path("specs/sp-3.md"),
            why: LeftAloneWhy::Comment { line: 4 },
        },
        LeftAlone {
            file: path("specs/sp-4.md"),
            why: LeftAloneWhy::ReadsOtherwise,
        },
        LeftAlone {
            file: path("specs/broken.md"),
            why: LeftAloneWhy::Unparsed(RawProblem::Unclosed),
        },
    ]
}

/// Every document's hash and standing, by identity.
fn hashes_and_standings(app: &ShelfApp<FakeShelf>) -> Vec<(DocId, Option<DocHash>, Standing)> {
    let status = block_on(app.status()).unwrap();
    status
        .rows
        .iter()
        .map(|row| {
            let hash = block_on(app.get(&row.id.to_string().parse().unwrap()))
                .unwrap()
                .doc
                .map(|doc| *doc.hash());
            (row.id, hash, row.standing.clone())
        })
        .collect()
}

#[test]
fn fmt_rewrites_a_file_not_in_the_form_and_leaves_the_rest() {
    let mut app = world();
    let report = block_on(app.fmt()).unwrap();
    assert_eq!(
        report,
        FmtReport {
            rewritten: paths(&["findings/fr-1.md"]),
            in_form: paths(&["specs/sp-2.md"]),
            not_in_form: Vec::new(),
            left_alone: left_alone(),
        }
    );
    assert_eq!(app.ports().replaced_count(), 1);
    let files = app.ports().files.lock().unwrap().clone();
    assert_eq!(files[&path("findings/fr-1.md")], b"in the form");
    assert_eq!(files[&path("specs/sp-2.md")], b"in the form");
    assert_eq!(files[&path("specs/sp-3.md")], b"a # comment");
    assert_eq!(files[&path("specs/sp-4.md")], b"reads otherwise");
    assert_eq!(files[&path("specs/broken.md")], b"---");

    // A second run finds it in the form and writes nothing more.
    let again = block_on(app.fmt()).unwrap();
    assert!(again.rewritten.is_empty());
    assert_eq!(again.in_form, paths(&["findings/fr-1.md", "specs/sp-2.md"]));
    assert_eq!(app.ports().replaced_count(), 1);
}

#[test]
fn fmt_changes_no_hash_and_no_approval() {
    let mut app = world();
    let before = hashes_and_standings(&app);
    assert!(before.iter().any(|(id, hash, standing)| *id == docid(1)
        && hash.is_some()
        && *standing == Standing::Approved(Freshness::Fresh)));
    let report = block_on(app.fmt()).unwrap();
    assert_eq!(report.rewritten.len(), 1);
    assert_eq!(hashes_and_standings(&app), before);
}

#[test]
fn fmt_check_writes_nothing_and_lists_the_files() {
    let app = world();
    let files = app.ports().files.lock().unwrap().clone();
    let records = app.ports().record_count();
    let report = block_on(app.fmt_check()).unwrap();
    assert_eq!(
        report,
        FmtReport {
            rewritten: Vec::new(),
            in_form: paths(&["specs/sp-2.md"]),
            not_in_form: paths(&["findings/fr-1.md"]),
            left_alone: left_alone(),
        }
    );
    assert_eq!(app.ports().replaced_count(), 0);
    assert_eq!(*app.ports().files.lock().unwrap(), files);
    assert_eq!(app.ports().record_count(), records);
}

#[test]
fn a_file_edited_between_the_reading_and_the_write_is_left_alone() {
    let mut app = world();
    app.ports()
        .edited_before_replace
        .lock()
        .unwrap()
        .insert(path("findings/fr-1.md"));
    let report = block_on(app.fmt()).unwrap();
    assert!(report.rewritten.is_empty());
    assert_eq!(
        report.left_alone[0],
        LeftAlone {
            file: path("findings/fr-1.md"),
            why: LeftAloneWhy::Changed,
        }
    );
    assert_eq!(app.ports().replaced_count(), 0);
    assert_eq!(
        app.ports().files.lock().unwrap()[&path("findings/fr-1.md")],
        b"id: unquoted edited"
    );
}

#[test]
fn a_read_only_file_and_a_hard_link_are_left_alone() {
    let fake = FakeShelf::new().with_files(vec![fr(1, "one", &[]), fr(2, "two", &[])]);
    fake.set_form("findings/fr-1.md", Form::ReadOnly);
    fake.set_form("findings/fr-2.md", Form::Linked);
    let mut app = ShelfApp::new(fake);
    let check = block_on(app.fmt_check()).unwrap();
    assert!(!check.all_in_form());
    assert!(check.not_in_form.is_empty());
    let report = block_on(app.fmt()).unwrap();
    assert_eq!(
        report.left_alone,
        vec![
            LeftAlone {
                file: path("findings/fr-1.md"),
                why: LeftAloneWhy::ReadOnly,
            },
            LeftAlone {
                file: path("findings/fr-2.md"),
                why: LeftAloneWhy::Linked,
            },
        ]
    );
    assert_eq!(app.ports().replaced_count(), 0);
}

#[test]
fn every_file_is_in_the_form_only_when_none_is_to_write_or_left_alone() {
    let app = ShelfApp::new(FakeShelf::new().with_files(vec![fr(1, "one", &[])]));
    assert!(block_on(app.fmt_check()).unwrap().all_in_form());
    let report = block_on(world().fmt_check()).unwrap();
    assert!(!report.all_in_form());
}

#[test]
fn a_port_that_fails_is_an_error_of_the_run() {
    let mut fake = FakeShelf::new().with_files(vec![fr(1, "slow", &[])]);
    fake.failing = Some("disk");
    let mut app = ShelfApp::new(fake);
    let error = block_on(app.fmt()).unwrap_err();
    assert_eq!(error.class(), super::ErrorClass::Failed);
}
