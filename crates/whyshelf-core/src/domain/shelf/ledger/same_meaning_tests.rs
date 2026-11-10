//! Tests of the declarations that an edit keeps the meaning: their canonical form,
//! what the ledger joins through them, what `frame_same_meaning` gathers, and the
//! withdrawal of a record by its identity.

use std::collections::BTreeSet;

use crate::domain::canon::Canon;
use crate::domain::hash::{ContentHash, HashKind};
use crate::domain::shelf::artifact::ArtifactHash;
use crate::domain::shelf::fixtures_tests::{
    actor, approve, deny, docid, file_artifact, fr, fr_titled, jid, ledger, name, reading, shelf,
    shelf_with, sp_testing,
};

use super::*;

/// A hash of a document, from a small number.
fn dh(n: u8) -> DocHash {
    DocHash::restore(ContentHash::of_bytes(HashKind::ShelfDoc, &[n]))
}

/// A hash of a file, from its content.
fn ah(text: &str) -> ArtifactHash {
    ArtifactHash::of_content(text.as_bytes())
}

fn because() -> NonBlank {
    NonBlank::new("a typo fixed").unwrap()
}

fn declaration(judgement: u128, of: SameMeaningOf) -> Record {
    Record::SameMeaning(
        SameMeaning::new(jid(judgement), of, actor(), JudgedAt::new(50), because()).unwrap(),
    )
}

/// A declaration that the hashes `from` of the document `doc` mean what `to` means.
fn doc_declaration(judgement: u128, doc: u128, from: &[u8], to: u8) -> Record {
    declaration(
        judgement,
        SameMeaningOf::Doc {
            id: docid(doc),
            from: NonEmptySet::from_set(from.iter().map(|n| dh(*n)).collect()).unwrap(),
            to: dh(to),
        },
    )
}

/// As `doc_declaration`, for the file at `at`.
fn artifact_declaration(judgement: u128, at: &str, from: &[&str], to: &str) -> Record {
    declaration(
        judgement,
        SameMeaningOf::Artifact {
            artifact: file_artifact(at),
            from: NonEmptySet::from_set(from.iter().map(|t| ah(t)).collect()).unwrap(),
            to: ah(to),
        },
    )
}

fn revoke(judgement: u128, targets: &[u128], at: i64) -> Record {
    Record::Revocation(Revocation::new(
        jid(judgement),
        NonEmptySet::from_set(targets.iter().map(|t| jid(*t)).collect()).unwrap(),
        actor(),
        JudgedAt::new(at),
        NonBlank::new("not the same after all").unwrap(),
    ))
}

fn map_mut(canon: &mut Canon) -> &mut BTreeMap<String, Canon> {
    match canon {
        Canon::Map(map) => map,
        other => panic!("not a map: {other:?}"),
    }
}

/// The canonical form of a declaration about FR-1, read back after `change`.
fn changed(change: impl FnOnce(&mut BTreeMap<String, Canon>)) -> Result<Record, RecordProblem> {
    let mut canon = doc_declaration(2001, 1, &[1], 2).to_canon();
    change(map_mut(&mut canon));
    Record::from_canon(&canon)
}

fn ids(way: Option<NonEmptyVec<JudgementId>>) -> Option<Vec<JudgementId>> {
    way.map(NonEmptyVec::into_vec)
}

// The canonical form.

#[test]
fn a_declaration_about_a_document_survives_its_canonical_form() {
    let record = doc_declaration(2001, 1, &[1, 3], 2);
    let canon = record.to_canon();
    let mut owned = canon.clone();
    let map = map_mut(&mut owned);
    let keys: Vec<&str> = map.keys().map(String::as_str).collect();
    assert_eq!(
        keys,
        [
            "actor", "at", "from", "id", "kind", "reason", "subject", "to", "v"
        ]
    );
    assert_eq!(map.get("kind"), Some(&Canon::str("same_meaning")));
    assert_eq!(
        map.get("subject"),
        Some(&Canon::map([
            ("kind", Canon::str("doc")),
            ("id", Canon::str(docid(1).to_string())),
        ]))
    );
    assert_eq!(map.get("to"), Some(&Canon::str(dh(2).to_string())));
    assert!(matches!(map.get("from"), Some(Canon::List(items)) if items.len() == 2));
    let back = Record::from_canon(&canon).unwrap();
    assert_eq!(back, record);
    assert_eq!(back.hash(), record.hash());
}

#[test]
fn a_declaration_about_a_file_survives_its_canonical_form() {
    let record = artifact_declaration(2001, "tests/a.rs", &["a"], "b");
    let canon = record.to_canon();
    let mut owned = canon.clone();
    assert_eq!(
        map_mut(&mut owned).get("subject"),
        Some(&Canon::map([
            ("kind", Canon::str("artifact")),
            ("space", Canon::str("file")),
            ("name", Canon::str("tests/a.rs")),
        ]))
    );
    // Stored before spaces existed, with `path` alone: read as the space `file`, and
    // written again as it was stored, with the hash of that form.
    let mut stored = canon.clone();
    map_mut(&mut stored).insert(
        "subject".to_owned(),
        Canon::map([
            ("kind", Canon::str("artifact")),
            ("path", Canon::str("tests/a.rs")),
        ]),
    );
    let old = Record::from_canon(&stored).unwrap();
    assert_eq!(old.to_canon(), stored);
    assert_eq!(
        old.hash(),
        RecordHash(ContentHash::of(HashKind::ShelfJudgement, &stored))
    );
    let Record::SameMeaning(old) = &old else {
        panic!("a declaration");
    };
    assert!(matches!(
        old.of(),
        SameMeaningOf::Artifact { artifact, .. } if *artifact == file_artifact("tests/a.rs")
    ));
    let back = Record::from_canon(&canon).unwrap();
    assert_eq!(back, record);
    assert_eq!(back.hash(), record.hash());
    let Record::SameMeaning(declared) = &back else {
        panic!("a declaration");
    };
    assert_eq!(declared.id(), jid(2001));
    assert_eq!(declared.reason().as_str(), "a typo fixed");
}

#[test]
fn a_declaration_whose_hash_now_is_among_the_earlier_ones_is_refused() {
    let made = SameMeaning::new(
        jid(2001),
        SameMeaningOf::Doc {
            id: docid(1),
            from: NonEmptySet::from_set(BTreeSet::from([dh(1), dh(2)])).unwrap(),
            to: dh(2),
        },
        actor(),
        JudgedAt::new(50),
        because(),
    );
    assert_eq!(
        made,
        Err(SameMeaningProblem::ToAmongFrom {
            to: dh(2).to_string()
        })
    );

    let result = changed(|map| {
        map.insert("to".to_owned(), Canon::str(dh(1).to_string()));
    });
    assert!(
        matches!(
            result,
            Err(RecordProblem::SameMeaning(
                SameMeaningProblem::ToAmongFrom { .. }
            ))
        ),
        "{result:?}"
    );
}

#[test]
fn a_declaration_with_no_earlier_hash_is_refused() {
    let result = changed(|map| {
        map.insert("from".to_owned(), Canon::list([]));
    });
    assert!(
        matches!(result, Err(RecordProblem::EmptyFrom)),
        "{result:?}"
    );
}

#[test]
fn a_declaration_about_a_kind_of_thing_this_reader_does_not_know_is_refused() {
    let result = changed(|map| {
        let subject = map_mut(map.get_mut("subject").unwrap());
        subject.insert("kind".to_owned(), Canon::str("section"));
    });
    assert!(
        matches!(&result, Err(RecordProblem::UnknownSubjectKind { found }) if found == "section"),
        "{result:?}"
    );
}

#[test]
fn a_declaration_needs_a_reason() {
    let missing = changed(|map| {
        map.remove("reason");
    });
    assert!(
        matches!(&missing, Err(RecordProblem::MissingKey { key }) if key == "reason"),
        "{missing:?}"
    );
    let blank = changed(|map| {
        map.insert("reason".to_owned(), Canon::str("   "));
    });
    assert!(
        matches!(&blank, Err(RecordProblem::BadValue { key, .. }) if key == "reason"),
        "{blank:?}"
    );
    // A decision may have a null reason; a declaration may not.
    let null = changed(|map| {
        map.insert("reason".to_owned(), Canon::Null);
    });
    assert!(null.is_err(), "{null:?}");
}

#[test]
fn a_declaration_with_a_key_too_many_is_refused() {
    let top = changed(|map| {
        map.insert("note".to_owned(), Canon::str("extra"));
    });
    assert!(
        matches!(&top, Err(RecordProblem::ExtraKey { key }) if key == "note"),
        "{top:?}"
    );
    // In what it is about, too: a document has no path.
    let subject = changed(|map| {
        let subject = map_mut(map.get_mut("subject").unwrap());
        subject.insert("path".to_owned(), Canon::str("tests/a.rs"));
    });
    assert!(
        matches!(&subject, Err(RecordProblem::ExtraKey { key }) if key == "path"),
        "{subject:?}"
    );
}

// What the ledger joins.

#[test]
fn two_declarations_in_a_chain_join_its_ends_both_ways_in_the_order_of_the_way() {
    // A -> B, then B -> C.
    let l = ledger(vec![
        doc_declaration(2001, 1, &[1], 2),
        doc_declaration(2002, 1, &[2], 3),
    ]);
    assert_eq!(
        ids(l.doc_same_meaning(docid(1), &dh(1), &dh(3))),
        Some(vec![jid(2001), jid(2002)])
    );
    assert_eq!(
        ids(l.doc_same_meaning(docid(1), &dh(3), &dh(1))),
        Some(vec![jid(2002), jid(2001)])
    );
    assert_eq!(
        ids(l.doc_same_meaning(docid(1), &dh(2), &dh(1))),
        Some(vec![jid(2001)])
    );
    assert!(l.problems().is_empty());
}

#[test]
fn a_hash_no_declaration_reaches_does_not_mean_the_same() {
    let l = ledger(vec![doc_declaration(2001, 1, &[1], 2)]);
    assert_eq!(l.doc_same_meaning(docid(1), &dh(1), &dh(4)), None);
    assert_eq!(l.doc_same_meaning(docid(1), &dh(4), &dh(5)), None);
    assert_eq!(
        ledger(vec![]).doc_same_meaning(docid(1), &dh(1), &dh(2)),
        None
    );
}

#[test]
fn the_shortest_way_is_the_answer_whatever_the_order_of_the_records() {
    // A -> B -> C, and A -> C directly.
    let records = vec![
        doc_declaration(2001, 1, &[1], 2),
        doc_declaration(2002, 1, &[2], 3),
        doc_declaration(2003, 1, &[1], 3),
    ];
    let mut reversed = records.clone();
    reversed.reverse();
    for l in [ledger(records), ledger(reversed)] {
        assert_eq!(
            ids(l.doc_same_meaning(docid(1), &dh(1), &dh(3))),
            Some(vec![jid(2003)])
        );
    }
}

#[test]
fn a_revoked_declaration_no_longer_joins_anything() {
    let l = ledger(vec![
        doc_declaration(2001, 1, &[1], 2),
        doc_declaration(2002, 1, &[2], 3),
        revoke(2003, &[2002], 60),
    ]);
    assert!(l.problems().is_empty(), "{:?}", l.problems());
    assert_eq!(l.doc_same_meaning(docid(1), &dh(1), &dh(3)), None);
    assert_eq!(
        ids(l.doc_same_meaning(docid(1), &dh(1), &dh(2))),
        Some(vec![jid(2001)])
    );
    assert!(l.same_meaning(jid(2002)).is_none());
    assert!(l.same_meaning(jid(2001)).is_some());
}

#[test]
fn a_declaration_about_another_document_or_path_does_not_count() {
    let l = ledger(vec![
        doc_declaration(2001, 2, &[1], 2),
        artifact_declaration(2002, "tests/b.rs", &["a"], "b"),
    ]);
    assert_eq!(l.doc_same_meaning(docid(1), &dh(1), &dh(2)), None);
    assert!(l.doc_same_meaning(docid(2), &dh(1), &dh(2)).is_some());
    assert_eq!(
        l.artifact_same_meaning(&file_artifact("tests/a.rs"), &ah("a"), &ah("b")),
        None
    );
    assert_eq!(
        ids(l.artifact_same_meaning(&file_artifact("tests/b.rs"), &ah("b"), &ah("a"))),
        Some(vec![jid(2002)])
    );
}

#[test]
fn files_are_joined_through_a_chain_as_documents_are() {
    let l = ledger(vec![
        artifact_declaration(2001, "tests/a.rs", &["a"], "b"),
        artifact_declaration(2002, "tests/a.rs", &["b"], "c"),
    ]);
    assert_eq!(
        ids(l.artifact_same_meaning(&file_artifact("tests/a.rs"), &ah("a"), &ah("c"))),
        Some(vec![jid(2001), jid(2002)])
    );
    assert_eq!(
        ids(l.artifact_same_meaning(&file_artifact("tests/a.rs"), &ah("c"), &ah("a"))),
        Some(vec![jid(2002), jid(2001)])
    );
}

#[test]
fn two_different_declarations_with_one_id_are_set_aside() {
    let l = ledger(vec![
        doc_declaration(2001, 1, &[1], 2),
        doc_declaration(2001, 1, &[3], 2),
    ]);
    assert!(
        l.problems()
            .iter()
            .any(|p| matches!(p, LedgerProblem::Conflict { .. }))
    );
    assert_eq!(l.doc_same_meaning(docid(1), &dh(1), &dh(2)), None);
    assert!(l.same_meaning(jid(2001)).is_none());
}

// What `frame_same_meaning` gathers.

fn approving(shelf: &Shelf, subjects: &[u128], first_judgement: u128) -> Vec<Record> {
    let mut records = Vec::new();
    for (i, subject) in subjects.iter().enumerate() {
        let so_far = ledger(records.clone());
        records.push(approve(
            shelf,
            &so_far,
            *subject,
            first_judgement + i as u128,
            10,
            None,
        ));
    }
    records
}

fn hash_of(shelf: &Shelf, n: u128) -> DocHash {
    *shelf.doc(docid(n)).unwrap().hash()
}

/// `frame_same_meaning` with no hash named.
fn frame(
    shelf: &Shelf,
    ledger: &Ledger,
    target: SameMeaningTarget,
) -> Result<SameMeaningOf, SameMeaningRefusal> {
    frame_same_meaning(shelf, ledger, target, None)
}

/// `frame_same_meaning` of FR-1 with the hash `named`.
fn frame_named(
    shelf: &Shelf,
    ledger: &Ledger,
    named: ContentHash,
) -> Result<SameMeaningOf, SameMeaningRefusal> {
    frame_same_meaning(shelf, ledger, SameMeaningTarget::Doc(docid(1)), Some(named))
}

/// FR-1 titled `title`, FR-2, FR-3 and FR-4 on it.
fn four(title: &str) -> Shelf {
    shelf(vec![
        fr_titled(1, title, &[]),
        fr(2, &["FR-1"]),
        fr(3, &["FR-1"]),
        fr(4, &["FR-1"]),
    ])
}

fn doc_of(from: &[DocHash], to: DocHash) -> SameMeaningOf {
    SameMeaningOf::Doc {
        id: docid(1),
        from: NonEmptySet::from_set(from.iter().copied().collect()).unwrap(),
        to,
    }
}

#[test]
fn an_older_downstream_pin_from_before_a_rewrite_approved_again_is_not_covered() {
    // FR-2 pinned FR-1 at "a"; FR-1 was rewritten and approved again at "b"; a typo
    // fix makes it "c". The declaration is of b -> c only: a -> b was never declared.
    let at_a = four("a");
    let at_b = four("b");
    let at_c = four("c");
    let mut records = approving(&at_a, &[1, 2], 1001);
    records.push(approve(&at_b, &ledger(records.clone()), 1, 1003, 20, None));
    let l = ledger(records);

    let of = frame(&at_c, &l, SameMeaningTarget::Doc(docid(1))).unwrap();
    assert_eq!(of, doc_of(&[hash_of(&at_b, 1)], hash_of(&at_c, 1)));
}

#[test]
fn an_own_approval_at_the_hash_now_takes_the_one_older_downstream_class() {
    // FR-2 pinned FR-1 at "a"; FR-1 was edited to "b" and approved again there.
    let at_a = four("a");
    let at_b = four("b");
    let mut records = approving(&at_a, &[1, 2], 1001);
    records.push(approve(&at_b, &ledger(records.clone()), 1, 1003, 20, None));
    let of = frame(&at_b, &ledger(records), SameMeaningTarget::Doc(docid(1))).unwrap();
    assert_eq!(of, doc_of(&[hash_of(&at_a, 1)], hash_of(&at_b, 1)));
}

/// FR-2 pins FR-1 at "a", FR-4 at "d", FR-3 at "b", and FR-1's own approval is at
/// "c", the hash now; a declaration says "a" means what "d" means.
fn three_downstream_pins() -> (Shelf, Ledger, [DocHash; 3]) {
    let at = |t| four(t);
    let (at_a, at_d, at_b, at_c) = (at("a"), at("d"), at("b"), at("c"));
    let mut records = approving(&at_a, &[1, 2], 1001);
    records.push(approve(&at_d, &ledger(records.clone()), 1, 1003, 20, None));
    records.push(approve(&at_d, &ledger(records.clone()), 4, 1004, 20, None));
    records.push(approve(&at_b, &ledger(records.clone()), 1, 1005, 30, None));
    records.push(approve(&at_b, &ledger(records.clone()), 3, 1006, 30, None));
    records.push(approve(&at_c, &ledger(records.clone()), 1, 1007, 40, None));
    records.push(declaration(
        2001,
        doc_of(&[hash_of(&at_a, 1)], hash_of(&at_d, 1)),
    ));
    (
        at_c,
        ledger(records),
        [hash_of(&at_a, 1), hash_of(&at_d, 1), hash_of(&at_b, 1)],
    )
}

#[test]
fn downstream_pins_of_different_meanings_are_several_earlier_hashes() {
    let (now, l, [a, d, b]) = three_downstream_pins();
    match frame(&now, &l, SameMeaningTarget::Doc(docid(1))) {
        Err(SameMeaningRefusal::SeveralEarlier { hashes }) => {
            let got: BTreeSet<String> = hashes.into_iter().collect();
            let expected: BTreeSet<String> = [a, d, b].iter().map(ToString::to_string).collect();
            assert_eq!(got, expected);
        }
        other => panic!("expected several earlier hashes, got {other:?}"),
    }
}

#[test]
fn a_named_hash_covers_itself_and_the_pins_already_the_same_as_it() {
    let (now, l, [a, d, b]) = three_downstream_pins();
    let c = hash_of(&now, 1);
    assert_eq!(frame_named(&now, &l, *a.hash()), Ok(doc_of(&[a, d], c)));
    assert_eq!(frame_named(&now, &l, *d.hash()), Ok(doc_of(&[a, d], c)));
    assert_eq!(frame_named(&now, &l, *b.hash()), Ok(doc_of(&[b], c)));
}

#[test]
fn a_named_hash_no_approval_pinned_or_the_hash_now_is_refused() {
    let (now, l, _) = three_downstream_pins();
    for named in [dh(99), hash_of(&now, 1)] {
        assert_eq!(
            frame_named(&now, &l, *named.hash()),
            Err(SameMeaningRefusal::NotPinned {
                hash: named.to_string()
            })
        );
    }
}

#[test]
fn a_pin_of_an_approval_that_does_not_stand_is_not_gathered() {
    // FR-2 pinned FR-1 at "a" and was then denied; FR-1 itself was approved at "b".
    let at_a = shelf(vec![fr_titled(1, "a", &[]), fr(2, &["FR-1"])]);
    let at_b = shelf(vec![fr_titled(1, "b", &[]), fr(2, &["FR-1"])]);
    let at_c = shelf(vec![fr_titled(1, "c", &[]), fr(2, &["FR-1"])]);
    let mut records = approving(&at_a, &[1, 2], 1001);
    records.push(approve(&at_b, &ledger(records.clone()), 1, 1003, 20, None));
    records.push(deny(&at_b, 2, 1004, 20));
    let l = ledger(records);
    let of = frame(&at_c, &l, SameMeaningTarget::Doc(docid(1))).unwrap();
    let SameMeaningOf::Doc { from, .. } = of else {
        panic!("about a document");
    };
    assert_eq!(from.as_set(), &BTreeSet::from([hash_of(&at_b, 1)]));
}

#[test]
fn nothing_is_declared_when_every_pin_is_at_the_hash_now() {
    let s = shelf(vec![fr(1, &[]), fr(2, &["FR-1"])]);
    let l = ledger(approving(&s, &[1, 2], 1001));
    for n in [1, 2] {
        assert_eq!(
            frame(&s, &l, SameMeaningTarget::Doc(docid(n))),
            Err(SameMeaningRefusal::NothingToDeclare)
        );
    }
    // Nor of a document no approval is about.
    assert_eq!(
        frame(&s, &ledger(vec![]), SameMeaningTarget::Doc(docid(1))),
        Err(SameMeaningRefusal::NothingToDeclare)
    );
}

#[test]
fn an_edit_declared_once_has_nothing_more_to_declare_until_it_is_edited_again() {
    let then = shelf(vec![fr_titled(1, "a", &[]), fr(2, &["FR-1"])]);
    let now = shelf(vec![fr_titled(1, "b", &[]), fr(2, &["FR-1"])]);
    let mut records = approving(&then, &[1, 2], 1001);
    let of = frame(
        &now,
        &ledger(records.clone()),
        SameMeaningTarget::Doc(docid(1)),
    )
    .unwrap();
    records.push(declaration(2001, of));
    assert_eq!(
        frame(
            &now,
            &ledger(records.clone()),
            SameMeaningTarget::Doc(docid(1))
        ),
        Err(SameMeaningRefusal::NothingToDeclare)
    );

    let again = shelf(vec![fr_titled(1, "c", &[]), fr(2, &["FR-1"])]);
    let of = frame(&again, &ledger(records), SameMeaningTarget::Doc(docid(1))).unwrap();
    assert_eq!(
        of,
        SameMeaningOf::Doc {
            id: docid(1),
            from: NonEmptySet::from_set(BTreeSet::from([hash_of(&then, 1)])).unwrap(),
            to: hash_of(&again, 1),
        }
    );
}

#[test]
fn a_declaration_about_a_document_that_is_not_valid_is_refused() {
    let then = shelf(vec![fr(1, &[])]);
    let l = ledger(approving(&then, &[1], 1001));
    // No title, which the book requires.
    let invalid = shelf(vec![reading("findings/fr-1.md", 1, "FR-1", &[])]);
    assert_eq!(
        frame(&invalid, &l, SameMeaningTarget::Doc(docid(1))),
        Err(SameMeaningRefusal::DocNotValid {
            id: docid(1),
            tag: FailureTag::Invalid
        })
    );
    assert_eq!(
        frame(&shelf(vec![]), &l, SameMeaningTarget::Doc(docid(1))),
        Err(SameMeaningRefusal::DocNotValid {
            id: docid(1),
            tag: FailureTag::Gone
        })
    );
}

/// FR-1 and SP-2 on it, tested by `tests/a.rs`, with that file's hash.
fn tested(hash: Option<ArtifactHash>) -> Shelf {
    shelf_with(
        vec![fr(1, &[]), sp_testing(2, &["FR-1"], &["tests/a.rs"])],
        &[("tests/a.rs", hash)],
    )
}

#[test]
fn a_declaration_about_a_file_gathers_every_pin_of_its_path() {
    let then = tested(Some(ah("a")));
    let l = ledger(approving(&then, &[1, 2], 1001));
    let now = tested(Some(ah("b")));
    let target = || SameMeaningTarget::Artifact(name("tests/a.rs"));
    assert_eq!(
        frame(&now, &l, target()),
        Ok(SameMeaningOf::Artifact {
            artifact: file_artifact("tests/a.rs"),
            from: NonEmptySet::from_set(BTreeSet::from([ah("a")])).unwrap(),
            to: ah("b"),
        })
    );
    assert_eq!(
        frame(&then, &l, target()),
        Err(SameMeaningRefusal::NothingToDeclare)
    );
}

/// FR-1, and SP-2 and SP-3 on it, both tested by `tests/a.rs` holding `content`.
fn tested_twice(content: &str) -> Shelf {
    shelf_with(
        vec![
            fr(1, &[]),
            sp_testing(2, &["FR-1"], &["tests/a.rs"]),
            sp_testing(3, &["FR-1"], &["tests/a.rs"]),
        ],
        &[("tests/a.rs", Some(ah(content)))],
    )
}

#[test]
fn a_file_pinned_at_two_different_hashes_needs_one_named() {
    // SP-2 pinned the file at "a", SP-3 at "b"; it is "c" now.
    let at_a = tested_twice("a");
    let mut records = approving(&at_a, &[1, 2], 1001);
    records.push(approve(
        &tested_twice("b"),
        &ledger(records.clone()),
        3,
        1003,
        20,
        None,
    ));
    let l = ledger(records);
    let now = tested_twice("c");
    let at = || name("tests/a.rs");
    let named = |hash: ArtifactHash| {
        frame_same_meaning(
            &now,
            &l,
            SameMeaningTarget::Artifact(at()),
            Some(*hash.hash()),
        )
    };

    assert!(
        matches!(
            frame(&now, &l, SameMeaningTarget::Artifact(at())),
            Err(SameMeaningRefusal::SeveralEarlier { ref hashes }) if hashes.len() == 2
        ),
        "{:?}",
        frame(&now, &l, SameMeaningTarget::Artifact(at()))
    );
    for hash in [ah("a"), ah("b")] {
        assert_eq!(
            named(hash),
            Ok(SameMeaningOf::Artifact {
                artifact: file_artifact("tests/a.rs"),
                from: NonEmptySet::from_set(BTreeSet::from([hash])).unwrap(),
                to: ah("c"),
            })
        );
    }
    for hash in [ah("z"), ah("c")] {
        assert_eq!(
            named(hash),
            Err(SameMeaningRefusal::NotPinned {
                hash: hash.to_string()
            })
        );
    }
}

#[test]
fn a_declaration_about_a_file_no_document_links_or_that_is_not_there_is_refused() {
    let then = tested(Some(ah("a")));
    let l = ledger(approving(&then, &[1, 2], 1001));
    assert_eq!(
        frame(&then, &l, SameMeaningTarget::Artifact(name("tests/z.rs"))),
        Err(SameMeaningRefusal::ArtifactNotLinked {
            name: name("tests/z.rs")
        })
    );
    assert_eq!(
        frame(
            &tested(None),
            &l,
            SameMeaningTarget::Artifact(name("tests/a.rs"))
        ),
        Err(SameMeaningRefusal::ArtifactAbsent {
            artifact: file_artifact("tests/a.rs")
        })
    );
}

// The withdrawal of a record by its identity.

#[test]
fn a_decision_or_a_declaration_can_be_withdrawn_once() {
    let s = shelf(vec![fr(1, &[])]);
    let approval = approve(&s, &ledger(vec![]), 1, 1001, 10, None);
    let declared = doc_declaration(2001, 1, &[1], 2);
    let l = ledger(vec![approval.clone(), declared.clone()]);
    assert_eq!(l.check_withdraw(jid(1001)), Ok(()));
    assert_eq!(l.check_withdraw(jid(2001)), Ok(()));

    let withdrawn = ledger(vec![approval, declared, revoke(3001, &[1001, 2001], 60)]);
    for id in [1001, 2001] {
        assert_eq!(
            withdrawn.check_withdraw(jid(id)),
            Err(WithdrawRefusal::AlreadyWithdrawn { id: jid(id) })
        );
    }
}

#[test]
fn what_is_no_decision_or_declaration_cannot_be_withdrawn() {
    let s = shelf(vec![fr(1, &[])]);
    let approval = approve(&s, &ledger(vec![]), 1, 1001, 10, None);
    let l = ledger(vec![approval, revoke(3001, &[1001], 60)]);
    // A revocation, and an identity no record has.
    for id in [3001, 9999] {
        assert_eq!(
            l.check_withdraw(jid(id)),
            Err(WithdrawRefusal::Unknown { id: jid(id) })
        );
    }
    // Two records of one identity are a conflict, and neither counts.
    let conflicted = ledger(vec![
        doc_declaration(2001, 1, &[1], 2),
        doc_declaration(2001, 1, &[3], 2),
    ]);
    assert_eq!(
        conflicted.check_withdraw(jid(2001)),
        Err(WithdrawRefusal::Unknown { id: jid(2001) })
    );
}
