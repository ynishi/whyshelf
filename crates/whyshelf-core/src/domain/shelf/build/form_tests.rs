//! Tests of the entries a file is written with (`Shelf::form_entries`, #136): which
//! entries that say nothing are left out, and that the build of the result is the
//! build of what it was made from, for valid and invalid files alike.

use super::*;
use crate::domain::shelf::fixtures_tests::{
    decls, docid, entries, list, path, schema, spaces, text,
};

fn book(name: &str) -> BookName {
    BookName::new(name).unwrap()
}

/// Every way a value can say nothing.
fn nothings() -> Vec<RawValue> {
    vec![
        RawValue::Null,
        text(""),
        text("  "),
        text("\n\t"),
        RawValue::List(Vec::new()),
    ]
}

/// The schema of the fixtures with every field but `title` optional or every field
/// required, and every link of the three books counting `cardinality`.
fn schema_of(required: bool, cardinality: Cardinality) -> Schema {
    let mut decls = decls();
    for decl in &mut decls {
        for field in &mut decl.fields {
            field.presence = if required || field.name == "title" {
                FieldPresence::Required
            } else {
                FieldPresence::Optional
            };
        }
        for link in &mut decl.links {
            link.cardinality = cardinality;
        }
    }
    Schema::validate(decls, &spaces()).unwrap()
}

/// Every schema the invariant is run on: both presences, every count.
fn schemas() -> Vec<(String, Schema)> {
    let mut all = Vec::new();
    for required in [false, true] {
        for cardinality in Cardinality::ALL {
            all.push((
                format!("required={required}, count={cardinality:?}"),
                schema_of(required, cardinality),
            ));
        }
    }
    all
}

/// The file a document of `book` is read from, as an adapter would read it.
fn parsed(book: &str, entries: &BTreeMap<String, RawValue>) -> DocReading {
    let file = match book {
        "finding" => "findings/fr-1.md",
        _ => "specs/sp-1.md",
    };
    DocReading::Parsed {
        origin: path(file),
        entries: entries.clone(),
        body: "body\n".to_owned(),
    }
}

fn build_of(schema: &Schema, reading: DocReading) -> Shelf {
    Shelf::build(schema, vec![reading], &ArtifactReadings::new())
}

/// The `id`, `key` and `title` a document of `book` is written with.
fn identity(book: &str) -> Vec<(&'static str, RawValue)> {
    let key = if book == "finding" { "FR-1" } else { "SP-1" };
    vec![
        ("id", text(&docid(1).to_string())),
        ("key", text(key)),
        ("title", text("t")),
    ]
}

/// The frames the entry under test is put in: a document that is valid without it, one
/// that holds other values, and one that is invalid for other reasons too.
fn frames(book: &str) -> Vec<BTreeMap<String, RawValue>> {
    let bare = identity(book);
    let mut some = identity(book);
    let mut invalid = identity(book);
    invalid.push(("bogus", text("x")));
    invalid.push(("zeta", RawValue::Integer(1)));
    if book == "finding" {
        some.push(("note", text("n")));
        some.push(("base", list(&["FR-2"])));
        invalid.push(("count", text("x")));
        invalid.push(("flag", RawValue::Integer(1)));
    } else {
        some.push(("tests", list(&["a.rs"])));
    }
    [bare, some, invalid]
        .into_iter()
        .map(|items| {
            items
                .into_iter()
                .map(|(name, value)| (name.to_owned(), value))
                .collect()
        })
        .collect()
}

/// Every entry name the invariant puts a value that says nothing under: each declared
/// one, an undeclared one, and the `id` and the `key`.
fn names(book: &str) -> Vec<&'static str> {
    let mut names = vec!["title", "id", "key", "undeclared"];
    if book == "finding" {
        names.extend(["note", "count", "flag", "tags", "level"]);
        names.extend(["base", "related", "supersedes"]);
    } else {
        names.extend(["grounds", "tests", "code"]);
    }
    names
}

// ---- the invariant: the build does not change -----------------------------------

#[test]
fn the_build_of_the_form_is_the_build_of_the_entries() {
    let mut runs = 0;
    for (label, schema) in schemas() {
        for book_name in ["finding", "spec"] {
            for frame in frames(book_name) {
                for name in names(book_name) {
                    for nothing in nothings() {
                        let mut written = frame.clone();
                        written.insert(name.to_owned(), nothing.clone());
                        assert_same_build(
                            &schema,
                            book_name,
                            &written,
                            &format!("{label}: {book_name}: {name} = {nothing:?} in {frame:?}"),
                        );
                        runs += 1;
                    }
                }
                // Every entry that says nothing at once.
                let mut written = frame.clone();
                for name in names(book_name) {
                    written.insert(name.to_owned(), RawValue::Null);
                }
                assert_same_build(
                    &schema,
                    book_name,
                    &written,
                    &format!("{label}: {book_name}: all nothing in {frame:?}"),
                );
            }
        }
    }
    assert!(runs > 1000, "the table is run: {runs}");
}

fn assert_same_build(
    schema: &Schema,
    book_name: &str,
    written: &BTreeMap<String, RawValue>,
    context: &str,
) {
    let form = Shelf::form_entries(schema, &book(book_name), written);
    assert!(
        form.as_map().len() <= written.len(),
        "{context}: an entry was added"
    );
    let before = build_of(schema, parsed(book_name, written));
    let after = build_of(schema, parsed(book_name, form.as_map()));
    assert_eq!(before, after, "{context}");
}

#[test]
fn a_file_with_nothing_left_out_is_valid_where_the_entries_were() {
    // The shelf of the left-out form is valid, not only equal to an invalid one.
    let schema = schema_of(false, Cardinality::Many);
    let written = entries(&[
        ("id", text(&docid(1).to_string())),
        ("key", text("FR-1")),
        ("title", text("t")),
        ("note", text("")),
        ("tags", list(&[])),
        ("base", RawValue::Null),
    ]);
    let form = Shelf::form_entries(&schema, &book("finding"), &written);
    let shelf = build_of(&schema, parsed("finding", form.as_map()));
    assert!(shelf.invalid().is_empty() && shelf.unreadable().is_empty());
    assert!(shelf.doc(docid(1)).is_some());
}

// ---- which entries are left out --------------------------------------------------

fn names_in(form: &FormEntries) -> Vec<&str> {
    form.iter().map(|(name, _)| name.as_str()).collect()
}

/// A finding with this entry added to its title, `id` and `key`.
fn finding_with(schema: &Schema, name: &str, value: RawValue) -> FormEntries {
    let mut written = entries(&[
        ("id", text(&docid(1).to_string())),
        ("key", text("FR-1")),
        ("title", text("t")),
    ]);
    written.insert(name.to_owned(), value);
    Shelf::form_entries(schema, &book("finding"), &written)
}

#[test]
fn an_optional_field_that_says_nothing_is_left_out_whatever_its_type() {
    let schema = schema_of(false, Cardinality::Many);
    for name in ["note", "count", "flag", "tags", "level"] {
        for nothing in nothings() {
            let form = finding_with(&schema, name, nothing.clone());
            assert_eq!(form.get(name), None, "{name}: {nothing:?}");
            assert_eq!(names_in(&form), ["id", "key", "title"], "{name}");
        }
    }
}

#[test]
fn a_required_field_that_says_nothing_is_kept() {
    let schema = schema_of(true, Cardinality::Many);
    for name in ["title", "note", "count", "flag", "tags", "level"] {
        for nothing in nothings() {
            let form = finding_with(&schema, name, nothing.clone());
            assert_eq!(form.get(name), Some(&nothing), "{name}: {nothing:?}");
        }
    }
}

#[test]
fn a_link_that_says_nothing_is_left_out_only_when_its_count_admits_none() {
    for cardinality in Cardinality::ALL {
        let schema = schema_of(false, cardinality);
        let admits_none = matches!(cardinality, Cardinality::Optional | Cardinality::Many);
        for nothing in nothings() {
            let form = finding_with(&schema, "base", nothing.clone());
            assert_eq!(
                form.get("base").is_none(),
                admits_none,
                "{cardinality:?}: {nothing:?}"
            );
        }
    }
}

#[test]
fn a_link_to_artifacts_that_says_nothing_is_left_out_only_when_its_count_admits_none() {
    for cardinality in Cardinality::ALL {
        let schema = schema_of(false, cardinality);
        let admits_none = matches!(cardinality, Cardinality::Optional | Cardinality::Many);
        for nothing in nothings() {
            let mut written = entries(&[
                ("id", text(&docid(1).to_string())),
                ("key", text("SP-1")),
                ("title", text("t")),
            ]);
            written.insert("tests".to_owned(), nothing.clone());
            let form = Shelf::form_entries(&schema, &book("spec"), &written);
            assert_eq!(
                form.get("tests").is_none(),
                admits_none,
                "{cardinality:?}: {nothing:?}"
            );
        }
    }
}

#[test]
fn the_id_the_key_and_an_undeclared_entry_that_say_nothing_are_kept() {
    let schema = schema_of(false, Cardinality::Many);
    for nothing in nothings() {
        for name in ["id", "key", "undeclared"] {
            let form = finding_with(&schema, name, nothing.clone());
            assert_eq!(form.get(name), Some(&nothing), "{name}: {nothing:?}");
        }
    }
}

#[test]
fn every_entry_that_says_something_is_kept() {
    let schema = schema_of(false, Cardinality::Many);
    let written = entries(&[
        ("id", text(&docid(1).to_string())),
        ("key", text("FR-1")),
        ("title", text(" padded ")),
        ("note", text("n")),
        ("count", RawValue::Integer(0)),
        ("flag", RawValue::Bool(false)),
        ("tags", list(&["a"])),
        ("base", list(&["FR-2"])),
        // A list with a blank item says something, and the build tells the item.
        ("related", list(&[" "])),
        ("undeclared", text("x")),
    ]);
    let form = Shelf::form_entries(&schema, &book("finding"), &written);
    assert_eq!(form.as_map(), &written);
}

#[test]
fn a_book_the_schema_does_not_declare_leaves_nothing_out() {
    let schema = schema();
    let written = entries(&[("note", text("")), ("tags", list(&[]))]);
    let form = Shelf::form_entries(&schema, &book("nope"), &written);
    assert_eq!(form.as_map(), &written);
    assert_eq!(form.len(), 2);
    assert!(!form.is_empty());
}
