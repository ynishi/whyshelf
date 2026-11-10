//! Tests of the pins of artifacts, the addresses and the revision of an approval
//! (rules A2 and A5), and of reading a record of the first version back unchanged.

use std::collections::BTreeMap;

use crate::domain::canon::Canon;
use crate::domain::shelf::artifact::{
    ArtifactHash, ArtifactReading, ArtifactReadings, ContentAddress, Revision,
};
use crate::domain::shelf::fixtures_tests::{
    actor, approve, docid, file_artifact, file_space, fr, jid, ledger, name, shelf, shelf_with,
    sp_testing,
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
    // What the adapter answered for the artifacts, with an address for each that is
    // there (one for b.rs too, which was pinned as absent and keeps none).
    let artifacts: ArtifactReadings = ["tests/a.rs", "tests/b.rs"]
        .into_iter()
        .map(|at| {
            let mut reading = ArtifactReading::of_bytes(b"a", &Default::default(), None);
            reading.set_address(address(&format!("blob-of-{}", at.replace('/', "-"))));
            (file_space(), name(at), reading)
        })
        .collect();
    let frame = frame.addressed(
        &addresses,
        &artifacts,
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
                p.name().as_str(),
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
        Err(RecordProblem::Invariant(DecisionProblem::ArtifactPinsTwice { artifact, .. }))
            if artifact == file_artifact("tests/a.rs")
    ));
}

/// The pin of `tests/a.rs` in an approval, as a record of this version writes it.
fn pin_of_a(canon: &Canon) -> BTreeMap<String, Canon> {
    let Some(Canon::List(items)) = map_of(canon).get("artifacts") else {
        panic!("artifacts");
    };
    items
        .iter()
        .map(|i| map_of(i).clone())
        .find(|m| m.get("name") == Some(&Canon::str("tests/a.rs")))
        .expect("the pin of tests/a.rs")
}

#[test]
fn a_new_pin_writes_its_space_and_its_name() {
    let canon = addressed_approval().to_canon();
    let pin = pin_of_a(&canon);
    assert_eq!(pin.get("space"), Some(&Canon::str("file")));
    assert_eq!(pin.get("name"), Some(&Canon::str("tests/a.rs")));
    assert!(!pin.contains_key("path"));
}

#[test]
fn a_stored_pin_without_a_space_reads_as_a_file_and_its_record_is_unchanged() {
    // The record as one written before spaces existed: `path` in place of `space`
    // and `name`.
    let mut stored = addressed_approval().to_canon();
    if let Canon::Map(map) = &mut stored
        && let Some(Canon::List(items)) = map.get_mut("artifacts")
    {
        for item in items {
            if let Canon::Map(pin) = item {
                pin.remove("space");
                let name = pin.remove("name").expect("a name");
                pin.insert("path".to_owned(), name);
            }
        }
    }
    let record = Record::from_canon(&stored).unwrap();
    let Record::Decision(decision) = &record else {
        panic!("a decision");
    };
    let DecisionVerb::Approve { artifacts, .. } = decision.verb() else {
        panic!("an approval");
    };
    let read: Vec<ArtifactKey> = artifacts.iter().map(|p| p.artifact().clone()).collect();
    assert_eq!(
        read,
        [file_artifact("tests/a.rs"), file_artifact("tests/b.rs")]
    );
    // Written again, it is the form it was stored in, and so its hash.
    assert_eq!(record.to_canon(), stored);
    assert_eq!(
        record.hash(),
        RecordHash(ContentHash::of(HashKind::ShelfJudgement, &stored))
    );
    // And it means what the record of this version means.
    let now = addressed_approval();
    let Record::Decision(made) = &now else {
        panic!("a decision");
    };
    let DecisionVerb::Approve {
        artifacts: made, ..
    } = made.verb()
    else {
        panic!("an approval");
    };
    assert!(
        artifacts
            .iter()
            .zip(made)
            .all(|(a, b)| a.artifact() == b.artifact()
                && a.seen() == b.seen()
                && a.comparison() == b.comparison())
    );
}

#[test]
fn a_pin_with_both_forms_or_neither_is_refused() {
    let mut both = addressed_approval().to_canon();
    let mut neither = both.clone();
    for (canon, edit) in [(&mut both, true), (&mut neither, false)] {
        if let Canon::Map(map) = canon
            && let Some(Canon::List(items)) = map.get_mut("artifacts")
            && let Some(Canon::Map(pin)) = items.first_mut()
        {
            if edit {
                pin.insert("path".to_owned(), Canon::str("tests/a.rs"));
            } else {
                pin.remove("space");
                pin.remove("name");
            }
        }
    }
    assert!(matches!(
        Record::from_canon(&both),
        Err(RecordProblem::ExtraKey { key }) if key == "path"
    ));
    assert!(matches!(
        Record::from_canon(&neither),
        Err(RecordProblem::MissingKey { .. })
    ));
}

#[test]
fn a_pin_of_a_space_that_is_no_space_is_refused() {
    let mut canon = addressed_approval().to_canon();
    if let Canon::Map(map) = &mut canon
        && let Some(Canon::List(items)) = map.get_mut("artifacts")
        && let Some(Canon::Map(pin)) = items.first_mut()
    {
        pin.insert("space".to_owned(), Canon::str("File"));
    }
    assert!(matches!(
        Record::from_canon(&canon),
        Err(RecordProblem::BadSpace(_))
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
