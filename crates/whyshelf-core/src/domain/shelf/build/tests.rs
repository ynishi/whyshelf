//! Tests of the shelf.

use super::*;
use crate::domain::nonempty::NonEmptySet;
use crate::domain::shelf::fixtures_tests::{
    decls, docid, entries, fr, fr_titled, list, path, reading, schema, shelf, shelf_reading, sided,
    spaces, text, unreadable_at,
};
use crate::domain::shelf::graph::CycleId;
use crate::domain::shelf::schema::{Cardinality, FieldEntryDecl, FieldTypeDecl, FieldWord};

fn problems_of(shelf: &Shelf, n: u128) -> Vec<DocProblem> {
    match shelf.state_of(docid(n)) {
        DocState::Invalid(docs) => docs
            .iter()
            .flat_map(|d| d.problems().iter().cloned())
            .collect(),
        other => panic!("not invalid: {other:?}"),
    }
}

fn links(shelf: &Shelf, n: u128, name: &str) -> Vec<LinkState> {
    shelf
        .doc(docid(n))
        .unwrap()
        .links()
        .get(name)
        .unwrap()
        .states()
        .to_vec()
}

fn hash_of(shelf: &Shelf, n: u128) -> DocHash {
    *shelf.doc(docid(n)).unwrap().hash()
}

fn form(key: &str) -> KeyForm {
    schema().book("finding").unwrap().key().parse(key).unwrap()
}

fn finding() -> BookName {
    BookName::new("finding").unwrap()
}

#[test]
fn a_well_formed_file_is_a_valid_doc() {
    let s = shelf(vec![reading(
        "findings/fr-1.md",
        1,
        "FR-1",
        &[
            ("title", text("t")),
            ("note", text("n")),
            ("count", RawValue::Integer(3)),
            ("flag", RawValue::Bool(true)),
            ("tags", list(&["a", "b"])),
            ("level", text("high")),
        ],
    )]);
    let doc = s.doc(docid(1)).unwrap();
    assert_eq!(doc.key().to_string(), "finding:FR-1");
    assert_eq!(doc.origin().as_str(), "findings/fr-1.md");
    assert_eq!(doc.body(), "body\n");
    assert_eq!(doc.fields().len(), 6);
    assert_eq!(doc.fields().get("count"), Some(&FieldValue::Integer(3)));
    assert_eq!(
        doc.fields().get("level"),
        Some(&FieldValue::Enum(FieldWord::new("high").unwrap()))
    );
    assert!(s.invalid().is_empty());
    // Every link of the book is there, empty when none was written.
    assert_eq!(doc.links().len(), 3);
    assert!(links(&s, 1, "base").is_empty());
}

#[test]
fn a_file_under_no_book_is_outside_the_books() {
    let s = shelf(vec![reading(
        "notes/n.md",
        1,
        "FR-1",
        &[("title", text("t"))],
    )]);
    assert_eq!(problems_of(&s, 1), vec![DocProblem::OutsideBooks]);
}

#[test]
fn a_file_under_no_book_and_with_no_id_is_anonymous() {
    let s = shelf(vec![DocReading::Parsed {
        origin: path("notes/n.md"),
        entries: entries(&[]),
        body: String::new(),
    }]);
    assert!(s.unreadable().is_empty());
    assert_eq!(s.anonymous().len(), 1);
    assert_eq!(
        s.anonymous()[0].problems().first(),
        &DocProblem::OutsideBooks
    );
}

#[test]
fn an_identity_is_gone_when_only_files_with_no_id_exist() {
    let s = shelf(vec![
        fr(1, &[]),
        DocReading::Parsed {
            origin: path("findings/a.md"),
            entries: entries(&[("key", text("FR-2")), ("title", text("t"))]),
            body: String::new(),
        },
    ]);
    assert_eq!(s.anonymous().len(), 1);
    assert!(s.unreadable().is_empty());
    assert_eq!(s.state_of(docid(9)), DocState::Gone);
    assert_eq!(s.state_of(docid(9)).presence(), DocPresence::Gone);
}

#[test]
fn an_identity_is_unknown_when_an_unparsed_file_has_an_unreadable_id_beside_id_less_files() {
    let s = shelf(vec![
        DocReading::Parsed {
            origin: path("findings/a.md"),
            entries: entries(&[("key", text("FR-2")), ("title", text("t"))]),
            body: String::new(),
        },
        DocReading::Unparsed {
            origin: path("findings/x.md"),
            id: ReadId::Unreadable,
            why: RawProblem::Syntax { line: None },
        },
    ]);
    assert_eq!(s.anonymous().len(), 1);
    assert_eq!(s.unreadable().len(), 1);
    assert!(matches!(s.state_of(docid(9)), DocState::Unknown { .. }));
}

#[test]
fn an_identity_may_be_in_its_files_or_in_any_whose_identity_cannot_be_read() {
    let in_files = |s: &Shelf, n: u128| -> Vec<String> {
        s.state_of(docid(n))
            .may_be_in()
            .into_iter()
            .map(|p| p.as_str().to_owned())
            .collect()
    };
    let s = shelf(vec![
        fr(1, &[]),
        reading("notes/n.md", 2, "FR-2", &[("title", text("t"))]),
    ]);
    assert_eq!(in_files(&s, 1), [fr(1, &[]).origin().as_str()]);
    assert_eq!(in_files(&s, 2), ["notes/n.md"]);
    assert!(in_files(&s, 9).is_empty(), "gone");
    let s = shelf(vec![DocReading::Unparsed {
        origin: path("findings/x.md"),
        id: ReadId::Unreadable,
        why: RawProblem::Syntax { line: None },
    }]);
    assert_eq!(in_files(&s, 9), ["findings/x.md"]);
    // The files known to carry it are none.
    assert!(s.state_of(docid(9)).files().is_empty());
}

#[test]
fn the_id_must_be_there_and_be_an_identifier() {
    let no_id = DocReading::Parsed {
        origin: path("findings/a.md"),
        entries: entries(&[("key", text("FR-1")), ("title", text("t"))]),
        body: String::new(),
    };
    let not_text = DocReading::Parsed {
        origin: path("findings/b.md"),
        entries: entries(&[
            ("id", RawValue::Integer(5)),
            ("key", text("FR-2")),
            ("title", text("t")),
        ]),
        body: String::new(),
    };
    let bad = DocReading::Parsed {
        origin: path("findings/c.md"),
        entries: entries(&[
            ("id", text("not-a-uuid")),
            ("key", text("FR-3")),
            ("title", text("t")),
        ]),
        body: String::new(),
    };
    let s = shelf(vec![no_id, not_text, bad]);
    // No id at all is anonymous; an id that cannot be read is unreadable.
    assert_eq!(s.anonymous().len(), 1);
    assert_eq!(s.anonymous()[0].problems().first(), &DocProblem::IdMissing);
    let kinds: Vec<_> = s
        .unreadable()
        .iter()
        .map(|d| d.problems().first().clone())
        .collect();
    assert_eq!(kinds.len(), 2);
    assert!(kinds.contains(&DocProblem::IdNotText));
    assert!(kinds.iter().any(|p| matches!(p, DocProblem::IdInvalid(_))));
}

#[test]
fn the_key_must_be_there_and_fit_the_book() {
    let missing = DocReading::Parsed {
        origin: path("findings/a.md"),
        entries: entries(&[("id", text(&docid(1).to_string())), ("title", text("t"))]),
        body: String::new(),
    };
    let not_text = DocReading::Parsed {
        origin: path("findings/b.md"),
        entries: entries(&[
            ("id", text(&docid(2).to_string())),
            ("key", RawValue::Bool(true)),
            ("title", text("t")),
        ]),
        body: String::new(),
    };
    let s = shelf(vec![
        missing,
        not_text,
        reading("findings/c.md", 3, "SP-3", &[("title", text("t"))]),
        reading("findings/d.md", 4, "FR-04", &[("title", text("t"))]),
    ]);
    assert_eq!(problems_of(&s, 1), vec![DocProblem::KeyMissing]);
    assert_eq!(problems_of(&s, 2), vec![DocProblem::KeyNotText]);
    assert!(matches!(
        problems_of(&s, 3).as_slice(),
        [DocProblem::KeyInvalid(KeyProblem::WrongPrefix { .. })]
    ));
    assert!(matches!(
        problems_of(&s, 4).as_slice(),
        [DocProblem::KeyInvalid(KeyProblem::NumberLeadingZero)]
    ));
}

#[test]
fn a_prefixed_key_that_could_be_hexadecimal_is_a_key() {
    let mut d = decls();
    d[0].key = crate::domain::shelf::schema::KeyPatternDecl::Prefixed {
        prefix: "CAFE".to_owned(),
    };
    let schema = Schema::validate(d, &spaces()).unwrap();
    let s = Shelf::build(
        &schema,
        vec![reading(
            "findings/c.md",
            1,
            "CAFE-1234",
            &[("title", text("t"))],
        )],
        &Default::default(),
    );
    assert!(s.doc(docid(1)).is_some());
}

#[test]
fn the_same_id_in_two_files_makes_both_invalid() {
    let s = shelf(vec![
        reading("findings/a.md", 1, "FR-1", &[("title", text("a"))]),
        reading("findings/b.md", 1, "FR-3", &[("title", text("b"))]),
        fr(2, &[]),
    ]);
    match s.state_of(docid(1)) {
        DocState::Invalid(docs) => {
            assert_eq!(docs.len(), 2);
            for d in docs.iter() {
                assert!(
                    d.problems().iter().any(
                        |p| matches!(p, DocProblem::DuplicateId { others } if others.len() == 1)
                    )
                );
            }
        }
        other => panic!("{other:?}"),
    }
    assert!(s.doc(docid(1)).is_none());
    assert!(s.doc(docid(2)).is_some());
}

#[test]
fn the_same_key_in_a_book_makes_every_holder_invalid() {
    let s = shelf(vec![
        reading("findings/a.md", 1, "FR-9", &[("title", text("a"))]),
        reading("findings/b.md", 2, "FR-9", &[("title", text("b"))]),
        reading("findings/c.md", 3, "FR-9", &[("title", text("c"))]),
        // The same text is a different key in another book.
        reading("specs/a.md", 4, "SP-9", &[("title", text("d"))]),
    ]);
    for n in [1, 2, 3] {
        assert!(matches!(
            problems_of(&s, n).as_slice(),
            [DocProblem::DuplicateKey { others, .. }] if others.len() == 2
        ));
    }
    assert!(s.doc(docid(4)).is_some());
}

#[test]
fn an_entry_the_book_does_not_declare_is_a_problem() {
    let s = shelf(vec![reading(
        "findings/a.md",
        1,
        "FR-1",
        &[("title", text("t")), ("surprise", text("x"))],
    )]);
    assert_eq!(
        problems_of(&s, 1),
        vec![DocProblem::UnknownEntry {
            name: "surprise".to_owned(),
            book: BookName::new("finding").unwrap(),
        }]
    );
}

#[test]
fn a_required_field_must_be_there() {
    let s = shelf(vec![reading("findings/a.md", 1, "FR-1", &[])]);
    assert_eq!(
        problems_of(&s, 1),
        vec![DocProblem::FieldMissing {
            field: EntryName::new("title").unwrap()
        }]
    );
}

fn title() -> EntryName {
    EntryName::new("title").unwrap()
}

/// The schema of the fixtures with the finding's `tags` list required.
fn schema_with_required_tags() -> Schema {
    let mut decls = decls();
    let tags = decls[0]
        .fields
        .iter_mut()
        .find(|f| f.name == "tags")
        .expect("the finding has tags");
    tags.presence = FieldPresence::Required;
    Schema::validate(decls, &spaces()).unwrap()
}

#[test]
fn a_required_text_that_says_nothing_is_empty() {
    for written in ["", "   ", "\t\n"] {
        let s = shelf(vec![reading(
            "findings/a.md",
            1,
            "FR-1",
            &[("title", text(written))],
        )]);
        assert_eq!(
            problems_of(&s, 1),
            vec![DocProblem::FieldEmpty { field: title() }],
            "title {written:?}"
        );
    }
}

#[test]
fn a_required_text_with_a_character_is_a_value_even_after_a_line_break() {
    let s = shelf(vec![reading(
        "findings/a.md",
        1,
        "FR-1",
        &[("title", text("\nsecond line"))],
    )]);
    assert!(s.invalid().is_empty());
    assert_eq!(
        s.doc(docid(1)).unwrap().fields().get("title"),
        Some(&FieldValue::Text(
            NonBlank::new("\nsecond line").expect("a text that says something")
        ))
    );
}

#[test]
fn a_required_list_with_no_item_is_empty() {
    let schema = schema_with_required_tags();
    let readings = |tags: &[&str]| {
        vec![reading(
            "findings/a.md",
            1,
            "FR-1",
            &[("title", text("t")), ("tags", list(tags))],
        )]
    };
    let empty = shelf_reading(&schema, readings(&[]), &[]);
    assert_eq!(
        problems_of(&empty, 1),
        vec![DocProblem::FieldEmpty {
            field: EntryName::new("tags").unwrap()
        }]
    );
    let one = shelf_reading(&schema, readings(&["a"]), &[]);
    assert!(one.invalid().is_empty());
}

#[test]
fn a_blank_required_field_is_told_before_a_missing_one() {
    let mut decls = decls();
    decls[0].fields.insert(
        0,
        FieldEntryDecl {
            name: "summary".to_owned(),
            ty: FieldTypeDecl::Word("text".to_owned()),
            presence: FieldPresence::Required,
        },
    );
    let schema = Schema::validate(decls, &spaces()).unwrap();
    // `title` is written blank, `summary` is left out; the declaration writes
    // `summary` first, so only the build's order of steps puts the empty one first.
    let s = shelf_reading(
        &schema,
        vec![reading("findings/a.md", 1, "FR-1", &[("title", text(" "))])],
        &[],
    );
    assert_eq!(
        problems_of(&s, 1),
        vec![
            DocProblem::FieldEmpty { field: title() },
            DocProblem::FieldMissing {
                field: EntryName::new("summary").unwrap()
            },
        ]
    );
}

#[test]
fn a_field_must_have_the_type_the_book_declares() {
    let s = shelf(vec![reading(
        "findings/a.md",
        1,
        "FR-1",
        &[
            ("title", RawValue::Integer(1)),
            ("count", text("3")),
            ("flag", RawValue::Integer(1)),
            ("tags", text("a")),
            ("note", list(&["a"])),
        ],
    )]);
    let problems = problems_of(&s, 1);
    assert_eq!(problems.len(), 5);
    assert!(
        problems
            .iter()
            .all(|p| matches!(p, DocProblem::FieldMismatch { .. }))
    );
    assert!(problems.contains(&DocProblem::FieldMismatch {
        field: EntryName::new("count").unwrap(),
        expected: FieldType::Scalar(ScalarType::Integer),
        found: RawKind::Text,
    }));
}

#[test]
fn an_enum_field_takes_only_its_words() {
    let s = shelf(vec![
        reading(
            "findings/a.md",
            1,
            "FR-1",
            &[("title", text("t")), ("level", text("medium"))],
        ),
        reading(
            "findings/b.md",
            2,
            "FR-2",
            &[("title", text("t")), ("level", RawValue::Integer(1))],
        ),
        reading(
            "findings/c.md",
            3,
            "FR-3",
            &[("title", text("t")), ("level", text("Low"))],
        ),
    ]);
    assert_eq!(
        problems_of(&s, 1),
        vec![DocProblem::FieldNotInEnum {
            field: EntryName::new("level").unwrap(),
            found: "medium".to_owned()
        }]
    );
    assert!(matches!(
        problems_of(&s, 2).as_slice(),
        [DocProblem::FieldMismatch { .. }]
    ));
    assert!(matches!(
        problems_of(&s, 3).as_slice(),
        [DocProblem::FieldNotInEnum { .. }]
    ));
}

#[test]
fn a_link_must_be_a_list_of_keys_within_its_cardinality() {
    let mut d = decls();
    // `grounds` of a spec: at least one. `base` of a finding: one at most.
    d[1].links[0].cardinality = Cardinality::AtLeastOne;
    d[0].links[0].cardinality = Cardinality::Optional;
    let schema = Schema::validate(d, &spaces()).unwrap();
    let s = Shelf::build(
        &schema,
        vec![
            reading("specs/a.md", 1, "SP-1", &[("title", text("t"))]),
            reading(
                "specs/b.md",
                2,
                "SP-2",
                &[("title", text("t")), ("grounds", text("FR-1"))],
            ),
            reading(
                "findings/c.md",
                3,
                "FR-3",
                &[("title", text("t")), ("base", list(&["FR-1", "FR-2"]))],
            ),
            reading("findings/d.md", 4, "FR-4", &[("title", text("t"))]),
        ],
        &Default::default(),
    );
    assert!(matches!(
        problems_of(&s, 1).as_slice(),
        [DocProblem::LinkCount {
            found: 0,
            cardinality: Cardinality::AtLeastOne,
            ..
        }]
    ));
    assert!(matches!(
        problems_of(&s, 2).as_slice(),
        [DocProblem::LinkNotList {
            found: RawKind::Text,
            ..
        }]
    ));
    assert!(matches!(
        problems_of(&s, 3).as_slice(),
        [DocProblem::LinkCount {
            found: 2,
            cardinality: Cardinality::Optional,
            ..
        }]
    ));
    assert!(s.doc(docid(4)).is_some());
}

#[test]
fn the_problems_of_a_count_a_type_and_a_book_are_told_in_the_words_of_the_declaration() {
    let link = || EntryName::new("grounds").unwrap();
    let count = |cardinality, found| DocProblem::LinkCount {
        link: link(),
        cardinality,
        found,
    };
    assert_eq!(
        count(Cardinality::Optional, 2).to_string(),
        "the link `grounds` has 2 keys, which its count `optional` (none or one) does not admit"
    );
    assert_eq!(
        count(Cardinality::One, 0).to_string(),
        "the link `grounds` has 0 keys, which its count `one` (exactly one) does not admit"
    );
    assert_eq!(
        count(Cardinality::AtLeastOne, 1).to_string(),
        "the link `grounds` has 1 key, which its count `at-least-one` (one or more) does not admit"
    );
    let mismatch = |expected, found| {
        DocProblem::FieldMismatch {
            field: EntryName::new("n").unwrap(),
            expected,
            found,
        }
        .to_string()
    };
    assert_eq!(
        mismatch(FieldType::Scalar(ScalarType::Integer), RawKind::Text),
        "the field `n` is a text, not `integer`"
    );
    assert_eq!(
        mismatch(FieldType::Scalar(ScalarType::TextList), RawKind::Integer),
        "the field `n` is a whole number, not `list`"
    );
    let words = std::iter::once(FieldWord::new("low").unwrap()).collect();
    assert_eq!(
        mismatch(
            FieldType::Enum(NonEmptySet::from_set(words).unwrap()),
            RawKind::Bool
        ),
        "the field `n` is a truth value, not `enum`"
    );
    assert_eq!(
        DocProblem::LinkNotList {
            link: link(),
            found: RawKind::Text
        }
        .to_string(),
        "the link `grounds` is a text, not a list of keys"
    );
    assert_eq!(
        DocProblem::LinkNotList {
            link: link(),
            found: RawKind::List
        }
        .to_string(),
        "the link `grounds` is a list, not a list of keys"
    );
    assert_eq!(
        DocProblem::UnknownEntry {
            name: "source".to_owned(),
            book: BookName::new("note").unwrap(),
        }
        .to_string(),
        "the book `note` declares no field or link `source`: declare it in the book first"
    );
}

#[test]
fn a_problem_found_in_a_file_is_told_without_the_name_of_a_rust_type() {
    let mut d = decls();
    d[1].links[0].cardinality = Cardinality::Optional;
    let schema = Schema::validate(d, &spaces()).unwrap();
    let s = Shelf::build(
        &schema,
        vec![
            reading(
                "specs/a.md",
                1,
                "SP-1",
                &[("title", text("t")), ("grounds", list(&["FR-1", "FR-2"]))],
            ),
            reading(
                "findings/b.md",
                2,
                "FR-1",
                &[
                    ("title", text("t")),
                    ("count", text("x")),
                    ("what", text("y")),
                ],
            ),
        ],
        &Default::default(),
    );
    let said: Vec<String> = [1, 2]
        .into_iter()
        .flat_map(|n| problems_of(&s, n))
        .map(|p| p.to_string())
        .collect();
    assert!(
        said.contains(
            &"the link `grounds` has 2 keys, which its count `optional` (none or one) does not admit"
                .to_owned()
        ),
        "{said:?}"
    );
    assert!(
        said.contains(&"the field `count` is a text, not `integer`".to_owned()),
        "{said:?}"
    );
    assert!(
        said.contains(
            &"the book `finding` declares no field or link `what`: declare it in the book first"
                .to_owned()
        ),
        "{said:?}"
    );
    for text in &said {
        for debug in ["Optional", "Integer", "Text", "AtLeastOne"] {
            assert!(!text.contains(debug), "{text}");
        }
    }
}

#[test]
fn every_problem_of_a_file_is_listed() {
    let s = shelf(vec![reading(
        "findings/a.md",
        1,
        "FR-1",
        &[("surprise", text("x")), ("count", text("x"))],
    )]);
    let problems = problems_of(&s, 1);
    assert_eq!(problems.len(), 3);
    assert!(
        problems
            .iter()
            .any(|p| matches!(p, DocProblem::FieldMissing { .. }))
    );
    assert!(
        problems
            .iter()
            .any(|p| matches!(p, DocProblem::FieldMismatch { .. }))
    );
    assert!(
        problems
            .iter()
            .any(|p| matches!(p, DocProblem::UnknownEntry { .. }))
    );
}

#[test]
fn a_link_resolves_to_the_valid_document_with_the_key() {
    let s = shelf(vec![fr(1, &[]), fr(2, &["FR-1"])]);
    match links(&s, 2, "base").as_slice() {
        // It carries what the shelf has for the target: its identity, key and hash.
        [LinkState::Resolved(target)] => {
            assert_eq!(target.id(), docid(1));
            assert_eq!(target.key().to_string(), "finding:FR-1");
            assert_eq!(*target.hash(), hash_of(&s, 1));
        }
        other => panic!("{other:?}"),
    }
    // And the link carries its kind.
    let link = s.doc(docid(2)).unwrap().links().get("base").unwrap();
    assert_eq!(link.kind().propagation, Propagation::Depends);
    assert_eq!(link.kind().target, finding());
}

#[test]
fn a_link_in_the_wrong_grammar_is_malformed() {
    let s = shelf(vec![
        fr(1, &["nonsense"]),
        fr(2, &["SP-1"]),
        fr(3, &["FR-01"]),
    ]);
    for n in [1, 2, 3] {
        assert!(matches!(
            links(&s, n, "base").as_slice(),
            [LinkState::Unresolved(u)] if matches!(u.why(), UnresolvedWhy::Malformed(_))
        ));
    }
    match links(&s, 1, "base").as_slice() {
        [LinkState::Unresolved(u)] => {
            assert_eq!(u.written(), "nonsense");
            // It points into the book, and reads as no key there.
            assert_eq!(*u.book(), finding());
            assert_eq!(u.key(), None);
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn a_link_to_no_key_is_missing() {
    let s = shelf(vec![fr(1, &["FR-99"])]);
    match links(&s, 1, "base").as_slice() {
        [LinkState::Unresolved(u)] => {
            assert_eq!(*u.why(), UnresolvedWhy::Missing);
            // The key it was read as is kept, so nothing reads the text again.
            assert_eq!(
                u.key().map(ToString::to_string).as_deref(),
                Some("finding:FR-99")
            );
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn a_link_to_a_key_two_documents_have_is_ambiguous() {
    let s = shelf(vec![
        reading("findings/a.md", 5, "FR-5", &[("title", text("a"))]),
        reading("findings/b.md", 6, "FR-5", &[("title", text("b"))]),
        fr(7, &["FR-5"]),
    ]);
    match links(&s, 7, "base").as_slice() {
        [LinkState::Unresolved(u)] => match u.why() {
            UnresolvedWhy::Ambiguous(collision) => {
                assert_eq!(
                    collision.ids().into_iter().collect::<Vec<_>>(),
                    vec![docid(5), docid(6)]
                );
                assert_eq!(
                    collision
                        .files()
                        .map(ToString::to_string)
                        .collect::<Vec<_>>(),
                    ["findings/a.md", "findings/b.md"]
                );
            }
            other => panic!("{other:?}"),
        },
        other => panic!("{other:?}"),
    }
}

#[test]
fn a_link_to_an_invalid_document_cannot_be_stood_on() {
    let s = shelf(vec![
        reading(
            "findings/a.md",
            8,
            "FR-8",
            &[("title", text("a")), ("surprise", text("x"))],
        ),
        fr(9, &["FR-8"]),
    ]);
    assert!(matches!(
        links(&s, 9, "base").as_slice(),
        [LinkState::Unresolved(u)] if *u.why() == UnresolvedWhy::TargetInvalid(docid(8))
    ));
}

#[test]
fn find_key_tells_its_four_answers() {
    let s = shelf(vec![
        fr(1, &[]),
        reading(
            "findings/b.md",
            2,
            "FR-2",
            &[("title", text("b")), ("surprise", text("x"))],
        ),
        reading("findings/c.md", 3, "FR-3", &[("title", text("c"))]),
        reading("findings/d.md", 4, "FR-3", &[("title", text("d"))]),
    ]);
    assert_eq!(
        s.find_key(&finding(), &form("FR-1")),
        KeyHit::Valid(docid(1))
    );
    assert_eq!(
        s.find_key(&finding(), &form("FR-2")),
        KeyHit::Invalid(docid(2))
    );
    assert!(matches!(
        s.find_key(&finding(), &form("FR-3")),
        KeyHit::Ambiguous(ids) if ids.len() == 2
    ));
    assert_eq!(s.find_key(&finding(), &form("FR-4")), KeyHit::Missing);
    // The same key in another book is another key.
    assert_eq!(
        s.find_key(&BookName::new("spec").unwrap(), &form("FR-1")),
        KeyHit::Missing
    );
}

#[test]
fn the_same_id_and_key_twice_is_one_invalid_identity_not_an_ambiguity() {
    let s = shelf(vec![
        reading("findings/a.md", 1, "FR-1", &[("title", text("a"))]),
        reading("findings/b.md", 1, "FR-1", &[("title", text("b"))]),
    ]);
    assert_eq!(
        s.find_key(&finding(), &form("FR-1")),
        KeyHit::Invalid(docid(1))
    );
}

#[test]
fn renaming_an_upstream_key_does_not_change_the_downstream_hash() {
    let before = shelf(vec![fr(1, &[]), fr(2, &["FR-1"])]);
    let after = shelf(vec![
        reading("findings/fr-1.md", 1, "FR-10", &[("title", text("title"))]),
        fr(2, &["FR-10"]),
    ]);
    assert_eq!(hash_of(&before, 2), hash_of(&after, 2));
    // Nor does the key of the document itself.
    assert_eq!(hash_of(&before, 1), hash_of(&after, 1));
}

#[test]
fn the_hash_covers_the_body_after_the_normalization_whatever_the_adapter_did() {
    let with_body = |body: &str| {
        let DocReading::Parsed {
            origin, entries, ..
        } = fr(1, &[])
        else {
            unreachable!("fr is parsed")
        };
        shelf(vec![DocReading::Parsed {
            origin,
            entries,
            body: body.to_owned(),
        }])
    };
    let normalized = hash_of(&with_body("one\ntwo\n"), 1);
    assert_eq!(hash_of(&with_body("one  \r\ntwo\t\r\n\r\n"), 1), normalized);
    assert_ne!(hash_of(&with_body("one\nthree\n"), 1), normalized);
}

#[test]
fn pointing_a_link_elsewhere_does_not_change_the_hash() {
    let a = shelf(vec![fr(1, &[]), fr(3, &[]), fr(2, &["FR-1"])]);
    let b = shelf(vec![fr(1, &[]), fr(3, &[]), fr(2, &["FR-3"])]);
    let c = shelf(vec![fr(1, &[]), fr(2, &["FR-nowhere"])]);
    assert_eq!(hash_of(&a, 2), hash_of(&b, 2));
    assert_eq!(hash_of(&a, 2), hash_of(&c, 2));
}

#[test]
fn moving_the_file_within_its_book_does_not_change_the_hash() {
    let a = shelf(vec![reading(
        "findings/fr-1.md",
        1,
        "FR-1",
        &[("title", text("title"))],
    )]);
    let b = shelf(vec![reading(
        "findings/sub/elsewhere.md",
        1,
        "FR-1",
        &[("title", text("title"))],
    )]);
    assert_eq!(hash_of(&a, 1), hash_of(&b, 1));
}

#[test]
fn changing_a_field_or_the_body_changes_the_hash() {
    let base = hash_of(&shelf(vec![fr(1, &[])]), 1);
    let retitled = hash_of(&shelf(vec![fr_titled(1, "other", &[])]), 1);
    assert_ne!(base, retitled);

    let extra_field = hash_of(
        &shelf(vec![reading(
            "findings/fr-1.md",
            1,
            "FR-1",
            &[("title", text("title")), ("note", text("n"))],
        )]),
        1,
    );
    assert_ne!(base, extra_field);

    let rebodied = DocReading::Parsed {
        origin: path("findings/fr-1.md"),
        entries: entries(&[
            ("id", text(&docid(1).to_string())),
            ("key", text("FR-1")),
            ("title", text("title")),
        ]),
        body: "other\n".to_owned(),
    };
    assert_ne!(base, hash_of(&shelf(vec![rebodied]), 1));
}

#[test]
fn a_field_that_changes_type_changes_the_hash() {
    // The same text as an enum word and as a free text.
    let as_text = shelf(vec![reading(
        "findings/a.md",
        1,
        "FR-1",
        &[("title", text("title")), ("note", text("high"))],
    )]);
    let as_enum = shelf(vec![reading(
        "findings/a.md",
        1,
        "FR-1",
        &[("title", text("title")), ("level", text("high"))],
    )]);
    assert_ne!(hash_of(&as_text, 1), hash_of(&as_enum, 1));
}

#[test]
fn moving_a_document_to_another_book_changes_the_hash() {
    let in_finding = shelf(vec![reading(
        "findings/a.md",
        1,
        "FR-1",
        &[("title", text("title"))],
    )]);
    let in_spec = shelf(vec![reading(
        "specs/a.md",
        1,
        "SP-1",
        &[("title", text("title"))],
    )]);
    assert_ne!(hash_of(&in_finding, 1), hash_of(&in_spec, 1));
}

#[test]
fn a_directory_that_is_moved_in_the_schema_moves_its_files_out() {
    let mut d = decls();
    d[0].dir = "findings-v2".to_owned();
    let moved = Shelf::build(
        &Schema::validate(d, &spaces()).unwrap(),
        vec![reading(
            "findings/a.md",
            1,
            "FR-1",
            &[("title", text("title"))],
        )],
        &Default::default(),
    );
    assert_eq!(problems_of(&moved, 1), vec![DocProblem::OutsideBooks]);
}

#[test]
fn links_that_stand_on_each_other_are_a_cycle_even_when_pending() {
    let s = shelf(vec![
        fr(1, &["FR-2"]),
        fr(2, &["FR-1"]),
        fr(3, &["FR-3"]),
        fr(4, &["FR-1"]),
    ]);
    assert_eq!(s.cycles().len(), 2);
    let ring = &s.cycles()[0];
    assert_eq!(
        ring.id(),
        CycleId {
            source: CycleSource::Links,
            min: docid(1)
        }
    );
    assert_eq!(ring.ids(), vec![docid(1), docid(2)]);
    // A document that stands on itself is a cycle of one.
    let own = &s.cycles()[1];
    assert_eq!(own.ids(), vec![docid(3)]);
    // A document that only stands on a cycle is not in it.
    assert!(!s.cycles().iter().any(|c| c.contains(docid(4))));
}

#[test]
fn a_chain_is_no_cycle_and_informs_and_replaces_links_are_not_followed() {
    let s = shelf(vec![
        fr(1, &[]),
        fr(2, &["FR-1"]),
        reading(
            "findings/fr-3.md",
            3,
            "FR-3",
            &[
                ("title", text("t")),
                ("related", list(&["FR-4"])),
                ("supersedes", list(&["FR-4"])),
            ],
        ),
        reading(
            "findings/fr-4.md",
            4,
            "FR-4",
            &[
                ("title", text("t")),
                ("related", list(&["FR-3"])),
                ("supersedes", list(&["FR-3"])),
            ],
        ),
    ]);
    assert!(s.cycles().is_empty());
}

#[test]
fn a_cycle_through_an_invalid_document_is_not_followed() {
    let s = shelf(vec![
        fr(1, &["FR-2"]),
        reading(
            "findings/fr-2.md",
            2,
            "FR-2",
            &[
                ("title", text("t")),
                ("base", list(&["FR-1"])),
                ("x", text("x")),
            ],
        ),
    ]);
    assert!(s.cycles().is_empty());
}

#[test]
fn an_unparsed_file_is_an_invalid_doc_that_says_why() {
    let s = shelf(vec![
        DocReading::Unparsed {
            origin: path("findings/x.md"),
            id: ReadId::Unreadable,
            why: RawProblem::Syntax { line: Some(3) },
        },
        DocReading::Unparsed {
            origin: path("findings/y.md"),
            id: ReadId::Read(docid(7)),
            why: RawProblem::DuplicateKey {
                key: "title".to_owned(),
            },
        },
    ]);
    assert_eq!(s.unreadable().len(), 1);
    assert_eq!(
        s.unreadable()[0].problems().first(),
        &DocProblem::Unreadable(RawProblem::Syntax { line: Some(3) })
    );
    assert_eq!(s.unreadable()[0].id(), None);
    // The one whose id line could be read is that identity, invalid.
    assert_eq!(
        problems_of(&s, 7),
        vec![DocProblem::Unreadable(RawProblem::DuplicateKey {
            key: "title".to_owned()
        })]
    );
}

#[test]
fn an_unparsed_file_with_an_id_blocks_a_valid_copy_of_it() {
    let s = shelf(vec![
        DocReading::Unparsed {
            origin: path("findings/x.md"),
            id: ReadId::Read(docid(1)),
            why: RawProblem::NotAMapping,
        },
        fr(1, &[]),
    ]);
    assert!(s.doc(docid(1)).is_none());
    // The unreadable file's own problem, and the shared id on each of the two.
    assert_eq!(problems_of(&s, 1).len(), 3);
}

#[test]
fn state_of_tells_valid_invalid_gone_and_unknown() {
    let s = shelf(vec![fr(1, &[]), reading("findings/b.md", 2, "FR-2", &[])]);
    assert!(matches!(s.state_of(docid(1)), DocState::Valid(d) if d.id() == docid(1)));
    assert!(matches!(s.state_of(docid(2)), DocState::Invalid(_)));
    assert_eq!(s.state_of(docid(3)), DocState::Gone);
}

#[test]
fn an_identity_is_unknown_not_gone_while_a_file_cannot_say_its_id() {
    let s = shelf(vec![
        fr(1, &[]),
        DocReading::Unparsed {
            origin: path("findings/x.md"),
            id: ReadId::Unreadable,
            why: RawProblem::Syntax { line: None },
        },
        DocReading::Unparsed {
            origin: path("findings/y.md"),
            id: ReadId::Unreadable,
            why: RawProblem::NotAMapping,
        },
    ]);
    match s.state_of(docid(9)) {
        DocState::Unknown(may) => {
            assert_eq!(
                may.any()
                    .iter()
                    .map(|p| p.as_str().to_owned())
                    .collect::<Vec<_>>(),
                vec!["findings/x.md", "findings/y.md"]
            );
            assert!(may.shown().is_empty());
        }
        other => panic!("{other:?}"),
    }
    // A document the shelf does have is still valid.
    assert!(matches!(s.state_of(docid(1)), DocState::Valid(_)));
}

#[test]
fn the_owned_form_of_a_state_carries_every_problem() {
    let s = shelf(vec![
        reading("findings/a.md", 1, "FR-1", &[("title", text("a"))]),
        reading("findings/b.md", 1, "FR-2", &[("title", text("b"))]),
    ]);
    let failure = s.state_of(docid(1)).valid_or_failure().err();
    assert_eq!(
        failure.as_ref().map(DocFailure::tag),
        Some(FailureTag::Invalid)
    );
    match failure {
        Some(DocFailure::Invalid(problems)) => assert_eq!(problems.len(), 2),
        other => panic!("{other:?}"),
    }
    let gone = s.state_of(docid(2)).valid_or_failure().err();
    assert_eq!(gone, Some(DocFailure::Gone));
    assert_eq!(gone.as_ref().map(DocFailure::tag), Some(FailureTag::Gone));
    let s = shelf(vec![fr(1, &[])]);
    assert!(s.state_of(docid(1)).valid_or_failure().is_ok());
}

#[test]
fn replacers_are_the_valid_documents_that_replace_a_document() {
    let replacing = |n: u128, target: &str, extra: &[(&str, RawValue)]| {
        let mut e = vec![("title", text("t")), ("supersedes", list(&[target]))];
        e.extend(extra.iter().cloned());
        reading(&format!("findings/fr-{n}.md"), n, &format!("FR-{n}"), &e)
    };
    let s = shelf(vec![
        fr(1, &[]),
        replacing(2, "FR-1", &[]),
        replacing(3, "FR-1", &[]),
        // Invalid: it names its replacement but cannot count as one.
        replacing(4, "FR-1", &[("surprise", text("x"))]),
        // Informs and Depends are not replacing.
        reading(
            "findings/fr-5.md",
            5,
            "FR-5",
            &[
                ("title", text("t")),
                ("related", list(&["FR-1"])),
                ("base", list(&["FR-1"])),
            ],
        ),
    ]);
    assert_eq!(s.replacers_of(docid(1)), vec![docid(2), docid(3)]);
    assert!(s.replacers_of(docid(2)).is_empty());
}

#[test]
fn links_of_gives_the_links_of_one_meaning() {
    let s = shelf(vec![
        fr(1, &[]),
        reading(
            "findings/fr-2.md",
            2,
            "FR-2",
            &[
                ("title", text("t")),
                ("base", list(&["FR-1"])),
                ("related", list(&["FR-1"])),
            ],
        ),
    ]);
    let doc = s.doc(docid(2)).unwrap();
    let names = |p| {
        doc.links_of(p)
            .into_iter()
            .map(|(n, _)| n.as_str().to_owned())
            .collect::<Vec<_>>()
    };
    assert_eq!(names(Propagation::Depends), vec!["base"]);
    assert_eq!(names(Propagation::Informs), vec!["related"]);
    assert_eq!(names(Propagation::Replaces), vec!["supersedes"]);
}

#[test]
fn ids_are_every_identity_some_file_carries() {
    let s = shelf(vec![
        fr(1, &[]),
        reading("findings/b.md", 2, "FR-2", &[]),
        DocReading::Unparsed {
            origin: path("findings/x.md"),
            id: ReadId::Unreadable,
            why: RawProblem::NotAMapping,
        },
    ]);
    assert_eq!(
        s.ids().into_iter().collect::<Vec<_>>(),
        vec![docid(1), docid(2)]
    );
}

#[test]
fn raw_values_know_their_kind() {
    assert_eq!(RawValue::Null.kind(), RawKind::Null);
    assert_eq!(text("a").kind(), RawKind::Text);
    assert_eq!(RawValue::Integer(1).kind(), RawKind::Integer);
    assert_eq!(RawValue::Bool(true).kind(), RawKind::Bool);
    assert_eq!(list(&[]).kind(), RawKind::List);
}

#[test]
fn tally_by_book_counts_valid_docs_and_invalid_files_per_book() {
    let s = shelf(vec![
        fr(1, &[]),
        fr(2, &[]),
        // Invalid: no title.
        reading("findings/bad.md", 3, "FR-3", &[]),
        reading("specs/sp-4.md", 4, "SP-4", &[("title", text("t"))]),
    ]);
    let tally = s.tally_by_book();
    assert_eq!(tally.len(), 3);
    assert_eq!(
        tally[&finding()],
        BookTally {
            valid: 2,
            invalid: 1
        }
    );
    assert_eq!(
        tally[&BookName::new("spec").unwrap()],
        BookTally {
            valid: 1,
            invalid: 0
        }
    );
}

#[test]
fn the_files_that_are_no_valid_document_are_the_invalid_the_unreadable_and_the_anonymous_in_that_order()
 {
    let s = shelf(vec![
        fr(1, &[]),
        // With no `id:`: anonymous.
        DocReading::Parsed {
            origin: path("findings/a.md"),
            entries: entries(&[("key", text("FR-2")), ("title", text("t"))]),
            body: String::new(),
        },
        // An identity that cannot be read: unreadable.
        DocReading::Unparsed {
            origin: path("findings/b.md"),
            id: ReadId::Unreadable,
            why: RawProblem::Syntax { line: None },
        },
        // No title: invalid, its identity read.
        reading("findings/c.md", 3, "FR-3", &[]),
        // Invalid, of another book.
        reading("specs/sp-4.md", 4, "SP-4", &[]),
    ]);
    let origins = |files: Vec<InvalidDoc>| -> Vec<String> {
        files
            .iter()
            .map(|file| file.origin().as_str().to_owned())
            .collect()
    };
    assert_eq!(
        origins(s.invalid_files()),
        [
            "findings/c.md",
            "specs/sp-4.md",
            "findings/b.md",
            "findings/a.md"
        ]
    );
    // Of one book, by the book each file is under, in the same order.
    assert_eq!(
        origins(s.invalid_files_in(&finding())),
        ["findings/c.md", "findings/b.md", "findings/a.md"]
    );
    assert_eq!(
        origins(s.invalid_files_in(&BookName::new("spec").unwrap())),
        ["specs/sp-4.md"]
    );
    // The tally counts the same three: one of each kind.
    assert_eq!(
        s.tally_by_book()[&finding()],
        BookTally {
            valid: 1,
            invalid: 3
        }
    );
}

#[test]
fn tally_by_book_lists_an_empty_declared_book_with_zeros() {
    let s = shelf(vec![fr(1, &[])]);
    assert_eq!(
        s.tally_by_book()[&BookName::new("design").unwrap()],
        BookTally::default()
    );
}

#[test]
fn tally_by_book_counts_an_unreadable_file_as_invalid_in_its_book() {
    let s = shelf(vec![
        fr(1, &[]),
        DocReading::Unparsed {
            origin: path("specs/x.md"),
            id: ReadId::Unreadable,
            why: RawProblem::Syntax { line: None },
        },
    ]);
    let tally = s.tally_by_book();
    assert_eq!(
        tally[&BookName::new("spec").unwrap()],
        BookTally {
            valid: 0,
            invalid: 1
        }
    );
    assert_eq!(
        tally[&finding()],
        BookTally {
            valid: 1,
            invalid: 0
        }
    );
}

#[test]
fn presence_of_a_valid_doc_has_its_key_file_and_hash() {
    let s = shelf(vec![fr(1, &[])]);
    match s.state_of(docid(1)).presence() {
        DocPresence::Valid { key, origin, hash } => {
            assert_eq!(&key, s.doc(docid(1)).unwrap().key());
            assert_eq!(origin, path("findings/fr-1.md"));
            assert_eq!(hash, hash_of(&s, 1));
        }
        other => panic!("expected valid, got {other:?}"),
    }
}

#[test]
fn presence_of_two_files_with_one_id_lists_both_files_and_the_problems_of_both() {
    let s = shelf(vec![
        reading("findings/a.md", 1, "FR-1", &[("title", text("t"))]),
        reading("findings/b.md", 1, "FR-1", &[("title", text("t"))]),
    ]);
    match s.state_of(docid(1)).presence() {
        DocPresence::Invalid { files, problems } => {
            let files: Vec<&str> = files.iter().map(RelPath::as_str).collect();
            assert_eq!(files, vec!["findings/a.md", "findings/b.md"]);
            let duplicates = problems
                .iter()
                .filter(|p| matches!(p, DocProblem::DuplicateId { .. }))
                .count();
            assert_eq!(duplicates, 2);
        }
        other => panic!("expected invalid, got {other:?}"),
    }
}

#[test]
fn presence_of_an_identity_no_file_carries_is_gone() {
    let s = shelf(vec![fr(1, &[])]);
    assert_eq!(s.state_of(docid(9)).presence(), DocPresence::Gone);
}

#[test]
fn presence_is_unknown_while_a_file_cannot_say_its_id() {
    let s = shelf(vec![DocReading::Unparsed {
        origin: path("findings/x.md"),
        id: ReadId::Unreadable,
        why: RawProblem::Syntax { line: None },
    }]);
    match s.state_of(docid(9)).presence() {
        DocPresence::Unknown(may) => {
            assert_eq!(may.any(), [path("findings/x.md")]);
            assert!(may.shown().is_empty());
        }
        other => panic!("expected unknown, got {other:?}"),
    }
}

// A file left with both sides of a merge conflict whose sides show different
// identities (`ReadId::OneOf`): X shows A and B and is under the book of findings;
// C is carried by no file; U is a file whose identity cannot be read; Y carries A.

const A: u128 = 10;
const B: u128 = 11;
const C: u128 = 12;
const X: &str = "findings/x.md";
const U: &str = "findings/u.md";

/// The files that may hold `n`, `(shown, any)`, when it is unknown.
fn may_hold(s: &Shelf, n: u128) -> Option<(Vec<RelPath>, Vec<RelPath>)> {
    match s.state_of(docid(n)) {
        DocState::Unknown(may) => Some((may.shown().to_vec(), may.any().to_vec())),
        _ => None,
    }
}

fn paths(texts: &[&str]) -> Vec<RelPath> {
    texts.iter().map(|t| path(t)).collect()
}

/// The problems of the invalid file `file`.
fn problems_at(s: &Shelf, file: &str) -> Vec<DocProblem> {
    s.invalid_files()
        .into_iter()
        .find(|doc| doc.origin() == &path(file))
        .unwrap_or_else(|| panic!("{file} is not invalid"))
        .problems()
        .iter()
        .cloned()
        .collect()
}

/// The problems of `file` of the id `n` that may be carried twice, by the other files.
fn maybe_twice(s: &Shelf, file: &str) -> Vec<(DocId, Vec<RelPath>)> {
    problems_at(s, file)
        .into_iter()
        .filter_map(|p| match p {
            DocProblem::MaybeDuplicateId { id, others } => Some((id, others.into_vec())),
            _ => None,
        })
        .collect()
}

#[test]
fn the_identities_of_the_sides_are_one_identity_when_they_are_one_and_one_of_them_when_two() {
    assert_eq!(ReadId::of_sides([]), ReadId::Unreadable);
    assert_eq!(ReadId::of_sides([docid(A)]), ReadId::Read(docid(A)));
    assert_eq!(
        ReadId::of_sides([docid(A), docid(A)]),
        ReadId::Read(docid(A))
    );
    let ReadId::OneOf(sides) = ReadId::of_sides([docid(B), docid(A), docid(B)]) else {
        panic!("one of two");
    };
    assert_eq!(sides.ids(), &BTreeSet::from([docid(A), docid(B)]));
    assert!(sides.contains(docid(A)) && sides.contains(docid(B)));
    assert!(!sides.contains(docid(C)));
    assert_eq!(
        ReadId::of_sides([docid(A), docid(B)]),
        ReadId::of_sides([docid(B), docid(A)])
    );
}

#[test]
fn a_file_whose_sides_show_two_identities_is_sorted_apart_with_its_sides_and_no_id() {
    let s = shelf(vec![sided(X, &[A, B])]);
    assert_eq!(s.sided().len(), 1);
    assert!(s.unreadable().is_empty());
    assert!(s.invalid().is_empty());
    assert!(s.anonymous().is_empty());
    let file = &s.sided()[0];
    assert_eq!(file.origin(), &path(X));
    assert_eq!(file.id(), None);
    assert_eq!(
        file.sides().map(Sides::ids),
        Some(&BTreeSet::from([docid(A), docid(B)]))
    );
    assert!(matches!(
        file.problems().first(),
        DocProblem::Unreadable(RawProblem::ConflictMarkers)
    ));
    // Any other file shows no sides.
    let s = shelf(vec![unreadable_at(U), invalid_titleless(A)]);
    assert!(s.invalid_files().iter().all(|file| file.sides().is_none()));
}

/// The finding `FR-<n>` at `findings/fr-<n>.md`, invalid: it has no title.
fn invalid_titleless(n: u128) -> DocReading {
    reading(&format!("findings/fr-{n}.md"), n, &format!("FR-{n}"), &[])
}

#[test]
fn a_document_a_side_shows_is_unknown_in_that_file_and_one_no_side_shows_is_gone() {
    // X only.
    let s = shelf(vec![fr(1, &[]), sided(X, &[A, B])]);
    for n in [A, B] {
        assert_eq!(may_hold(&s, n), Some((paths(&[X]), Vec::new())), "{n}");
        assert_eq!(s.state_of(docid(n)).may_be_in(), BTreeSet::from([&path(X)]));
        assert_eq!(
            s.state_of(docid(n))
                .valid_or_failure()
                .err()
                .map(|f| f.tag()),
            Some(FailureTag::Unknown)
        );
        assert_eq!(s.now_named(docid(n)), NowNamed::Unknown);
    }
    assert_eq!(s.state_of(docid(C)), DocState::Gone);
    assert_eq!(s.state_of(docid(C)).presence(), DocPresence::Gone);
    assert_eq!(s.now_named(docid(C)), NowNamed::Gone);
    assert!(s.state_of(docid(C)).may_be_in().is_empty());
    // X and U: U may hold any, so C is unknown in U only.
    let s = shelf(vec![fr(1, &[]), sided(X, &[A, B]), unreadable_at(U)]);
    assert_eq!(may_hold(&s, A), Some((paths(&[X]), paths(&[U]))));
    assert_eq!(may_hold(&s, C), Some((Vec::new(), paths(&[U]))));
    assert_eq!(
        s.state_of(docid(A)).may_be_in(),
        BTreeSet::from([&path(X), &path(U)])
    );
    let DocPresence::Unknown(may) = s.state_of(docid(A)).presence() else {
        panic!("unknown");
    };
    assert_eq!(may.files(), vec![&path(X), &path(U)]);
}

#[test]
fn a_document_a_side_shows_and_another_file_carries_is_invalid_in_that_file_only() {
    let s = shelf(vec![sided(X, &[A, B]), fr(A, &[])]);
    let DocState::Invalid(files) = s.state_of(docid(A)) else {
        panic!("invalid");
    };
    assert_eq!(files.len(), 1);
    assert_eq!(files.first().origin(), &path("findings/fr-10.md"));
    assert_eq!(
        s.state_of(docid(A)).may_be_in(),
        BTreeSet::from([&path("findings/fr-10.md")])
    );
    assert_eq!(may_hold(&s, B), Some((paths(&[X]), Vec::new())));
    let s = shelf(vec![sided(X, &[A, B]), fr(A, &[]), unreadable_at(U)]);
    assert!(matches!(s.state_of(docid(A)), DocState::Invalid(_)));
    assert_eq!(may_hold(&s, B), Some((paths(&[X]), paths(&[U]))));
}

#[test]
fn the_identities_the_shelf_knows_are_those_carried_and_those_a_side_shows() {
    let s = shelf(vec![fr(1, &[]), invalid_titleless(2), sided(X, &[A, B])]);
    assert_eq!(
        s.known_ids(),
        BTreeMap::from([
            (docid(1), Known::Carried),
            (docid(2), Known::Carried),
            (docid(A), Known::Shown),
            (docid(B), Known::Shown),
        ])
    );
    assert_eq!(s.ids(), BTreeSet::from([docid(1), docid(2)]));
    // An identity a side shows and a file carries is carried.
    let s = shelf(vec![sided(X, &[A, B]), fr(A, &[])]);
    assert_eq!(
        s.known_ids(),
        BTreeMap::from([(docid(A), Known::Carried), (docid(B), Known::Shown)])
    );
    // A file that may hold any adds none.
    let s = shelf(vec![unreadable_at(U)]);
    assert!(s.known_ids().is_empty());
}

#[test]
fn the_book_of_a_document_a_side_shows_is_the_one_book_of_every_file_that_may_hold_it() {
    let spec = BookName::new("spec").unwrap();
    // X under the book of findings.
    let s = shelf(vec![sided(X, &[A, B])]);
    assert_eq!(s.book_of(docid(A)), Some(&finding()));
    assert_eq!(s.book_of(docid(B)), Some(&finding()));
    assert_eq!(s.book_of(docid(C)), None);
    // Two files whose sides show A, under two books.
    let s = shelf(vec![sided(X, &[A, B]), sided("specs/x.md", &[A, C])]);
    assert_eq!(s.book_of(docid(A)), None);
    assert_eq!(s.book_of(docid(C)), Some(&spec));
    // No file carries A, so neither file is told it may carry it twice.
    assert!(maybe_twice(&s, X).is_empty());
    assert!(maybe_twice(&s, "specs/x.md").is_empty());
    // U under the same book, and under another.
    let s = shelf(vec![sided(X, &[A, B]), unreadable_at(U)]);
    assert_eq!(s.book_of(docid(A)), Some(&finding()));
    let s = shelf(vec![sided(X, &[A, B]), unreadable_at("specs/u.md")]);
    assert_eq!(s.book_of(docid(A)), None);
    // U alone: no side shows C, so it has no book.
    let s = shelf(vec![unreadable_at(U)]);
    assert_eq!(s.book_of(docid(C)), None);
}

#[test]
fn a_file_that_carries_an_id_a_side_shows_and_the_file_of_that_side_are_told_so_both() {
    let y = "findings/fr-10.md";
    let s = shelf(vec![sided(X, &[A, B]), fr(A, &[])]);
    assert_eq!(maybe_twice(&s, y), vec![(docid(A), paths(&[X]))]);
    assert_eq!(maybe_twice(&s, X), vec![(docid(A), paths(&[y]))]);
    assert!(matches!(
        problems_at(&s, X).first(),
        Some(DocProblem::Unreadable(RawProblem::ConflictMarkers))
    ));
    assert_eq!(
        DocProblem::MaybeDuplicateId {
            id: docid(A),
            others: NonEmptyVec::new(path(X), Vec::new()),
        }
        .to_string(),
        format!(
            "the id {} may be carried twice, by this file and findings/x.md: one of them holds both sides of a merge conflict",
            docid(A)
        )
    );
    // One problem for each id: Y carries A, Y2 carries B.
    let y2 = "findings/fr-11.md";
    let s = shelf(vec![sided(X, &[A, B]), fr(A, &[]), fr(B, &[])]);
    assert_eq!(
        maybe_twice(&s, X),
        vec![(docid(A), paths(&[y])), (docid(B), paths(&[y2]))]
    );
    assert_eq!(maybe_twice(&s, y), vec![(docid(A), paths(&[X]))]);
    assert_eq!(maybe_twice(&s, y2), vec![(docid(B), paths(&[X]))]);
    // A file whose sides show B and C, not A, is told nothing of A.
    let z = "findings/z.md";
    let s = shelf(vec![sided(z, &[B, C]), fr(A, &[])]);
    assert!(maybe_twice(&s, z).is_empty());
    assert!(matches!(s.state_of(docid(A)), DocState::Valid(_)));
}

#[test]
fn the_invalid_files_are_those_read_then_those_left_in_a_conflict_then_the_unreadable_then_the_anonymous()
 {
    let anonymous = DocReading::Parsed {
        origin: path("findings/anonymous.md"),
        entries: entries(&[("key", text("FR-50")), ("title", text("t"))]),
        body: "body\n".to_owned(),
    };
    let s = shelf(vec![
        anonymous,
        unreadable_at(U),
        sided(X, &[A, B]),
        invalid_titleless(2),
    ]);
    let order: Vec<RelPath> = s
        .invalid_files()
        .iter()
        .map(|file| file.origin().clone())
        .collect();
    assert_eq!(
        order,
        paths(&["findings/fr-2.md", X, U, "findings/anonymous.md"])
    );
}
