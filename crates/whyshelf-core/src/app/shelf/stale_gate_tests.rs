//! Tests of the gate of the stale report: each case of DEC-26.

use crate::domain::shelf::ledger::JudgedAt;
use crate::domain::shelf::lookup::Reference;

use super::fake_tests::{FakeShelf, block_on, file_space, fr, sp, sp_tested};
use super::{Gate, JudgeRequest, Judgement, ShelfApp};

fn approve(reference: &str, until: Option<JudgedAt>) -> JudgeRequest {
    JudgeRequest {
        judgement: Judgement::Approve {
            until,
            reason: None,
            again: false,
        },
        reference: reference.parse::<Reference>().unwrap(),
        by: Default::default(),
    }
}

fn gate(app: &ShelfApp<FakeShelf>) -> Gate {
    block_on(app.stale()).unwrap().gate()
}

#[test]
fn a_shelf_whose_approvals_hold_passes_and_a_pending_document_does_not_fail_it() {
    let fake = FakeShelf::new().with_files(vec![fr(1, "slow", &[]), sp(2, "cache", &["FR-1"])]);
    let mut app = ShelfApp::new(fake);
    // Nothing approved yet: both are pending, and pending passes.
    assert_eq!(gate(&app), Gate::Holds);
    block_on(app.judge_all(vec![approve("FR-1", None)])).unwrap();
    // FR-1 holds, SP-2 is still pending.
    let holds = gate(&app);
    assert_eq!(holds, Gate::Holds);
    assert!(holds.passes());
}

#[test]
fn an_approval_that_does_not_hold_fails() {
    let fake = FakeShelf::new().with_files(vec![fr(1, "slow", &[])]);
    let mut app = ShelfApp::new(fake);
    block_on(app.judge_all(vec![approve("FR-1", None)])).unwrap();
    app.ports().edit(fr(1, "slower", &[]), 1);
    let does_not_hold = gate(&app);
    assert_eq!(does_not_hold, Gate::DoesNotHold);
    assert!(!does_not_hold.passes());
}

#[test]
fn an_approval_that_only_expired_fails() {
    let fake = FakeShelf::new().with_files(vec![fr(1, "slow", &[])]);
    fake.set_now(1000);
    let mut app = ShelfApp::new(fake);
    block_on(app.judge_all(vec![approve("FR-1", Some(JudgedAt::new(5000)))])).unwrap();
    assert_eq!(gate(&app), Gate::Holds);
    app.ports().set_now(6000);
    assert_eq!(gate(&app), Gate::DoesNotHold);
}

#[test]
fn an_approval_on_a_name_that_cannot_be_told_now_fails_while_it_stays_fresh() {
    let fake = FakeShelf::new().with_files(vec![
        fr(1, "slow", &[]),
        sp_tested(2, "cache", &["FR-1"], &["tests/a.rs"]),
    ]);
    fake.put_file("tests/a.rs", b"assert!(fast);\n");
    let mut app = ShelfApp::new(fake);
    block_on(app.judge_all(vec![approve("FR-1", None), approve("SP-2", None)])).unwrap();
    app.ports()
        .cannot_tell(&file_space(), "tests/a.rs", "the tree is not mounted");
    let report = block_on(app.stale()).unwrap();
    assert!(report.entries.is_empty(), "its freshness is unchanged");
    let cannot_tell = report.gate();
    assert_eq!(cannot_tell, Gate::CannotTell);
    assert!(!cannot_tell.passes());
    // Told again, it holds.
    app.ports().can_tell(&file_space(), "tests/a.rs");
    assert_eq!(gate(&app), Gate::Holds);
}
