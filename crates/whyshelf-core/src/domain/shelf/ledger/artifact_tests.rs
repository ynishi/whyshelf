//! Tests of the pins of artifacts, the addresses and the revision of an approval
//! (rules A2 and A5), and of reading a record of the first version back unchanged.

use std::collections::BTreeMap;

use crate::domain::canon::Canon;
use crate::domain::shelf::artifact::{ArtifactHash, ContentAddress, Revision};
use crate::domain::shelf::fixtures_tests::{
    actor, approve, docid, fr, jid, ledger, path, shelf, shelf_with, sp_testing,
};

use super::*;

fn address(text: &str) -> ContentAddress {
    ContentAddress::from_validated(text.to_owned()).unwrap()
}

fn map_of(canon: &Canon) -> &BTreeMap<String, Canon> {
    match canon {
        Canon::Map(map) => map,
        other => panic!("not a map: {other:?}"),
    }
}

/// An approval of SP-2 (on FR-1, tested by a.rs that is there and b.rs that is
/// not), with every file addressed and a revision.
fn addressed_approval() -> Record {
    let s = shelf_with(
        vec![
            fr(1, &[]),
            sp_testing(2, &["FR-1"], &["tests/a.rs", "tests/b.rs"]),
        ],
        &[("tests/a.rs", Some(ArtifactHash::of_content(b"a")))],
    );
    let l = ledger(vec![approve(&s, &ledger(vec![]), 1, 1001, 10, None)]);
    let frame = frame_approve(&s, &l, docid(2)).unwrap();
    let addresses: BTreeMap<RelPath, ContentAddress> = frame
        .files()
        .into_iter()
        .map(|f| {
            let a = address(&format!("blob-of-{}", f.as_str().replace('/', "-")));
            (f, a)
        })
        .collect();
    let frame = frame.addressed(
        &addresses,
        Some(Revision::from_validated("c1".into()).unwrap()),
    );
    Record::Decision(
        Decision::approve(jid(1002), frame, None, actor(), JudgedAt::new(20), None).unwrap(),
    )
}

/// A record of the first version, as it is written in a file: no `artifacts`, no
/// `seen_address`, no `revision`, and a pin with no `address`.
fn first_version_approval() -> Canon {
    let pin = Canon::map([
        ("link", Canon::str("grounds")),
        ("target", Canon::str(docid(1).to_string())),
        (
            "key",
            Canon::map([
                ("book", Canon::str("finding")),
                (
                    "form",
                    Canon::map([
                        ("kind", Canon::str("prefixed")),
                        ("prefix", Canon::str("FR")),
                        (
                            "tail",
                            Canon::map([
                                ("kind", Canon::str("number")),
                                ("value", Canon::str("1")),
                            ]),
                        ),
                    ]),
                ),
            ]),
        ),
        (
            "hash",
            Canon::str("sha256:0000000000000000000000000000000000000000000000000000000000000001"),
        ),
    ]);
    Canon::map([
        ("v", Canon::Int(1)),
        ("kind", Canon::str("decision")),
        ("verb", Canon::str("approve")),
        ("id", Canon::str(jid(1001).to_string())),
        ("subject", Canon::str(docid(2).to_string())),
        ("actor", Canon::str("tester")),
        ("at", Canon::Int(10)),
        ("reason", Canon::Null),
        (
            "seen",
            Canon::str("sha256:0000000000000000000000000000000000000000000000000000000000000002"),
        ),
        ("frame", Canon::list([pin])),
        ("expires", Canon::Null),
    ])
}

#[test]
fn a_record_of_the_first_version_reads_back_unchanged() {
    let golden = first_version_approval();
    let record = Record::from_canon(&golden).unwrap();
    // Written again, it is the same form, so its hash (and every conflict check on
    // it) is what it was.
    assert_eq!(record.to_canon(), golden);
    let Record::Decision(decision) = &record else {
        panic!("a decision");
    };
    let DecisionVerb::Approve {
        frame,
        artifacts,
        seen_address,
        revision,
        ..
    } = decision.verb()
    else {
        panic!("an approval");
    };
    assert!(artifacts.is_empty());
    assert_eq!(*seen_address, None);
    assert_eq!(*revision, None);
    assert!(frame.iter().all(|pin| pin.address().is_none()));
}

#[test]
fn an_approval_without_artifacts_or_addresses_has_the_first_form() {
    let s = shelf(vec![fr(1, &[])]);
    let canon = approve(&s, &ledger(vec![]), 1, 1001, 10, None).to_canon();
    let keys: Vec<&str> = map_of(&canon).keys().map(String::as_str).collect();
    assert_eq!(
        keys,
        [
            "actor", "at", "expires", "frame", "id", "kind", "reason", "seen", "subject", "v",
            "verb"
        ]
    );
}

#[test]
fn an_approval_pins_every_artifact_of_a_depends_link_present_or_not() {
    let record = addressed_approval();
    let Record::Decision(decision) = &record else {
        panic!("a decision");
    };
    let DecisionVerb::Approve {
        frame,
        artifacts,
        seen_address,
        revision,
        ..
    } = decision.verb()
    else {
        panic!("an approval");
    };
    let pins: Vec<(&str, bool, bool)> = artifacts
        .iter()
        .map(|p| {
            (
                p.path().as_str(),
                matches!(p.seen(), PinnedArtifact::Present { .. }),
                p.seen().address().is_some(),
            )
        })
        .collect();
    assert_eq!(
        pins,
        [("tests/a.rs", true, true), ("tests/b.rs", false, false)]
    );
    assert_eq!(*seen_address, Some(address("blob-of-specs-sp-2.md")));
    assert_eq!(
        frame.iter().next().and_then(FramePin::address),
        Some(&address("blob-of-findings-fr-1.md"))
    );
    assert_eq!(revision.as_ref().map(Revision::as_str), Some("c1"));
}

#[test]
fn an_approval_with_artifacts_and_addresses_survives_its_canonical_form() {
    let record = addressed_approval();
    let canon = record.to_canon();
    let map = map_of(&canon);
    assert!(matches!(map.get("artifacts"), Some(Canon::List(items)) if items.len() == 2));
    assert_eq!(map.get("revision"), Some(&Canon::str("c1")));
    assert!(map.contains_key("seen_address"));
    let back = Record::from_canon(&canon).unwrap();
    assert_eq!(back, record);
    assert_eq!(back.hash(), record.hash());
}

#[test]
fn an_absent_artifact_with_an_address_is_refused() {
    let mut canon = addressed_approval().to_canon();
    let Canon::Map(map) = &mut canon else {
        panic!("a map");
    };
    let Some(Canon::List(items)) = map.get_mut("artifacts") else {
        panic!("artifacts");
    };
    let absent = items
        .iter_mut()
        .find(|i| map_of(i).get("state") == Some(&Canon::str("absent")))
        .unwrap();
    if let Canon::Map(pin) = absent {
        pin.insert("address".to_owned(), Canon::str("x"));
    }
    assert!(Record::from_canon(&canon).is_err());
}

#[test]
fn an_address_that_is_no_opaque_value_is_refused() {
    let mut canon = addressed_approval().to_canon();
    if let Canon::Map(map) = &mut canon {
        map.insert("revision".to_owned(), Canon::str("a b"));
    }
    assert!(matches!(
        Record::from_canon(&canon),
        Err(RecordProblem::BadOpaque { key, .. }) if key == "revision"
    ));
}

#[test]
fn a_frame_that_pins_one_path_of_one_link_twice_is_refused() {
    let mut canon = addressed_approval().to_canon();
    if let Canon::Map(map) = &mut canon
        && let Some(Canon::List(items)) = map.get_mut("artifacts")
    {
        // The present pin again, said to be absent.
        let mut twin = items[0].clone();
        if let Canon::Map(pin) = &mut twin {
            pin.insert("state".to_owned(), Canon::str("absent"));
            pin.insert("hash".to_owned(), Canon::Null);
            pin.remove("address");
        }
        items.push(twin);
    }
    assert!(matches!(
        Record::from_canon(&canon),
        Err(RecordProblem::Invariant(DecisionProblem::ArtifactPinsTwice { path: p, .. }))
            if p == path("tests/a.rs")
    ));
}

#[test]
fn an_artifact_that_is_not_there_is_pinned_and_refuses_nothing() {
    // Rule A2: no file is a fact an approval stands on; it is not an upstream that
    // does not stand.
    let s = shelf_with(
        vec![fr(1, &[]), sp_testing(2, &[], &["tests/none.rs"])],
        &[],
    );
    let frame = frame_approve(&s, &ledger(vec![]), docid(2)).unwrap();
    assert!(matches!(
        frame.artifacts().iter().next().map(ArtifactPin::seen),
        Some(PinnedArtifact::Absent)
    ));
}
