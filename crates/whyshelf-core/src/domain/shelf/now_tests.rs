//! Tests of what a name of an artifact or a document comes to now: told, or cannot be
//! told (issue #152).
//!
//! One promise, over every reader: what cannot be told is not read as gone, nor as
//! absent, and no approval holds it.
//!
//! ```text
//!   documents (T1)                       artifacts (T2)
//!   rows    valid, invalid, gone,        rows    present, absent, and the four
//!           cannot be told                       ways a name cannot be told
//!   columns tag, presence, named now,    columns told, an approval, a cause or a
//!           lookup, retirement,                  notice, a move, a diff, and two
//!           snapshot (now, file),                approvals beside each other
//!           files, changed
//! ```

use std::collections::BTreeMap;

use crate::domain::nonempty::NonBlank;
use crate::domain::shelf::artifact::{
    ArtifactHash, ArtifactReading, ArtifactReadings, ArtifactSeen, ArtifactState, Comparison,
    Untold,
};
use crate::domain::shelf::build::{
    DocFailure, DocPresence, DocReading, DocState, FailureTag, NowNamed, RawProblem, ReadId, Shelf,
};
use crate::domain::shelf::exemption::{ExemptRefusal, frame_exempt};
use crate::domain::shelf::fixtures_tests::{
    actor, approve, approved, docid, eid, file_artifact, file_space, fr, ledger, name, path,
    read_pins, reading, retire, schema, shelf, sided, sp_testing,
};
use crate::domain::shelf::freshness::assess_unasked as assess;
use crate::domain::shelf::freshness::{Assessment, Cause, Freshness, Notice, Standing};
use crate::domain::shelf::ledger::{
    ApprovalKind, Asked, Before, DecisionVerb, FrameRefusal, JudgedAt, Ledger, MovedOf,
    MovedRefusal, Record, Seen, frame_approve, frame_moved, frame_present,
};
use crate::domain::shelf::lookup::{Absence, Reference, Resolution, resolve};
use crate::domain::shelf::schema::{BookName, EntryName};
use crate::domain::shelf::snapshot::{ItemKind, Snapshot, SnapshotItem, doc_now};
use crate::domain::shelf::stats;

// ---------------------------------------------------------------------------------
// T1: a document
// ---------------------------------------------------------------------------------

/// What an identity comes to in a shelf, by the four ways of a `DocState`.
#[derive(Debug, Clone, Copy)]
enum Row {
    Valid,
    Invalid,
    Gone,
    Unknown,
}

const ROWS: [Row; 4] = [Row::Valid, Row::Invalid, Row::Gone, Row::Unknown];

/// A finding with no title: invalid.
fn untitled() -> DocReading {
    reading("findings/fr-2.md", 2, "FR-2", &[])
}

/// A file whose identity cannot be read: any identity no file carries may be in it.
fn hole() -> DocReading {
    DocReading::Unparsed {
        origin: path("findings/x.md"),
        id: ReadId::Unreadable,
        why: RawProblem::NotAMapping,
    }
}

/// The shelf of a row: the finding 1 is valid, the finding 2 is invalid, the finding
/// 3 is in no file, and, in the row that cannot be told, a file's identity cannot be
/// read.
fn shelf_of(row: Row) -> Shelf {
    let mut readings = vec![fr(1, &[]), untitled()];
    if matches!(row, Row::Unknown) {
        readings.push(hole());
    }
    shelf(readings)
}

/// The document the row asks about.
fn id_of(row: Row) -> u128 {
    match row {
        Row::Valid => 1,
        Row::Invalid => 2,
        Row::Gone | Row::Unknown => 3,
    }
}

/// The ledger of the approvals of the findings 1, 2 and 3, made when each was valid.
fn approvals_of_all() -> Ledger {
    approved(&shelf(vec![fr(1, &[]), fr(2, &[]), fr(3, &[])]), &[1, 2, 3])
}

#[test]
fn a_document_that_cannot_be_told_is_not_valid_and_not_gone_in_what_it_fails_with() {
    let tags = [
        None,
        Some(FailureTag::Invalid),
        Some(FailureTag::Gone),
        Some(FailureTag::Unknown),
    ];
    for (row, expected) in ROWS.into_iter().zip(tags) {
        let s = shelf_of(row);
        let tag = s
            .state_of(docid(id_of(row)))
            .valid_or_failure()
            .err()
            .map(|failure| failure.tag());
        assert_eq!(tag, expected, "{row:?}");
    }
}

#[test]
fn a_document_that_cannot_be_told_has_its_own_presence() {
    let shape = |presence: &DocPresence| match presence {
        DocPresence::Valid { .. } => "valid",
        DocPresence::Invalid { .. } => "invalid",
        DocPresence::Gone => "gone",
        DocPresence::Unknown { .. } => "unknown",
    };
    for (row, expected) in ROWS
        .into_iter()
        .zip(["valid", "invalid", "gone", "unknown"])
    {
        let s = shelf_of(row);
        let presence = s.state_of(docid(id_of(row))).presence();
        assert_eq!(shape(&presence), expected, "{row:?}");
    }
}

#[test]
fn a_document_that_cannot_be_told_is_named_as_none_that_is_gone() {
    for row in ROWS {
        let s = shelf_of(row);
        let named = s.now_named(docid(id_of(row)));
        let ok = match row {
            Row::Valid => {
                matches!(&named, NowNamed::Valid(key) if Some(key) == s.doc(docid(1)).map(|d| d.key()))
            }
            Row::Invalid => {
                matches!(&named, NowNamed::Invalid(Some(key)) if key.to_string() == "finding:FR-2")
            }
            Row::Gone => named == NowNamed::Gone,
            Row::Unknown => named == NowNamed::Unknown,
        };
        assert!(ok, "{row:?}: {named:?}");
    }
}

#[test]
fn a_document_that_cannot_be_told_is_not_gone_to_a_lookup() {
    let reference = |id: u128| -> Reference { eid(id).to_string().parse().unwrap() };
    let known = approvals_of_all();
    let nothing = ledger(Vec::new());
    for row in ROWS {
        let s = shelf_of(row);
        let id = id_of(row);
        // The ledger has records about it.
        let with = resolve(&s, &known, &reference(id));
        let expected = match row {
            Row::Valid | Row::Invalid => Resolution::Found(docid(id)),
            Row::Gone => Resolution::LedgerOnly {
                id: docid(id),
                state: Absence::Gone,
            },
            Row::Unknown => {
                let DocState::Unknown(may) = s.state_of(docid(id)) else {
                    panic!("unknown");
                };
                assert_eq!(may.any(), [path("findings/x.md")]);
                Resolution::LedgerOnly {
                    id: docid(id),
                    state: Absence::Unknown(may),
                }
            }
        };
        assert_eq!(with, expected, "{row:?}, with records");
        // It has none: a document with neither a file nor a record is named by nothing.
        let without = resolve(&s, &nothing, &reference(id));
        let expected = match row {
            Row::Valid | Row::Invalid => Resolution::Found(docid(id)),
            Row::Gone | Row::Unknown => Resolution::Missing,
        };
        assert_eq!(without, expected, "{row:?}, with no record");
    }
}

#[test]
fn a_retirement_keeps_the_tag_of_a_document_that_cannot_be_told() {
    for row in ROWS {
        let s = shelf_of(row);
        let Record::Decision(decision) = retire(&s, id_of(row), 1001, 10) else {
            panic!("a retirement is a decision");
        };
        let DecisionVerb::Retire { seen } = decision.verb() else {
            panic!("a retirement retires");
        };
        let expected = match row {
            Row::Valid => Seen::At(*s.doc(docid(1)).unwrap().hash()),
            Row::Invalid => Seen::Not(FailureTag::Invalid),
            Row::Gone => Seen::Not(FailureTag::Gone),
            Row::Unknown => Seen::Not(FailureTag::Unknown),
        };
        assert_eq!(*seen, expected, "{row:?}");
    }
}

#[test]
fn a_snapshot_keeps_a_document_that_cannot_be_told_apart_from_one_that_is_gone() {
    for row in ROWS {
        let s = shelf_of(row);
        let now = doc_now(&s, docid(id_of(row)));
        let (expected, file) = match row {
            Row::Valid => (
                Ok(*s.doc(docid(1)).unwrap().hash()),
                Some("findings/fr-1.md"),
            ),
            Row::Invalid => (Err(FailureTag::Invalid), Some("findings/fr-2.md")),
            Row::Gone => (Err(FailureTag::Gone), None),
            Row::Unknown => (Err(FailureTag::Unknown), None),
        };
        assert_eq!(now.now, expected, "{row:?}");
        assert_eq!(now.file.as_ref().map(|f| f.as_str()), file, "{row:?}");
    }
}

/// Beside a file left in a merge conflict whose sides show 4 and 5 (`X`), the
/// finding 3, in no file, is gone, and the finding 4 cannot be told; each was approved.
fn beside_a_conflict() -> (Shelf, Ledger) {
    let then = shelf(vec![fr(1, &[]), fr(3, &[]), fr(4, &[])]);
    let now = shelf(vec![fr(1, &[]), sided("findings/x.md", &[4, 5])]);
    (now, approved(&then, &[1, 3, 4]))
}

#[test]
fn beside_a_conflict_every_reader_tells_a_document_no_side_shows_as_gone_and_one_it_shows_as_unknown()
 {
    let (s, known) = beside_a_conflict();
    for (n, tag) in [(3, FailureTag::Gone), (4, FailureTag::Unknown)] {
        let id = docid(n);
        // The snapshot of what it is now: no file carries it.
        let now = doc_now(&s, id);
        assert_eq!(now.now, Err(tag), "{n}");
        assert_eq!(now.file, None, "{n}");
        // A retirement keeps the tag.
        let Record::Decision(decision) = retire(&s, n, 2001, 10) else {
            panic!("a retirement is a decision");
        };
        assert_eq!(
            decision.verb(),
            &DecisionVerb::Retire {
                seen: Seen::Not(tag)
            },
            "{n}"
        );
        // An approval, a presence and an exemption are refused for it.
        match frame_approve(&s, &known, id) {
            Err(FrameRefusal::NotValid(failure)) => assert_eq!(failure.tag(), tag, "{n}"),
            other => panic!("{n}: {other:?}"),
        }
        match frame_present(&s, id) {
            Err(FrameRefusal::NotValid(failure)) => assert_eq!(failure.tag(), tag, "{n}"),
            other => panic!("{n}: {other:?}"),
        }
        assert_eq!(
            frame_exempt(
                &s,
                &known,
                id,
                BookName::new("finding").unwrap(),
                EntryName::new("base").unwrap()
            ),
            Err(ExemptRefusal::DocNotValid { id, tag }),
            "{n}"
        );
        // It has changed from what was approved.
        let snapshot = Snapshot::of(&known, id).expect("an approval stands");
        assert!(
            snapshot.items()[0]
                .compared(&s, &ArtifactReadings::new())
                .changed(),
            "{n}"
        );
    }
    // The approval of 4 is refused with the file a side shows it in.
    let Err(FrameRefusal::NotValid(DocFailure::Unknown(may))) = frame_approve(&s, &known, docid(4))
    else {
        panic!("unknown");
    };
    assert_eq!(may.shown(), [path("findings/x.md")]);
    assert!(may.any().is_empty());
}

#[test]
fn only_a_valid_or_an_invalid_document_has_a_file_that_carries_it() {
    for (row, expected) in ROWS.into_iter().zip([
        vec!["findings/fr-1.md"],
        vec!["findings/fr-2.md"],
        vec![],
        vec![],
    ]) {
        let s = shelf_of(row);
        let state = s.state_of(docid(id_of(row)));
        let files: Vec<&str> = state.files().into_iter().map(|f| f.as_str()).collect();
        assert_eq!(files, expected, "{row:?}");
    }
}

#[test]
fn the_files_of_an_invalid_identity_are_in_the_order_of_its_files_and_the_snapshot_names_the_first()
{
    let two = shelf(vec![
        reading("findings/b.md", 2, "FR-2", &[]),
        reading("findings/a.md", 2, "FR-3", &[]),
    ]);
    let state = two.state_of(docid(2));
    let files: Vec<String> = state.files().iter().map(|f| f.to_string()).collect();
    assert_eq!(files.len(), 2, "{files:?}");
    // The order of the files of the presence, the one a person reads them in.
    let DocPresence::Invalid { files: said, .. } = state.presence() else {
        panic!("two invalid files carry the identity");
    };
    let said: Vec<String> = said.iter().map(ToString::to_string).collect();
    assert_eq!(files, said);
    // The snapshot's file is the first.
    let now = doc_now(&two, docid(2));
    assert_eq!(now.file.map(|f| f.to_string()), Some(files[0].clone()));
    assert_eq!(now.now, Err(FailureTag::Invalid));
}

#[test]
fn a_document_that_is_not_valid_has_changed_from_what_was_approved_whatever_it_cannot_be() {
    let known = approvals_of_all();
    for (row, changed) in ROWS.into_iter().zip([false, true, true, true]) {
        let s = shelf_of(row);
        let id = id_of(row);
        let snapshot = Snapshot::of(&known, docid(id)).expect("an approval stands");
        let subject = &snapshot.items()[0];
        assert_eq!(
            subject.compared(&s, &ArtifactReadings::new()).changed(),
            changed,
            "{row:?}"
        );
    }
}

// ---------------------------------------------------------------------------------
// T2: an artifact
// ---------------------------------------------------------------------------------

/// What the adapter says of a name, or does not say.
#[derive(Debug, Clone, Copy)]
enum Told {
    Present,
    Absent,
    Said,
    NoAnswer,
    NotAsked,
    NotAName,
}

const TOLD: [Told; 6] = [
    Told::Present,
    Told::Absent,
    Told::Said,
    Told::NoAnswer,
    Told::NotAsked,
    Told::NotAName,
];

/// The rows an approval can be made on: a name that is not a name of the space makes
/// the document that writes it invalid, so no approval is asked of it.
const WRITTEN: [Told; 5] = [
    Told::Present,
    Told::Absent,
    Told::Said,
    Told::NoAnswer,
    Told::NotAsked,
];

fn h(text: &str) -> ArtifactHash {
    ArtifactHash::of_content(text.as_bytes())
}

fn why() -> NonBlank {
    NonBlank::new("offline").unwrap()
}

/// The answer of the adapter for `tests/a.rs`, if it gives one.
fn reading_of(row: Told) -> Option<ArtifactReading> {
    match row {
        Told::Present => Some(ArtifactReading::of_hashes(
            BTreeMap::from([(Comparison::Bytes, h("now"))]),
            None,
        )),
        Told::Absent => Some(ArtifactReading::absent()),
        Told::Said => Some(ArtifactReading::cannot_tell(why())),
        Told::NoAnswer => None,
        Told::NotAsked => Some(ArtifactReading::of_hashes(
            BTreeMap::from([(Comparison::Presence, Comparison::Presence.fingerprint(&[]))]),
            None,
        )),
        Told::NotAName => Some(ArtifactReading::not_a_name(why())),
    }
}

fn readings_of(row: Told) -> ArtifactReadings {
    let mut readings = ArtifactReadings::new();
    if let Some(reading) = reading_of(row) {
        readings.insert(file_space(), name("tests/a.rs"), reading);
    }
    readings
}

/// What the readings of a row come to for `tests/a.rs` compared by its bytes.
fn state_of(row: Told) -> ArtifactState {
    readings_of(row).state_of(&file_artifact("tests/a.rs"), &Comparison::Bytes)
}

/// What is to be said of the row: what an approval can pin, or why it cannot be told.
fn told_of(row: Told) -> Result<ArtifactSeen, Untold> {
    match row {
        Told::Present => Ok(ArtifactSeen::Present(h("now"))),
        Told::Absent => Ok(ArtifactSeen::Absent),
        Told::Said => Err(Untold::Said(why())),
        Told::NoAnswer => Err(Untold::NoAnswer),
        Told::NotAsked => Err(Untold::NotAsked),
        Told::NotAName => Err(Untold::NotAName(why())),
    }
}

fn documents() -> Vec<DocReading> {
    vec![fr(1, &[]), sp_testing(2, &["FR-1"], &["tests/a.rs"])]
}

/// The shelf of the finding 1 and the spec 2, tested by `tests/a.rs`, which the row
/// says.
fn spec_shelf(row: Told) -> Shelf {
    Shelf::build(&schema(), documents(), &readings_of(row))
}

/// The answer for `tests/a.rs` there with this content, or with nothing there.
fn readings_holding(content: Option<&str>) -> ArtifactReadings {
    let reading = match content {
        Some(text) => {
            ArtifactReading::of_hashes(BTreeMap::from([(Comparison::Bytes, h(text))]), None)
        }
        None => ArtifactReading::absent(),
    };
    let mut readings = ArtifactReadings::new();
    readings.insert(file_space(), name("tests/a.rs"), reading);
    readings
}

/// The same, with `tests/a.rs` there with this content, or with nothing there.
fn spec_shelf_holding(content: Option<&str>) -> Shelf {
    Shelf::build(&schema(), documents(), &readings_holding(content))
}

#[test]
fn what_an_adapter_said_is_told_as_what_an_approval_can_pin_or_why_it_cannot() {
    for row in TOLD {
        let state = state_of(row);
        assert_eq!(state.told().map_err(Clone::clone), told_of(row), "{row:?}");
    }
    // A pin is of a thing there or of nothing, and a thing there comes first.
    assert!(ArtifactSeen::Present(h("a")) < ArtifactSeen::Absent);
    assert_eq!(ArtifactSeen::Present(h("a")).hash(), Some(&h("a")));
    assert_eq!(ArtifactSeen::Absent.hash(), None);
}

#[test]
fn an_approval_pins_what_can_be_told_and_is_refused_what_cannot() {
    for row in WRITTEN {
        let s = spec_shelf(row);
        let so_far = approved(&s, &[1]);
        match (frame_approve(&s, &so_far, docid(2)), told_of(row)) {
            (Ok(_), Ok(seen)) => {
                let both = ledger(vec![
                    approve(&s, &ledger(Vec::new()), 1, 1001, 10, None),
                    approve(&s, &so_far, 2, 1002, 10, None),
                ]);
                let pins = read_pins(&both, docid(2));
                assert_eq!(pins.len(), 1, "{row:?}");
                assert_eq!(pins[0].pin().seen().state(), seen, "{row:?}");
            }
            (Err(FrameRefusal::ArtifactUnknown { why, .. }), Err(untold)) => {
                assert_eq!(why, untold, "{row:?}");
            }
            (frame, told) => panic!("{row:?}: {frame:?} for {told:?}"),
        }
    }
}

fn causes_of(assessment: &Assessment, n: u128) -> Vec<Cause> {
    match assessment.standing_of(docid(n)) {
        fresh if fresh.is_fresh_met() => Vec::new(),
        Standing::Approved {
            freshness: Freshness::Degraded(d),
            ..
        } => d.causes().iter().cloned().collect(),
        other => panic!("not an approval: {other:?}"),
    }
}

/// The ledger of the approvals of the finding 1 and the spec 2 made while
/// `tests/a.rs` holds this content, or has nothing.
fn approved_while(content: Option<&str>) -> Ledger {
    approved(&spec_shelf_holding(content), &[1, 2])
}

#[test]
fn a_name_that_cannot_be_told_is_no_cause_and_a_notice_where_one_there_or_not_is_a_cause() {
    let unknown_notice = |assessment: &Assessment, untold: &Untold| {
        assessment.notices_of(docid(2)).iter().any(|notice| {
            matches!(notice, Notice::ArtifactUnknown { why, .. } if why.as_str() == untold.as_str())
        })
    };
    for row in WRITTEN {
        let s = spec_shelf(row);
        for pinned in [Some("then"), None] {
            let assessment = assess(&s, &approved_while(pinned), JudgedAt::new(100));
            let causes = causes_of(&assessment, 2);
            let kinds: Vec<&str> = causes
                .iter()
                .map(|cause| match cause {
                    Cause::ArtifactChanged { .. } => "changed",
                    Cause::ArtifactGone { .. } => "gone",
                    Cause::ArtifactArrived { .. } => "arrived",
                    other => panic!("{row:?}, pinned {pinned:?}: {other:?}"),
                })
                .collect();
            let expected: &[&str] = match (row, pinned) {
                (Told::Present, Some(_)) => &["changed"],
                (Told::Present, None) => &["arrived"],
                (Told::Absent, Some(_)) => &["gone"],
                _ => &[],
            };
            assert_eq!(kinds, expected, "{row:?}, pinned {pinned:?}");
            match told_of(row) {
                Ok(_) => assert!(
                    !assessment
                        .notices_of(docid(2))
                        .iter()
                        .any(|notice| matches!(notice, Notice::ArtifactUnknown { .. })),
                    "{row:?}"
                ),
                Err(untold) => assert!(
                    unknown_notice(&assessment, &untold),
                    "{row:?}: {:?}",
                    assessment.notices_of(docid(2))
                ),
            }
        }
    }
}

#[test]
fn a_move_is_to_a_name_that_is_there_and_is_refused_to_one_that_is_not_or_cannot_be_told() {
    for row in TOLD {
        let mut readings = readings_of(row);
        readings.insert(file_space(), name("from"), ArtifactReading::absent());
        let of = MovedOf::new(file_space(), name("from"), name("tests/a.rs")).unwrap();
        let moved = frame_moved(of, &readings);
        let ok = match row {
            // A reading that is there is there, under whichever comparison.
            Told::Present | Told::NotAsked => moved.is_ok(),
            Told::Absent => matches!(moved, Err(MovedRefusal::ToAbsent { .. })),
            Told::Said | Told::NoAnswer | Told::NotAName => {
                let untold = told_of(row).unwrap_err();
                matches!(&moved, Err(MovedRefusal::ToUntold { why, .. }) if *why == untold)
            }
        };
        assert!(ok, "{row:?}: {moved:?}");
    }
}

/// The item of the approval of the spec 2 that is its artifact.
fn artifact_item(snapshot: &Snapshot) -> &SnapshotItem {
    snapshot
        .items()
        .iter()
        .find(|item| matches!(item.kind(), ItemKind::Artifact(_)))
        .expect("the approval pinned the artifact")
}

#[test]
fn a_diff_tells_a_name_that_cannot_be_told_changed_and_a_name_as_it_was_unchanged() {
    for pinned in [Some("then"), None] {
        let snapshot = Snapshot::of(&approved_while(pinned), docid(2)).unwrap();
        let item = artifact_item(&snapshot);
        let s = spec_shelf_holding(pinned);
        assert!(
            !item.compared(&s, &readings_holding(pinned)).changed(),
            "{pinned:?}"
        );
        for row in TOLD {
            let state = state_of(row);
            let same = match (&state, pinned) {
                (ArtifactState::Present(hash), Some(_)) => *hash == h("then"),
                (ArtifactState::Absent, None) => true,
                _ => false,
            };
            assert_eq!(
                item.compared(&s, &readings_of(row)).changed(),
                !same,
                "{row:?}, pinned {pinned:?}"
            );
        }
    }
}

/// What two approvals of the spec 2, made while `tests/a.rs` held `then` and while it
/// held `now`, say of each other: what the second is beside the first, and the
/// artifacts whose pins differ between the two.
fn beside(then: Option<&str>, now: Option<&str>) -> (ApprovalKind, usize) {
    let s_then = spec_shelf_holding(then);
    let s_now = spec_shelf_holding(now);
    let first = approve(&s_then, &ledger(Vec::new()), 1, 1001, 10, None);
    let second = approve(&s_then, &ledger(vec![first.clone()]), 2, 1002, 10, None);
    let so_far = ledger(vec![first.clone(), second.clone()]);
    let frame = frame_approve(&s_now, &so_far, docid(2)).unwrap();
    let kind = so_far.kind_of(
        &Asked::frame(&frame, &actor(), None, JudgedAt::new(60)),
        Before::Now,
    );
    let third = approve(&s_now, &so_far, 2, 1003, 20, None);
    let after = ledger(vec![first, second, third]);
    (kind, stats::of(&after, &s_now, None).origins.len())
}

#[test]
fn two_approvals_are_beside_each_other_by_what_they_pinned_alone() {
    let same = ApprovalKind::Repeat {
        standing: jid(1002),
    };
    // The same pin twice: the second reads what the first read, and nothing is counted
    // as an origin of approving again.
    for held in [Some("a"), None] {
        assert_eq!(beside(held, held), (same, 0), "{held:?}");
    }
    // Present(a) to Present(b), Present to Absent and Absent to Present: each is a
    // new pin, and the artifact is an origin.
    for (then, now) in [(Some("a"), Some("b")), (Some("a"), None), (None, Some("a"))] {
        assert_eq!(
            beside(then, now),
            (ApprovalKind::New, 1),
            "{then:?} to {now:?}"
        );
    }
}

fn jid(n: u128) -> crate::domain::shelf::ledger::JudgementId {
    crate::domain::shelf::fixtures_tests::jid(n)
}
