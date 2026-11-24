//! Tests of the gate: whether a change reaches a document (`Claim::of`, W1–W6 and
//! what cannot be told), where a document stands against the reach before the change
//! (`Place::of`), and the table of verdicts; each change is two real shelves read by
//! the real `impact`, in both directions.

use crate::domain::shelf::artifact::{ArtifactHash, ArtifactReadings, Comparison};
use crate::domain::shelf::build::{DocReading, RawProblem, ReadId};
use crate::domain::shelf::fixtures_tests::{
    approve, approved, deny, docid, fr, fr_titled, ledger, path, reading, schema, shelf,
    shelf_with, sp_testing,
};
use crate::domain::shelf::freshness::assess_unasked as assess;
use crate::domain::shelf::impact::{both_ways, impact};
use crate::domain::shelf::ledger::{JudgedAt, Record};
use crate::domain::shelf::reach::walk;

use super::*;

fn h(text: &str) -> ArtifactHash {
    Comparison::Bytes.fingerprint(text.as_bytes())
}

/// The finding `FR-<n>` with identity `n`, not valid: it has no title.
fn invalid(n: u128) -> DocReading {
    reading(&format!("findings/fr-{n}.md"), n, &format!("FR-{n}"), &[])
}

/// A file of the book of findings whose identity cannot be read.
fn unreadable(file: &str) -> DocReading {
    DocReading::Unparsed {
        origin: path(file),
        id: ReadId::Unreadable,
        why: RawProblem::NotAMapping,
    }
}

fn named(ns: &[u128]) -> NonEmptySet<DocId> {
    NonEmptySet::from_set(ns.iter().map(|n| docid(*n)).collect()).unwrap()
}

fn witnesses(ws: &[Witness]) -> NonEmptySet<Witness> {
    NonEmptySet::from_set(ws.iter().copied().collect()).unwrap()
}

/// One change: each end's shelf and ledger, the records each ledger was built from,
/// and each end's files of the ledger that are no record.
struct Change {
    before: (Shelf, Ledger),
    after: (Shelf, Ledger),
    records: (Vec<Record>, Vec<Record>),
    unreadable_records: (Vec<RelPath>, Vec<RelPath>),
}

impl Change {
    /// A change whose two ledgers are built from the same records (or none), so no
    /// record is at one end only.
    fn new(before: (Shelf, Ledger), after: (Shelf, Ledger)) -> Self {
        Self {
            before,
            after,
            records: (Vec::new(), Vec::new()),
            unreadable_records: (Vec::new(), Vec::new()),
        }
    }

    /// A change whose ledgers are built from these records, each end its own.
    fn of_records(before: (Shelf, Vec<Record>), after: (Shelf, Vec<Record>)) -> Self {
        Self {
            before: (before.0, ledger(before.1.clone())),
            after: (after.0, ledger(after.1.clone())),
            records: (before.1, after.1),
            unreadable_records: (Vec::new(), Vec::new()),
        }
    }

    /// The same change, with these files of each ledger that are no record.
    fn with_unreadable_records(mut self, before: &[&str], after: &[&str]) -> Self {
        let paths = |files: &[&str]| files.iter().map(|f| path(f)).collect();
        self.unreadable_records = (paths(before), paths(after));
        self
    }

    fn ends(&self) -> (End<'_>, End<'_>) {
        (
            End {
                shelf: &self.before.0,
                ledger: &self.before.1,
                records: &self.records.0,
                unreadable_records: &self.unreadable_records.0,
            },
            End {
                shelf: &self.after.0,
                ledger: &self.after.1,
                records: &self.records.1,
                unreadable_records: &self.unreadable_records.1,
            },
        )
    }

    /// Each identity's rows of the change and of the change read backwards, each end
    /// assessed once. The forward rows are `impact`'s, and each pair is of one
    /// identity, read each way (G1).
    fn rows(&self) -> Vec<(ImpactEntry, ImpactEntry)> {
        let at = JudgedAt::new(100);
        let was = assess(&self.before.0, &self.before.1, at);
        let is = assess(&self.after.0, &self.after.1, at);
        let (before, after) = ((&self.before.0, &was), (&self.after.0, &is));
        let rows = both_ways(before, after);
        let forward: Vec<ImpactEntry> = rows.iter().map(|(f, _)| f.clone()).collect();
        assert_eq!(
            forward,
            impact(before, after),
            "the forward rows are impact's"
        );
        let backward: Vec<ImpactEntry> = rows.iter().map(|(_, b)| b.clone()).collect();
        assert_eq!(backward, impact(after, before), "the backward rows too");
        assert!(
            rows.iter().all(|(f, b)| f.id() == b.id()),
            "paired by identity"
        );
        rows
    }

    /// Whether the change reaches the document `n`.
    fn claim(&self, n: u128) -> Claim {
        let (forward, backward) = self
            .rows()
            .into_iter()
            .find(|(forward, _)| forward.id() == docid(n))
            .unwrap_or_else(|| panic!("a row of {n}"));
        Claim::of(
            &forward,
            &backward,
            (&self.before.0, &self.before.1),
            (&self.after.0, &self.after.1),
            &Recorded::of(&self.records.0, &self.records.1),
        )
    }

    /// The gate, the walk down from `ns` on the shelf before.
    fn within(&self, ns: &[u128]) -> Within {
        let named = named(ns);
        let walked = walk(&named, &self.before.0);
        let (before, after) = self.ends();
        within(named, walked, before, after, &self.rows())
    }

    /// The verdict of the document `n` when `ns` are named.
    fn verdict(&self, ns: &[u128], n: u128) -> VerdictKind {
        self.within(ns)
            .entries()
            .iter()
            .find(|entry| entry.id() == docid(n))
            .map(|entry| entry.verdict().kind().clone())
            .unwrap_or_else(|| panic!("a verdict of {n}"))
    }
}

/// The records that approve each of `subjects` in order on `s`, as the fixtures'
/// `approved` builds its ledger.
fn approvals(s: &Shelf, subjects: &[u128]) -> Vec<Record> {
    let mut records = Vec::new();
    for subject in subjects {
        let so_far = ledger(records.clone());
        records.push(approve(s, &so_far, *subject, 1000 + subject, 10, None));
    }
    records
}

fn untold_of(claim: Claim) -> ClaimUntold {
    match claim {
        Claim::CannotTell(untold) => untold,
        other => panic!("cannot tell, got {other:?}"),
    }
}

/// FR-1 and SP-2 grounded in it and tested by `tests/a.rs`, approved while the file
/// held `a`.
fn tested() -> (impl Fn() -> Vec<DocReading>, Ledger) {
    let docs = || vec![fr(1, &[]), sp_testing(2, &["FR-1"], &["tests/a.rs"])];
    let then = shelf_with(docs(), &[("tests/a.rs", Some(h("a")))]);
    let ledger = approved(&then, &[1, 2]);
    (docs, ledger)
}

mod claim {
    use super::*;

    /// W1: an edit of the file an approval stands on adds a cause.
    #[test]
    fn a_change_that_adds_a_cause_reaches_the_document() {
        let (docs, ledger) = tested();
        let change = Change::new(
            (
                shelf_with(docs(), &[("tests/a.rs", Some(h("a")))]),
                ledger.clone(),
            ),
            (
                shelf_with(docs(), &[("tests/a.rs", Some(h("edited")))]),
                ledger,
            ),
        );
        assert_eq!(
            change.claim(2),
            Claim::Claimed(witnesses(&[Witness::AddsCause]))
        );
        assert_eq!(change.claim(1), Claim::NotClaimed);
    }

    /// W2: putting the file back as it was approved takes the cause away.
    #[test]
    fn a_change_that_makes_an_approval_fresh_again_takes_a_cause_away() {
        let (docs, ledger) = tested();
        let change = Change::new(
            (
                shelf_with(docs(), &[("tests/a.rs", Some(h("edited")))]),
                ledger.clone(),
            ),
            (shelf_with(docs(), &[("tests/a.rs", Some(h("a")))]), ledger),
        );
        assert_eq!(
            change.claim(2),
            Claim::Claimed(witnesses(&[Witness::RemovesCause]))
        );
    }

    /// F-70: a name the end before could not tell and the end after can (docs not
    /// built before, built after) changes the holdings and not the shelf: no witness
    /// stands, and what cannot be told is kept by direction, in the change's words.
    #[test]
    fn a_name_one_end_alone_cannot_tell_is_no_witness_and_is_kept_by_direction() {
        let (docs, ledger) = tested();
        let told = || shelf_with(docs(), &[("tests/a.rs", Some(h("a")))]);
        let untold = || Shelf::build(&schema(), docs(), &ArtifactReadings::default());
        let change = Change::new((untold(), ledger.clone()), (told(), ledger.clone()));
        let rows = change.rows();
        assert!(rows[1].0.changed(), "the holdings differ by the reading");
        let why = untold_of(change.claim(2));
        assert_eq!(why.adds(), None);
        let removes = why
            .removes()
            .expect("the change read backwards cannot tell");
        assert_eq!(removes.side(), Sides::Before);
        assert_eq!(removes.untold().len(), 1);
        assert_eq!((why.edit(), why.shelf()), (None, None));
        // The other way round, it is the change that cannot tell, from the end after.
        let change = Change::new((told(), ledger.clone()), (untold(), ledger));
        let why = untold_of(change.claim(2));
        assert_eq!(why.adds().map(MadeCannotTell::side), Some(Sides::After));
        assert_eq!(why.removes(), None);
    }

    /// W3: a second approval changes what was decided.
    #[test]
    fn a_second_approval_changes_what_was_decided() {
        let s = shelf(vec![fr(1, &[])]);
        let first = approve(&s, &ledger(Vec::new()), 1, 1001, 10, None);
        let second = approve(&s, &ledger(vec![first.clone()]), 1, 1002, 20, None);
        let change = Change::of_records((s.clone(), vec![first.clone()]), (s, vec![first, second]));
        assert_eq!(
            change.claim(1),
            Claim::Claimed(witnesses(&[Witness::Decided, Witness::Recorded]))
        );
    }

    /// W4: a document taken off the shelf (its file removed, or its book out of the
    /// declaration), and a gone one whose file is put back.
    #[test]
    fn a_document_taken_off_the_shelf_or_put_back_is_on_the_shelf_at_one_end() {
        let both = shelf(vec![fr(1, &[]), fr(2, &[])]);
        let approvals = approved(&both, &[1, 2]);
        let one = shelf(vec![fr(1, &[])]);
        let off = Change::new(
            (both.clone(), approvals.clone()),
            (one.clone(), approvals.clone()),
        );
        let Claim::Claimed(off) = off.claim(2) else {
            panic!("claimed");
        };
        assert!(off.contains(&Witness::OnShelf), "{off:?}");
        let back = Change::new((one, approvals.clone()), (both, approvals));
        let Claim::Claimed(back) = back.claim(2) else {
            panic!("claimed");
        };
        assert!(back.contains(&Witness::OnShelf), "{back:?}");
    }

    /// W4: a document `Unknown` before (a file whose identity cannot be read) and
    /// valid after is on the shelf at one end.
    #[test]
    fn a_document_unknown_at_one_end_and_valid_at_the_other_is_on_the_shelf_at_one_end() {
        let change = Change::new(
            (
                shelf(vec![fr(1, &[]), unreadable("findings/x.md")]),
                ledger(Vec::new()),
            ),
            (shelf(vec![fr(1, &[]), fr(5, &[])]), ledger(Vec::new())),
        );
        let Claim::Claimed(found) = change.claim(5) else {
            panic!("claimed");
        };
        assert!(found.contains(&Witness::OnShelf), "{found:?}");
    }

    /// A document on the shelf at neither end and `Unknown` at one or both is no
    /// witness: whether the change put it back or rewrote it cannot be told, and each
    /// end's unreadable files are kept.
    #[test]
    fn a_document_on_the_shelf_at_neither_end_and_unknown_cannot_be_told() {
        let with_five = shelf(vec![fr(1, &[]), fr(5, &[])]);
        let denied = ledger(vec![deny(&with_five, 5, 3001, 10)]);
        let gone = shelf(vec![fr(1, &[])]);
        let unknown = |file: &str| shelf(vec![fr(1, &[]), unreadable(file)]);
        // Gone before, Unknown after.
        let change = Change::new(
            (gone, denied.clone()),
            (unknown("findings/x.md"), denied.clone()),
        );
        let why = untold_of(change.claim(5));
        let at = why.shelf().expect("unknown at one end");
        assert_eq!(at.before(), None);
        assert_eq!(
            at.after()
                .map(|files| files.iter().cloned().collect::<Vec<_>>()),
            Some(vec![path("findings/x.md")])
        );
        // Unknown at both ends: both are kept.
        let change = Change::new(
            (unknown("findings/x.md"), denied.clone()),
            (unknown("findings/y.md"), denied),
        );
        let why = untold_of(change.claim(5));
        let at = why.shelf().expect("unknown at both ends");
        assert!(at.before().is_some() && at.after().is_some(), "{at:?}");
    }

    /// W5: the text of a pending document, and a valid document made invalid.
    #[test]
    fn a_change_of_content_is_an_edit_whatever_was_decided() {
        let change = Change::new(
            (shelf(vec![fr_titled(2, "one", &[])]), ledger(Vec::new())),
            (shelf(vec![fr_titled(2, "two", &[])]), ledger(Vec::new())),
        );
        assert_eq!(
            change.claim(2),
            Claim::Claimed(witnesses(&[Witness::Edited]))
        );
        let change = Change::new(
            (shelf(vec![fr(2, &[])]), ledger(Vec::new())),
            (shelf(vec![invalid(2)]), ledger(Vec::new())),
        );
        assert_eq!(
            change.claim(2),
            Claim::Claimed(witnesses(&[Witness::Edited]))
        );
    }

    /// A document invalid at both ends has no hash: whether its content changed
    /// cannot be told.
    #[test]
    fn a_document_invalid_at_both_ends_cannot_be_told() {
        let change = Change::new(
            (shelf(vec![invalid(2)]), ledger(Vec::new())),
            (shelf(vec![invalid(2)]), ledger(Vec::new())),
        );
        let why = untold_of(change.claim(2));
        assert_eq!(why.edit(), Some(&InvalidBoth));
        assert_eq!((why.adds(), why.removes(), why.shelf()), (None, None, None));
    }
}

mod place {
    use super::*;

    /// A document the change adds is outside the reach before unless it is named,
    /// whatever its links write.
    #[test]
    fn a_document_the_change_adds_is_unproven_unless_named() {
        let before = shelf(vec![fr(1, &[])]);
        let after = shelf(vec![fr(1, &[]), fr(3, &["FR-1"])]);
        // FR-1 is approved at both ends, so FR-3's approval holds after the change.
        let change = Change::of_records(
            (before.clone(), approvals(&before, &[1])),
            (after.clone(), approvals(&after, &[1, 3])),
        );
        assert_eq!(
            change.verdict(&[1], 3),
            VerdictKind::Unproven {
                why: UnprovenWhy::NotBefore,
                witnesses: witnesses(&[Witness::Decided, Witness::OnShelf, Witness::Recorded]),
            }
        );
        assert!(!change.within(&[1]).passes());
        assert!(matches!(
            change.verdict(&[1, 3], 3),
            VerdictKind::Inside {
                by: InsideBy::Named,
                ..
            }
        ));
        assert!(change.within(&[1, 3]).passes());
    }

    /// DEC-108: a link the change writes is not walked. A document outside the reach
    /// that the change gives a depends link to the named is still outside.
    #[test]
    fn a_link_to_the_named_the_change_writes_does_not_put_a_document_inside() {
        let change = Change::new(
            (shelf(vec![fr(1, &[]), fr(5, &[])]), ledger(Vec::new())),
            (
                shelf(vec![fr(1, &[]), fr(5, &["FR-1"])]),
                ledger(Vec::new()),
            ),
        );
        assert_eq!(
            change.verdict(&[1], 5),
            VerdictKind::Outside {
                witnesses: witnesses(&[Witness::Edited])
            }
        );
        assert!(!change.within(&[1]).passes());
    }

    /// A gone document whose file the change puts back is not on the shelf before.
    #[test]
    fn a_gone_document_put_back_is_unproven_unless_named() {
        let both = shelf(vec![fr(1, &[]), fr(2, &["FR-1"])]);
        let approvals = approved(&both, &[1, 2]);
        let change = Change::new(
            (shelf(vec![fr(1, &[])]), approvals.clone()),
            (both, approvals),
        );
        assert!(matches!(
            change.verdict(&[1], 2),
            VerdictKind::Unproven {
                why: UnprovenWhy::NotBefore,
                ..
            }
        ));
    }

    /// A document reached by the walk before is inside; one the walk does not reach is
    /// outside.
    #[test]
    fn what_depends_on_the_named_before_is_inside_and_the_rest_outside() {
        let docs = |title: &str| {
            vec![
                fr(1, &[]),
                fr_titled(2, title, &["FR-1"]),
                fr(4, &[]),
                fr_titled(5, title, &["FR-4"]),
            ]
        };
        let change = Change::new(
            (shelf(docs("one")), ledger(Vec::new())),
            (shelf(docs("two")), ledger(Vec::new())),
        );
        let within = change.within(&[1]);
        assert_eq!(within.named(), &named(&[1]));
        assert_eq!(
            within.reached_before(),
            &[docid(1), docid(2)].into_iter().collect()
        );
        assert_eq!(within.untold_before(), None);
        assert!(matches!(
            change.verdict(&[1], 2),
            VerdictKind::Inside {
                by: InsideBy::Reached,
                claim: InsideClaim::Witnessed(_),
            }
        ));
        assert_eq!(
            change.verdict(&[1], 5),
            VerdictKind::Outside {
                witnesses: witnesses(&[Witness::Edited])
            }
        );
        assert_eq!(change.verdict(&[1], 4), VerdictKind::NotClaimed);
        assert!(!within.passes());
    }

    /// An invalid file before may depend on the named: a document the change reaches
    /// outside the walk cannot be shown outside, and the count is carried.
    #[test]
    fn an_invalid_file_before_makes_what_is_reached_outside_unproven() {
        let change = Change::new(
            (
                shelf(vec![fr(1, &[]), fr_titled(4, "one", &[]), invalid(9)]),
                ledger(Vec::new()),
            ),
            (
                shelf(vec![fr(1, &[]), fr_titled(4, "two", &[]), invalid(9)]),
                ledger(Vec::new()),
            ),
        );
        let VerdictKind::Unproven {
            why: UnprovenWhy::ReachUntold(walk),
            witnesses: found,
        } = change.verdict(&[1], 4)
        else {
            panic!("unproven");
        };
        assert_eq!((walk.invalid(), walk.unreadable()), (1, 0));
        assert_eq!(found, witnesses(&[Witness::Edited]));
        // The invalid file itself, invalid at both ends, cannot be told and is away.
        assert!(matches!(
            change.verdict(&[1], 9),
            VerdictKind::Undetermined {
                place: Away::ReachUntold(_),
                ..
            }
        ));
    }

    /// A document `Unknown` before stands where the walk could not read.
    #[test]
    fn a_document_unknown_before_stands_where_the_walk_could_not_read() {
        let change = Change::new(
            (
                shelf(vec![fr(1, &[]), unreadable("findings/x.md")]),
                ledger(Vec::new()),
            ),
            (shelf(vec![fr(1, &[]), fr(5, &[])]), ledger(Vec::new())),
        );
        let VerdictKind::Unproven {
            why: UnprovenWhy::ReachUntold(walk),
            ..
        } = change.verdict(&[1], 5)
        else {
            panic!("unproven");
        };
        assert_eq!(walk.unreadable(), 1);
    }
}

mod unreadable {
    use super::*;

    /// No document is claimed, and the gate does not pass.
    fn blocked_with_nothing_claimed(within: &Within) {
        assert!(
            within
                .entries()
                .iter()
                .all(|entry| entry.verdict().kind() == &VerdictKind::NotClaimed),
            "no document is claimed"
        );
        assert!(
            !within.passes(),
            "what cannot be read is never read as passing"
        );
    }

    /// A file of the shelf whose identity cannot be read, at either end, may be any
    /// document the change touched: the gate carries it and does not pass, whether the
    /// change adds it, removes it or leaves it there (it may have edited it).
    #[test]
    fn a_file_whose_identity_cannot_be_read_at_either_end_keeps_the_gate_from_passing() {
        let clean = || shelf(vec![fr(1, &[])]);
        let broken = || shelf(vec![fr(1, &[]), unreadable("findings/x.md")]);
        let x = || vec![path("findings/x.md")];
        for (before, after, at_before, at_after) in [
            (clean(), broken(), vec![], x()),
            (broken(), clean(), x(), vec![]),
            (broken(), broken(), x(), x()),
        ] {
            let change = Change::new((before, ledger(Vec::new())), (after, ledger(Vec::new())));
            let within = change.within(&[1]);
            blocked_with_nothing_claimed(&within);
            let found = within.unreadable().expect("what cannot be read");
            assert_eq!(found.files_before(), at_before.as_slice());
            assert_eq!(found.files_after(), at_after.as_slice());
            assert!(found.records_before().is_empty() && found.records_after().is_empty());
        }
    }

    /// M1: a file of the ledger that is no record, at either end, may be a record about
    /// any document: the gate carries it and does not pass.
    #[test]
    fn a_file_of_the_ledger_that_is_no_record_keeps_the_gate_from_passing() {
        let s = || shelf(vec![fr(1, &[])]);
        let change = Change::new((s(), ledger(Vec::new())), (s(), ledger(Vec::new())))
            .with_unreadable_records(&[], &[".whyshelf/ledger/x.json"]);
        let within = change.within(&[1]);
        blocked_with_nothing_claimed(&within);
        let found = within.unreadable().expect("what cannot be read");
        assert_eq!(found.records_after(), [path(".whyshelf/ledger/x.json")]);
        assert!(found.records_before().is_empty());
        let change = Change::new((s(), ledger(Vec::new())), (s(), ledger(Vec::new())))
            .with_unreadable_records(&[".whyshelf/ledger/x.json"], &[]);
        blocked_with_nothing_claimed(&change.within(&[1]));
        // With nothing unreadable at either end, the same change passes.
        let change = Change::new((s(), ledger(Vec::new())), (s(), ledger(Vec::new())));
        assert_eq!(change.within(&[1]).unreadable(), None);
        assert!(change.within(&[1]).passes());
    }
}

mod recorded {
    use super::*;
    use crate::domain::actor::{Actor, Attribution};
    use crate::domain::nonempty::NonBlank;
    use crate::domain::shelf::fixtures_tests::jid;
    use crate::domain::shelf::ledger::{Exempt, ExemptOf, Revocation};
    use crate::domain::shelf::schema::{BookName, EntryName};

    /// The exemption `judgement` of FR-`doc` from `spec.grounds`, a link the fixtures
    /// declare with no cover: it moves neither what was decided nor any freshness.
    fn exempt(judgement: u128, doc: u128) -> Record {
        Record::Exempt(Exempt::new(
            jid(judgement),
            ExemptOf::new(
                docid(doc),
                BookName::new("spec").unwrap(),
                EntryName::new("grounds").unwrap(),
            ),
            Attribution::one(Actor::new("alice").unwrap()),
            JudgedAt::new(50),
            NonBlank::new("kept on purpose").unwrap(),
        ))
    }

    fn withdraw(judgement: u128, target: u128) -> Record {
        Record::Revocation(Revocation::new(
            jid(judgement),
            NonEmptySet::new(jid(target), []),
            Attribution::one(Actor::new("alice").unwrap()),
            JudgedAt::new(60),
            NonBlank::new("not needed").unwrap(),
        ))
    }

    /// W6: a record the change adds about a document reaches it, though it moves
    /// nothing else; and so does the withdrawal of one, through what it withdraws.
    #[test]
    fn a_record_the_change_adds_or_withdraws_reaches_the_document_it_is_about() {
        let s = || shelf(vec![fr(1, &[]), fr(4, &[])]);
        let only = Claim::Claimed(witnesses(&[Witness::Recorded]));
        let added = Change::of_records((s(), Vec::new()), (s(), vec![exempt(3001, 4)]));
        assert_eq!(added.claim(4), only);
        assert_eq!(added.claim(1), Claim::NotClaimed);
        assert_eq!(
            added.verdict(&[1], 4),
            VerdictKind::Outside {
                witnesses: witnesses(&[Witness::Recorded])
            }
        );
        let withdrawn = Change::of_records(
            (s(), vec![exempt(3001, 4)]),
            (s(), vec![exempt(3001, 4), withdraw(3002, 3001)]),
        );
        assert_eq!(withdrawn.claim(4), only);
        // A record of one identity rewritten at the end after reaches its document,
        // though what was decided stays the same.
        let s1 = shelf(vec![fr(1, &[])]);
        let first = approve(&s1, &ledger(Vec::new()), 1, 1001, 10, None);
        let rewritten = approve(&s1, &ledger(Vec::new()), 1, 1001, 20, None);
        assert_ne!(first, rewritten);
        let change = Change::of_records((s1.clone(), vec![first]), (s1, vec![rewritten]));
        assert_eq!(change.claim(1), only);
        // The same records at both ends reach nothing.
        let same = Change::of_records((s(), vec![exempt(3001, 4)]), (s(), vec![exempt(3001, 4)]));
        assert_eq!(same.claim(4), Claim::NotClaimed);
    }

    /// A record the change adds about a document `impact` has no row for (on the shelf
    /// at neither end, and no decision about it) gets a verdict all the same, by the
    /// one table: not before the change, so it does not pass unless named.
    #[test]
    fn a_record_about_a_document_with_no_row_gets_a_verdict_that_does_not_pass() {
        let s = || shelf(vec![fr(1, &[])]);
        let change = Change::of_records((s(), Vec::new()), (s(), vec![exempt(3001, 9)]));
        assert!(
            change.rows().iter().all(|(row, _)| row.id() != docid(9)),
            "impact has no row of it"
        );
        assert_eq!(
            change.verdict(&[1], 9),
            VerdictKind::Unproven {
                why: UnprovenWhy::NotBefore,
                witnesses: witnesses(&[Witness::Recorded]),
            }
        );
        assert!(!change.within(&[1]).passes());
        let ids: Vec<DocId> = change
            .within(&[1])
            .entries()
            .iter()
            .map(|e| e.id())
            .collect();
        assert_eq!(ids, [docid(1), docid(9)], "in the order of identities");
        assert!(matches!(
            change.verdict(&[1, 9], 9),
            VerdictKind::Inside {
                by: InsideBy::Named,
                ..
            }
        ));
    }
}

mod links {
    use super::*;
    use crate::domain::shelf::fixtures_tests::{list, retire, text};

    /// FR-2, which only informs FR-1 (its `related` link).
    fn informing() -> DocReading {
        reading(
            "findings/fr-2.md",
            2,
            "FR-2",
            &[("title", text("t")), ("related", list(&["FR-1"]))],
        )
    }

    /// A link whose target stops resolving names it by text, not identity: the
    /// document that only informs a named one the change removes or makes not valid is
    /// edited, and outside, though its own file is untouched.
    #[test]
    fn a_link_whose_target_stops_resolving_makes_the_linking_document_edited() {
        let both = shelf(vec![fr(1, &[]), informing()]);
        let removed = Change::new(
            (both.clone(), ledger(Vec::new())),
            (shelf(vec![informing()]), ledger(Vec::new())),
        );
        assert_eq!(
            removed.verdict(&[1], 2),
            VerdictKind::Outside {
                witnesses: witnesses(&[Witness::Edited])
            }
        );
        let invalid = Change::new(
            (both.clone(), ledger(Vec::new())),
            (shelf(vec![invalid(1), informing()]), ledger(Vec::new())),
        );
        assert_eq!(
            invalid.verdict(&[1], 2),
            VerdictKind::Outside {
                witnesses: witnesses(&[Witness::Edited])
            }
        );
        // A retired document is still on the shelf, so a link to it still resolves:
        // retiring one changes nothing of the documents that link to it.
        let retired = Change::of_records(
            (both.clone(), Vec::new()),
            (both.clone(), vec![retire(&both, 1, 4001, 10)]),
        );
        assert_eq!(retired.verdict(&[1], 2), VerdictKind::NotClaimed);
    }
}

mod verdict {
    use super::*;

    /// What a walk could not follow, from a real walk.
    fn unwalked() -> Unwalked {
        *walk(&named(&[1]), &shelf(vec![fr(1, &[]), invalid(9)]))
            .untold()
            .unwrap()
    }

    fn untold() -> ClaimUntold {
        ClaimUntold {
            adds: None,
            removes: None,
            edit: Some(InvalidBoth),
            shelf: None,
        }
    }

    /// Every cell of the table, and whether each passes; what cannot be told inside
    /// the reach is carried, not dropped.
    #[test]
    fn the_table_of_verdicts() {
        let w = witnesses(&[Witness::Edited]);
        let u = unwalked();
        let claims = [
            Claim::Claimed(w.clone()),
            Claim::CannotTell(untold()),
            Claim::NotClaimed,
        ];
        let places = [
            Place::Named,
            Place::Reached,
            Place::NotBefore,
            Place::Untold(u),
            Place::Outside,
        ];
        use InsideClaim::{CannotTell as Told, Witnessed};
        let expected = [
            [
                VerdictKind::Inside {
                    by: InsideBy::Named,
                    claim: Witnessed(w.clone()),
                },
                VerdictKind::Inside {
                    by: InsideBy::Reached,
                    claim: Witnessed(w.clone()),
                },
                VerdictKind::Unproven {
                    why: UnprovenWhy::NotBefore,
                    witnesses: w.clone(),
                },
                VerdictKind::Unproven {
                    why: UnprovenWhy::ReachUntold(u),
                    witnesses: w.clone(),
                },
                VerdictKind::Outside {
                    witnesses: w.clone(),
                },
            ],
            [
                VerdictKind::Inside {
                    by: InsideBy::Named,
                    claim: Told(untold()),
                },
                VerdictKind::Inside {
                    by: InsideBy::Reached,
                    claim: Told(untold()),
                },
                VerdictKind::Undetermined {
                    place: Away::NotBefore,
                    untold: untold(),
                },
                VerdictKind::Undetermined {
                    place: Away::ReachUntold(u),
                    untold: untold(),
                },
                VerdictKind::Undetermined {
                    place: Away::Outside,
                    untold: untold(),
                },
            ],
            std::array::from_fn(|_| VerdictKind::NotClaimed),
        ];
        for (claim, row) in claims.iter().zip(expected) {
            for (place, kind) in places.iter().zip(row) {
                let verdict = Verdict::of(claim.clone(), *place);
                assert_eq!(verdict.kind(), &kind, "{claim:?} at {place:?}");
                let passes = matches!(kind, VerdictKind::NotClaimed | VerdictKind::Inside { .. });
                assert_eq!(verdict.passes(), passes, "{claim:?} at {place:?}");
            }
        }
    }

    /// The gate passes only when every verdict does.
    #[test]
    fn the_gate_passes_when_every_verdict_does() {
        let (docs, approvals) = tested();
        let change = Change::new(
            (
                shelf_with(docs(), &[("tests/a.rs", Some(h("a")))]),
                approvals.clone(),
            ),
            (
                shelf_with(docs(), &[("tests/a.rs", Some(h("edited")))]),
                approvals,
            ),
        );
        assert!(change.within(&[1]).passes(), "SP-2 depends on FR-1");
        assert!(change.within(&[2]).passes());
        let unchanged = Change::new(
            (shelf(vec![fr(1, &[]), fr(4, &[])]), ledger(Vec::new())),
            (shelf(vec![fr(1, &[]), fr(4, &[])]), ledger(Vec::new())),
        );
        let within = unchanged.within(&[4]);
        assert!(within.passes());
        assert!(
            within
                .entries()
                .iter()
                .all(|entry| entry.verdict().kind() == &VerdictKind::NotClaimed)
        );
    }
}
