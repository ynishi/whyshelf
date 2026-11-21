//! Tests of the trial of the moves of a prefix (`frame_prefixes`) over the in-memory
//! fake: that each move is held as `judge moved --prefix` holds it, on one reading of
//! the names of all of them, and that `moves` offers the moves its candidates share
//! by that trial and nothing else.

use std::collections::BTreeSet;

use crate::domain::shelf::artifact::{ArtifactKey, NamePrefix, PrefixMove};
use crate::domain::shelf::ledger::{
    FoundBy, MoveCandidate, NameSuggestions, PinnedNames, PrefixRefusal, SpaceSuggestions,
};
use crate::domain::shelf::lookup::Reference;

use super::fake_tests::{FakeShelf, aname, artifact, block_on, file, file_space, fr, other_space};
use super::load::load;
use super::{JudgeRequest, Judgement, ShelfApp};

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

fn moving(from: &str, to: &str) -> PrefixMove {
    PrefixMove::new(NamePrefix::new(from).unwrap(), NamePrefix::new(to).unwrap()).unwrap()
}

/// FR-1 and SP-2 on it, tested by every file of `tests` (each holding its own name);
/// both approved. The files take prefixes at `/`; the space `other` takes none, and
/// is pinned by `elsewhere`.
fn approved(tests: &[&str], elsewhere: &[&str]) -> ShelfApp<FakeShelf> {
    let fake = FakeShelf::new().with_files(vec![
        fr(1, "slow", &[]),
        file(
            "specs/sp-2.md",
            2,
            "SP-2",
            "cache",
            &[
                ("grounds", &["FR-1"]),
                ("tests", tests),
                ("code", elsewhere),
            ],
        ),
    ]);
    for at in tests {
        fake.put_file(at, at.as_bytes());
    }
    fake.decls.lock().unwrap()[1].links.push(code_link());
    for at in elsewhere {
        fake.put_in(&other_space(), at, at.as_bytes());
    }
    fake.set_boundaries(&file_space(), &["/"]);
    let mut app = ShelfApp::new(fake);
    block_on(app.judge_all(vec![approve("FR-1"), approve("SP-2")])).unwrap();
    app
}

/// The link `code` of the spec: to artifacts of the space `other`, which the fake
/// gives no boundary for.
fn code_link() -> crate::domain::shelf::schema::LinkEntryDecl {
    use crate::domain::shelf::schema::{Cardinality, LinkEntryDecl, LinkTargetDecl, Propagation};
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

fn move_file(app: &ShelfApp<FakeShelf>, from: &str, to: &str) {
    app.ports().remove_file(from);
    app.ports().put_file(to, from.as_bytes());
}

fn pinned(app: &ShelfApp<FakeShelf>) -> PinnedNames {
    PinnedNames::of(&block_on(load(app.ports())).unwrap().ledger)
}

#[test]
fn each_move_is_tried_in_order_and_every_name_of_every_plan_is_read_once() {
    let app = approved(&["src/m/a.rs", "src/m/b.rs", "lib/x.rs"], &[]);
    move_file(&app, "src/m/a.rs", "src/n/a.rs");
    move_file(&app, "src/m/b.rs", "src/n/b.rs");
    // `lib/x.rs` goes to a place that is not under the new prefix of the second move.
    app.ports().remove_file("lib/x.rs");
    let pinned = pinned(&app);
    let (reads, appended) = (
        app.ports().artifact_read_count(),
        app.ports().append_count(),
    );
    let (first, second) = (moving("src/m", "src/n"), moving("lib", "lib2"));

    let tried = block_on(app.frame_prefixes(&pinned, &[first.clone(), second.clone()])).unwrap();

    assert_eq!(tried.len(), 2);
    assert_eq!((&tried[0].of, &tried[1].of), (&first, &second));
    let moved: Vec<(ArtifactKey, ArtifactKey)> = tried[0]
        .framed
        .as_ref()
        .unwrap()
        .iter()
        .map(|m| (m.from(), m.to()))
        .collect();
    assert_eq!(
        moved,
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
    // The second stands on nothing: the new name has nothing there, and it is refused
    // by name, as `judge moved --prefix` refuses it.
    assert!(matches!(
        tried[1].framed,
        Err(PrefixRefusal::Names(ref names)) if names.len() == 1
    ));
    assert!(tried.iter().all(|t| t.not_asked.is_empty()));

    // One reading for all of them, of the old and new names of the plans, and nothing
    // written.
    assert_eq!(app.ports().artifact_read_count(), reads + 1);
    let asked: BTreeSet<String> = app
        .ports()
        .asked()
        .last()
        .unwrap()
        .iter()
        .map(|(_, name, _)| name.to_string())
        .collect();
    assert_eq!(
        asked,
        BTreeSet::from(
            [
                "src/m/a.rs",
                "src/n/a.rs",
                "src/m/b.rs",
                "src/n/b.rs",
                "lib/x.rs",
                "lib2/x.rs"
            ]
            .map(str::to_owned)
        )
    );
    assert_eq!(app.ports().append_count(), appended);
}

#[test]
fn a_move_with_nothing_under_it_is_refused_and_when_every_move_is_so_no_name_is_read() {
    let app = approved(&["src/m/a.rs"], &[]);
    let pinned = pinned(&app);
    let reads = app.ports().artifact_read_count();

    let tried = block_on(app.frame_prefixes(&pinned, &[moving("docs", "doc")])).unwrap();

    assert_eq!(tried.len(), 1);
    assert!(matches!(
        tried[0].framed,
        Err(PrefixRefusal::NothingUnder { .. })
    ));
    assert_eq!(app.ports().artifact_read_count(), reads);

    // No move, nothing asked.
    assert!(
        block_on(app.frame_prefixes(&pinned, &[]))
            .unwrap()
            .is_empty()
    );
    assert_eq!(app.ports().artifact_read_count(), reads);
}

#[test]
fn the_spaces_whose_adapter_takes_no_prefix_are_told_with_each_move_that_was_planned() {
    let app = approved(&["src/m/a.rs"], &["f"]);
    move_file(&app, "src/m/a.rs", "src/n/a.rs");
    let pinned = pinned(&app);

    let tried = block_on(app.frame_prefixes(&pinned, &[moving("src/m", "src/n")])).unwrap();

    assert!(tried[0].framed.is_ok());
    assert_eq!(tried[0].not_asked, [other_space()]);
}

// What `moves` offers.

/// A candidate of `to`, reached by the move of the prefix `of`.
fn under(to: &str, of: &PrefixMove) -> MoveCandidate {
    MoveCandidate::new(aname(to), FoundBy::SameEnd, true).under(of.clone())
}

fn answer(app: &ShelfApp<FakeShelf>, of: &[(&str, Vec<MoveCandidate>)]) {
    app.ports().answer_moves(
        &file_space(),
        SpaceSuggestions::Suggested(
            of.iter()
                .map(|(gone, candidates)| {
                    (
                        aname(gone),
                        NameSuggestions {
                            candidates: candidates.clone(),
                            notes: Vec::new(),
                        },
                    )
                })
                .collect(),
        ),
    );
}

#[test]
fn a_prefix_two_gone_names_share_is_tried_and_offered_when_it_stands() {
    let app = approved(&["src/m/a.rs", "src/m/b.rs"], &[]);
    move_file(&app, "src/m/a.rs", "src/n/a.rs");
    move_file(&app, "src/m/b.rs", "src/n/b.rs");
    let of = moving("src/m", "src/n");
    answer(
        &app,
        &[
            ("src/m/a.rs", vec![under("src/n/a.rs", &of)]),
            ("src/m/b.rs", vec![under("src/n/b.rs", &of)]),
        ],
    );
    let (reads, appended) = (
        app.ports().artifact_read_count(),
        app.ports().append_count(),
    );

    let report = block_on(app.moves()).unwrap();

    let suggestions = &report.suggestions;
    assert_eq!(suggestions.prefixes.len(), 1);
    assert_eq!(suggestions.prefixes[0].of(), &of);
    let names: Vec<&ArtifactKey> = suggestions.prefixes[0].moved().iter().collect();
    assert_eq!(
        names,
        [
            &artifact(&file_space(), "src/m/a.rs"),
            &artifact(&file_space(), "src/m/b.rs")
        ]
    );
    assert!(suggestions.prefixes[0].not_asked().is_empty());
    assert!(suggestions.refused_prefixes.is_empty());
    assert_eq!(suggestions.untried_prefixes, 0);
    // The load, the pinned names, and the one trial.
    assert_eq!(app.ports().artifact_read_count(), reads + 3);
    assert_eq!(app.ports().append_count(), appended);
}

#[test]
fn a_prefix_that_stands_carries_the_spaces_whose_adapter_takes_no_prefix() {
    // `f` is pinned in the space `other`, whose adapter takes no prefix: names of it
    // may be under the prefix, and are not moved.
    let app = approved(&["src/m/a.rs", "src/m/b.rs"], &["f"]);
    move_file(&app, "src/m/a.rs", "src/n/a.rs");
    move_file(&app, "src/m/b.rs", "src/n/b.rs");
    let of = moving("src/m", "src/n");
    answer(
        &app,
        &[
            ("src/m/a.rs", vec![under("src/n/a.rs", &of)]),
            ("src/m/b.rs", vec![under("src/n/b.rs", &of)]),
        ],
    );

    let report = block_on(app.moves()).unwrap();

    let suggestions = &report.suggestions;
    assert_eq!(suggestions.prefixes.len(), 1);
    assert_eq!(suggestions.prefixes[0].of(), &of);
    assert_eq!(suggestions.prefixes[0].not_asked(), [other_space()]);
    assert!(suggestions.refused_prefixes.is_empty());
}

#[test]
fn a_prefix_that_would_be_refused_is_told_as_refused_and_not_offered() {
    let app = approved(&["src/m/a.rs", "src/m/b.rs"], &[]);
    // `src/n/b.rs` is not there: the move of the prefix is refused for `src/m/b.rs`.
    move_file(&app, "src/m/a.rs", "src/n/a.rs");
    app.ports().remove_file("src/m/b.rs");
    let of = moving("src/m", "src/n");
    answer(
        &app,
        &[
            ("src/m/a.rs", vec![under("src/n/a.rs", &of)]),
            ("src/m/b.rs", vec![under("src/n/b.rs", &of)]),
        ],
    );

    let report = block_on(app.moves()).unwrap();

    assert!(report.suggestions.prefixes.is_empty());
    assert_eq!(report.suggestions.refused_prefixes.len(), 1);
    assert_eq!(report.suggestions.refused_prefixes[0].of(), &of);
    assert_eq!(report.suggestions.refused_prefixes[0].refused(), 1);
}

#[test]
fn a_prefix_one_gone_name_has_is_not_tried() {
    let app = approved(&["src/m/a.rs", "src/m/b.rs"], &[]);
    move_file(&app, "src/m/a.rs", "src/n/a.rs");
    move_file(&app, "src/m/b.rs", "src/n/b.rs");
    let of = moving("src/m", "src/n");
    answer(
        &app,
        &[
            ("src/m/a.rs", vec![under("src/n/a.rs", &of)]),
            (
                "src/m/b.rs",
                vec![MoveCandidate::new(
                    aname("src/n/b.rs"),
                    FoundBy::SameEnd,
                    true,
                )],
            ),
        ],
    );
    let reads = app.ports().artifact_read_count();

    let report = block_on(app.moves()).unwrap();

    assert!(report.suggestions.prefixes.is_empty());
    assert!(report.suggestions.refused_prefixes.is_empty());
    // The load and the pinned names only: nothing was tried.
    assert_eq!(app.ports().artifact_read_count(), reads + 2);
}
