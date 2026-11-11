//! Tests of `judge` over the in-memory fake: an approval that would only repeat the
//! one that stands (the same frame, the same expiry) is left out, with no record,
//! unless the request says `again`; the rest of the run is judged as before. And
//! `judge approve --stale` leaves out, as a repeat, one stale only through an
//! upstream's upstream the run approved again, and lets what stands on it be
//! decided as any other.

use crate::domain::nonempty::NonBlank;
use crate::domain::shelf::freshness::{Freshness, Notice, Standing};
use crate::domain::shelf::ledger::{FrameRefusal, JudgedAt, JudgementId, Record};
use crate::domain::shelf::lookup::Reference;
use crate::domain::shelf::schema::{CompareDecl, LinkTargetDecl};

use super::fake_tests::{FakeShelf, block_on, docid, file_space, fr, sp, sp_tested};
use super::{
    ApproveStaleRequest, JudgeRequest, JudgedAll, Judgement, LeftOutWhy, SameMeaningAbout,
    SameMeaningRequest, ShelfApp, ShelfError,
};

fn r(text: &str) -> Reference {
    text.parse().unwrap()
}

fn approve_as(reference: &str, until: Option<i64>, again: bool) -> JudgeRequest {
    JudgeRequest {
        judgement: Judgement::Approve {
            until: until.map(JudgedAt::new),
            reason: None,
            again,
        },
        reference: r(reference),
        actor: None,
    }
}

fn approve(reference: &str) -> JudgeRequest {
    approve_as(reference, None, false)
}

fn verb(reference: &str, judgement: Judgement) -> JudgeRequest {
    JudgeRequest {
        judgement,
        reference: r(reference),
        actor: None,
    }
}

fn why(text: &str) -> NonBlank {
    NonBlank::new(text).unwrap()
}

/// FR-1, and SP-2 on it.
fn app() -> ShelfApp<FakeShelf> {
    ShelfApp::new(FakeShelf::new().with_files(vec![fr(1, "slow", &[]), sp(2, "cache", &["FR-1"])]))
}

/// The identity of the `n`th record written.
fn record_id(app: &ShelfApp<FakeShelf>, n: usize) -> JudgementId {
    app.ports().records()[n].id()
}

/// The documents left out, each with the approval it would have repeated.
fn left_repeating(run: &JudgedAll) -> Vec<(String, JudgementId)> {
    run.left_out
        .iter()
        .map(|left| {
            let LeftOutWhy::Repeats { approval } = left.why else {
                panic!("left out as a repeat: {left:?}");
            };
            (left.key.as_ref().unwrap().to_string(), approval)
        })
        .collect()
}

fn subjects(run: &JudgedAll) -> Vec<crate::domain::id::DocId> {
    run.judged.iter().map(|j| j.subject).collect()
}

#[test]
fn a_fresh_approval_named_again_is_left_out_and_nothing_is_written() {
    let mut app = app();
    block_on(app.judge(approve("FR-1"))).unwrap();
    let first = record_id(&app, 0);

    let run = block_on(app.judge(approve("FR-1"))).unwrap();
    assert!(run.judged.is_empty());
    assert_eq!(left_repeating(&run), [("finding:FR-1".to_owned(), first)]);
    assert_eq!(run.left_out[0].doc, docid(1));
    assert_eq!(app.ports().record_count(), 1);
    assert_eq!(app.ports().append_count(), 1);
}

#[test]
fn a_stale_approval_is_approved_again() {
    let mut app = app();
    block_on(app.judge(approve("FR-1"))).unwrap();
    app.ports().edit(fr(1, "slower", &[]), 1);
    let run = block_on(app.judge(approve("FR-1"))).unwrap();
    assert_eq!(subjects(&run), [docid(1)]);
    assert!(run.left_out.is_empty());
    assert_eq!(app.ports().record_count(), 2);
}

#[test]
fn again_writes_an_approval_that_would_repeat_a_fresh_one() {
    let mut app = app();
    block_on(app.judge(approve("FR-1"))).unwrap();
    let run = block_on(app.judge(approve_as("FR-1", None, true))).unwrap();
    assert_eq!(subjects(&run), [docid(1)]);
    assert!(run.left_out.is_empty());
    assert_eq!(app.ports().record_count(), 2);
}

#[test]
fn a_run_leaves_out_the_fresh_one_and_writes_the_others() {
    let mut app = app();
    block_on(app.judge(approve("FR-1"))).unwrap();
    let first = record_id(&app, 0);

    let run = block_on(app.judge_all(vec![approve("FR-1"), approve("SP-2")])).unwrap();
    assert_eq!(subjects(&run), [docid(2)]);
    assert_eq!(left_repeating(&run), [("finding:FR-1".to_owned(), first)]);
    assert_eq!(app.ports().record_count(), 2);
    // The spec stands on the approval of the finding that was left standing.
    let row = block_on(app.get(&r("SP-2"))).unwrap().row;
    assert_eq!(row.standing, Standing::Approved(Freshness::Fresh));
}

#[test]
fn a_run_that_leaves_out_every_one_writes_nothing_and_fails_nothing() {
    let mut app = app();
    block_on(app.judge_all(vec![approve("FR-1"), approve("SP-2")])).unwrap();
    let run = block_on(app.judge_all(vec![approve("FR-1"), approve("SP-2")])).unwrap();
    assert!(run.judged.is_empty());
    assert_eq!(
        left_repeating(&run),
        [
            ("finding:FR-1".to_owned(), record_id(&app, 0)),
            ("spec:SP-2".to_owned(), record_id(&app, 1)),
        ]
    );
    assert_eq!(app.ports().record_count(), 2);
    assert_eq!(app.ports().append_count(), 2);
}

#[test]
fn a_document_named_twice_in_a_run_is_approved_once() {
    let mut app = app();
    let run = block_on(app.judge_all(vec![approve("FR-1"), approve("FR-1")])).unwrap();
    assert_eq!(subjects(&run), [docid(1)]);
    // The second is decided on the ledger the first left: it would repeat it.
    assert_eq!(
        left_repeating(&run),
        [("finding:FR-1".to_owned(), run.judged[0].record.id())]
    );
    assert_eq!(app.ports().record_count(), 1);
}

#[test]
fn a_refusal_later_in_the_run_still_writes_nothing() {
    let mut app = app();
    block_on(app.judge(approve("FR-1"))).unwrap();
    let err = block_on(app.judge_all(vec![approve("FR-1"), approve("SP-2"), approve("SP-9")]))
        .unwrap_err();
    assert!(matches!(err, ShelfError::NotFound { .. }), "{err:?}");
    assert_eq!(app.ports().record_count(), 1);
}

#[test]
fn a_fresh_approval_with_a_name_pinned_as_absent_is_left_out() {
    // `tests/b.rs` is not there: it was pinned as absent, and still is.
    let fake = FakeShelf::new().with_files(vec![sp_tested(2, "cache", &[], &["tests/b.rs"])]);
    let mut app = ShelfApp::new(fake);
    block_on(app.judge(approve("SP-2"))).unwrap();
    let row = block_on(app.get(&r("SP-2"))).unwrap().row;
    assert_eq!(row.standing, Standing::Approved(Freshness::Fresh));
    assert!(
        row.notices
            .iter()
            .any(|n| matches!(n, Notice::ArtifactAbsent { .. })),
        "{:?}",
        row.notices
    );

    let run = block_on(app.judge(approve("SP-2"))).unwrap();
    assert_eq!(
        left_repeating(&run),
        [("spec:SP-2".to_owned(), record_id(&app, 0))]
    );
    assert_eq!(app.ports().record_count(), 1);
}

#[test]
fn a_fresh_approval_on_a_name_that_cannot_be_told_now_is_refused_as_before() {
    let fake = FakeShelf::new().with_files(vec![sp_tested(2, "cache", &[], &["tests/a.rs"])]);
    fake.put_file("tests/a.rs", b"assert!(fast);\n");
    let mut app = ShelfApp::new(fake);
    block_on(app.judge(approve("SP-2"))).unwrap();
    app.ports()
        .cannot_tell(&file_space(), "tests/a.rs", "the adapter is offline");
    // Its freshness is unchanged by a name that cannot be told...
    let row = block_on(app.get(&r("SP-2"))).unwrap().row;
    assert_eq!(row.standing, Standing::Approved(Freshness::Fresh));
    // ...but an approval cannot be made on it: it is refused, not left out.
    let err = block_on(app.judge(approve("SP-2"))).unwrap_err();
    assert!(
        matches!(
            err,
            ShelfError::Refused(FrameRefusal::ArtifactUnknown { .. })
        ),
        "{err:?}"
    );
    assert_eq!(app.ports().record_count(), 1);
}

#[test]
fn an_edit_declared_to_keep_the_meaning_is_approved_on_the_hash_now() {
    let mut app = app();
    block_on(app.judge(approve("FR-1"))).unwrap();
    app.ports().edit(fr(1, "slower", &[]), 1);
    block_on(app.same_meaning(vec![SameMeaningRequest {
        about: SameMeaningAbout::Doc(r("FR-1")),
        reason: why("a typo fixed"),
        actor: None,
        from: None,
    }]))
    .unwrap();
    let row = block_on(app.get(&r("FR-1"))).unwrap().row;
    assert_eq!(row.standing, Standing::Approved(Freshness::Fresh));
    // A new approval would pin the hash now, which the standing one did not.
    let run = block_on(app.judge(approve("FR-1"))).unwrap();
    assert_eq!(subjects(&run), [docid(1)]);
    assert!(run.left_out.is_empty());
    let row = block_on(app.get(&r("FR-1"))).unwrap().row;
    assert!(row.notices.is_empty(), "{:?}", row.notices);
}

#[test]
fn an_upstream_edit_declared_to_keep_the_meaning_is_approved_on_the_hash_now() {
    let mut app = app();
    block_on(app.judge_all(vec![approve("FR-1"), approve("SP-2")])).unwrap();
    app.ports().edit(fr(1, "slower", &[]), 1);
    block_on(app.same_meaning(vec![SameMeaningRequest {
        about: SameMeaningAbout::Doc(r("FR-1")),
        reason: why("a typo fixed"),
        actor: None,
        from: None,
    }]))
    .unwrap();
    // SP-2 is fresh, with a notice that its upstream is not the hash it pinned.
    let row = block_on(app.get(&r("SP-2"))).unwrap().row;
    assert_eq!(row.standing, Standing::Approved(Freshness::Fresh));
    assert!(
        matches!(row.notices.as_slice(), [Notice::UpstreamSameMeaning { .. }]),
        "{:?}",
        row.notices
    );
    let run = block_on(app.judge(approve("SP-2"))).unwrap();
    assert_eq!(subjects(&run), [docid(2)]);
}

#[test]
fn an_approval_with_another_expiry_is_written_and_one_with_the_same_is_left_out() {
    let mut app = app();
    block_on(app.judge(approve_as("FR-1", Some(2000), false))).unwrap();
    let same = block_on(app.judge(approve_as("FR-1", Some(2000), false))).unwrap();
    assert_eq!(
        left_repeating(&same),
        [("finding:FR-1".to_owned(), record_id(&app, 0))]
    );
    // None lapses later than 2000: it stands another way.
    let none = block_on(app.judge(approve("FR-1"))).unwrap();
    assert_eq!(subjects(&none), [docid(1)]);
    let later = block_on(app.judge(approve_as("FR-1", Some(3000), false))).unwrap();
    assert_eq!(subjects(&later), [docid(1)]);
    assert_eq!(app.ports().record_count(), 3);
}

#[test]
fn a_denied_document_is_approved_as_before() {
    let mut app = app();
    block_on(app.judge(verb("FR-1", Judgement::Deny { reason: None }))).unwrap();
    let run = block_on(app.judge(approve("FR-1"))).unwrap();
    assert_eq!(subjects(&run), [docid(1)]);
    assert_eq!(app.ports().record_count(), 2);
}

#[test]
fn an_approval_withdrawn_with_nothing_before_it_is_approved_as_before() {
    let mut app = app();
    block_on(app.judge(approve("FR-1"))).unwrap();
    block_on(app.judge(verb(
        "FR-1",
        Judgement::Revoke {
            reason: why("not read"),
        },
    )))
    .unwrap();
    let run = block_on(app.judge(approve("FR-1"))).unwrap();
    assert_eq!(subjects(&run), [docid(1)]);
    assert_eq!(app.ports().record_count(), 3);
}

#[test]
fn an_approval_withdrawn_over_an_earlier_fresh_one_leaves_that_one_to_repeat() {
    let mut app = app();
    block_on(app.judge(approve("FR-1"))).unwrap();
    app.ports().set_now(1500);
    block_on(app.judge(approve_as("FR-1", None, true))).unwrap();
    block_on(app.judge(verb(
        "FR-1",
        Judgement::Revoke {
            reason: why("one is enough"),
        },
    )))
    .unwrap();
    // The first approval stands again, fresh: a new one would repeat it.
    let run = block_on(app.judge(approve("FR-1"))).unwrap();
    assert_eq!(
        left_repeating(&run),
        [("finding:FR-1".to_owned(), record_id(&app, 0))]
    );
    assert_eq!(app.ports().record_count(), 3);
}

#[test]
fn the_other_verbs_write_as_before() {
    let mut app = app();
    for _ in 0..2 {
        let run = block_on(app.judge(verb("FR-1", Judgement::Deny { reason: None }))).unwrap();
        assert_eq!(subjects(&run), [docid(1)]);
        assert!(run.left_out.is_empty());
    }
    assert_eq!(app.ports().record_count(), 2);
    assert!(matches!(
        app.ports().records()[1],
        Record::Decision(ref d) if d.subject() == docid(1)
    ));
}

#[test]
fn an_upstream_whose_key_changed_is_pinned_again() {
    let mut app = app();
    block_on(app.judge_all(vec![approve("FR-1"), approve("SP-2")])).unwrap();
    // The book of findings is renamed in the declaration: FR-1's key is now
    // `lesson:FR-1`, and neither file changes. (The hash of a document covers its
    // key, so FR-1's hash moves too; the pin of the key is compared all the same.)
    {
        let mut decls = app.ports().decls.lock().unwrap();
        for book in decls.iter_mut() {
            if book.name == "finding" {
                book.name = "lesson".to_owned();
            }
            for link in &mut book.links {
                if link.target == LinkTargetDecl::Book("finding".to_owned()) {
                    link.target = LinkTargetDecl::Book("lesson".to_owned());
                }
            }
        }
    }
    // The frame taken now pins the key `lesson:FR-1`; the standing one pinned
    // `finding:FR-1`.
    let run = block_on(app.judge(approve("SP-2"))).unwrap();
    assert_eq!(subjects(&run), [docid(2)]);
    assert!(run.left_out.is_empty());
    assert_eq!(app.ports().record_count(), 3);
}

#[test]
fn the_same_approval_twice_over_a_pending_upstream_is_written_once() {
    let mut app = app();
    block_on(app.judge(approve("SP-2"))).unwrap();
    // The approval is stale from the start: FR-1 is pending.
    let row = block_on(app.get(&r("SP-2"))).unwrap().row;
    assert!(matches!(
        row.standing,
        Standing::Approved(Freshness::Degraded(_))
    ));
    let run = block_on(app.judge(approve("SP-2"))).unwrap();
    assert!(run.judged.is_empty());
    assert_eq!(
        left_repeating(&run),
        [("spec:SP-2".to_owned(), record_id(&app, 0))]
    );
    assert_eq!(app.ports().record_count(), 1);
}

#[test]
fn an_absent_artifact_under_a_changed_comparison_is_pinned_again() {
    // `tests/b.rs` is not there, then and now; the link's comparison changes.
    let fake = FakeShelf::new().with_files(vec![sp_tested(2, "cache", &[], &["tests/b.rs"])]);
    let mut app = ShelfApp::new(fake);
    block_on(app.judge(approve("SP-2"))).unwrap();
    {
        let mut decls = app.ports().decls.lock().unwrap();
        let spec = decls.iter_mut().find(|b| b.name == "spec").unwrap();
        spec.links
            .iter_mut()
            .find(|l| l.name == "tests")
            .unwrap()
            .compare = Some(CompareDecl {
            kind: "lines".to_owned(),
            ignore: vec!["//".to_owned()],
        });
    }
    let run = block_on(app.judge(approve("SP-2"))).unwrap();
    assert_eq!(subjects(&run), [docid(2)]);
    assert!(run.left_out.is_empty());
    assert_eq!(app.ports().record_count(), 2);
}

#[test]
fn approve_stale_leaves_out_as_a_repeat_one_stale_only_through_an_upstream_it_approved() {
    // FR-1 on FR-2 on FR-3, all approved; FR-3 is edited.
    let mut app = ShelfApp::new(FakeShelf::new().with_files(vec![
        fr(1, "top", &["FR-2"]),
        fr(2, "middle", &["FR-3"]),
        fr(3, "bottom", &[]),
    ]));
    block_on(app.judge_all(vec![approve("FR-3"), approve("FR-2"), approve("FR-1")])).unwrap();
    let first_of_fr1 = record_id(&app, 2);
    app.ports().edit(fr(3, "bottom, edited", &[]), 3);
    assert_eq!(block_on(app.stale()).unwrap().entries.len(), 3);

    let done = block_on(app.approve_stale(ApproveStaleRequest {
        reason: why("looked again"),
        actor: None,
        until: None,
    }))
    .unwrap();
    let judged: Vec<_> = done.judged.iter().map(|j| j.judged.subject).collect();
    assert_eq!(judged, [docid(3), docid(2)]);
    assert_eq!(done.left_out.len(), 1);
    assert_eq!(done.left_out[0].left.doc, docid(1));
    assert_eq!(
        done.left_out[0].left.why,
        LeftOutWhy::Repeats {
            approval: first_of_fr1
        }
    );
    assert!(block_on(app.stale()).unwrap().entries.is_empty());
    assert_eq!(app.ports().record_count(), 5);
}

#[test]
fn approve_stale_does_not_hold_back_what_stands_on_a_repeat() {
    // FR-1 on FR-2 on FR-3 on FR-4, all approved; FR-4 is edited.
    let mut app = ShelfApp::new(FakeShelf::new().with_files(vec![
        fr(1, "top", &["FR-2"]),
        fr(2, "upper", &["FR-3"]),
        fr(3, "lower", &["FR-4"]),
        fr(4, "bottom", &[]),
    ]));
    block_on(app.judge_all(vec![
        approve("FR-4"),
        approve("FR-3"),
        approve("FR-2"),
        approve("FR-1"),
    ]))
    .unwrap();
    let (of_fr2, of_fr1) = (record_id(&app, 2), record_id(&app, 3));
    app.ports().edit(fr(4, "bottom, edited", &[]), 4);
    assert_eq!(block_on(app.stale()).unwrap().entries.len(), 4);

    let done = block_on(app.approve_stale(ApproveStaleRequest {
        reason: why("looked again"),
        actor: None,
        until: None,
    }))
    .unwrap();
    let judged: Vec<_> = done.judged.iter().map(|j| j.judged.subject).collect();
    assert_eq!(judged, [docid(4), docid(3)]);
    // FR-2 repeats its approval, which stands; FR-1, on it, is decided as any other
    // and repeats too: neither is held back.
    let left: Vec<_> = done
        .left_out
        .iter()
        .map(|l| (l.left.doc, l.left.why.clone()))
        .collect();
    assert_eq!(
        left,
        [
            (docid(1), LeftOutWhy::Repeats { approval: of_fr1 }),
            (docid(2), LeftOutWhy::Repeats { approval: of_fr2 }),
        ]
    );
    assert!(block_on(app.stale()).unwrap().entries.is_empty());
    assert_eq!(app.ports().record_count(), 6);
}

#[test]
fn an_expiry_that_has_passed_is_refused_not_left_out() {
    let mut app = app();
    block_on(app.judge(approve_as("FR-1", Some(2000), false))).unwrap();
    app.ports().set_now(3000);
    // The same expiry, now past: no approval can be made with it, so none repeats.
    let err = block_on(app.judge(approve_as("FR-1", Some(2000), false))).unwrap_err();
    assert!(matches!(err, ShelfError::Decision(_)), "{err:?}");
    assert_eq!(app.ports().record_count(), 1);
}
