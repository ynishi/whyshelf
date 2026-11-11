//! Tests of `moves` over the in-memory fake: which names the source of suggested
//! moves is asked about, once per space; a space that cannot suggest; the fixed order
//! of the answer; and that nothing is written.

use std::collections::{BTreeMap, BTreeSet};

use crate::domain::shelf::artifact::ArtifactKey;
use crate::domain::shelf::ledger::{
    FoundBy, MoveCandidate, MoveNote, NameSuggestions, Similarity, SpaceSuggestions,
};
use crate::domain::shelf::lookup::Reference;
use crate::domain::shelf::schema::{Cardinality, LinkEntryDecl, LinkTargetDecl, Propagation};

use super::fake_tests::{
    FakeShelf, aname, artifact, block_on, docid, file, file_space, fr, other_space,
};
use super::{JudgeRequest, Judgement, MovedRequest, ShelfApp};

fn approve(reference: &str) -> JudgeRequest {
    JudgeRequest {
        judgement: Judgement::Approve {
            until: None,
            reason: None,
            again: false,
        },
        reference: reference.parse::<Reference>().unwrap(),
        actor: None,
    }
}

/// FR-1, and SP-2 on it, tested by three files and coded by two names of the space
/// `other`, every one there; both approved.
fn approved() -> ShelfApp<FakeShelf> {
    let fake = FakeShelf::new().with_files(vec![
        fr(1, "slow", &[]),
        file(
            "specs/sp-2.md",
            2,
            "SP-2",
            "cache",
            &[
                ("grounds", &["FR-1"]),
                ("tests", &["tests/a.rs", "tests/b.rs", "tests/c.rs"]),
                ("code", &["x", "y"]),
            ],
        ),
    ]);
    fake.decls.lock().unwrap()[1].links.push(code_link());
    for at in ["tests/a.rs", "tests/b.rs", "tests/c.rs"] {
        fake.put_file(at, at.as_bytes());
    }
    for at in ["x", "y"] {
        fake.put_in(&other_space(), at, at.as_bytes());
    }
    let mut app = ShelfApp::new(fake);
    block_on(app.judge_all(vec![approve("FR-1"), approve("SP-2")])).unwrap();
    app
}

fn renamed(name: &str, similarity: Option<u8>) -> MoveCandidate {
    MoveCandidate::new(
        aname(name),
        FoundBy::Renamed {
            similarity: similarity.map(|s| Similarity::new(s).unwrap()),
        },
        true,
    )
}

#[test]
fn each_space_is_asked_once_about_its_gone_names_and_nothing_is_written() {
    let app = approved();
    app.ports().remove_file("tests/a.rs");
    app.ports().remove_file("tests/c.rs");
    lock_remove(&app, "x");
    let appends = app.ports().append_count();
    let reads = app.ports().artifact_read_count();

    let report = block_on(app.moves()).unwrap();

    assert_eq!(
        app.ports().suggest_asked(),
        [
            (file_space(), vec![aname("tests/a.rs"), aname("tests/c.rs")]),
            (other_space(), vec![aname("x")]),
        ]
    );
    // One reading for the load and one, of the pinned names only, for the question.
    assert_eq!(app.ports().artifact_read_count(), reads + 2);
    let asked = app.ports().asked();
    let last: BTreeSet<ArtifactKey> = asked
        .last()
        .unwrap()
        .iter()
        .map(|(space, name, _)| ArtifactKey::new(space.clone(), name.clone()))
        .collect();
    assert_eq!(
        last,
        BTreeSet::from([
            artifact(&file_space(), "tests/a.rs"),
            artifact(&file_space(), "tests/b.rs"),
            artifact(&file_space(), "tests/c.rs"),
            artifact(&other_space(), "x"),
            artifact(&other_space(), "y"),
        ])
    );
    let gone: Vec<ArtifactKey> = report
        .suggestions
        .absent
        .iter()
        .map(|s| s.gone().artifact().clone())
        .collect();
    assert_eq!(
        gone,
        [
            artifact(&file_space(), "tests/a.rs"),
            artifact(&file_space(), "tests/c.rs"),
            artifact(&other_space(), "x"),
        ]
    );
    for one in &report.suggestions.absent {
        assert_eq!(one.gone().pinned_by(), BTreeSet::from([docid(2)]));
        assert!(report.names.key_of(docid(2)).is_some());
    }
    assert_eq!(app.ports().append_count(), appends);
    assert_eq!(app.ports().created_count(), 0);
    assert_eq!(app.ports().replaced_count(), 0);
}

/// Take the artifact `name` of the space `other` away.
fn lock_remove(app: &ShelfApp<FakeShelf>, name: &str) {
    app.ports()
        .elsewhere
        .lock()
        .unwrap()
        .remove(&artifact(&other_space(), name));
}

#[test]
fn a_name_declared_moved_is_asked_about_as_its_new_name_and_nothing_gone_asks_nothing() {
    let mut app = approved();
    app.ports().remove_file("tests/a.rs");
    app.ports().put_file("tests/d.rs", b"tests/a.rs");
    block_on(app.moved(vec![MovedRequest {
        from: aname("tests/a.rs"),
        to: aname("tests/d.rs"),
        reason: crate::domain::nonempty::NonBlank::new("split").unwrap(),
        actor: None,
    }]))
    .unwrap();
    let report = block_on(app.moves()).unwrap();
    assert!(report.suggestions.absent.is_empty());
    assert!(report.suggestions.cannot_suggest.is_empty());
    assert!(app.ports().suggest_asked().is_empty());

    // The new name gone too: it is the one asked about.
    app.ports().remove_file("tests/d.rs");
    let report = block_on(app.moves()).unwrap();
    assert_eq!(
        app.ports().suggest_asked(),
        [(file_space(), vec![aname("tests/d.rs")])]
    );
    assert_eq!(report.suggestions.absent.len(), 1);
}

#[test]
fn a_space_that_cannot_suggest_says_so_and_keeps_its_names() {
    let app = approved();
    app.ports().remove_file("tests/a.rs");
    lock_remove(&app, "x");
    app.ports()
        .answer_moves(&other_space(), SpaceSuggestions::CannotSuggest);
    let report = block_on(app.moves()).unwrap();
    assert_eq!(
        report.suggestions.cannot_suggest,
        BTreeSet::from([other_space()])
    );
    let x = report
        .suggestions
        .absent
        .iter()
        .find(|s| s.gone().artifact() == &artifact(&other_space(), "x"))
        .unwrap();
    assert!(x.candidates().is_empty());
    assert!(x.notes().is_empty());
}

#[test]
fn the_answer_is_in_one_order_whatever_the_adapter_gives() {
    let app = approved();
    app.ports().remove_file("tests/a.rs");
    app.ports().remove_file("tests/b.rs");
    app.ports().answer_moves(
        &file_space(),
        SpaceSuggestions::Suggested(BTreeMap::from([
            (
                aname("tests/b.rs"),
                NameSuggestions {
                    candidates: vec![
                        renamed("t/z.rs", Some(51)),
                        renamed("t/y.rs", None),
                        MoveCandidate::new(aname("t/x.rs"), FoundBy::SameContent, true),
                        renamed("t/w.rs", Some(97)),
                        renamed("t/x.rs", Some(88)),
                        renamed("tests/a.rs", Some(100)),
                    ],
                    // Out of the order of kinds, and one twice.
                    notes: vec![
                        MoveNote::ScanTruncated { read: 3 },
                        MoveNote::NoRevision,
                        MoveNote::ScanTruncated { read: 3 },
                    ],
                },
            ),
            (aname("tests/a.rs"), NameSuggestions::default()),
        ])),
    );
    let report = block_on(app.moves()).unwrap();
    let absent = &report.suggestions.absent;
    assert_eq!(absent.len(), 2);
    assert_eq!(absent[0].gone().name(), &aname("tests/a.rs"));
    assert!(absent[0].candidates().is_empty());
    let b = &absent[1];
    assert_eq!(b.gone().name(), &aname("tests/b.rs"));
    let names: Vec<&str> = b.candidates().iter().map(|c| c.name().as_str()).collect();
    assert_eq!(names, ["t/x.rs", "t/w.rs", "t/z.rs", "t/y.rs"]);
    assert_eq!(b.candidates()[0].found(), FoundBy::SameContent);
    assert_eq!(
        b.notes(),
        [MoveNote::NoRevision, MoveNote::ScanTruncated { read: 3 }]
    );
}

#[test]
fn a_name_pinned_in_two_spaces_is_listed_with_a_note_and_never_asked_about() {
    // SP-2 codes `tests/c.rs` in the space `other` too.
    let fake = FakeShelf::new().with_files(vec![
        fr(1, "slow", &[]),
        file(
            "specs/sp-2.md",
            2,
            "SP-2",
            "cache",
            &[
                ("grounds", &["FR-1"]),
                ("tests", &["tests/a.rs", "tests/c.rs"]),
                ("code", &["tests/c.rs"]),
            ],
        ),
    ]);
    fake.decls.lock().unwrap()[1].links.push(code_link());
    for at in ["tests/a.rs", "tests/c.rs"] {
        fake.put_file(at, at.as_bytes());
    }
    fake.put_in(&other_space(), "tests/c.rs", b"elsewhere");
    let mut app = ShelfApp::new(fake);
    block_on(app.judge_all(vec![approve("FR-1"), approve("SP-2")])).unwrap();
    app.ports().remove_file("tests/a.rs");
    app.ports().remove_file("tests/c.rs");

    let report = block_on(app.moves()).unwrap();
    // Only `tests/a.rs` is asked about; `tests/c.rs` is listed with the note.
    assert_eq!(
        app.ports().suggest_asked(),
        [(file_space(), vec![aname("tests/a.rs")])]
    );
    let absent = &report.suggestions.absent;
    assert_eq!(absent.len(), 2);
    assert_eq!(absent[1].gone().name(), &aname("tests/c.rs"));
    assert!(absent[1].candidates().is_empty());
    assert_eq!(
        absent[1].notes(),
        [MoveNote::InTwoSpaces {
            spaces: BTreeSet::from([file_space(), other_space()])
        }]
    );
    assert!(report.suggestions.cannot_suggest.is_empty());
    // `judge moved` refuses the same name, for the same reason.
    app.ports().put_file("tests/d.rs", b"tests/c.rs");
    let refused = block_on(app.moved(vec![MovedRequest {
        from: aname("tests/c.rs"),
        to: aname("tests/d.rs"),
        reason: crate::domain::nonempty::NonBlank::new("split").unwrap(),
        actor: None,
    }]));
    assert!(refused.is_err());
}

/// The link `code` of the spec: to artifacts of the space `other`.
fn code_link() -> LinkEntryDecl {
    LinkEntryDecl {
        name: "code".to_owned(),
        target: LinkTargetDecl::Artifact,
        cardinality: Cardinality::Many,
        propagation: Propagation::Depends,
        space: Some("other".to_owned()),
        compare: None,
        cover: None,
    }
}
