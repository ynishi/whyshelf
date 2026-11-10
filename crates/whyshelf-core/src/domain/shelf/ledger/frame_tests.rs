//! Tests of the frames a decision is taken from (`frame_approve`, `frame_present`,
//! `frame_retire`) and of the expiry an approval may have.

use crate::domain::shelf::fixtures_tests::{
    actor, approve, docid, fr, fr_titled, jid, ledger, reading, shelf,
};
use crate::domain::shelf::graph::CycleSource;

use super::*;

#[test]
fn an_approval_that_would_close_a_cycle_of_frames_is_refused() {
    // FR-1 stands on FR-2 and both are approved. Then the links turn round: FR-2
    // names FR-1, and FR-1 names nothing, so the shelf has no cycle of links.
    let before = shelf(vec![fr(1, &["FR-2"]), fr(2, &[])]);
    let first = approve(&before, &ledger(vec![]), 2, 1002, 10, None);
    let second = approve(&before, &ledger(vec![first.clone()]), 1, 1001, 10, None);
    let ledger = ledger(vec![first, second]);
    let after = shelf(vec![fr(1, &[]), fr(2, &["FR-1"])]);
    assert!(after.cycles().is_empty());

    match frame_approve(&after, &ledger, docid(2)) {
        Err(FrameRefusal::InCycle(cycle)) => {
            assert_eq!(cycle.id().source, CycleSource::Frames);
            assert_eq!(cycle.ids(), vec![docid(1), docid(2)]);
        }
        other => panic!("expected a cycle of frames, got {other:?}"),
    }
    // FR-1 may still be approved again: it names nothing now.
    assert!(frame_approve(&after, &ledger, docid(1)).is_ok());
}

#[test]
fn an_approval_in_a_cycle_of_links_is_refused() {
    let shelf = shelf(vec![fr(1, &["FR-2"]), fr(2, &["FR-1"])]);
    for n in [1, 2] {
        match frame_approve(&shelf, &ledger(vec![]), docid(n)) {
            Err(FrameRefusal::InCycle(cycle)) => {
                assert_eq!(cycle.id().source, CycleSource::Links, "FR-{n}");
                assert!(cycle.contains(docid(1)) && cycle.contains(docid(2)));
            }
            other => panic!("FR-{n}: expected a cycle of links, got {other:?}"),
        }
    }
}

#[test]
fn an_approval_of_a_document_that_is_not_valid_is_refused() {
    // FR-1 has no title, which its book requires.
    let invalid = shelf(vec![reading("findings/fr-1.md", 1, "FR-1", &[])]);
    assert!(matches!(
        frame_approve(&invalid, &ledger(vec![]), docid(1)),
        Err(FrameRefusal::NotValid(DocFailure::Invalid(_)))
    ));
    let empty = shelf(vec![]);
    assert_eq!(
        frame_approve(&empty, &ledger(vec![]), docid(1)),
        Err(FrameRefusal::NotValid(DocFailure::Gone))
    );
}

#[test]
fn a_denial_or_a_deprecation_of_a_document_that_is_not_valid_is_refused() {
    // FR-1 has no title, which its book requires.
    let invalid = shelf(vec![reading("findings/fr-1.md", 1, "FR-1", &[])]);
    assert!(matches!(
        frame_present(&invalid, docid(1)),
        Err(FrameRefusal::NotValid(DocFailure::Invalid(_)))
    ));
    let empty = shelf(vec![]);
    assert_eq!(
        frame_present(&empty, docid(1)),
        Err(FrameRefusal::NotValid(DocFailure::Gone))
    );

    let valid = shelf(vec![fr(1, &[])]);
    let frame = frame_present(&valid, docid(1)).unwrap();
    assert_eq!(frame.seen, *valid.doc(docid(1)).unwrap().hash());
}

#[test]
fn a_retirement_frame_is_taken_of_any_document_and_sees_a_gone_one_as_gone() {
    let gone = frame_retire(&shelf(vec![]), docid(1));
    assert_eq!(gone.subject, docid(1));
    assert_eq!(gone.seen, Seen::Not(FailureTag::Gone));

    let invalid = frame_retire(
        &shelf(vec![reading("findings/fr-1.md", 1, "FR-1", &[])]),
        docid(1),
    );
    assert_eq!(invalid.seen, Seen::Not(FailureTag::Invalid));

    let valid_shelf = shelf(vec![fr_titled(1, "kept", &[])]);
    let valid = frame_retire(&valid_shelf, docid(1));
    assert_eq!(
        valid.seen,
        Seen::At(*valid_shelf.doc(docid(1)).unwrap().hash())
    );

    // Each is a retirement that can be made.
    for frame in [gone, invalid, valid] {
        assert!(Decision::retire(jid(1001), frame, actor(), JudgedAt::new(10), None).is_ok());
    }
}

#[test]
fn an_approval_must_expire_after_it_is_made() {
    let shelf = shelf(vec![fr(1, &[])]);
    let at = JudgedAt::new(10);
    let make = |expires: i64| {
        let frame = frame_approve(&shelf, &ledger(vec![]), docid(1)).unwrap();
        Decision::approve(
            jid(1001),
            frame,
            Some(JudgedAt::new(expires)),
            actor(),
            at,
            None,
        )
    };
    for expires in [10, 9, -1] {
        assert_eq!(
            make(expires),
            Err(DecisionProblem::ExpiresNotAfter {
                at,
                expires: JudgedAt::new(expires),
            }),
            "expires at {expires}"
        );
    }
    assert!(make(11).is_ok());
}
