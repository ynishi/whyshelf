//! The reading of the links of a key two documents wrote, and the plan made of it.
//!
//! Two findings write `FR-1` after a merge (ids 1 and 2). The spec 11 was approved on
//! its own branch, when `FR-1` was the finding 1, so its approval pins the finding 1;
//! the spec 12 came from the other branch and is not approved.

use super::*;
use crate::domain::shelf::artifact::ArtifactReadings;
use crate::domain::shelf::build::{DocReading, RawProblem, ReadId};
use crate::domain::shelf::fixtures_tests::{
    approved, docid, fr, ledger, path, reading, shelf, sp_testing, text,
};

fn key(written: &str) -> DocKey {
    DocKey {
        book: crate::domain::shelf::schema::BookName::new("finding").unwrap(),
        form: crate::domain::shelf::fixtures_tests::schema()
            .book("finding")
            .unwrap()
            .key()
            .parse(written)
            .unwrap(),
    }
}

/// The second finding of the merge: another document, with the same key.
fn other_finding(id: u128) -> DocReading {
    reading(
        &format!("findings/other-{id}.md"),
        id,
        "FR-1",
        &[("title", text("other"))],
    )
}

/// A file whose front matter was never closed.
fn unclosed(file: &str) -> DocReading {
    DocReading::Unparsed {
        origin: path(file),
        id: ReadId::Unreadable,
        why: RawProblem::Unclosed,
    }
}

/// What was approved before the merge: the spec 11, grounded in the finding 1.
fn before_the_merge() -> Ledger {
    let before = shelf(vec![fr(1, &[]), sp_testing(11, &["FR-1"], &[])]);
    approved(&before, &[1, 11])
}

fn ask(target: u128, words: &[(u128, Side)]) -> RekeyAsk {
    RekeyAsk {
        target: docid(target),
        key: None,
        words: words
            .iter()
            .map(|(doc, side)| (docid(*doc), *side))
            .collect(),
    }
}

fn meanings(plan: &RekeyPlan) -> Vec<(String, Meaning)> {
    plan.writers()
        .iter()
        .map(|w| (w.file().to_string(), w.meaning()))
        .collect()
}

#[test]
fn when_nobody_else_writes_the_key_every_link_means_the_target() {
    let s = shelf(vec![
        fr(1, &[]),
        sp_testing(11, &["FR-1"], &[]),
        sp_testing(12, &["FR-1"], &[]),
    ]);
    let plan = plan(&s, &ledger(Vec::new()), &ask(1, &[])).unwrap();
    assert_eq!(plan.target().key, key("FR-1"));
    assert_eq!(plan.new_key().key(), &key("FR-2"));
    assert_eq!(plan.new_file().as_str(), "findings/FR-2.md");
    assert_eq!(
        meanings(&plan),
        [
            (
                "specs/sp-11.md".to_owned(),
                Meaning::Follows(FollowWhy::OnlyWriter)
            ),
            (
                "specs/sp-12.md".to_owned(),
                Meaning::Follows(FollowWhy::OnlyWriter)
            ),
        ]
    );
    assert_eq!(plan.writers()[0].items().len(), 1);
    assert_eq!(plan.writers()[0].items()[0].written, "FR-1");
}

#[test]
fn a_word_against_the_only_writer_is_a_contradiction() {
    let s = shelf(vec![fr(1, &[]), sp_testing(11, &["FR-1"], &[])]);
    for side in [Side::Stay, Side::Both] {
        match plan(&s, &ledger(Vec::new()), &ask(1, &[(11, side)])) {
            Err(RekeyProblem::Unsettled {
                undecided,
                contradicted,
            }) => {
                assert!(undecided.is_empty());
                assert_eq!(
                    contradicted[0].contradiction,
                    Contradiction {
                        evidence: Evidence::OnlyWriter,
                        asked: Some(side)
                    }
                );
            }
            other => panic!("{other:?}"),
        }
    }
    // The word that agrees with it is fine.
    let plan = plan(&s, &ledger(Vec::new()), &ask(1, &[(11, Side::Follow)])).unwrap();
    assert_eq!(
        plan.writers()[0].meaning(),
        Meaning::Follows(FollowWhy::OnlyWriter)
    );
}

#[test]
fn the_pins_of_an_approval_say_which_document_a_link_meant() {
    let s = shelf(vec![
        fr(1, &[]),
        other_finding(2),
        sp_testing(11, &["FR-1"], &[]),
    ]);
    let before = before_the_merge();
    // The spec 11 pinned the finding 1: it follows it, and stays when the finding 2
    // is the one that takes the new key.
    let follows = plan(&s, &before, &ask(1, &[])).unwrap();
    assert_eq!(
        meanings(&follows),
        [(
            "specs/sp-11.md".to_owned(),
            Meaning::Follows(FollowWhy::Pinned)
        )]
    );
    let stays = plan(&s, &before, &ask(2, &[])).unwrap();
    assert_eq!(
        meanings(&stays),
        [("specs/sp-11.md".to_owned(), Meaning::Stays(StayWhy::Pinned))]
    );
    // The target is an invalid document (both writers of a key are), in its one file.
    assert_eq!(stays.target().file.as_str(), "findings/other-2.md");
    assert_eq!(stays.new_key().key(), &key("FR-2"));
}

#[test]
fn a_document_with_no_approval_is_undecided_until_the_caller_says() {
    let s = shelf(vec![
        fr(1, &[]),
        other_finding(2),
        sp_testing(11, &["FR-1"], &[]),
        sp_testing(12, &["FR-1"], &[]),
    ]);
    let before = before_the_merge();
    match plan(&s, &before, &ask(1, &[])) {
        Err(RekeyProblem::Unsettled {
            undecided,
            contradicted,
        }) => {
            assert!(contradicted.is_empty());
            assert_eq!(undecided.len(), 1);
            assert_eq!(undecided[0].file.as_str(), "specs/sp-12.md");
            assert_eq!(undecided[0].id, Some(docid(12)));
            assert_eq!(undecided[0].hints, Hints::NoApproval);
        }
        other => panic!("{other:?}"),
    }
    let plan = plan(&s, &before, &ask(1, &[(12, Side::Stay)])).unwrap();
    assert_eq!(
        meanings(&plan),
        [
            (
                "specs/sp-11.md".to_owned(),
                Meaning::Follows(FollowWhy::Pinned)
            ),
            ("specs/sp-12.md".to_owned(), Meaning::Stays(StayWhy::Asked)),
        ]
    );
}

#[test]
fn a_word_is_laid_beside_the_pins_and_not_dropped() {
    let s = shelf(vec![
        fr(1, &[]),
        other_finding(2),
        sp_testing(11, &["FR-1"], &[]),
    ]);
    let before = before_the_merge();
    // The pins say follow (the target is the finding 1): `--stay` is against them,
    // `--both` holds them, `--follow` agrees.
    match plan(&s, &before, &ask(1, &[(11, Side::Stay)])) {
        Err(RekeyProblem::Unsettled { contradicted, .. }) => assert_eq!(
            contradicted[0].contradiction,
            Contradiction {
                evidence: Evidence::Pinned(Side::Follow),
                asked: Some(Side::Stay)
            }
        ),
        other => panic!("{other:?}"),
    }
    assert_eq!(
        plan(&s, &before, &ask(1, &[(11, Side::Both)]))
            .unwrap()
            .writers()[0]
            .meaning(),
        Meaning::Both
    );
    assert_eq!(
        plan(&s, &before, &ask(1, &[(11, Side::Follow)]))
            .unwrap()
            .writers()[0]
            .meaning(),
        Meaning::Follows(FollowWhy::Pinned)
    );
}

#[test]
fn pins_on_both_sides_are_a_contradiction_until_the_caller_says_both() {
    // Before the merge the spec grounded itself in two findings; on the merge the
    // second took the key of the first.
    let before = shelf(vec![
        fr(1, &[]),
        fr(9, &[]),
        sp_testing(11, &["FR-1", "FR-9"], &[]),
    ]);
    let ledger = approved(&before, &[1, 9, 11]);
    let s = shelf(vec![
        fr(1, &[]),
        reading("findings/fr-9.md", 9, "FR-1", &[("title", text("nine"))]),
        sp_testing(11, &["FR-1"], &[]),
    ]);
    match plan(&s, &ledger, &ask(9, &[])) {
        Err(RekeyProblem::Unsettled { contradicted, .. }) => assert_eq!(
            contradicted[0].contradiction,
            Contradiction {
                evidence: Evidence::Pinned(Side::Both),
                asked: None
            }
        ),
        other => panic!("{other:?}"),
    }
    // `--follow` and `--stay` are against pins that name both sides.
    assert!(plan(&s, &ledger, &ask(9, &[(11, Side::Follow)])).is_err());
    assert_eq!(
        plan(&s, &ledger, &ask(9, &[(11, Side::Both)]))
            .unwrap()
            .writers()[0]
            .meaning(),
        Meaning::Both
    );
}

#[test]
fn the_pin_of_another_link_is_no_evidence_for_this_one() {
    // The spec 11 wrote the key in `grounds` and was approved with a pin of the finding
    // 1 under that link; a document whose approval pins nothing under the link that
    // writes the key has no evidence.
    let s = shelf(vec![
        fr(1, &[]),
        other_finding(2),
        sp_testing(11, &["FR-1"], &[]),
    ]);
    let none = approved(&shelf(vec![fr(1, &[]), sp_testing(11, &[], &[])]), &[1, 11]);
    match plan(&s, &none, &ask(1, &[])) {
        Err(RekeyProblem::Unsettled { undecided, .. }) => {
            assert_eq!(undecided[0].hints, Hints::NoPin);
            assert_eq!(
                undecided[0].links,
                [crate::domain::shelf::schema::EntryName::new("grounds").unwrap()]
            );
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn every_document_that_is_not_decided_is_listed() {
    let s = shelf(vec![
        fr(1, &[]),
        other_finding(2),
        sp_testing(11, &["FR-1"], &[]),
        sp_testing(12, &["FR-1"], &[]),
        sp_testing(13, &["FR-1"], &[]),
    ]);
    match plan(&s, &before_the_merge(), &ask(1, &[(11, Side::Stay)])) {
        Err(RekeyProblem::Unsettled {
            undecided,
            contradicted,
        }) => {
            assert_eq!(
                undecided
                    .iter()
                    .map(|u| u.file.to_string())
                    .collect::<Vec<_>>(),
                ["specs/sp-12.md", "specs/sp-13.md"]
            );
            assert_eq!(contradicted.len(), 1);
            assert_eq!(contradicted[0].file.as_str(), "specs/sp-11.md");
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn a_file_that_could_not_be_read_stops_the_plan_and_is_named() {
    // In the book of the key: it may write the key.
    let s = shelf(vec![
        fr(1, &[]),
        sp_testing(11, &["FR-1"], &[]),
        unclosed("findings/broken.md"),
    ]);
    match plan(&s, &ledger(Vec::new()), &ask(1, &[])) {
        Err(RekeyProblem::NotAllRead { keys, links }) => {
            assert_eq!(keys, [path("findings/broken.md")]);
            // The findings link to findings too (`base`), so the file may link to it.
            assert_eq!(links, [Unread::File(path("findings/broken.md"))]);
        }
        other => panic!("{other:?}"),
    }
    // In a book that links into the key's book: it may link to the key.
    let s = shelf(vec![
        fr(1, &[]),
        sp_testing(11, &["FR-1"], &[]),
        unclosed("specs/broken.md"),
    ]);
    match plan(&s, &ledger(Vec::new()), &ask(1, &[])) {
        Err(RekeyProblem::NotAllRead { keys, links }) => {
            assert!(keys.is_empty());
            assert_eq!(links, [Unread::File(path("specs/broken.md"))]);
        }
        other => panic!("{other:?}"),
    }
    // In a book that links to neither: it is no place of the key.
    let s = shelf(vec![
        fr(1, &[]),
        sp_testing(11, &["FR-1"], &[]),
        unclosed("designs/broken.md"),
    ]);
    assert!(plan(&s, &ledger(Vec::new()), &ask(1, &[])).is_ok());
}

#[test]
fn a_link_that_is_no_list_stops_the_plan_and_is_named() {
    let mut text_link = sp_testing(11, &[], &[]);
    if let DocReading::Parsed { entries, .. } = &mut text_link {
        entries.insert("grounds".to_owned(), text("FR-1"));
    }
    let s = shelf(vec![fr(1, &[]), text_link]);
    match plan(&s, &ledger(Vec::new()), &ask(1, &[])) {
        Err(RekeyProblem::NotAllRead { keys, links }) => {
            assert!(keys.is_empty());
            assert_eq!(
                links,
                [Unread::Link {
                    file: path("specs/sp-11.md"),
                    link: crate::domain::shelf::schema::EntryName::new("grounds").unwrap()
                }]
            );
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn the_target_is_one_file_with_a_key_that_was_read() {
    let s = shelf(vec![fr(1, &[])]);
    assert_eq!(
        plan(&s, &ledger(Vec::new()), &ask(9, &[])),
        Err(RekeyProblem::Gone)
    );
    // The identity in two files.
    let twice = shelf(vec![
        fr(1, &[]),
        reading("findings/copy.md", 1, "FR-9", &[("title", text("copy"))]),
    ]);
    assert!(matches!(
        plan(&twice, &ledger(Vec::new()), &ask(1, &[])),
        Err(RekeyProblem::InSeveralFiles { files }) if files.len() == 2
    ));
    // A key that is no key of the book: nothing to name the document by.
    let no_key = shelf(vec![reading(
        "findings/a.md",
        5,
        "nonsense",
        &[("title", text("a"))],
    )]);
    assert!(matches!(
        plan(&no_key, &ledger(Vec::new()), &ask(5, &[])),
        Err(RekeyProblem::NoKey { files }) if files.len() == 1
    ));
}

#[test]
fn a_key_that_is_given_must_be_free() {
    let s = shelf(vec![fr(1, &[]), fr(2, &[])]);
    let mut taken = ask(1, &[]);
    taken.key = Some("FR-2".to_owned());
    assert!(matches!(
        plan(&s, &ledger(Vec::new()), &taken),
        Err(RekeyProblem::ClaimKey(ClaimKeyProblem::Taken { .. }))
    ));
    let mut free = ask(1, &[]);
    free.key = Some("FR-7".to_owned());
    let plan = plan(&s, &ledger(Vec::new()), &free).unwrap();
    assert_eq!(plan.new_key().key(), &key("FR-7"));
    assert_eq!(plan.new_file().as_str(), "findings/FR-7.md");
}

#[test]
fn a_word_names_a_document_that_writes_the_key_in_a_link() {
    let s = shelf(vec![fr(1, &[]), sp_testing(11, &["FR-1"], &[])]);
    assert_eq!(
        plan(&s, &ledger(Vec::new()), &ask(1, &[(5, Side::Follow)])),
        Err(RekeyProblem::NotAWriter { doc: docid(5) })
    );
}

#[test]
fn the_documents_whose_body_names_the_old_key_are_listed_and_left_as_they_are() {
    let mention = |id: u128, body: &str| {
        let mut file = sp_testing(id, &[], &[]);
        if let DocReading::Parsed { body: text, .. } = &mut file {
            *text = body.to_owned();
        }
        file
    };
    let s = shelf(vec![
        fr(1, &[]),
        mention(11, "see FR-1 for why\n"),
        mention(12, "FR-10 is another\n"),
        mention(13, "(FR-1)\n"),
    ]);
    let plan = plan(&s, &ledger(Vec::new()), &ask(1, &[])).unwrap();
    assert_eq!(plan.mentions(), [docid(11), docid(13)]);
    assert!(plan.writers().is_empty());
}

#[test]
fn the_body_of_the_target_is_listed_when_it_names_the_old_key_though_the_target_is_invalid() {
    // The finding 2 is invalid while `FR-1` is written twice, so the shelf has no body of
    // it: the caller gives the body of its file.
    let s = shelf(vec![fr(1, &[]), other_finding(2)]);
    let mut invalid = plan(&s, &ledger(Vec::new()), &ask(2, &[])).unwrap();
    assert!(invalid.mentions().is_empty());
    // The same reading of "names" as for the others: `FR-10` is not `FR-1`.
    invalid.mention_own_body("see FR-10 only\n");
    assert!(invalid.mentions().is_empty());
    invalid.mention_own_body("a copy of FR-1\n");
    assert_eq!(invalid.mentions(), [docid(2)]);
    // Never twice.
    invalid.mention_own_body("FR-1 again\n");
    assert_eq!(invalid.mentions(), [docid(2)]);

    // A valid target is read by `plan` already, and is not added again.
    let valid = shelf(vec![{
        let mut own = fr(1, &[]);
        if let DocReading::Parsed { body, .. } = &mut own {
            *body = "this is FR-1\n".to_owned();
        }
        own
    }]);
    let mut own = plan(&valid, &ledger(Vec::new()), &ask(1, &[])).unwrap();
    assert_eq!(own.mentions(), [docid(1)]);
    own.mention_own_body("this is FR-1\n");
    assert_eq!(own.mentions(), [docid(1)]);
}

#[test]
fn a_key_is_named_whole() {
    for (body, named) in [
        ("FR-1", true),
        ("see FR-1.", true),
        ("FR-10", false),
        ("XFR-1", false),
        ("FR-1-b", false),
        ("a_FR-1", false),
    ] {
        assert_eq!(names(body, "FR-1"), named, "{body}");
    }
}

/// The files a plan writes, as an adapter would say they read: each file's entries by
/// the plan, the target's in its new file.
fn planned(plan: &RekeyPlan, readings: &[DocReading]) -> Vec<DocReading> {
    let files = plan.files();
    std::iter::once((&files.from, &files.to))
        .chain(files.rewritten.iter().map(|file| (file, file)))
        .map(|(from, to)| {
            let Some(DocReading::Parsed { entries, body, .. }) =
                readings.iter().find(|reading| reading.origin() == from)
            else {
                panic!("{from} is not a parsed file");
            };
            DocReading::Parsed {
                origin: to.clone(),
                entries: plan.entries_of(from, entries),
                body: body.clone(),
            }
        })
        .collect()
}

/// The merge, with the findings 1 and 2 and the specs 11 and 12 linking to `FR-1`.
fn merge() -> Vec<DocReading> {
    vec![
        fr(1, &[]),
        other_finding(2),
        sp_testing(11, &["FR-1"], &[]),
        sp_testing(12, &["FR-1"], &[]),
    ]
}

fn grounds(reading: &DocReading) -> Vec<String> {
    match reading {
        DocReading::Parsed { entries, .. } => match entries.get("grounds") {
            Some(crate::domain::shelf::build::RawValue::List(keys)) => keys.clone(),
            other => panic!("{other:?}"),
        },
        other => panic!("{other:?}"),
    }
}

#[test]
fn the_entries_a_plan_gives_change_the_key_and_the_items_of_the_links_that_follow_or_keep_both() {
    let readings = merge();
    let s = shelf(readings.clone());
    let plan = plan(
        &s,
        &before_the_merge(),
        &ask(1, &[(11, Side::Both), (12, Side::Follow)]),
    )
    .unwrap();
    let written = planned(&plan, &readings);
    let at = |file: &str| {
        written
            .iter()
            .find(|reading| reading.origin().as_str() == file)
            .unwrap()
    };
    // The target is written in the file of its new key, with that key and nothing more
    // changed.
    let DocReading::Parsed { entries, .. } = at("findings/FR-2.md") else {
        panic!("parsed");
    };
    assert_eq!(entries["key"], text("FR-2"));
    assert_eq!(entries["id"], text(&docid(1).to_string()));
    assert_eq!(grounds(at("specs/sp-11.md")), ["FR-1", "FR-2"]);
    assert_eq!(grounds(at("specs/sp-12.md")), ["FR-2"]);
    let files = plan.files();
    assert_eq!(files.from.as_str(), "findings/fr-1.md");
    assert_eq!(files.to.as_str(), "findings/FR-2.md");
    assert_eq!(
        files
            .rewritten
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>(),
        ["specs/sp-11.md", "specs/sp-12.md"]
    );
}

#[test]
fn the_files_a_plan_writes_build_the_shelf_the_plan_said() {
    let readings = merge();
    let s = shelf(readings.clone());
    for words in [
        vec![(12, Side::Follow)],
        vec![(11, Side::Both), (12, Side::Follow)],
        vec![(12, Side::Stay)],
    ] {
        let plan = plan(&s, &before_the_merge(), &ask(1, &words)).unwrap();
        let written = planned(&plan, &readings);
        plan.confirm(&s, readings.clone(), written, &ArtifactReadings::new())
            .unwrap_or_else(|differs| panic!("{words:?}: {differs}"));
    }
}

#[test]
fn files_that_do_not_read_as_planned_are_refused_with_every_difference() {
    let readings = merge();
    let s = shelf(readings.clone());
    let plan = plan(&s, &before_the_merge(), &ask(1, &[(12, Side::Follow)])).unwrap();
    // Only the target's file is written: the links that were to follow still name `FR-1`.
    let mut written = planned(&plan, &readings);
    written.retain(|reading| reading.origin().as_str() == "findings/FR-2.md");
    let differs = plan
        .confirm(&s, readings.clone(), written, &ArtifactReadings::new())
        .unwrap_err();
    let items: Vec<(String, Side)> = differs
        .differences
        .iter()
        .map(|difference| match difference {
            Difference::Item { file, side, .. } => (file.to_string(), *side),
            other => panic!("{other}"),
        })
        .collect();
    assert_eq!(
        items,
        [
            ("specs/sp-11.md".to_owned(), Side::Follow),
            ("specs/sp-12.md".to_owned(), Side::Follow)
        ]
    );

    // A key that does not read as the new key: the target is not the document planned.
    let mut written = planned(&plan, &readings);
    for reading in &mut written {
        if let DocReading::Parsed { entries, .. } = reading
            && reading_is_target(entries)
        {
            entries.insert("key".to_owned(), text("FR-9"));
        }
    }
    let differs = plan
        .confirm(&s, readings, written, &ArtifactReadings::new())
        .unwrap_err();
    assert!(
        differs
            .differences
            .iter()
            .any(|difference| matches!(difference, Difference::Target { .. })),
        "{differs}"
    );
}

fn reading_is_target(
    entries: &std::collections::BTreeMap<String, crate::domain::shelf::build::RawValue>,
) -> bool {
    entries.get("id") == Some(&text(&docid(1).to_string()))
}

#[test]
fn files_that_changed_since_the_plan_was_made_are_refused() {
    let readings = merge();
    let s = shelf(readings.clone());
    let plan = plan(&s, &before_the_merge(), &ask(1, &[(12, Side::Follow)])).unwrap();
    let written = planned(&plan, &readings);
    let mut changed = readings;
    changed.push(sp_testing(13, &[], &[]));
    let differs = plan
        .confirm(&s, changed, written, &ArtifactReadings::new())
        .unwrap_err();
    assert_eq!(differs.differences.len(), 1);
    assert_eq!(*differs.differences.first(), Difference::Read);
}
