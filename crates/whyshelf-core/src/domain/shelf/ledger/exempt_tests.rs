//! Tests of the exemption as a record: its canonical form, and its withdrawal. What
//! one may be made of is tested with its rule, in `exemption`.

use std::collections::BTreeMap;

use crate::domain::canon::Canon;
use crate::domain::hash::{ContentHash, HashKind};
use crate::domain::shelf::fixtures_tests::{actor, docid, jid, ledger};

use super::*;

fn because() -> NonBlank {
    NonBlank::new("kept on purpose: it grounds no requirement").unwrap()
}

fn book(text: &str) -> BookName {
    BookName::new(text).unwrap()
}

fn link(text: &str) -> EntryName {
    EntryName::new(text).unwrap()
}

/// The exemption `judgement` of `doc` from `spec.grounds`, at `at`.
fn exempt_of(judgement: u128, doc: u128, at: i64) -> Exempt {
    Exempt::new(
        jid(judgement),
        ExemptOf::new(docid(doc), book("spec"), link("grounds")),
        actor(),
        JudgedAt::new(at),
        because(),
    )
}

fn exempt(judgement: u128, doc: u128, at: i64) -> Record {
    Record::Exempt(exempt_of(judgement, doc, at))
}

fn map_mut(canon: &mut Canon) -> &mut BTreeMap<String, Canon> {
    match canon {
        Canon::Map(map) => map,
        _ => panic!("a record is a map"),
    }
}

fn changed(change: impl FnOnce(&mut BTreeMap<String, Canon>)) -> Result<Record, RecordProblem> {
    let mut canon = exempt(2001, 1, 50).to_canon();
    change(map_mut(&mut canon));
    Record::from_canon(&canon)
}

// The canonical form.

#[test]
fn an_exemption_survives_its_canonical_form_with_exact_keys() {
    let record = exempt(2001, 1, 50);
    let canon = record.to_canon();
    let mut owned = canon.clone();
    let map = map_mut(&mut owned);
    let keys: Vec<&str> = map.keys().map(String::as_str).collect();
    assert_eq!(
        keys,
        [
            "actor", "at", "book", "id", "kind", "link", "reason", "subject", "v"
        ]
    );
    assert_eq!(map.get("v"), Some(&Canon::Int(1)));
    assert_eq!(map.get("kind"), Some(&Canon::str("exempt")));
    assert_eq!(map.get("subject"), Some(&Canon::str(docid(1).to_string())));
    assert_eq!(map.get("book"), Some(&Canon::str("spec")));
    assert_eq!(map.get("link"), Some(&Canon::str("grounds")));
    assert_eq!(map.get("at"), Some(&Canon::Int(50)));
    assert_eq!(
        map.get("reason"),
        Some(&Canon::str("kept on purpose: it grounds no requirement"))
    );

    let back = Record::from_canon(&canon).unwrap();
    assert_eq!(back, record);
    assert_eq!(back.to_canon(), canon);
    assert_eq!(
        back.hash(),
        RecordHash(ContentHash::of(HashKind::ShelfJudgement, &canon))
    );
    let Record::Exempt(back) = &back else {
        panic!("an exemption");
    };
    assert_eq!(back.id(), jid(2001));
    assert_eq!(back.of().doc(), docid(1));
    assert_eq!(back.of().book(), &book("spec"));
    assert_eq!(back.of().link(), &link("grounds"));
    assert_eq!(back.actor(), &actor());
    assert_eq!(back.at(), JudgedAt::new(50));
    assert_eq!(back.reason(), &because());
}

#[test]
fn a_canonical_form_of_an_exemption_with_a_key_too_many_or_too_few_is_refused() {
    assert!(matches!(
        changed(|m| {
            m.insert("extra".to_owned(), Canon::Null);
        }),
        Err(RecordProblem::ExtraKey { key }) if key == "extra"
    ));
    for key in ["subject", "book", "link", "reason"] {
        assert!(
            matches!(
                changed(|m| {
                    m.remove(key);
                }),
                Err(RecordProblem::MissingKey { key: k }) if k == key
            ),
            "{key}"
        );
    }
}

#[test]
fn a_canonical_form_of_an_exemption_with_a_bad_value_is_refused() {
    assert!(matches!(
        changed(|m| {
            m.insert("book".to_owned(), Canon::str("No Book"));
        }),
        Err(RecordProblem::BadName(_))
    ));
    assert!(matches!(
        changed(|m| {
            m.insert("link".to_owned(), Canon::str("key"));
        }),
        Err(RecordProblem::BadName(_))
    ));
    assert!(matches!(
        changed(|m| {
            m.insert("reason".to_owned(), Canon::str("  "));
        }),
        Err(RecordProblem::BadValue { key, .. }) if key == "reason"
    ));
    assert!(matches!(
        changed(|m| {
            m.insert("subject".to_owned(), Canon::str("not-an-id"));
        }),
        Err(RecordProblem::BadValue { key, .. }) if key == "subject"
    ));
}

#[test]
fn the_records_of_the_other_kinds_read_as_they_did() {
    // A ledger written before exemptions holds no `exempt` record; adding the kind
    // changes the form of no other.
    let revocation = Record::Revocation(Revocation::new(
        jid(3001),
        NonEmptySet::from_set([jid(2001)].into()).unwrap(),
        actor(),
        JudgedAt::new(60),
        NonBlank::new("a mistake").unwrap(),
    ));
    let canon = revocation.to_canon();
    assert_eq!(Record::from_canon(&canon).unwrap(), revocation);
    let mut owned = canon.clone();
    assert_eq!(
        map_mut(&mut owned).get("kind"),
        Some(&Canon::str("revocation"))
    );
}

// Withdrawing.

#[test]
fn an_exemption_is_withdrawn_by_its_identity_and_then_no_longer_stands() {
    let records = vec![exempt(2001, 1, 50)];
    let l = ledger(records.clone());
    assert_eq!(l.exemptions().count(), 1);
    assert_eq!(l.exemption(jid(2001)), Some(&exempt_of(2001, 1, 50)));
    assert_eq!(l.check_withdraw(jid(2001)), Ok(()));

    let mut withdrawn = records;
    withdrawn.push(Record::Revocation(Revocation::new(
        jid(3001),
        NonEmptySet::from_set([jid(2001)].into()).unwrap(),
        actor(),
        JudgedAt::new(60),
        NonBlank::new("it is grounded after all").unwrap(),
    )));
    let l = ledger(withdrawn);
    assert!(l.problems().is_empty(), "{:?}", l.problems());
    assert_eq!(l.exemptions().count(), 0);
    assert_eq!(l.exemption(jid(2001)), None);
    assert!(l.is_revoked(jid(2001)));
    assert_eq!(
        l.check_withdraw(jid(2001)),
        Err(WithdrawRefusal::AlreadyWithdrawn { id: jid(2001) })
    );
}
