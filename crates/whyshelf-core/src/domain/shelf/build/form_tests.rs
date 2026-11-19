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
    let form = Shelf::form_entries(schema, &book(book_name), written, &DropNames::none());
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
    let form = Shelf::form_entries(&schema, &book("finding"), &written, &DropNames::none());
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
    Shelf::form_entries(schema, &book("finding"), &written, &DropNames::none())
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
            let form = Shelf::form_entries(&schema, &book("spec"), &written, &DropNames::none());
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
    let form = Shelf::form_entries(&schema, &book("finding"), &written, &DropNames::none());
    assert_eq!(form.as_map(), &written);
    assert!(form.dropped().is_empty());
}

#[test]
fn a_book_the_schema_does_not_declare_leaves_nothing_out() {
    let schema = schema();
    let written = entries(&[("note", text("")), ("tags", list(&[]))]);
    let form = Shelf::form_entries(&schema, &book("nope"), &written, &DropNames::none());
    assert_eq!(form.as_map(), &written);
    assert_eq!(form.len(), 2);
    assert!(!form.is_empty());
}

// ---- the names asked to drop (#143) ----------------------------------------------

fn blank(name: &str) -> NonBlank {
    NonBlank::new(name).unwrap()
}

fn ask(names: &[&str]) -> DropAsk {
    DropAsk {
        names: names.iter().map(|n| blank(n)).collect(),
    }
}

fn drops(schema: &Schema, names: &[&str]) -> DropNames {
    DropNames::new(schema, ask(names)).expect("names that fit")
}

fn problems(schema: &Schema, names: &[&str]) -> Vec<DropProblem> {
    DropNames::new(schema, ask(names))
        .expect_err("names that do not fit")
        .into_vec()
}

/// A document of `book` with these entries besides its identity and `title`.
fn written_in(book_name: &str, extra: &[(&str, RawValue)]) -> BTreeMap<String, RawValue> {
    let mut written: BTreeMap<String, RawValue> = identity(book_name)
        .into_iter()
        .map(|(name, value)| (name.to_owned(), value))
        .collect();
    written.extend(entries(extra));
    written
}

#[test]
fn an_entry_of_a_dropped_name_is_left_out_where_the_book_does_not_declare_it() {
    let schema = schema();
    // `note` is a field of `finding` only, and `base` a link of `finding` only.
    let drop = drops(&schema, &["note", "base"]);
    let held = [("note", text("n")), ("base", list(&["FR-2"]))];
    let in_spec = Shelf::form_entries(&schema, &book("spec"), &written_in("spec", &held), &drop);
    assert_eq!(names_in(&in_spec), ["id", "key", "title"]);
    assert_eq!(in_spec.dropped(), &ask(&["base", "note"]).names);
    // Whatever the entry holds, and in a book of another kind of key.
    for value in nothings().into_iter().chain([
        text("x"),
        RawValue::Integer(1),
        RawValue::Bool(true),
        list(&["a"]),
    ]) {
        let held = [("note", value.clone())];
        let form = Shelf::form_entries(
            &schema,
            &book("design"),
            &written_in("design", &held),
            &drop,
        );
        assert_eq!(names_in(&form), ["id", "key", "title"], "{value:?}");
        assert_eq!(form.dropped(), &ask(&["note"]).names, "{value:?}");
    }
}

#[test]
fn an_entry_of_a_dropped_name_is_kept_where_the_book_declares_it() {
    let schema = schema();
    let drop = drops(&schema, &["note", "base"]);
    let held = [("note", text("n")), ("base", list(&["FR-2"]))];
    let in_finding = Shelf::form_entries(
        &schema,
        &book("finding"),
        &written_in("finding", &held),
        &drop,
    );
    assert_eq!(
        names_in(&in_finding),
        ["base", "id", "key", "note", "title"]
    );
    assert!(in_finding.dropped().is_empty());
}

#[test]
fn the_name_of_a_link_to_artifacts_is_dropped_where_the_book_declares_no_such_link() {
    let schema = schema();
    // `tests` is a link to artifacts of `spec` only.
    let drop = drops(&schema, &["tests"]);
    let held = [("tests", list(&["a.rs"]))];
    let in_finding = Shelf::form_entries(
        &schema,
        &book("finding"),
        &written_in("finding", &held),
        &drop,
    );
    assert_eq!(names_in(&in_finding), ["id", "key", "title"]);
    assert_eq!(in_finding.dropped(), &ask(&["tests"]).names);
    let in_spec = Shelf::form_entries(&schema, &book("spec"), &written_in("spec", &held), &drop);
    assert_eq!(names_in(&in_spec), ["id", "key", "tests", "title"]);
    assert!(in_spec.dropped().is_empty());
}

#[test]
fn the_names_dropped_are_only_those_an_entry_was_left_out_for() {
    let schema = schema();
    let drop = drops(&schema, &["note", "gone", "base"]);
    // `note` is held, `gone` no file holds, and `base` is held where it is declared;
    // an entry that says nothing is left out for that, not for being dropped.
    let held = [
        ("note", RawValue::Null),
        ("base", list(&["FR-2"])),
        ("tags", list(&[])),
    ];
    let form = Shelf::form_entries(
        &schema,
        &book("finding"),
        &written_in("finding", &held),
        &drop,
    );
    assert_eq!(names_in(&form), ["base", "id", "key", "title"]);
    assert!(form.dropped().is_empty());
    let form = Shelf::form_entries(&schema, &book("spec"), &written_in("spec", &held), &drop);
    assert_eq!(form.dropped(), &ask(&["base", "note"]).names);
}

#[test]
fn an_entry_the_ask_does_not_list_is_kept_though_the_book_does_not_declare_it() {
    let schema = schema();
    let drop = drops(&schema, &["note"]);
    let held = [("note", text("n")), ("undeclared", text("u"))];
    let form = Shelf::form_entries(&schema, &book("spec"), &written_in("spec", &held), &drop);
    assert_eq!(names_in(&form), ["id", "key", "title", "undeclared"]);
}

#[test]
fn the_build_of_the_form_is_the_build_of_the_entries_less_the_unknown_entry_of_a_dropped_name() {
    let schema = schema();
    let drop = drops(&schema, &["note", "tests"]);
    let held = [
        ("note", text("n")),
        ("tests", list(&["a.rs"])),
        ("undeclared", text("u")),
    ];
    let written = written_in("spec", &held);
    let form = Shelf::form_entries(&schema, &book("spec"), &written, &drop);
    let before = build_of(&schema, parsed("spec", &written));
    let after = build_of(&schema, parsed("spec", form.as_map()));
    let problems_of = |shelf: &Shelf| -> Vec<DocProblem> {
        shelf
            .invalid()
            .values()
            .flat_map(|files| files.iter())
            .flat_map(|file| file.problems.iter().cloned())
            .collect()
    };
    let unknown = |name: &str| DocProblem::UnknownEntry {
        name: name.to_owned(),
        book: book("spec"),
    };
    // `tests` is declared, so it is not unknown; `note` and `undeclared` are.
    assert_eq!(
        problems_of(&before),
        [unknown("note"), unknown("undeclared")]
    );
    assert_eq!(problems_of(&after), [unknown("undeclared")]);
}

#[test]
fn a_book_the_schema_does_not_declare_drops_nothing() {
    let schema = schema();
    let drop = drops(&schema, &["note"]);
    let written = entries(&[("note", text("n"))]);
    let form = Shelf::form_entries(&schema, &book("nope"), &written, &drop);
    assert_eq!(form.as_map(), &written);
    assert!(form.dropped().is_empty());
}

// ---- DropNames::new ---------------------------------------------------------------

#[test]
fn the_id_and_the_key_cannot_be_dropped() {
    let schema = schema();
    assert_eq!(
        problems(&schema, &["id"]),
        vec![DropProblem::Reserved(blank("id"))]
    );
    assert_eq!(
        problems(&schema, &["key"]),
        vec![DropProblem::Reserved(blank("key"))]
    );
}

#[test]
fn a_name_every_book_declares_cannot_be_dropped() {
    // `title` is a field of the three books.
    let schema = schema();
    assert_eq!(
        problems(&schema, &["title"]),
        vec![DropProblem::DeclaredEverywhere(blank("title"))]
    );
    // A name every book declares as another kind of entry counts too: here a link of
    // the same name in each.
    let mut decls = decls();
    for decl in &mut decls {
        decl.links
            .push(crate::domain::shelf::fixtures_tests::artifact_link(
                "evidence",
                Propagation::Informs,
            ));
    }
    let every = Schema::validate(decls, &spaces()).unwrap();
    assert_eq!(
        problems(&every, &["evidence"]),
        vec![DropProblem::DeclaredEverywhere(blank("evidence"))]
    );
}

#[test]
fn a_declaration_of_no_book_declares_no_name() {
    // With no book, no book declares the name: it is not refused as declared by every
    // book, and drops from no file.
    let empty = Schema::validate(Vec::new(), &spaces()).unwrap();
    assert_eq!(drops(&empty, &["retired"]).names().len(), 1);
}

#[test]
fn a_name_some_book_does_not_declare_fits_whether_or_not_any_file_holds_it() {
    let schema = schema();
    // `note` (a field) and `base` (a link) of `finding`, `tests` (a link to artifacts)
    // of `spec`, and a name no book declares.
    let names = drops(&schema, &["note", "base", "tests", "retired"]);
    assert_eq!(names.names().len(), 4);
    assert!(!names.is_empty());
    assert!(DropNames::none().is_empty());
    assert!(drops(&schema, &[]).is_empty());
}

#[test]
fn every_problem_is_collected_in_the_order_of_the_names() {
    let schema = schema();
    assert_eq!(
        problems(&schema, &["title", "note", "key", "id"]),
        vec![
            DropProblem::Reserved(blank("id")),
            DropProblem::Reserved(blank("key")),
            DropProblem::DeclaredEverywhere(blank("title")),
        ]
    );
    assert_eq!(
        DropProblem::Reserved(blank("id")).to_string(),
        "`id` cannot be dropped: every document has an `id` and a `key`"
    );
    assert!(
        DropProblem::DeclaredEverywhere(blank("title"))
            .to_string()
            .starts_with("every book declares `title`")
    );
}
