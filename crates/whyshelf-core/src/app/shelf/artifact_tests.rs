//! Tests of the use cases of the shelf with files that are no document: the load
//! reads them, an approval records their addresses and the revision, and
//! `show_approved` and `diff` bring back what the approval saw.

use crate::domain::shelf::freshness::{Cause, Freshness, Standing};
use crate::domain::shelf::ledger::{DecisionVerb, PinnedArtifact, Record};
use crate::domain::shelf::lookup::Reference;
use crate::domain::shelf::snapshot::{ItemNow, ItemRole, Retrieval};

use super::fake_tests::{FakeShelf, block_on, docid, fake_address, fr, sp, sp_tested};
use super::{ErrorClass, JudgeRequest, Judgement, ShelfApp, ShelfError};

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

/// FR-1, and SP-2 on it, tested by `tests/a.rs` (there) and `tests/b.rs` (not yet).
fn project() -> ShelfApp<FakeShelf> {
    let fake = FakeShelf::new().with_files(vec![
        fr(1, "slow", &[]),
        sp_tested(2, "cache", &["FR-1"], &["tests/a.rs", "tests/b.rs"]),
    ]);
    fake.put_file("tests/a.rs", b"assert!(fast);\n");
    fake.put_file("specs/sp-2.md", b"---\nthe spec as a file\n");
    fake.put_file("findings/fr-1.md", b"---\nthe finding as a file\n");
    ShelfApp::new(fake)
}

fn approval_of(record: &Record) -> &DecisionVerb {
    match record {
        Record::Decision(d) => d.verb(),
        Record::Revocation(_) | Record::SameMeaning(_) | Record::Moved(_) | Record::Exempt(_) => {
            panic!("a decision")
        }
    }
}

#[test]
fn a_shelf_with_no_link_to_artifacts_asks_the_artifact_source_nothing() {
    let app = ShelfApp::new(FakeShelf::new().with_files(vec![fr(1, "slow", &[]), sp(2, "c", &[])]));
    block_on(app.status()).unwrap();
    assert_eq!(app.ports().artifact_read_count(), 0);
}

#[test]
fn an_approval_records_the_addresses_of_what_it_stood_on_and_the_revision() {
    let mut app = project();
    app.ports().commit("rev-1");
    block_on(app.judge_all(vec![approve("FR-1"), approve("SP-2")])).unwrap();
    let records = app.ports().records();
    let DecisionVerb::Approve {
        frame,
        artifacts,
        seen_address,
        revision,
        ..
    } = approval_of(&records[1])
    else {
        panic!("an approval");
    };
    assert_eq!(
        seen_address.as_ref(),
        Some(&fake_address(b"---\nthe spec as a file\n"))
    );
    assert_eq!(
        frame.iter().next().and_then(|p| p.address()),
        Some(&fake_address(b"---\nthe finding as a file\n"))
    );
    assert_eq!(revision.as_ref().map(|r| r.as_str()), Some("rev-1"));
    let seen: Vec<(&str, bool)> = artifacts
        .iter()
        .map(|p| {
            (
                p.name().as_str(),
                matches!(
                    p.seen(),
                    PinnedArtifact::Present {
                        address: Some(_),
                        ..
                    }
                ),
            )
        })
        .collect();
    assert_eq!(seen, [("tests/a.rs", true), ("tests/b.rs", false)]);
}

#[test]
fn outside_version_control_an_approval_keeps_no_address_and_holds_the_same() {
    let mut fake = FakeShelf::new().with_files(vec![
        fr(1, "slow", &[]),
        sp_tested(2, "cache", &["FR-1"], &["tests/a.rs"]),
    ]);
    fake.versioned = false;
    fake.put_file("tests/a.rs", b"one\n");
    let mut app = ShelfApp::new(fake);
    block_on(app.judge_all(vec![approve("FR-1"), approve("SP-2")])).unwrap();
    let records = app.ports().records();
    let DecisionVerb::Approve {
        seen_address,
        revision,
        ..
    } = approval_of(&records[1])
    else {
        panic!("an approval");
    };
    assert_eq!((seen_address, revision), (&None, &None));
    let row = block_on(app.get(&r("SP-2"))).unwrap().row;
    assert_eq!(row.standing, Standing::Approved(Freshness::Fresh));

    // The approved content cannot be read back, and says why.
    let shown = block_on(app.show_approved(&r("SP-2"))).unwrap();
    assert!(
        shown
            .items
            .iter()
            .all(|i| i.retrieval == Retrieval::NoAddress)
    );
}

#[test]
fn a_test_edited_after_the_approval_makes_it_stale_and_diff_shows_both_contents() {
    let mut app = project();
    app.ports().commit("rev-1");
    block_on(app.judge_all(vec![approve("FR-1"), approve("SP-2")])).unwrap();

    app.ports().put_file("tests/a.rs", b"assert!(faster);\n");
    let row = block_on(app.get(&r("SP-2"))).unwrap().row;
    let Standing::Approved(Freshness::Degraded(d)) = &row.standing else {
        panic!("the approval no longer holds: {:?}", row.standing);
    };
    assert!(
        d.causes()
            .iter()
            .any(|c| matches!(c, Cause::ArtifactChanged { .. }))
    );

    let diff = block_on(app.diff(&r("SP-2"))).unwrap();
    assert_eq!(diff.snapshot.subject(), docid(2));
    let a = diff
        .items
        .iter()
        .find(|i| matches!(i.item.role(), ItemRole::Artifact { artifact, .. } if artifact.name().as_str() =="tests/a.rs"))
        .unwrap();
    assert!(a.changed);
    assert_eq!(a.approved, Retrieval::Read(b"assert!(fast);\n".to_vec()));
    assert_eq!(a.current.as_deref(), Some(&b"assert!(faster);\n"[..]));

    let b = diff
        .items
        .iter()
        .find(|i| matches!(i.item.role(), ItemRole::Artifact { artifact, .. } if artifact.name().as_str() =="tests/b.rs"))
        .unwrap();
    assert!(!b.changed);
    assert_eq!(b.approved, Retrieval::NoAddress);
    assert_eq!(b.current, None);

    let subject = &diff.items[0];
    assert!(matches!(subject.item.role(), ItemRole::Subject { .. }));
    assert!(!subject.changed);
    assert!(matches!(subject.now, ItemNow::Doc(Some(_))));
    assert_eq!(app.ports().append_count(), 2);
}

#[test]
fn content_approved_and_never_committed_is_an_address_that_cannot_be_read() {
    let mut app = project();
    // Nothing committed: the addresses are named, and nothing is behind them.
    block_on(app.judge_all(vec![approve("FR-1"), approve("SP-2")])).unwrap();
    let shown = block_on(app.show_approved(&r("SP-2"))).unwrap();
    let a = shown
        .items
        .iter()
        .find(|i| matches!(i.item.role(), ItemRole::Artifact { artifact, .. } if artifact.name().as_str() =="tests/a.rs"))
        .unwrap();
    assert!(matches!(a.retrieval, Retrieval::AddressNotReadable(_)));
}

#[test]
fn a_test_that_arrives_and_one_that_goes_are_stale_and_broken() {
    let mut app = project();
    block_on(app.judge_all(vec![approve("FR-1"), approve("SP-2")])).unwrap();
    app.ports().put_file("tests/b.rs", b"new\n");
    app.ports().remove_file("tests/a.rs");
    let row = block_on(app.get(&r("SP-2"))).unwrap().row;
    let Standing::Approved(Freshness::Degraded(d)) = &row.standing else {
        panic!("degraded");
    };
    assert!(
        d.causes()
            .iter()
            .any(|c| matches!(c, Cause::ArtifactGone { .. }))
    );
    assert!(
        d.causes()
            .iter()
            .any(|c| matches!(c, Cause::ArtifactArrived { .. }))
    );
}

#[test]
fn what_no_approval_stands_about_has_nothing_to_show() {
    let app = project();
    let err = block_on(app.show_approved(&r("SP-2"))).unwrap_err();
    assert!(matches!(err, ShelfError::NotApproved { .. }));
    assert_eq!(err.class(), ErrorClass::Request);
    let err = block_on(app.diff(&r("SP-2"))).unwrap_err();
    assert!(matches!(err, ShelfError::NotApproved { .. }));
}

#[test]
fn show_approved_reads_back_the_upstream_as_it_was_pinned() {
    let mut app = project();
    app.ports().commit("rev-1");
    block_on(app.judge_all(vec![approve("FR-1"), approve("SP-2")])).unwrap();
    // The finding's file is rewritten after the approval; what was pinned comes back.
    app.ports()
        .put_file("findings/fr-1.md", b"---\nrewritten\n");
    let shown = block_on(app.show_approved(&r("SP-2"))).unwrap();
    let upstream = shown
        .items
        .iter()
        .find(|i| matches!(i.item.role(), ItemRole::Upstream { .. }))
        .unwrap();
    assert_eq!(
        upstream.retrieval,
        Retrieval::Read(b"---\nthe finding as a file\n".to_vec())
    );
    assert_eq!(shown.snapshot.revision().map(|r| r.as_str()), Some("rev-1"));
}
