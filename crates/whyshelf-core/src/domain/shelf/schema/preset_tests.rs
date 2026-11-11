//! Expansion of presets and the default ([`expand`]).

use std::collections::BTreeSet;

use super::*;
use crate::domain::shelf::artifact::Space;

fn pname(text: &str) -> PresetName {
    PresetName::new(text).unwrap()
}

fn values(space: Option<&str>, compare: Option<&str>, means: Option<Propagation>) -> PresetValues {
    PresetValues {
        space: space.map(str::to_owned),
        compare: compare.map(str::to_owned),
        ignore: None,
        means,
    }
}

/// The built-in presets of these tests: `file` (space `file`, `bytes`, `depends`).
fn builtins() -> BuiltinPresets {
    BuiltinPresets::from([(
        pname("file"),
        values(Some("file"), Some("bytes"), Some(Propagation::Depends)),
    )])
}

/// The spaces offered in these tests: `file` and `other`.
fn spaces() -> BTreeSet<Space> {
    BTreeSet::from([Space::new("file").unwrap(), Space::new("other").unwrap()])
}

fn preset(name: &str, from: Option<&str>, values: PresetValues) -> PresetDecl {
    PresetDecl {
        name: name.to_owned(),
        from: from.map(str::to_owned),
        values,
    }
}

/// A link to artifacts named `name` that writes nothing but `preset`.
fn artifact(name: &str, preset: Option<&str>) -> WrittenLinkDecl {
    WrittenLinkDecl {
        name: name.to_owned(),
        target: LinkTargetDecl::Artifact,
        cardinality: Cardinality::Many,
        propagation: None,
        preset: preset.map(str::to_owned),
        space: None,
        compare: None,
        ignore: None,
        cover: None,
    }
}

fn into_book(name: &str, target: &str) -> WrittenLinkDecl {
    WrittenLinkDecl {
        target: LinkTargetDecl::Book(target.to_owned()),
        propagation: Some(Propagation::Depends),
        ..artifact(name, None)
    }
}

/// A declaration of one book `b` with these links, presets and default.
fn shelf(
    links: Vec<WrittenLinkDecl>,
    presets: Vec<PresetDecl>,
    default: Option<&str>,
) -> ShelfDecl {
    ShelfDecl {
        books: vec![BookSchemaDecl {
            name: "b".to_owned(),
            dir: "b".to_owned(),
            scope: Default::default(),
            key: KeyPatternDecl::Slug,
            description: None,
            fields: Vec::new(),
            links,
        }],
        presets,
        defaults: DefaultsDecl {
            artifact: default.map(str::to_owned),
        },
    }
}

/// The expanded links of the one book.
fn links(decl: ShelfDecl) -> Vec<LinkEntryDecl> {
    let expanded = expand(decl, &builtins(), &spaces()).unwrap();
    expanded.books()[0].links.clone()
}

fn problems(decl: ShelfDecl) -> Vec<SchemaProblem> {
    expand(decl, &builtins(), &spaces()).unwrap_err().into_vec()
}

fn compare(kind: &str, ignore: &[&str]) -> Option<CompareDecl> {
    Some(CompareDecl {
        kind: kind.to_owned(),
        ignore: ignore.iter().map(|p| (*p).to_owned()).collect(),
    })
}

#[test]
fn a_link_takes_the_values_of_its_preset_and_each_value_it_writes_wins() {
    let mut own = artifact("tests", Some("file"));
    own.propagation = Some(Propagation::Informs);
    own.compare = Some("lines".to_owned());
    own.ignore = Some(vec!["//".to_owned()]);
    let expanded = links(shelf(
        vec![artifact("plain", Some("file")), own],
        Vec::new(),
        None,
    ));
    assert_eq!(expanded[0].space.as_deref(), Some("file"));
    assert_eq!(expanded[0].compare, compare("bytes", &[]));
    assert_eq!(expanded[0].propagation, Propagation::Depends);
    assert_eq!(expanded[1].space.as_deref(), Some("file"));
    assert_eq!(expanded[1].compare, compare("lines", &["//"]));
    assert_eq!(expanded[1].propagation, Propagation::Informs);
}

#[test]
fn a_preset_of_the_project_takes_its_own_values_then_those_of_its_from_chain() {
    let lines = preset(
        "lines",
        Some("file"),
        PresetValues {
            compare: Some("lines".to_owned()),
            ignore: Some(vec!["#".to_owned()]),
            ..PresetValues::default()
        },
    );
    let tests = preset(
        "tests",
        Some("lines"),
        values(None, None, Some(Propagation::Informs)),
    );
    let expanded = links(shelf(
        vec![artifact("tests", Some("tests"))],
        // Declared before the preset it comes from: the order does not count.
        vec![tests, lines],
        None,
    ));
    assert_eq!(expanded[0].space.as_deref(), Some("file"));
    assert_eq!(expanded[0].compare, compare("lines", &["#"]));
    assert_eq!(expanded[0].propagation, Propagation::Informs);
}

#[test]
fn a_preset_of_the_project_replaces_a_built_in_one_of_its_name() {
    let file = preset("file", None, values(Some("other"), Some("presence"), None));
    let mut link = artifact("tests", Some("file"));
    link.propagation = Some(Propagation::Depends);
    let expanded = links(shelf(vec![link], vec![file], None));
    assert_eq!(expanded[0].space.as_deref(), Some("other"));
    assert_eq!(expanded[0].compare, compare("presence", &[]));
}

#[test]
fn the_default_is_used_only_by_a_link_to_artifacts_that_names_neither_a_preset_nor_a_space() {
    let mut spaced = artifact("spaced", None);
    spaced.space = Some("other".to_owned());
    spaced.propagation = Some(Propagation::Informs);
    let lines = preset("lines", Some("file"), values(None, Some("lines"), None));
    let decl = shelf(
        vec![
            artifact("bare", None),
            spaced,
            artifact("named", Some("lines")),
            into_book("up", "b"),
        ],
        vec![lines],
        Some("file"),
    );
    let expanded = expand(decl, &builtins(), &spaces()).unwrap();
    let book = &expanded.books()[0];
    assert_eq!(book.links[0].space.as_deref(), Some("file"));
    assert_eq!(book.links[0].compare, compare("bytes", &[]));
    assert_eq!(
        expanded.origin("b", "bare"),
        Some(PresetOrigin::Default(pname("file")))
    );
    // A space written: no preset, the default included.
    assert_eq!(book.links[1].space.as_deref(), Some("other"));
    assert_eq!(book.links[1].compare, None);
    assert_eq!(expanded.origin("b", "spaced"), None);
    // A preset named: that one, not the default.
    assert_eq!(book.links[2].compare, compare("lines", &[]));
    assert_eq!(
        expanded.origin("b", "named"),
        Some(PresetOrigin::Named(pname("lines")))
    );
    // A link into a book takes no default.
    assert_eq!(book.links[3].space, None);
    assert_eq!(book.links[3].compare, None);
    assert_eq!(expanded.origin("b", "up"), None);
}

#[test]
fn without_a_default_a_link_that_names_no_space_is_refused_as_before() {
    let mut link = artifact("tests", None);
    link.propagation = Some(Propagation::Depends);
    let expanded = expand(shelf(vec![link], Vec::new(), None), &builtins(), &spaces()).unwrap();
    assert_eq!(expanded.books()[0].links[0].space, None);
    let (books, _) = expanded.into_parts();
    let problems = Schema::validate(books, &BTreeSet::from([Space::new("file").unwrap()]))
        .unwrap_err()
        .into_vec();
    assert!(
        matches!(&problems[..], [SchemaProblem::NoSpace { link, .. }] if link.as_str() == "tests"),
        "{problems:?}"
    );
}

#[test]
fn a_link_written_in_full_expands_to_itself() {
    let full = LinkEntryDecl {
        name: "tests".to_owned(),
        target: LinkTargetDecl::Artifact,
        cardinality: Cardinality::One,
        propagation: Propagation::Informs,
        space: Some("file".to_owned()),
        compare: compare("lines", &["//"]),
        cover: None,
    };
    let book = BookSchemaDecl {
        name: "b".to_owned(),
        dir: "b".to_owned(),
        scope: Default::default(),
        key: KeyPatternDecl::Slug,
        description: None,
        fields: Vec::new(),
        links: vec![full],
    };
    let expanded = expand(
        ShelfDecl::of_books(vec![book.clone()]),
        &builtins(),
        &spaces(),
    )
    .unwrap();
    assert_eq!(expanded.books(), [book]);
}

#[test]
fn a_name_of_a_preset_that_is_not_there_is_a_problem_where_it_is_written() {
    let found = problems(shelf(
        vec![artifact("tests", Some("nope"))],
        vec![preset("mine", Some("gone"), PresetValues::default())],
        Some("missing"),
    ));
    assert_eq!(
        found,
        [
            SchemaProblem::UnknownPreset {
                place: PresetPlace::From {
                    preset: "mine".to_owned()
                },
                name: "gone".to_owned(),
            },
            SchemaProblem::UnknownPreset {
                place: PresetPlace::Default,
                name: "missing".to_owned(),
            },
            SchemaProblem::UnknownPreset {
                place: PresetPlace::Link {
                    book: "b".to_owned(),
                    link: "tests".to_owned()
                },
                name: "nope".to_owned(),
            },
        ]
    );
    assert!(found[2].to_string().contains("book `b`: the link `tests`"));
}

#[test]
fn a_default_that_is_not_there_is_told_once_and_not_on_each_link() {
    let found = problems(shelf(
        vec![artifact("one", None), artifact("two", None)],
        Vec::new(),
        Some("missing"),
    ));
    assert_eq!(
        found,
        [SchemaProblem::UnknownPreset {
            place: PresetPlace::Default,
            name: "missing".to_owned(),
        }]
    );
}

#[test]
fn a_cycle_of_from_is_one_problem_and_every_link_on_it_is_left_to_it() {
    let found = problems(shelf(
        vec![artifact("tests", Some("b"))],
        vec![
            preset("a", Some("b"), PresetValues::default()),
            preset("b", Some("a"), PresetValues::default()),
            preset("self", Some("self"), PresetValues::default()),
        ],
        None,
    ));
    assert_eq!(
        found,
        [
            SchemaProblem::PresetCycle {
                presets: NonEmptyVec::new(pname("a"), vec![pname("b")]),
            },
            // A `from` of its own name is no cycle: it names the built-in one of
            // the name, and there is none.
            SchemaProblem::UnknownPreset {
                place: PresetPlace::From {
                    preset: "self".to_owned()
                },
                name: "self".to_owned(),
            },
        ]
    );
    assert!(found[0].to_string().contains("`a` -> `b` -> `a`"));
}

#[test]
fn a_preset_declared_twice_or_misnamed_is_a_problem() {
    let found = problems(shelf(
        Vec::new(),
        vec![
            preset("twice", None, PresetValues::default()),
            preset("twice", None, PresetValues::default()),
            preset("Bad Name", None, PresetValues::default()),
        ],
        None,
    ));
    assert!(
        matches!(
            &found[..],
            [
                SchemaProblem::DuplicatePreset { name },
                SchemaProblem::BadName {
                    place: NamePlace::Preset,
                    ..
                },
            ] if name.as_str() == "twice"
        ),
        "{found:?}"
    );
}

#[test]
fn a_preset_on_a_link_into_a_book_is_a_problem() {
    let mut link = into_book("up", "b");
    link.preset = Some("file".to_owned());
    assert_eq!(
        problems(shelf(vec![link], Vec::new(), None)),
        [SchemaProblem::PresetOnBookLink {
            book: "b".to_owned(),
            link: "up".to_owned(),
        }]
    );
}

#[test]
fn a_preset_that_replaces_is_a_value_it_cannot_hold() {
    let found = problems(shelf(
        vec![artifact("tests", Some("swap"))],
        vec![preset(
            "swap",
            Some("file"),
            values(None, None, Some(Propagation::Replaces)),
        )],
        None,
    ));
    // The link that names it is left to the problem of the preset.
    assert_eq!(
        found,
        [SchemaProblem::PresetReplaces {
            preset: "swap".to_owned()
        }]
    );
}

#[test]
fn a_link_left_with_no_means_is_a_problem() {
    let mut up = into_book("up", "b");
    up.propagation = None;
    let found = problems(shelf(
        vec![up, artifact("tests", Some("bare"))],
        vec![preset("bare", None, values(Some("file"), None, None))],
        None,
    ));
    assert_eq!(
        found,
        [
            SchemaProblem::NoMeans {
                book: "b".to_owned(),
                link: "up".to_owned(),
            },
            SchemaProblem::NoMeans {
                book: "b".to_owned(),
                link: "tests".to_owned(),
            },
        ]
    );
}

#[test]
fn a_declaration_with_a_default_is_the_schema_of_one_that_writes_the_space() {
    let spaces = BTreeSet::from([Space::new("file").unwrap()]);
    let written = Schema::declared(
        shelf(vec![artifact("tests", None)], Vec::new(), Some("file")),
        &builtins(),
        &spaces,
    )
    .unwrap();
    let mut full = artifact("tests", None);
    full.space = Some("file".to_owned());
    full.propagation = Some(Propagation::Depends);
    let plain =
        Schema::declared(shelf(vec![full], Vec::new(), None), &builtins(), &spaces).unwrap();
    let (a, b) = (written.book("b").unwrap(), plain.book("b").unwrap());
    assert_eq!(a.all_links(), b.all_links());
    assert_eq!(
        a.preset_of("tests"),
        Some(&PresetOrigin::Default(pname("file")))
    );
    assert_eq!(b.preset_of("tests"), None);
}

/// A preset `lines` from `file` that compares lines and leaves out `//`.
fn lines_preset() -> PresetDecl {
    preset(
        "lines",
        Some("file"),
        PresetValues {
            compare: Some("lines".to_owned()),
            ignore: Some(vec!["//".to_owned()]),
            ..PresetValues::default()
        },
    )
}

#[test]
fn compare_and_its_ignore_are_one_value() {
    // `compare` written: none of the preset's `ignore` comes with the preset's.
    let mut compared = artifact("compared", Some("lines"));
    compared.compare = Some("lines".to_owned());
    // `compare` written with `ignore`: those of the link.
    let mut both = artifact("both", Some("lines"));
    both.compare = Some("lines".to_owned());
    both.ignore = Some(vec!["#".to_owned()]);
    // `ignore` alone: the preset's `compare`, with the link's `ignore`.
    let mut ignored = artifact("ignored", Some("lines"));
    ignored.ignore = Some(vec!["#".to_owned()]);
    // `compare = "bytes"` over a preset that ignores: bytes, and nothing ignored.
    let mut bytes = artifact("bytes", Some("lines"));
    bytes.compare = Some("bytes".to_owned());
    let expanded = links(shelf(
        vec![compared, both, ignored, bytes],
        vec![lines_preset()],
        None,
    ));
    let compares: Vec<Option<CompareDecl>> = expanded.into_iter().map(|l| l.compare).collect();
    assert_eq!(
        compares,
        [
            compare("lines", &[]),
            compare("lines", &["#"]),
            compare("lines", &["#"]),
            compare("bytes", &[]),
        ]
    );
    // The same between presets: `bytes` from `lines` takes no `ignore`.
    let bytes = preset("plain", Some("lines"), values(None, Some("bytes"), None));
    let expanded = links(shelf(
        vec![artifact("tests", Some("plain"))],
        vec![lines_preset(), bytes],
        None,
    ));
    assert_eq!(expanded[0].compare, compare("bytes", &[]));
}

#[test]
fn a_from_of_the_preset_s_own_name_refines_the_built_in_one() {
    let refined = preset(
        "file",
        Some("file"),
        values(None, None, Some(Propagation::Informs)),
    );
    let expanded = links(shelf(
        vec![artifact("tests", Some("file"))],
        vec![refined],
        None,
    ));
    assert_eq!(expanded[0].space.as_deref(), Some("file"));
    assert_eq!(expanded[0].compare, compare("bytes", &[]));
    assert_eq!(expanded[0].propagation, Propagation::Informs);
}

#[test]
fn a_preset_of_a_built_in_name_without_from_takes_none_of_its_values() {
    let replaced = preset("file", None, values(Some("other"), None, None));
    let found = problems(shelf(
        vec![artifact("tests", Some("file"))],
        vec![replaced],
        None,
    ));
    // No `means` and no `compare` of the built-in `file` come with it.
    assert_eq!(
        found,
        [SchemaProblem::NoMeans {
            book: "b".to_owned(),
            link: "tests".to_owned(),
        }]
    );
    let mut link = artifact("tests", Some("file"));
    link.propagation = Some(Propagation::Depends);
    let expanded = links(shelf(
        vec![link],
        vec![preset("file", None, values(Some("other"), None, None))],
        None,
    ));
    assert_eq!(expanded[0].space.as_deref(), Some("other"));
    assert_eq!(expanded[0].compare, None);
}

#[test]
fn a_chain_that_leads_into_a_cycle_tells_the_cycle_once_from_the_one_declared_first() {
    let found = problems(shelf(
        vec![artifact("tests", Some("x"))],
        vec![
            preset("x", Some("b"), PresetValues::default()),
            preset("b", Some("a"), PresetValues::default()),
            preset("a", Some("b"), PresetValues::default()),
        ],
        None,
    ));
    // `x` leads into the cycle and is not on it; the link that names it is left out.
    assert_eq!(
        found,
        [SchemaProblem::PresetCycle {
            presets: NonEmptyVec::new(pname("b"), vec![pname("a")]),
        }]
    );
}

#[test]
fn the_values_of_a_preset_are_checked_on_it_whether_a_link_uses_it_or_not() {
    let found = problems(shelf(
        Vec::new(),
        vec![
            preset("far", None, values(Some("nowhere"), None, None)),
            preset("bad", None, values(Some("Not A Space"), None, None)),
            preset("odd", None, values(None, Some("words"), None)),
            preset(
                "blank",
                Some("file"),
                PresetValues {
                    // `bytes` from `file`, and an `ignore` it cannot take.
                    ignore: Some(vec!["//".to_owned()]),
                    ..PresetValues::default()
                },
            ),
            preset(
                "spaced",
                None,
                PresetValues {
                    compare: Some("lines".to_owned()),
                    ignore: Some(vec!["a b".to_owned()]),
                    ..PresetValues::default()
                },
            ),
        ],
        None,
    ));
    let named: Vec<&str> = found
        .iter()
        .map(|p| match p {
            SchemaProblem::PresetUnknownSpace { preset, .. }
            | SchemaProblem::PresetBadSpace { preset, .. }
            | SchemaProblem::PresetBadComparison { preset, .. } => preset.as_str(),
            other => panic!("a problem of a preset's value: {other:?}"),
        })
        .collect();
    assert_eq!(named, ["far", "bad", "odd", "blank", "spaced"]);
    assert!(matches!(
        &found[3],
        SchemaProblem::PresetBadComparison {
            why: crate::domain::shelf::artifact::ComparisonProblem::IgnoreWithBytes,
            ..
        }
    ));
}

#[test]
fn a_link_on_a_preset_with_a_bad_value_tells_only_the_preset() {
    let found = problems(shelf(
        vec![artifact("tests", Some("far"))],
        vec![preset(
            "far",
            Some("file"),
            values(Some("nowhere"), None, None),
        )],
        None,
    ));
    assert!(
        matches!(&found[..], [SchemaProblem::PresetUnknownSpace { preset, .. }] if preset == "far"),
        "{found:?}"
    );
}

#[test]
fn the_problems_of_expansion_and_of_the_books_are_told_at_once() {
    let mut decl = shelf(
        vec![artifact("tests", Some("nope")), artifact("plain", None)],
        Vec::new(),
        Some("file"),
    );
    // A problem of the books, apart from any preset.
    decl.books.push(BookSchemaDecl {
        name: "Bad Book".to_owned(),
        dir: "c".to_owned(),
        scope: Default::default(),
        key: KeyPatternDecl::Slug,
        description: None,
        fields: Vec::new(),
        links: Vec::new(),
    });
    let found = Schema::declared(decl, &builtins(), &spaces())
        .unwrap_err()
        .into_vec();
    assert!(
        matches!(
            &found[..],
            [
                SchemaProblem::UnknownPreset { name, .. },
                SchemaProblem::BadName {
                    place: NamePlace::Book,
                    ..
                },
            ] if name == "nope"
        ),
        "{found:?}"
    );
}
