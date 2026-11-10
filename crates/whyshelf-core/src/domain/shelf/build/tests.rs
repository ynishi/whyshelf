//! Tests of the shelf.

use super::*;
use crate::domain::shelf::fixtures_tests::{
    decls, docid, entries, fr, fr_titled, list, path, reading, schema, shelf, text,
};
use crate::domain::shelf::schema::Cardinality;

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
        .cloned()
        .unwrap()
}

fn hash_of(shelf: &Shelf, n: u128) -> DocHash {
    *shelf.doc(docid(n)).unwrap().hash()
}

fn form(key: &str) -> KeyForm {
    schema()
        .book_named("finding")
        .unwrap()
        .key()
        .parse(key)
        .unwrap()
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
fn a_file_under_no_book_and_with_no_id_is_unreadable() {
    let s = shelf(vec![DocReading::Parsed {
        origin: path("notes/n.md"),
        entries: entries(&[]),
        body: String::new(),
    }]);
    assert_eq!(s.unreadable().len(), 1);
    assert_eq!(
        s.unreadable()[0].problems().first(),
        &DocProblem::OutsideBooks
    );
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
    let kinds: Vec<_> = s
        .unreadable()
        .iter()
        .map(|d| d.problems().first().clone())
        .collect();
    assert_eq!(kinds.len(), 3);
    assert!(kinds.contains(&DocProblem::IdMissing));
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
    let schema = Schema::validate(d).unwrap();
    let s = Shelf::build(
        &schema,
        vec![reading(
            "findings/c.md",
            1,
            "CAFE-1234",
            &[("title", text("t"))],
        )],
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
            name: "surprise".to_owned()
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
        expected: FieldType::Integer,
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
    let schema = Schema::validate(d).unwrap();
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
    assert_eq!(
        links(&s, 2, "base"),
        vec![LinkState::Resolved { target: docid(1) }]
    );
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
        [LinkState::Unresolved(u)] => assert_eq!(u.written(), "nonsense"),
        other => panic!("{other:?}"),
    }
}

#[test]
fn a_link_to_no_key_is_missing() {
    let s = shelf(vec![fr(1, &["FR-99"])]);
    assert!(matches!(
        links(&s, 1, "base").as_slice(),
        [LinkState::Unresolved(u)] if *u.why() == UnresolvedWhy::Missing
    ));
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
            UnresolvedWhy::Ambiguous(ids) => {
                assert_eq!(ids.clone().into_vec(), vec![docid(5), docid(6)]);
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
        &Schema::validate(d).unwrap(),
        vec![reading(
            "findings/a.md",
            1,
            "FR-1",
            &[("title", text("title"))],
        )],
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
    assert!(matches!(ring.members(), CycleMembers::Ring(m) if m.len() == 2));
    let own = &s.cycles()[1];
    assert!(matches!(own.members(), CycleMembers::SelfLoop(id) if *id == docid(3)));
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
        DocState::Unknown { unreadable } => {
            assert_eq!(
                unreadable
                    .iter()
                    .map(|p| p.as_str().to_owned())
                    .collect::<Vec<_>>(),
                vec!["findings/x.md", "findings/y.md"]
            );
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
    match s.state_of(docid(1)).to_kind() {
        Some(DocStateKind::Invalid(problems)) => assert_eq!(problems.len(), 2),
        other => panic!("{other:?}"),
    }
    assert_eq!(s.state_of(docid(2)).to_kind(), Some(DocStateKind::Gone));
    let s = shelf(vec![fr(1, &[])]);
    assert_eq!(s.state_of(docid(1)).to_kind(), None);
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
        s.links_of(doc, p)
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

fn edges(pairs: &[(u128, &[u128])]) -> BTreeMap<DocId, BTreeSet<DocId>> {
    pairs
        .iter()
        .map(|(from, to)| (docid(*from), to.iter().map(|t| docid(*t)).collect()))
        .collect()
}

#[test]
fn components_come_upstream_first() {
    // 3 stands on 2, 2 stands on 1; 4 on 3 and 1.
    let graph = edges(&[(3, &[2]), (2, &[1]), (4, &[3, 1])]);
    let order: Vec<DocId> = components(&graph).into_iter().flatten().collect();
    let position = |n: u128| order.iter().position(|id| *id == docid(n)).unwrap();
    assert_eq!(order.len(), 4);
    assert!(position(1) < position(2));
    assert!(position(2) < position(3));
    assert!(position(3) < position(4));
}

#[test]
fn a_cycle_is_one_component_and_its_upstream_comes_before_it() {
    let graph = edges(&[(1, &[2]), (2, &[1, 3]), (3, &[])]);
    let all = components(&graph);
    assert_eq!(all, vec![vec![docid(3)], vec![docid(1), docid(2)]]);
}

#[test]
fn cycles_of_finds_rings_and_self_loops_and_names_them_by_source() {
    let graph = edges(&[(1, &[2]), (2, &[3]), (3, &[1]), (4, &[4]), (5, &[1])]);
    let cycles = cycles_of(CycleSource::Frames, &graph);
    assert_eq!(cycles.len(), 2);
    assert_eq!(
        cycles[0].id(),
        CycleId {
            source: CycleSource::Frames,
            min: docid(1)
        }
    );
    assert_eq!(cycles[0].ids(), vec![docid(1), docid(2), docid(3)]);
    assert_eq!(cycles[1].ids(), vec![docid(4)]);
    // Two cycles of different graphs on one document are two ids.
    let links = cycles_of(CycleSource::Links, &graph);
    assert_ne!(links[0].id(), cycles[0].id());
}

#[test]
fn a_very_long_chain_does_not_overflow_the_stack() {
    let n = 20_000u128;
    let graph: BTreeMap<DocId, BTreeSet<DocId>> = (2..=n)
        .map(|i| (docid(i), BTreeSet::from([docid(i - 1)])))
        .collect();
    assert!(cycles_of(CycleSource::Links, &graph).is_empty());
    assert_eq!(components(&graph).len(), usize::try_from(n).unwrap());
}

#[test]
fn raw_values_know_their_kind() {
    assert_eq!(text("a").kind(), RawKind::Text);
    assert_eq!(RawValue::Integer(1).kind(), RawKind::Integer);
    assert_eq!(RawValue::Bool(true).kind(), RawKind::Bool);
    assert_eq!(list(&[]).kind(), RawKind::List);
}
