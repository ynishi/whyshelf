//! Tests of the use cases with spaces: a declaration held against the spaces the
//! artifact source offers, a name its adapter says is none, a name it cannot tell
//! about now, and two spaces in one shelf.

use crate::domain::shelf::build::DocProblem;
use crate::domain::shelf::freshness::{Freshness, Notice, Standing};
use crate::domain::shelf::ledger::{DecisionVerb, FrameRefusal, Record};
use crate::domain::shelf::lookup::Reference;
use crate::domain::shelf::schema::{
    Cardinality, LinkEntryDecl, LinkTargetDecl, Propagation, SchemaProblem,
};

use super::fake_tests::{
    FakeShelf, artifact, block_on, file, file_space, fr, other_space, sp_tested,
};
use super::{BooksCheck, ErrorClass, JudgeRequest, Judgement, ShelfApp, ShelfError};

fn r(text: &str) -> Reference {
    text.parse().unwrap()
}

fn approve(reference: &str) -> JudgeRequest {
    JudgeRequest {
        judgement: Judgement::Approve {
            until: None,
            reason: None,
        },
        reference: r(reference),
        actor: None,
    }
}

/// FR-1, and SP-2 on it, tested by `tests/a.rs`, which is there.
fn project() -> FakeShelf {
    let fake = FakeShelf::new().with_files(vec![
        fr(1, "slow", &[]),
        sp_tested(2, "cache", &["FR-1"], &["tests/a.rs"]),
    ]);
    fake.put_file("tests/a.rs", b"assert!(fast);\n");
    fake
}

/// A link `code` of the spec, in the space `space` (none when `None`).
fn code_link(space: Option<&str>) -> LinkEntryDecl {
    LinkEntryDecl {
        name: "code".to_owned(),
        target: LinkTargetDecl::Artifact,
        cardinality: Cardinality::Many,
        propagation: Propagation::Depends,
        space: space.map(str::to_owned),
        compare: None,
        cover: None,
    }
}

fn schema_problems(fake: FakeShelf) -> Vec<SchemaProblem> {
    let app = ShelfApp::new(fake);
    match block_on(app.books_check()).unwrap() {
        BooksCheck::SchemaInvalid(problems) => problems.iter().cloned().collect(),
        BooksCheck::Checked { .. } => panic!("the declaration is refused"),
    }
}

#[test]
fn a_space_the_source_does_not_offer_and_a_link_with_no_space_are_refused() {
    let fake = FakeShelf::new();
    fake.decls.lock().unwrap()[1]
        .links
        .push(code_link(Some("rust-item")));
    let problems = schema_problems(fake);
    assert!(matches!(
        problems.as_slice(),
        [SchemaProblem::UnknownSpace { link, space, offered, .. }]
            if link.as_str() == "code" && space.as_str() == "rust-item"
                && offered == "`file`, `other`"
    ));

    let fake = FakeShelf::new();
    fake.decls.lock().unwrap()[1].links.push(code_link(None));
    let problems = schema_problems(fake);
    assert!(matches!(
        problems.as_slice(),
        [SchemaProblem::NoSpace { link, .. }] if link.as_str() == "code"
    ));

    // The same declaration is sound once the source offers the space.
    let fake = FakeShelf::new();
    fake.decls.lock().unwrap()[1]
        .links
        .push(code_link(Some("rust-item")));
    fake.spaces
        .lock()
        .unwrap()
        .insert(crate::domain::shelf::artifact::Space::new("rust-item").unwrap());
    let app = ShelfApp::new(fake);
    assert!(matches!(
        block_on(app.books_check()).unwrap(),
        BooksCheck::Checked { .. }
    ));
}

#[test]
fn a_text_the_adapter_says_is_no_name_makes_the_document_invalid() {
    let fake = project();
    fake.not_a_name(&file_space(), "tests/a.rs", "not in this tree");
    let app = ShelfApp::new(fake);
    let health = block_on(app.status()).unwrap().health;
    let problems: Vec<&DocProblem> = health
        .invalid
        .iter()
        .flat_map(|d| d.problems().iter())
        .collect();
    assert!(
        problems.iter().any(|p| matches!(
            p,
            DocProblem::NotAnArtifactName { link, space, written, why }
                if link.as_str() == "tests" && *space == file_space()
                    && written == "tests/a.rs" && why.as_str() == "not in this tree"
        )),
        "{problems:?}"
    );
}

#[test]
fn a_new_document_whose_name_the_adapter_says_is_none_is_refused_and_not_written() {
    let fake = FakeShelf::new().with_files(vec![fr(1, "slow", &[])]);
    let mut app = ShelfApp::new(fake);
    let request = super::NewDocRequest {
        book: crate::domain::shelf::schema::BookName::new("spec").unwrap(),
        key: None,
        entries: vec![
            ("title".to_owned(), "cache".to_owned()),
            ("grounds".to_owned(), "FR-1".to_owned()),
            ("tests".to_owned(), "../outside.rs".to_owned()),
        ],
        items: Vec::new(),
        body: "Body.\n".to_owned(),
    };
    let error = block_on(app.new_doc(request)).unwrap_err();
    let ShelfError::NewDoc(crate::domain::shelf::build::NewDocProblem::Invalid(problems)) = &error
    else {
        panic!("refused as invalid: {error:?}");
    };
    assert!(problems.iter().any(
        |p| matches!(p, DocProblem::NotAnArtifactName { written, .. } if written == "../outside.rs")
    ));
    assert_eq!(app.ports().created_count(), 0);
}

#[test]
fn an_approval_on_a_name_that_cannot_be_told_is_refused_and_writes_nothing() {
    let fake = project();
    fake.cannot_tell(&file_space(), "tests/a.rs", "the tree is not mounted");
    let mut app = ShelfApp::new(fake);
    let error = block_on(app.judge_all(vec![approve("FR-1"), approve("SP-2")])).unwrap_err();
    let ShelfError::Refused(FrameRefusal::ArtifactUnknown {
        link,
        artifact: named,
        why,
    }) = &error
    else {
        panic!("refused for the name that cannot be told: {error:?}");
    };
    assert_eq!(link.as_str(), "tests");
    assert_eq!(*named, artifact(&file_space(), "tests/a.rs"));
    assert_eq!(why.as_str(), "the tree is not mounted");
    assert_eq!(error.class(), ErrorClass::Refused);
    // The approval of FR-1 before it in the run is not written either.
    assert_eq!(app.ports().append_count(), 0);
}

#[test]
fn a_pinned_name_that_later_cannot_be_told_is_a_notice_and_the_approval_stays_fresh() {
    let fake = project();
    let mut app = ShelfApp::new(fake);
    block_on(app.judge_all(vec![approve("FR-1"), approve("SP-2")])).unwrap();

    // Edited while it cannot be told: the edit is not seen, and is no cause.
    app.ports().put_file("tests/a.rs", b"assert!(faster);\n");
    app.ports()
        .cannot_tell(&file_space(), "tests/a.rs", "the tree is not mounted");
    let row = block_on(app.get(&r("SP-2"))).unwrap().row;
    assert_eq!(row.standing, Standing::Approved(Freshness::Fresh));
    assert!(row.notices.iter().any(|n| matches!(
        n,
        Notice::ArtifactUnknown { link, artifact: named, why }
            if link.as_str() == "tests" && *named == artifact(&file_space(), "tests/a.rs")
                && why.as_str() == "the tree is not mounted"
    )));
    // `check` lists it, and stays clean.
    let check = block_on(app.books_check()).unwrap();
    assert!(check.is_clean());
    let BooksCheck::Checked { unknown, .. } = &check else {
        panic!("checked");
    };
    assert_eq!(unknown.len(), 1);
    assert_eq!(unknown[0].artifact, artifact(&file_space(), "tests/a.rs"));

    // Told again, the edit is a change.
    app.ports().can_tell(&file_space(), "tests/a.rs");
    let row = block_on(app.get(&r("SP-2"))).unwrap().row;
    assert!(matches!(
        row.standing,
        Standing::Approved(Freshness::Degraded(_))
    ));
}

#[test]
fn the_same_name_in_two_spaces_is_two_artifacts() {
    let fake = FakeShelf::new().with_files(vec![
        fr(1, "slow", &[]),
        file(
            "specs/sp-2.md",
            2,
            "SP-2",
            "cache",
            &[("grounds", &["FR-1"]), ("tests", &["a"]), ("code", &["a"])],
        ),
    ]);
    fake.decls.lock().unwrap()[1]
        .links
        .push(code_link(Some("other")));
    fake.put_file("a", b"in the files\n");
    fake.put_in(&other_space(), "a", b"elsewhere\n");
    let mut app = ShelfApp::new(fake);

    let doc = block_on(app.get(&r("SP-2"))).unwrap().doc.unwrap();
    let tests = &doc.artifact_links()["tests"].artifacts()[0];
    let code = &doc.artifact_links()["code"].artifacts()[0];
    assert_eq!(*tests.key(), artifact(&file_space(), "a"));
    assert_eq!(*code.key(), artifact(&other_space(), "a"));
    assert_ne!(tests.state(), code.state());

    block_on(app.judge_all(vec![approve("FR-1"), approve("SP-2")])).unwrap();
    let records = app.ports().records();
    let Record::Decision(decision) = &records[1] else {
        panic!("a decision");
    };
    let DecisionVerb::Approve { artifacts, .. } = decision.verb() else {
        panic!("an approval");
    };
    let pinned: Vec<_> = artifacts.iter().map(|p| p.artifact().clone()).collect();
    assert_eq!(
        pinned,
        [artifact(&other_space(), "a"), artifact(&file_space(), "a")]
    );

    // An edit in one space is a change of that artifact only.
    app.ports()
        .put_in(&other_space(), "a", b"edited elsewhere\n");
    let row = block_on(app.get(&r("SP-2"))).unwrap().row;
    let Standing::Approved(Freshness::Degraded(degradation)) = &row.standing else {
        panic!("degraded: {:?}", row.standing);
    };
    let changed: Vec<_> = degradation
        .causes()
        .iter()
        .filter_map(|c| match c {
            crate::domain::shelf::freshness::Cause::ArtifactChanged { pin, .. } => {
                Some(pin.artifact().clone())
            }
            _ => None,
        })
        .collect();
    assert_eq!(changed, [artifact(&other_space(), "a")]);
}

#[test]
fn one_address_from_two_spaces_is_read_back_by_each_space_s_adapter() {
    use super::ContentOf;
    use super::fake_tests::fake_address;
    use crate::domain::shelf::snapshot::{ItemRole, Retrieval};

    // The same bytes in both spaces: the fake names them by one address.
    let same = b"one content\n";
    let fake = FakeShelf::new().with_files(vec![
        fr(1, "slow", &[]),
        file(
            "specs/sp-2.md",
            2,
            "SP-2",
            "cache",
            &[("grounds", &["FR-1"]), ("tests", &["a"]), ("code", &["b"])],
        ),
    ]);
    fake.decls.lock().unwrap()[1]
        .links
        .push(code_link(Some("other")));
    fake.put_file("a", same);
    fake.put_in(&other_space(), "b", same);
    let address = fake_address(same);
    // Each adapter keeps something of its own at that address.
    for (space, kept) in [
        (file_space(), "kept by file"),
        (other_space(), "kept by other"),
    ] {
        fake.kept_by.lock().unwrap().insert(
            (ContentOf::Artifact(space), address.clone()),
            kept.as_bytes().to_vec(),
        );
    }
    let mut app = ShelfApp::new(fake);
    block_on(app.judge_all(vec![approve("FR-1"), approve("SP-2")])).unwrap();

    let shown = block_on(app.show_approved(&r("SP-2"))).unwrap();
    let read: Vec<(String, Retrieval)> = shown
        .items
        .iter()
        .filter_map(|i| match i.item.role() {
            ItemRole::Artifact { artifact, .. } => {
                assert_eq!(i.item.address(), Some(&address));
                Some((artifact.space().to_string(), i.retrieval.clone()))
            }
            _ => None,
        })
        .collect();
    assert_eq!(
        read,
        [
            (
                "other".to_owned(),
                Retrieval::Read(b"kept by other".to_vec())
            ),
            ("file".to_owned(), Retrieval::Read(b"kept by file".to_vec())),
        ]
    );
    let diff = block_on(app.diff(&r("SP-2"))).unwrap();
    let approved: Vec<&Retrieval> = diff
        .items
        .iter()
        .filter(|i| matches!(i.item.role(), ItemRole::Artifact { .. }))
        .map(|i| &i.approved)
        .collect();
    assert_eq!(
        approved,
        [
            &Retrieval::Read(b"kept by other".to_vec()),
            &Retrieval::Read(b"kept by file".to_vec()),
        ]
    );
}
