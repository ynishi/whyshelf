//! Tests of `exempt` over the in-memory fake: an exemption under a `true` cover makes
//! `check` clean and is listed apart; a run of several is written whole; a refusal
//! writes nothing; a link added later makes the exemption unneeded; `withdraw` takes
//! it back.

use crate::domain::actor::Actor;
use crate::domain::nonempty::NonBlank;
use crate::domain::shelf::build::DocReading;
use crate::domain::shelf::exemption::{CoverMiss, ExemptRefusal, Unneeded};
use crate::domain::shelf::freshness::Notice;
use crate::domain::shelf::ledger::{Exempt, ExemptOf, JudgedAt, JudgementId, Record};
use crate::domain::shelf::lookup::Reference;
use crate::domain::shelf::schema::{BookName, CoverDecl, CoverSeverity, EntryName};

use super::fake_tests::{FakeShelf, block_on, decls, docid, eid, fr, sp};
use super::{
    BooksCheck, ErrorClass, ExemptRequest, JudgeRequest, Judgement, ShelfApp, ShelfError,
    WithdrawRequest,
};

/// The fake's books with `cover = true` on the spec's `grounds`, over these files.
fn app(files: Vec<DocReading>) -> ShelfApp<FakeShelf> {
    let mut covering = decls();
    covering
        .iter_mut()
        .find(|b| b.name == "spec")
        .and_then(|b| b.links.iter_mut().find(|l| l.name == "grounds"))
        .expect("the fake declares it")
        .cover = Some(CoverDecl::Truth(true));
    let fake = FakeShelf::new().with_files(files);
    *fake.decls.lock().unwrap() = covering;
    ShelfApp::new(fake)
}

/// FR-1 grounded by SP-3; FR-2 and FR-4 alone.
fn two_alone() -> ShelfApp<FakeShelf> {
    app(vec![
        fr(1, "grounded", &[]),
        fr(2, "kept", &[]),
        fr(4, "also kept", &[]),
        sp(3, "cache", &["FR-1"]),
    ])
}

fn exempt_from(reference: &str, book: &str, link: &str) -> ExemptRequest {
    ExemptRequest {
        reference: reference.parse::<Reference>().unwrap(),
        book: BookName::new(book).unwrap(),
        link: EntryName::new(link).unwrap(),
        reason: NonBlank::new("kept on purpose").unwrap(),
        actor: None,
    }
}

fn exempt(reference: &str) -> ExemptRequest {
    exempt_from(reference, "spec", "grounds")
}

fn check(app: &ShelfApp<FakeShelf>) -> BooksCheck {
    block_on(app.books_check()).unwrap()
}

#[test]
fn an_exemption_under_a_true_cover_makes_check_clean_and_is_listed_apart() {
    let mut app = two_alone();
    let before = check(&app);
    assert!(!before.is_clean());

    let done = block_on(app.exempt(vec![exempt("FR-2"), exempt("FR-4")])).unwrap();
    assert_eq!(done.len(), 2);
    assert_eq!(app.ports().append_count(), 2);
    assert_eq!(done[0].doc(), docid(2));
    assert_eq!(
        done[0].key.as_ref().map(ToString::to_string).as_deref(),
        Some("finding:FR-2")
    );
    assert_eq!(done[0].record.actor().as_str(), "alice");
    assert_eq!(done[0].record.at().millis(), 1000);
    assert_eq!(
        app.ports().records(),
        done.iter()
            .map(|d| Record::Exempt(d.record.clone()))
            .collect::<Vec<_>>()
    );

    let after = check(&app);
    assert!(after.is_clean(), "{after:?}");
    let BooksCheck::Checked {
        uncovered,
        exempt,
        unneeded,
        ..
    } = &after
    else {
        panic!("the declaration is sound");
    };
    assert!(uncovered.is_empty(), "{uncovered:?}");
    assert!(unneeded.is_empty(), "{unneeded:?}");
    let listed: Vec<_> = exempt.iter().map(|e| (e.doc, e.severity)).collect();
    assert_eq!(
        listed,
        [
            (docid(2), CoverSeverity::Error),
            (docid(4), CoverSeverity::Error)
        ]
    );
    assert_eq!(exempt[0].record, done[0].record);
    assert_eq!(exempt[0].record.reason().as_str(), "kept on purpose");

    let row = block_on(app.get(&"FR-2".parse().unwrap())).unwrap().row;
    assert!(
        matches!(row.notices.as_slice(), [Notice::Exempt { exemption, .. }]
            if *exemption == done[0].record.id()),
        "{:?}",
        row.notices
    );
}

#[test]
fn a_refused_exemption_writes_nothing_of_its_run() {
    let mut app = two_alone();
    // The second names the first's document again: decided on what the first left,
    // it is a second exemption, refused, and the first is not written either.
    let err = block_on(app.exempt(vec![exempt("FR-2"), exempt("FR-2")])).unwrap_err();
    // Its id was never written, so the refusal names the run, not a record.
    let ShelfError::ExemptRefused(refusal) = &err else {
        panic!("refused: {err:?}");
    };
    assert!(
        matches!(refusal, ExemptRefusal::AlreadyExemptInRun { .. }),
        "{refusal:?}"
    );
    assert_eq!(
        refusal.to_string(),
        "an earlier exemption of this run already exempts it from spec.grounds"
    );
    assert_eq!(err.class(), ErrorClass::Request);
    assert_eq!(app.ports().append_count(), 0);

    // One written before the run is named by its id.
    let first = block_on(app.exempt(vec![exempt("FR-2")])).unwrap();
    let err = block_on(app.exempt(vec![exempt("FR-2")])).unwrap_err();
    assert!(
        matches!(
            &err,
            ShelfError::ExemptRefused(ExemptRefusal::AlreadyExempt { record, .. })
                if *record == first[0].record.id()
        ),
        "{err:?}"
    );
    app.ports().records.lock().unwrap().clear();
    *app.ports().appends.lock().unwrap() = 0;

    let err = block_on(app.exempt(vec![exempt("FR-4"), exempt_from("FR-2", "spec", "related")]))
        .unwrap_err();
    assert!(
        matches!(
            &err,
            ShelfError::ExemptRefused(ExemptRefusal::NotCovering {
                miss: CoverMiss::NoCover,
                ..
            })
        ),
        "{err:?}"
    );
    assert_eq!(err.class(), ErrorClass::Request);
    assert_eq!(app.ports().append_count(), 0);

    let err = block_on(app.exempt(vec![exempt("FR-9")])).unwrap_err();
    assert!(matches!(err, ShelfError::NotFound { .. }), "{err:?}");
    assert_eq!(app.ports().append_count(), 0);

    let err = block_on(app.exempt(Vec::new())).unwrap_err();
    assert!(matches!(err, ShelfError::NothingRequested), "{err:?}");
    assert_eq!(app.ports().record_count(), 0);
}

/// An exemption of `doc` from `spec.grounds` as merged branches can leave it.
fn written(n: u128, doc: u128) -> Record {
    Record::Exempt(Exempt::new(
        JudgementId::new(eid(n)),
        ExemptOf::new(
            docid(doc),
            BookName::new("spec").unwrap(),
            EntryName::new("grounds").unwrap(),
        ),
        Actor::new("bob").unwrap(),
        JudgedAt::new(500),
        NonBlank::new("kept on a branch").unwrap(),
    ))
}

#[test]
fn two_exemptions_of_one_document_from_one_link_are_listed_as_repeated() {
    let app = two_alone();
    app.ports()
        .records
        .lock()
        .unwrap()
        .extend([written(7001, 2), written(7002, 2)]);
    let check = check(&app);
    let BooksCheck::Checked {
        exempt, repeated, ..
    } = &check
    else {
        panic!("the declaration is sound");
    };
    assert_eq!(exempt.len(), 2, "{exempt:?}");
    assert_eq!(repeated.len(), 1, "{repeated:?}");
    assert_eq!(repeated[0].doc, docid(2));
    assert_eq!(
        repeated[0].exemptions.iter().copied().collect::<Vec<_>>(),
        [JudgementId::new(eid(7001)), JudgementId::new(eid(7002))]
    );
    // It is a notice: the check is as the gaps make it (FR-4 is one, of `true`).
    assert!(!check.is_clean());
}

#[test]
fn an_exemption_unneeded_at_once_is_refused_and_writes_nothing() {
    let mut app = two_alone();
    // FR-1 is grounded by SP-3 now.
    let err = block_on(app.exempt(vec![exempt("FR-2"), exempt("FR-1")])).unwrap_err();
    let ShelfError::ExemptRefused(ExemptRefusal::Reached { by, .. }) = &err else {
        panic!("refused as reached: {err:?}");
    };
    assert_eq!(
        by.iter().map(ToString::to_string).collect::<Vec<_>>(),
        ["spec:SP-3"]
    );
    assert_eq!(err.class(), ErrorClass::Request);
    assert_eq!(app.ports().append_count(), 0);

    // FR-2 denied: no covering link expects it.
    block_on(app.judge_all(vec![JudgeRequest {
        judgement: Judgement::Deny { reason: None },
        reference: "FR-2".parse().unwrap(),
        actor: None,
    }]))
    .unwrap();
    let appended = app.ports().append_count();
    let err = block_on(app.exempt(vec![exempt("FR-4"), exempt("FR-2")])).unwrap_err();
    assert!(
        matches!(
            &err,
            ShelfError::ExemptRefused(ExemptRefusal::DecidedAgainst { id, .. }) if *id == docid(2)
        ),
        "{err:?}"
    );
    assert_eq!(err.class(), ErrorClass::Request);
    assert_eq!(app.ports().append_count(), appended);
}

#[test]
fn an_exemption_is_unneeded_once_the_link_reaches_the_document() {
    let mut app = two_alone();
    let done = block_on(app.exempt(vec![exempt("FR-2"), exempt("FR-4")])).unwrap();
    app.ports().edit(sp(3, "cache", &["FR-1", "FR-2"]), 3);

    // An exemption not needed is a notice: it leaves the check clean.
    let after = check(&app);
    assert!(after.is_clean(), "{after:?}");
    let BooksCheck::Checked {
        uncovered,
        exempt,
        unneeded,
        ..
    } = &after
    else {
        panic!("the declaration is sound");
    };
    // FR-4 is still exempt; FR-2 is reached, and its exemption is not needed.
    assert!(uncovered.is_empty(), "{uncovered:?}");
    assert_eq!(exempt.iter().map(|e| e.doc).collect::<Vec<_>>(), [docid(4)]);
    assert_eq!(unneeded.len(), 1, "{unneeded:?}");
    assert_eq!(unneeded[0].doc, docid(2));
    assert_eq!(unneeded[0].record, done[0].record);
    assert!(
        matches!(&unneeded[0].why, Unneeded::Reached { by } if by.clone().into_vec() == [docid(3)]),
        "{:?}",
        unneeded[0].why
    );
}

#[test]
fn an_exemption_is_withdrawn_by_its_record_id() {
    let mut app = two_alone();
    let done = block_on(app.exempt(vec![exempt("FR-2"), exempt("FR-4")])).unwrap();
    let ids: Vec<JudgementId> = done.iter().map(|d| d.record.id()).collect();
    block_on(app.withdraw(WithdrawRequest {
        records: ids,
        reason: NonBlank::new("to be grounded").unwrap(),
        actor: None,
    }))
    .unwrap();
    let after = check(&app);
    assert!(!after.is_clean());
    let BooksCheck::Checked {
        uncovered,
        exempt: listed,
        ..
    } = &after
    else {
        panic!("the declaration is sound");
    };
    assert_eq!(uncovered.len(), 2, "{uncovered:?}");
    assert!(listed.is_empty(), "{listed:?}");
    // Withdrawn, the document may be exempted again.
    assert!(block_on(app.exempt(vec![exempt("FR-2")])).is_ok());
}
