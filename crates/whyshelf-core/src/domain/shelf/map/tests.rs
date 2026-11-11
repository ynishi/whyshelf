//! Tests of the map of a declaration.

use super::*;
use crate::domain::shelf::artifact::LinePrefix;
use crate::domain::shelf::fixtures_tests::spaces;
use crate::domain::shelf::schema::{
    BookSchemaDecl, CompareDecl, CoverDecl, FieldEntryDecl, FieldPresence, FieldType,
    FieldTypeDecl, KeyPatternDecl, LinkEntryDecl, LinkTargetDecl,
};

fn link(name: &str, target: &str, count: Cardinality, means: Propagation) -> LinkEntryDecl {
    LinkEntryDecl {
        name: name.to_owned(),
        target: LinkTargetDecl::Book(target.to_owned()),
        cardinality: count,
        propagation: means,
        space: None,
        compare: None,
        cover: None,
    }
}

/// `decision` (prefixed, described) is declared before `requirement` (a slug, no
/// description), so the map keeps that order and not the one of the names.
fn schema() -> Schema {
    let decls = vec![
        BookSchemaDecl {
            name: "decision".to_owned(),
            dir: "books/decision".to_owned(),
            scope: Default::default(),
            key: KeyPatternDecl::Prefixed {
                prefix: "DEC".to_owned(),
            },
            description: Some("how it is met".to_owned()),
            fields: vec![
                FieldEntryDecl {
                    name: "title".to_owned(),
                    ty: FieldTypeDecl::Text,
                    presence: FieldPresence::Required,
                },
                FieldEntryDecl {
                    name: "level".to_owned(),
                    ty: FieldTypeDecl::Enum(vec!["low".to_owned(), "high".to_owned()]),
                    presence: FieldPresence::Optional,
                },
            ],
            links: vec![
                LinkEntryDecl {
                    cover: Some(CoverDecl::Word("warn".to_owned())),
                    ..link(
                        "serves",
                        "requirement",
                        Cardinality::AtLeastOne,
                        Propagation::Depends,
                    )
                },
                link(
                    "replaces",
                    "decision",
                    Cardinality::Optional,
                    Propagation::Replaces,
                ),
                LinkEntryDecl {
                    target: LinkTargetDecl::Artifact,
                    space: Some("file".to_owned()),
                    compare: Some(CompareDecl {
                        kind: "lines".to_owned(),
                        ignore: vec!["//".to_owned()],
                    }),
                    ..link("tests", "", Cardinality::Many, Propagation::Depends)
                },
                link(
                    "main",
                    "requirement",
                    Cardinality::One,
                    Propagation::Informs,
                ),
            ],
        },
        BookSchemaDecl {
            name: "requirement".to_owned(),
            dir: "books/requirement".to_owned(),
            scope: Default::default(),
            key: KeyPatternDecl::Slug,
            description: None,
            fields: Vec::new(),
            links: Vec::new(),
        },
    ];
    Schema::validate(decls, &spaces()).unwrap()
}

fn book(text: &str) -> BookName {
    BookName::new(text).unwrap()
}

#[test]
fn the_books_are_mapped_as_declared() {
    let map = ShelfMap::of(&schema());
    let names: Vec<&str> = map.books.iter().map(|b| b.name.as_str()).collect();
    assert_eq!(names, ["decision", "requirement"]);

    let decision = &map.books[0];
    assert_eq!(decision.dir.to_string(), "books/decision");
    assert!(matches!(&decision.key, KeyPattern::Prefixed(p) if p.as_str() == "DEC"));
    assert_eq!(
        decision.description.as_ref().map(|d| d.as_str()),
        Some("how it is met")
    );
    let fields: Vec<(&str, FieldPresence)> = decision
        .fields
        .iter()
        .map(|(name, decl)| (name.as_str(), decl.presence))
        .collect();
    assert_eq!(
        fields,
        [
            ("title", FieldPresence::Required),
            ("level", FieldPresence::Optional)
        ]
    );
    assert_eq!(decision.fields[0].1.ty, FieldType::Text);
    assert!(matches!(decision.fields[1].1.ty, FieldType::Enum(_)));

    let requirement = &map.books[1];
    assert_eq!(requirement.key, KeyPattern::Slug);
    assert_eq!(requirement.description, None);
    assert!(requirement.fields.is_empty());
}

#[test]
fn the_links_are_mapped_as_declared_with_every_count() {
    let map = ShelfMap::of(&schema());
    let summary: Vec<(&str, &str, Propagation, Cardinality)> = map
        .links
        .iter()
        .map(|l| {
            (
                l.from.as_str(),
                l.name.as_str(),
                l.propagation,
                l.cardinality,
            )
        })
        .collect();
    assert_eq!(
        summary,
        [
            (
                "decision",
                "serves",
                Propagation::Depends,
                Cardinality::AtLeastOne
            ),
            (
                "decision",
                "replaces",
                Propagation::Replaces,
                Cardinality::Optional
            ),
            ("decision", "tests", Propagation::Depends, Cardinality::Many),
            ("decision", "main", Propagation::Informs, Cardinality::One),
        ]
    );
    assert_eq!(
        map.links[0].target,
        MapTarget::Book {
            book: book("requirement"),
            cover: Some(CoverSeverity::Warn)
        }
    );
    // A link into its own book names that book; the map adds nothing to say so.
    assert_eq!(
        map.links[1].target,
        MapTarget::Book {
            book: book("decision"),
            cover: None
        }
    );
    assert_eq!(map.links[1].from, book("decision"));
    assert_eq!(
        map.links[2].target,
        MapTarget::Artifact {
            space: Space::new("file").unwrap(),
            comparison: Comparison::Lines {
                ignore: [LinePrefix::new("//").unwrap()].into(),
            },
            preset: None,
        }
    );
    assert_eq!(
        map.links[3].target,
        MapTarget::Book {
            book: book("requirement"),
            cover: None
        }
    );
}

#[test]
fn an_empty_declaration_maps_to_nothing() {
    let map = ShelfMap::of(&Schema::validate(Vec::new(), &spaces()).unwrap());
    assert!(map.books.is_empty());
    assert!(map.links.is_empty());
}

#[test]
fn a_link_to_artifacts_is_mapped_with_its_values_and_the_preset_it_came_from() {
    use crate::domain::shelf::schema::{
        BuiltinPresets, DefaultsDecl, PresetName, PresetValues, ShelfDecl, WrittenLinkDecl,
    };
    let artifact = |name: &str, preset: Option<&str>, space: Option<&str>| WrittenLinkDecl {
        name: name.to_owned(),
        target: LinkTargetDecl::Artifact,
        cardinality: Cardinality::Many,
        propagation: None,
        preset: preset.map(str::to_owned),
        space: space.map(str::to_owned),
        compare: None,
        ignore: None,
        cover: None,
    };
    let file = PresetName::new("file").unwrap();
    let builtins = BuiltinPresets::from([(
        file.clone(),
        PresetValues {
            space: Some("file".to_owned()),
            means: Some(Propagation::Depends),
            ..PresetValues::default()
        },
    )]);
    let decl = ShelfDecl {
        books: vec![BookSchemaDecl {
            name: "decision".to_owned(),
            dir: "books/decision".to_owned(),
            scope: Default::default(),
            key: KeyPatternDecl::Slug,
            description: None,
            fields: Vec::new(),
            links: vec![
                artifact("named", Some("file"), None),
                artifact("defaulted", None, None),
                WrittenLinkDecl {
                    propagation: Some(Propagation::Informs),
                    ..artifact("plain", None, Some("file"))
                },
            ],
        }],
        presets: Vec::new(),
        defaults: DefaultsDecl {
            artifact: Some("file".to_owned()),
        },
    };
    let map = ShelfMap::of(&Schema::declared(decl, &builtins, &spaces()).unwrap());
    let presets: Vec<Option<PresetOrigin>> = map
        .links
        .iter()
        .map(|link| match &link.target {
            MapTarget::Artifact { space, preset, .. } => {
                assert_eq!(space.as_str(), "file");
                preset.clone()
            }
            MapTarget::Book { .. } => panic!("a link to artifacts"),
        })
        .collect();
    assert_eq!(
        presets,
        [
            Some(PresetOrigin::Named(file.clone())),
            Some(PresetOrigin::Default(file)),
            None,
        ]
    );
}
