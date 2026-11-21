//! Tests of a cover over artifacts in the use cases, over the in-memory fake: what
//! `check` lists and when it is not clean, the one listing two links with one
//! selection share, `cover` and the exemption of an artifact, and that the one way a
//! declaration becomes a schema leaves no selection unasked, for `load` and for
//! `init`.

use std::collections::BTreeSet;

use crate::domain::shelf::artifact::{Completeness, NameListing, Selection};
use crate::domain::shelf::coverage::{CoverAssessment, NameCover};
use crate::domain::shelf::exemption::{CoverMiss, ExemptRefusal, Unneeded};
use crate::domain::shelf::schema::{
    BookName, BookSchemaDecl, Cardinality, CoverDecl, CoverSeverity, EntryName, LinkEntryDecl,
    LinkTargetDecl, Propagation, Schema, SchemaProblem, ShelfDecl,
};

use super::fake_tests::{
    FakeShelf, aname, block_on, decls, file_space, other_space, reason, sp_tested,
};
use super::load::declared_schema;
use super::ports::ArtifactSource;
use super::{
    ArtifactCoverLists, BooksCheck, CoverLinkAsk, CoverReport, ErrorClass, ExemptAsk,
    ExemptRequest, ShelfApp, ShelfError,
};

fn table(strength: Option<&str>) -> CoverDecl {
    CoverDecl::Table {
        strength: strength.map(str::to_owned),
        include: vec!["src/**".to_owned()],
        exclude: Vec::new(),
    }
}

fn selection() -> Selection {
    Selection::new(vec!["src/**".to_owned()], Vec::new()).unwrap()
}

/// Every name of the listing, `a.rs` and `b.rs`.
fn all() -> NameListing {
    NameListing::All(BTreeSet::from([aname("a.rs"), aname("b.rs")]))
}

/// The fake's books with the spec's `tests` (to the space `file`) covering as `cover`
/// says, a document that links `a.rs`, and these listings for the space.
fn app_listing(cover: CoverDecl, listing: NameListing) -> ShelfApp<FakeShelf> {
    let mut covering = decls();
    link_of(&mut covering, "tests").cover = Some(cover);
    let fake = FakeShelf::new().with_files(vec![sp_tested(3, "cache", &[], &["a.rs"])]);
    *fake.decls.lock().unwrap() = covering;
    fake.list_names(&file_space(), listing);
    ShelfApp::new(fake)
}

fn link_of<'d>(decls: &'d mut [BookSchemaDecl], name: &str) -> &'d mut LinkEntryDecl {
    decls
        .iter_mut()
        .find(|b| b.name == "spec")
        .and_then(|b| b.links.iter_mut().find(|l| l.name == name))
        .expect("the fake declares it")
}

fn check(app: &ShelfApp<FakeShelf>) -> BooksCheck {
    block_on(app.books_check()).unwrap()
}

fn parts(check: &BooksCheck) -> (&ArtifactCoverLists, &CoverAssessment) {
    match check {
        BooksCheck::Checked {
            artifacts, cover, ..
        } => (artifacts, cover),
        BooksCheck::SchemaInvalid(problems) => panic!("the declaration is sound: {problems:?}"),
    }
}

#[test]
fn a_name_the_listing_has_and_no_document_reaches_is_a_gap_and_an_error_one_fails_the_check() {
    let app = app_listing(table(None), all());
    let checked = check(&app);
    let (lists, cover) = parts(&checked);
    assert!(!checked.is_clean());
    assert_eq!(lists.uncovered.len(), 1, "{lists:?}");
    assert_eq!(lists.uncovered[0].artifact.name(), &aname("b.rs"));
    assert_eq!(lists.uncovered[0].severity, CoverSeverity::Error);
    assert_eq!(cover.links().len(), 1);
    assert_eq!(cover.links()[0].completeness(), &Completeness::All);
    assert_eq!(
        cover.links()[0].names(),
        [
            (aname("a.rs"), NameCover::Reached),
            (aname("b.rs"), NameCover::Gap)
        ]
    );

    let warned = check(&app_listing(table(Some("warn")), all()));
    assert!(warned.is_clean(), "{warned:?}");
    assert_eq!(parts(&warned).0.uncovered[0].severity, CoverSeverity::Warn);
}

#[test]
fn a_cover_that_is_all_reached_is_clean() {
    let app = app_listing(
        table(None),
        NameListing::All(BTreeSet::from([aname("a.rs")])),
    );
    let checked = check(&app);
    assert!(checked.is_clean(), "{checked:?}");
}

#[test]
fn a_listing_that_is_not_the_whole_fails_an_error_cover_and_not_a_warn_one() {
    let why = reason("cut at 1");
    for listing in [
        NameListing::Some {
            found: BTreeSet::from([aname("a.rs")]),
            why: why.clone(),
        },
        NameListing::CannotList(why.clone()),
    ] {
        let failing = check(&app_listing(table(None), listing.clone()));
        assert!(!failing.is_clean(), "{listing:?}");
        let (lists, cover) = parts(&failing);
        // No gap is told of a listing that proves none, and the link says why.
        assert!(lists.uncovered.is_empty(), "{lists:?}");
        assert!(!cover.links()[0].completeness().is_all());
        assert!(cover.links()[0].names().is_empty());

        let warned = check(&app_listing(table(Some("warn")), listing.clone()));
        assert!(warned.is_clean(), "{listing:?}");
    }
}

#[test]
fn a_selection_that_takes_nothing_fails_an_error_cover_and_is_told_by_a_warn_one() {
    let empty = NameListing::All(BTreeSet::new());
    let failing = check(&app_listing(table(None), empty.clone()));
    assert!(!failing.is_clean());
    assert_eq!(parts(&failing).1.nothing_selected().count(), 1);

    let warned = check(&app_listing(table(Some("warn")), empty));
    assert!(warned.is_clean());
    assert_eq!(parts(&warned).1.nothing_selected().count(), 1);
}

#[test]
fn two_links_with_one_selection_share_one_listing_and_both_show_the_same_completeness() {
    let mut covering = decls();
    // Another link to the same space with the same selection, of another strength.
    let mut twin = link_of(&mut covering, "tests").clone();
    twin.name = "more".to_owned();
    link_of(&mut covering, "tests").cover = Some(table(None));
    twin.cover = Some(table(Some("warn")));
    covering
        .iter_mut()
        .find(|b| b.name == "spec")
        .unwrap()
        .links
        .push(twin);
    let fake = FakeShelf::new().with_files(vec![sp_tested(3, "cache", &[], &["a.rs"])]);
    *fake.decls.lock().unwrap() = covering;
    fake.list_names(
        &file_space(),
        NameListing::Some {
            found: BTreeSet::from([aname("a.rs")]),
            why: reason("cut at 1"),
        },
    );
    let app = ShelfApp::new(fake);
    let checked = check(&app);
    let (_, cover) = parts(&checked);
    assert_eq!(cover.links().len(), 2);
    assert_eq!(
        cover.links()[0].completeness(),
        cover.links()[1].completeness()
    );
    assert!(matches!(
        cover.links()[0].completeness(),
        Completeness::Partial(_)
    ));
    // One ask of the adapter, whatever the number of links that declare it.
    assert_eq!(app.ports().listed(), [(file_space(), selection())]);
    // The two links differ only in strength: the `error` one fails the check.
    assert!(!checked.is_clean());
}

#[test]
fn a_check_that_declares_no_cover_over_artifacts_asks_no_listing() {
    let fake = FakeShelf::new().with_files(vec![sp_tested(3, "cache", &[], &["a.rs"])]);
    fake.list_names(&file_space(), all());
    let app = ShelfApp::new(fake);
    assert!(check(&app).is_clean());
    assert!(app.ports().listed().is_empty());
}

#[test]
fn cover_tells_each_link_with_its_names_and_the_exemptions_of_them() {
    let mut app = app_listing(table(None), all());
    let before = block_on(app.cover(None)).unwrap();
    let CoverReport::Covered { links, lists, .. } = before else {
        panic!("the declaration is sound");
    };
    assert_eq!(links.len(), 1);
    assert_eq!(links[0].book().as_str(), "spec");
    assert_eq!(links[0].link().as_str(), "tests");
    assert_eq!(links[0].selection(), &selection());
    assert_eq!(lists.uncovered.len(), 1);

    // Exempt the gap: it stands as exempt, and is no gap.
    let done = block_on(app.exempt(vec![exempt_request("b.rs")])).unwrap();
    assert_eq!(done[0].doc(), None);
    assert_eq!(done[0].artifact().unwrap().name(), &aname("b.rs"));
    let after = block_on(app.cover(Some(CoverLinkAsk {
        book: BookName::new("spec").unwrap(),
        link: EntryName::new("tests").unwrap(),
    })))
    .unwrap();
    let CoverReport::Covered { links, lists, .. } = after else {
        panic!("the declaration is sound");
    };
    assert!(lists.uncovered.is_empty());
    assert_eq!(lists.exempt.len(), 1);
    assert!(matches!(
        links[0].names()[1].1,
        NameCover::Exempt(id) if id == lists.exempt[0].record.id()
    ));
    assert!(check(&app).is_clean());
}

fn exempt_request(name: &str) -> ExemptRequest {
    ExemptRequest {
        subject: ExemptAsk::Artifact(aname(name)),
        book: BookName::new("spec").unwrap(),
        link: EntryName::new("tests").unwrap(),
        reason: reason("kept on purpose"),
        by: Default::default(),
    }
}

#[test]
fn cover_of_a_link_that_covers_no_artifacts_is_a_request_error() {
    let app = app_listing(table(None), all());
    for (book, link) in [
        ("spec", "grounds"),
        ("spec", "nowhere"),
        ("finding", "base"),
    ] {
        let error = block_on(app.cover(Some(CoverLinkAsk {
            book: BookName::new(book).unwrap(),
            link: EntryName::new(link).unwrap(),
        })))
        .unwrap_err();
        assert!(
            matches!(error, ShelfError::NotACoverLink { .. }),
            "{error:?}"
        );
        assert_eq!(error.class(), ErrorClass::Request);
    }
}

#[test]
fn an_artifact_the_selection_does_not_take_or_a_document_reaches_or_a_listing_not_whole_is_refused()
{
    let mut app = app_listing(table(None), all());
    for (name, expect) in [("z.rs", "not_listed"), ("a.rs", "reached")] {
        let error = block_on(app.exempt(vec![exempt_request(name)])).unwrap_err();
        let ShelfError::ExemptRefused(refusal) = &error else {
            panic!("{error:?}");
        };
        match (expect, refusal) {
            ("not_listed", ExemptRefusal::NotCovering { .. }) => {}
            ("reached", ExemptRefusal::Reached { .. }) => {}
            other => panic!("{name}: {other:?}"),
        }
        assert_eq!(error.class(), ErrorClass::Request);
    }

    // The names cannot all be listed: no exemption can be told to be needed.
    let mut app = app_listing(
        table(None),
        NameListing::Some {
            found: BTreeSet::from([aname("a.rs")]),
            why: reason("cut at 1"),
        },
    );
    let error = block_on(app.exempt(vec![exempt_request("b.rs")])).unwrap_err();
    assert!(
        matches!(
            error,
            ShelfError::ExemptRefused(ExemptRefusal::CannotTell { .. })
        ),
        "{error:?}"
    );
    assert_eq!(error.class(), ErrorClass::Refused);
    // Nothing was written.
    assert_eq!(app.ports().record_count(), 0);

    // A link to no artifacts cannot take one.
    let mut app = app_listing(table(None), all());
    let mut wrong = exempt_request("b.rs");
    wrong.link = EntryName::new("grounds").unwrap();
    let error = block_on(app.exempt(vec![wrong])).unwrap_err();
    assert!(
        matches!(
            error,
            ShelfError::ExemptRefused(ExemptRefusal::NotCovering { .. })
        ),
        "{error:?}"
    );
}

#[test]
fn an_exemption_whose_name_left_the_listing_is_not_listed_and_one_the_listing_cannot_tell_is_not_said()
 {
    let mut app = app_listing(table(None), all());
    block_on(app.exempt(vec![exempt_request("b.rs")])).unwrap();
    // `b.rs` is gone from what the selection takes now.
    app.ports().list_names(
        &file_space(),
        NameListing::All(BTreeSet::from([aname("a.rs"), aname("c.rs")])),
    );
    let checked = check(&app);
    let (lists, _) = parts(&checked);
    assert_eq!(lists.unneeded.len(), 1);
    assert!(matches!(
        &lists.unneeded[0].why,
        Unneeded::Cover(CoverMiss::NotListed)
    ));
    assert!(lists.exempt.is_empty());
    // And `c.rs` is the new gap.
    assert_eq!(lists.uncovered[0].artifact.name(), &aname("c.rs"));

    // With the listing no longer the whole, the exemption is neither needed nor
    // unneeded: nothing is said of it.
    app.ports().list_names(
        &file_space(),
        NameListing::CannotList(reason("no work tree")),
    );
    let checked = check(&app);
    let (lists, _) = parts(&checked);
    assert!(lists.exempt.is_empty() && lists.unneeded.is_empty());
}

#[test]
fn a_selection_that_its_space_does_not_list_is_a_problem_of_the_declaration() {
    // The fake lists nothing for `other`: a cover over it is refused by `declared`.
    let mut covering = decls();
    let link = link_of(&mut covering, "tests");
    link.space = Some("other".to_owned());
    link.cover = Some(table(None));
    let fake = FakeShelf::new();
    *fake.decls.lock().unwrap() = covering;
    let app = ShelfApp::new(fake);
    let BooksCheck::SchemaInvalid(problems) = check(&app) else {
        panic!("the space lists nothing");
    };
    assert!(
        problems.iter().any(
            |p| matches!(p, SchemaProblem::SpaceCannotList { space, .. } if *space == other_space())
        ),
        "{problems:?}"
    );
}

/// The declaration of the fake's books, one link to the space `file` that covers and
/// another that covers the same selection, and the second a book's second link by
/// the preset of the fake (`file`), which names the space: both are asked about once.
fn declared_with_covers() -> ShelfDecl {
    let mut covering = decls();
    link_of(&mut covering, "tests").cover = Some(table(None));
    let spec = covering.iter_mut().find(|b| b.name == "spec").unwrap();
    spec.links.push(LinkEntryDecl {
        name: "more".to_owned(),
        target: LinkTargetDecl::Artifact,
        cardinality: Cardinality::Many,
        propagation: Propagation::Informs,
        space: Some("file".to_owned()),
        compare: None,
        cover: Some(table(Some("warn"))),
    });
    ShelfDecl::of_books(covering)
}

#[test]
fn declared_schema_leaves_no_selection_unasked_and_asks_none_the_schema_does_not_read() {
    let fake = FakeShelf::new();
    fake.list_names(&file_space(), all());
    let decl = declared_with_covers();
    let asks = Schema::selection_asks(&decl, &fake.spaces(), &fake.presets());
    let schema = declared_schema(decl, &fake)
        .unwrap()
        .unwrap_or_else(|problems| panic!("a question left unanswered: {problems:?}"));
    // What the schema reads of the covers is exactly what was asked, once each.
    assert_eq!(asks, schema.cover_selections());
    assert_eq!(asks, BTreeSet::from([(file_space(), selection())]));
}

#[test]
fn a_declaration_whose_space_lists_nothing_is_refused_by_declared_schema_for_init_too() {
    let fake = FakeShelf::new();
    let decl = declared_with_covers();
    // The fake lists nothing for `file` here: every selection is answered "cannot
    // list", so the cover is refused and nothing is written.
    let refused = declared_schema(decl.clone(), &fake).unwrap().unwrap_err();
    assert!(
        refused
            .iter()
            .all(|p| matches!(p, SchemaProblem::SpaceCannotList { .. })),
        "{refused:?}"
    );
    fake.read_text_as("the declaration", decl);
    let mut app = ShelfApp::new(fake);
    let error = block_on(app.init(super::InitRequest {
        declaration: super::DeclarationText::new("the declaration"),
        files: Vec::new(),
        docs: Vec::new(),
    }))
    .unwrap_err();
    assert!(matches!(error, ShelfError::Schema(_)), "{error:?}");
    assert_eq!(app.ports().declaration_file(), None);
}
