//! Tests of `moved_prefix` over the in-memory fake: that a directory or a module
//! renamed is declared once for every name under it, in every space, that the
//! approvals stand again after one declaration, and that a name that cannot be moved
//! is named, with nothing written.

use crate::domain::actor::Actor;
use crate::domain::nonempty::{NonBlank, NonEmptyVec};
use crate::domain::shelf::artifact::{NamePrefix, PrefixMove, Space};
use crate::domain::shelf::freshness::{Cause, Freshness, Standing};
use crate::domain::shelf::ledger::{
    MovedRefusal, NamedRefusal, NamedWhy, NothingWhy, PrefixRefusal, Record,
};
use crate::domain::shelf::lookup::Reference;
use crate::domain::shelf::schema::{Cardinality, LinkEntryDecl, LinkTargetDecl, Propagation};

use super::fake_tests::{
    FakeShelf, aname, artifact, block_on, file, file_space, fr, other_space, sp_tested,
};
use super::{ErrorClass, JudgeRequest, Judgement, PrefixMovedRequest, ShelfApp, ShelfError};

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
        actor: None,
    }
}

fn prefix(text: &str) -> NamePrefix {
    NamePrefix::new(text).unwrap()
}

fn request(from: &str, to: &str) -> PrefixMovedRequest {
    PrefixMovedRequest {
        of: PrefixMove::new(prefix(from), prefix(to)).unwrap(),
        reason: NonBlank::new("the directory was renamed").unwrap(),
        actor: None,
    }
}

fn standing(app: &ShelfApp<FakeShelf>, reference: &str) -> Standing {
    block_on(app.get(&r(reference))).unwrap().row.standing
}

fn names(refusal: &PrefixRefusal) -> Vec<(String, &NamedWhy)> {
    match refusal {
        PrefixRefusal::Names(names) => names.iter().map(|n| (n.name.to_string(), &n.why)).collect(),
        PrefixRefusal::NothingUnder { .. } => panic!("something is under the prefix"),
    }
}

// The approvals stand again after one declaration.

/// FR-1 and SP-2 on it, tested by every name, each file holding its own name, both
/// approved; the files take prefixes at `/`.
fn approved(tests: &[&str]) -> ShelfApp<FakeShelf> {
    let fake = FakeShelf::new().with_files(vec![
        fr(1, "slow", &[]),
        sp_tested(2, "cache", &["FR-1"], tests),
    ]);
    for at in tests {
        fake.put_file(at, at.as_bytes());
    }
    fake.set_boundaries(&file_space(), &["/"]);
    let mut app = ShelfApp::new(fake);
    block_on(app.judge_all(vec![approve("FR-1"), approve("SP-2")])).unwrap();
    app
}

/// Move the file at `from` to `to`, its bytes as they are.
fn move_file(app: &ShelfApp<FakeShelf>, from: &str, to: &str) {
    app.ports().remove_file(from);
    app.ports().put_file(to, from.as_bytes());
}

#[test]
fn a_renamed_module_is_declared_once_and_the_approval_stands_again() {
    let mut app = approved(&["src/m/a.rs", "src/m/b.rs"]);
    move_file(&app, "src/m/a.rs", "src/n/a.rs");
    move_file(&app, "src/m/b.rs", "src/n/b.rs");
    app.ports().edit(
        sp_tested(2, "cache", &["FR-1"], &["src/n/a.rs", "src/n/b.rs"]),
        2,
    );
    assert!(matches!(
        standing(&app, "SP-2"),
        Standing::Approved { freshness: Freshness::Degraded(d), .. }
            if matches!(d.causes().iter().next(), Some(Cause::ArtifactLinksChanged { .. }))
    ));

    let (appended, persisted, asked) = (
        app.ports().append_count(),
        app.ports().persist_calls().len(),
        app.ports().asked().len(),
    );
    let done = block_on(app.moved_prefix(PrefixMovedRequest {
        actor: Some(Actor::new("bob").unwrap()),
        ..request("src/m", "src/n")
    }))
    .unwrap();

    // One declaration for each name, in the order of the names, each by the one actor
    // at the time of the load and with an identity of its own.
    let got: Vec<_> = done
        .moved
        .iter()
        .map(|m| (m.of().from(), m.of().to()))
        .collect();
    assert_eq!(
        got,
        [
            (
                artifact(&file_space(), "src/m/a.rs"),
                artifact(&file_space(), "src/n/a.rs")
            ),
            (
                artifact(&file_space(), "src/m/b.rs"),
                artifact(&file_space(), "src/n/b.rs")
            ),
        ]
    );
    assert!(done.moved.iter().all(|m| m.actor().as_str() == "bob"));
    assert!(done.moved.iter().all(|m| m.at().millis() == 1000));
    assert_ne!(done.moved[0].id(), done.moved[1].id());
    assert!(done.not_asked.is_empty());

    // Written whole, put on disk once, and the names asked of the adapters in one
    // reading beside the load's.
    assert_eq!(app.ports().append_count(), appended + 2);
    assert_eq!(app.ports().persist_calls().len(), persisted + 1);
    let records = app.ports().records();
    assert!(
        records[2..]
            .iter()
            .all(|record| matches!(record, Record::Moved(_)))
    );
    let asks = app.ports().asked();
    assert_eq!(asks.len(), asked + 2, "the load's reading, and one more");
    let ask = asks.last().unwrap();
    for name in ["src/m/a.rs", "src/m/b.rs", "src/n/a.rs", "src/n/b.rs"] {
        assert!(ask.get(&file_space(), &aname(name)).is_some(), "{name}");
    }

    assert_eq!(standing(&app, "SP-2"), Standing::FRESH);
}

#[test]
fn a_name_still_at_the_old_name_is_named_and_nothing_is_written() {
    let mut app = approved(&["src/m/a.rs", "src/m/b.rs"]);
    // `a` moved; `b` is still there (a copy, not a move), and has nothing at its new
    // name either.
    move_file(&app, "src/m/a.rs", "src/n/a.rs");
    let (appended, persisted) = (
        app.ports().append_count(),
        app.ports().persist_calls().len(),
    );
    let error = block_on(app.moved_prefix(request("src/m", "src/n"))).unwrap_err();
    let ShelfError::MovedPrefixRefused(refusal) = &error else {
        panic!("{error:?}");
    };
    let got = names(refusal);
    assert_eq!(got.len(), 1);
    assert_eq!(got[0].0, "src/m/b.rs");
    assert!(matches!(
        got[0].1,
        NamedWhy::Move(MovedRefusal::FromPresent { .. })
    ));
    assert_eq!(error.class(), ErrorClass::Refused);
    assert_eq!(app.ports().append_count(), appended);
    assert_eq!(app.ports().persist_calls().len(), persisted);
}

#[test]
fn nothing_under_the_prefix_is_a_request_that_names_each_space() {
    let mut app = approved(&["src/m/a.rs"]);
    let appended = app.ports().append_count();
    let error = block_on(app.moved_prefix(request("lib", "src"))).unwrap_err();
    let ShelfError::MovedPrefixRefused(PrefixRefusal::NothingUnder { prefix, spaces }) = &error
    else {
        panic!("{error:?}");
    };
    assert_eq!(prefix.as_str(), "lib");
    assert_eq!(spaces, &[(file_space(), NothingWhy::NoneUnder)]);
    assert_eq!(error.class(), ErrorClass::Request);
    assert_eq!(app.ports().append_count(), appended);
}

#[test]
fn a_name_no_space_has_under_the_new_prefix_is_named_and_the_run_is_a_request() {
    let mut app = approved(&["src/m/a.rs"]);
    let too_long = "z".repeat(4095);
    let error = block_on(app.moved_prefix(request("src/m", &too_long))).unwrap_err();
    let ShelfError::MovedPrefixRefused(refusal) = &error else {
        panic!("{error:?}");
    };
    let got = names(refusal);
    assert_eq!(got.len(), 1);
    assert!(matches!(
        got[0].1,
        NamedWhy::Unbuildable { space, .. } if *space == file_space()
    ));
    assert_eq!(error.class(), ErrorClass::Request);
}

#[test]
fn the_class_of_names_that_cannot_be_moved_is_refused_when_any_one_is() {
    let named = |why| NamedRefusal {
        name: aname("a"),
        why,
    };
    let unbuildable = NamedWhy::Unbuildable {
        space: file_space(),
        why: NonBlank::new("too long").unwrap(),
    };
    let present = NamedWhy::Move(MovedRefusal::FromPresent {
        artifact: artifact(&file_space(), "a"),
    });
    let not_known = NamedWhy::Move(MovedRefusal::NotKnown { name: aname("a") });
    let class = |first: NamedWhy, rest: Vec<NamedWhy>| {
        ShelfError::MovedPrefixRefused(PrefixRefusal::Names(NonEmptyVec::new(
            named(first),
            rest.into_iter().map(named).collect(),
        )))
        .class()
    };
    assert_eq!(
        class(unbuildable.clone(), vec![not_known.clone()]),
        ErrorClass::Request
    );
    assert_eq!(
        class(unbuildable.clone(), vec![present.clone()]),
        ErrorClass::Refused
    );
    assert_eq!(class(present, vec![unbuildable]), ErrorClass::Refused);
    // The rule's class is held where a single move gets it: the same refusals.
    assert_eq!(
        ShelfError::MovedRefused(MovedRefusal::NotKnown { name: aname("a") }).class(),
        ErrorClass::Request
    );
}

// Three spaces in one project.

fn rust_test() -> Space {
    Space::new("rust-test").unwrap()
}

fn rust_item() -> Space {
    Space::new("rust-item").unwrap()
}

fn artifact_link(name: &str, space: &Space) -> LinkEntryDecl {
    LinkEntryDecl {
        name: name.to_owned(),
        target: LinkTargetDecl::Artifact,
        cardinality: Cardinality::Many,
        propagation: Propagation::Depends,
        space: Some(space.to_string()),
        compare: None,
        cover: None,
    }
}

/// SP-2 pins files, the tests of a Rust file and the items of a module, approved:
///
/// ```text
///   file       src/a/x.rs  src/a/y.rs  src/keep.rs
///   rust-test  src/a/x.rs::t
///   rust-item  app::m::f   app::m
/// ```
///
/// The first two spaces take prefixes at `/` (and `::` for the tests' items); the
/// items take them at `::` when `items_take_prefixes`.
fn three_spaces(items_take_prefixes: bool) -> ShelfApp<FakeShelf> {
    let fake = FakeShelf::new().with_files(vec![
        fr(1, "slow", &[]),
        file(
            "specs/sp-2.md",
            2,
            "SP-2",
            "cache",
            &[
                ("grounds", &["FR-1"]),
                ("tests", &["src/a/x.rs", "src/a/y.rs", "src/keep.rs"]),
                ("rtests", &["src/a/x.rs::t"]),
                ("items", &["app::m::f", "app::m"]),
            ],
        ),
    ]);
    {
        let mut decls = fake.decls.lock().unwrap();
        decls[1].links.push(artifact_link("rtests", &rust_test()));
        decls[1].links.push(artifact_link("items", &rust_item()));
    }
    fake.spaces
        .lock()
        .unwrap()
        .extend([rust_test(), rust_item()]);
    fake.set_boundaries(&file_space(), &["/"]);
    fake.set_boundaries(&rust_test(), &["/", "::"]);
    if items_take_prefixes {
        fake.set_boundaries(&rust_item(), &["::"]);
    }
    for at in ["src/a/x.rs", "src/a/y.rs", "src/keep.rs"] {
        fake.put_file(at, at.as_bytes());
    }
    fake.put_in(&rust_test(), "src/a/x.rs::t", b"t");
    fake.put_in(&rust_item(), "app::m::f", b"f");
    fake.put_in(&rust_item(), "app::m", b"m");
    let mut app = ShelfApp::new(fake);
    block_on(app.judge_all(vec![approve("FR-1"), approve("SP-2")])).unwrap();
    app
}

/// Move the artifact of `space` from `from` to `to`, as it is.
fn move_in(app: &ShelfApp<FakeShelf>, space: &Space, from: &str, to: &str) {
    let mut there = app.ports().elsewhere.lock().unwrap();
    let bytes = there.remove(&artifact(space, from)).unwrap();
    there.insert(artifact(space, to), bytes);
}

fn moves(done: &super::MovedPrefix) -> Vec<(Space, String, String)> {
    done.moved
        .iter()
        .map(|m| {
            (
                m.of().space().clone(),
                m.of().from_name().to_string(),
                m.of().to_name().to_string(),
            )
        })
        .collect()
}

#[test]
fn a_renamed_directory_is_moved_in_every_space_it_has_names_in_with_one_declaration() {
    let mut app = three_spaces(true);
    for at in ["x", "y"] {
        move_file(&app, &format!("src/a/{at}.rs"), &format!("src/b/{at}.rs"));
    }
    move_in(&app, &rust_test(), "src/a/x.rs::t", "src/b/x.rs::t");
    let done = block_on(app.moved_prefix(request("src/a", "src/b"))).unwrap();
    let t = |s: &Space, a: &str, b: &str| (s.clone(), a.to_owned(), b.to_owned());
    assert_eq!(
        moves(&done),
        [
            t(&file_space(), "src/a/x.rs", "src/b/x.rs"),
            t(&file_space(), "src/a/y.rs", "src/b/y.rs"),
            t(&rust_test(), "src/a/x.rs::t", "src/b/x.rs::t"),
        ]
    );
    assert!(done.not_asked.is_empty());
    assert_eq!(app.ports().append_count(), 2 + 3);
    assert_eq!(app.ports().persist_calls().last().unwrap().len(), 3);
}

#[test]
fn a_renamed_module_is_moved_in_every_space_it_has_names_in_with_one_declaration() {
    let mut app = three_spaces(true);
    move_in(&app, &rust_item(), "app::m::f", "app::n::f");
    move_in(&app, &rust_item(), "app::m", "app::n");
    let done = block_on(app.moved_prefix(request("app::m", "app::n"))).unwrap();
    let t = |a: &str, b: &str| (rust_item(), a.to_owned(), b.to_owned());
    assert_eq!(
        moves(&done),
        [t("app::m", "app::n"), t("app::m::f", "app::n::f")]
    );
    assert_eq!(app.ports().append_count(), 2 + 2);
    // Another space's names are not under it: nothing of them moved.
    assert!(done.moved.iter().all(|m| m.of().space() == &rust_item()));
}

#[test]
fn a_space_whose_adapter_takes_no_prefix_is_told_and_does_not_stop_the_others() {
    let mut app = three_spaces(false);
    for at in ["x", "y"] {
        move_file(&app, &format!("src/a/{at}.rs"), &format!("src/b/{at}.rs"));
    }
    move_in(&app, &rust_test(), "src/a/x.rs::t", "src/b/x.rs::t");
    let done = block_on(app.moved_prefix(request("src/a", "src/b"))).unwrap();
    assert_eq!(done.moved.len(), 3);
    assert_eq!(done.not_asked, [rust_item()]);
    // Not another space: the names of the project in no space the fake offers are
    // none of the answer's business.
    assert!(!done.not_asked.contains(&other_space()));
}
