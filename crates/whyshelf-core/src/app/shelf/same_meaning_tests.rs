//! Tests of `same_meaning` and `withdraw` over the in-memory fake: what is written,
//! in what order, and that a refusal writes nothing.

use crate::domain::actor::Actor;
use crate::domain::canon::Canon;
use crate::domain::nonempty::NonBlank;
use crate::domain::shelf::freshness::{Cause, Freshness, Notice, Standing};
use crate::domain::shelf::ledger::{
    JudgementId, Record, SameMeaningOf, SameMeaningRefusal, WithdrawRefusal,
};
use crate::domain::shelf::lookup::Reference;

use super::fake_tests::{
    FakeShelf, aname, artifact, block_on, docid, eid, file_space, fr, sp, sp_tested,
};
use super::{
    DeclaredAbout, ErrorClass, JudgeRequest, Judgement, SameMeaningAbout, SameMeaningRequest,
    ShelfApp, ShelfError, WithdrawRequest,
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

fn same(reference: &str) -> SameMeaningRequest {
    SameMeaningRequest {
        about: SameMeaningAbout::Doc(r(reference)),
        reason: NonBlank::new("a typo fixed").unwrap(),
        actor: None,
        from: None,
    }
}

fn withdraw(records: Vec<JudgementId>) -> WithdrawRequest {
    WithdrawRequest {
        records,
        reason: NonBlank::new("not the same after all").unwrap(),
        actor: None,
    }
}

/// FR-1 and SP-2 on it, both approved.
fn approved() -> ShelfApp<FakeShelf> {
    let mut app = ShelfApp::new(
        FakeShelf::new().with_files(vec![fr(1, "slow", &[]), sp(2, "cache", &["FR-1"])]),
    );
    block_on(app.judge_all(vec![approve("FR-1"), approve("SP-2")])).unwrap();
    app
}

fn standing(app: &ShelfApp<FakeShelf>, reference: &str) -> Standing {
    block_on(app.get(&r(reference))).unwrap().row.standing
}

fn is_stale(standing: &Standing) -> bool {
    matches!(standing, Standing::Approved(Freshness::Degraded(_)))
}

#[test]
fn a_declared_edit_is_written_and_keeps_the_approvals_fresh() {
    let mut app = approved();
    app.ports().edit(fr(1, "slower", &[]), 1);
    assert!(is_stale(&standing(&app, "SP-2")));

    let declared = block_on(app.same_meaning(vec![same("FR-1")])).unwrap();
    assert_eq!(declared.len(), 1);
    let DeclaredAbout::Doc { id, key } = &declared[0].about else {
        panic!("about a document: {:?}", declared[0].about);
    };
    assert_eq!(*id, docid(1));
    assert_eq!(
        key.as_ref().map(ToString::to_string).as_deref(),
        Some("finding:FR-1")
    );
    let Record::SameMeaning(record) = &declared[0].record else {
        panic!("a declaration: {:?}", declared[0].record);
    };
    assert!(matches!(record.of(), SameMeaningOf::Doc { id, .. } if *id == docid(1)));
    assert_eq!(record.actor().as_str(), "alice");
    assert_eq!(record.at().millis(), 1000);
    assert_eq!(app.ports().append_count(), 3);
    assert_eq!(app.ports().records()[2], declared[0].record);

    for reference in ["FR-1", "SP-2"] {
        assert_eq!(
            standing(&app, reference),
            Standing::Approved(Freshness::Fresh),
            "{reference}"
        );
    }
    let row = block_on(app.get(&r("SP-2"))).unwrap().row;
    assert!(
        matches!(row.notices.as_slice(), [Notice::UpstreamSameMeaning { declared: ids, .. }]
            if ids.clone().into_vec() == [record.id()]),
        "{:?}",
        row.notices
    );
}

#[test]
fn a_second_declaration_of_the_same_edit_is_refused_and_writes_nothing() {
    let mut app = approved();
    app.ports().edit(fr(1, "slower", &[]), 1);
    block_on(app.same_meaning(vec![same("FR-1")])).unwrap();
    let err = block_on(app.same_meaning(vec![same("FR-1")])).unwrap_err();
    assert!(
        matches!(
            err,
            ShelfError::SameMeaningRefused(SameMeaningRefusal::NothingToDeclare)
        ),
        "{err:?}"
    );
    // As a revocation of nothing: a request error.
    assert_eq!(err.class(), ErrorClass::Request);
    assert_eq!(app.ports().append_count(), 3);
}

#[test]
fn the_same_edit_named_twice_in_one_run_is_refused_the_second_time() {
    let mut app = approved();
    app.ports().edit(fr(1, "slower", &[]), 1);
    // Each request is decided on the ledger the ones before it left.
    let err = block_on(app.same_meaning(vec![same("FR-1"), same("FR-1")])).unwrap_err();
    assert!(matches!(
        err,
        ShelfError::SameMeaningRefused(SameMeaningRefusal::NothingToDeclare)
    ));
    assert_eq!(app.ports().append_count(), 2);
}

#[test]
fn a_refused_later_request_writes_none_of_the_run() {
    let mut app = approved();
    app.ports().edit(fr(1, "slower", &[]), 1);
    // SP-2 was not edited: nothing to declare of it.
    let err = block_on(app.same_meaning(vec![same("FR-1"), same("SP-2")])).unwrap_err();
    assert!(matches!(
        err,
        ShelfError::SameMeaningRefused(SameMeaningRefusal::NothingToDeclare)
    ));
    // A reference that names nothing, after a sound one.
    let err = block_on(app.same_meaning(vec![same("FR-1"), same("SP-9")])).unwrap_err();
    assert!(matches!(err, ShelfError::NotFound { .. }), "{err:?}");
    assert_eq!(app.ports().append_count(), 2);
    assert_eq!(app.ports().record_count(), 2);
    assert!(is_stale(&standing(&app, "SP-2")));
}

#[test]
fn a_run_declares_an_upstream_and_its_downstream_in_order() {
    let mut app = approved();
    app.ports().edit(fr(1, "slower", &[]), 1);
    app.ports().edit(sp(2, "a cache", &["FR-1"]), 2);
    let declared = block_on(app.same_meaning(vec![same("FR-1"), same("SP-2")])).unwrap();
    let about: Vec<_> = declared
        .iter()
        .map(|d| match &d.about {
            DeclaredAbout::Doc { id, .. } => *id,
            DeclaredAbout::Artifact { .. } => panic!("about a document"),
        })
        .collect();
    assert_eq!(about, [docid(1), docid(2)]);
    let records = app.ports().records();
    assert_eq!(
        records[2..],
        [declared[0].record.clone(), declared[1].record.clone()]
    );

    let row = block_on(app.get(&r("SP-2"))).unwrap().row;
    assert_eq!(row.standing, Standing::Approved(Freshness::Fresh));
    let kinds: Vec<_> = row
        .notices
        .iter()
        .map(|n| match n {
            Notice::SelfSameMeaning { .. } => "self",
            Notice::UpstreamSameMeaning { .. } => "upstream",
            other => panic!("{other:?}"),
        })
        .collect();
    assert_eq!(kinds.len(), 2);
    assert!(kinds.contains(&"self") && kinds.contains(&"upstream"));
}

#[test]
fn a_declaration_of_nothing_is_a_request_that_writes_nothing() {
    let mut app = approved();
    let err = block_on(app.same_meaning(vec![])).unwrap_err();
    assert!(matches!(err, ShelfError::NothingRequested), "{err:?}");
    assert_eq!(err.class(), ErrorClass::Request);
    assert_eq!(app.ports().append_count(), 2);
}

#[test]
fn a_declared_change_of_a_file_keeps_its_approval_fresh() {
    let fake = FakeShelf::new().with_files(vec![
        fr(1, "slow", &[]),
        sp_tested(2, "cache", &["FR-1"], &["tests/a.rs"]),
    ]);
    fake.put_file("tests/a.rs", b"assert!(fast);\n");
    let mut app = ShelfApp::new(fake);
    block_on(app.judge_all(vec![approve("FR-1"), approve("SP-2")])).unwrap();
    app.ports().put_file("tests/a.rs", b"assert!( fast );\n");
    assert!(is_stale(&standing(&app, "SP-2")));

    let declared = block_on(app.same_meaning(vec![SameMeaningRequest {
        about: SameMeaningAbout::Artifact(aname("tests/a.rs")),
        reason: NonBlank::new("formatting").unwrap(),
        actor: Some(Actor::new("bob").unwrap()),
        from: None,
    }]))
    .unwrap();
    assert_eq!(
        declared[0].about,
        DeclaredAbout::Artifact {
            artifact: artifact(&file_space(), "tests/a.rs")
        }
    );
    let Record::SameMeaning(record) = &declared[0].record else {
        panic!("a declaration");
    };
    assert_eq!(record.actor().as_str(), "bob");
    let row = block_on(app.get(&r("SP-2"))).unwrap().row;
    assert_eq!(row.standing, Standing::Approved(Freshness::Fresh));
    assert!(
        matches!(row.notices.as_slice(), [Notice::ArtifactSameMeaning { pin, .. }] if pin.pin().name() == &aname("tests/a.rs")),
        "{:?}",
        row.notices
    );

    // A path no document links is refused.
    let err = block_on(app.same_meaning(vec![SameMeaningRequest {
        about: SameMeaningAbout::Artifact(aname("tests/z.rs")),
        reason: NonBlank::new("formatting").unwrap(),
        actor: None,
        from: None,
    }]))
    .unwrap_err();
    assert!(matches!(
        err,
        ShelfError::SameMeaningRefused(SameMeaningRefusal::ArtifactNotLinked { .. })
    ));
    assert_eq!(err.class(), ErrorClass::Request);

    // A path with no file now is a refusal of the model.
    app.ports().remove_file("tests/a.rs");
    let err = block_on(app.same_meaning(vec![SameMeaningRequest {
        about: SameMeaningAbout::Artifact(aname("tests/a.rs")),
        reason: NonBlank::new("formatting").unwrap(),
        actor: None,
        from: None,
    }]))
    .unwrap_err();
    assert!(matches!(
        err,
        ShelfError::SameMeaningRefused(SameMeaningRefusal::ArtifactAbsent { .. })
    ));
    assert_eq!(err.class(), ErrorClass::Refused);
}

#[test]
fn a_named_earlier_hash_chooses_the_edit_and_an_unpinned_one_is_a_request_error() {
    // SP-2 pinned FR-1 at "slow"; FR-1 was rewritten and approved again at "fast";
    // then a typo fix. Without a name the own approval's hash is taken.
    let mut app = approved();
    app.ports().edit(fr(1, "fast", &[]), 1);
    let at_fast = block_on(app.get(&r("FR-1"))).unwrap().row;
    block_on(app.judge(approve("FR-1"))).unwrap();
    app.ports().edit(fr(1, "fats", &[]), 1);
    let fast = match at_fast.presence {
        crate::domain::shelf::build::DocPresence::Valid { hash, .. } => hash,
        other => panic!("{other:?}"),
    };

    let named = |hash| SameMeaningRequest {
        from: Some(hash),
        ..same("FR-1")
    };
    let unknown = crate::domain::hash::ContentHash::of_bytes(
        crate::domain::hash::HashKind::ShelfDoc,
        b"never pinned",
    );
    let err = block_on(app.same_meaning(vec![named(unknown)])).unwrap_err();
    assert!(
        matches!(
            err,
            ShelfError::SameMeaningRefused(SameMeaningRefusal::NotPinned { .. })
        ),
        "{err:?}"
    );
    assert_eq!(err.class(), ErrorClass::Request);
    assert_eq!(app.ports().append_count(), 3);

    let declared = block_on(app.same_meaning(vec![named(*fast.hash())])).unwrap();
    let Record::SameMeaning(record) = &declared[0].record else {
        panic!("a declaration");
    };
    let SameMeaningOf::Doc { from, .. } = record.of() else {
        panic!("about a document");
    };
    assert_eq!(from.iter().copied().collect::<Vec<_>>(), [fast]);
    // FR-1 holds; SP-2 saw "slow", and slow -> fast was never declared.
    assert_eq!(standing(&app, "FR-1"), Standing::Approved(Freshness::Fresh));
    assert!(is_stale(&standing(&app, "SP-2")));
}

// Withdrawing records by their identity.

#[test]
fn withdrawing_a_declaration_makes_the_declared_edit_stale_again() {
    let mut app = approved();
    app.ports().edit(fr(1, "slower", &[]), 1);
    let declared = block_on(app.same_meaning(vec![same("FR-1")])).unwrap();
    assert_eq!(standing(&app, "SP-2"), Standing::Approved(Freshness::Fresh));

    let record = block_on(app.withdraw(withdraw(vec![declared[0].record.id()]))).unwrap();
    assert!(matches!(record, Record::Revocation(_)), "{record:?}");
    let Canon::Map(canon) = record.to_canon() else {
        panic!("a record is a map");
    };
    assert_eq!(
        canon.get("targets"),
        Some(&Canon::list([Canon::str(
            declared[0].record.id().to_string()
        )]))
    );
    assert_eq!(app.ports().append_count(), 4);
    let row = block_on(app.get(&r("SP-2"))).unwrap().row;
    let Standing::Approved(Freshness::Degraded(d)) = &row.standing else {
        panic!("stale again: {:?}", row.standing);
    };
    assert!(
        d.causes()
            .iter()
            .any(|c| matches!(c, Cause::UpstreamChanged { .. }))
    );
    // And the edit can be declared again.
    block_on(app.same_meaning(vec![same("FR-1")])).unwrap();
}

#[test]
fn withdrawing_a_decision_by_its_identity_takes_it_back() {
    let mut app = approved();
    let approval = app.ports().records()[1].id();
    block_on(app.withdraw(withdraw(vec![approval]))).unwrap();
    assert_eq!(standing(&app, "SP-2"), Standing::Pending);
    assert_eq!(standing(&app, "FR-1"), Standing::Approved(Freshness::Fresh));
}

#[test]
fn withdrawing_what_cannot_be_withdrawn_is_refused_and_writes_nothing() {
    let mut app = approved();
    let approval = app.ports().records()[1].id();
    let revocation = block_on(app.withdraw(withdraw(vec![approval])))
        .unwrap()
        .id();
    assert_eq!(app.ports().append_count(), 3);

    let unknown = JudgementId::new(eid(424242));
    for (ids, expected) in [
        (vec![unknown], "unknown"),
        (vec![revocation], "unknown"),
        (vec![approval], "withdrawn"),
        // A sound one with an unsound one: nothing is written.
        (vec![app.ports().records()[0].id(), unknown], "unknown"),
    ] {
        let err = block_on(app.withdraw(withdraw(ids.clone()))).unwrap_err();
        let found = match &err {
            ShelfError::Withdraw(WithdrawRefusal::Unknown { .. }) => "unknown",
            ShelfError::Withdraw(WithdrawRefusal::AlreadyWithdrawn { .. }) => "withdrawn",
            other => panic!("{ids:?}: {other:?}"),
        };
        assert_eq!(found, expected, "{ids:?}");
        assert_eq!(err.class(), ErrorClass::Request);
    }
    assert_eq!(app.ports().append_count(), 3);
    assert_eq!(standing(&app, "FR-1"), Standing::Approved(Freshness::Fresh));

    let err = block_on(app.withdraw(withdraw(vec![]))).unwrap_err();
    assert!(matches!(err, ShelfError::NothingRequested), "{err:?}");
    assert_eq!(app.ports().append_count(), 3);
}
