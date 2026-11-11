//! Every decision of one run of `judge` takes one time, the time of the reading,
//! even when the clock moves while the run is decided.

use std::collections::BTreeSet;

use crate::domain::shelf::ledger::{JudgedAt, Record};
use crate::domain::shelf::lookup::Reference;

use super::fake_tests::{FakeShelf, block_on, fr, reason, sp};
use super::{ApproveStaleRequest, JudgeRequest, Judgement, ShelfApp};

fn judge(judgement: Judgement, reference: &str) -> JudgeRequest {
    JudgeRequest {
        judgement,
        reference: reference.parse::<Reference>().unwrap(),
        actor: None,
    }
}

fn approve(reference: &str) -> JudgeRequest {
    judge(
        Judgement::Approve {
            until: None,
            reason: None,
        },
        reference,
    )
}

fn times(records: &[Record]) -> BTreeSet<JudgedAt> {
    records
        .iter()
        .map(|record| match record {
            Record::Decision(d) => d.at(),
            other => panic!("a decision, not {other:?}"),
        })
        .collect()
}

#[test]
fn the_decisions_of_one_run_share_one_time() {
    let fake = FakeShelf::new().with_files(vec![
        fr(1, "slow", &[]),
        fr(3, "other", &[]),
        sp(2, "cache", &["FR-1"]),
    ]);
    fake.set_clock_step(7);
    let mut app = ShelfApp::new(fake);
    block_on(app.judge_all(vec![
        approve("FR-1"),
        approve("SP-2"),
        judge(Judgement::Deny { reason: None }, "FR-3"),
    ]))
    .unwrap();
    let first = app.ports().records();
    assert_eq!(first.len(), 3);
    assert_eq!(times(&first), BTreeSet::from([JudgedAt::new(1000)]));

    // FR-1 edited: `--stale` approves it and SP-2 again, in one run of its own.
    app.ports().edit(fr(1, "slower", &[]), 1);
    block_on(app.approve_stale(ApproveStaleRequest {
        reason: reason("reviewed"),
        actor: None,
        until: None,
    }))
    .unwrap();
    let records = app.ports().records();
    let again = &records[first.len()..];
    assert_eq!(again.len(), 2);
    let at = times(again);
    assert_eq!(at.len(), 1, "one time: {at:?}");
    assert!(!at.contains(&JudgedAt::new(1000)), "a run of its own");
}
