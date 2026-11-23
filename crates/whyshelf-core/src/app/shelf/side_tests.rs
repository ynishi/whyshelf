//! Tests of the reading of one side of a change ([`load_at`]): each side read through
//! the same ports, at one time read once by the caller.

use crate::domain::shelf::ledger::JudgedAt;

use super::error::ShelfError;
use super::fake_tests::{FakeBefore, FakeShelf, block_on, docid, fr, sp};
use super::load::{load, load_at};
use super::ports::ShelfPort;
use super::state::Side;

/// A fake with one document after, and two before.
fn two_sides() -> FakeShelf {
    let ports = FakeShelf::new().with_files(vec![fr(1, "slow", &[])]);
    *ports.before.lock().unwrap() = Some(FakeBefore {
        readings: vec![fr(1, "slow", &[]), sp(2, "cache", &["FR-1"])],
        ..FakeBefore::default()
    });
    ports
}

#[test]
fn each_side_is_read_from_its_own_state_at_the_one_time_given() {
    let ports = two_sides();
    let at = JudgedAt::new(5000);
    let before = block_on(load_at(&ports, Side::Before, at)).unwrap();
    let after = block_on(load_at(&ports, Side::After, at)).unwrap();
    assert!(before.shelf.doc(docid(2)).is_some());
    assert!(after.shelf.doc(docid(2)).is_none());
    assert!(after.shelf.doc(docid(1)).is_some());
    assert_eq!(before.now, at);
    assert_eq!(after.now, at);
}

#[test]
fn a_side_the_run_does_not_read_is_an_error_of_a_port_never_the_other_side() {
    let ports = FakeShelf::new().with_files(vec![fr(1, "slow", &[])]);
    let error = block_on(load_at(&ports, Side::Before, JudgedAt::new(5000))).unwrap_err();
    assert!(
        matches!(
            error,
            ShelfError::Port {
                port: ShelfPort::Schema,
                ..
            }
        ),
        "{error:?}"
    );
}

#[test]
fn a_load_reads_the_side_after_and_the_clock_once() {
    let ports = two_sides();
    *ports.clock_step.lock().unwrap() = 7;
    let loaded = block_on(load(&ports)).unwrap();
    assert!(loaded.shelf.doc(docid(2)).is_none());
    assert_eq!(loaded.now, JudgedAt::new(1000));
    assert_eq!(*ports.now.lock().unwrap(), 1007);
}
