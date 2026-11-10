//! Tests of the schema.

use super::*;
use crate::domain::shelf::fixtures_tests::{decls, eid, path};

fn problems_of(decls: Vec<BookSchemaDecl>) -> Vec<SchemaProblem> {
    Schema::validate(decls).unwrap_err().into_vec()
}

fn book(name: &str, dir: &str, prefix: &str) -> BookSchemaDecl {
    BookSchemaDecl {
        name: name.to_owned(),
        dir: dir.to_owned(),
        key: KeyPatternDecl::Prefixed {
            prefix: prefix.to_owned(),
        },
        fields: Vec::new(),
        links: Vec::new(),
    }
}

fn text_field(name: &str) -> FieldEntryDecl {
    FieldEntryDecl {
        name: name.to_owned(),
        ty: FieldTypeDecl::Text,
        presence: FieldPresence::Optional,
    }
}

fn link_to(name: &str, target: &str, propagation: Propagation) -> LinkEntryDecl {
    LinkEntryDecl {
        name: name.to_owned(),
        target: LinkTargetDecl::Book(target.to_owned()),
        cardinality: Cardinality::Many,
        propagation,
        compare: None,
    }
}

#[test]
fn a_sound_declaration_makes_a_schema() {
    let schema = Schema::validate(decls()).unwrap();
    assert_eq!(schema.books().len(), 3);
    let finding = schema.book("finding").unwrap();
    assert_eq!(finding.name().as_str(), "finding");
    assert_eq!(finding.dir().to_string(), "findings");
    assert_eq!(finding.fields().len(), 6);
    assert_eq!(finding.links().len(), 3);
}

#[test]
fn an_empty_declaration_is_a_schema_with_no_book() {
    assert!(Schema::validate(Vec::new()).unwrap().books().is_empty());
}

#[test]
fn names_follow_their_grammar() {
    assert!(BookName::new("finding-2_x").is_ok());
    assert_eq!(BookName::new(""), Err(NameProblem::Empty));
    assert_eq!(
        BookName::new("Finding"),
        Err(NameProblem::BadChar { ch: 'F' })
    );
    assert_eq!(
        BookName::new(&"a".repeat(65)),
        Err(NameProblem::TooLong { len: 65, max: 64 })
    );
    assert!(BookName::new(&"a".repeat(64)).is_ok());
}

#[test]
fn id_and_key_are_reserved_for_fields_and_links_only() {
    assert_eq!(
        EntryName::new("id"),
        Err(NameProblem::Reserved {
            name: "id".to_owned()
        })
    );
    assert!(EntryName::new("key").is_err());
    // A book may be called `key`: it is not an entry of a front matter.
    assert!(BookName::new("key").is_ok());
    assert!(FieldWord::new("id").is_ok());
}

#[test]
fn bad_names_are_reported_where_they_were_written() {
    let mut d = vec![book("Bad Book", "a", "AA")];
    d.push(BookSchemaDecl {
        fields: vec![text_field("Title"), text_field("id")],
        links: vec![
            link_to("Up", "ok", Propagation::Informs),
            link_to("down", "No Such", Propagation::Informs),
            link_to("key", "ok", Propagation::Informs),
        ],
        ..book("ok", "b", "BB")
    });
    d.push(BookSchemaDecl {
        fields: vec![FieldEntryDecl {
            name: "level".to_owned(),
            ty: FieldTypeDecl::Enum(vec!["Low".to_owned(), "high".to_owned()]),
            presence: FieldPresence::Optional,
        }],
        ..book("third", "c", "CC")
    });
    let problems = problems_of(d);
    let place = |p: &SchemaProblem| match p {
        SchemaProblem::BadName { place, .. } => Some(place.clone()),
        _ => None,
    };
    let places: Vec<NamePlace> = problems.iter().filter_map(place).collect();
    assert!(places.contains(&NamePlace::Book));
    assert!(
        places
            .iter()
            .filter(|p| matches!(p, NamePlace::Field { .. }))
            .count()
            == 2
    );
    assert!(
        places
            .iter()
            .filter(|p| matches!(p, NamePlace::Link { .. }))
            .count()
            == 2
    );
    assert!(
        places
            .iter()
            .any(|p| matches!(p, NamePlace::LinkTarget { .. }))
    );
    assert!(
        places
            .iter()
            .any(|p| matches!(p, NamePlace::EnumWord { .. }))
    );
    assert!(problems.iter().any(|p| matches!(
        p,
        SchemaProblem::BadName {
            why: NameProblem::Reserved { .. },
            ..
        }
    )));
}

#[test]
fn a_directory_must_be_a_relative_path() {
    for dir in ["/abs", "a//b", "a/../b", "", "a/"] {
        let problems = problems_of(vec![book("one", dir, "AA")]);
        assert!(
            matches!(problems.as_slice(), [SchemaProblem::BadDir { .. }]),
            "{dir}: {problems:?}"
        );
    }
}

#[test]
fn a_key_prefix_must_follow_its_grammar() {
    let too_long = "A".repeat(17);
    for prefix in ["fr", "1A", "", "A-B", too_long.as_str()] {
        let problems = problems_of(vec![book("one", "a", prefix)]);
        assert!(
            matches!(problems.as_slice(), [SchemaProblem::BadKeyPrefix { .. }]),
            "{prefix}: {problems:?}"
        );
    }
    assert!(Schema::validate(vec![book("one", "a", &"A".repeat(16))]).is_ok());
}

#[test]
fn an_enum_must_list_a_word() {
    let mut d = book("one", "a", "AA");
    d.fields.push(FieldEntryDecl {
        name: "level".to_owned(),
        ty: FieldTypeDecl::Enum(Vec::new()),
        presence: FieldPresence::Required,
    });
    let problems = problems_of(vec![d]);
    assert!(matches!(
        problems.as_slice(),
        [SchemaProblem::EmptyEnum { .. }]
    ));
}

#[test]
fn a_book_declared_twice_is_reported_once() {
    let problems = problems_of(vec![
        book("one", "a", "AA"),
        book("one", "b", "BB"),
        book("one", "c", "CC"),
    ]);
    let twice: Vec<_> = problems
        .iter()
        .filter(|p| matches!(p, SchemaProblem::DuplicateBook { .. }))
        .collect();
    assert_eq!(twice.len(), 1);
}

#[test]
fn a_field_or_a_link_written_twice_is_reported() {
    let mut d = book("one", "a", "AA");
    d.fields = vec![text_field("title"), text_field("title")];
    d.links = vec![
        link_to("up", "one", Propagation::Informs),
        link_to("up", "one", Propagation::Informs),
    ];
    let problems = problems_of(vec![d]);
    assert!(
        problems
            .iter()
            .any(|p| matches!(p, SchemaProblem::DuplicateField { .. }))
    );
    assert!(
        problems
            .iter()
            .any(|p| matches!(p, SchemaProblem::DuplicateLink { .. }))
    );
}

#[test]
fn a_name_cannot_be_a_field_and_a_link() {
    let mut d = book("one", "a", "AA");
    d.fields = vec![text_field("up")];
    d.links = vec![link_to("up", "one", Propagation::Informs)];
    let problems = problems_of(vec![d]);
    assert!(matches!(
        problems.as_slice(),
        [SchemaProblem::NameCollision { .. }]
    ));
}

#[test]
fn a_link_into_an_undeclared_book_is_reported() {
    let mut d = book("one", "a", "AA");
    d.links = vec![link_to("up", "nowhere", Propagation::Depends)];
    let problems = problems_of(vec![d]);
    assert!(matches!(
        problems.as_slice(),
        [SchemaProblem::UnknownLinkTarget { target, .. }] if target.as_str() == "nowhere"
    ));
}

#[test]
fn books_cannot_share_a_key_prefix() {
    let problems = problems_of(vec![
        book("one", "a", "SP"),
        book("two", "b", "SP"),
        book("three", "c", "SP"),
        book("four", "d", "OK"),
    ]);
    match problems.as_slice() {
        [SchemaProblem::PrefixClash { prefix, books }] => {
            assert_eq!(prefix.as_str(), "SP");
            assert_eq!(books.len(), 3);
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn slug_books_do_not_clash_on_a_prefix() {
    let mut a = book("one", "a", "AA");
    a.key = KeyPatternDecl::Slug;
    let mut b = book("two", "b", "BB");
    b.key = KeyPatternDecl::Slug;
    assert!(Schema::validate(vec![a, b]).is_ok());
}

#[test]
fn directories_cannot_be_the_same_or_nested() {
    for (a, b) in [("docs", "docs"), ("docs", "docs/sub"), ("docs/sub", "docs")] {
        let problems = problems_of(vec![book("one", a, "AA"), book("two", b, "BB")]);
        assert!(
            matches!(problems.as_slice(), [SchemaProblem::DirOverlap { .. }]),
            "{a} {b}: {problems:?}"
        );
    }
    // A shared name prefix is not nesting.
    assert!(Schema::validate(vec![book("one", "docs", "AA"), book("two", "docs-2", "BB")]).is_ok());
}

#[test]
fn a_replaces_link_must_stay_in_its_book() {
    let mut a = book("one", "a", "AA");
    a.links = vec![link_to("next", "two", Propagation::Replaces)];
    let problems = problems_of(vec![a, book("two", "b", "BB")]);
    assert!(matches!(
        problems.as_slice(),
        [SchemaProblem::ReplacesForeign { target, .. }] if target.as_str() == "two"
    ));

    let mut own = book("one", "a", "AA");
    own.links = vec![link_to("next", "one", Propagation::Replaces)];
    assert!(Schema::validate(vec![own]).is_ok());
}

fn artifact_link(name: &str, propagation: Propagation) -> LinkEntryDecl {
    LinkEntryDecl {
        name: name.to_owned(),
        target: LinkTargetDecl::Artifact,
        cardinality: Cardinality::Many,
        propagation,
        compare: None,
    }
}

#[test]
fn a_link_to_artifacts_depends_or_informs_and_is_kept_apart_from_links_into_books() {
    let mut a = book("one", "a", "AA");
    a.links = vec![
        artifact_link("tests", Propagation::Depends),
        artifact_link("code", Propagation::Informs),
        link_to("up", "one", Propagation::Depends),
    ];
    let schema = Schema::validate(vec![a]).unwrap();
    let one = schema.book("one").unwrap();
    assert_eq!(one.links().len(), 1);
    assert_eq!(one.artifact_links().len(), 2);
    assert_eq!(
        one.artifact_links().get("tests").unwrap().propagation,
        ArtifactPropagation::Depends
    );
    assert_eq!(
        one.artifact_links().get("code").unwrap().propagation,
        ArtifactPropagation::Informs
    );
}

#[test]
fn a_link_to_artifacts_that_replaces_is_refused() {
    let mut a = book("one", "a", "AA");
    a.links = vec![artifact_link("old", Propagation::Replaces)];
    let problems = problems_of(vec![a]);
    assert!(matches!(
        problems.as_slice(),
        [SchemaProblem::ArtifactReplaces { link, .. }] if link.as_str() == "old"
    ));
}

#[test]
fn a_name_is_one_link_whether_it_points_at_a_book_or_at_artifacts() {
    let mut a = book("one", "a", "AA");
    a.links = vec![
        link_to("up", "one", Propagation::Depends),
        artifact_link("up", Propagation::Depends),
    ];
    assert!(matches!(
        problems_of(vec![a]).as_slice(),
        [SchemaProblem::DuplicateLink { .. }]
    ));
}

#[test]
fn no_book_is_named_with_the_word_for_artifacts() {
    assert!(matches!(
        problems_of(vec![book(ARTIFACT_TARGET, "a", "AA")]).as_slice(),
        [SchemaProblem::ReservedBookName]
    ));
}

#[test]
fn every_problem_is_returned_at_once() {
    let mut a = book("one", "/abs", "aa");
    a.fields = vec![text_field("id"), text_field("title"), text_field("title")];
    a.links = vec![link_to("title", "nowhere", Propagation::Replaces)];
    let b = book("one", "x", "BB");
    let problems = problems_of(vec![a, b]);
    // BadDir, BadKeyPrefix, reserved, DuplicateField, NameCollision, DuplicateBook,
    // UnknownLinkTarget, ReplacesForeign.
    assert!(problems.len() >= 8, "{problems:?}");
    let has = |f: fn(&SchemaProblem) -> bool| problems.iter().any(f);
    assert!(has(|p| matches!(p, SchemaProblem::BadDir { .. })));
    assert!(has(|p| matches!(p, SchemaProblem::BadKeyPrefix { .. })));
    assert!(has(|p| matches!(p, SchemaProblem::DuplicateField { .. })));
    assert!(has(|p| matches!(p, SchemaProblem::NameCollision { .. })));
    assert!(has(|p| matches!(p, SchemaProblem::DuplicateBook { .. })));
    assert!(has(|p| matches!(
        p,
        SchemaProblem::UnknownLinkTarget { .. }
    )));
    assert!(has(|p| matches!(p, SchemaProblem::ReplacesForeign { .. })));
}

#[test]
fn membership_is_the_book_whose_directory_a_file_is_under() {
    let schema = Schema::validate(decls()).unwrap();
    let of = |p: &str| schema.membership(&path(p));
    let book = |name: &str| Membership::In(schema.book(name).unwrap());
    assert_eq!(of("findings/fr-1.md"), book("finding"));
    assert_eq!(of("findings/deep/er/fr-1.md"), book("finding"));
    assert_eq!(of("designs/x.md"), book("design"));
    assert_eq!(of("readme.md"), Membership::Outside);
    assert_eq!(of("findings"), Membership::Outside);
    assert_eq!(of("findings-old/fr-1.md"), Membership::Outside);
    assert_eq!(of("docs/findings/fr-1.md"), Membership::Outside);
}

#[test]
fn a_prefixed_key_has_one_spelling() {
    let pattern = KeyPattern::Prefixed(KeyPrefix::new("FR").unwrap());
    assert_eq!(pattern.parse("FR-12").unwrap().to_string(), "FR-12");
    assert_eq!(pattern.parse("FR-012"), Err(KeyProblem::NumberLeadingZero));
    assert_eq!(pattern.parse("FR-0"), Err(KeyProblem::NumberLeadingZero));
    assert_eq!(
        pattern.parse("FR-1234567890123456789").unwrap().to_string(),
        "FR-1234567890123456789"
    );
    assert_eq!(
        pattern.parse("FR-12345678901234567890"),
        Err(KeyProblem::NumberTooLong { len: 20, max: 19 })
    );
}

#[test]
fn a_prefixed_key_may_end_in_a_name() {
    let pattern = KeyPattern::Prefixed(KeyPrefix::new("FR").unwrap());
    let form = pattern.parse("FR-LATENCY-BUDGET_2").unwrap();
    assert_eq!(form.to_string(), "FR-LATENCY-BUDGET_2");
    assert!(matches!(
        form,
        KeyForm::Prefixed {
            tail: KeyTail::Name(_),
            ..
        }
    ));
}

#[test]
fn a_prefixed_key_is_refused_when_it_is_malformed() {
    let pattern = KeyPattern::Prefixed(KeyPrefix::new("FR").unwrap());
    assert_eq!(pattern.parse(""), Err(KeyProblem::Empty));
    assert_eq!(pattern.parse("FR12"), Err(KeyProblem::NoSeparator));
    assert!(matches!(
        pattern.parse("fr-1"),
        Err(KeyProblem::BadPrefix { .. })
    ));
    assert!(matches!(
        pattern.parse("SP-1"),
        Err(KeyProblem::WrongPrefix { .. })
    ));
    for tail in ["", "1A", "a", "-1", "A b"] {
        assert!(
            matches!(
                pattern.parse(&format!("FR-{tail}")),
                Err(KeyProblem::BadTail { .. })
            ),
            "{tail}"
        );
    }
    assert!(matches!(
        pattern.parse(&format!("FR-A{}", "B".repeat(64))),
        Err(KeyProblem::BadTail { .. })
    ));
}

#[test]
fn a_prefixed_key_is_never_taken_for_a_short_id() {
    // `CAFE-1234` is hexadecimal around its hyphen, but the hyphen keeps it out of
    // the form of a short id, so a lookup never reads it as one.
    assert!(!ShortId::looks_like("CAFE-1234"));
    assert!(!ShortId::looks_like("DEAD-BEEF"));
    let pattern = KeyPattern::Prefixed(KeyPrefix::new("CAFE").unwrap());
    assert!(pattern.parse("CAFE-1234").is_ok());
}

#[test]
fn a_slug_follows_the_rules_of_an_alias_and_one_more() {
    let slug = KeyPattern::Slug;
    assert!(slug.parse("cache/timeout").is_ok());
    assert!(slug.parse("a.b-c_d/e").is_ok());
    // As an alias, these are fine.
    assert!(slug.parse("cafe-babe").is_ok());
    assert!(slug.parse("v20240101").is_ok());
    assert_eq!(slug.parse(""), Err(KeyProblem::Empty));
    assert_eq!(slug.parse("a//b"), Err(KeyProblem::SlugEmptySegment));
    assert_eq!(slug.parse("/a"), Err(KeyProblem::SlugEmptySegment));
    assert_eq!(slug.parse("a/"), Err(KeyProblem::SlugEmptySegment));
    assert_eq!(slug.parse("a b"), Err(KeyProblem::SlugBadChar { ch: ' ' }));
    assert_eq!(slug.parse("Abc"), Err(KeyProblem::SlugBadChar { ch: 'A' }));
    assert_eq!(slug.parse("a:b"), Err(KeyProblem::SlugBadChar { ch: ':' }));
    assert_eq!(
        slug.parse(&"a".repeat(129)),
        Err(KeyProblem::SlugTooLong { len: 129, max: 128 })
    );
    assert!(slug.parse(&"a".repeat(128)).is_ok());
}

#[test]
fn a_slug_is_never_a_short_id() {
    let slug = KeyPattern::Slug;
    for text in ["deadbeef", "DEADBEEF", "0123456789abcdef0123456789abcdef"] {
        assert_eq!(
            slug.parse(text),
            Err(KeyProblem::SlugLooksLikeShortId),
            "{text}"
        );
    }
}

#[test]
fn a_slug_is_never_a_uuid() {
    let slug = KeyPattern::Slug;
    let uuid = eid(42).to_string();
    assert_eq!(slug.parse(&uuid), Err(KeyProblem::SlugLooksLikeUuid));
    // Any UUID-shaped text, whatever its version.
    assert_eq!(
        slug.parse("12345678-1234-4234-8234-123456789abc"),
        Err(KeyProblem::SlugLooksLikeUuid)
    );
    // One group short of a UUID is an alias-like slug.
    assert!(slug.parse("12345678-1234-4234-8234").is_ok());
}

#[test]
fn a_cardinality_admits_its_counts() {
    assert!(Cardinality::One.admits(1));
    assert!(!Cardinality::One.admits(0));
    assert!(!Cardinality::One.admits(2));
    assert!(Cardinality::Optional.admits(0));
    assert!(Cardinality::Optional.admits(1));
    assert!(!Cardinality::Optional.admits(2));
    assert!(Cardinality::Many.admits(0));
    assert!(Cardinality::Many.admits(9));
    assert!(!Cardinality::AtLeastOne.admits(0));
    assert!(Cardinality::AtLeastOne.admits(3));
}

#[test]
fn a_book_dir_knows_what_is_under_it() {
    let docs = BookDir::new("docs").unwrap();
    let sub = BookDir::new("docs/sub").unwrap();
    assert!(docs.contains(&path("docs/a.md")));
    assert!(!docs.contains(&path("docs")));
    assert!(!docs.contains(&path("docsx/a.md")));
    assert!(docs.overlaps(&sub));
    assert!(sub.overlaps(&docs));
    assert!(docs.overlaps(&docs));
    assert!(!docs.overlaps(&BookDir::new("other").unwrap()));
}
