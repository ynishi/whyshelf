//! Tests of the gate of the stale report: each case of DEC-26.

use std::collections::{BTreeMap, BTreeSet};

use crate::domain::id::DocId;
use crate::domain::shelf::artifact::Listings;
use crate::domain::shelf::freshness::{Holding, HoldsAs};
use crate::domain::shelf::ledger::JudgedAt;
use crate::domain::shelf::lookup::Reference;
use crate::domain::shelf::schema::{
    BookRuleDecl, DecideDecl, GroupDecl, MemberDecl, RuleDecl, ShelfDecl,
};

use super::fake_tests::{FakeShelf, block_on, docid, file_space, fr, sp, sp_tested};
use super::load::load;
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

/// Every approval asks for the names of alice and bo.
fn asking_alice_and_bo() -> DecideDecl {
    let both = vec!["both".to_owned()];
    let member = |name: &str| MemberDecl {
        name: name.to_owned(),
        from: None,
        until: None,
    };
    let rule = || RuleDecl {
        default: Some(both.clone()),
        approvals: Some(2),
        ..RuleDecl::default()
    };
    DecideDecl {
        groups: vec![GroupDecl {
            name: "both".to_owned(),
            members: vec![member("alice"), member("bo")],
        }],
        shelf: Some(rule()),
        books: vec![
            BookRuleDecl {
                book: "finding".to_owned(),
                rule: rule(),
            },
            BookRuleDecl {
                book: "spec".to_owned(),
                rule: rule(),
            },
        ],
    }
}

/// The gate of `stale` is the fold of every document's holding, and its entries are
/// the degraded approvals: on a shelf with a degraded approval that stands on a name
/// that cannot be told, a degraded one, a fresh one that stands on such a name, one
/// awaiting a name, a pending document and a denied one.
#[test]
fn the_gate_is_the_fold_of_each_documents_holding() {
    let fake = FakeShelf::new().with_files(vec![
        fr(1, "slow", &[]),
        sp_tested(2, "cache", &["FR-1"], &["tests/a.rs"]),
        fr(3, "fast", &[]),
        sp_tested(4, "warm", &["FR-3"], &["tests/b.rs"]),
        fr(5, "pending", &[]),
        fr(6, "denied", &[]),
        fr(7, "awaiting", &[]),
    ]);
    fake.put_file("tests/a.rs", b"assert!(fast);\n");
    fake.put_file("tests/b.rs", b"assert!(warm);\n");
    let books = fake.decls.lock().unwrap().clone();
    *fake.written.lock().unwrap() = Some(ShelfDecl {
        decide: asking_alice_and_bo(),
        ..ShelfDecl::of_books(books)
    });
    let mut app = ShelfApp::new(fake);
    let by = |reference: &str, name: &str| JudgeRequest {
        by: super::fake_tests::named(name),
        ..approve(reference, None)
    };
    let mut asks = Vec::new();
    for reference in ["FR-1", "SP-2", "FR-3", "SP-4"] {
        asks.push(by(reference, "alice"));
        asks.push(by(reference, "bo"));
    }
    asks.push(by("FR-7", "alice"));
    asks.push(JudgeRequest {
        judgement: Judgement::Deny { reason: None },
        ..by("FR-6", "alice")
    });
    block_on(app.judge_all(asks)).unwrap();
    app.ports().edit(fr(1, "slower", &[]), 1);
    app.ports()
        .cannot_tell(&file_space(), "tests/a.rs", "the tree is not mounted");
    app.ports()
        .cannot_tell(&file_space(), "tests/b.rs", "the tree is not mounted");

    let report = block_on(app.stale()).unwrap();
    let loaded = block_on(load(app.ports())).unwrap();
    let assessment = loaded.assess(&Listings::NotAsked);
    let holdings: BTreeMap<DocId, Holding> = assessment
        .standings()
        .keys()
        .map(|id| (*id, assessment.holding(&loaded.shelf, *id)))
        .collect();
    let kind = |n: u128| match &holdings[&docid(n)] {
        Holding::NotThere => "not there",
        Holding::Pending => "pending",
        Holding::Against(_) => "against",
        Holding::Holds(HoldsAs::Fresh) => "fresh",
        Holding::Holds(HoldsAs::Awaiting) => "awaiting",
        Holding::CannotTell { .. } => "cannot tell",
        Holding::DoesNotHold { untold, .. } if untold.is_empty() => "does not hold",
        Holding::DoesNotHold { .. } => "does not hold, untold",
    };
    assert_eq!(
        (1..=7).map(kind).collect::<Vec<_>>(),
        [
            "does not hold",
            "does not hold, untold",
            "fresh",
            "cannot tell",
            "pending",
            "against",
            "awaiting"
        ]
    );
    assert_eq!(report.gate(), Gate::fold(holdings.values().map(Gate::from)));
    assert_eq!(report.gate(), Gate::DoesNotHold);
    let entries: BTreeSet<DocId> = report.entries.iter().map(|e| e.row.id).collect();
    let degraded: BTreeSet<DocId> = assessment.degraded().map(|(id, _)| id).collect();
    assert_eq!(entries, degraded);
    // The names that cannot be told, of the degraded approval and of the fresh one.
    let unknown: Vec<(DocId, &str)> = report
        .unknown
        .iter()
        .map(|u| (u.doc, u.artifact.name().as_str()))
        .collect();
    assert_eq!(
        unknown,
        [(docid(2), "tests/a.rs"), (docid(4), "tests/b.rs")]
    );
}

#[test]
fn the_fold_takes_what_does_not_hold_over_what_cannot_be_told_over_what_holds() {
    use Gate::{CannotTell, DoesNotHold, Holds};
    assert_eq!(Gate::fold([]), Holds);
    assert_eq!(Gate::fold([Holds, Holds]), Holds);
    assert_eq!(Gate::fold([Holds, CannotTell]), CannotTell);
    assert_eq!(Gate::fold([CannotTell, DoesNotHold, Holds]), DoesNotHold);
    assert_eq!(Gate::fold([DoesNotHold, CannotTell]), DoesNotHold);
    // Each holding's one gate: no approval is no reason to fail.
    assert_eq!(Gate::from(&Holding::NotThere), Holds);
    assert_eq!(Gate::from(&Holding::Pending), Holds);
    assert_eq!(Gate::from(&Holding::Holds(HoldsAs::Awaiting)), Holds);
}
