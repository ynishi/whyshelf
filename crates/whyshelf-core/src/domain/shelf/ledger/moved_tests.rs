//! Tests of the declarations that an artifact moved: their canonical form, how the
//! ledger reads a pin through them, and the rule of a declaration
//! ([`moved_space`], [`frame_moved`]).

use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet};

use crate::domain::canon::Canon;
use crate::domain::hash::{ContentHash, HashKind};
use crate::domain::nonempty::NonEmptyVec;
use crate::domain::shelf::artifact::{
    ArtifactHash, ArtifactName, ArtifactReading, NamePrefix, PrefixMove,
};
use crate::domain::shelf::fixtures_tests::{
    actor, approve, by, docid, file_artifact, file_space, fr, jid, ledger, name, shelf_with,
    sp_testing,
};

use super::*;

fn because() -> NonBlank {
    NonBlank::new("the module was split").unwrap()
}

fn other_space() -> Space {
    Space::new("other").unwrap()
}

/// The declaration `judgement`, at `at`, that `from` moved to `to` in `space`.
fn moved_in(space: Space, judgement: u128, from: &str, to: &str, at: i64) -> Moved {
    Moved::new(
        jid(judgement),
        MovedOf::new(space, name(from), name(to)).unwrap(),
        by(),
        JudgedAt::new(at),
        because(),
    )
}

/// As [`moved_in`], in the space `file`, as a record.
fn moved(judgement: u128, from: &str, to: &str, at: i64) -> Record {
    Record::Moved(moved_in(file_space(), judgement, from, to, at))
}

fn map_mut(canon: &mut Canon) -> &mut BTreeMap<String, Canon> {
    match canon {
        Canon::Map(map) => map,
        _ => panic!("a record is a map"),
    }
}

/// The canonical form of a declaration, read back after `change`.
fn changed(change: impl FnOnce(&mut BTreeMap<String, Canon>)) -> Result<Record, RecordProblem> {
    let mut canon = moved(2001, "tests/a.rs", "tests/b.rs", 50).to_canon();
    change(map_mut(&mut canon));
    Record::from_canon(&canon)
}

fn ids(to: Option<MovedTo>) -> Option<(ArtifactKey, Vec<JudgementId>)> {
    to.map(|m| (m.artifact().clone(), m.declared().into_vec()))
}

// The canonical form.

#[test]
fn a_declaration_of_a_move_survives_its_canonical_form() {
    let record = moved(2001, "tests/a.rs", "tests/b.rs", 50);
    let canon = record.to_canon();
    let mut owned = canon.clone();
    let map = map_mut(&mut owned);
    let keys: Vec<&str> = map.keys().map(String::as_str).collect();
    assert_eq!(
        keys,
        [
            "actor", "at", "from", "id", "kind", "reason", "space", "to", "v"
        ]
    );
    assert_eq!(map.get("v"), Some(&Canon::Int(1)));
    assert_eq!(map.get("kind"), Some(&Canon::str("moved")));
    assert_eq!(map.get("space"), Some(&Canon::str("file")));
    assert_eq!(map.get("from"), Some(&Canon::str("tests/a.rs")));
    assert_eq!(map.get("to"), Some(&Canon::str("tests/b.rs")));
    assert_eq!(map.get("at"), Some(&Canon::Int(50)));
    assert_eq!(map.get("reason"), Some(&Canon::str("the module was split")));

    let back = Record::from_canon(&canon).unwrap();
    assert_eq!(back, record);
    assert_eq!(back.to_canon(), canon);
    assert_eq!(
        back.hash(),
        RecordHash(ContentHash::of(HashKind::ShelfJudgement, &canon))
    );
    let Record::Moved(back) = &back else {
        panic!("a declaration of a move");
    };
    assert_eq!(back.id(), jid(2001));
    assert_eq!(back.of().from(), file_artifact("tests/a.rs"));
    assert_eq!(back.of().to(), file_artifact("tests/b.rs"));
    assert_eq!(back.actor(), &actor());
    assert_eq!(back.at(), JudgedAt::new(50));
    assert_eq!(back.reason(), &because());
}

#[test]
fn a_move_in_another_space_survives_its_canonical_form() {
    let record = Record::Moved(moved_in(other_space(), 2001, "crate::a", "crate::b", 50));
    let back = Record::from_canon(&record.to_canon()).unwrap();
    assert_eq!(back, record);
}

#[test]
fn a_declaration_of_a_move_that_is_not_one_is_refused() {
    assert!(matches!(
        changed(|m| {
            m.insert("to".to_owned(), Canon::str("tests/a.rs"));
        }),
        Err(RecordProblem::Moved(MovedProblem::SameName { .. }))
    ));
    assert!(matches!(
        changed(|m| {
            m.remove("reason");
        }),
        Err(RecordProblem::MissingKey { .. })
    ));
    assert!(matches!(
        changed(|m| {
            m.insert("reason".to_owned(), Canon::str("  "));
        }),
        Err(RecordProblem::BadValue { .. })
    ));
    assert!(matches!(
        changed(|m| {
            m.insert("path".to_owned(), Canon::str("tests/a.rs"));
        }),
        Err(RecordProblem::ExtraKey { .. })
    ));
    assert!(matches!(
        changed(|m| {
            m.insert("space".to_owned(), Canon::str("File"));
        }),
        Err(RecordProblem::BadSpace(_))
    ));
    assert!(matches!(
        changed(|m| {
            m.insert("from".to_owned(), Canon::str(""));
        }),
        Err(RecordProblem::BadArtifactName(_))
    ));
    assert!(matches!(
        changed(|m| {
            m.insert("from".to_owned(), Canon::Int(1));
        }),
        Err(RecordProblem::WrongType { .. })
    ));
    assert_eq!(
        MovedOf::new(file_space(), name("a"), name("a")),
        Err(MovedProblem::SameName { name: name("a") })
    );
}

#[test]
fn a_ledger_of_old_records_reads_every_pin_as_written() {
    let s = shelf_with(
        vec![fr(1, &[]), sp_testing(2, &["FR-1"], &["tests/a.rs"])],
        &[("tests/a.rs", Some(ArtifactHash::of_content(b"a")))],
    );
    let first = approve(&s, &ledger(vec![]), 1, 1001, 10, None);
    let records = vec![
        first.clone(),
        approve(&s, &ledger(vec![first]), 2, 1002, 10, None),
    ];
    let l = ledger(records);
    let read = l.read_pins(docid(2));
    let pin = read.first().unwrap();
    assert_eq!(pin.moved(), None);
    assert_eq!(pin.read_as(), pin.pin().artifact());
    assert_eq!(
        l.moved_to(&file_artifact("tests/a.rs"), JudgedAt::new(0)),
        None
    );
}

// How a pin is read through moves.

#[test]
fn a_pin_made_before_a_move_is_read_as_the_new_name() {
    let l = ledger(vec![moved(2001, "tests/a.rs", "tests/b.rs", 50)]);
    assert_eq!(
        ids(l.moved_to(&file_artifact("tests/a.rs"), JudgedAt::new(10))),
        Some((file_artifact("tests/b.rs"), vec![jid(2001)]))
    );
    // At the time of the declaration too.
    assert!(
        l.moved_to(&file_artifact("tests/a.rs"), JudgedAt::new(50))
            .is_some()
    );
    // After it, as written; and another name is not moved.
    assert_eq!(
        l.moved_to(&file_artifact("tests/a.rs"), JudgedAt::new(51)),
        None
    );
    assert_eq!(
        l.moved_to(&file_artifact("tests/b.rs"), JudgedAt::new(10)),
        None
    );
}

#[test]
fn a_chain_of_moves_is_followed_in_order_of_time() {
    // Written in any order: b -> c is declared after a -> b.
    let l = ledger(vec![
        moved(2002, "tests/b.rs", "tests/c.rs", 60),
        moved(2001, "tests/a.rs", "tests/b.rs", 50),
    ]);
    assert_eq!(
        ids(l.moved_to(&file_artifact("tests/a.rs"), JudgedAt::new(10))),
        Some((file_artifact("tests/c.rs"), vec![jid(2001), jid(2002)]))
    );
    // A pin of b made between the two is read through the second only.
    assert_eq!(
        ids(l.moved_to(&file_artifact("tests/b.rs"), JudgedAt::new(55))),
        Some((file_artifact("tests/c.rs"), vec![jid(2002)]))
    );
    // A move declared before b -> a is not followed back in time.
    let l = ledger(vec![
        moved(2001, "tests/b.rs", "tests/c.rs", 40),
        moved(2002, "tests/a.rs", "tests/b.rs", 50),
    ]);
    assert_eq!(
        ids(l.moved_to(&file_artifact("tests/a.rs"), JudgedAt::new(10))),
        Some((file_artifact("tests/b.rs"), vec![jid(2002)]))
    );
}

#[test]
fn a_move_back_ends_at_the_name_pinned_and_leaves_the_middle_name_behind() {
    let l = ledger(vec![
        moved(2001, "tests/a.rs", "tests/b.rs", 50),
        moved(2002, "tests/b.rs", "tests/a.rs", 60),
    ]);
    let way = l.moved_to(&file_artifact("tests/a.rs"), JudgedAt::new(10));
    assert_eq!(
        ids(way.clone()),
        Some((file_artifact("tests/a.rs"), vec![jid(2001), jid(2002)]))
    );
    let from: Vec<ArtifactKey> = way.unwrap().moved_from().cloned().collect();
    assert_eq!(
        from,
        [file_artifact("tests/a.rs"), file_artifact("tests/b.rs")]
    );
    // A pin of b made while it was there is read as a.
    assert_eq!(
        ids(l.moved_to(&file_artifact("tests/b.rs"), JudgedAt::new(55))),
        Some((file_artifact("tests/a.rs"), vec![jid(2002)]))
    );
}

#[test]
fn of_two_moves_of_one_name_at_one_time_the_larger_identity_is_followed() {
    // In either order of the records.
    for records in [
        vec![
            moved(2001, "tests/a.rs", "tests/b.rs", 50),
            moved(2002, "tests/a.rs", "tests/c.rs", 50),
        ],
        vec![
            moved(2002, "tests/a.rs", "tests/c.rs", 50),
            moved(2001, "tests/a.rs", "tests/b.rs", 50),
        ],
    ] {
        let l = ledger(records);
        assert_eq!(
            ids(l.moved_to(&file_artifact("tests/a.rs"), JudgedAt::new(10))),
            Some((file_artifact("tests/c.rs"), vec![jid(2002)]))
        );
    }
    // A chain at one time is still followed.
    let l = ledger(vec![
        moved(2002, "tests/b.rs", "tests/c.rs", 50),
        moved(2001, "tests/a.rs", "tests/b.rs", 50),
    ]);
    assert_eq!(
        ids(l.moved_to(&file_artifact("tests/a.rs"), JudgedAt::new(10))),
        Some((file_artifact("tests/c.rs"), vec![jid(2001), jid(2002)]))
    );
}

#[test]
fn a_pin_of_a_record_written_before_spaces_is_read_through_a_move() {
    let s = shelf_with(
        vec![fr(1, &[]), sp_testing(2, &["FR-1"], &["tests/a.rs"])],
        &[("tests/a.rs", Some(ArtifactHash::of_content(b"a")))],
    );
    let first = approve(&s, &ledger(vec![]), 1, 1001, 10, None);
    let second = approve(&s, &ledger(vec![first.clone()]), 2, 1002, 10, None);
    // The approval of SP-2 as stored before spaces: `path` alone.
    let mut stored = second.to_canon();
    let Canon::Map(map) = &mut stored else {
        panic!("a record is a map");
    };
    let Some(Canon::List(items)) = map.get_mut("artifacts") else {
        panic!("artifacts");
    };
    for item in items {
        let Canon::Map(pin) = item else {
            panic!("a pin is a map");
        };
        pin.remove("space");
        let name = pin.remove("name").expect("a name");
        pin.insert("path".to_owned(), name);
    }
    let old = Record::from_canon(&stored).unwrap();
    assert_eq!(old.to_canon(), stored);
    let l = ledger(vec![
        first,
        old,
        moved(2001, "tests/a.rs", "tests/b.rs", 50),
    ]);
    let read = l.read_pins(docid(2));
    let [pin] = read.as_slice() else {
        panic!("one pin: {read:?}");
    };
    assert_eq!(pin.pin().artifact(), &file_artifact("tests/a.rs"));
    assert_eq!(pin.read_as(), &file_artifact("tests/b.rs"));
}

#[test]
fn a_move_of_another_space_or_withdrawn_moves_nothing() {
    let l = ledger(vec![Record::Moved(moved_in(
        other_space(),
        2001,
        "tests/a.rs",
        "tests/b.rs",
        50,
    ))]);
    assert_eq!(
        l.moved_to(&file_artifact("tests/a.rs"), JudgedAt::new(10)),
        None
    );

    let l = ledger(vec![
        moved(2001, "tests/a.rs", "tests/b.rs", 50),
        Record::Revocation(Revocation::new(
            jid(3001),
            NonEmptySet::from_set([jid(2001)].into()).unwrap(),
            by(),
            JudgedAt::new(60),
            NonBlank::new("it was a copy").unwrap(),
        )),
    ]);
    assert!(l.problems().is_empty(), "{:?}", l.problems());
    assert_eq!(
        l.moved_to(&file_artifact("tests/a.rs"), JudgedAt::new(10)),
        None
    );
    assert!(l.moved(jid(2001)).is_none());
    assert_eq!(
        l.check_withdraw(jid(2001)),
        Err(WithdrawRefusal::AlreadyWithdrawn { id: jid(2001) })
    );
    let l = ledger(vec![moved(2001, "tests/a.rs", "tests/b.rs", 50)]);
    assert_eq!(l.check_withdraw(jid(2001)), Ok(()));
}

#[test]
fn each_pin_is_read_at_the_time_of_its_approval_and_the_status_keeps_it_as_written() {
    let s = shelf_with(
        vec![fr(1, &[]), sp_testing(2, &["FR-1"], &["tests/a.rs"])],
        &[("tests/a.rs", Some(ArtifactHash::of_content(b"a")))],
    );
    let first = approve(&s, &ledger(vec![]), 1, 1001, 10, None);
    let second = approve(&s, &ledger(vec![first.clone()]), 2, 1002, 10, None);
    let l = ledger(vec![
        first,
        second,
        moved(2001, "tests/a.rs", "tests/b.rs", 50),
    ]);
    let read = l.read_pins(docid(2));
    let pin = read.first().unwrap();
    assert_eq!(pin.pin().artifact(), &file_artifact("tests/a.rs"));
    assert_eq!(pin.read_as(), &file_artifact("tests/b.rs"));
    assert_eq!(
        pin.moved().map(|m| m.declared().into_vec()),
        Some(vec![jid(2001)])
    );
    // The status and the record keep the pin as it was made.
    let DeclaredStatus::Approved { artifacts, .. } = l.status_of(docid(2)) else {
        panic!("approved");
    };
    assert_eq!(artifacts.first(), Some(pin.pin()));
    let Some(decision) = l.standing_decision(docid(2)) else {
        panic!("a decision");
    };
    let DecisionVerb::Approve { artifacts, .. } = decision.verb() else {
        panic!("an approval");
    };
    assert_eq!(artifacts.first(), Some(pin.pin()));
    // No approval stands about FR-9: nothing to read.
    assert!(l.read_pins(docid(9)).is_empty());
}

// The rule of a declaration.

fn spec_on(tests: &[&str], files: &[(&str, Option<ArtifactHash>)]) -> Shelf {
    shelf_with(vec![fr(1, &[]), sp_testing(2, &["FR-1"], tests)], files)
}

#[test]
fn the_space_of_a_move_is_the_one_a_pin_is_read_as_the_old_name_in() {
    // No approval: nothing is read as the name, whatever a link writes.
    assert_eq!(
        moved_space(&ledger(vec![]), &name("tests/a.rs")),
        Err(MovedRefusal::NotKnown {
            name: name("tests/a.rs")
        })
    );
    let then = spec_on(
        &["tests/a.rs"],
        &[("tests/a.rs", Some(ArtifactHash::of_content(b"a")))],
    );
    let first = approve(&then, &ledger(vec![]), 1, 1001, 10, None);
    let records = vec![
        first.clone(),
        approve(&then, &ledger(vec![first]), 2, 1002, 10, None),
    ];
    assert_eq!(
        moved_space(&ledger(records.clone()), &name("tests/a.rs")),
        Ok(file_space())
    );
    // After a -> b, the pin is read as b: b is the name to move on from, and a is
    // known no more (a second move of it would be a copy in two steps).
    let mut moved_once = records;
    moved_once.push(moved(2001, "tests/a.rs", "tests/b.rs", 50));
    let l = ledger(moved_once.clone());
    assert_eq!(moved_space(&l, &name("tests/b.rs")), Ok(file_space()));
    assert_eq!(
        moved_space(&l, &name("tests/a.rs")),
        Err(MovedRefusal::NotKnown {
            name: name("tests/a.rs")
        })
    );
    // After b -> a, a is known again: a move back.
    moved_once.push(moved(2002, "tests/b.rs", "tests/a.rs", 60));
    let l = ledger(moved_once);
    assert_eq!(moved_space(&l, &name("tests/a.rs")), Ok(file_space()));
    assert!(moved_space(&l, &name("tests/b.rs")).is_err());
}

#[test]
fn a_name_pinned_only_as_absent_after_a_move_cannot_be_moved_again() {
    let then = spec_on(
        &["tests/a.rs"],
        &[("tests/a.rs", Some(ArtifactHash::of_content(b"a")))],
    );
    let first = approve(&then, &ledger(vec![]), 1, 1001, 10, None);
    let mut records = vec![
        first.clone(),
        approve(&then, &ledger(vec![first]), 2, 1002, 10, None),
        moved(2001, "tests/a.rs", "tests/b.rs", 50),
    ];
    // SP-2 approved again after the move, its link still on a.rs, with nothing there.
    let now = spec_on(&["tests/a.rs"], &[]);
    let again = approve(&now, &ledger(records.clone()), 2, 1003, 60, None);
    records.push(again);
    let l = ledger(records);
    let read = l.read_pins(docid(2));
    assert_eq!(read.len(), 1);
    assert!(matches!(read[0].pin().seen(), PinnedArtifact::Absent));
    // a -> c would say that a moved twice: no pin of something there reads as a.
    assert_eq!(
        moved_space(&l, &name("tests/a.rs")),
        Err(MovedRefusal::NotKnown {
            name: name("tests/a.rs")
        })
    );
}

#[test]
fn a_move_stands_when_the_old_name_is_empty_and_the_new_one_is_not() {
    let readings = |old: ArtifactReading, new: ArtifactReading| {
        let mut r = ArtifactReadings::new();
        r.insert(file_space(), name("a"), old);
        r.insert(file_space(), name("b"), new);
        r
    };
    let there = || ArtifactReading::of_bytes(b"x", &Default::default(), None);
    let frame = |r: &ArtifactReadings| {
        frame_moved(MovedOf::new(file_space(), name("a"), name("b")).unwrap(), r)
    };
    let why = || NonBlank::new("offline").unwrap();

    let of = frame(&readings(ArtifactReading::absent(), there())).unwrap();
    assert_eq!(of.from(), file_artifact("a"));
    assert_eq!(of.to(), file_artifact("b"));
    // A thing with no bytes, there by its presence only, is there.
    assert!(
        frame(&readings(
            ArtifactReading::absent(),
            ArtifactReading::there(why())
        ))
        .is_ok()
    );

    assert!(matches!(
        frame(&readings(there(), there())),
        Err(MovedRefusal::FromPresent { .. })
    ));
    assert!(matches!(
        frame(&readings(ArtifactReading::cannot_tell(why()), there())),
        Err(MovedRefusal::FromUntold { .. })
    ));
    assert!(matches!(
        frame(&readings(ArtifactReading::not_a_name(why()), there())),
        Err(MovedRefusal::FromUntold {
            why: Untold::NotAName(_),
            ..
        })
    ));
    assert!(matches!(
        frame(&readings(
            ArtifactReading::absent(),
            ArtifactReading::absent()
        )),
        Err(MovedRefusal::ToAbsent { .. })
    ));
    assert!(matches!(
        frame(&readings(
            ArtifactReading::absent(),
            ArtifactReading::cannot_tell(why())
        )),
        Err(MovedRefusal::ToUntold { .. })
    ));
    // No answer is no answer that the old name is empty.
    assert!(matches!(
        frame(&ArtifactReadings::new()),
        Err(MovedRefusal::FromUntold {
            why: Untold::NoAnswer,
            ..
        })
    ));
    // The one rule of a move, as a refusal.
    assert_eq!(
        MovedOf::new(file_space(), name("a"), name("a")).map_err(MovedRefusal::from),
        Err(MovedRefusal::NotAMove(MovedProblem::SameName {
            name: name("a")
        }))
    );
}

// The names a move may be declared of, and those `moves` asks about.

/// FR-1, and SP-2 tested by `tests/a.rs` (there) and `tests/none.rs` (pinned as
/// absent), both approved at time 10.
fn approved_records() -> Vec<Record> {
    let then = spec_on(
        &["tests/a.rs", "tests/none.rs"],
        &[("tests/a.rs", Some(ArtifactHash::of_content(b"a")))],
    );
    let first = approve(&then, &ledger(vec![]), 1, 1001, 10, None);
    vec![
        first.clone(),
        approve(&then, &ledger(vec![first]), 2, 1002, 10, None),
    ]
}

/// The names [`PinnedNames::asks`] asks about, in order.
fn asked(l: &Ledger) -> Vec<ArtifactKey> {
    PinnedNames::of(l)
        .asks()
        .iter()
        .map(|(space, n, _)| ArtifactKey::new(space.clone(), n.clone()))
        .collect()
}

#[test]
fn the_names_asked_about_are_those_pins_of_something_there_are_read_as_now() {
    let records = approved_records();
    // A pin of a name as absent is not asked about: it held nothing to move.
    let l = ledger(records.clone());
    assert_eq!(asked(&l), [file_artifact("tests/a.rs")]);
    let pinned = PinnedNames::of(&l);
    let (_, pins) = pinned.names().next().unwrap();
    assert_eq!(pins.len(), 1);
    assert_eq!(pins.first().subject(), docid(2));
    assert_eq!(pins.first().written(), &file_artifact("tests/a.rs"));
    assert_eq!(pins.first().hash(), &ArtifactHash::of_content(b"a"));
    assert_eq!(pins.first().comparison(), &Comparison::Bytes);
    assert!(pins.first().content().is_some());

    // Moved and declared: the pin is asked about as the name it is read as, and the
    // old name is not asked about.
    let mut moved_once = records.clone();
    moved_once.push(moved(2001, "tests/a.rs", "tests/b.rs", 50));
    let l = ledger(moved_once.clone());
    assert_eq!(asked(&l), [file_artifact("tests/b.rs")]);
    let pinned = PinnedNames::of(&l);
    let (_, pins) = pinned.names().next().unwrap();
    assert_eq!(pins.first().written(), &file_artifact("tests/a.rs"));

    // The move withdrawn: the pin is read as written, and asked about so.
    moved_once.push(Record::Revocation(Revocation::new(
        jid(3001),
        NonEmptySet::from_set([jid(2001)].into()).unwrap(),
        by(),
        JudgedAt::new(60),
        NonBlank::new("it was a copy").unwrap(),
    )));
    let l = ledger(moved_once);
    assert!(l.problems().is_empty(), "{:?}", l.problems());
    assert_eq!(asked(&l), [file_artifact("tests/a.rs")]);
}

#[test]
fn a_name_is_gone_when_the_adapter_says_nothing_is_there() {
    let l = ledger(approved_records());
    let readings = |reading: Option<ArtifactReading>| {
        let mut r = ArtifactReadings::new();
        if let Some(reading) = reading {
            r.insert(file_space(), name("tests/a.rs"), reading);
        }
        r
    };
    let gone = |reading| {
        PinnedNames::of(&l)
            .gone(&readings(reading))
            .spaces()
            .flat_map(|(_, names)| names.into_iter().map(|n| n.artifact().clone()))
            .collect::<Vec<_>>()
    };
    assert_eq!(
        gone(Some(ArtifactReading::absent())),
        [file_artifact("tests/a.rs")]
    );
    let why = || NonBlank::new("offline").unwrap();
    // Something there, a name that cannot be told about now, and no answer: not gone.
    assert!(
        gone(Some(ArtifactReading::of_bytes(
            b"a",
            &Default::default(),
            None
        )))
        .is_empty()
    );
    assert!(gone(Some(ArtifactReading::there(why()))).is_empty());
    assert!(gone(Some(ArtifactReading::cannot_tell(why()))).is_empty());
    assert!(gone(None).is_empty());

    let gone = PinnedNames::of(&l).gone(&readings(Some(ArtifactReading::absent())));
    let (space, names) = gone.spaces().next().unwrap();
    assert_eq!(space, &file_space());
    assert_eq!(names[0].pinned_by(), BTreeSet::from([docid(2)]));
}

#[test]
fn a_pin_of_presence_only_has_no_content_to_look_for() {
    // The rule is the comparison's: one hash for everything there.
    for (comparison, has) in [
        (Comparison::Bytes, true),
        (crate::domain::shelf::fixtures_tests::lines(&["//"]), true),
        (Comparison::Presence, false),
    ] {
        let there = PinnedThere {
            subject: docid(2),
            written: file_artifact("a"),
            comparison: comparison.clone(),
            hash: comparison.fingerprint(b"a"),
            address: None,
            revision: None,
        };
        assert_eq!(there.content().is_some(), has, "{comparison:?}");
    }
}

#[test]
fn suggestions_keep_every_gone_name_and_put_candidates_in_one_order() {
    let mut records = approved_records();
    // SP-2 approved again, on two files.
    let then = spec_on(
        &["tests/a.rs", "tests/c.rs"],
        &[
            ("tests/a.rs", Some(ArtifactHash::of_content(b"a"))),
            ("tests/c.rs", Some(ArtifactHash::of_content(b"c"))),
        ],
    );
    records.push(approve(&then, &ledger(records.clone()), 2, 1003, 20, None));
    let l = ledger(records);
    let mut readings = ArtifactReadings::new();
    readings.insert(file_space(), name("tests/a.rs"), ArtifactReading::absent());
    readings.insert(file_space(), name("tests/c.rs"), ArtifactReading::absent());
    let gone = PinnedNames::of(&l).gone(&readings);

    let renamed = |n: &str, s: Option<u8>| {
        MoveCandidate::new(
            name(n),
            FoundBy::Renamed {
                similarity: s.map(|s| Similarity::new(s).unwrap()),
            },
            true,
        )
    };
    let same = |n: &str| MoveCandidate::new(name(n), FoundBy::SameContent, true);
    let answer = SpaceSuggestions::Suggested(BTreeMap::from([(
        name("tests/a.rs"),
        NameSuggestions {
            candidates: vec![
                renamed("z", Some(60)),
                renamed("v", None),
                renamed("y", Some(90)),
                renamed("w", Some(99)),
                same("w"),
                // The other gone name, and the name itself: never a candidate.
                renamed("tests/c.rs", Some(100)),
                same("tests/a.rs"),
            ],
            notes: Vec::new(),
        },
    )]));
    let out = gone.suggest(BTreeMap::from([(file_space(), answer)]));
    assert!(out.cannot_suggest.is_empty());
    let names: Vec<_> = out.absent.iter().map(|s| s.gone().name().clone()).collect();
    assert_eq!(names, [name("tests/a.rs"), name("tests/c.rs")]);
    let order: Vec<(&str, FoundBy)> = out.absent[0]
        .candidates()
        .iter()
        .map(|c| (c.name().as_str(), c.found()))
        .collect();
    assert_eq!(
        order,
        [
            ("w", FoundBy::SameContent),
            ("y", renamed("y", Some(90)).found()),
            ("z", renamed("z", Some(60)).found()),
            ("v", renamed("v", None).found()),
        ]
    );
    // A name the adapter gave nothing for is listed with none.
    assert!(out.absent[1].candidates().is_empty());

    // A space with no answer, or one that cannot suggest, keeps its names, with none.
    let gone = PinnedNames::of(&l).gone(&readings);
    let out = gone.suggest(BTreeMap::new());
    assert_eq!(out.cannot_suggest, BTreeSet::from([file_space()]));
    assert_eq!(out.absent.len(), 2);
    assert!(Similarity::new(101).is_none());
}

#[test]
fn a_name_read_in_two_spaces_names_the_spaces_in_words() {
    let refused = MovedRefusal::InSpaces {
        name: crate::domain::shelf::artifact::ArtifactName::new("a.rs").unwrap(),
        spaces: vec![
            file_space(),
            crate::domain::shelf::artifact::Space::new("rust-item").unwrap(),
        ],
    };
    assert_eq!(
        refused.to_string(),
        "pins are read as `a.rs` in more than one space: `file` and `rust-item`"
    );
}

// The order of the candidates, and the moves of a prefix they carry.

fn found(n: &str, by: FoundBy) -> MoveCandidate {
    MoveCandidate::new(name(n), by, true)
}

fn renamed_at(n: &str, percent: Option<u8>) -> MoveCandidate {
    found(
        n,
        FoundBy::Renamed {
            similarity: percent.map(|p| Similarity::new(p).unwrap()),
        },
    )
}

fn moving(from: &str, to: &str) -> PrefixMove {
    PrefixMove::new(NamePrefix::new(from).unwrap(), NamePrefix::new(to).unwrap()).unwrap()
}

/// The names of `names`, all approved on and all gone now.
fn gone_of(names: &[&str]) -> GoneNames {
    let files: Vec<(&str, Option<ArtifactHash>)> = names
        .iter()
        .map(|n| (*n, Some(ArtifactHash::of_content(n.as_bytes()))))
        .collect();
    let then = spec_on(names, &files);
    let l = crate::domain::shelf::fixtures_tests::approved(&then, &[1, 2]);
    let mut readings = ArtifactReadings::new();
    for n in names {
        readings.insert(file_space(), name(n), ArtifactReading::absent());
    }
    PinnedNames::of(&l).gone(&readings)
}

/// What the space `file` answers for the gone name `of`: these candidates.
fn answer_for(of: &str, candidates: Vec<MoveCandidate>) -> (ArtifactName, NameSuggestions) {
    (
        name(of),
        NameSuggestions {
            candidates,
            notes: Vec::new(),
        },
    )
}

fn suggested(answers: Vec<(ArtifactName, NameSuggestions)>) -> BTreeMap<Space, SpaceSuggestions> {
    BTreeMap::from([(
        file_space(),
        SpaceSuggestions::Suggested(answers.into_iter().collect()),
    )])
}

#[test]
fn candidates_are_in_the_order_of_how_they_were_found_and_not_of_their_names() {
    let in_order = [
        found("z", FoundBy::SameContent),
        renamed_at("z", Some(90)),
        renamed_at("a", Some(60)),
        renamed_at("a", None),
        found("a", FoundBy::SameEnd),
    ];
    for (at, one) in in_order.iter().enumerate() {
        for (later, other) in in_order.iter().enumerate() {
            assert_eq!(one.cmp_rank(other), at.cmp(&later), "{one:?} {other:?}");
        }
    }
    // Rank is how a name was found: the name and the move of a prefix are not in it.
    assert_eq!(
        found("a", FoundBy::SameEnd)
            .cmp_rank(&found("b", FoundBy::SameEnd).under(moving("p", "q"))),
        Ordering::Equal
    );
}

#[test]
fn a_candidate_carries_the_moves_of_a_prefix_it_was_given_in_order_and_once_each() {
    let plain = found("a/x", FoundBy::SameEnd);
    assert!(plain.prefixes().is_empty());
    let under = plain.clone().under(moving("a", "b"));
    assert_eq!(under.prefixes(), [moving("a", "b")]);
    assert_eq!(under.name(), plain.name());
    assert_eq!(under.found(), FoundBy::SameEnd);
    assert!(under.present());
    // A second move is kept after the first, and one given again is not added.
    let both = under
        .clone()
        .under(moving("a/x", "b/x"))
        .under(moving("a", "b"));
    assert_eq!(both.prefixes(), [moving("a", "b"), moving("a/x", "b/x")]);
}

#[test]
fn a_name_given_twice_is_one_candidate_and_it_has_the_prefixes_both_had() {
    let gone = gone_of(&["tests/a.rs", "tests/c.rs"]);
    let (to_b, to_d) = (moving("tests", "b"), moving("tests", "d"));
    let answers = suggested(vec![answer_for(
        "tests/a.rs",
        vec![
            // The same name by its end, then by its history: the better one is kept,
            // and the prefix the first had stays with it.
            found("b/a.rs", FoundBy::SameEnd).under(to_b.clone()),
            renamed_at("b/a.rs", Some(70)),
            // Two of one rank, each with a move of its own: one candidate, with both,
            // the earlier's first.
            found("d/a.rs", FoundBy::SameEnd).under(to_d.clone()),
            found("d/a.rs", FoundBy::SameEnd).under(to_b.clone()),
            // Two of one rank, and only the later has a prefix.
            found("e/a.rs", FoundBy::SameEnd),
            found("e/a.rs", FoundBy::SameEnd).under(to_b.clone()),
            // The better one has none, and the worse one has: the prefix is kept.
            found("f/a.rs", FoundBy::SameContent),
            found("f/a.rs", FoundBy::SameEnd).under(to_d.clone()),
            // Two that carry the same move, one of them another too: each once.
            found("g/a.rs", FoundBy::SameEnd).under(to_b.clone()),
            found("g/a.rs", FoundBy::SameEnd)
                .under(to_b.clone())
                .under(to_d.clone()),
            // The name itself and the other gone name are never candidates, of any kind.
            found("tests/a.rs", FoundBy::SameEnd),
            found("tests/c.rs", FoundBy::SameEnd).under(to_b.clone()),
        ],
    )]);
    let out = gone.suggest(answers);
    let got: Vec<(&str, FoundBy, &[PrefixMove])> = out.absent[0]
        .candidates()
        .iter()
        .map(|c| (c.name().as_str(), c.found(), c.prefixes()))
        .collect();
    assert_eq!(
        got,
        [
            ("f/a.rs", FoundBy::SameContent, &[to_d.clone()][..]),
            (
                "b/a.rs",
                renamed_at("b/a.rs", Some(70)).found(),
                &[to_b.clone()][..]
            ),
            (
                "d/a.rs",
                FoundBy::SameEnd,
                &[to_d.clone(), to_b.clone()][..]
            ),
            ("e/a.rs", FoundBy::SameEnd, &[to_b.clone()][..]),
            (
                "g/a.rs",
                FoundBy::SameEnd,
                &[to_b.clone(), to_d.clone()][..]
            ),
        ]
    );
    // The gone name nothing was said of has none.
    assert!(out.absent[1].candidates().is_empty());
}

#[test]
fn a_candidate_with_two_prefixes_counts_toward_both_for_the_gone_name_it_is_of() {
    let gone = gone_of(&["g/1", "g/2", "g/3"]);
    let (wide, narrow, lone) = (moving("a", "b"), moving("a/m", "b/m"), moving("c", "d"));
    let answers = suggested(vec![
        answer_for(
            "g/1",
            vec![
                // One candidate reached by two moves, and a second of the same name
                // under one of them: the name is one holder of each.
                found("b/m/1", FoundBy::SameEnd)
                    .under(wide.clone())
                    .under(narrow.clone()),
                found("b/m/1x", FoundBy::SameEnd).under(narrow.clone()),
            ],
        ),
        answer_for(
            "g/2",
            vec![
                found("b/m/2", FoundBy::SameEnd)
                    .under(wide.clone())
                    .under(narrow.clone()),
            ],
        ),
        // The third is held by `narrow` alone, and by `lone` which only it has.
        answer_for(
            "g/3",
            vec![
                found("b/m/3", FoundBy::SameEnd).under(narrow.clone()),
                found("d/3", FoundBy::SameEnd).under(lone),
            ],
        ),
    ]);
    let out = gone.suggest(answers);
    // `narrow` has three gone names, `wide` two: both are tried, the most held first;
    // `lone` has one only.
    assert_eq!(out.prefix_trials(), [narrow, wide]);
    assert_eq!(out.untried_prefixes, 0);
}

#[test]
fn a_prefix_is_tried_when_gone_names_share_it_the_most_first() {
    let gone = gone_of(&["g/1", "g/2", "g/3"]);
    let (once, twice, thrice) = (moving("a", "b"), moving("c", "d"), moving("e", "f"));
    let answers = suggested(vec![
        answer_for(
            "g/1",
            vec![
                found("b/1", FoundBy::SameEnd).under(once.clone()),
                found("d/1", FoundBy::SameEnd).under(twice.clone()),
                // Two candidates of one name under one prefix are one holder.
                found("d/1x", FoundBy::SameEnd).under(twice.clone()),
                found("f/1", FoundBy::SameEnd).under(thrice.clone()),
                // No prefix: nothing to try.
                found("z/1", FoundBy::SameEnd),
            ],
        ),
        answer_for(
            "g/2",
            vec![
                found("d/2", FoundBy::SameEnd).under(twice.clone()),
                found("f/2", FoundBy::SameEnd).under(thrice.clone()),
            ],
        ),
        answer_for(
            "g/3",
            vec![found("f/3", FoundBy::SameEnd).under(thrice.clone())],
        ),
    ]);
    let out = gone.suggest(answers);
    // `a` -> `b` is held by one gone name only, and two candidates of `g/1` under `c`
    // are still one name.
    assert_eq!(out.prefix_trials(), [thrice, twice]);
    assert_eq!(out.untried_prefixes, 0);
    assert!(out.prefixes.is_empty() && out.refused_prefixes.is_empty());

    // Of equal holders, by the move.
    let gone = gone_of(&["g/1", "g/2"]);
    let (late, early) = (moving("x", "y"), moving("m", "n"));
    let both = |at: &str| {
        vec![
            found(&format!("y/{at}"), FoundBy::SameEnd).under(late.clone()),
            found(&format!("n/{at}"), FoundBy::SameEnd).under(early.clone()),
        ]
    };
    let out = gone.suggest(suggested(vec![
        answer_for("g/1", both("1")),
        answer_for("g/2", both("2")),
    ]));
    assert_eq!(out.prefix_trials(), [early, late]);

    // No candidate carries a prefix: none is tried.
    let out = gone_of(&["g/1", "g/2"]).suggest(BTreeMap::new());
    assert!(out.prefix_trials().is_empty());
}

#[test]
fn the_prefixes_tried_are_bounded_and_the_rest_are_counted() {
    const SHARED: usize = 35;
    let gone = gone_of(&["g/1", "g/2", "g/3"]);
    let prefix = |i: usize| moving(&format!("from{i:02}"), &format!("to{i:02}"));
    let candidates = |skip: usize| -> Vec<MoveCandidate> {
        (0..SHARED)
            .map(|i| found(&format!("to{i:02}/x{skip}"), FoundBy::SameEnd).under(prefix(i)))
            .collect()
    };
    // The last of them is held by a third name: it is tried first, and the ones that
    // stay out are the last by the move of those held by two.
    let mut third = candidates(3);
    third.retain(|c| c.prefixes() == [prefix(SHARED - 1)]);
    let out = gone.suggest(suggested(vec![
        answer_for("g/1", candidates(1)),
        answer_for("g/2", candidates(2)),
        answer_for("g/3", third),
    ]));
    let tried = out.prefix_trials();
    assert_eq!(tried.len(), 32);
    assert_eq!(tried[0], prefix(SHARED - 1));
    assert_eq!(tried[1..], (0..31).map(prefix).collect::<Vec<_>>());
    assert_eq!(out.untried_prefixes, SHARED - 32);
}

#[test]
fn a_prefix_of_the_same_content_is_tried_before_the_end_only_ones_when_the_bound_is_reached() {
    const ENDS: usize = 33;
    let gone = gone_of(&["g/1", "g/2", "g/3"]);
    let prefix = |i: usize| moving(&format!("from{i:02}"), &format!("to{i:02}"));
    // The move found by content is the last by the move, held by two gone names; the
    // others, of the end only, are held by two as well, and the first by a third.
    let content = moving("zz", "yy");
    let ends = |skip: usize| -> Vec<MoveCandidate> {
        (0..ENDS)
            .map(|i| found(&format!("to{i:02}/x{skip}"), FoundBy::SameEnd).under(prefix(i)))
            .collect()
    };
    let with_content = |skip: usize| {
        let mut candidates = ends(skip);
        candidates.push(found(&format!("yy/x{skip}"), FoundBy::SameContent).under(content.clone()));
        candidates
    };
    let mut third = ends(3);
    third.retain(|c| c.prefixes() == [prefix(0)]);
    let out = gone.suggest(suggested(vec![
        answer_for("g/1", with_content(1)),
        answer_for("g/2", with_content(2)),
        answer_for("g/3", third),
    ]));
    let tried = out.prefix_trials();
    assert_eq!(tried.len(), 32);
    // Held by two, but found by content: before `from00`, which three hold.
    assert_eq!(tried[0], content);
    assert_eq!(tried[1], prefix(0));
    assert_eq!(tried[2..], (1..31).map(prefix).collect::<Vec<_>>());
    assert_eq!(out.untried_prefixes, ENDS + 1 - 32);

    // The best way any holder found it counts, not that of every holder: one holder
    // that found it by content is enough.
    let gone = gone_of(&["g/1", "g/2"]);
    let (late, early) = (moving("x", "y"), moving("m", "n"));
    let out = gone.suggest(suggested(vec![
        answer_for(
            "g/1",
            vec![
                found("y/1", FoundBy::SameEnd).under(late.clone()),
                found("n/1", FoundBy::SameEnd).under(early.clone()),
            ],
        ),
        answer_for(
            "g/2",
            vec![
                found("y/2", FoundBy::SameContent).under(late.clone()),
                found("n/2", FoundBy::SameEnd).under(early.clone()),
            ],
        ),
    ]));
    assert_eq!(out.prefix_trials(), [late, early]);
}

fn moved_of(from: &str, to: &str) -> MovedOf {
    MovedOf::new(file_space(), name(from), name(to)).unwrap()
}

#[test]
fn a_prefix_that_stands_is_offered_with_the_names_it_moves() {
    let mut out = MoveSuggestions::default();
    let of = moving("a", "b");
    out.offer(
        of.clone(),
        Ok(NonEmptyVec::new(
            moved_of("a/x", "b/x"),
            vec![moved_of("a/y", "b/y")],
        )),
        vec![other_space()],
    );
    assert!(out.refused_prefixes.is_empty());
    assert_eq!(out.prefixes.len(), 1);
    assert_eq!(out.prefixes[0].of(), &of);
    let moved: Vec<ArtifactKey> = out.prefixes[0].moved().iter().cloned().collect();
    assert_eq!(moved, [file_artifact("a/x"), file_artifact("a/y")]);
    // The spaces whose adapter takes no prefix are carried with it.
    assert_eq!(out.prefixes[0].not_asked(), [other_space()]);
}

#[test]
fn a_prefix_that_is_refused_is_offered_with_how_many_names_it_cannot_move() {
    let mut out = MoveSuggestions::default();
    let refusal = |at: &str| NamedRefusal {
        name: name(at),
        why: NamedWhy::Move(MovedRefusal::ToAbsent {
            artifact: file_artifact(at),
        }),
    };
    out.offer(
        moving("a", "b"),
        Err(PrefixRefusal::Names(NonEmptyVec::new(
            refusal("a/x"),
            vec![refusal("a/y")],
        ))),
        Vec::new(),
    );
    // Nothing under the prefix is refused too, with none that cannot be moved.
    out.offer(
        moving("c", "d"),
        Err(PrefixRefusal::NothingUnder {
            prefix: NamePrefix::new("c").unwrap(),
            spaces: Vec::new(),
        }),
        Vec::new(),
    );
    assert!(out.prefixes.is_empty());
    let refused: Vec<(&PrefixMove, usize)> = out
        .refused_prefixes
        .iter()
        .map(|r| (r.of(), r.refused()))
        .collect();
    assert_eq!(refused, [(&moving("a", "b"), 2), (&moving("c", "d"), 0)]);
}
