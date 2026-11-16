//! The words of a declaration for a count, a type and a meaning: what each reads
//! back to, what the one reader ([`read_word`]) refuses, and how a refusal is told.

use super::*;
use crate::domain::shelf::fixtures_tests::spaces;

fn word_of<T: DeclWord>(values: &[T]) -> Vec<&'static str> {
    values.iter().map(|value| value.word()).collect()
}

#[test]
fn every_value_of_a_type_reads_back_from_its_word_and_no_other_text_does() {
    for scalar in ScalarType::ALL {
        assert_eq!(ScalarType::from_word(scalar.word()), Some(scalar));
    }
    for count in Cardinality::ALL {
        assert_eq!(Cardinality::from_word(count.word()), Some(count));
    }
    for means in Propagation::ALL {
        assert_eq!(Propagation::from_word(means.word()), Some(means));
    }
    for unknown in ["", "several", "One", " one", "one ", "AT-LEAST-ONE", "enum"] {
        assert_eq!(Cardinality::from_word(unknown), None, "{unknown:?}");
        assert_eq!(ScalarType::from_word(unknown), None, "{unknown:?}");
        assert_eq!(Propagation::from_word(unknown), None, "{unknown:?}");
    }
}

#[test]
fn the_words_are_those_a_declaration_writes_and_each_is_one_value() {
    assert_eq!(
        word_of(&ScalarType::ALL),
        ["text", "integer", "bool", "list"]
    );
    assert_eq!(
        word_of(&Cardinality::ALL),
        ["one", "optional", "many", "at-least-one"]
    );
    assert_eq!(
        word_of(&Propagation::ALL),
        ["depends", "informs", "replaces"]
    );
}

#[test]
fn the_word_of_a_field_type_is_that_of_its_scalar_or_enum() {
    for scalar in ScalarType::ALL {
        assert_eq!(FieldType::Scalar(scalar).word(), scalar.word());
    }
    let set = std::iter::once(FieldWord::new("low").unwrap()).collect();
    let ty = FieldType::Enum(NonEmptySet::from_set(set).unwrap());
    assert_eq!(ty.word(), "enum");
}

#[test]
fn a_count_says_in_words_how_many_it_admits() {
    let said: Vec<(&str, &str)> = Cardinality::ALL
        .into_iter()
        .map(|count| (count.word(), count.admits_in_words()))
        .collect();
    assert_eq!(
        said,
        [
            ("one", "exactly one"),
            ("optional", "none or one"),
            ("many", "any number"),
            ("at-least-one", "one or more"),
        ]
    );
}

#[test]
fn a_count_that_is_not_written_is_many() {
    assert_eq!(Cardinality::WHEN_UNWRITTEN, Cardinality::Many);
    let link = WrittenLinkDecl {
        name: "up".to_owned(),
        target: LinkTargetDecl::Book("b".to_owned()),
        count: None,
        means: Some("depends".to_owned()),
        preset: None,
        space: None,
        compare: None,
        ignore: None,
        cover: None,
    };
    let book = WrittenBookDecl {
        name: "b".to_owned(),
        dir: "b".to_owned(),
        scope: Default::default(),
        key: KeyPatternDecl::Slug,
        description: None,
        fields: Vec::new(),
        links: vec![link],
    };
    let decl = ShelfDecl {
        books: vec![book],
        ..ShelfDecl::default()
    };
    let expanded = expand(decl, &BuiltinPresets::new(), &spaces()).unwrap();
    assert_eq!(expanded.books()[0].links[0].cardinality, Cardinality::Many);
}

#[test]
fn a_link_put_back_into_words_expands_to_the_same_link() {
    for cardinality in Cardinality::ALL {
        for propagation in Propagation::ALL {
            let link = LinkEntryDecl {
                name: "up".to_owned(),
                target: LinkTargetDecl::Book("b".to_owned()),
                cardinality,
                propagation,
                space: None,
                compare: None,
                cover: None,
            };
            let written = WrittenLinkDecl::from(link.clone());
            assert_eq!(written.count.as_deref(), Some(cardinality.word()));
            assert_eq!(written.means.as_deref(), Some(propagation.word()));
            let book = WrittenBookDecl {
                name: "b".to_owned(),
                dir: "b".to_owned(),
                scope: Default::default(),
                key: KeyPatternDecl::Slug,
                description: None,
                fields: Vec::new(),
                links: vec![written],
            };
            let decl = ShelfDecl {
                books: vec![book],
                ..ShelfDecl::default()
            };
            let expanded = expand(decl, &BuiltinPresets::new(), &spaces()).unwrap();
            assert_eq!(expanded.books()[0].links, [link]);
        }
    }
}

#[test]
fn read_word_reads_a_word_or_says_where_it_was_refused() {
    let at = || WordAt::Field {
        book: "note".to_owned(),
        field: "n".to_owned(),
    };
    assert_eq!(
        read_word::<ScalarType>("integer", at()),
        Ok(ScalarType::Integer)
    );
    assert_eq!(
        read_word::<ScalarType>("number", at()),
        Err(SchemaProblem::UnknownWord {
            at: at(),
            word: "number".to_owned(),
            of: WordOf::Type,
        })
    );
    assert_eq!(
        read_word::<Cardinality>("none", at()),
        Err(SchemaProblem::UnknownWord {
            at: at(),
            word: "none".to_owned(),
            of: WordOf::Count,
        })
    );
    assert_eq!(
        read_word::<Propagation>("leans", at()),
        Err(SchemaProblem::UnknownWord {
            at: at(),
            word: "leans".to_owned(),
            of: WordOf::Means,
        })
    );
}

/// A book `note` with a field `n` of the type `ty` and a link `grounds` into itself
/// with this count and meaning, as written.
fn note(ty: &str, count: Option<&str>, means: &str) -> WrittenBookDecl {
    WrittenBookDecl {
        name: "note".to_owned(),
        dir: "notes".to_owned(),
        scope: Default::default(),
        key: KeyPatternDecl::Slug,
        description: None,
        fields: vec![FieldEntryDecl {
            name: "n".to_owned(),
            ty: FieldTypeDecl::Word(ty.to_owned()),
            presence: FieldPresence::Optional,
        }],
        links: vec![WrittenLinkDecl {
            name: "grounds".to_owned(),
            target: LinkTargetDecl::Book("note".to_owned()),
            count: count.map(str::to_owned),
            means: Some(means.to_owned()),
            preset: None,
            space: None,
            compare: None,
            ignore: None,
            cover: None,
        }],
    }
}

#[test]
fn a_word_that_is_none_is_a_problem_with_the_book_the_entry_and_the_words_it_takes() {
    let decl = ShelfDecl {
        books: vec![note("number", Some("several"), "leans")],
        ..ShelfDecl::default()
    };
    let found: Vec<String> = Schema::declared(decl, &BuiltinPresets::new(), &spaces())
        .unwrap_err()
        .into_vec()
        .iter()
        .map(ToString::to_string)
        .collect();
    assert_eq!(
        found,
        [
            "book `note`: the count of the link `grounds` is \"several\", and it takes one of \
             \"one\", \"optional\", \"many\", \"at-least-one\"",
            "book `note`: the meaning of the link `grounds` is \"leans\", and it takes one of \
             \"depends\", \"informs\", \"replaces\"",
            "book `note`: the type of the field `n` is \"number\", and it takes one of \
             \"text\", \"integer\", \"bool\", \"list\"",
        ]
    );
}

#[test]
fn a_word_that_is_none_is_told_with_the_other_problems_of_the_declaration() {
    let mut bad_book = note("text", Some("one"), "depends");
    bad_book.name = "Bad Book".to_owned();
    let decl = ShelfDecl {
        books: vec![note("number", Some("several"), "depends"), bad_book],
        ..ShelfDecl::default()
    };
    let found = Schema::declared(decl, &BuiltinPresets::new(), &spaces())
        .unwrap_err()
        .into_vec();
    let words = found
        .iter()
        .filter(|p| matches!(p, SchemaProblem::UnknownWord { .. }))
        .count();
    assert_eq!(words, 2, "{found:?}");
    assert!(
        found.iter().any(|p| matches!(
            p,
            SchemaProblem::BadName {
                place: NamePlace::Book,
                ..
            }
        )),
        "{found:?}"
    );
}
