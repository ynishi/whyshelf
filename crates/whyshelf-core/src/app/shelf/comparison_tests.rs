//! Tests of the use cases when a link to artifacts compares lines: what the load
//! asks the artifact source, and what `diff` compares.

use std::collections::BTreeSet;

use crate::domain::shelf::artifact::{Comparison, LinePrefix};
use crate::domain::shelf::freshness::{Cause, Freshness, Standing};
use crate::domain::shelf::lookup::Reference;
use crate::domain::shelf::schema::CompareDecl;
use crate::domain::shelf::snapshot::ItemRole;

use crate::domain::nonempty::NonBlank;
use crate::domain::shelf::ledger::SameMeaningRefusal;

use super::fake_tests::{FakeShelf, aname, block_on, file_space, fr, sp_tested};
use super::{
    ErrorClass, JudgeRequest, Judgement, SameMeaningAbout, SameMeaningRequest, ShelfApp, ShelfError,
};

fn r(text: &str) -> Reference {
    text.parse().unwrap()
}

fn approve(reference: &str) -> JudgeRequest {
    JudgeRequest {
        judgement: Judgement::Approve {
            until: None,
            reason: None,
            again: false,
        },
        reference: r(reference),
        actor: None,
    }
}

fn slashes() -> Comparison {
    Comparison::Lines {
        ignore: BTreeSet::from([LinePrefix::new("//").unwrap()]),
    }
}

/// Make the spec's `tests` link compare as `compare` says.
fn declare(app: &ShelfApp<FakeShelf>, compare: Option<CompareDecl>) {
    let mut decls = app.ports().decls.lock().unwrap();
    let spec = decls.iter_mut().find(|b| b.name == "spec").unwrap();
    spec.links
        .iter_mut()
        .find(|l| l.name == "tests")
        .unwrap()
        .compare = compare;
}

fn lines_without_slashes() -> Option<CompareDecl> {
    Some(CompareDecl {
        kind: "lines".to_owned(),
        ignore: vec!["//".to_owned()],
    })
}

/// FR-1, and SP-2 on it tested by `tests/a.rs`, compared by lines without `//`, both
/// approved.
fn approved() -> ShelfApp<FakeShelf> {
    let fake = FakeShelf::new().with_files(vec![
        fr(1, "slow", &[]),
        sp_tested(2, "cache", &["FR-1"], &["tests/a.rs"]),
    ]);
    fake.put_file("tests/a.rs", b"assert!(fast);\n");
    let mut app = ShelfApp::new(fake);
    declare(&app, lines_without_slashes());
    block_on(app.judge_all(vec![approve("FR-1"), approve("SP-2")])).unwrap();
    app
}

/// Whether `diff` of SP-2 says `tests/a.rs` changed.
fn file_changed(app: &ShelfApp<FakeShelf>) -> bool {
    let diff = block_on(app.diff(&r("SP-2"))).unwrap();
    diff.items
        .iter()
        .find(|i| matches!(i.item.role(), ItemRole::Artifact { artifact, .. } if artifact.name().as_str() == "tests/a.rs"))
        .unwrap()
        .changed
}

#[test]
fn a_hash_pinned_under_another_comparison_is_a_request_error_and_writes_nothing() {
    let mut app = approved();
    declare(&app, None);
    app.ports().put_file("tests/a.rs", b"assert!(slow);\n");
    let pinned = slashes().fingerprint(b"assert!(fast);\n");
    let err = block_on(app.same_meaning(vec![SameMeaningRequest {
        about: SameMeaningAbout::Artifact(aname("tests/a.rs")),
        reason: NonBlank::new("renamed").unwrap(),
        actor: None,
        from: Some(*pinned.hash()),
    }]))
    .unwrap_err();
    assert!(
        matches!(
            err,
            ShelfError::SameMeaningRefused(SameMeaningRefusal::PinnedUnderOtherComparison { .. })
        ),
        "{err:?}"
    );
    assert_eq!(err.class(), ErrorClass::Request);
    assert_eq!(app.ports().append_count(), 2);
}

#[test]
fn the_load_asks_each_path_under_the_comparison_of_its_link() {
    let app = approved();
    let before = app.ports().asked().len();
    block_on(app.status()).unwrap();
    let asked = app.ports().asked();
    assert_eq!(asked.len(), before + 1);
    assert_eq!(
        asked
            .last()
            .unwrap()
            .get(&file_space(), &aname("tests/a.rs")),
        Some(&BTreeSet::from([slashes()]))
    );
}

#[test]
fn a_comment_added_keeps_the_approval_fresh_and_diff_says_unchanged() {
    let app = approved();
    app.ports().put_file(
        "tests/a.rs",
        b"// why it is fast\n    assert!(fast);   \n\n",
    );
    let row = block_on(app.get(&r("SP-2"))).unwrap().row;
    assert_eq!(row.standing, Standing::Approved(Freshness::Fresh));
    assert!(!file_changed(&app));

    app.ports().put_file("tests/a.rs", b"assert!(slow);\n");
    assert!(file_changed(&app));
}

#[test]
fn diff_compares_by_the_pin_s_comparison_after_the_declaration_changed() {
    let app = approved();
    let ask_count = app.ports().asked().len();
    // The link now compares bytes; the approval pinned lines.
    declare(&app, None);
    app.ports()
        .put_file("tests/a.rs", b"// why it is fast\nassert!(fast);\n");
    let row = block_on(app.get(&r("SP-2"))).unwrap().row;
    let Standing::Approved(Freshness::Degraded(d)) = &row.standing else {
        panic!("the comparison changed: {:?}", row.standing);
    };
    assert!(
        d.causes()
            .iter()
            .any(|c| matches!(c, Cause::ArtifactComparisonChanged { .. }))
    );
    // `diff` reads the file as the pin's hash was made: only a comment was added.
    assert!(!file_changed(&app));
    let asked = app.ports().asked();
    assert!(
        asked[ask_count..].iter().any(|a| a
            .get(&file_space(), &aname("tests/a.rs"))
            .is_some_and(|c| c.contains(&slashes()))),
        "{asked:?}"
    );
}
