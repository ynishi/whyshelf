//! Tests of freshness when a link to artifacts compares other than every byte: what
//! a `lines` comparison does not see, what it does, and a change of the comparison
//! itself.

use std::collections::BTreeSet;

use crate::domain::nonempty::{NonBlank, NonEmptySet};
use crate::domain::shelf::artifact::Comparison;
use crate::domain::shelf::fixtures_tests::{
    actor, approve, decls, docid, file_artifact, fr, jid, ledger, lines, list, name, reading,
    schema, schema_of_lines, shelf_reading, sp_testing, spaces, text,
};
use crate::domain::shelf::ledger::{
    JudgedAt, Record, SameMeaning, SameMeaningOf, SameMeaningRefusal, SameMeaningTarget,
    frame_same_meaning,
};
use crate::domain::shelf::schema::{
    Cardinality, LinkEntryDecl, LinkTargetDecl, Propagation, Schema,
};

use super::*;

const CODE: &[u8] = b"fn a() {\n    one();\n}\n";

/// FR-1 and SP-2 on it, tested by `tests/a.rs` holding `bytes`, under `schema`.
fn spec_shelf(schema: &Schema, bytes: &[u8]) -> Shelf {
    shelf_reading(
        schema,
        vec![fr(1, &[]), sp_testing(2, &["FR-1"], &["tests/a.rs"])],
        &[("tests/a.rs", bytes)],
    )
}

/// The approvals of `subjects` in order on `shelf`, ids `1000 + subject`.
fn approvals(shelf: &Shelf, subjects: &[u128]) -> Vec<Record> {
    let mut records = Vec::new();
    for subject in subjects {
        let so_far = ledger(records.clone());
        records.push(approve(shelf, &so_far, *subject, 1000 + subject, 10, None));
    }
    records
}

fn causes_of(assessment: &Assessment, n: u128) -> Vec<Cause> {
    match assessment.standing_of(docid(n)) {
        Standing::Approved(Freshness::Fresh) => Vec::new(),
        Standing::Approved(Freshness::Degraded(d)) => d.causes().iter().cloned().collect(),
        other => panic!("not an approval: {other:?}"),
    }
}

fn now() -> JudgedAt {
    JudgedAt::new(100)
}

#[test]
fn a_comparison_declared_since_the_approval_is_a_cause_and_no_hash_is_compared() {
    let bytes = schema();
    let slashes = schema_of_lines(&["//"]);
    let l = ledger(approvals(&spec_shelf(&bytes, CODE), &[1, 2]));
    // The same file, now compared by its lines.
    let assessment = assess(&spec_shelf(&slashes, CODE), &l, now());
    let causes = causes_of(&assessment, 2);
    assert!(
        matches!(causes.as_slice(), [Cause::ArtifactComparisonChanged { pin, now }]
            if pin.comparison() == &Comparison::Bytes && *now == lines(&["//"])),
        "{causes:?}"
    );
    assert_eq!(causes[0].severity(), Severity::Stale);
    // Edited as well, it is still that one cause, and no `ArtifactChanged`.
    let edited = assess(&spec_shelf(&slashes, b"fn b() {}\n"), &l, now());
    assert!(
        matches!(
            causes_of(&edited, 2).as_slice(),
            [Cause::ArtifactComparisonChanged { .. }]
        ),
        "{:?}",
        causes_of(&edited, 2)
    );
    assert!(
        assess(&spec_shelf(&slashes, CODE), &l, now())
            .roots_of(docid(2))
            .iter()
            .any(|r| matches!(r.cause, Cause::ArtifactComparisonChanged { .. }))
    );

    // Other prefixes are another comparison too, and so is going back to bytes.
    let l = ledger(approvals(&spec_shelf(&slashes, CODE), &[1, 2]));
    for other in [schema_of_lines(&["#"]), bytes] {
        assert!(matches!(
            causes_of(&assess(&spec_shelf(&other, CODE), &l, now()), 2).as_slice(),
            [Cause::ArtifactComparisonChanged { .. }]
        ));
    }
    // Approved again under the comparison declared, it holds.
    let again = spec_shelf(&schema_of_lines(&["#"]), CODE);
    assert!(
        causes_of(
            &assess(&again, &ledger(approvals(&again, &[1, 2])), now()),
            2
        )
        .is_empty()
    );
}

#[test]
fn under_lines_a_comment_white_space_or_a_blank_line_keeps_the_approval_fresh() {
    let s = schema_of_lines(&["//"]);
    let l = ledger(approvals(&spec_shelf(&s, CODE), &[1, 2]));
    for edit in [
        &b"// why\nfn a() {\n    one();\n}\n"[..],
        b"fn a() {\n\n        one();   // not a line of its own: see below\n}\n",
        b"fn  a()  {\r\n  one();\r\n}\r\n\r\n",
    ] {
        let causes = causes_of(&assess(&spec_shelf(&s, edit), &l, now()), 2);
        if edit.windows(3).any(|w| w == b"see") {
            // A comment after code is part of the line.
            assert!(
                matches!(causes.as_slice(), [Cause::ArtifactChanged { .. }]),
                "{causes:?}"
            );
        } else {
            assert!(
                causes.is_empty(),
                "{:?}: {causes:?}",
                String::from_utf8_lossy(edit)
            );
        }
    }
}

#[test]
fn under_lines_a_change_of_code_is_a_change() {
    let s = schema_of_lines(&["//"]);
    let l = ledger(approvals(&spec_shelf(&s, CODE), &[1, 2]));
    let edited = b"fn a() {\n    two();\n}\n";
    let causes = causes_of(&assess(&spec_shelf(&s, edited), &l, now()), 2);
    assert!(
        matches!(causes.as_slice(), [Cause::ArtifactChanged { pin, now }]
            if pin.artifact() == &file_artifact("tests/a.rs")
                && *now == lines(&["//"]).fingerprint(edited)),
        "{causes:?}"
    );
}

#[test]
fn a_declaration_of_the_same_meaning_holds_under_lines() {
    let s = schema_of_lines(&["//"]);
    let mut records = approvals(&spec_shelf(&s, CODE), &[1, 2]);
    let edited = spec_shelf(&s, b"fn a() {\n    one_again();\n}\n");
    let of = frame_same_meaning(
        &edited,
        &ledger(records.clone()),
        SameMeaningTarget::Artifact(name("tests/a.rs")),
        None,
    )
    .unwrap();
    records.push(Record::SameMeaning(
        SameMeaning::new(
            jid(2001),
            of,
            actor(),
            JudgedAt::new(50),
            NonBlank::new("renamed").unwrap(),
        )
        .unwrap(),
    ));
    let assessment = assess(&edited, &ledger(records), now());
    assert!(causes_of(&assessment, 2).is_empty());
    assert!(
        matches!(assessment.notices_of(docid(2)), [Notice::ArtifactSameMeaning { pin, .. }]
            if pin.comparison() == &lines(&["//"])),
        "{:?}",
        assessment.notices_of(docid(2))
    );
}

/// The schema where the spec's `tests` compares lines without `//`, and a design's
/// `checks` (a `Depends` link to artifacts) compares bytes.
fn schema_of_both() -> Schema {
    let mut decls = decls();
    for book in &mut decls {
        if book.name == "spec" {
            let tests = book.links.iter_mut().find(|l| l.name == "tests").unwrap();
            tests.compare = Some(crate::domain::shelf::schema::CompareDecl {
                kind: "lines".to_owned(),
                ignore: vec!["//".to_owned()],
            });
        }
        if book.name == "design" {
            book.links.push(LinkEntryDecl {
                name: "checks".to_owned(),
                target: LinkTargetDecl::Artifact,
                cardinality: Cardinality::Many,
                propagation: Propagation::Depends,
                space: Some("file".to_owned()),
                compare: None,
                cover: false,
            });
        }
    }
    Schema::validate(decls, &spaces()).unwrap()
}

/// SP-2 tests `src/a.rs` by its lines; the design `x` (identity 3) checks its bytes.
fn both_shelf(bytes: &[u8]) -> Shelf {
    shelf_reading(
        &schema_of_both(),
        vec![
            sp_testing(2, &[], &["src/a.rs"]),
            reading(
                "designs/x.md",
                3,
                "x",
                &[("title", text("design")), ("checks", list(&["src/a.rs"]))],
            ),
        ],
        &[("src/a.rs", bytes)],
    )
}

#[test]
fn one_file_under_two_comparisons_is_judged_by_each_link_s_own() {
    let l = ledger(approvals(&both_shelf(CODE), &[2, 3]));
    let assessment = assess(&both_shelf(b"// why\nfn a() {\n    one();\n}\n"), &l, now());
    assert!(causes_of(&assessment, 2).is_empty());
    assert!(
        matches!(causes_of(&assessment, 3).as_slice(), [Cause::ArtifactChanged { pin, .. }]
            if pin.comparison() == &Comparison::Bytes),
        "{:?}",
        causes_of(&assessment, 3)
    );
}

/// A file linked under two comparisons, edited, and declared to keep the meaning:
/// the declaration is framed under the one comparison that has an earlier hash to
/// declare (the design's `bytes`; SP-2's `lines` saw no change), with the hash now of
/// that comparison, so the design's pin holds again.
#[test]
fn a_declaration_about_a_file_under_two_comparisons_keeps_each_link_s_edit() {
    let records = approvals(&both_shelf(CODE), &[2, 3]);
    let edited = both_shelf(b"// why\nfn a() {\n    one();\n}\n");
    let of = frame_same_meaning(
        &edited,
        &ledger(records.clone()),
        SameMeaningTarget::Artifact(name("src/a.rs")),
        None,
    )
    .unwrap();
    let mut records = records;
    records.push(Record::SameMeaning(
        SameMeaning::new(
            jid(2001),
            of,
            actor(),
            JudgedAt::new(50),
            NonBlank::new("a comment").unwrap(),
        )
        .unwrap(),
    ));
    let assessment = assess(&edited, &ledger(records), now());
    assert!(
        causes_of(&assessment, 3).is_empty(),
        "{:?}",
        causes_of(&assessment, 3)
    );
}

fn frame_of_a(
    shelf: &Shelf,
    records: &[Record],
    named: Option<ArtifactHash>,
) -> Result<SameMeaningOf, SameMeaningRefusal> {
    frame_same_meaning(
        shelf,
        &ledger(records.to_vec()),
        SameMeaningTarget::Artifact(name("src/a.rs")),
        named.map(|h| *h.hash()),
    )
}

#[test]
fn a_code_edit_under_two_comparisons_needs_the_earlier_hash_named_and_takes_its_comparison() {
    let records = approvals(&both_shelf(CODE), &[2, 3]);
    let edited_bytes = b"fn a() {\n    two();\n}\n";
    let edited = both_shelf(edited_bytes);
    // Both comparisons have an earlier hash to declare.
    match frame_of_a(&edited, &records, None) {
        Err(SameMeaningRefusal::SeveralEarlier { hashes }) => assert_eq!(hashes.len(), 2),
        other => panic!("expected several earlier hashes, got {other:?}"),
    }
    // Naming one declares under its comparison only, with that comparison's hash now.
    let slashes = lines(&["//"]);
    for comparison in [Comparison::Bytes, slashes] {
        let earlier = comparison.fingerprint(CODE);
        assert_eq!(
            frame_of_a(&edited, &records, Some(earlier)),
            Ok(SameMeaningOf::Artifact {
                artifact: file_artifact("src/a.rs"),
                from: NonEmptySet::from_set(BTreeSet::from([earlier])).unwrap(),
                to: comparison.fingerprint(edited_bytes),
            }),
            "{comparison:?}"
        );
    }
    // The hash now of either comparison is nothing to declare.
    assert!(matches!(
        frame_of_a(
            &edited,
            &records,
            Some(Comparison::Bytes.fingerprint(edited_bytes))
        ),
        Err(SameMeaningRefusal::NotPinned { .. })
    ));

    // Declared under `bytes`, the design holds and SP-2 is still a change.
    let mut declared = records.clone();
    declared.push(Record::SameMeaning(
        SameMeaning::new(
            jid(2001),
            frame_of_a(&edited, &records, Some(Comparison::Bytes.fingerprint(CODE))).unwrap(),
            actor(),
            JudgedAt::new(50),
            NonBlank::new("renamed").unwrap(),
        )
        .unwrap(),
    ));
    let assessment = assess(&edited, &ledger(declared.clone()), now());
    assert!(causes_of(&assessment, 3).is_empty());
    assert!(matches!(
        causes_of(&assessment, 2).as_slice(),
        [Cause::ArtifactChanged { .. }]
    ));
    // Only `lines` is left to declare, so it needs no name now.
    assert!(matches!(
        frame_of_a(&edited, &declared, None),
        Ok(SameMeaningOf::Artifact { to, .. }) if to == lines(&["//"]).fingerprint(edited_bytes)
    ));
}

#[test]
fn a_hash_pinned_under_a_comparison_no_link_declares_now_cannot_be_declared() {
    // Approved under `bytes`; the link now compares lines, and the file was edited.
    let records = approvals(&spec_shelf(&schema(), CODE), &[1, 2]);
    let now = spec_shelf(&schema_of_lines(&["//"]), b"fn a() {\n    two();\n}\n");
    let pinned = Comparison::Bytes.fingerprint(CODE);
    let target = || SameMeaningTarget::Artifact(name("tests/a.rs"));
    let l = ledger(records);
    assert_eq!(
        frame_same_meaning(&now, &l, target(), Some(*pinned.hash())),
        Err(SameMeaningRefusal::PinnedUnderOtherComparison {
            hash: pinned.to_string()
        })
    );
    // Unnamed, the pin of the other comparison is not an earlier hash at all.
    assert_eq!(
        frame_same_meaning(&now, &l, target(), None),
        Err(SameMeaningRefusal::NothingToDeclare)
    );
    // A hash no approval pinned is still `NotPinned`.
    let never = Comparison::Bytes.fingerprint(b"never");
    assert_eq!(
        frame_same_meaning(&now, &l, target(), Some(*never.hash())),
        Err(SameMeaningRefusal::NotPinned {
            hash: never.to_string()
        })
    );
}

#[test]
fn a_changed_comparison_of_a_file_gone_is_gone_and_of_none_both_times_is_nothing() {
    let bytes = schema();
    let slashes = schema_of_lines(&["//"]);
    let no_file = |schema: &Schema| {
        shelf_reading(
            schema,
            vec![fr(1, &[]), sp_testing(2, &["FR-1"], &["tests/a.rs"])],
            &[],
        )
    };

    // There then, gone now, under another comparison: broken, as any file gone.
    let l = ledger(approvals(&spec_shelf(&bytes, CODE), &[1, 2]));
    let causes = causes_of(&assess(&no_file(&slashes), &l, now()), 2);
    assert!(
        matches!(causes.as_slice(), [Cause::ArtifactGone { pin }] if pin.artifact() == &file_artifact("tests/a.rs")),
        "{causes:?}"
    );
    assert_eq!(causes[0].severity(), Severity::Broken);

    // None then and none now: nothing, whatever the comparison.
    let l = ledger(approvals(&no_file(&bytes), &[1, 2]));
    assert!(causes_of(&assess(&no_file(&slashes), &l, now()), 2).is_empty());

    // None then, there now: arrived, not a change of comparison.
    let causes = causes_of(&assess(&spec_shelf(&slashes, CODE), &l, now()), 2);
    assert!(
        matches!(causes.as_slice(), [Cause::ArtifactArrived { .. }]),
        "{causes:?}"
    );
}
