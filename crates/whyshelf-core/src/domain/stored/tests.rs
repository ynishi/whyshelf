use std::collections::{BTreeMap, BTreeSet};

use super::*;
use crate::domain::id::test_support::{docid, eid, iid};
use crate::domain::integration::FingerprintUnavailable;
use crate::domain::judge::test_support::{agent, judge};
use crate::domain::judgement::test_support::{
    approve, ca_passed, cm_passed, cm_passed_on, cr_passed, cs_passed, cs_passed_under, hash_of,
    jid, ts, why,
};
use crate::domain::judgement::{FilePresence, Target};
use crate::domain::revision::{Revision, RevisionId};

/// The record read back from the bytes it writes, and those bytes again.
fn round_trip(judgement: Judgement) -> StoredJudgement {
    let bytes = judgement.canonical_bytes();
    let stored = StoredJudgement::decode(&bytes).expect("a record this build writes decodes");
    assert_eq!(stored.canonical_bytes(), bytes);
    assert_eq!(stored, StoredJudgement::from(judgement));
    stored
}

fn adoptable(n: u128) -> Adoptable {
    Adoptable::for_test(
        (Gate::Cr, eid(n), hash_of("adopted")),
        vec![Pinned::new(eid(n + 1), hash_of("up").into())],
        SourceUri::Uri("https://example.org/decision".parse().unwrap()),
    )
}

fn adopt(n: u128) -> Judgement {
    let adoptable = adoptable(n);
    let object = ObjectKey::new(*adoptable.attestation().hash(), *adoptable.text());
    Judgement::adopt(
        jid(n),
        ts(),
        judge("ann"),
        Visibility::Private,
        adoptable,
        object,
    )
    .unwrap()
}

/// The bytes of `judgement` with the map at `path` changed by `edit`.
fn edited(
    judgement: &Judgement,
    path: &[&str],
    edit: impl FnOnce(&mut BTreeMap<String, Canon>),
) -> Vec<u8> {
    let mut canon = judgement.to_canon();
    let mut at = &mut canon;
    for key in path {
        let Canon::Map(entries) = at else {
            panic!("{key}: not a map")
        };
        at = entries.get_mut(*key).expect("the key is there");
    }
    let Canon::Map(entries) = at else {
        panic!("not a map")
    };
    edit(entries);
    canon.to_bytes()
}

#[test]
fn every_kind_reads_back_as_the_bytes_it_was_written_as() {
    let ann = judge("ann");
    let bot = agent("bot", "claude");
    round_trip(approve(1, &ann, Visibility::Public, cr_passed(1, "f")));
    round_trip(approve(
        2,
        &bot,
        Visibility::Private,
        cs_passed(2, "c", "h"),
    ));
    round_trip(approve(
        3,
        &ann,
        Visibility::Private,
        cs_passed_under(3, "c", "h", GatePolicy::new(false, false)),
    ));
    round_trip(approve(
        4,
        &ann,
        Visibility::Public,
        cm_passed(4, "h", "cs"),
    ));
    round_trip(approve(5, &ann, Visibility::Public, ca_passed(5, "i")));
    round_trip(adopt(6));
    round_trip(Judgement::assemble(
        jid(7),
        ts(),
        ann.clone(),
        Visibility::Private,
        JudgementKind::Revoke {
            targets: NonEmptyVec::new(jid(1), vec![jid(2)]),
            reason: why(),
        },
    ));
    round_trip(
        Judgement::retire(
            jid(8),
            ts(),
            ann.clone(),
            Visibility::Public,
            Target::Item(iid(3)),
            FilePresence::Absent,
            why(),
        )
        .unwrap(),
    );
    round_trip(Judgement::purge(
        jid(9),
        ts(),
        ann.clone(),
        Visibility::Private,
        docid(1),
        TextHash::of("gone"),
        why(),
    ));
    round_trip(Judgement::assemble(
        jid(10),
        ts(),
        ann,
        Visibility::Public,
        JudgementKind::Published {
            doc: docid(1),
            records: BTreeSet::from([jid(1), jid(8)]),
        },
    ));
}

#[test]
fn a_restored_proof_cannot_start_a_new_approval() {
    let stored = round_trip(approve(
        1,
        &judge("ann"),
        Visibility::Public,
        cr_passed(1, "f"),
    ));
    let JudgementKind::Approve { passed, object } = stored.kind().clone() else {
        panic!("an approval")
    };
    assert_eq!(
        Judgement::approve(
            jid(2),
            ts(),
            judge("ann"),
            Visibility::Public,
            passed,
            object
        ),
        Err(JudgementError::ProofRestored)
    );
}

#[test]
fn a_restored_proof_cannot_start_a_new_adoption() {
    let stored = round_trip(adopt(1));
    let JudgementKind::Adopt { adoptable, object } = stored.kind().clone() else {
        panic!("an adoption")
    };
    assert_eq!(
        Judgement::adopt(
            jid(2),
            ts(),
            judge("ann"),
            Visibility::Public,
            adoptable,
            object
        ),
        Err(JudgementError::ProofRestored)
    );
}

#[test]
fn bytes_that_are_not_a_value_are_refused() {
    let bytes = approve(1, &judge("ann"), Visibility::Public, cr_passed(1, "f")).canonical_bytes();
    assert!(matches!(
        StoredJudgement::decode(&bytes[..bytes.len() - 1]),
        Err(DecodeError::Bytes(_))
    ));
}

#[test]
fn a_key_this_build_does_not_write_is_refused() {
    let record = approve(1, &judge("ann"), Visibility::Public, cr_passed(1, "f"));
    let bytes = edited(&record, &[], |root| {
        root.insert("extra".into(), Canon::Bool(true));
    });
    assert_eq!(
        StoredJudgement::decode(&bytes),
        Err(DecodeError::NotRecanonical)
    );
}

#[test]
fn a_default_policy_written_out_is_refused() {
    let record = approve(1, &judge("ann"), Visibility::Public, cs_passed(1, "c", "h"));
    let bytes = edited(&record, &["body", "attestation"], |attestation| {
        attestation.insert("policy".into(), GatePolicy::default().to_canon());
    });
    assert_eq!(
        StoredJudgement::decode(&bytes),
        Err(DecodeError::NotRecanonical)
    );
}

#[test]
fn a_kind_this_build_does_not_know_is_refused() {
    let record = approve(1, &judge("ann"), Visibility::Public, cr_passed(1, "f"));
    let bytes = edited(&record, &["body"], |body| {
        body.insert("kind".into(), Canon::str("endorse"));
    });
    assert!(matches!(
        StoredJudgement::decode(&bytes),
        Err(DecodeError::Refused { .. })
    ));
}

#[test]
fn a_missing_part_is_named() {
    let record = approve(1, &judge("ann"), Visibility::Public, cr_passed(1, "f"));
    let bytes = edited(&record, &[], |root| {
        root.remove("judge");
    });
    assert_eq!(
        StoredJudgement::decode(&bytes),
        Err(DecodeError::Missing {
            at: "record".into(),
            key: "judge".into()
        })
    );
}

#[test]
fn an_object_of_another_version_is_refused() {
    let record = approve(1, &judge("ann"), Visibility::Public, cr_passed(1, "f"));
    let bytes = edited(&record, &["body", "object"], |object| {
        object.insert("content".into(), hash_of("other").to_canon());
    });
    assert!(matches!(
        StoredJudgement::decode(&bytes),
        Err(DecodeError::Inconsistent(_))
    ));
}

/// A CA proof for item 5 that records `change`.
fn ca_recording(change: CaChange) -> Passed {
    let Attestation::Ca {
        item,
        design,
        revision,
        tests,
        ..
    } = ca_passed(5, "i").attestation().clone()
    else {
        panic!("a CA");
    };
    Passed::for_test(Attestation::Ca {
        item,
        design,
        revision,
        tests,
        change,
    })
}

fn change_of(record: &StoredJudgement) -> CaChange {
    let JudgementKind::Approve { passed, .. } = record.kind() else {
        panic!("an approval");
    };
    passed.attestation().change().expect("a CA").clone()
}

#[test]
fn what_a_ca_records_of_its_change_is_read_back_case_by_case() {
    use crate::domain::integration::{ChangeFingerprint, FingerprintUnavailable, Hunk, HunkScheme};
    let ann = judge("ann");
    let rev = || Revision::Commit(RevisionId::from_validated("b1".into()).unwrap());
    let fingerprint = ChangeFingerprint::of_hunks(
        rev(),
        rev(),
        &[Hunk::new(
            "src/a.rs".parse().unwrap(),
            vec![],
            vec!["let timeout = Duration::from_secs(30);".to_owned()],
        )],
        HunkScheme::CURRENT,
    )
    .unwrap();
    let cases = [
        CaChange::NotRecorded,
        CaChange::NothingToFingerprint,
        CaChange::Fingerprinted(fingerprint),
    ]
    .into_iter()
    .chain(FingerprintUnavailable::ALL.map(CaChange::NotFingerprinted));
    for (n, change) in (1u128..).zip(cases) {
        let stored = round_trip(approve(
            n,
            &ann,
            Visibility::Public,
            ca_recording(change.clone()),
        ));
        assert_eq!(change_of(&stored), change);
    }
}

#[test]
fn a_ca_with_no_change_key_is_one_that_recorded_none() {
    let record = approve(
        1,
        &judge("ann"),
        Visibility::Public,
        ca_recording(CaChange::NotRecorded),
    );
    let stored = round_trip(record);
    assert_eq!(change_of(&stored), CaChange::NotRecorded);
}

#[test]
fn a_change_of_a_kind_or_a_reason_this_build_does_not_know_is_refused() {
    let record = approve(
        1,
        &judge("ann"),
        Visibility::Public,
        ca_recording(CaChange::NotFingerprinted(FingerprintUnavailable::Shallow)),
    );
    let edit = |key: &str, value: &str| {
        edited(&record, &["body", "attestation", "change"], |change| {
            change.insert(key.into(), Canon::str(value));
        })
    };
    for bytes in [edit("kind", "partial"), edit("why", "elsewhere")] {
        assert!(matches!(
            StoredJudgement::decode(&bytes),
            Err(DecodeError::Refused { .. })
        ));
    }
}

#[test]
fn a_cm_names_the_cs_it_stands_on_as_an_event_or_as_a_judgement() {
    let ann = judge("ann");
    let on_event = round_trip(approve(
        1,
        &ann,
        Visibility::Public,
        cm_passed(1, "h", "cs"),
    ));
    let on_judgement = round_trip(approve(
        2,
        &ann,
        Visibility::Public,
        cm_passed_on(1, "h", jid(9)),
    ));
    let cs_of = |record: &StoredJudgement| match record.kind() {
        JudgementKind::Approve { passed, .. } => match passed.attestation() {
            Attestation::Cm { cs, .. } => *cs,
            other => panic!("a CM, got {other:?}"),
        },
        other => panic!("an approval, got {other:?}"),
    };
    assert!(matches!(cs_of(&on_event), CsRef::Event(_)));
    assert_eq!(cs_of(&on_judgement), CsRef::Judgement(jid(9)));
}

#[test]
fn an_adopted_measurement_is_refused() {
    let record = adopt(1);
    let bytes = edited(
        &record,
        &["body", "adoptable", "attestation"],
        |attestation| {
            attestation.insert("gate".into(), Canon::str(Gate::Cm.name()));
        },
    );
    assert!(matches!(
        StoredJudgement::decode(&bytes),
        Err(DecodeError::Refused { .. })
    ));
}
