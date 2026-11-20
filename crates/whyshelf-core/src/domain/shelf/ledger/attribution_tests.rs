//! Tests of who ran the command, kept beside the actor of a record (`ran_by`): a
//! record with none has the form, and the hash, it had before the key existed; one
//! that has it is read and written back as it is; a `ran_by` that is the actor is no
//! record; and no judgement reads it.

use std::collections::BTreeMap;

use crate::domain::actor::{Actor, Attribution};
use crate::domain::canon::Canon;
use crate::domain::error::ValueError;
use crate::domain::hash::ContentHash;
use crate::domain::id::DocId;
use crate::domain::shelf::fixtures_tests::{
    actor, approve, docid, fr, jid, ledger, shelf, sp_testing,
};
use crate::domain::shelf::freshness::assess;
use crate::domain::shelf::stats;

use super::*;

const SEEN: &str = "sha256:0000000000000000000000000000000000000000000000000000000000000002";
const EARLIER: &str = "sha256:0000000000000000000000000000000000000000000000000000000000000003";
const SUBJECT: &str = "01a1c332-342c-7260-9bfe-75a7d05fe5aa";

/// A record of each kind (a decision of each verb) as an earlier build wrote it: no
/// `ran_by`, its actor `Alice`, and the hash of its canonical form as that build
/// made it.
struct Written {
    name: &'static str,
    canon: Canon,
    hash: &'static str,
}

fn base(id: &str, kind: &str) -> Vec<(&'static str, Canon)> {
    vec![
        ("v", Canon::Int(1)),
        ("kind", Canon::str(kind)),
        ("id", Canon::str(id)),
        ("actor", Canon::str("Alice")),
        ("at", Canon::Int(1_794_284_172_507)),
    ]
}

fn with(mut entries: Vec<(&'static str, Canon)>, more: Vec<(&'static str, Canon)>) -> Canon {
    entries.extend(more);
    Canon::map(entries)
}

/// The hashes were taken from the code before the key existed, by reading each form
/// and hashing the record read (`Record::hash`); they are the form's, so they never
/// change with the code.
fn written() -> Vec<Written> {
    let reason = Canon::str("a reason");
    vec![
        Written {
            name: "approve",
            canon: with(
                base("01a1c3ab-b4ed-73a3-b256-57a0410ebfa4", "decision"),
                vec![
                    ("verb", Canon::str("approve")),
                    ("subject", Canon::str(SUBJECT)),
                    ("reason", reason.clone()),
                    ("seen", Canon::str(SEEN)),
                    ("frame", Canon::list([])),
                    ("expires", Canon::Null),
                ],
            ),
            hash: "sha256:abba3f36a18961fc877683d7726b1651cd215a6cea7fcfb06fe224f25fc03b60",
        },
        Written {
            name: "deny",
            canon: with(
                base("01a1c3ab-b4fc-77b6-af1c-5a1eafee7ce9", "decision"),
                vec![
                    ("verb", Canon::str("deny")),
                    ("subject", Canon::str(SUBJECT)),
                    ("reason", Canon::Null),
                    ("seen", Canon::str(SEEN)),
                ],
            ),
            hash: "sha256:ebd2b258e2aa070ffbc907419c70451591d0380a00bd1a063d678f1856195dc3",
        },
        Written {
            name: "deprecate",
            canon: with(
                base("01a1c3ab-b50b-759f-9802-d5f15a787f6c", "decision"),
                vec![
                    ("verb", Canon::str("deprecate")),
                    ("subject", Canon::str(SUBJECT)),
                    ("reason", reason.clone()),
                    ("seen", Canon::str(SEEN)),
                ],
            ),
            hash: "sha256:7553e37b8e5532c84217f88b2a1ce828c92945e80c932eb319dba63094ed0b0e",
        },
        Written {
            name: "retire",
            canon: with(
                base("01a1c3ab-b51a-7d02-8a5e-3e6f2d9a1c40", "decision"),
                vec![
                    ("verb", Canon::str("retire")),
                    ("subject", Canon::str(SUBJECT)),
                    ("reason", reason.clone()),
                    (
                        "seen",
                        Canon::map([("state", Canon::str("at")), ("hash", Canon::str(SEEN))]),
                    ),
                ],
            ),
            hash: "sha256:341c239a6d0e4b9a6245480ed9d705e19227cd614108028b8f851f4d21a99a6d",
        },
        Written {
            name: "revocation",
            canon: with(
                base("01a1c3ab-b529-7a11-9c4d-1b2f3e4d5c6b", "revocation"),
                vec![
                    (
                        "targets",
                        Canon::list([Canon::str("01a1c3ab-b4ed-73a3-b256-57a0410ebfa4")]),
                    ),
                    ("reason", reason.clone()),
                ],
            ),
            hash: "sha256:5171910c4867458b74a33ecd98503c281d65996f8c78d91ecdd41b94548c4434",
        },
        Written {
            name: "same_meaning",
            canon: with(
                base("01a1c3ab-b538-7b22-8d5e-2c3a4f5e6d7c", "same_meaning"),
                vec![
                    (
                        "subject",
                        Canon::map([("kind", Canon::str("doc")), ("id", Canon::str(SUBJECT))]),
                    ),
                    ("from", Canon::list([Canon::str(EARLIER)])),
                    ("to", Canon::str(SEEN)),
                    ("reason", reason.clone()),
                ],
            ),
            hash: "sha256:868abea5aac11ba5b2b34e7fd7707f1d6de4b25f930eee03e8c4c4a31af9f24d",
        },
        Written {
            name: "moved",
            canon: with(
                base("01a1c3ab-b547-7c33-9e6f-3d4b5a6f7e8d", "moved"),
                vec![
                    ("space", Canon::str("file")),
                    ("from", Canon::str("src/old.rs")),
                    ("to", Canon::str("src/new.rs")),
                    ("reason", reason.clone()),
                ],
            ),
            hash: "sha256:6f2a0878bc75186f2f6144c1eb719dd8167e42f242b4355789433a8084e5a6bd",
        },
        Written {
            name: "exempt",
            canon: with(
                base("01a1c3ab-b556-7d44-8f70-4e5c6b7a8f9e", "exempt"),
                vec![
                    ("subject", Canon::str(SUBJECT)),
                    ("book", Canon::str("finding")),
                    ("link", Canon::str("grounds")),
                    ("reason", reason),
                ],
            ),
            hash: "sha256:6f368eeb1220c865f794291fc87f52df2dbf98f7d73a96ae5ea8806fd7e5d86e",
        },
    ]
}

fn map_mut(canon: &mut Canon) -> &mut BTreeMap<String, Canon> {
    match canon {
        Canon::Map(map) => map,
        other => panic!("not a map: {other:?}"),
    }
}

/// `canon` with `ran_by` set to `value`.
fn with_ran_by_value(canon: &Canon, value: Canon) -> Canon {
    let mut canon = canon.clone();
    map_mut(&mut canon).insert("ran_by".to_owned(), value);
    canon
}

fn named(text: &str) -> Actor {
    Actor::new(text).unwrap()
}

/// The record whose form is `canon` with the name `ran_by` set.
fn ran_by_bot(canon: &Canon) -> Record {
    Record::from_canon(&with_ran_by_value(canon, Canon::str("bot"))).unwrap()
}

fn attribution_of(record: &Record) -> &Attribution {
    match record {
        Record::Decision(d) => d.attribution(),
        Record::Revocation(r) => &r.by,
        Record::SameMeaning(s) => s.attribution(),
        Record::Moved(m) => m.attribution(),
        Record::Exempt(e) => e.attribution(),
    }
}

fn ran_by_of(record: &Record) -> Option<&str> {
    attribution_of(record).ran_by().map(Actor::as_str)
}

#[test]
fn a_record_with_no_ran_by_is_read_and_written_back_as_it_was_with_the_hash_it_had() {
    for one in written() {
        let record = Record::from_canon(&one.canon).unwrap_or_else(|e| panic!("{}: {e}", one.name));
        assert_eq!(ran_by_of(&record), None, "{}", one.name);
        assert_eq!(attribution_of(&record).actor().as_str(), "Alice");
        assert_eq!(record.to_canon(), one.canon, "{}", one.name);
        assert_eq!(record.hash().hash().to_string(), one.hash, "{}", one.name);
    }
}

#[test]
fn a_ran_by_of_every_kind_is_read_and_written_back_as_it_is_and_tells_its_hash() {
    for one in written() {
        let record = ran_by_bot(&one.canon);
        assert_eq!(ran_by_of(&record), Some("bot"), "{}", one.name);
        assert_eq!(attribution_of(&record).actor().as_str(), "Alice");
        assert_eq!(
            record.to_canon(),
            with_ran_by_value(&one.canon, Canon::str("bot")),
            "{}",
            one.name
        );
        assert_ne!(record.hash().hash().to_string(), one.hash, "{}", one.name);
    }
}

#[test]
fn a_record_made_here_writes_ran_by_only_when_it_is_not_the_actor() {
    let targets = || NonEmptySet::from_set(BTreeSet::from([jid(1)])).unwrap();
    let revocation = |by: Attribution| {
        Record::Revocation(Revocation::new(
            jid(2),
            targets(),
            by,
            JudgedAt::new(10),
            NonBlank::new("wrong").unwrap(),
        ))
    };
    let ana = named("ana");
    let key_of = |record: &Record| map_of(&record.to_canon()).contains_key("ran_by");

    assert!(!key_of(&revocation(Attribution::one(ana.clone()))));
    assert!(!key_of(&revocation(Attribution::new(
        ana.clone(),
        Some(ana.clone())
    ))));
    let ran_for = revocation(Attribution::new(ana.clone(), Some(named("bot"))));
    assert!(key_of(&ran_for));
    assert_eq!(ran_by_of(&ran_for), Some("bot"));

    // The same record with and without the key are two records.
    assert_ne!(
        ran_for.hash(),
        revocation(Attribution::one(ana.clone())).hash()
    );
}

fn map_of(canon: &Canon) -> &BTreeMap<String, Canon> {
    match canon {
        Canon::Map(map) => map,
        other => panic!("not a map: {other:?}"),
    }
}

#[test]
fn a_ran_by_that_is_the_actor_is_no_record_of_any_kind() {
    for one in written() {
        // As it is, and as `Actor::new` reads a name (trimmed).
        for same in ["Alice", "  Alice "] {
            let canon = with_ran_by_value(&one.canon, Canon::str(same));
            assert_eq!(
                Record::from_canon(&canon),
                Err(RecordProblem::RanByIsActor {
                    name: "Alice".to_owned()
                }),
                "{}: {same:?}",
                one.name
            );
        }
    }
}

#[test]
fn a_ran_by_that_is_no_name_or_no_text_is_no_record() {
    for one in written() {
        for (value, why) in [
            ("", ValueError::ActorEmpty),
            ("   ", ValueError::ActorEmpty),
            ("a\nb", ValueError::ActorBadShape),
            (&"a".repeat(129), ValueError::ActorBadShape),
        ] {
            let canon = with_ran_by_value(&one.canon, Canon::str(value));
            match Record::from_canon(&canon) {
                Err(RecordProblem::BadValue { key, source }) => {
                    assert_eq!(key, "ran_by", "{}", one.name);
                    assert_eq!(source, why, "{}: {value:?}", one.name);
                }
                other => panic!("{}: {value:?}: {other:?}", one.name),
            }
        }
        for value in [Canon::Null, Canon::Int(1), Canon::list([])] {
            let canon = with_ran_by_value(&one.canon, value.clone());
            assert_eq!(
                Record::from_canon(&canon),
                Err(RecordProblem::WrongType {
                    key: "ran_by".to_owned(),
                    expected: "a text"
                }),
                "{}: {value:?}",
                one.name
            );
        }
    }
}

#[test]
fn two_records_of_one_id_that_differ_only_in_ran_by_conflict() {
    let s = shelf(vec![fr(1, &[])]);
    let alone = approve(&s, &ledger(vec![]), 1, 1001, 10, None);
    let ran = ran_by_bot(&alone.to_canon());
    assert_ne!(alone.hash(), ran.hash());

    let both = ledger(vec![alone.clone(), ran]);
    assert!(matches!(
        both.problems(),
        [LedgerProblem::Conflict { id, .. }] if *id == jid(1001)
    ));
    assert_eq!(both.status_of(docid(1)), DeclaredStatus::Pending);
    // The same record twice is one record, with the key as without it.
    let ran = ran_by_bot(&alone.to_canon());
    assert!(ledger(vec![ran.clone(), ran]).problems().is_empty());
}

/// The ledger of `records` as written, and the same with each record's `ran_by` set
/// to `bot`.
fn alone_and_ran_by_bot(records: Vec<Record>) -> (Ledger, Ledger) {
    let ran: Vec<Record> = records.iter().map(|r| ran_by_bot(&r.to_canon())).collect();
    (ledger(records), ledger(ran))
}

#[test]
fn no_judgement_reads_ran_by() {
    let s = shelf(vec![fr(1, &[]), sp_testing(2, &["FR-1"], &["t.rs"])]);
    let none = ledger(vec![]);
    let first = approve(&s, &none, 1, 1001, 10, None);
    let again = approve(&s, &ledger(vec![first.clone()]), 1, 1002, 20, None);
    let (alone, ran) = alone_and_ran_by_bot(vec![first, again]);

    for subject in [docid(1), docid(2)] {
        assert_eq!(alone.status_of(subject), ran.status_of(subject));
        assert_eq!(
            alone.standing_decision(subject).map(Decision::id),
            ran.standing_decision(subject).map(Decision::id)
        );
    }
    assert_eq!(alone.problems(), ran.problems());
    let at = JudgedAt::new(100);
    assert_eq!(assess(&s, &alone, at), assess(&s, &ran, at));
    assert_eq!(stats::of(&alone, &s, None), stats::of(&ran, &s, None));

    // What a new approval would be beside the readers: by the actor who read it, by
    // the one who ran the command, and by another.
    let frame = frame_approve(&s, &alone, docid(1)).unwrap();
    for by in [actor(), named("bot"), named("someone")] {
        let asked = Asked::frame(&frame, &by, None, JudgedAt::new(30));
        assert_eq!(
            alone.kind_of(&asked, Before::Now),
            ran.kind_of(&asked, Before::Now),
            "{by}"
        );
    }
    // The one who ran the command did not read it.
    let by = named("bot");
    let asked = Asked::frame(&frame, &by, None, JudgedAt::new(30));
    assert!(
        matches!(
            ran.kind_of(&asked, Before::Now),
            ApprovalKind::SecondReading { .. }
        ),
        "the name that ran the command is no reader"
    );
}

#[test]
fn who_ran_the_command_is_no_name_that_declared_the_edit() {
    let id: DocId = SUBJECT.parse().unwrap();
    let hash = |text: &str| DocHash::restore(text.parse::<ContentHash>().unwrap());
    let edge = || SameMeaningOf::Doc {
        id,
        from: NonEmptySet::new(hash(EARLIER), []),
        to: hash(SEEN),
    };
    let declared = written()
        .into_iter()
        .find(|one| one.name == "same_meaning")
        .unwrap();
    let (alone, ran) = alone_and_ran_by_bot(vec![Record::from_canon(&declared.canon).unwrap()]);

    // The actor declared it already, with or without who ran the command.
    for ledger in [&alone, &ran] {
        assert!(matches!(
            ledger.not_declared_by(edge(), &named("Alice")),
            Err(SameMeaningRefusal::AlreadyDeclaredBy { .. })
        ));
    }
    // Who ran the command declared nothing.
    assert!(ran.not_declared_by(edge(), &named("bot")).is_ok());
    assert!(alone.not_declared_by(edge(), &named("bot")).is_ok());
}
