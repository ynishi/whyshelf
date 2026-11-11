//! Tests of `moved` over the in-memory fake: what a declaration of a move asks the
//! artifact source, what it writes, that a refusal writes nothing, and that a run is
//! written whole or not at all.

use crate::domain::actor::Actor;
use crate::domain::canon::Canon;
use crate::domain::nonempty::NonBlank;
use crate::domain::shelf::freshness::{Cause, Freshness, Standing};
use crate::domain::shelf::ledger::{MovedRefusal, Record};
use crate::domain::shelf::lookup::Reference;

use super::fake_tests::{FakeShelf, aname, artifact, block_on, file_space, fr, sp_tested};
use super::{
    ErrorClass, JudgeRequest, Judgement, MovedRequest, ShelfApp, ShelfError, WithdrawRequest,
};

fn r(text: &str) -> Reference {
    text.parse().unwrap()
}

fn approve(reference: &str) -> JudgeRequest {
    JudgeRequest {
        judgement: Judgement::Approve {
            until: None,
            reason: None,
            again: false,
        },
        reference: r(reference),
        actor: None,
    }
}

fn moved(from: &str, to: &str) -> MovedRequest {
    MovedRequest {
        from: aname(from),
        to: aname(to),
        reason: NonBlank::new("the module was split").unwrap(),
        actor: None,
    }
}

/// FR-1 and SP-2 on it, tested by `tests` (each holding its own name), both
/// approved.
fn approved(tests: &[&str]) -> ShelfApp<FakeShelf> {
    let fake = FakeShelf::new().with_files(vec![
        fr(1, "slow", &[]),
        sp_tested(2, "cache", &["FR-1"], tests),
    ]);
    for at in tests {
        fake.put_file(at, at.as_bytes());
    }
    let mut app = ShelfApp::new(fake);
    block_on(app.judge_all(vec![approve("FR-1"), approve("SP-2")])).unwrap();
    app
}

/// Move the file at `from` to `to`, its bytes as they are, or `edited`.
fn move_file(app: &ShelfApp<FakeShelf>, from: &str, to: &str, edited: Option<&[u8]>) {
    app.ports().remove_file(from);
    app.ports().put_file(to, edited.unwrap_or(from.as_bytes()));
}

fn standing(app: &ShelfApp<FakeShelf>, reference: &str) -> Standing {
    block_on(app.get(&r(reference))).unwrap().row.standing
}

fn causes(app: &ShelfApp<FakeShelf>, reference: &str) -> Vec<Cause> {
    match standing(app, reference) {
        Standing::Approved(Freshness::Fresh) => Vec::new(),
        Standing::Approved(Freshness::Degraded(d)) => d.causes().iter().cloned().collect(),
        other => panic!("not an approval: {other:?}"),
    }
}

#[test]
fn a_declared_move_with_the_link_rewritten_keeps_the_approval_fresh() {
    let mut app = approved(&["tests/a.rs"]);
    move_file(&app, "tests/a.rs", "tests/b.rs", None);
    app.ports()
        .edit(sp_tested(2, "cache", &["FR-1"], &["tests/b.rs"]), 2);
    // Before the declaration the link names what the approval did not pin.
    assert!(
        matches!(
            causes(&app, "SP-2").as_slice(),
            [Cause::ArtifactLinksChanged { .. }]
        ),
        "{:?}",
        causes(&app, "SP-2")
    );

    let asked_before = app.ports().asked().len();
    let declared = block_on(app.moved(vec![MovedRequest {
        actor: Some(Actor::new("bob").unwrap()),
        ..moved("tests/a.rs", "tests/b.rs")
    }]))
    .unwrap();
    assert_eq!(declared.len(), 1);
    let one = &declared[0];
    assert_eq!(one.of().from(), artifact(&file_space(), "tests/a.rs"));
    assert_eq!(one.of().to(), artifact(&file_space(), "tests/b.rs"));
    assert_eq!(one.actor().as_str(), "bob");
    assert_eq!(one.at().millis(), 1000);
    // The two names were asked of the space's adapter, through the artifact port.
    let asked = app.ports().asked();
    let ask = asked.last().unwrap();
    assert!(asked.len() > asked_before);
    assert!(ask.get(&file_space(), &aname("tests/a.rs")).is_some());
    assert!(ask.get(&file_space(), &aname("tests/b.rs")).is_some());

    assert_eq!(app.ports().append_count(), 3);
    assert_eq!(app.ports().records()[2], Record::Moved(one.clone()));
    let Canon::Map(canon) = Record::Moved(one.clone()).to_canon() else {
        panic!("a record is a map");
    };
    assert_eq!(canon.get("kind"), Some(&Canon::str("moved")));

    assert_eq!(standing(&app, "SP-2"), Standing::Approved(Freshness::Fresh));
    let row = block_on(app.get(&r("SP-2"))).unwrap().row;
    assert!(row.notices.is_empty(), "{:?}", row.notices);
}

#[test]
fn a_move_with_an_edit_is_stale_with_the_change_at_the_new_name() {
    let mut app = approved(&["tests/a.rs"]);
    move_file(&app, "tests/a.rs", "tests/b.rs", Some(b"edited"));
    app.ports()
        .edit(sp_tested(2, "cache", &["FR-1"], &["tests/b.rs"]), 2);
    block_on(app.moved(vec![moved("tests/a.rs", "tests/b.rs")])).unwrap();
    match causes(&app, "SP-2").as_slice() {
        [Cause::ArtifactChanged { pin, .. }] => {
            assert_eq!(pin.pin().artifact(), &artifact(&file_space(), "tests/a.rs"));
            assert_eq!(pin.read_as(), &artifact(&file_space(), "tests/b.rs"));
        }
        other => panic!("one change at the new name: {other:?}"),
    }
}

#[test]
fn a_link_still_on_the_old_name_is_stale_and_checked() {
    let mut app = approved(&["tests/a.rs"]);
    move_file(&app, "tests/a.rs", "tests/b.rs", None);
    block_on(app.moved(vec![moved("tests/a.rs", "tests/b.rs")])).unwrap();
    // One cause, and no change of the link claimed.
    assert!(
        matches!(
            causes(&app, "SP-2").as_slice(),
            [Cause::ArtifactMoved { written, pin, .. }]
                if written == &artifact(&file_space(), "tests/a.rs")
                    && pin.read_as() == &artifact(&file_space(), "tests/b.rs")
        ),
        "{:?}",
        causes(&app, "SP-2")
    );
    let check = block_on(app.books_check()).unwrap();
    let super::BooksCheck::Checked { moved, absent, .. } = &check else {
        panic!("checked");
    };
    assert_eq!(moved.len(), 1, "{moved:?}");
    assert_eq!(moved[0].artifact, artifact(&file_space(), "tests/a.rs"));
    assert_eq!(moved[0].moved_to, artifact(&file_space(), "tests/b.rs"));
    // The old name has nothing behind it, which is told too.
    assert_eq!(absent.len(), 1);
    assert!(check.is_clean());
}

#[test]
fn a_name_moved_away_cannot_be_moved_again() {
    let mut app = approved(&["tests/a.rs"]);
    move_file(&app, "tests/a.rs", "tests/b.rs", None);
    block_on(app.moved(vec![moved("tests/a.rs", "tests/b.rs")])).unwrap();
    // A copy in two steps: no pin reads as a.rs any more.
    app.ports().put_file("tests/c.rs", b"tests/a.rs");
    let err = block_on(app.moved(vec![moved("tests/a.rs", "tests/c.rs")])).unwrap_err();
    assert!(
        matches!(err, ShelfError::MovedRefused(MovedRefusal::NotKnown { .. })),
        "{err:?}"
    );
    assert_eq!(app.ports().append_count(), 3);
    // Moved on from where its pin is read now, it is accepted; and back too.
    app.ports().remove_file("tests/b.rs");
    block_on(app.moved(vec![moved("tests/b.rs", "tests/c.rs")])).unwrap();
    app.ports().remove_file("tests/c.rs");
    app.ports().put_file("tests/a.rs", b"tests/a.rs");
    block_on(app.moved(vec![moved("tests/c.rs", "tests/a.rs")])).unwrap();
    assert_eq!(app.ports().append_count(), 5);
    assert_eq!(standing(&app, "SP-2"), Standing::Approved(Freshness::Fresh));
}

#[test]
fn a_name_pinned_as_absent_after_its_move_cannot_be_moved_again() {
    let mut app = approved(&["tests/a.rs"]);
    move_file(&app, "tests/a.rs", "tests/b.rs", None);
    block_on(app.moved(vec![moved("tests/a.rs", "tests/b.rs")])).unwrap();
    // Approved again with the link still on a.rs: a pin of a.rs as absent.
    block_on(app.judge_all(vec![JudgeRequest {
        judgement: Judgement::Approve {
            until: None,
            reason: None,
            again: true,
        },
        ..approve("SP-2")
    }]))
    .unwrap();
    app.ports().put_file("tests/c.rs", b"c");
    let err = block_on(app.moved(vec![moved("tests/a.rs", "tests/c.rs")])).unwrap_err();
    assert!(
        matches!(err, ShelfError::MovedRefused(MovedRefusal::NotKnown { .. })),
        "{err:?}"
    );
    assert_eq!(app.ports().append_count(), 4);
}

#[test]
fn a_name_only_a_pending_document_links_is_not_known() {
    // SP-2 is linked to tests/a.rs and never approved.
    let fake = FakeShelf::new().with_files(vec![
        fr(1, "slow", &[]),
        sp_tested(2, "cache", &["FR-1"], &["tests/a.rs"]),
    ]);
    fake.put_file("tests/b.rs", b"b");
    let mut app = ShelfApp::new(fake);
    let err = block_on(app.moved(vec![moved("tests/a.rs", "tests/b.rs")])).unwrap_err();
    assert!(
        matches!(err, ShelfError::MovedRefused(MovedRefusal::NotKnown { .. })),
        "{err:?}"
    );
    assert_eq!(err.class(), ErrorClass::Request);
    assert_eq!(app.ports().append_count(), 0);
}

#[test]
fn a_new_approval_after_a_move_pins_the_new_name_and_repeats_nothing() {
    let mut app = approved(&["tests/a.rs"]);
    move_file(&app, "tests/a.rs", "tests/b.rs", None);
    app.ports()
        .edit(sp_tested(2, "cache", &["FR-1"], &["tests/b.rs"]), 2);
    block_on(app.moved(vec![moved("tests/a.rs", "tests/b.rs")])).unwrap();
    assert_eq!(standing(&app, "SP-2"), Standing::Approved(Freshness::Fresh));
    // Fresh, and yet no repeat: the approval that stands pinned the old name.
    let run = block_on(app.judge_all(vec![approve("SP-2")])).unwrap();
    assert!(run.left_out.is_empty(), "{:?}", run.left_out);
    assert_eq!(run.judged.len(), 1);
    // The one made now is repeated by the next.
    let run = block_on(app.judge_all(vec![approve("SP-2")])).unwrap();
    assert!(run.judged.is_empty());
    assert_eq!(run.left_out.len(), 1);
}

#[test]
fn refused_moves_write_nothing() {
    let mut app = approved(&["tests/a.rs", "tests/c.rs"]);
    app.ports().put_file("tests/b.rs", b"b");
    let refused = |app: &mut ShelfApp<FakeShelf>, from: &str, to: &str| {
        block_on(app.moved(vec![moved(from, to)])).unwrap_err()
    };

    // A copy: the old name is still there.
    let err = refused(&mut app, "tests/a.rs", "tests/b.rs");
    assert!(
        matches!(
            err,
            ShelfError::MovedRefused(MovedRefusal::FromPresent { .. })
        ),
        "{err:?}"
    );
    assert_eq!(err.class(), ErrorClass::Refused);

    // Nothing at the new name.
    app.ports().remove_file("tests/a.rs");
    let err = refused(&mut app, "tests/a.rs", "tests/z.rs");
    assert!(
        matches!(err, ShelfError::MovedRefused(MovedRefusal::ToAbsent { .. })),
        "{err:?}"
    );
    assert_eq!(err.class(), ErrorClass::Refused);

    // A name the adapter cannot tell about now.
    app.ports()
        .cannot_tell(&file_space(), "tests/b.rs", "the disk is offline");
    let err = refused(&mut app, "tests/a.rs", "tests/b.rs");
    assert!(
        matches!(err, ShelfError::MovedRefused(MovedRefusal::ToUntold { .. })),
        "{err:?}"
    );
    app.ports().can_tell(&file_space(), "tests/b.rs");

    // The two names are one.
    let err = refused(&mut app, "tests/a.rs", "tests/a.rs");
    assert!(
        matches!(err, ShelfError::MovedRefused(MovedRefusal::NotAMove(_))),
        "{err:?}"
    );
    assert_eq!(err.class(), ErrorClass::Request);

    // A name no pin is read as.
    app.ports().put_file("tests/y.rs", b"y");
    let err = refused(&mut app, "tests/x.rs", "tests/y.rs");
    assert!(
        matches!(err, ShelfError::MovedRefused(MovedRefusal::NotKnown { .. })),
        "{err:?}"
    );
    assert_eq!(err.class(), ErrorClass::Request);

    let err = block_on(app.moved(vec![])).unwrap_err();
    assert!(matches!(err, ShelfError::NothingRequested), "{err:?}");
    assert_eq!(app.ports().append_count(), 2);
}

#[test]
fn a_run_of_moves_is_written_whole_or_not_at_all() {
    let mut app = approved(&["tests/a.rs", "tests/c.rs"]);
    move_file(&app, "tests/a.rs", "tests/b.rs", None);
    move_file(&app, "tests/c.rs", "tests/d.rs", None);
    app.ports().edit(
        sp_tested(2, "cache", &["FR-1"], &["tests/b.rs", "tests/d.rs"]),
        2,
    );

    // A sound move, then a copy: nothing of the run is written.
    app.ports().put_file("tests/c.rs", b"tests/c.rs");
    let err = block_on(app.moved(vec![
        moved("tests/a.rs", "tests/b.rs"),
        moved("tests/c.rs", "tests/d.rs"),
    ]))
    .unwrap_err();
    assert!(
        matches!(
            err,
            ShelfError::MovedRefused(MovedRefusal::FromPresent { .. })
        ),
        "{err:?}"
    );
    assert_eq!(app.ports().append_count(), 2);
    app.ports().remove_file("tests/c.rs");

    let declared = block_on(app.moved(vec![
        moved("tests/a.rs", "tests/b.rs"),
        moved("tests/c.rs", "tests/d.rs"),
    ]))
    .unwrap();
    assert_eq!(declared.len(), 2);
    let records = app.ports().records();
    assert_eq!(
        records[2..],
        [
            Record::Moved(declared[0].clone()),
            Record::Moved(declared[1].clone())
        ]
    );
    assert_eq!(standing(&app, "SP-2"), Standing::Approved(Freshness::Fresh));
}

#[test]
fn withdrawing_a_move_reads_the_pin_as_written_again() {
    let mut app = approved(&["tests/a.rs"]);
    move_file(&app, "tests/a.rs", "tests/b.rs", None);
    app.ports()
        .edit(sp_tested(2, "cache", &["FR-1"], &["tests/b.rs"]), 2);
    let declared = block_on(app.moved(vec![moved("tests/a.rs", "tests/b.rs")])).unwrap();
    assert_eq!(standing(&app, "SP-2"), Standing::Approved(Freshness::Fresh));
    block_on(app.withdraw(WithdrawRequest {
        records: vec![declared[0].id()],
        reason: NonBlank::new("it was a copy after all").unwrap(),
        actor: None,
    }))
    .unwrap();
    assert!(matches!(
        causes(&app, "SP-2").as_slice(),
        [Cause::ArtifactLinksChanged { .. }]
    ));
}

#[test]
fn a_link_left_on_the_old_name_with_no_declaration_is_broken() {
    // What the issue starts from: the file moved, nothing declared.
    let app = approved(&["tests/a.rs"]);
    move_file(&app, "tests/a.rs", "tests/b.rs", None);
    assert!(matches!(
        causes(&app, "SP-2").as_slice(),
        [Cause::ArtifactGone { .. }]
    ));
}
