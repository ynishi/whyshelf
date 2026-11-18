//! Tests of the move of a prefix: which names of the approvals that stand move
//! ([`plan_prefix_moves`]), that the refusals of both steps are one answer
//! ([`frame_prefix_moves`]), and the one rule of which space a name is known in
//! ([`PinnedNames::space_of`]).

use std::collections::{BTreeMap, BTreeSet};

use crate::domain::nonempty::NonBlank;
use crate::domain::shelf::artifact::{
    ArtifactNameProblem, ArtifactReading, ArtifactReadings, NamePrefix, PrefixAnswers, PrefixMove,
    Renamed, Space, SpacePrefixes, SpaceRenamed,
};
use crate::domain::shelf::fixtures_tests::{
    approve, decls, file_artifact, file_space, fr, ledger, list, name, reading, shelf_reading,
    spaces, text,
};
use crate::domain::shelf::schema::{Propagation, Schema};

use super::*;

fn other() -> Space {
    Space::new("other").unwrap()
}

fn why(text: &str) -> NonBlank {
    NonBlank::new(text).unwrap()
}

fn moving(from: &str, to: &str) -> PrefixMove {
    PrefixMove::new(NamePrefix::new(from).unwrap(), NamePrefix::new(to).unwrap()).unwrap()
}

/// The names approvals pin, in the space `file` (as SP-2's `tests`) and in the space
/// `other` (as its `code`), all there when they were approved.
fn pinned(files: &[&str], elsewhere: &[&str]) -> PinnedNames {
    let mut all = decls();
    let code = all[1]
        .links
        .iter_mut()
        .find(|l| l.name == "code")
        .expect("the spec has a code link");
    code.space = Some("other".to_owned());
    code.propagation = Propagation::Depends;
    let schema = Schema::validate(all, &spaces()).unwrap();
    let docs = vec![
        fr(1, &[]),
        reading(
            "specs/sp-2.md",
            2,
            "SP-2",
            &[
                ("title", text("spec")),
                ("grounds", list(&["FR-1"])),
                ("tests", list(files)),
                ("code", list(elsewhere)),
            ],
        ),
    ];
    let bytes: Vec<(&str, &[u8])> = files
        .iter()
        .chain(elsewhere)
        .map(|n| (*n, n.as_bytes()))
        .collect();
    let shelf = shelf_reading(&schema, docs, &bytes);
    let first = approve(&shelf, &ledger(vec![]), 1, 1001, 10, None);
    let second = approve(&shelf, &ledger(vec![first.clone()]), 2, 1002, 10, None);
    PinnedNames::of(&ledger(vec![first, second]))
}

/// What a space's adapter answers when it reads the prefix and finds each of
/// `under` under it (and no other name).
fn answered(under: &[(&str, &str)], outside: &[&str]) -> (SpacePrefixes, SpaceRenamed) {
    let renamed = under
        .iter()
        .map(|(old, new)| (name(old), Renamed::Under(name(new))))
        .chain(outside.iter().map(|old| (name(old), Renamed::Outside)))
        .collect();
    (SpacePrefixes::Answered, renamed)
}

fn answers(of: PrefixMove, spaces: Vec<(Space, (SpacePrefixes, SpaceRenamed))>) -> PrefixAnswers {
    let mut all = PrefixAnswers::new(of);
    for (space, (prefixes, renamed)) in spaces {
        all.answer(space, prefixes, renamed);
    }
    all
}

fn moves_of(plan: &PrefixPlan) -> Vec<(Space, String, String)> {
    plan.moves
        .iter()
        .map(|m| {
            (
                m.space().clone(),
                m.from_name().to_string(),
                m.to_name().to_string(),
            )
        })
        .collect()
}

fn triple(space: &Space, from: &str, to: &str) -> (Space, String, String) {
    (space.clone(), from.to_owned(), to.to_owned())
}

#[test]
fn the_names_under_a_prefix_in_every_space_are_moved_with_the_prefix_itself() {
    let pinned = pinned(
        &["src/a", "src/a/x.rs", "src/a/y.rs", "src/ab.rs"],
        &["src::a::f"],
    );
    let of = moving("src/a", "src/b");
    let (file, elsewhere) = (file_space(), other());
    let answers = answers(
        of,
        vec![
            (
                file.clone(),
                answered(
                    &[
                        ("src/a", "src/b"),
                        ("src/a/x.rs", "src/b/x.rs"),
                        ("src/a/y.rs", "src/b/y.rs"),
                    ],
                    &["src/ab.rs"],
                ),
            ),
            (
                elsewhere.clone(),
                answered(&[("src::a::f", "src::b::f")], &[]),
            ),
        ],
    );
    let plan = plan_prefix_moves(&pinned, &answers).unwrap();
    // The prefix itself is a name under it; each space's names are in the order of
    // the names, and the spaces in the order of their names.
    assert_eq!(
        moves_of(&plan),
        [
            triple(&file, "src/a", "src/b"),
            triple(&file, "src/a/x.rs", "src/b/x.rs"),
            triple(&file, "src/a/y.rs", "src/b/y.rs"),
            triple(&elsewhere, "src::a::f", "src::b::f"),
        ]
    );
    assert!(plan.refused.is_empty());
    assert!(plan.not_asked.is_empty());
    assert_eq!(plan.prefix, NamePrefix::new("src/a").unwrap());
}

#[test]
fn nothing_under_the_prefix_is_refused_with_why_for_each_space() {
    let pinned = pinned(&["src/a.rs"], &["src::a"]);
    let of = || moving("lib", "src2");
    // Each space says why: read and none under, no prefix of it, or no adapter.
    let refused = plan_prefix_moves(
        &pinned,
        &answers(
            of(),
            vec![
                (file_space(), answered(&[], &["src/a.rs"])),
                (
                    other(),
                    (
                        SpacePrefixes::NotUnder(why("`lib` is no module path")),
                        BTreeMap::new(),
                    ),
                ),
            ],
        ),
    )
    .unwrap_err();
    assert_eq!(
        refused,
        PrefixRefusal::NothingUnder {
            prefix: NamePrefix::new("lib").unwrap(),
            spaces: vec![
                (file_space(), NothingWhy::NoneUnder),
                (
                    other(),
                    NothingWhy::NotUnder(why("`lib` is no module path"))
                ),
            ],
        }
    );
    assert_eq!(
        refused.to_string(),
        "no name of an approval that stands is under `lib`: `file`: none of its names is; \
         `other`: `lib` is no module path"
    );

    // A space with no answer cannot be told: the refusal names it as such.
    let refused = plan_prefix_moves(
        &pinned,
        &answers(of(), vec![(file_space(), answered(&[], &["src/a.rs"]))]),
    )
    .unwrap_err();
    assert_eq!(
        refused,
        PrefixRefusal::NothingUnder {
            prefix: NamePrefix::new("lib").unwrap(),
            spaces: vec![
                (file_space(), NothingWhy::NoneUnder),
                (other(), NothingWhy::NoPrefixes),
            ],
        }
    );

    // No approval pins a name at all: nothing under it, in no space.
    let none = PinnedNames::of(&ledger(Vec::new()));
    let refused = plan_prefix_moves(&none, &PrefixAnswers::new(of())).unwrap_err();
    assert_eq!(
        refused.to_string(),
        "no name of an approval that stands is under `lib`"
    );
}

#[test]
fn a_space_the_prefix_is_none_of_is_not_reported_and_one_with_no_adapter_is() {
    let pinned = pinned(&["src/a/x.rs"], &["src::a::f"]);
    // `other` reads the prefix as none of its names: no name of it is under it, and
    // the answer does not mention it.
    let plan = plan_prefix_moves(
        &pinned,
        &answers(
            moving("src/a", "src/b"),
            vec![
                (file_space(), answered(&[("src/a/x.rs", "src/b/x.rs")], &[])),
                (
                    other(),
                    (
                        SpacePrefixes::NotUnder(why("no module path")),
                        BTreeMap::new(),
                    ),
                ),
            ],
        ),
    )
    .unwrap();
    assert_eq!(plan.moves.len(), 1);
    assert!(plan.not_asked.is_empty());

    // A space that takes no prefix, or was not answered, may have names under it that
    // nobody can tell: it is named in the answer, and the others still move.
    for taken in [
        vec![(other(), (SpacePrefixes::NoPrefixes, BTreeMap::new()))],
        Vec::new(),
    ] {
        let mut all = vec![(file_space(), answered(&[("src/a/x.rs", "src/b/x.rs")], &[]))];
        all.extend(taken);
        let plan = plan_prefix_moves(&pinned, &answers(moving("src/a", "src/b"), all)).unwrap();
        assert_eq!(plan.moves.len(), 1);
        assert_eq!(plan.not_asked, [other()]);
    }
}

#[test]
fn a_name_that_cannot_be_moved_is_refused_by_name_with_the_rule_that_refused_it() {
    let pinned = pinned(&["dup", "p/x", "p/y", "p/z"], &["dup"]);
    let of = moving("p", "q");
    let unbuildable = why("a part of the name is too long");
    let (file, elsewhere) = (file_space(), other());
    let mut renamed = answered(&[("p/x", "q/x")], &[]).1;
    renamed.insert(name("p/y"), Renamed::Unbuildable(unbuildable.clone()));
    // The adapter answers the same name for `p/z` (a bug of it, or a `to` that reads
    // as `from`): no move.
    renamed.insert(name("p/z"), Renamed::Under(name("p/z")));
    // `dup` is under the prefix in both spaces, and pins are read as it in two.
    renamed.insert(name("dup"), Renamed::Under(name("dup2")));
    let plan = plan_prefix_moves(
        &pinned,
        &answers(
            of,
            vec![
                (file.clone(), (SpacePrefixes::Answered, renamed)),
                (elsewhere.clone(), answered(&[("dup", "dup2")], &[])),
            ],
        ),
    )
    .unwrap();
    assert_eq!(moves_of(&plan), [triple(&file, "p/x", "q/x")]);
    let refused: Vec<(String, NamedWhy)> = plan
        .refused
        .iter()
        .map(|r| (r.name.to_string(), r.why.clone()))
        .collect();
    assert_eq!(
        refused,
        [
            (
                "dup".to_owned(),
                NamedWhy::Move(MovedRefusal::InSpaces {
                    name: name("dup"),
                    spaces: vec![file.clone(), elsewhere.clone()],
                })
            ),
            (
                "p/y".to_owned(),
                NamedWhy::Unbuildable {
                    space: file.clone(),
                    why: unbuildable
                }
            ),
            (
                "p/z".to_owned(),
                NamedWhy::Move(MovedRefusal::NotAMove(MovedProblem::SameName {
                    name: name("p/z")
                }))
            ),
        ]
    );
    // What the refusal is about, where it is of one space.
    assert_eq!(plan.refused[0].space(), None);
    assert_eq!(plan.refused[1].space(), Some(&file));
    assert_eq!(plan.refused[2].space(), None);
}

#[test]
fn a_name_the_adapter_gave_no_answer_for_is_refused_and_not_read_as_outside() {
    let pinned = pinned(&["p/x", "p/y"], &[]);
    let plan = plan_prefix_moves(
        &pinned,
        &answers(
            moving("p", "q"),
            vec![(file_space(), answered(&[("p/x", "q/x")], &[]))],
        ),
    )
    .unwrap();
    assert_eq!(plan.moves.len(), 1);
    assert_eq!(
        plan.refused,
        [NamedRefusal {
            name: name("p/y"),
            why: NamedWhy::NoAnswer {
                space: file_space()
            }
        }]
    );
}

fn readings_of(there: &[&str]) -> ArtifactReadings {
    let present = ArtifactReading::of_bytes(b"x", &Default::default(), None);
    let mut all = ArtifactReadings::new();
    for old_or_new in ["p/x", "p/y", "q/x", "q/y", "a/x", "a/b/x", "a/b/b/x", "a/y"] {
        let reading = if there.contains(&old_or_new) {
            present.clone()
        } else {
            ArtifactReading::absent()
        };
        all.insert(file_space(), name(old_or_new), reading);
    }
    all
}

#[test]
fn the_refusals_of_both_steps_are_one_answer() {
    let pinned = pinned(&["p/x", "p/y", "p/z"], &[]);
    let mut renamed = answered(&[("p/x", "q/x"), ("p/y", "q/y")], &[]).1;
    renamed.insert(name("p/z"), Renamed::Unbuildable(why("too long")));
    let plan = plan_prefix_moves(
        &pinned,
        &answers(
            moving("p", "q"),
            vec![(file_space(), (SpacePrefixes::Answered, renamed))],
        ),
    )
    .unwrap();

    // `p/x` is still there, and `q/y` is not: the plan's refusal and both moves' own.
    let refused = frame_prefix_moves(plan.clone(), &readings_of(&["p/x", "q/x"])).unwrap_err();
    let PrefixRefusal::Names(names) = refused else {
        panic!("names are refused");
    };
    let got: Vec<(String, bool)> = names
        .iter()
        .map(|r| {
            (
                r.name.to_string(),
                matches!(
                    r.why,
                    NamedWhy::Move(
                        MovedRefusal::FromPresent { .. } | MovedRefusal::ToAbsent { .. }
                    )
                ),
            )
        })
        .collect();
    assert_eq!(
        got,
        [
            ("p/z".to_owned(), false),
            ("p/x".to_owned(), true),
            ("p/y".to_owned(), true)
        ]
    );
    assert!(matches!(
        names.first().why,
        NamedWhy::Unbuildable { ref why, .. } if why.as_str() == "too long"
    ));
    assert!(names.iter().all(|r| r.space() == Some(&file_space())));

    // Nothing refused: every move, as `frame_moved` framed it.
    let ok = PrefixPlan {
        refused: Vec::new(),
        ..plan
    };
    let moves = frame_prefix_moves(ok, &readings_of(&["q/x", "q/y"])).unwrap();
    assert_eq!(
        moves.iter().map(MovedOf::from).collect::<Vec<_>>(),
        [file_artifact("p/x"), file_artifact("p/y")]
    );
}

#[test]
fn a_rename_into_itself_is_held_against_what_is_there_and_one_that_does_not_overlap_stands() {
    // `a` -> `a/b` moves `a/x` to `a/b/x`, and `a/b/x` (another name pinned) to
    // `a/b/b/x`: the second old name is where the first one went.
    let overlapping = pinned(&["a/x", "a/b/x"], &[]);
    let plan = plan_prefix_moves(
        &overlapping,
        &answers(
            moving("a", "a/b"),
            vec![(
                file_space(),
                answered(&[("a/x", "a/b/x"), ("a/b/x", "a/b/b/x")], &[]),
            )],
        ),
    )
    .unwrap();
    // After the move, `a/b/x` and `a/b/b/x` are there and `a/x` is not.
    let refused = frame_prefix_moves(plan, &readings_of(&["a/b/x", "a/b/b/x"])).unwrap_err();
    let PrefixRefusal::Names(names) = refused else {
        panic!("a name is refused");
    };
    assert_eq!(names.len(), 1);
    assert_eq!(names.first().name, name("a/b/x"));
    assert!(matches!(
        names.first().why,
        NamedWhy::Move(MovedRefusal::FromPresent { .. })
    ));

    // The same rename of a directory with no name below the new prefix stands.
    let disjoint = pinned(&["a/x", "a/y"], &[]);
    let plan = plan_prefix_moves(
        &disjoint,
        &answers(
            moving("a", "a/b"),
            vec![(
                file_space(),
                answered(&[("a/x", "a/b/x"), ("a/y", "a/b/y")], &[]),
            )],
        ),
    )
    .unwrap();
    let mut readings = readings_of(&["a/b/x"]);
    readings.insert(
        file_space(),
        name("a/b/y"),
        ArtifactReading::of_bytes(b"y", &Default::default(), None),
    );
    let moves = frame_prefix_moves(plan, &readings).unwrap();
    assert_eq!(moves.len(), 2);
}

#[test]
fn the_space_of_a_name_is_the_one_a_pin_is_read_as_it_in() {
    let both = pinned(&["only-file", "both"], &["only-other", "both"]);
    assert_eq!(both.space_of(&name("only-file")), Ok(file_space()));
    assert_eq!(both.space_of(&name("only-other")), Ok(other()));
    assert_eq!(
        both.space_of(&name("none")),
        Err(MovedRefusal::NotKnown { name: name("none") })
    );
    assert_eq!(
        both.space_of(&name("both")),
        Err(MovedRefusal::InSpaces {
            name: name("both"),
            spaces: vec![file_space(), other()],
        })
    );
    // The gone names ask the same rule: a name gone in both spaces says which.
    let readings: ArtifactReadings = [
        (file_space(), name("both"), ArtifactReading::absent()),
        (other(), name("both"), ArtifactReading::absent()),
    ]
    .into_iter()
    .collect();
    let out = both.gone(&readings).suggest(BTreeMap::new());
    let in_spaces: Vec<BTreeSet<Space>> = out
        .absent
        .iter()
        .map(|s| s.gone().in_spaces().clone())
        .collect();
    let two = BTreeSet::from([file_space(), other()]);
    assert_eq!(
        in_spaces,
        [two.clone(), two],
        "both pins of `both` are gone in two spaces"
    );
}

#[test]
fn a_prefix_is_a_name_that_names_nothing_and_a_move_to_itself_is_no_move() {
    assert_eq!(NamePrefix::new("").unwrap_err(), ArtifactNameProblem::Empty);
    assert!(NamePrefix::new("a\nb").is_err());
    assert!(NamePrefix::new("src::m").is_ok());
    let same = NamePrefix::new("src/a").unwrap();
    assert_eq!(
        PrefixMove::new(same.clone(), same).unwrap_err().to_string(),
        "`src/a` is moved to itself"
    );
}
