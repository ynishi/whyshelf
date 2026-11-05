use std::collections::BTreeSet;

use super::*;
use crate::app::commit::fixtures::{
    Publishing, ada, approval_with_object, no_ledgers, object_view, plain_view, private_corpus,
    public_corpus, text_of,
};
use crate::app::fake::{FakeCorpus, FakePorts, block_on};
use crate::domain::gate::Passed;
use crate::domain::id::test_support::docid;
use crate::domain::judge::test_support::judge;
use crate::domain::judgement::Split;
use crate::domain::judgement::test_support::{cr_passed, hash_of, jid, object_of, ts, why};
use crate::domain::ledger::Ledger;

fn key_of(judgement: &ObjectJudgement) -> ObjectKey {
    *judgement.object()
}

fn purge(origin: Visibility) -> Judgement {
    Judgement::purge(
        jid(9),
        ts(),
        ada(),
        origin,
        docid(1),
        TextHash::of("an old version"),
        why(),
    )
}

/// The chain as a use case runs it, up to the evidence that the object is stored.
fn stored_for(
    fake: &FakeCorpus,
    corpus: &Corpus,
    judgement: &ObjectJudgement,
    text: &NormalizedText,
) -> Result<StoredObject, Stop> {
    block_on(async {
        let scanned = scan(&FakePorts::new(), corpus, &no_ledgers(), text, judgement).await?;
        store(fake, &scanned).await
    })
}

// --- Scanned: built after the scan, from the version the gate read ------------------

#[test]
fn a_scanned_text_is_the_version_the_judgement_keeps_and_the_ledger_the_rule_chooses() {
    let judgement = approval_with_object("v1", Visibility::Public);
    let text = text_of("v1");
    let scanned = block_on(scan(
        &FakePorts::new(),
        &public_corpus(),
        &no_ledgers(),
        &text,
        &judgement,
    ))
    .expect("a scanned text");
    assert_eq!(scanned.key(), key_of(&judgement));
    assert_eq!(scanned.ledger(), Visibility::Public);
    assert_eq!(scanned.text(), text.as_str());
}

#[test]
fn a_text_whose_hash_differs_from_the_judgements_is_not_scanned() {
    let judgement = approval_with_object("v1", Visibility::Public);
    let other = text_of("v2");
    let err = block_on(scan(
        &FakePorts::new(),
        &public_corpus(),
        &no_ledgers(),
        &other,
        &judgement,
    ))
    .expect_err("another version");
    let Stop::Broken(ChainError::TextDiffers { expected, found }) = err else {
        panic!("expected a text that differs, got {err:?}");
    };
    assert_eq!(&expected, judgement.checked_text());
    assert_eq!(found, TextHash::of(other.as_str()));
}

#[test]
fn the_text_scanned_is_the_one_the_gate_read() {
    // The gate read "the file as read"; that is the text the approval keeps, and no
    // other text of the same content is scanned in its place.
    let read = NormalizedText::new("the file as read\n".to_owned());
    let passed = Passed::for_test_read(
        cr_passed(1, "v1").attestation().clone(),
        TextHash::of(read.as_str()),
    );
    let content = *passed.attestation().target_hash();
    let judgement = object_view(
        Judgement::approve(
            jid(1),
            ts(),
            ada(),
            Visibility::Public,
            passed,
            ObjectKey::new(content, TextHash::of(read.as_str())),
        )
        .expect("an approval of the text read"),
    );
    assert!(Scanned::new(&read, &judgement, Visibility::Public).is_ok());
    let retagged = NormalizedText::new("the file as read, retagged\n".to_owned());
    assert_eq!(
        Scanned::new(&retagged, &judgement, Visibility::Public).unwrap_err(),
        ChainError::TextDiffers {
            expected: TextHash::of(read.as_str()),
            found: TextHash::of(retagged.as_str()),
        }
    );
}

#[test]
fn the_constructor_itself_refuses_a_text_of_another_version() {
    let judgement = approval_with_object("v1", Visibility::Public);
    let other = text_of("v2");
    assert!(matches!(
        Scanned::new(&other, &judgement, Visibility::Public),
        Err(ChainError::TextDiffers { .. })
    ));
    assert!(Scanned::new(&text_of("v1"), &judgement, Visibility::Public).is_ok());
}

#[test]
fn a_judgement_is_split_once_into_the_view_that_keeps_an_object_and_the_one_that_does_not() {
    // What has nothing to scan is not an `ObjectJudgement`, so it cannot be handed
    // to `Scanned::new`; the split is the one place that tells them apart.
    let approval = approval_with_object("v1", Visibility::Public);
    assert_eq!(approval.checked_text(), approval.object().text());
    let Split::Plain(purged) = purge(Visibility::Public).split() else {
        panic!("a purge keeps no object");
    };
    assert_eq!(purged.judgement().kind().name(), "purge");
}

#[test]
fn a_secret_stops_the_scan_before_anything_else_is_asked() {
    let judgement = approval_with_object("v1", Visibility::Public);
    let text = NormalizedText::new("key AKIA-SECRET\n".to_owned());
    let err = block_on(scan(
        &FakePorts::new(),
        &public_corpus(),
        &no_ledgers(),
        &text,
        &judgement,
    ))
    .expect_err("a secret");
    let Stop::Refused(Refusal::Secret(hits)) = err else {
        panic!("expected a secret, got {err:?}");
    };
    assert_eq!(hits.first().line(), 1);
}

#[test]
fn a_judgement_about_nothing_known_has_no_ledger_to_scan_for() {
    // The Finding is not in the corpus, and no ledger holds a record about it.
    let judgement = approval_with_object("v1", Visibility::Public);
    let empty = crate::domain::judgement::test_support::corpus_of(Vec::new());
    let err = block_on(scan(
        &FakePorts::new(),
        &empty,
        &no_ledgers(),
        &text_of("v1"),
        &judgement,
    ))
    .expect_err("no ledger");
    assert!(matches!(
        err,
        Stop::Refused(Refusal::Judgement(JudgementError::UnknownTarget { .. }))
    ));
}

// --- StoredObject: only from Put::Stored ----------------------------------------------

#[test]
fn a_stored_text_gives_the_evidence_and_is_in_the_store_of_its_ledger() {
    let fake = FakeCorpus::new();
    let judgement = approval_with_object("v1", Visibility::Public);
    let stored = stored_for(&fake, &public_corpus(), &judgement, &text_of("v1")).expect("stored");
    assert_eq!(stored.key(), key_of(&judgement));
    assert_eq!(stored.ledger(), Visibility::Public);
    assert!(fake.has_object(Visibility::Public, *key_of(&judgement).text()));
    assert!(!fake.has_object(Visibility::Private, *key_of(&judgement).text()));
}

#[test]
fn a_purged_version_is_not_stored_and_gives_no_evidence() {
    let fake = FakeCorpus::new();
    let judgement = approval_with_object("v1", Visibility::Public);
    let hash = *key_of(&judgement).text();
    fake.purge(Visibility::Public, hash);
    let err = stored_for(&fake, &public_corpus(), &judgement, &text_of("v1"))
        .expect_err("the version was purged");
    assert!(matches!(err, Stop::Refused(Refusal::ObjectPurged(h)) if h == hash));
    assert!(!fake.has_object(Visibility::Public, hash));
}

#[test]
fn only_a_put_that_stored_gives_the_evidence() {
    let judgement = approval_with_object("v1", Visibility::Public);
    let text = text_of("v1");
    let scanned = Scanned::new(&text, &judgement, Visibility::Public).expect("scanned");
    assert!(stored(&scanned, Put::Stored).is_ok());
    assert_eq!(
        stored(&scanned, Put::Purged),
        Err(Refusal::ObjectPurged(*key_of(&judgement).text()))
    );
}

#[test]
fn a_failing_object_store_is_a_port_failure() {
    let fake = FakeCorpus::new();
    fake.fail(PortName::Objects);
    let judgement = approval_with_object("v1", Visibility::Public);
    let err = stored_for(&fake, &public_corpus(), &judgement, &text_of("v1")).expect_err("fails");
    assert!(matches!(
        err,
        Stop::Port(AppError::Port {
            port: PortName::Objects,
            ..
        })
    ));
}

// --- Recordable::with_object ----------------------------------------------------------

fn stored_evidence(judgement: &ObjectJudgement, tag: &str, ledger: Visibility) -> StoredObject {
    let text = text_of(tag);
    let scanned = Scanned::new(&text, judgement, ledger).expect("scanned");
    stored(&scanned, Put::Stored).expect("stored")
}

#[test]
fn an_approval_is_recordable_with_the_object_it_keeps() {
    let judgement = approval_with_object("v1", Visibility::Public);
    let evidence = stored_evidence(&judgement, "v1", Visibility::Public);
    let record = Recordable::with_object(judgement.clone(), &evidence).expect("recordable");
    assert_eq!(record.judgement(), judgement.judgement());
    assert_eq!(record.ledger(), Visibility::Public);
    assert_eq!(record.into_judgement(), judgement.into_judgement());
}

#[test]
fn with_object_refuses_an_object_of_another_version() {
    let v1 = approval_with_object("v1", Visibility::Public);
    let v2 = object_view(
        crate::domain::judgement::Judgement::approve(
            jid(2),
            ts(),
            ada(),
            Visibility::Public,
            cr_passed(1, "v2"),
            object_of(&hash_of("v2")),
        )
        .expect("an approval"),
    );
    let of_v2 = stored_evidence(&v2, "v2", Visibility::Public);
    assert_eq!(
        Recordable::with_object(v1.clone(), &of_v2),
        Err(ChainError::ObjectDiffers {
            judgement: Box::new(key_of(&v1)),
            stored: Box::new(key_of(&v2)),
        })
    );
}

#[test]
fn with_object_refuses_an_object_stored_for_the_other_ledger() {
    let judgement = approval_with_object("v1", Visibility::Public);
    let in_private = stored_evidence(&judgement, "v1", Visibility::Private);
    assert_eq!(
        Recordable::with_object(judgement, &in_private),
        Err(ChainError::LedgerDiffers {
            origin: Visibility::Public,
            ledger: Visibility::Private,
        })
    );
}

// --- Recordable::without_object -------------------------------------------------------

fn plain_purge(origin: Visibility) -> PlainJudgement {
    plain_view(purge(origin))
}

#[test]
fn without_object_takes_the_ledger_from_the_rule() {
    let record = Recordable::without_object(
        plain_purge(Visibility::Public),
        &public_corpus(),
        &no_ledgers(),
    )
    .expect("a purge");
    assert_eq!(record.ledger(), Visibility::Public);

    // The same purge of a private document belongs in the private ledger.
    let record = Recordable::without_object(
        plain_purge(Visibility::Private),
        &private_corpus(),
        &no_ledgers(),
    )
    .expect("a purge");
    assert_eq!(record.ledger(), Visibility::Private);
}

#[test]
fn without_object_refuses_a_record_first_written_in_the_other_ledger() {
    assert_eq!(
        Recordable::without_object(
            plain_purge(Visibility::Private),
            &public_corpus(),
            &no_ledgers()
        ),
        Err(ChainError::LedgerDiffers {
            origin: Visibility::Private,
            ledger: Visibility::Public,
        })
    );
}

#[test]
fn a_publication_is_recordable_in_the_public_ledger() {
    let published = Judgement::published(
        jid(3),
        ts(),
        ada(),
        docid(21),
        BTreeSet::new(),
        &no_ledgers(),
    )
    .expect("a publication");
    let record = Recordable::without_object(plain_view(published), &public_corpus(), &no_ledgers())
        .expect("recordable");
    assert_eq!(record.ledger(), Visibility::Public);
}

#[test]
fn what_no_rule_can_place_is_refused_with_the_domains_reason() {
    let nowhere = crate::domain::judgement::test_support::corpus_of(Vec::new());
    let Err(ChainError::Placement(reason)) =
        Recordable::without_object(plain_purge(Visibility::Public), &nowhere, &no_ledgers())
    else {
        panic!("expected a placement that failed");
    };
    assert!(matches!(*reason, JudgementError::UnknownTarget { .. }));
}

#[test]
fn a_ledger_no_rule_can_choose_is_a_refusal_at_the_append_as_at_the_scan() {
    let fake = FakeCorpus::new();
    let nowhere = crate::domain::judgement::test_support::corpus_of(Vec::new());
    let err = block_on(append_without_object(
        &fake,
        plain_purge(Visibility::Public),
        &nowhere,
        &no_ledgers(),
    ))
    .expect_err("no ledger");
    assert!(matches!(
        err,
        Stop::Refused(Refusal::Judgement(JudgementError::UnknownTarget { .. }))
    ));
    assert!(fake.records(Visibility::Public).is_empty());
}

// --- Recordable::copied ---------------------------------------------------------------

#[test]
fn a_record_the_plan_carries_is_copied_to_the_public_ledger_unchanged() {
    let publishing = Publishing::of(b"bytes");
    let record = publishing.carried[0].clone();
    let copy =
        Recordable::copied(record.clone(), &publishing.plan, &publishing.private).expect("a copy");
    assert_eq!(copy.ledger(), Visibility::Public);
    assert_eq!(copy.judgement().origin(), Visibility::Private);
    assert_eq!(copy.judgement().canonical_bytes(), record.canonical_bytes());
}

#[test]
fn a_record_the_plan_does_not_carry_is_not_copied() {
    let publishing = Publishing::of(b"bytes");
    assert!(!publishing.carried_ids().contains(&publishing.other.id()));
    assert_eq!(
        Recordable::copied(
            publishing.other.clone(),
            &publishing.plan,
            &publishing.private
        ),
        Err(ChainError::NotInPlan {
            record: publishing.other.id(),
        })
    );
}

#[test]
fn a_record_of_the_plans_id_with_other_bytes_is_not_copied() {
    let publishing = Publishing::of(b"bytes");
    let read = &publishing.carried[0];
    // The same id, another judge: not the record the private ledger holds.
    let altered = Judgement::approve(
        read.id(),
        ts(),
        judge("grace"),
        Visibility::Private,
        cr_passed(1, "v1"),
        object_of(&hash_of("v1")),
    )
    .expect("an approval");
    assert_ne!(altered.canonical_bytes(), read.canonical_bytes());
    assert_eq!(
        Recordable::copied(altered, &publishing.plan, &publishing.private),
        Err(ChainError::NotAsRead { record: read.id() })
    );
}

// --- the whole chain, on the fake ledger ---------------------------------------------

#[test]
fn an_approval_reaches_the_ledger_only_after_its_object_is_stored() {
    let fake = FakeCorpus::new();
    let corpus = public_corpus();
    let judgement = approval_with_object("v1", Visibility::Public);
    let evidence = stored_for(&fake, &corpus, &judgement, &text_of("v1")).expect("stored");

    block_on(append_with_object(&fake, judgement.clone(), &evidence)).expect("appended");
    assert_eq!(
        fake.records(Visibility::Public),
        vec![judgement.judgement().clone()]
    );
    assert!(fake.records(Visibility::Private).is_empty());

    // The same record again is success and writes nothing more.
    block_on(append_with_object(&fake, judgement, &evidence)).expect("appended again");
    assert_eq!(fake.records(Visibility::Public).len(), 1);
}

#[test]
fn a_purge_reaches_the_ledger_and_takes_the_text_out() {
    let fake = FakeCorpus::new();
    let corpus = public_corpus();
    let judgement = approval_with_object("v1", Visibility::Public);
    let hash = *key_of(&judgement).text();
    stored_for(&fake, &corpus, &judgement, &text_of("v1")).expect("stored");
    assert!(fake.has_object(Visibility::Public, hash));

    let purge = Judgement::purge(
        jid(4),
        ts(),
        judge("ada"),
        Visibility::Public,
        docid(1),
        hash,
        why(),
    );
    block_on(append_without_object(
        &fake,
        plain_view(purge.clone()),
        &corpus,
        &no_ledgers(),
    ))
    .expect("appended");
    assert!(fake.records(Visibility::Public).contains(&purge));
    assert!(!fake.has_object(Visibility::Public, hash));

    // And it is not put back.
    let err = stored_for(&fake, &corpus, &judgement, &text_of("v1")).expect_err("purged");
    assert!(matches!(err, Stop::Refused(Refusal::ObjectPurged(h)) if h == hash));
}

#[test]
fn a_publications_copy_is_the_same_record_in_both_ledgers() {
    let fake = FakeCorpus::new();
    let publishing = Publishing::of(b"bytes");
    for record in &publishing.carried {
        fake.seed_record(Visibility::Private, record.clone());
        block_on(append_copied(
            &fake,
            record.clone(),
            &publishing.plan,
            &publishing.private,
        ))
        .expect("copied");
    }
    let private = Ledger::from_judgements(fake.records(Visibility::Private));
    let public = Ledger::from_judgements(fake.records(Visibility::Public));
    let both = Ledgers::new(public, private);
    // Byte for byte the same in both, so nothing is corrupt.
    assert!(both.corrupt().is_empty());
    for id in publishing.carried_ids() {
        assert!(both.public().contains(&id));
    }
}

#[test]
fn a_failing_ledger_is_a_port_failure() {
    let fake = FakeCorpus::new();
    let corpus = public_corpus();
    let judgement = approval_with_object("v1", Visibility::Public);
    let evidence = stored_for(&fake, &corpus, &judgement, &text_of("v1")).expect("stored");
    fake.fail(PortName::Ledger);
    let err = block_on(append_with_object(&fake, judgement, &evidence)).expect_err("fails");
    assert!(matches!(
        err,
        Stop::Port(AppError::Port {
            port: PortName::Ledger,
            ..
        })
    ));
}
