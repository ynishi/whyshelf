//! Tests of `cover` on a link: no expectation unless declared, each accepted value
//! read to its strength, a word it does not know refused, and a strength refused
//! on a link to artifacts.

use super::*;
use crate::domain::shelf::fixtures_tests::{decls, spaces};

fn link_decl<'d>(decls: &'d mut [BookSchemaDecl], book: &str, link: &str) -> &'d mut LinkEntryDecl {
    decls
        .iter_mut()
        .find(|b| b.name == book)
        .and_then(|b| b.links.iter_mut().find(|l| l.name == link))
        .expect("the fixtures declare it")
}

fn cover_of(schema: &Schema, book: &str, link: &str) -> Option<CoverSeverity> {
    schema
        .book(book)
        .expect("the book")
        .links()
        .iter()
        .find(|(name, _)| name.as_str() == link)
        .map(|(_, kind)| kind.cover)
        .expect("the link")
}

fn word(text: &str) -> Option<CoverDecl> {
    Some(CoverDecl::Word(text.to_owned()))
}

#[test]
fn cover_is_none_unless_declared() {
    let schema = Schema::validate(decls(), &spaces()).unwrap();
    for (book, link) in [
        ("finding", "base"),
        ("finding", "related"),
        ("finding", "supersedes"),
        ("spec", "grounds"),
        ("design", "specs"),
    ] {
        assert_eq!(cover_of(&schema, book, link), None, "{book}.{link}");
    }
}

#[test]
fn each_accepted_cover_is_read_to_its_strength() {
    for (decl, strength) in [
        (Some(CoverDecl::Truth(true)), Some(CoverSeverity::Error)),
        (word("error"), Some(CoverSeverity::Error)),
        (word("warn"), Some(CoverSeverity::Warn)),
        (Some(CoverDecl::Truth(false)), None),
        (None, None),
    ] {
        let mut decls = decls();
        link_decl(&mut decls, "spec", "grounds").cover = decl.clone();
        let schema = Schema::validate(decls, &spaces()).unwrap();
        assert_eq!(cover_of(&schema, "spec", "grounds"), strength, "{decl:?}");
        assert_eq!(cover_of(&schema, "finding", "base"), None);
    }
}

#[test]
fn a_cover_word_that_is_no_strength_is_refused_and_named() {
    for text in ["fatal", "Warn", "true", ""] {
        let mut decls = decls();
        link_decl(&mut decls, "spec", "grounds").cover = word(text);
        let problems = Schema::validate(decls, &spaces()).unwrap_err().into_vec();
        assert_eq!(problems.len(), 1, "{problems:?}");
        assert!(
            matches!(
                &problems[0],
                SchemaProblem::BadCover { book, link, value }
                    if book == "spec" && link.as_str() == "grounds" && value == text
            ),
            "{problems:?}"
        );
        assert!(
            problems[0].to_string().contains(&format!("\"{text}\"")),
            "{}",
            problems[0]
        );
    }
}

#[test]
fn an_unknown_cover_word_on_a_link_to_artifacts_is_refused_and_named() {
    let mut decls = decls();
    link_decl(&mut decls, "spec", "tests").cover = word("sometimes");
    let problems = Schema::validate(decls, &spaces()).unwrap_err().into_vec();
    assert_eq!(problems.len(), 1, "{problems:?}");
    assert!(
        matches!(
            &problems[0],
            SchemaProblem::BadCover { book, link, value }
                if book == "spec" && link.as_str() == "tests" && value == "sometimes"
        ),
        "{problems:?}"
    );
    assert!(
        problems[0].to_string().contains("\"sometimes\""),
        "{}",
        problems[0]
    );
}

#[test]
fn a_strength_on_a_link_to_artifacts_is_refused() {
    for decl in [Some(CoverDecl::Truth(true)), word("error"), word("warn")] {
        let mut decls = decls();
        link_decl(&mut decls, "spec", "tests").cover = decl.clone();
        let problems = Schema::validate(decls, &spaces()).unwrap_err().into_vec();
        assert_eq!(problems.len(), 1, "{problems:?}");
        assert!(
            matches!(
                &problems[0],
                SchemaProblem::CoverOnArtifactLink { book, link }
                    if book == "spec" && link.as_str() == "tests"
            ),
            "{decl:?}: {problems:?}"
        );
    }
    // No expectation is no `cover` at all.
    let mut decls = decls();
    link_decl(&mut decls, "spec", "tests").cover = Some(CoverDecl::Truth(false));
    assert!(Schema::validate(decls, &spaces()).is_ok());
}
