//! Tests of who may decide, read against the records: a record whose name the rule
//! of its book does not admit is not used and is a problem, until a revocation the
//! rule admits withdraws it; the write path accepts only what is admitted; `since`,
//! the times of a member, the kinds a rule does not name, and the book a record is
//! read with (the key `book`, the book its document is in now, `[decide]`).

use std::collections::BTreeSet;

use crate::domain::actor::Actor;
use crate::domain::canon::Canon;
use crate::domain::shelf::artifact::ArtifactReadings;
use crate::domain::shelf::fixtures_tests::{decls, docid, fr, jid, shelf, spaces};
use crate::domain::shelf::schema::{
    BookRuleDecl, BuiltinPresets, DecideDecl, DecideKind, GroupDecl, GroupName, MemberDecl,
    RuleDecl, RuleTime, Schema, ShelfDecl,
};

use super::*;

fn member(name: &str, until: Option<i64>) -> MemberDecl {
    MemberDecl {
        name: name.to_owned(),
        from: None,
        until: until.map(RuleTime::from_millis),
    }
}

/// Who may decide: `maintainers` (`ana`, and `cy` until 20) and `others` (`dee`);
/// in the book `finding`, `kinds` (each to `maintainers`), with `since`; the records
/// of no book, `others`.
fn rule(kinds: &[DecideKind], since: Option<i64>) -> DecideDecl {
    let maintainers = vec!["maintainers".to_owned()];
    DecideDecl {
        groups: vec![
            GroupDecl {
                name: "maintainers".to_owned(),
                members: vec![member("ana", None), member("cy", Some(20))],
            },
            GroupDecl {
                name: "others".to_owned(),
                members: vec![member("dee", None)],
            },
        ],
        shelf: Some(RuleDecl {
            default: Some(vec!["others".to_owned()]),
            ..RuleDecl::default()
        }),
        books: vec![BookRuleDecl {
            book: "finding".to_owned(),
            rule: RuleDecl {
                kinds: kinds.iter().map(|k| (*k, maintainers.clone())).collect(),
                default: None,
                since: since.map(RuleTime::from_millis),
                approvals: None,
            },
        }],
    }
}

/// The findings `FR-1` and `FR-2`, under `decide`.
fn ruled(decide: DecideDecl) -> Shelf {
    let mut decl = ShelfDecl::of_books(decls());
    decl.decide = decide;
    let schema = Schema::declared(decl, &BuiltinPresets::new(), &spaces()).unwrap();
    Shelf::build(
        &schema,
        vec![fr(1, &[]), fr(2, &[])],
        &ArtifactReadings::new(),
    )
}

fn approve_by(shelf: &Shelf, subject: u128, judgement: u128, by: &str, at: i64) -> Record {
    let frame = frame_approve(
        shelf,
        &Ledger::build(Vec::new(), &Admit::of(shelf)),
        docid(subject),
    )
    .unwrap();
    Record::Decision(
        Decision::approve(
            jid(judgement),
            frame,
            None,
            Actor::new(by).unwrap(),
            JudgedAt::new(at),
            None,
        )
        .unwrap(),
    )
}

fn deny_by(shelf: &Shelf, subject: u128, judgement: u128, by: &str, at: i64) -> Record {
    Record::Decision(
        Decision::deny(
            jid(judgement),
            frame_present(shelf, docid(subject)).unwrap(),
            Actor::new(by).unwrap(),
            JudgedAt::new(at),
            None,
        )
        .unwrap(),
    )
}

fn revoke_by(judgement: u128, targets: &[u128], by: &str, at: i64) -> Record {
    let targets: BTreeSet<JudgementId> = targets.iter().map(|n| jid(*n)).collect();
    Record::Revocation(Revocation::new(
        jid(judgement),
        NonEmptySet::from_set(targets).unwrap(),
        Actor::new(by).unwrap(),
        JudgedAt::new(at),
        NonBlank::new("not admitted").unwrap(),
    ))
}

/// The record with its key `book` taken out, or set to `book`.
fn with_book(record: &Record, book: Option<&str>) -> Record {
    let Canon::Map(mut map) = record.to_canon() else {
        panic!("a record is a map");
    };
    match book {
        None => map.remove("book"),
        Some(book) => map.insert("book".to_owned(), Canon::str(book)),
    };
    Record::from_canon(&Canon::Map(map)).unwrap()
}

fn not_admitted(problems: &[LedgerProblem]) -> Vec<(JudgementId, Option<JudgementId>)> {
    problems
        .iter()
        .filter_map(|p| match p {
            LedgerProblem::NotAdmitted(not) => Some((not.record, not.target)),
            _ => None,
        })
        .collect()
}

#[test]
fn a_project_that_declares_no_rule_admits_every_name() {
    let shelf = shelf(vec![fr(1, &[])]);
    let record = approve_by(&shelf, 1, 1001, "bo", 10);
    let ledger = Ledger::build(vec![record.clone()], &Admit::of(&shelf));
    assert!(ledger.problems().is_empty());
    assert!(matches!(
        ledger.status_of(docid(1)),
        DeclaredStatus::Approved { .. }
    ));
    assert!(ledger.accept(record).is_ok());
}

#[test]
fn a_record_the_rule_does_not_admit_is_not_used_and_is_a_problem_of_the_ledger() {
    let shelf = ruled(rule(&[DecideKind::Approve], None));
    let by_bo = approve_by(&shelf, 1, 1001, "bo", 10);
    let ledger = Ledger::build(vec![by_bo.clone()], &Admit::of(&shelf));
    assert_eq!(ledger.status_of(docid(1)), DeclaredStatus::Pending);
    assert!(ledger.decisions().next().is_none());
    let [LedgerProblem::NotAdmitted(not)] = ledger.problems() else {
        panic!("one problem: {:?}", ledger.problems());
    };
    assert_eq!(not.record, jid(1001));
    assert_eq!(not.kind, DecideKind::Approve);
    assert_eq!(
        not.book,
        RecordBook::Named(BookName::new("finding").unwrap())
    );
    assert_eq!(
        not.groups,
        NonEmptySet::new(GroupName::new("maintainers").unwrap(), [])
    );
    assert!(not.to_string().contains("`bo` may not approve"), "{not}");

    let by_ana = approve_by(&shelf, 1, 1002, "ana", 10);
    let ledger = Ledger::build(vec![by_bo, by_ana], &Admit::of(&shelf));
    assert!(matches!(
        ledger.status_of(docid(1)),
        DeclaredStatus::Approved { judgement, .. } if judgement == jid(1002)
    ));
}

#[test]
fn a_record_is_accepted_for_writing_only_when_the_rule_admits_it() {
    let shelf = ruled(rule(&[DecideKind::Approve], None));
    let ledger = Ledger::build(Vec::new(), &Admit::of(&shelf));
    let refused = ledger
        .accept(approve_by(&shelf, 1, 1001, "bo", 10))
        .unwrap_err();
    assert_eq!((refused.record, refused.target), (jid(1001), None));
    let admitted = ledger
        .accept(approve_by(&shelf, 1, 1002, "ana", 10))
        .unwrap();
    assert_eq!(admitted.record().id(), jid(1002));
}

#[test]
fn a_record_not_admitted_is_withdrawn_by_a_name_admitted_to_revoke_and_by_no_other() {
    let shelf = ruled(rule(&[DecideKind::Approve, DecideKind::Revoke], None));
    let admit = Admit::of(&shelf);
    let by_bo = approve_by(&shelf, 1, 1001, "bo", 10);
    let base = Ledger::build(vec![by_bo.clone()], &admit);
    assert_eq!(base.check_withdraw(jid(1001)), Ok(()));

    // bo may not revoke in the book: the revocation withdraws nothing, and is refused.
    let refused = revoke_by(2001, &[1001], "bo", 20);
    let not = base.accept(refused.clone()).unwrap_err();
    assert_eq!((not.record, not.target), (jid(2001), Some(jid(1001))));
    let ledger = Ledger::build(vec![by_bo.clone(), refused], &admit);
    assert_eq!(
        not_admitted(ledger.problems()),
        [(jid(2001), Some(jid(1001))), (jid(1001), None)]
    );

    // ana may: the problem is gone.
    let withdrawn = revoke_by(2002, &[1001], "ana", 20);
    assert!(base.accept(withdrawn.clone()).is_ok());
    let ledger = Ledger::build(vec![by_bo.clone(), withdrawn.clone()], &admit);
    assert!(ledger.problems().is_empty(), "{:?}", ledger.problems());
    assert_eq!(ledger.status_of(docid(1)), DeclaredStatus::Pending);

    // bo's refused revocation, written by hand beside it, is put right with the
    // record: a revocation is never withdrawn, so it would otherwise fail check for
    // ever.
    let refused = revoke_by(2001, &[1001], "bo", 20);
    let ledger = Ledger::build(vec![by_bo, refused, withdrawn], &admit);
    assert!(ledger.problems().is_empty(), "{:?}", ledger.problems());
}

#[test]
fn a_kind_the_rule_neither_names_nor_defaults_is_open() {
    let shelf = ruled(rule(&[DecideKind::Approve], None));
    let ledger = Ledger::build(vec![deny_by(&shelf, 1, 1001, "bo", 10)], &Admit::of(&shelf));
    assert!(ledger.problems().is_empty());
    assert!(matches!(
        ledger.status_of(docid(1)),
        DeclaredStatus::Denied { .. }
    ));
}

#[test]
fn a_record_made_before_since_is_unruled() {
    let shelf = ruled(rule(&[DecideKind::Approve], Some(100)));
    let admit = Admit::of(&shelf);
    let before = Ledger::build(vec![approve_by(&shelf, 1, 1001, "bo", 99)], &admit);
    assert!(before.problems().is_empty());
    let from = Ledger::build(vec![approve_by(&shelf, 1, 1001, "bo", 100)], &admit);
    assert_eq!(not_admitted(from.problems()), [(jid(1001), None)]);
}

#[test]
fn a_member_is_in_the_group_only_for_the_records_made_in_its_time() {
    let shelf = ruled(rule(&[DecideKind::Approve], None));
    let admit = Admit::of(&shelf);
    let early = Ledger::build(vec![approve_by(&shelf, 1, 1001, "cy", 19)], &admit);
    assert!(early.problems().is_empty());
    let late = Ledger::build(vec![approve_by(&shelf, 1, 1001, "cy", 20)], &admit);
    assert_eq!(not_admitted(late.problems()), [(jid(1001), None)]);
}

#[test]
fn a_record_is_read_with_the_book_it_names_else_the_one_its_document_is_in_else_decide() {
    let shelf = ruled(rule(&[DecideKind::Approve], None));
    let admit = Admit::of(&shelf);
    let by_ana = approve_by(&shelf, 1, 1001, "ana", 10);
    let Record::Decision(decision) = &by_ana else {
        panic!("a decision");
    };
    assert_eq!(decision.book().map(BookName::as_str), Some("finding"));

    // Naming no book (as a record made before records named one), it is read with
    // the book its document is in now.
    let unnamed = with_book(&by_ana, None);
    assert_eq!(
        admit.book_of(&unnamed),
        RecordBook::Now(BookName::new("finding").unwrap())
    );
    assert!(Ledger::build(vec![unnamed], &admit).problems().is_empty());

    // Naming a book not declared now, it is read with `[decide]`, where only `dee` may.
    let elsewhere = with_book(&by_ana, Some("gone"));
    assert_eq!(admit.book_of(&elsewhere), RecordBook::Shelf);
    let ledger = Ledger::build(vec![elsewhere], &admit);
    let [LedgerProblem::NotAdmitted(not)] = ledger.problems() else {
        panic!("one problem: {:?}", ledger.problems());
    };
    assert_eq!(not.book, RecordBook::Shelf);
}

#[test]
fn a_record_that_names_no_book_keeps_its_form_and_one_that_names_one_reads_back() {
    let shelf = ruled(rule(&[DecideKind::Approve], None));
    let by_ana = approve_by(&shelf, 1, 1001, "ana", 10);
    // In a project that declares a rule, a decision made now names its book.
    let Canon::Map(named) = by_ana.to_canon() else {
        panic!("a map");
    };
    assert!(named.contains_key("book"));
    let unnamed = with_book(&by_ana, None);
    let Canon::Map(map) = unnamed.to_canon() else {
        panic!("a map");
    };
    assert!(!map.contains_key("book"));
    assert_eq!(Record::from_canon(&by_ana.to_canon()).unwrap(), by_ana);
    assert_ne!(unnamed.hash(), by_ana.hash());
}
