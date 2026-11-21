//! Tests of `cover` on a link: no expectation unless declared, each accepted value
//! read to its strength, a word it does not know refused, a strength alone refused on
//! a link to artifacts, and the table of a selection that a link to artifacts takes:
//! what is asked of the adapters, and what is refused of what they answer.

use super::*;
use crate::domain::shelf::artifact::SelectionCheck;
use crate::domain::shelf::fixtures_tests::{decls, file_space, spaces};

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
fn a_strength_alone_on_a_link_to_artifacts_is_refused_for_it_needs_a_selection() {
    for decl in [Some(CoverDecl::Truth(true)), word("error"), word("warn")] {
        let mut decls = decls();
        link_decl(&mut decls, "spec", "tests").cover = decl.clone();
        let problems = Schema::validate(decls, &spaces()).unwrap_err().into_vec();
        assert_eq!(problems.len(), 1, "{problems:?}");
        assert!(
            matches!(
                &problems[0],
                SchemaProblem::CoverNeedsSelection { book, link }
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

fn table(strength: Option<&str>, include: &[&str], exclude: &[&str]) -> Option<CoverDecl> {
    Some(CoverDecl::Table {
        strength: strength.map(str::to_owned),
        include: include.iter().map(|t| (*t).to_owned()).collect(),
        exclude: exclude.iter().map(|t| (*t).to_owned()).collect(),
    })
}

fn selection(include: &[&str], exclude: &[&str]) -> Selection {
    Selection::new(
        include.iter().map(|t| (*t).to_owned()).collect(),
        exclude.iter().map(|t| (*t).to_owned()).collect(),
    )
    .unwrap()
}

/// What the adapter of `file` answers for a selection it reads.
fn read_by_file(include: &[&str], exclude: &[&str]) -> SelectionChecks {
    SelectionChecks::from([(
        (file_space(), selection(include, exclude)),
        SelectionCheck::Ok,
    )])
}

fn declared_with(
    decls: Vec<BookSchemaDecl>,
    checks: &SelectionChecks,
) -> Result<Schema, Vec<SchemaProblem>> {
    Schema::declared(
        ShelfDecl::of_books(decls),
        &BuiltinPresets::new(),
        &spaces(),
        checks,
    )
    .map_err(NonEmptyVec::into_vec)
}

#[test]
fn a_table_on_a_link_to_artifacts_that_its_space_reads_is_the_cover_of_that_selection() {
    for (strength, severity) in [
        (None, CoverSeverity::Error),
        (Some("error"), CoverSeverity::Error),
        (Some("warn"), CoverSeverity::Warn),
    ] {
        let mut decls = decls();
        link_decl(&mut decls, "spec", "tests").cover =
            table(strength, &["src/**/*.rs"], &["src/gen/**"]);
        let schema =
            declared_with(decls, &read_by_file(&["src/**/*.rs"], &["src/gen/**"])).unwrap();
        let kind = schema
            .book("spec")
            .unwrap()
            .artifact_links()
            .get("tests")
            .unwrap();
        let cover = kind.cover.as_ref().expect("a cover");
        assert_eq!(cover.severity(), severity, "{strength:?}");
        assert_eq!(
            cover.selection(),
            &selection(&["src/**/*.rs"], &["src/gen/**"])
        );
        assert_eq!(
            schema.cover_selections(),
            BTreeSet::from([(file_space(), selection(&["src/**/*.rs"], &["src/gen/**"]))])
        );
    }
}

#[test]
fn a_selection_that_its_space_does_not_read_or_cannot_list_or_nobody_asked_about_is_refused() {
    let why = NonBlank::new("not a glob").unwrap();
    for (checks, expect) in [
        (
            SelectionChecks::from([(
                (file_space(), selection(&["a"], &[])),
                SelectionCheck::NotASelection(why.clone()),
            )]),
            "not_a_selection",
        ),
        (
            SelectionChecks::from([(
                (file_space(), selection(&["a"], &[])),
                SelectionCheck::CannotList(why.clone()),
            )]),
            "cannot_list",
        ),
        // No answer for the selection: read as a space that cannot list.
        (SelectionChecks::new(), "not asked"),
    ] {
        let mut decls = decls();
        link_decl(&mut decls, "spec", "tests").cover = table(None, &["a"], &[]);
        let problems = declared_with(decls, &checks).unwrap_err();
        assert_eq!(problems.len(), 1, "{expect}: {problems:?}");
        let text = problems[0].to_string();
        match expect {
            "not_a_selection" => assert!(
                matches!(&problems[0], SchemaProblem::NotASelection { why: w, .. } if *w == why),
                "{problems:?}"
            ),
            "cannot_list" => assert!(
                matches!(&problems[0], SchemaProblem::SpaceCannotList { why: w, .. } if *w == why),
                "{problems:?}"
            ),
            _ => assert!(
                matches!(&problems[0], SchemaProblem::SpaceCannotList { why: w, .. } if w.as_str() == "not asked"),
                "{problems:?}"
            ),
        }
        assert!(
            text.contains("`spec`") && text.contains("`tests`"),
            "{text}"
        );
    }
}

#[test]
fn the_validate_that_asks_nothing_refuses_a_cover_of_artifacts_as_a_space_that_cannot_list() {
    let mut decls = decls();
    link_decl(&mut decls, "spec", "tests").cover = table(None, &["a"], &[]);
    let problems = Schema::validate(decls, &spaces()).unwrap_err().into_vec();
    assert!(
        matches!(&problems[..], [SchemaProblem::SpaceCannotList { why, .. }] if why.as_str() == "not asked"),
        "{problems:?}"
    );
}

#[test]
fn a_selection_that_cannot_be_made_or_a_table_on_a_book_link_or_a_bad_strength_is_refused() {
    let mut decls_ = decls();
    link_decl(&mut decls_, "spec", "tests").cover = table(None, &[], &[]);
    let problems = Schema::validate(decls_, &spaces()).unwrap_err().into_vec();
    assert!(
        matches!(
            &problems[..],
            [SchemaProblem::BadSelection {
                why: SelectionProblem::NoInclude,
                ..
            }]
        ),
        "{problems:?}"
    );

    let mut decls_ = decls();
    link_decl(&mut decls_, "spec", "tests").cover = table(None, &["a", ""], &[]);
    let problems = Schema::validate(decls_, &spaces()).unwrap_err().into_vec();
    assert!(
        matches!(
            &problems[..],
            [SchemaProblem::BadSelection {
                why: SelectionProblem::EmptyText { .. },
                ..
            }]
        ),
        "{problems:?}"
    );

    let mut decls_ = decls();
    link_decl(&mut decls_, "spec", "grounds").cover = table(None, &["a"], &[]);
    let problems = Schema::validate(decls_, &spaces()).unwrap_err().into_vec();
    assert!(
        matches!(&problems[..], [SchemaProblem::SelectionOnBookLink { book, link }]
            if book == "spec" && link.as_str() == "grounds"),
        "{problems:?}"
    );

    let mut decls_ = decls();
    link_decl(&mut decls_, "spec", "tests").cover = table(Some("sometimes"), &["a"], &[]);
    let problems = Schema::validate(decls_, &spaces()).unwrap_err().into_vec();
    assert!(
        matches!(&problems[..], [SchemaProblem::BadCover { value, .. }] if value == "sometimes"),
        "{problems:?}"
    );
}

#[test]
fn the_selections_asked_are_those_validate_reads_and_two_links_that_share_one_ask_once() {
    let mut decls_ = decls();
    link_decl(&mut decls_, "spec", "tests").cover = table(None, &["src/**/*.rs"], &[]);
    // Another link, to artifacts too, declaring the same selection in the same space.
    let book = decls_.iter_mut().find(|b| b.name == "design").unwrap();
    let mut twin = decls()
        .iter()
        .find(|b| b.name == "spec")
        .unwrap()
        .links
        .iter()
        .find(|l| l.name == "tests")
        .unwrap()
        .clone();
    twin.name = "evidence".to_owned();
    twin.cover = table(Some("warn"), &["src/**/*.rs"], &[]);
    book.links.push(twin);
    let decl = ShelfDecl::of_books(decls_.clone());
    let asks = Schema::selection_asks(&decl, &spaces(), &BuiltinPresets::new());
    assert_eq!(
        asks,
        BTreeSet::from([(file_space(), selection(&["src/**/*.rs"], &[]))])
    );
    // What validate would read of the same declaration is what is asked: answered, no
    // problem is left.
    let checks: SelectionChecks = asks
        .into_iter()
        .map(|key| (key, SelectionCheck::Ok))
        .collect();
    let schema = declared_with(decls_, &checks).unwrap();
    assert_eq!(schema.cover_selections().len(), 1);
}

#[test]
fn a_link_that_has_a_problem_of_its_own_asks_nothing() {
    // A space no adapter offers, a cover that is refused, a link that is no link to
    // artifacts: none is asked, and `declared` tells the problem of each once.
    let mut decls_ = decls();
    link_decl(&mut decls_, "spec", "tests").space = Some("nowhere".to_owned());
    link_decl(&mut decls_, "spec", "tests").cover = table(None, &["a"], &[]);
    link_decl(&mut decls_, "spec", "grounds").cover = table(None, &["b"], &[]);
    let decl = ShelfDecl::of_books(decls_.clone());
    assert!(Schema::selection_asks(&decl, &spaces(), &BuiltinPresets::new()).is_empty());
    let problems = declared_with(decls_, &SelectionChecks::new()).unwrap_err();
    assert_eq!(problems.len(), 2, "{problems:?}");
}
