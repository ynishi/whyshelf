//! Tests of `cover` on a link: false unless declared, read when declared, and
//! refused on a link to artifacts.

use super::*;
use crate::domain::shelf::fixtures_tests::{decls, spaces};

fn link_decl<'d>(decls: &'d mut [BookSchemaDecl], book: &str, link: &str) -> &'d mut LinkEntryDecl {
    decls
        .iter_mut()
        .find(|b| b.name == book)
        .and_then(|b| b.links.iter_mut().find(|l| l.name == link))
        .expect("the fixtures declare it")
}

fn cover_of(schema: &Schema, book: &str, link: &str) -> bool {
    schema
        .book(book)
        .expect("the book")
        .links()
        .iter()
        .find(|(name, _)| name.as_str() == link)
        .map(|(_, kind)| kind.cover)
        .expect("the link")
}

#[test]
fn cover_is_false_unless_declared() {
    let schema = Schema::validate(decls(), &spaces()).unwrap();
    for (book, link) in [
        ("finding", "base"),
        ("finding", "related"),
        ("finding", "supersedes"),
        ("spec", "grounds"),
        ("design", "specs"),
    ] {
        assert!(!cover_of(&schema, book, link), "{book}.{link}");
    }
}

#[test]
fn cover_is_read_when_declared() {
    let mut decls = decls();
    link_decl(&mut decls, "spec", "grounds").cover = true;
    let schema = Schema::validate(decls, &spaces()).unwrap();
    assert!(cover_of(&schema, "spec", "grounds"));
    assert!(!cover_of(&schema, "finding", "base"));
}

#[test]
fn cover_on_a_link_to_artifacts_is_refused() {
    let mut decls = decls();
    link_decl(&mut decls, "spec", "tests").cover = true;
    let problems = Schema::validate(decls, &spaces()).unwrap_err().into_vec();
    assert_eq!(problems.len(), 1, "{problems:?}");
    assert!(
        matches!(
            &problems[0],
            SchemaProblem::CoverOnArtifactLink { book, link }
                if book == "spec" && link.as_str() == "tests"
        ),
        "{problems:?}"
    );
}
