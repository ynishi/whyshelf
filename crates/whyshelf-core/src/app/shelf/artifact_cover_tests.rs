//! Tests of a cover over artifacts in the use cases, over the in-memory fake: what
//! `check` lists and when it is not clean, the one listing two links with one
//! selection share, `cover` and the exemption of an artifact, and that the one way a
//! declaration becomes a schema leaves no selection unasked, for `load` and for
//! `init`.

use std::collections::BTreeSet;

use crate::domain::nonempty::NonEmptyVec;
use crate::domain::shelf::artifact::{
    ArtifactName, Completeness, ListingPart, NameListing, PartRead, Selection, Space,
};
use crate::domain::shelf::coverage::{
    Assessed, CoverAssessment, CoverProblem, CoverTarget, LinkCover, NameCover, PartCover,
};
use crate::domain::shelf::exemption::{CoverMiss, ExemptRefusal, ExemptionReading, Unneeded};
use crate::domain::shelf::schema::{
    BookName, BookSchemaDecl, Cardinality, CoverDecl, CoverSeverity, EntryName, LinkEntryDecl,
    LinkTargetDecl, PartsDecl, Propagation, Schema, SchemaProblem, ShelfDecl,
};

use super::fake_tests::{
    FakeShelf, aname, artifact, block_on, decls, file, file_space, other_space, reason, sp_tested,
};
use super::load::declared_schema;
use super::ports::ArtifactSource;
use super::{
    ArtifactCoverLists, BooksCheck, CoverLinkAsk, CoverReport, ErrorClass, ExemptAsk,
    ExemptRequest, ShelfApp, ShelfError, UncoveredOf,
};

fn table(strength: Option<&str>) -> CoverDecl {
    CoverDecl::Table {
        strength: strength.map(str::to_owned),
        include: vec!["src/**".to_owned()],
        exclude: Vec::new(),
        parts: None,
    }
}

/// The fake's books with a link `cases` of the spec, to the space `other`, covering
/// `src/**` with `strength`, taking each part whole through `tests` (to the files)
/// when `whole`; beside them `seen`, another link of the spec to the files that
/// depends on them. `SP-3` writes `links`, and `other` lists `listing`.
fn app_parts(
    strength: Option<&str>,
    whole: bool,
    links: &[(&str, &[&str])],
    listing: NameListing,
) -> ShelfApp<FakeShelf> {
    let mut covering = decls();
    let tests = link_of(&mut covering, "tests").clone();
    let mut cases = tests.clone();
    cases.name = "cases".to_owned();
    cases.space = Some("other".to_owned());
    cases.cover = Some(CoverDecl::Table {
        strength: strength.map(str::to_owned),
        include: vec!["src/**".to_owned()],
        exclude: Vec::new(),
        parts: whole.then(|| PartsDecl {
            whole: "tests".to_owned(),
        }),
    });
    let mut seen = tests;
    seen.name = "seen".to_owned();
    let spec = covering.iter_mut().find(|b| b.name == "spec").unwrap();
    spec.links.extend([cases, seen]);
    let fake = FakeShelf::new().with_files(vec![file("specs/sp-3.md", 3, "SP-3", "t", links)]);
    *fake.decls.lock().unwrap() = covering;
    fake.list_names(&other_space(), listing);
    ShelfApp::new(fake)
}

/// A listing of `other` in parts, each part a file: `a.rs` with a test found in it,
/// `b.rs` that could not be read, and `c.rs` read with no test in it.
fn in_parts(space_of_parts: &Space) -> NameListing {
    let part = |path: &str, read: PartRead| {
        ListingPart::new(artifact(space_of_parts, path), reason("a macro"), read)
    };
    NameListing::Parted {
        found: BTreeSet::from([aname("src/z.rs::t")]),
        parts: NonEmptyVec::new(
            part("a.rs", PartRead::Found(BTreeSet::from([aname("a.rs::t")]))),
            vec![
                part("b.rs", PartRead::Unread),
                part("c.rs", PartRead::Found(BTreeSet::new())),
            ],
        ),
    }
}

/// The covering link `cases` of a check.
fn cases(check: &BooksCheck) -> &LinkCover {
    parts(check)
        .1
        .links()
        .iter()
        .find(|l| l.link().as_str() == "cases")
        .expect("the link covers")
}

/// The names of a link and how each stands, when it is assessed whole.
fn names_of(link: &LinkCover) -> &[(ArtifactName, NameCover)] {
    match link.assessed() {
        Assessed::Whole { names, .. } => names,
        Assessed::NotWhole | Assessed::Unsound(_) => &[],
    }
}

/// How each part of a link stands, by the name of the part, when it is assessed whole.
fn parts_of(link: &LinkCover) -> Vec<(&str, PartCover)> {
    match link.assessed() {
        Assessed::Whole { parts, .. } => parts
            .iter()
            .map(|(p, s)| (p.key().name().as_str(), *s))
            .collect(),
        Assessed::NotWhole | Assessed::Unsound(_) => Vec::new(),
    }
}

#[test]
fn a_part_is_reached_only_through_the_link_of_parts_and_any_other_is_a_gap() {
    // `tests` names `a.rs`; `seen` names `b.rs`, which is no link of parts; the name
    // listed, `src/z.rs::t`, nobody names; `c.rs` was read with no test found in it.
    let app = app_parts(
        None,
        true,
        &[("tests", &["a.rs"]), ("seen", &["b.rs"])],
        in_parts(&file_space()),
    );
    let checked = check(&app);
    let link = cases(&checked);
    assert!(link.is_whole(), "{link:?}");
    assert!(matches!(link.completeness(), Completeness::Partial(_)));
    assert_eq!(link.parts_whole().map(EntryName::as_str), Some("tests"));
    // A part read with no name found in it is a gap too: what keeps it from being
    // listed whole may make one, so whether it holds one cannot be told.
    assert_eq!(
        parts_of(link),
        [
            ("a.rs", PartCover::Reached),
            ("b.rs", PartCover::Gap),
            ("c.rs", PartCover::Gap),
        ]
    );
    assert_eq!(names_of(link), [(aname("src/z.rs::t"), NameCover::Gap)]);
    // Each part that is a gap is a gap of the covering link, told as a part through
    // the link of parts, and fails the check.
    let (lists, cover) = parts(&checked);
    assert!(
        cover
            .gap_of(
                &CoverTarget::Part(artifact(&file_space(), "c.rs")),
                &BookName::new("spec").unwrap(),
                &EntryName::new("cases").unwrap(),
            )
            .is_some()
    );
    let part = UncoveredOf::Part {
        whole: EntryName::new("tests").unwrap(),
    };
    let uncovered: Vec<(String, UncoveredOf)> = lists
        .uncovered
        .iter()
        .map(|u| (format!("{}", u.artifact), u.of.clone()))
        .collect();
    assert_eq!(
        uncovered,
        [
            ("`src/z.rs::t` in `other`".to_owned(), UncoveredOf::Name),
            ("`b.rs` in `file`".to_owned(), part.clone()),
            ("`c.rs` in `file`".to_owned(), part),
        ]
    );
    assert!(!checked.is_clean());

    // `tests` names every part, and a document the name listed: each reached.
    let app = app_parts(
        None,
        true,
        &[
            ("tests", &["a.rs", "b.rs", "c.rs"]),
            ("cases", &["src/z.rs::t"]),
        ],
        in_parts(&file_space()),
    );
    let checked = check(&app);
    assert_eq!(
        parts_of(cases(&checked))
            .into_iter()
            .map(|(_, s)| s)
            .collect::<Vec<_>>(),
        [PartCover::Reached, PartCover::Reached, PartCover::Reached]
    );
    assert!(parts(&checked).0.uncovered.is_empty());
    assert!(checked.is_clean(), "{checked:?}");
}

#[test]
fn a_part_is_exempt_by_its_name_until_the_link_of_parts_reaches_it_or_it_is_no_part() {
    let mut app = app_parts(
        None,
        true,
        &[("tests", &["a.rs"]), ("cases", &["src/z.rs::t"])],
        in_parts(&file_space()),
    );
    assert!(!check(&app).is_clean());
    let request = |name: &str| ExemptRequest {
        link: EntryName::new("cases").unwrap(),
        ..exempt_request(name)
    };
    // Each part is exempt in the space it is in, `c.rs` (read, no test found) too;
    // a part reached already is refused.
    assert!(block_on(app.exempt(vec![request("a.rs")])).is_err());
    let done = block_on(app.exempt(vec![request("b.rs"), request("c.rs")])).unwrap();
    assert_eq!(done[0].artifact(), Some(&artifact(&file_space(), "b.rs")));
    assert_eq!(done[1].artifact(), Some(&artifact(&file_space(), "c.rs")));
    let checked = check(&app);
    assert!(matches!(
        &parts_of(cases(&checked))[1..],
        [
            ("b.rs", PartCover::Exempt(_)),
            ("c.rs", PartCover::Exempt(_))
        ]
    ));
    assert!(checked.is_clean(), "{checked:?}");

    // A document names `b.rs` under the link of parts: its exemption is unneeded,
    // reached from that document.
    *app.ports().readings.lock().unwrap() = vec![file(
        "specs/sp-3.md",
        3,
        "SP-3",
        "t",
        &[("tests", &["a.rs", "b.rs"]), ("cases", &["src/z.rs::t"])],
    )];
    let checked = check(&app);
    let (lists, _) = parts(&checked);
    assert_eq!(lists.unneeded.len(), 1, "{lists:?}");
    assert_eq!(lists.unneeded[0].artifact, artifact(&file_space(), "b.rs"));
    assert!(matches!(
        &lists.unneeded[0].why,
        Unneeded::Reached { by } if by.len() == 1
    ));
    // `c.rs` is no part of the listing now (listed whole, by name): its exemption is
    // of nothing the listing holds.
    app.ports().list_names(
        &other_space(),
        NameListing::Parted {
            found: BTreeSet::from([aname("src/z.rs::t"), aname("c.rs::t")]),
            parts: NonEmptyVec::new(
                ListingPart::new(
                    artifact(&file_space(), "a.rs"),
                    reason("a macro"),
                    PartRead::Unread,
                ),
                vec![ListingPart::new(
                    artifact(&file_space(), "b.rs"),
                    reason("a macro"),
                    PartRead::Unread,
                )],
            ),
        },
    );
    let checked = check(&app);
    let (lists, _) = parts(&checked);
    let c = lists
        .unneeded
        .iter()
        .find(|u| u.artifact == artifact(&file_space(), "c.rs"))
        .expect("the exemption of c.rs is unneeded");
    assert!(matches!(&c.why, Unneeded::Cover(CoverMiss::NotListed)));
}

#[test]
fn an_exemption_of_a_part_stands_only_while_no_name_is_found_in_it() {
    let mut app = app_parts(
        None,
        true,
        &[("cases", &["src/z.rs::t"])],
        in_parts(&file_space()),
    );
    let request = |name: &str| ExemptRequest {
        link: EntryName::new("cases").unwrap(),
        ..exempt_request(name)
    };
    // `a.rs` holds a test: an exemption of it would not stand, and is not made.
    let error = block_on(app.exempt(vec![request("a.rs")])).unwrap_err();
    assert!(
        matches!(
            &error,
            ShelfError::ExemptRefused(ExemptRefusal::NamesFound { names, .. })
                if names.iter().map(ArtifactName::as_str).collect::<Vec<_>>() == ["a.rs::t"]
        ),
        "{error:?}"
    );
    // `c.rs`, read with no test found in it, is exempt; the check passes once `a.rs`
    // and `b.rs` are named under the link of parts.
    block_on(app.exempt(vec![request("c.rs")])).unwrap();
    *app.ports().readings.lock().unwrap() = vec![file(
        "specs/sp-3.md",
        3,
        "SP-3",
        "t",
        &[("tests", &["a.rs", "b.rs"]), ("cases", &["src/z.rs::t"])],
    )];
    assert!(check(&app).is_clean());

    // A test is found in `c.rs` now: the exemption no longer clears it, the part is
    // a gap, and the exemption is told with the test.
    let with_test = |space_of_parts: &Space| {
        let part = |path: &str, read: PartRead| {
            ListingPart::new(artifact(space_of_parts, path), reason("a macro"), read)
        };
        NameListing::Parted {
            found: BTreeSet::from([aname("src/z.rs::t")]),
            parts: NonEmptyVec::new(
                part("a.rs", PartRead::Found(BTreeSet::from([aname("a.rs::t")]))),
                vec![
                    part("b.rs", PartRead::Unread),
                    part(
                        "c.rs",
                        PartRead::Found(BTreeSet::from([aname("c.rs::new")])),
                    ),
                ],
            ),
        }
    };
    app.ports()
        .list_names(&other_space(), with_test(&file_space()));
    let checked = check(&app);
    assert!(!checked.is_clean());
    assert_eq!(parts_of(cases(&checked))[2], ("c.rs", PartCover::Gap));
    let (lists, _) = parts(&checked);
    assert!(lists.exempt.is_empty(), "{lists:?}");
    assert!(matches!(
        &lists.unneeded[..],
        [u] if u.artifact == artifact(&file_space(), "c.rs")
            && matches!(&u.why, Unneeded::NamesFound { names }
                if names.iter().map(ArtifactName::as_str).collect::<Vec<_>>() == ["c.rs::new"])
    ));

    // Nothing of a link is assessed while a part is in another space: no exemption
    // from it can be told needed or not.
    app.ports()
        .list_names(&other_space(), in_parts(&other_space()));
    let checked = check(&app);
    let (lists, cover) = parts(&checked);
    assert!(
        lists.exempt.is_empty() && lists.unneeded.is_empty(),
        "{lists:?}"
    );
    assert_eq!(cover.exemptions().count(), 1);
    assert!(cover.exemptions().all(|group| {
        group
            .reads()
            .iter()
            .all(|read| matches!(read.reading(), ExemptionReading::CannotTell(_)))
    }));
}

#[test]
fn a_cover_with_no_parts_reads_a_listing_in_parts_as_not_the_whole() {
    let failing = check(&app_parts(
        None,
        false,
        &[("tests", &["a.rs", "b.rs"])],
        in_parts(&file_space()),
    ));
    let link = cases(&failing);
    assert!(!link.is_whole());
    assert_eq!(link.assessed(), &Assessed::NotWhole);
    assert!(parts(&failing).0.uncovered.is_empty());
    assert!(!failing.is_clean());
    // A `warn` one is told, and leaves the check clean, as for any listing not whole.
    assert!(
        check(&app_parts(
            Some("warn"),
            false,
            &[("tests", &["a.rs", "b.rs"])],
            in_parts(&file_space())
        ))
        .is_clean()
    );
}

#[test]
fn a_part_in_another_space_than_the_link_of_parts_is_a_problem_whatever_the_strength() {
    for strength in [None, Some("warn")] {
        let checked = check(&app_parts(
            strength,
            true,
            &[("tests", &["a.rs", "b.rs", "c.rs"])],
            in_parts(&other_space()),
        ));
        let link = cases(&checked);
        let Assessed::Unsound(problems) = link.assessed() else {
            panic!("a part of another space is a problem: {link:?}");
        };
        assert_eq!(problems.len(), 3, "{link:?}");
        assert!(matches!(
            problems.first(),
            CoverProblem::PartOfAnotherSpace { part, whole, space }
                if part == &artifact(&other_space(), "a.rs")
                    && whole.as_str() == "tests"
                    && space == &file_space()
        ));
        // Nothing of the link is assessed: it is not whole, and tells no gap.
        assert!(!link.is_whole());
        assert!(parts(&checked).0.uncovered.is_empty());
        assert!(!checked.is_clean(), "{strength:?}");
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
        names_of(&cover.links()[0]),
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
        assert_eq!(cover.links()[0].assessed(), &Assessed::NotWhole);

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
        names_of(&links[0])[1].1,
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
