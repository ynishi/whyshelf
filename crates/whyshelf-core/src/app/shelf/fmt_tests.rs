//! Tests of `fmt` and `fmt --check`, over the in-memory fake.

use std::collections::BTreeMap;

use crate::domain::id::DocId;
use crate::domain::location::RelPath;
use crate::domain::nonempty::NonBlank;
use crate::domain::shelf::build::{DocReading, DropAsk, DropProblem, RawProblem, RawValue, ReadId};
use crate::domain::shelf::doc::DocHash;
use crate::domain::shelf::freshness::Standing;
use crate::domain::shelf::lookup::Reference;
use crate::domain::shelf::schema::{Cardinality, FieldEntryDecl, FieldPresence, FieldTypeDecl};

use super::fake_tests::{FakeShelf, block_on, docid, fr, path, sp};
use super::ports::Replaceable;
use super::{
    Dropped, FmtReport, JudgeRequest, Judgement, LeftAlone, LeftAloneWhy, ShelfApp, ShelfError,
};

fn approve(reference: &str) -> JudgeRequest {
    JudgeRequest {
        judgement: Judgement::Approve {
            until: None,
            reason: None,
            again: false,
        },
        reference: reference.parse::<Reference>().unwrap(),
        by: Default::default(),
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
        ("specs/sp-3.md", "a # comment"),
        ("specs/sp-4.md", "reads otherwise"),
        ("specs/broken.md", "---"),
    ] {
        fake.put_file(at, bytes.as_bytes());
    }
    fake.put_in_form("specs/sp-2.md");
    fake.set_comment("specs/sp-3.md", 4);
    fake.set_reads_otherwise("specs/sp-4.md");
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
    let before = app.ports().files.lock().unwrap().clone();
    let report = block_on(app.fmt(&DropAsk::default())).unwrap();
    assert_eq!(
        report,
        FmtReport {
            rewritten: paths(&["findings/fr-1.md"]),
            in_form: paths(&["specs/sp-2.md"]),
            not_in_form: Vec::new(),
            left_alone: left_alone(),
            dropped: Vec::new(),
        }
    );
    assert_eq!(app.ports().replaced_count(), 1);
    let files = app.ports().files.lock().unwrap().clone();
    assert_ne!(files[&path("findings/fr-1.md")], b"id: unquoted");
    assert_eq!(
        files[&path("specs/sp-2.md")],
        before[&path("specs/sp-2.md")]
    );
    assert_eq!(files[&path("specs/sp-3.md")], b"a # comment");
    assert_eq!(files[&path("specs/sp-4.md")], b"reads otherwise");
    assert_eq!(files[&path("specs/broken.md")], b"---");

    // A second run finds it in the form and writes nothing more.
    let again = block_on(app.fmt(&DropAsk::default())).unwrap();
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
        && *standing == Standing::FRESH));
    let report = block_on(app.fmt(&DropAsk::default())).unwrap();
    assert_eq!(report.rewritten.len(), 1);
    assert_eq!(hashes_and_standings(&app), before);
}

#[test]
fn fmt_check_writes_nothing_and_lists_the_files() {
    let app = world();
    let files = app.ports().files.lock().unwrap().clone();
    let records = app.ports().record_count();
    let report = block_on(app.fmt_check(&DropAsk::default())).unwrap();
    assert_eq!(
        report,
        FmtReport {
            rewritten: Vec::new(),
            in_form: paths(&["specs/sp-2.md"]),
            not_in_form: paths(&["findings/fr-1.md"]),
            left_alone: left_alone(),
            dropped: Vec::new(),
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
    let report = block_on(app.fmt(&DropAsk::default())).unwrap();
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
    fake.set_replaceable("findings/fr-1.md", Replaceable::ReadOnly);
    fake.set_replaceable("findings/fr-2.md", Replaceable::Linked);
    let mut app = ShelfApp::new(fake);
    let check = block_on(app.fmt_check(&DropAsk::default())).unwrap();
    assert!(!check.all_in_form());
    assert!(check.not_in_form.is_empty());
    let report = block_on(app.fmt(&DropAsk::default())).unwrap();
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
    let fake = FakeShelf::new().with_files(vec![fr(1, "one", &["FR-2"])]);
    fake.put_in_form("findings/fr-1.md");
    let app = ShelfApp::new(fake);
    assert!(
        block_on(app.fmt_check(&DropAsk::default()))
            .unwrap()
            .all_in_form()
    );
    let report = block_on(world().fmt_check(&DropAsk::default())).unwrap();
    assert!(!report.all_in_form());
}

#[test]
fn a_port_that_fails_is_an_error_of_the_run() {
    let mut fake = FakeShelf::new().with_files(vec![fr(1, "slow", &[])]);
    fake.failing = Some("disk");
    let mut app = ShelfApp::new(fake);
    let error = block_on(app.fmt(&DropAsk::default())).unwrap_err();
    assert_eq!(error.class(), super::ErrorClass::Failed);
}

// ---- entries that say nothing (#136) ---------------------------------------------

/// A world whose findings have an optional `source` and a `base` link of this count,
/// with one finding that holds `entry` besides a title and a link to FR-2, written
/// with every entry (as a file was in the form before the form left out one that says
/// nothing).
fn world_with(base: Cardinality, entry: (&str, RawValue)) -> ShelfApp<FakeShelf> {
    let mut finding = fr(1, "one", &["FR-2"]);
    if let DocReading::Parsed { entries, .. } = &mut finding {
        entries.insert(entry.0.to_owned(), entry.1);
    }
    let fake = FakeShelf::new().with_files(vec![finding]);
    {
        let mut decls = fake.decls.lock().unwrap();
        decls[0].fields.push(FieldEntryDecl {
            name: "source".to_owned(),
            ty: FieldTypeDecl::Word("text".to_owned()),
            presence: FieldPresence::Optional,
        });
        decls[0].links[0].cardinality = base;
    }
    fake.put_with_every_entry("findings/fr-1.md");
    ShelfApp::new(fake)
}

fn entries_of_the_first_file(app: &ShelfApp<FakeShelf>) -> BTreeMap<String, RawValue> {
    match &app.ports().readings.lock().unwrap()[0] {
        DocReading::Parsed { entries, .. } => entries.clone(),
        other => panic!("not parsed: {other:?}"),
    }
}

fn text(value: &str) -> RawValue {
    RawValue::Text(value.to_owned())
}

/// Every way an entry can say nothing.
fn nothings() -> Vec<RawValue> {
    vec![
        text(""),
        text("  "),
        RawValue::Null,
        RawValue::List(Vec::new()),
    ]
}

#[test]
fn a_file_with_an_optional_field_written_empty_is_rewritten_without_it_and_reads_the_same() {
    for nothing in nothings() {
        let mut app = world_with(Cardinality::Many, ("source", nothing.clone()));
        assert!(entries_of_the_first_file(&app).contains_key("source"));
        let before = hashes_and_standings(&app);

        let check = block_on(app.fmt_check(&DropAsk::default())).unwrap();
        assert_eq!(
            check.not_in_form,
            paths(&["findings/fr-1.md"]),
            "{nothing:?}"
        );
        assert!(!check.all_in_form(), "{nothing:?}");

        let report = block_on(app.fmt(&DropAsk::default())).unwrap();
        assert_eq!(
            report.rewritten,
            paths(&["findings/fr-1.md"]),
            "{nothing:?}"
        );
        let written = entries_of_the_first_file(&app);
        assert!(!written.contains_key("source"), "{nothing:?}");
        assert!(written.contains_key("title"), "{nothing:?}");
        assert_eq!(written["base"], RawValue::List(vec!["FR-2".to_owned()]));
        assert_eq!(hashes_and_standings(&app), before, "{nothing:?}");

        // Written once: the next run finds it in the form.
        let again = block_on(app.fmt(&DropAsk::default())).unwrap();
        assert!(again.rewritten.is_empty(), "{nothing:?}");
        assert_eq!(again.in_form, paths(&["findings/fr-1.md"]), "{nothing:?}");
    }
}

#[test]
fn a_link_written_empty_that_admits_none_is_rewritten_without_it() {
    for count in [Cardinality::Many, Cardinality::Optional] {
        for nothing in nothings() {
            let mut app = world_with(count, ("base", nothing.clone()));
            let report = block_on(app.fmt(&DropAsk::default())).unwrap();
            assert_eq!(report.rewritten, paths(&["findings/fr-1.md"]), "{count:?}");
            assert!(
                !entries_of_the_first_file(&app).contains_key("base"),
                "{count:?}: {nothing:?}"
            );
        }
    }
}

#[test]
fn an_entry_that_says_nothing_and_that_the_build_tells_is_kept() {
    // A required field, an entry the book does not declare, and a link whose count
    // does not admit none: each stays, so the file is in the form as it is.
    let cases = [
        (Cardinality::Many, "title", text("")),
        (Cardinality::Many, "undeclared", text("")),
        (Cardinality::Many, "undeclared", RawValue::Null),
        (Cardinality::AtLeastOne, "base", RawValue::List(Vec::new())),
        (Cardinality::One, "base", RawValue::Null),
    ];
    for (count, name, nothing) in cases {
        let mut app = world_with(count, (name, nothing.clone()));
        let check = block_on(app.fmt_check(&DropAsk::default())).unwrap();
        assert_eq!(
            check.in_form,
            paths(&["findings/fr-1.md"]),
            "{name}: {nothing:?}"
        );
        assert!(check.all_in_form(), "{name}: {nothing:?}");
        let report = block_on(app.fmt(&DropAsk::default())).unwrap();
        assert!(report.rewritten.is_empty(), "{name}: {nothing:?}");
        assert_eq!(entries_of_the_first_file(&app)[name], nothing, "{name}");
        assert_eq!(app.ports().replaced_count(), 0);
    }
}

// ---- dropping a name (#143) --------------------------------------------------------

fn drop_ask(names: &[&str]) -> DropAsk {
    DropAsk {
        names: names.iter().map(|n| NonBlank::new(*n).unwrap()).collect(),
    }
}

fn dropped(file: &str, names: &[&str]) -> Dropped {
    Dropped {
        file: path(file),
        names: drop_ask(names).names,
    }
}

/// A reading with this entry added.
fn with_entry(mut reading: DocReading, name: &str, value: RawValue) -> DocReading {
    if let DocReading::Parsed { entries, .. } = &mut reading {
        entries.insert(name.to_owned(), value);
    }
    reading
}

/// The books of the fake, where a spec declares `source` (an optional field) beside
/// its `tests` link to artifacts, and a finding declares neither. FR-1 holds `source`
/// and `tests` (which its book does not declare), SP-2 holds both (which its book
/// does), FR-3 holds neither. Every file is in the form as it is, without a name to drop.
fn world_to_drop() -> ShelfApp<FakeShelf> {
    let fr1 = with_entry(fr(1, "one", &[]), "source", text("x"));
    let fr1 = with_entry(fr1, "tests", RawValue::List(vec!["a.rs".to_owned()]));
    let sp2 = with_entry(sp(2, "two", &[]), "source", text("y"));
    let sp2 = with_entry(sp2, "tests", RawValue::List(vec!["b.rs".to_owned()]));
    let fake = FakeShelf::new().with_files(vec![fr1, sp2, fr(3, "three", &[])]);
    fake.decls.lock().unwrap()[1].fields.push(FieldEntryDecl {
        name: "source".to_owned(),
        ty: FieldTypeDecl::Word("text".to_owned()),
        presence: FieldPresence::Optional,
    });
    for at in ["findings/fr-1.md", "specs/sp-2.md", "findings/fr-3.md"] {
        fake.put_in_form(at);
    }
    ShelfApp::new(fake)
}

#[test]
fn fmt_without_a_name_to_drop_keeps_an_entry_its_book_does_not_declare() {
    let mut app = world_to_drop();
    let report = block_on(app.fmt(&DropAsk::default())).unwrap();
    assert!(report.rewritten.is_empty());
    assert!(report.dropped.is_empty());
    assert_eq!(app.ports().replaced_count(), 0);
    assert!(entries_of_the_first_file(&app).contains_key("source"));
}

#[test]
fn fmt_drops_a_name_only_from_the_files_of_the_books_that_do_not_declare_it() {
    let mut app = world_to_drop();
    let report = block_on(app.fmt(&drop_ask(&["source"]))).unwrap();
    assert_eq!(
        report,
        FmtReport {
            rewritten: paths(&["findings/fr-1.md"]),
            in_form: paths(&["specs/sp-2.md", "findings/fr-3.md"]),
            not_in_form: Vec::new(),
            left_alone: Vec::new(),
            dropped: vec![dropped("findings/fr-1.md", &["source"])],
        }
    );
    assert_eq!(app.ports().replaced_count(), 1);
    // Only `source` goes from FR-1: `tests` is another name, held and not declared.
    let readings = app.ports().readings.lock().unwrap().clone();
    let held = |n: usize, name: &str| match &readings[n] {
        DocReading::Parsed { entries, .. } => entries.contains_key(name),
        other => panic!("not parsed: {other:?}"),
    };
    assert!(!held(0, "source") && held(0, "tests"));
    assert!(held(1, "source") && held(1, "tests"));
    drop(readings);

    // Written once: the next run finds every file in the form.
    let again = block_on(app.fmt(&drop_ask(&["source"]))).unwrap();
    assert!(again.rewritten.is_empty() && again.dropped.is_empty());
    assert_eq!(app.ports().replaced_count(), 1);
}

#[test]
fn a_document_whose_unknown_entries_are_all_dropped_is_valid_afterwards() {
    let mut app = world_to_drop();
    // FR-1 holds two entries its book does not declare, so it is not a valid document.
    let invalid = |app: &ShelfApp<FakeShelf>| {
        hashes_and_standings(app)
            .iter()
            .any(|(id, hash, _)| *id == docid(1) && hash.is_none())
    };
    assert!(invalid(&app));
    block_on(app.fmt(&drop_ask(&["source"]))).unwrap();
    assert!(invalid(&app), "`tests` is still there");
    let report = block_on(app.fmt(&drop_ask(&["source", "tests"]))).unwrap();
    assert_eq!(
        report.dropped,
        vec![dropped("findings/fr-1.md", &["tests"])]
    );
    assert!(!invalid(&app));
}

#[test]
fn a_name_of_a_link_to_artifacts_is_dropped_the_same_way() {
    let mut app = world_to_drop();
    let report = block_on(app.fmt(&drop_ask(&["tests"]))).unwrap();
    assert_eq!(report.rewritten, paths(&["findings/fr-1.md"]));
    assert_eq!(
        report.dropped,
        vec![dropped("findings/fr-1.md", &["tests"])]
    );
    let readings = app.ports().readings.lock().unwrap().clone();
    match &readings[0] {
        DocReading::Parsed { entries, .. } => {
            assert!(entries.contains_key("source") && !entries.contains_key("tests"));
        }
        other => panic!("not parsed: {other:?}"),
    }
    match &readings[1] {
        DocReading::Parsed { entries, .. } => assert!(entries.contains_key("tests")),
        other => panic!("not parsed: {other:?}"),
    }
}

#[test]
fn fmt_check_with_names_lists_each_file_with_the_names_it_would_drop_and_writes_nothing() {
    let app = world_to_drop();
    let files = app.ports().files.lock().unwrap().clone();
    let records = app.ports().record_count();
    let report = block_on(app.fmt_check(&drop_ask(&["tests", "source"]))).unwrap();
    assert_eq!(
        report,
        FmtReport {
            rewritten: Vec::new(),
            in_form: paths(&["specs/sp-2.md", "findings/fr-3.md"]),
            not_in_form: paths(&["findings/fr-1.md"]),
            left_alone: Vec::new(),
            dropped: vec![dropped("findings/fr-1.md", &["source", "tests"])],
        }
    );
    assert!(!report.all_in_form());
    assert_eq!(app.ports().replaced_count(), 0);
    assert_eq!(*app.ports().files.lock().unwrap(), files);
    assert_eq!(app.ports().record_count(), records);
    assert!(entries_of_the_first_file(&app).contains_key("source"));
}

#[test]
fn a_name_no_file_holds_drops_nothing() {
    let mut app = world_to_drop();
    let check = block_on(app.fmt_check(&drop_ask(&["nothing"]))).unwrap();
    assert!(check.all_in_form() && check.dropped.is_empty());
    let report = block_on(app.fmt(&drop_ask(&["nothing"]))).unwrap();
    assert!(report.rewritten.is_empty() && report.dropped.is_empty());
}

#[test]
fn names_that_do_not_fit_the_declaration_are_a_request_error_and_nothing_is_written() {
    let mut app = world_to_drop();
    // `id` and `key` are reserved, and `title` is declared by both books.
    let ask = drop_ask(&["title", "source", "id", "key"]);
    let expected = vec![
        DropProblem::Reserved(NonBlank::new("id").unwrap()),
        DropProblem::Reserved(NonBlank::new("key").unwrap()),
        DropProblem::DeclaredEverywhere(NonBlank::new("title").unwrap()),
    ];
    let error = block_on(app.fmt(&ask)).unwrap_err();
    assert_eq!(error.class(), super::ErrorClass::Request);
    let ShelfError::Drop(problems) = &error else {
        panic!("not a drop error: {error:?}");
    };
    assert_eq!(problems.iter().cloned().collect::<Vec<_>>(), expected);
    let error = block_on(app.fmt_check(&ask)).unwrap_err();
    assert!(matches!(&error, ShelfError::Drop(problems) if problems.len() == 3));
    assert_eq!(app.ports().replaced_count(), 0);
    assert!(entries_of_the_first_file(&app).contains_key("source"));
}

#[test]
fn a_file_left_alone_keeps_its_entry_and_is_not_listed_as_dropping_it() {
    for reason in ["comment", "read_only", "changed"] {
        let mut app = world_to_drop();
        match reason {
            "comment" => app.ports().set_comment("findings/fr-1.md", 3),
            "read_only" => app
                .ports()
                .set_replaceable("findings/fr-1.md", Replaceable::ReadOnly),
            _ => {
                app.ports()
                    .edited_before_replace
                    .lock()
                    .unwrap()
                    .insert(path("findings/fr-1.md"));
            }
        }
        let report = block_on(app.fmt(&drop_ask(&["source"]))).unwrap();
        assert!(report.rewritten.is_empty(), "{reason}");
        assert!(report.dropped.is_empty(), "{reason}");
        assert_eq!(report.left_alone.len(), 1, "{reason}");
        assert!(entries_of_the_first_file(&app).contains_key("source"));
    }
}

#[test]
fn a_file_that_would_read_otherwise_is_left_alone_with_its_entry() {
    let mut app = world_to_drop();
    app.ports().set_reads_otherwise("findings/fr-1.md");
    let report = block_on(app.fmt(&drop_ask(&["source"]))).unwrap();
    assert_eq!(
        report.left_alone,
        vec![LeftAlone {
            file: path("findings/fr-1.md"),
            why: LeftAloneWhy::ReadsOtherwise,
        }]
    );
    assert!(report.dropped.is_empty());
    assert!(entries_of_the_first_file(&app).contains_key("source"));
}

#[test]
fn a_file_that_also_has_a_comment_or_cannot_be_written_is_still_left_alone() {
    let mut app = world_with(Cardinality::Many, ("source", text("")));
    app.ports().set_comment("findings/fr-1.md", 3);
    let report = block_on(app.fmt(&DropAsk::default())).unwrap();
    assert_eq!(
        report.left_alone,
        vec![LeftAlone {
            file: path("findings/fr-1.md"),
            why: LeftAloneWhy::Comment { line: 3 },
        }]
    );

    let mut app = world_with(Cardinality::Many, ("source", text("")));
    app.ports()
        .set_replaceable("findings/fr-1.md", Replaceable::ReadOnly);
    let report = block_on(app.fmt(&DropAsk::default())).unwrap();
    assert_eq!(report.left_alone[0].why, LeftAloneWhy::ReadOnly);
    assert!(entries_of_the_first_file(&app).contains_key("source"));
}
