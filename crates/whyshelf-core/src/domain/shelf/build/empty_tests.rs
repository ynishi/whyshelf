//! Tests of what a value written with nothing in it, an item that says nothing and a
//! line break in a text come to: `Written::of` told once for every entry, the one
//! reader of items, and the type `line` (#128).

use super::*;
use crate::domain::shelf::fixtures_tests::{
    decls, docid, entries, list, path, reading, schema, shelf, sp_testing, spaces, text,
};
use crate::domain::shelf::schema::FieldTypeDecl;

fn problems_of(shelf: &Shelf, n: u128) -> Vec<DocProblem> {
    match shelf.state_of(docid(n)) {
        DocState::Invalid(docs) => docs
            .iter()
            .flat_map(|d| d.problems().iter().cloned())
            .collect(),
        other => panic!("not invalid: {other:?}"),
    }
}

fn hash_of(shelf: &Shelf, n: u128) -> DocHash {
    *shelf.doc(docid(n)).unwrap().hash()
}

fn entry(name: &str) -> EntryName {
    EntryName::new(name).unwrap()
}

fn said(text: &str) -> NonBlank {
    NonBlank::new(text).unwrap()
}

/// A finding with a title and this one more entry.
fn finding_with(n: u128, name: &str, value: RawValue) -> DocReading {
    reading(
        &format!("findings/fr-{n}.md"),
        n,
        &format!("FR-{n}"),
        &[("title", text("t")), (name, value)],
    )
}

/// A finding with only a title.
fn finding_bare(n: u128) -> DocReading {
    reading(
        &format!("findings/fr-{n}.md"),
        n,
        &format!("FR-{n}"),
        &[("title", text("t"))],
    )
}

/// The schema of the fixtures with the finding's `title` and `note` of the type `word`,
/// and `tags` required.
fn schema_with(title: &str, note: &str, tags_required: bool) -> Schema {
    let mut decls = decls();
    for field in &mut decls[0].fields {
        match field.name.as_str() {
            "title" => field.ty = FieldTypeDecl::Word(title.to_owned()),
            "note" => field.ty = FieldTypeDecl::Word(note.to_owned()),
            "tags" if tags_required => field.presence = FieldPresence::Required,
            _ => {}
        }
    }
    Schema::validate(decls, &spaces()).unwrap()
}

fn built(schema: &Schema, readings: Vec<DocReading>) -> Shelf {
    Shelf::build(schema, readings, &ArtifactReadings::new())
}

// ---- a field written with nothing in it ------------------------------------------

#[test]
fn an_optional_field_written_with_nothing_is_no_value_and_hashes_as_if_left_out() {
    let bare = shelf(vec![finding_bare(1)]);
    let want = hash_of(&bare, 1);
    // Whatever the declared type: a text, an integer, a bool, an enum, a list.
    let writings = [
        ("note", text("")),
        ("note", text("  ")),
        ("note", text("\t\n")),
        ("note", RawValue::Null),
        ("note", list(&[])),
        ("tags", list(&[])),
        ("tags", RawValue::Null),
        ("tags", text(" ")),
        ("count", text("")),
        ("count", text("  ")),
        ("count", RawValue::Null),
        ("flag", text(" ")),
        ("flag", RawValue::Null),
        ("level", text("")),
        ("level", RawValue::Null),
    ];
    for (name, written) in writings {
        let s = shelf(vec![finding_with(1, name, written.clone())]);
        assert!(s.invalid().is_empty(), "{name}: {written:?}");
        let doc = s.doc(docid(1)).unwrap();
        assert_eq!(doc.fields().get(name), None, "{name}: {written:?}");
        assert_eq!(hash_of(&s, 1), want, "{name}: {written:?}");
    }
}

#[test]
fn a_required_field_written_with_nothing_is_empty_and_one_left_out_is_missing() {
    for written in [RawValue::Null, text(""), text(" \t"), text("\n"), list(&[])] {
        let s = shelf(vec![reading(
            "findings/a.md",
            1,
            "FR-1",
            &[("title", written.clone())],
        )]);
        assert_eq!(
            problems_of(&s, 1),
            vec![DocProblem::FieldEmpty {
                field: entry("title")
            }],
            "{written:?}"
        );
    }
    let missing = shelf(vec![reading("findings/a.md", 1, "FR-1", &[])]);
    assert_eq!(
        problems_of(&missing, 1),
        vec![DocProblem::FieldMissing {
            field: entry("title")
        }]
    );
}

#[test]
fn a_required_list_written_with_nothing_is_empty() {
    let schema = schema_with("text", "text", true);
    for written in [list(&[]), RawValue::Null, text("")] {
        let s = built(
            &schema,
            vec![reading(
                "findings/a.md",
                1,
                "FR-1",
                &[("title", text("t")), ("tags", written.clone())],
            )],
        );
        assert_eq!(
            problems_of(&s, 1),
            vec![DocProblem::FieldEmpty {
                field: entry("tags")
            }],
            "{written:?}"
        );
    }
    let one = built(&schema, vec![finding_with(1, "tags", list(&["a"]))]);
    assert!(one.invalid().is_empty());
}

#[test]
fn a_text_that_says_something_is_kept_as_written_with_its_spaces_and_line_breaks() {
    let s = shelf(vec![finding_with(1, "note", text("  a\nb  "))]);
    assert_eq!(
        s.doc(docid(1)).unwrap().fields().get("note"),
        Some(&FieldValue::Text(said("  a\nb  ")))
    );
    let tags = shelf(vec![finding_with(1, "tags", list(&[" a ", "b"]))]);
    assert_eq!(
        tags.doc(docid(1)).unwrap().fields().get("tags"),
        Some(&FieldValue::TextList(NonEmptyVec::new(
            said(" a "),
            vec![said("b")]
        )))
    );
}

// ---- an item that says nothing ---------------------------------------------------

#[test]
fn an_item_that_says_nothing_is_a_problem_told_with_its_place() {
    for (items, at) in [
        (vec!["a", ""], 2),
        (vec!["a", "  ", "b"], 2),
        (vec!["\t"], 1),
    ] {
        let s = shelf(vec![finding_with(1, "tags", list(&items))]);
        assert_eq!(
            problems_of(&s, 1),
            vec![DocProblem::ItemBlank {
                entry: entry("tags"),
                at
            }],
            "{items:?}"
        );
    }
    assert_eq!(
        DocProblem::ItemBlank {
            entry: entry("tags"),
            at: 2
        }
        .to_string(),
        "the item 2 of `tags` is empty or blank"
    );
}

#[test]
fn every_item_that_says_nothing_is_a_problem_not_only_the_first() {
    let blank = |at| DocProblem::ItemBlank {
        entry: entry("tags"),
        at,
    };
    for (items, at) in [
        (vec!["", " "], vec![1, 2]),
        (vec!["", "a", "\t", " "], vec![1, 3, 4]),
    ] {
        let s = shelf(vec![finding_with(1, "tags", list(&items))]);
        let told: Vec<DocProblem> = at.iter().map(|at| blank(*at)).collect();
        assert_eq!(problems_of(&s, 1), told, "{items:?}");
    }
    // A link into a book tells each as well.
    let s = shelf(vec![finding_with(1, "base", list(&["", "FR-2", " "]))]);
    assert_eq!(
        problems_of(&s, 1),
        [1, 3]
            .iter()
            .map(|at| DocProblem::ItemBlank {
                entry: entry("base"),
                at: *at
            })
            .collect::<Vec<_>>()
    );
}

#[test]
fn an_item_that_says_nothing_does_not_stop_the_names_of_a_link_to_artifacts_being_judged() {
    let too_long = "a".repeat(crate::domain::shelf::artifact::NAME_MAX + 1);
    let s = shelf(vec![sp_testing(2, &[], &["", &too_long, "a\tb", " "])]);
    let tests = entry("tests");
    assert_eq!(
        problems_of(&s, 2),
        vec![
            DocProblem::ItemBlank {
                entry: tests.clone(),
                at: 1
            },
            DocProblem::ItemBlank {
                entry: tests.clone(),
                at: 4
            },
            DocProblem::ArtifactNameInvalid {
                link: tests.clone(),
                written: too_long,
                why: ArtifactNameProblem::TooLong {
                    bytes: crate::domain::shelf::artifact::NAME_MAX + 1
                },
            },
            DocProblem::ArtifactNameInvalid {
                link: tests,
                written: "a\tb".to_owned(),
                why: ArtifactNameProblem::ControlChar,
            },
        ]
    );
}

#[test]
fn an_item_that_says_nothing_does_not_hide_a_count_the_link_does_not_admit() {
    let mut d = decls();
    d[1].links[0].cardinality = Cardinality::One;
    let schema = Schema::validate(d, &spaces()).unwrap();
    let s = built(
        &schema,
        vec![reading(
            "specs/a.md",
            1,
            "SP-1",
            &[("title", text("t")), ("grounds", list(&["FR-1", ""]))],
        )],
    );
    let problems = problems_of(&s, 1);
    assert_eq!(problems.len(), 2, "{problems:?}");
    assert!(
        problems
            .iter()
            .any(|p| matches!(p, DocProblem::ItemBlank { at: 2, .. })),
        "{problems:?}"
    );
    assert!(
        problems
            .iter()
            .any(|p| matches!(p, DocProblem::LinkCount { found: 2, .. })),
        "{problems:?}"
    );
}

#[test]
fn an_item_that_says_nothing_is_a_problem_in_a_link_into_a_book_too() {
    let s = shelf(vec![finding_with(1, "base", list(&["FR-2", " "]))]);
    assert_eq!(
        problems_of(&s, 1),
        vec![DocProblem::ItemBlank {
            entry: entry("base"),
            at: 2
        }]
    );
}

#[test]
fn an_item_that_says_nothing_is_a_problem_in_a_link_to_artifacts_before_it_is_a_name() {
    let s = shelf(vec![sp_testing(2, &[], &["tests/a.rs", "  "])]);
    assert_eq!(
        problems_of(&s, 2),
        vec![DocProblem::ItemBlank {
            entry: entry("tests"),
            at: 2
        }]
    );
}

#[test]
fn the_names_of_a_list_with_an_item_that_says_nothing_are_asked_but_not_that_item() {
    let schema = schema();
    let asks = |tests: &[&str]| {
        Shelf::artifact_asks(&schema, &[sp_testing(2, &[], tests)])
            .iter()
            .count()
    };
    assert_eq!(asks(&["tests/a.rs"]), 1);
    // The build judges the other names of the list, so they are asked.
    assert_eq!(asks(&["tests/a.rs", " "]), 1);
    assert_eq!(asks(&[" "]), 0);
}

// ---- a link written with nothing in it -------------------------------------------

#[test]
fn a_link_written_with_nothing_is_a_link_with_no_key() {
    for written in [RawValue::Null, list(&[]), text(""), text("  ")] {
        let s = shelf(vec![finding_with(1, "base", written.clone())]);
        assert!(s.invalid().is_empty(), "{written:?}");
        let doc = s.doc(docid(1)).unwrap();
        assert!(doc.links()["base"].states().is_empty(), "{written:?}");
        let spec = shelf(vec![reading(
            "specs/a.md",
            2,
            "SP-2",
            &[("title", text("t")), ("tests", written.clone())],
        )]);
        assert!(spec.invalid().is_empty(), "{written:?}");
        let doc = spec.doc(docid(2)).unwrap();
        assert!(doc.artifact_links()["tests"].artifacts().is_empty());
    }
}

#[test]
fn a_link_written_with_nothing_is_counted_as_one_with_no_key() {
    let mut d = decls();
    d[1].links[0].cardinality = Cardinality::AtLeastOne;
    let schema = Schema::validate(d, &spaces()).unwrap();
    for written in [RawValue::Null, list(&[]), text(" ")] {
        let s = built(
            &schema,
            vec![reading(
                "specs/a.md",
                1,
                "SP-1",
                &[("title", text("t")), ("grounds", written.clone())],
            )],
        );
        assert!(
            matches!(
                problems_of(&s, 1).as_slice(),
                [DocProblem::LinkCount {
                    found: 0,
                    cardinality: Cardinality::AtLeastOne,
                    ..
                }]
            ),
            "{written:?}"
        );
    }
    // A text that says something is still no list.
    let s = built(
        &schema,
        vec![reading(
            "specs/a.md",
            1,
            "SP-1",
            &[("title", text("t")), ("grounds", text("FR-1"))],
        )],
    );
    assert!(matches!(
        problems_of(&s, 1).as_slice(),
        [DocProblem::LinkNotList {
            found: RawKind::Text,
            ..
        }]
    ));
}

// ---- the `id` and the `key` ------------------------------------------------------

fn file_with(id: Option<RawValue>, key: Option<RawValue>) -> DocReading {
    let mut all = entries(&[("title", text("t"))]);
    all.extend(id.map(|v| ("id".to_owned(), v)));
    all.extend(key.map(|v| ("key".to_owned(), v)));
    DocReading::Parsed {
        origin: path("findings/a.md"),
        entries: all,
        body: String::new(),
    }
}

#[test]
fn an_id_written_with_nothing_is_empty_and_its_file_may_still_carry_an_identity() {
    for written in [RawValue::Null, text(""), text("  "), list(&[])] {
        let s = shelf(vec![file_with(Some(written.clone()), Some(text("FR-1")))]);
        // Unreadable, not absent: an identity may be hidden in it.
        assert!(s.anonymous().is_empty(), "{written:?}");
        assert_eq!(s.unreadable().len(), 1, "{written:?}");
        assert_eq!(
            s.unreadable()[0].problems().first(),
            &DocProblem::IdEmpty,
            "{written:?}"
        );
    }
    assert_eq!(
        DocProblem::IdEmpty.to_string(),
        "`id:` is written empty or blank"
    );
    // A text that is no identifier is still told as it was.
    let bad = shelf(vec![file_with(Some(text("not-an-id")), Some(text("FR-1")))]);
    assert!(matches!(
        bad.unreadable()[0].problems().first(),
        DocProblem::IdInvalid(_)
    ));
}

#[test]
fn a_key_written_with_nothing_is_empty_and_not_missing() {
    for written in [RawValue::Null, text(""), text("  "), list(&[])] {
        let s = shelf(vec![file_with(
            Some(text(&docid(1).to_string())),
            Some(written.clone()),
        )]);
        assert_eq!(
            problems_of(&s, 1),
            vec![DocProblem::KeyEmpty],
            "{written:?}"
        );
    }
    let missing = shelf(vec![file_with(Some(text(&docid(1).to_string())), None)]);
    assert_eq!(problems_of(&missing, 1), vec![DocProblem::KeyMissing]);
    assert_eq!(
        DocProblem::KeyEmpty.to_string(),
        "`key:` is written empty or blank"
    );
}

#[test]
fn a_file_under_no_book_with_an_empty_id_is_unreadable_too() {
    let outside = DocReading::Parsed {
        origin: path("elsewhere/a.md"),
        entries: entries(&[("id", RawValue::Null)]),
        body: String::new(),
    };
    let s = shelf(vec![outside]);
    assert_eq!(s.unreadable().len(), 1);
    assert!(s.anonymous().is_empty());
}

// ---- the type `line` -------------------------------------------------------------

#[test]
fn a_line_holds_one_line_and_a_line_break_is_a_problem_even_at_the_end() {
    let schema = schema_with("line", "line", false);
    let s = built(
        &schema,
        vec![
            reading("findings/a.md", 1, "FR-1", &[("title", text("one line"))]),
            reading("findings/b.md", 2, "FR-2", &[("title", text("two\nlines"))]),
            // What a `|` block of YAML leaves.
            reading("findings/c.md", 3, "FR-3", &[("title", text("kept\n"))]),
            reading("findings/d.md", 4, "FR-4", &[("title", text("crlf\r\nx"))]),
            reading(
                "findings/e.md",
                5,
                "FR-5",
                &[("title", text("t")), ("note", text("a\nb"))],
            ),
        ],
    );
    assert_eq!(
        s.doc(docid(1)).unwrap().fields().get("title"),
        Some(&FieldValue::Line(OneLine::new("one line").unwrap()))
    );
    for n in [2, 3, 4] {
        assert_eq!(
            problems_of(&s, n),
            vec![DocProblem::NotOneLine {
                field: entry("title")
            }],
            "{n}"
        );
    }
    assert_eq!(
        problems_of(&s, 5),
        vec![DocProblem::NotOneLine {
            field: entry("note")
        }]
    );
    assert!(
        DocProblem::NotOneLine {
            field: entry("title")
        }
        .to_string()
        .starts_with("the field `title` is a text of more than one line, not `line`")
    );
}

#[test]
fn a_line_written_with_nothing_is_told_as_any_field_is() {
    let schema = schema_with("line", "line", false);
    for written in [RawValue::Null, text(""), text("  "), text("\n")] {
        let s = built(
            &schema,
            vec![
                reading("findings/a.md", 1, "FR-1", &[("title", written.clone())]),
                reading(
                    "findings/b.md",
                    2,
                    "FR-2",
                    &[("title", text("t")), ("note", written.clone())],
                ),
            ],
        );
        // Required: empty, a line break or not. Optional: no value.
        assert_eq!(
            problems_of(&s, 1),
            vec![DocProblem::FieldEmpty {
                field: entry("title")
            }],
            "{written:?}"
        );
        assert_eq!(s.doc(docid(2)).unwrap().fields().get("note"), None);
    }
}

#[test]
fn a_line_of_another_kind_is_a_mismatch_told_with_the_word_line() {
    let schema = schema_with("line", "text", false);
    let s = built(
        &schema,
        vec![reading(
            "findings/a.md",
            1,
            "FR-1",
            &[("title", RawValue::Integer(1))],
        )],
    );
    let problems = problems_of(&s, 1);
    assert_eq!(
        problems,
        vec![DocProblem::FieldMismatch {
            field: entry("title"),
            expected: FieldType::Scalar(ScalarType::Line),
            found: RawKind::Integer,
        }]
    );
    assert!(problems[0].to_string().contains("not `line`"));
}

#[test]
fn a_line_and_a_text_of_the_same_value_have_the_same_hash() {
    let as_text = built(&schema_with("text", "text", false), vec![finding_bare(1)]);
    let as_line = built(&schema_with("line", "line", false), vec![finding_bare(1)]);
    assert_eq!(hash_of(&as_text, 1), hash_of(&as_line, 1));
}
