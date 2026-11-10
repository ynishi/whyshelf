//! Tests of `compare` in the declaration of a link: only a link to artifacts takes
//! it, it must name a comparison, and none is `bytes`.

use super::*;
use crate::domain::shelf::artifact::LinePrefix;
use crate::domain::shelf::fixtures_tests::spaces;

/// A `compare` as written: its kind and the prefixes it ignores.
type Written<'a> = Option<(&'a str, &'a [&'a str])>;

/// One book `a` with one link `tests`, to artifacts or into `a`, comparing as given.
fn decl(target: LinkTargetDecl, compare: Option<(&str, &[&str])>) -> Vec<BookSchemaDecl> {
    vec![BookSchemaDecl {
        name: "a".to_owned(),
        dir: "a".to_owned(),
        key: KeyPatternDecl::Slug,
        fields: Vec::new(),
        links: vec![LinkEntryDecl {
            name: "tests".to_owned(),
            space: matches!(target, LinkTargetDecl::Artifact).then(|| "file".to_owned()),
            target,
            cardinality: Cardinality::Many,
            propagation: Propagation::Depends,
            compare: compare.map(|(kind, ignore)| CompareDecl {
                kind: kind.to_owned(),
                ignore: ignore.iter().map(|i| (*i).to_owned()).collect(),
            }),
            cover: false,
        }],
    }]
}

fn comparison_of(compare: Option<(&str, &[&str])>) -> Comparison {
    let schema = Schema::validate(decl(LinkTargetDecl::Artifact, compare), &spaces()).unwrap();
    schema.book("a").unwrap().artifact_links()["tests"]
        .comparison
        .clone()
}

fn problem_of(target: LinkTargetDecl, compare: Option<(&str, &[&str])>) -> SchemaProblem {
    let problems = Schema::validate(decl(target, compare), &spaces())
        .unwrap_err()
        .into_vec();
    assert_eq!(problems.len(), 1, "{problems:?}");
    problems.into_iter().next().unwrap()
}

#[test]
fn a_link_to_artifacts_compares_bytes_unless_it_says_otherwise() {
    assert_eq!(comparison_of(None), Comparison::Bytes);
    assert_eq!(comparison_of(Some(("bytes", &[]))), Comparison::Bytes);
    assert_eq!(
        comparison_of(Some(("lines", &["//", "#"]))),
        Comparison::Lines {
            ignore: ["#", "//"]
                .iter()
                .map(|p| LinePrefix::new(p).unwrap())
                .collect()
        }
    );
}

#[test]
fn compare_on_a_link_into_a_book_is_refused() {
    let problem = problem_of(LinkTargetDecl::Book("a".to_owned()), Some(("lines", &[])));
    assert!(
        matches!(&problem, SchemaProblem::CompareOnBookLink { book, link }
            if book == "a" && link.as_str() == "tests"),
        "{problem:?}"
    );
}

#[test]
fn a_compare_that_is_no_comparison_is_refused() {
    let cases: [(Written<'_>, ComparisonProblem); 4] = [
        (
            Some(("words", &[])),
            ComparisonProblem::UnknownKind {
                found: "words".to_owned(),
            },
        ),
        // `ignore` written without `compare`, as the adapter passes it on.
        (Some(("", &["//"])), ComparisonProblem::NoKind),
        (Some(("bytes", &["//"])), ComparisonProblem::IgnoreWithBytes),
        (
            Some(("lines", &["/ /"])),
            ComparisonProblem::BadPrefix {
                found: "/ /".to_owned(),
            },
        ),
    ];
    for (compare, expected) in cases {
        let problem = problem_of(LinkTargetDecl::Artifact, compare);
        assert!(
            matches!(&problem, SchemaProblem::BadComparison { book, link, why }
                if book == "a" && link.as_str() == "tests" && *why == expected),
            "{compare:?}: {problem:?}"
        );
    }
}
