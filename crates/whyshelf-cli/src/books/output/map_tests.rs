//! Tests of the answer of `map`: its drawing, exactly, and the shape of its JSON.

use std::collections::BTreeSet;

use whyshelf_core::domain::nonempty::NonEmptyVec;
use whyshelf_core::domain::shelf::artifact::Space;
use whyshelf_core::domain::shelf::map::ShelfMap;
use whyshelf_core::domain::shelf::schema::{Schema, SchemaProblem};
use whyshelf_infra::shelf::books::parse;

use super::*;

/// Three books, declared in an order that is not the one of their names: links of
/// every count, a cover of each strength, a link into its own book, one to
/// artifacts compared by lines, and a book of slugs with no link and no
/// description.
const BOOKS: &str = r##"
[[book]]
name = "verification"
dir = "books/verification"
key = { prefix = "V" }
description = "how a decision is checked"
[[book.field]]
name = "title"
type = "text"
required = true
[[book.field]]
name = "method"
type = { enum = ["unit", "e2e"] }
[[book.link]]
name = "tests"
target = "artifact"
space = "file"
means = "depends"
compare = "lines"
ignore = ["//", "#"]
[[book.link]]
name = "checks"
target = "decision"
count = "at-least-one"
means = "depends"
cover = true
[[book.link]]
name = "for"
target = "note"
means = "informs"

[[book]]
name = "decision"
dir = "books/decision"
key = { prefix = "DEC" }
description = "how it is met"
[[book.link]]
name = "replaces"
target = "decision"
count = "optional"
means = "replaces"
cover = "warn"
[[book.link]]
name = "main"
target = "note"
count = "one"
means = "informs"

[[book]]
name = "note"
dir = "books/note"
key = "slug"
"##;

/// The declaration, expanded with the presets of the file adapter and checked.
fn schema_of(books: &str) -> Schema {
    let space = Space::new("file").unwrap();
    let builtins = whyshelf_infra::shelf::artifact::file::presets(&space);
    Schema::declared(parse(books).unwrap(), &builtins, &BTreeSet::from([space])).unwrap()
}

fn schema() -> Schema {
    schema_of(BOOKS)
}

fn mapped() -> MapOut {
    map(&MapReport::Mapped(ShelfMap::of(&schema())))
}

#[test]
fn the_drawing_is_one_block_per_book_in_the_order_of_the_declaration() {
    // Every column is aligned across the blocks, the one of `cover` as wide as
    // `cover(warn)`, and it is in every block that has a link.
    let expected = "\
verification  V-    how a decision is checked
├─ tests     depends                0..   ──► artifact space=file  compare=lines ignore=\"#\",\"//\"
├─ checks    depends   cover        1..   ──► decision
└─ for       informs                0..   ──► note
decision      DEC-  how it is met
├─ replaces  replaces  cover(warn)  0..1  ──► decision (own book)
└─ main      informs                1     ──► note
note          slug
";
    assert_eq!(mapped().diagram.as_deref(), Some(expected));
    // The same declaration draws the same.
    assert_eq!(mapped().diagram, mapped().diagram);
}

#[test]
fn the_json_shows_the_range_of_a_book_when_it_is_written() {
    let books = "[[book]]\nname='a'\ndir='a'\nkey='slug'\n\
                 include=['*.md', 'notes/**']\nexclude=['archive/**']\n\
                 without_front_matter='skip'\n\
                 [[book]]\nname='b'\ndir='b'\nkey='slug'\n\
                 exclude=[]\nwithout_front_matter='invalid'\n\
                 [[book]]\nname='c'\ndir='c'\nkey='slug'\n";
    let answer =
        serde_json::to_value(map(&MapReport::Mapped(ShelfMap::of(&schema_of(books))))).unwrap();
    let a = &answer["books"][0];
    assert_eq!(a["include"], serde_json::json!(["*.md", "notes/**"]));
    assert_eq!(a["exclude"], serde_json::json!(["archive/**"]));
    assert_eq!(a["without_front_matter"], "skip");
    // What is written is shown as written, the default value included.
    let b = &answer["books"][1];
    assert_eq!(b["exclude"], serde_json::json!([]));
    assert_eq!(b["without_front_matter"], "invalid");
    // A book that writes none of them has every field, null, as `description`.
    let c = answer["books"][2].as_object().unwrap();
    for key in ["include", "exclude", "without_front_matter", "description"] {
        assert_eq!(c.get(key), Some(&serde_json::Value::Null), "{key}");
    }
}

/// The line of the link `tests` of a book `a` that ignores these prefixes.
fn ignore_line(ignore: &str) -> String {
    let books = format!(
        "[[book]]\nname='a'\ndir='a'\nkey='slug'\n\
         [[book.link]]\nname='tests'\ntarget='artifact'\nspace='file'\nmeans='depends'\n\
         compare='lines'\nignore={ignore}\n"
    );
    let drawn = map(&MapReport::Mapped(ShelfMap::of(&schema_of(&books))))
        .diagram
        .unwrap();
    drawn.lines().nth(1).unwrap().to_owned()
}

#[test]
fn each_ignored_prefix_is_drawn_as_a_quoted_string_so_a_comma_is_no_separator() {
    let comma = ignore_line(r##"["a,b", "#"]"##);
    let three = ignore_line(r##"["#", "a", "b"]"##);
    assert!(comma.ends_with(r##"ignore="#","a,b""##), "{comma}");
    assert!(three.ends_with(r##"ignore="#","a","b""##), "{three}");
    assert_ne!(comma, three);
    // A quote and a backslash are escaped as JSON escapes them.
    let quote = ignore_line(r#"['a"b', 'c\']"#);
    assert!(quote.ends_with(r#"ignore="a\"b","c\\""#), "{quote}");
}

#[test]
fn the_json_holds_the_books_and_links_as_data() {
    let answer = serde_json::to_value(mapped()).unwrap();
    assert_eq!(answer["schema_problems"], serde_json::json!([]));
    let books: Vec<&str> = answer["books"]
        .as_array()
        .unwrap()
        .iter()
        .map(|b| b["name"].as_str().unwrap())
        .collect();
    assert_eq!(books, ["verification", "decision", "note"]);
    assert_eq!(
        answer["books"][0],
        serde_json::json!({
            "name": "verification",
            "dir": "books/verification",
            "include": null,
            "exclude": null,
            "without_front_matter": null,
            "key": {"kind": "prefix", "prefix": "V"},
            "description": "how a decision is checked",
            "fields": [
                {"name": "title", "type": "text", "words": [], "required": true},
                {"name": "method", "type": "enum", "words": ["e2e", "unit"], "required": false},
            ],
        })
    );
    assert_eq!(
        answer["books"][2]["key"],
        serde_json::json!({"kind": "slug"})
    );
    assert_eq!(answer["books"][2]["description"], serde_json::Value::Null);

    let links = answer["links"].as_array().unwrap();
    assert_eq!(links.len(), 5);
    assert_eq!(
        links[0],
        serde_json::json!({
            "from": "verification",
            "name": "tests",
            "target": {"kind": "artifact", "space": "file", "compare": "lines", "ignore": ["#", "//"]},
            "means": "depends",
            "count": "many",
            "cover": false,
            "cover_severity": null,
            "preset": null,
            "preset_by_default": false,
        })
    );
    assert_eq!(
        links[1],
        serde_json::json!({
            "from": "verification",
            "name": "checks",
            "target": {"kind": "book", "book": "decision"},
            "means": "depends",
            "count": "at-least-one",
            "cover": true,
            "cover_severity": "error",
            "preset": null,
            "preset_by_default": false,
        })
    );
    // `cover` is true for either strength, and the strength stands beside it.
    assert_eq!(links[2]["cover"], false);
    assert_eq!(links[2]["cover_severity"], serde_json::Value::Null);
    assert_eq!(links[3]["target"]["book"], "decision");
    assert_eq!(links[3]["count"], "optional");
    assert_eq!(links[3]["cover"], true);
    assert_eq!(links[3]["cover_severity"], "warn");
    assert_eq!(links[4]["count"], "one");
}

/// A book whose links to artifacts take a preset each way: named, of the project
/// `from` a built-in one, by default, and none (a space written).
const PRESETS: &str = r##"
[defaults]
artifact = "file"

[[preset]]
name = "rust"
from = "file"
compare = "lines"
ignore = ["//"]

[[book]]
name = "verification"
dir = "books/verification"
key = { prefix = "V" }
[[book.link]]
name = "tests"
target = "artifact"
[[book.link]]
name = "code"
target = "artifact"
preset = "rust"
means = "informs"
[[book.link]]
name = "data"
target = "artifact"
space = "file"
means = "depends"
[[book.link]]
name = "checks"
target = "verification"
means = "informs"
"##;

#[test]
fn the_drawing_shows_the_preset_of_each_link_in_a_column_of_its_own() {
    let expected = "\
verification  V-
├─ tests   depends  0..  preset=file(default)  ──► artifact space=file  compare=bytes
├─ code    informs  0..  preset=rust           ──► artifact space=file  compare=lines ignore=\"//\"
├─ data    depends  0..                        ──► artifact space=file  compare=bytes
└─ checks  informs  0..                        ──► verification (own book)
";
    let drawn = map(&MapReport::Mapped(ShelfMap::of(&schema_of(PRESETS))));
    assert_eq!(drawn.diagram.as_deref(), Some(expected));
}

#[test]
fn the_json_tells_the_expanded_values_and_the_preset_of_each_link() {
    let answer =
        serde_json::to_value(map(&MapReport::Mapped(ShelfMap::of(&schema_of(PRESETS))))).unwrap();
    let links = answer["links"].as_array().unwrap();
    assert_eq!(
        links[0],
        serde_json::json!({
            "from": "verification",
            "name": "tests",
            "target": {"kind": "artifact", "space": "file", "compare": "bytes", "ignore": []},
            "means": "depends",
            "count": "many",
            "cover": false,
            "cover_severity": null,
            "preset": "file",
            "preset_by_default": true,
        })
    );
    assert_eq!(
        links[1]["target"],
        serde_json::json!({"kind": "artifact", "space": "file", "compare": "lines", "ignore": ["//"]})
    );
    assert_eq!(links[1]["means"], "informs");
    assert_eq!(links[1]["preset"], "rust");
    assert_eq!(links[1]["preset_by_default"], false);
    assert_eq!(links[2]["preset"], serde_json::Value::Null);
    assert_eq!(links[3]["preset"], serde_json::Value::Null);
    assert_eq!(links[3]["preset_by_default"], false);
}

#[test]
fn a_declaration_that_cannot_be_used_is_its_problems_and_no_drawing() {
    let problems = NonEmptyVec::from_vec(vec![SchemaProblem::ReservedBookName]).unwrap();
    let answer = map(&MapReport::SchemaInvalid(problems));
    assert_eq!(answer.schema_problems.len(), 1);
    assert!(answer.books.is_empty() && answer.links.is_empty());
    assert_eq!(answer.diagram, None);
}

#[test]
fn a_problem_of_the_declaration_says_its_reason() {
    let books = "[[preset]]\nname='odd'\ncompare='words'\n\
         [[book]]\nname='a'\ndir='a'\nkey='slug'\n\
         [[book.link]]\nname='tests'\ntarget='artifact'\nspace='file'\nmeans='depends'\n\
         compare='bytes'\nignore=['//']\n";
    let space = Space::new("file").unwrap();
    let builtins = whyshelf_infra::shelf::artifact::file::presets(&space);
    let problems =
        Schema::declared(parse(books).unwrap(), &builtins, &BTreeSet::from([space])).unwrap_err();
    let answer = map(&MapReport::SchemaInvalid(problems));
    assert_eq!(
        answer.schema_problems,
        [
            "the `compare` of the preset `odd` is not one: `words` is no comparison \
             (`bytes`, `lines` or `presence`)",
            "book `a`: the `compare` of the link `tests` is not one: `ignore` is for \
             `compare = \"lines\"`: `bytes` counts every byte",
        ]
    );
}
