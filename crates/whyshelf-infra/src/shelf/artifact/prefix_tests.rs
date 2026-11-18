//! Tests of what each adapter says of a move of a prefix
//! ([`ArtifactSource::renamed`]): that a prefix ends only at a boundary of the space's
//! own grammar of names, that the new name is built by it, and that a prefix or a name
//! the grammar cannot read is told as the domain wants it (not under, outside,
//! unbuildable), never by looking for a text in a name.

use std::path::Path;

use whyshelf_core::app::shelf::ArtifactSource;
use whyshelf_core::domain::shelf::artifact::{
    ArtifactName, NamePrefix, PrefixAnswers, PrefixAsks, PrefixMove, Renamed, Space, SpacePrefixes,
};

use super::ArtifactSpaces;
use super::file::FileSpace;
use super::js_test::JsTestSpace;
use super::rust_item::RustItemSpace;
use super::rust_test::RustTestSpace;

fn space(text: &str) -> Space {
    Space::new(text).unwrap()
}

fn name(text: &str) -> ArtifactName {
    ArtifactName::new(text).unwrap()
}

fn source() -> ArtifactSpaces {
    let root = Path::new(".");
    ArtifactSpaces::of_project(
        FileSpace::new(root.to_path_buf()),
        RustItemSpace::new(root.to_path_buf(), None),
        RustTestSpace::new(root.to_path_buf()),
        JsTestSpace::new(root.to_path_buf()),
    )
}

fn moving(from: &str, to: &str) -> PrefixMove {
    PrefixMove::new(NamePrefix::new(from).unwrap(), NamePrefix::new(to).unwrap()).unwrap()
}

/// The answers for these names of `in_space`, for the move `from` to `to`.
fn ask(in_space: &str, names: &[&str], from: &str, to: &str) -> PrefixAnswers {
    let mut asks = PrefixAsks::new();
    for text in names {
        asks.ask(space(in_space), name(text));
    }
    source().renamed(&asks, &moving(from, to)).unwrap()
}

/// What the space's adapter says of each name, in the order given.
fn renamed(answers: &PrefixAnswers, in_space: &str, names: &[&str]) -> Vec<Renamed> {
    names
        .iter()
        .map(|text| {
            answers
                .renamed(&space(in_space), &name(text))
                .cloned()
                .unwrap_or_else(|| panic!("no answer for {text}"))
        })
        .collect()
}

fn under(text: &str) -> Renamed {
    Renamed::Under(name(text))
}

fn unbuildable_why(answer: &Renamed) -> &str {
    match answer {
        Renamed::Unbuildable(why) => why.as_str(),
        other => panic!("not unbuildable: {other:?}"),
    }
}

fn not_under_why(answers: &PrefixAnswers, in_space: &str) -> String {
    match answers.prefixes_of(&space(in_space)) {
        Some(SpacePrefixes::NotUnder(why)) => why.to_string(),
        other => panic!("not `NotUnder`: {other:?}"),
    }
}

// The space `file`.

#[test]
fn a_path_is_under_a_prefix_at_a_part_and_never_in_the_middle_of_one() {
    let names = [
        "src/a",
        "src/a/x.rs",
        "src/a/deep/y.rs",
        "src/ab.rs",
        "src/a.rs",
        "lib/src/a/x.rs",
        "/src/a/x.rs",
    ];
    let answers = ask("file", &names, "src/a", "src/b");
    assert_eq!(
        answers.prefixes_of(&space("file")),
        Some(&SpacePrefixes::Answered)
    );
    assert_eq!(
        renamed(&answers, "file", &names),
        [
            under("src/b"),
            under("src/b/x.rs"),
            under("src/b/deep/y.rs"),
            Renamed::Outside,
            Renamed::Outside,
            Renamed::Outside,
            Renamed::Outside,
        ]
    );
}

#[test]
fn a_file_is_under_the_prefix_of_its_own_path_and_moves_to_a_deeper_or_shallower_one() {
    let names = ["a.rs", "docs/a.rs"];
    let answers = ask("file", &names, "docs", "a/b/c");
    assert_eq!(
        renamed(&answers, "file", &names),
        [Renamed::Outside, under("a/b/c/a.rs")]
    );
    let answers = ask("file", &names, "docs/a.rs", "z.rs");
    assert_eq!(
        renamed(&answers, "file", &names),
        [Renamed::Outside, under("z.rs")]
    );
}

#[test]
fn a_prefix_that_is_no_path_is_none_of_the_names_and_a_new_one_makes_each_unbuildable() {
    // `from` that is no path under the root: no name is under it.
    for from in ["src//a", "../a", "src\\a", "/a", "/", "src//"] {
        let answers = ask("file", &["src/a/x.rs"], from, "src/b");
        assert!(
            not_under_why(&answers, "file").contains("is no path under the root"),
            "{from}"
        );
    }
    // `to` that is none: each name under `from` is unbuildable, the others are not
    // under it.
    let names = ["src/a/x.rs", "src/c.rs"];
    let answers = ask("file", &names, "src/a", "../out");
    assert_eq!(
        answers.prefixes_of(&space("file")),
        Some(&SpacePrefixes::Answered)
    );
    let got = renamed(&answers, "file", &names);
    assert!(unbuildable_why(&got[0]).contains("is no path under the root"));
    assert_eq!(got[1], Renamed::Outside);
    // A new name that is too long is unbuildable, with why.
    let long = "z".repeat(4095);
    let answers = ask("file", &["src/a/x.rs"], "src/a", &long);
    let got = renamed(&answers, "file", &["src/a/x.rs"]);
    assert!(unbuildable_why(&got[0]).contains("at most"));
}

#[test]
fn one_slash_at_the_end_of_a_path_prefix_is_dropped_in_the_space_file() {
    let names = ["src/a/x.rs", "src/a", "src/ab.rs"];
    let plain = ask("file", &names, "src/a", "src/b");
    let slashed = ask("file", &names, "src/a/", "src/b/");
    assert_eq!(
        slashed.prefixes_of(&space("file")),
        Some(&SpacePrefixes::Answered)
    );
    assert_eq!(
        renamed(&slashed, "file", &names),
        renamed(&plain, "file", &names)
    );
    assert_eq!(
        renamed(&slashed, "file", &names),
        [under("src/b/x.rs"), under("src/b"), Renamed::Outside]
    );
    // Only one: a second `/` makes it no path, as for a name.
    let answers = ask("file", &names, "src/a//", "src/b");
    assert!(not_under_why(&answers, "file").contains("is no path under the root"));
    let answers = ask("file", &names, "src/a", "src/b//");
    assert!(unbuildable_why(&renamed(&answers, "file", &names)[0]).contains("is no path"));
}

// The space `rust-item`.

#[test]
fn an_item_is_under_a_prefix_at_a_segment_with_its_members() {
    let names = [
        "app::m",
        "app::m::f",
        "app::m::Type::member",
        "app::mx::f",
        "app::n::f",
        "app",
    ];
    let answers = ask("rust-item", &names, "app::m", "app::n");
    assert_eq!(
        renamed(&answers, "rust-item", &names),
        [
            under("app::n"),
            under("app::n::f"),
            under("app::n::Type::member"),
            Renamed::Outside,
            Renamed::Outside,
            Renamed::Outside,
        ]
    );
    // A type renamed takes its members; a crate renamed takes everything.
    let names = ["app::Type", "app::Type::member", "app::TypeOf"];
    let answers = ask("rust-item", &names, "app::Type", "app::Kind");
    assert_eq!(
        renamed(&answers, "rust-item", &names),
        [
            under("app::Kind"),
            under("app::Kind::member"),
            Renamed::Outside
        ]
    );
    let answers = ask("rust-item", &["app::m::f", "apple::f"], "app", "pear");
    assert_eq!(
        renamed(&answers, "rust-item", &["app::m::f", "apple::f"]),
        [under("pear::m::f"), Renamed::Outside]
    );
}

#[test]
fn a_directory_is_no_prefix_of_an_item_and_a_new_prefix_that_is_no_path_is_unbuildable() {
    let answers = ask("rust-item", &["app::m::f"], "src/a", "src/b");
    assert!(not_under_why(&answers, "rust-item").contains("no prefix of a Rust path"));
    // Relative paths and raw identifiers are no names of the space.
    for from in ["crate::m", "self::m", "app::r#type", "app::", "::app"] {
        let answers = ask("rust-item", &["app::m::f"], from, "app::n");
        assert!(
            matches!(
                answers.prefixes_of(&space("rust-item")),
                Some(SpacePrefixes::NotUnder(_))
            ),
            "{from}"
        );
    }
    let answers = ask("rust-item", &["app::m::f", "app::x"], "app::m", "crate::n");
    let got = renamed(&answers, "rust-item", &["app::m::f", "app::x"]);
    assert!(unbuildable_why(&got[0]).contains("no prefix of a Rust path"));
    assert_eq!(got[1], Renamed::Outside);
}

// The space `rust-test`.

#[test]
fn a_test_is_under_the_path_of_its_file_part_by_part() {
    let names = [
        "src/a.rs::m::f",
        "src/a.rs::t",
        "src/a/x.rs::t",
        "src/ab.rs::t",
        "src/b.rs::m::f",
    ];
    // `src/a.rs` is the parts `src` and `a.rs`: the tests of that file, and of no
    // `src/a/` directory.
    let answers = ask("rust-test", &names, "src/a.rs", "src/b.rs");
    assert_eq!(
        renamed(&answers, "rust-test", &names),
        [
            under("src/b.rs::m::f"),
            under("src/b.rs::t"),
            Renamed::Outside,
            Renamed::Outside,
            Renamed::Outside,
        ]
    );
    // `src/a` is the parts `src` and `a`: the directory, and not the file `a.rs`.
    let answers = ask("rust-test", &names, "src/a", "lib/a");
    assert_eq!(
        renamed(&answers, "rust-test", &names),
        [
            Renamed::Outside,
            Renamed::Outside,
            under("lib/a/x.rs::t"),
            Renamed::Outside,
            Renamed::Outside,
        ]
    );
}

#[test]
fn a_test_is_under_a_file_and_the_first_segments_of_its_path_in_it() {
    let names = [
        "src/a.rs::m::f",
        "src/a.rs::m::tests::g",
        "src/a.rs::m",
        "src/a.rs::mx::f",
        "src/b.rs::m::f",
        "src/a.rs::n::f",
    ];
    let answers = ask("rust-test", &names, "src/a.rs::m", "src/a.rs::n");
    assert_eq!(
        renamed(&answers, "rust-test", &names),
        [
            under("src/a.rs::n::f"),
            under("src/a.rs::n::tests::g"),
            under("src/a.rs::n"),
            Renamed::Outside,
            Renamed::Outside,
            Renamed::Outside,
        ]
    );
    // A module that moves to another file, and a prefix of two segments.
    let answers = ask("rust-test", &names, "src/a.rs::m::tests", "src/c.rs::t");
    assert_eq!(
        renamed(&answers, "rust-test", &names)[1],
        under("src/c.rs::t::g")
    );
}

#[test]
fn a_raw_identifier_keeps_the_spelling_it_was_written_in() {
    // `type` is a keyword: `r#type` is the only way to write it, and the name built
    // keeps it, not the `type` it reads as (which is no name).
    let names = ["src/a/x.rs::r#type", "src/a/x.rs::r#foo", "src/a/x.rs::bar"];
    let answers = ask("rust-test", &names, "src/a", "src/b");
    assert_eq!(
        renamed(&answers, "rust-test", &names),
        [
            under("src/b/x.rs::r#type"),
            under("src/b/x.rs::r#foo"),
            under("src/b/x.rs::bar"),
        ]
    );
    // The rest of a path in a file, after a module that moves.
    let names = ["src/a.rs::m::r#type", "src/a.rs::m::r#foo::t"];
    let answers = ask("rust-test", &names, "src/a.rs::m", "src/a.rs::n");
    assert_eq!(
        renamed(&answers, "rust-test", &names),
        [under("src/a.rs::n::r#type"), under("src/a.rs::n::r#foo::t")]
    );
    // A `to` as the person typed it: `r#mod` stays so.
    let answers = ask("rust-test", &names, "src/a.rs::m", "src/a.rs::r#mod");
    assert_eq!(
        renamed(&answers, "rust-test", &names)[0],
        under("src/a.rs::r#mod::r#type")
    );
    // A segment is the same one however it is written, in `from` too.
    let answers = ask(
        "rust-test",
        &names,
        "src/a.rs::m::foo",
        "src/a.rs::m::r#bar",
    );
    assert_eq!(renamed(&answers, "rust-test", &names)[0], Renamed::Outside);
    let names = ["src/a.rs::m::r#foo::t"];
    let answers = ask(
        "rust-test",
        &names,
        "src/a.rs::m::foo",
        "src/a.rs::m::r#bar",
    );
    assert_eq!(
        renamed(&answers, "rust-test", &names),
        [under("src/a.rs::m::r#bar::t")]
    );
}

#[test]
fn one_slash_at_the_end_of_a_path_prefix_is_dropped_in_the_space_rust_test() {
    let names = ["src/a/x.rs::t", "src/a.rs::t", "src/b.rs::t"];
    let plain = ask("rust-test", &names, "src/a", "lib/a");
    let slashed = ask("rust-test", &names, "src/a/", "lib/a/");
    assert_eq!(
        slashed.prefixes_of(&space("rust-test")),
        Some(&SpacePrefixes::Answered)
    );
    assert_eq!(
        renamed(&slashed, "rust-test", &names),
        renamed(&plain, "rust-test", &names)
    );
    assert_eq!(
        renamed(&slashed, "rust-test", &names),
        [under("lib/a/x.rs::t"), Renamed::Outside, Renamed::Outside]
    );
    // A second `/` makes it no path.
    let answers = ask("rust-test", &names, "src/a//", "lib/a");
    assert!(matches!(
        answers.prefixes_of(&space("rust-test")),
        Some(SpacePrefixes::NotUnder(_))
    ));
}

#[test]
fn a_prefix_of_another_form_or_a_name_that_cannot_be_built_is_unbuildable_by_name() {
    let names = ["src/a.rs::m::f", "src/b.rs::t"];
    // The file's path, and a file with its segments, are two forms: no name of one is
    // the same name under the other.
    let answers = ask("rust-test", &names, "src/a.rs", "src/b.rs::m");
    let got = renamed(&answers, "rust-test", &names);
    assert!(unbuildable_why(&got[0]).contains("another form"));
    assert_eq!(got[1], Renamed::Outside);
    let answers = ask("rust-test", &names, "src/a.rs::m", "src/b.rs");
    assert!(unbuildable_why(&renamed(&answers, "rust-test", &names)[0]).contains("another form"));
    // A directory that is renamed to something that is not one a `.rs` file can have:
    // the name built is no name of the space.
    let answers = ask("rust-test", &["src/a.rs::m::f"], "src/a.rs", "src/b");
    assert!(
        unbuildable_why(&renamed(&answers, "rust-test", &["src/a.rs::m::f"])[0])
            .contains("does not end in `.rs`")
    );
    // A segment that is no identifier, a keyword, and a new prefix that is no path.
    for to in ["src/a.rs::fn", "src/a.rs::1x", "src/a.rs::", "../x.rs::m"] {
        let answers = ask("rust-test", &["src/a.rs::m::f"], "src/a.rs::m", to);
        assert!(
            matches!(
                renamed(&answers, "rust-test", &["src/a.rs::m::f"])[0],
                Renamed::Unbuildable(_)
            ),
            "{to}"
        );
    }
}

#[test]
fn a_prefix_that_is_no_test_prefix_is_none_of_the_names() {
    for from in [
        "app::m",
        "src/a.rs::",
        "src/a.rs::fn",
        "../a.rs::m",
        "src/a.rs::m::",
    ] {
        let answers = ask("rust-test", &["src/a.rs::m::f"], from, "src/b.rs::m");
        assert!(
            matches!(
                answers.prefixes_of(&space("rust-test")),
                Some(SpacePrefixes::NotUnder(_))
            ),
            "{from}"
        );
    }
    // A text that is no name is outside every prefix.
    let answers = ask("rust-test", &["not a name", "src/a.rs"], "src", "lib");
    assert_eq!(
        renamed(&answers, "rust-test", &["not a name", "src/a.rs"]),
        [Renamed::Outside, Renamed::Outside]
    );
}

// The space `js-test`.

#[test]
fn a_js_test_is_under_the_path_of_its_file_or_its_file_and_its_first_titles() {
    let names = [
        "tests/cart.test.ts::Cart::adds",
        "tests/cart.test.ts::top",
        "tests/cartel.test.ts::Cart::adds",
        "tests/other.test.ts::Cart::adds",
    ];
    let answers = ask("js-test", &names, "tests/cart.test.ts", "spec/cart.spec.ts");
    assert_eq!(
        renamed(&answers, "js-test", &names),
        [
            under("spec/cart.spec.ts::Cart::adds"),
            under("spec/cart.spec.ts::top"),
            Renamed::Outside,
            Renamed::Outside,
        ]
    );
    let answers = ask(
        "js-test",
        &names,
        "tests/cart.test.ts::Cart",
        "tests/cart.test.ts::Basket",
    );
    assert_eq!(
        renamed(&answers, "js-test", &names),
        [
            under("tests/cart.test.ts::Basket::adds"),
            Renamed::Outside,
            Renamed::Outside,
            Renamed::Outside,
        ]
    );
    // A directory is the parts of a path, and a file's `.ts` stays in its part.
    let answers = ask("js-test", &names, "tests", "spec");
    assert_eq!(
        renamed(&answers, "js-test", &names)[2],
        under("spec/cartel.test.ts::Cart::adds")
    );
}

#[test]
fn a_title_is_read_with_its_escapes_and_kept_as_it_was_written() {
    // A title that holds `::` and `\`, in the prefix as in the names: `\:` is `:` and
    // `\\` is `\`, so the names are split at no `::` that is escaped. The names keep
    // their escapes as they have them, and `to` as it was typed: a title is never
    // written out again.
    let names = [
        r"a.test.ts::Cart\:\:Special::adds",
        r"a.test.ts::Cart\:\:Special::Back\\slash::t",
        r"a.test.ts::Cart::Special::adds",
    ];
    let answers = ask(
        "js-test",
        &names,
        r"a.test.ts::Cart\:\:Special",
        r"a.test.ts::Cart: A\\B",
    );
    assert_eq!(
        renamed(&answers, "js-test", &names),
        [
            under(r"a.test.ts::Cart: A\\B::adds"),
            under(r"a.test.ts::Cart: A\\B::Back\\slash::t"),
            Renamed::Outside,
        ]
    );
    // A `to` with an escape is built as typed.
    let answers = ask(
        "js-test",
        &names,
        r"a.test.ts::Cart\:\:Special",
        r"a.test.ts::A\:\:B",
    );
    assert_eq!(
        renamed(&answers, "js-test", &names)[0],
        under(r"a.test.ts::A\:\:B::adds")
    );
    // A `/` in a title is a character of it, and no boundary of a path.
    let names = [
        "api.test.ts::GET /items::returns 200",
        "api.test.ts::GET /items/1::returns 404",
    ];
    let answers = ask(
        "js-test",
        &names,
        "api.test.ts::GET /items",
        "api.test.ts::GET /v2/items",
    );
    assert_eq!(
        renamed(&answers, "js-test", &names),
        [
            under("api.test.ts::GET /v2/items::returns 200"),
            Renamed::Outside
        ]
    );
}

#[test]
fn the_titles_under_a_moved_file_keep_the_spelling_they_were_written_in() {
    // A `:` that is no `::` needs no escape in a name, and a name that has none is
    // not given one: the name a person writes in a link is the name built.
    let names = [
        "tests/a.test.ts::Cart: adds::x",
        r"tests/a.test.ts::a\:\:b::x",
        r"tests/a.test.ts::Back\\slash::GET /items::t",
        "tests/a.test.ts::trailing:",
        "tests/ab.test.ts::Cart: adds::x",
    ];
    let answers = ask("js-test", &names, "tests/a.test.ts", "tests/b.test.ts");
    assert_eq!(
        renamed(&answers, "js-test", &names),
        [
            under("tests/b.test.ts::Cart: adds::x"),
            under(r"tests/b.test.ts::a\:\:b::x"),
            under(r"tests/b.test.ts::Back\\slash::GET /items::t"),
            under("tests/b.test.ts::trailing:"),
            Renamed::Outside,
        ]
    );
    // The titles after a title that moves keep theirs too.
    let answers = ask(
        "js-test",
        &names,
        "tests/a.test.ts::Cart: adds",
        "tests/a.test.ts::Basket: puts",
    );
    assert_eq!(
        renamed(&answers, "js-test", &names)[0],
        under("tests/a.test.ts::Basket: puts::x")
    );
    // A `to` that ends in a `:` no `\` escapes cannot be followed by a `::`: the name
    // would read as another one, so it is unbuildable (and a last title may end so).
    let answers = ask(
        "js-test",
        &names,
        "tests/a.test.ts::Cart: adds",
        "tests/a.test.ts::Basket:",
    );
    assert!(
        unbuildable_why(&renamed(&answers, "js-test", &names)[0]).contains("reads as other parts")
    );
    let answers = ask(
        "js-test",
        &names[..1],
        "tests/a.test.ts::Cart: adds::x",
        "tests/a.test.ts::Basket:",
    );
    assert_eq!(
        renamed(&answers, "js-test", &names[..1]),
        [under("tests/a.test.ts::Basket:")]
    );
}

#[test]
fn one_slash_at_the_end_of_a_path_prefix_is_dropped_in_the_space_js_test() {
    let names = [
        "tests/a/x.test.ts::Cart::adds",
        "tests/a.test.ts::t",
        "tests/b.test.ts::t",
    ];
    let plain = ask("js-test", &names, "tests/a", "spec/a");
    let slashed = ask("js-test", &names, "tests/a/", "spec/a/");
    assert_eq!(
        slashed.prefixes_of(&space("js-test")),
        Some(&SpacePrefixes::Answered)
    );
    assert_eq!(
        renamed(&slashed, "js-test", &names),
        renamed(&plain, "js-test", &names)
    );
    assert_eq!(
        renamed(&slashed, "js-test", &names),
        [
            under("spec/a/x.test.ts::Cart::adds"),
            Renamed::Outside,
            Renamed::Outside
        ]
    );
    // A second `/` makes it no path.
    let answers = ask("js-test", &names, "tests/a//", "spec/a");
    assert!(matches!(
        answers.prefixes_of(&space("js-test")),
        Some(SpacePrefixes::NotUnder(_))
    ));
}

#[test]
fn a_js_prefix_of_another_form_or_none_is_told_as_the_domain_wants() {
    let names = ["a.test.ts::Cart::adds"];
    let answers = ask("js-test", &names, "a.test.ts", "a.test.ts::Cart");
    assert!(unbuildable_why(&renamed(&answers, "js-test", &names)[0]).contains("another form"));
    let answers = ask("js-test", &names, "a.test.ts::Cart", "b.test.ts");
    assert!(unbuildable_why(&renamed(&answers, "js-test", &names)[0]).contains("another form"));
    // A file that is no script, an empty title and a bad escape make no prefix.
    for from in [
        "a.txt::Cart",
        "a.test.ts::",
        r"a.test.ts::Ca\rt",
        "../a.test.ts::Cart",
    ] {
        let answers = ask("js-test", &names, from, "b.test.ts::Cart");
        assert!(
            matches!(
                answers.prefixes_of(&space("js-test")),
                Some(SpacePrefixes::NotUnder(_))
            ),
            "{from}"
        );
    }
    // A new title that makes the name longer than a name can be is unbuildable.
    let long = format!("a.test.ts::{}", "z".repeat(4080));
    let answers = ask("js-test", &names, "a.test.ts::Cart", &long);
    assert!(matches!(
        renamed(&answers, "js-test", &names)[0],
        Renamed::Unbuildable(_)
    ));
}

// The dispatch.

#[test]
fn a_space_with_no_adapter_takes_no_prefix_and_every_space_asked_is_answered() {
    let mut asks = PrefixAsks::new();
    asks.ask(space("file"), name("src/a/x.rs"));
    asks.ask(space("other"), name("src/a/x.rs"));
    asks.ask(space("rust-test"), name("src/a/x.rs::t"));
    let answers = source().renamed(&asks, &moving("src/a", "src/b")).unwrap();
    assert_eq!(answers.of(), &moving("src/a", "src/b"));
    assert_eq!(
        answers.prefixes_of(&space("other")),
        Some(&SpacePrefixes::NoPrefixes)
    );
    assert_eq!(answers.renamed(&space("other"), &name("src/a/x.rs")), None);
    assert_eq!(
        answers.renamed(&space("file"), &name("src/a/x.rs")),
        Some(&under("src/b/x.rs"))
    );
    assert_eq!(
        answers.renamed(&space("rust-test"), &name("src/a/x.rs::t")),
        Some(&under("src/b/x.rs::t"))
    );
    // A space not asked has no answer.
    assert_eq!(answers.prefixes_of(&space("js-test")), None);
}
