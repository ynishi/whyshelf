//! The next step: what waits on a person, decided in one place.
//!
//! Every view that says "run this next" asks [`next_step`], and the order in which
//! the reasons for waiting are looked at is its alone:
//!
//! ```text
//!   Invalid                                       -> Fix
//!   the first upstream problem
//!     Unresolved (a store this environment
//!     cannot read, further up)                    -> OpenWithPrivateStore
//!     any, while the entity is Blocked            -> ApproveUpstream (the root of that chain)
//!   Blocked by something no entity names          -> (nothing to point at)
//!   Modified, or Stale                            -> Reapprove
//!   Draft                                         -> Approve (an item whose tests do
//!                                                    not carry its pins yet: Implement)
//!   Approved and current
//!     a Spec whose items are all integrated
//!     and whose hypotheses nobody measured        -> Measure
//!     an item approved at a revision that has
//!     not reached the baseline                    -> AwaitIntegration
//! ```
//!
//! The problems of the upstreams are given, already ordered, by
//! [`upstream_problems`](super::lint::upstream_problems): this module reads the
//! first and keeps no order of its own for them. A command line, a tool server or a
//! report turns a [`NextStep`] into its own words; the rule is not theirs.
//!
//! # When a measurement is due
//!
//! [`CmDue`] says a Spec is waiting for its measurement: it has items, every one of
//! them has reached the baseline, and no CM was recorded for its hypotheses as they
//! are (a CM, a pass as much as a failure, is a record, not an approval). The CM
//! records are read from the whole set of approvals with the duplicate registrations
//! folded, so a measurement recorded before two branches merged still counts.

use super::approval::{ApprovalSet, Gate};
use super::evaluate::Cause;
use super::id::EntityId;
use super::lint::UpstreamProblem;
use super::spec::Spec;
use super::state::{Approval, EntityState, Integration, ItemProgress, Upstream};

/// Whether a Spec is waiting for its measurement.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CmDue {
    /// Every item is integrated and the hypotheses are not measured yet.
    Due,
    /// Not yet (an item is not integrated, or there is none), or already measured.
    NotDue,
}

impl CmDue {
    /// Whether the Spec's measurement is due.
    ///
    /// `items` are the integration states of the items of every Design of the
    /// Spec. The hash of the hypotheses is the Spec's own.
    pub fn of<'a>(
        items: impl IntoIterator<Item = &'a Integration>,
        ledger: &ApprovalSet,
        spec: &Spec,
    ) -> Self {
        let mut items = items.into_iter().peekable();
        let all_integrated = items.peek().is_some()
            && items.all(|integration| matches!(integration, Integration::Integrated { .. }));
        let measured = ledger
            .matching(Gate::Cm, spec.id().entity(), spec.hyp_hash())
            .next()
            .is_some();
        if all_integrated && !measured {
            Self::Due
        } else {
            Self::NotDue
        }
    }
}

/// What a next step is asked about, beyond the state of the entity.
#[derive(Debug, Clone, Copy)]
pub enum NextInput<'a> {
    /// A Finding or a Design: the gate that approves it.
    Doc {
        /// The gate.
        gate: Gate,
    },
    /// A Spec (gate CS).
    Spec {
        /// Whether its measurement is due.
        cm_due: CmDue,
    },
    /// A work item (gate CA).
    Item {
        /// Which of its pins the tests carry.
        progress: &'a ItemProgress,
        /// Whether its approved revision reached the baseline.
        integration: &'a Integration,
    },
}

impl NextInput<'_> {
    fn gate(&self) -> Gate {
        match self {
            Self::Doc { gate } => *gate,
            Self::Spec { .. } => Gate::Cs,
            Self::Item { .. } => Gate::Ca,
        }
    }
}

/// What waits on a person, for one entity.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NextStep {
    /// The file is Invalid: fix it.
    Fix,
    /// Something it stands on is in a store this environment cannot read: open the
    /// project where the private store is. It comes before every other step, because
    /// none of them can be carried out without it.
    OpenWithPrivateStore,
    /// Something upstream must be approved first: the root of the chain that blocks
    /// this entity.
    ApproveUpstream {
        /// The upstream entity to start from.
        root: EntityId,
    },
    /// The approval no longer holds (edited, or an upstream changed): run the gate
    /// again.
    Reapprove(Gate),
    /// Run the gate and approve.
    Approve(Gate),
    /// An item whose tests do not carry all its pins yet.
    Implement,
    /// Every item is integrated: record the measurement.
    Measure,
    /// Approved at a revision that has not reached the baseline.
    AwaitIntegration,
}

/// The upstream step, if the upstream asks for one.
fn from_upstream(
    state: &EntityState,
    causes: &[Cause],
    problems: &[(EntityId, UpstreamProblem)],
) -> Option<NextStep> {
    let (upstream, problem) = problems.first()?;
    if *problem == UpstreamProblem::Unresolved {
        return Some(NextStep::OpenWithPrivateStore);
    }
    if !matches!(state.upstream(), Upstream::Blocked(_)) {
        return None;
    }
    // A Blocked entity names the root of the chain through that upstream, not the
    // root of some other upstream's chain.
    let root = causes
        .iter()
        .find(|cause| cause.through() == *upstream)?
        .via();
    Some(NextStep::ApproveUpstream { root })
}

/// The step the state asks for, before what an approved entity may still be
/// waiting for.
fn from_state(
    state: &EntityState,
    causes: &[Cause],
    problems: &[(EntityId, UpstreamProblem)],
    input: &NextInput<'_>,
) -> Option<NextStep> {
    let gate = input.gate();
    let upstream = from_upstream(state, causes, problems);
    match (state.approval(), state.upstream()) {
        (Approval::Invalid(_), _) => Some(NextStep::Fix),
        _ if upstream.is_some() => upstream,
        // Blocked by something no entity of the model names: nothing to point at.
        (_, Upstream::Blocked(_)) => None,
        (Approval::Modified { .. }, _) | (_, Upstream::Stale(_)) => Some(NextStep::Reapprove(gate)),
        (Approval::Draft, _) => Some(match input {
            NextInput::Item { progress, .. } if progress.is_implementing() => NextStep::Implement,
            _ => NextStep::Approve(gate),
        }),
        (Approval::Approved, Upstream::Ok) => None,
    }
}

/// What an entity waits for, if it waits for anything.
///
/// `state` and `causes` are the entity's own; `problems` are those of its upstreams,
/// in the order of [`upstream_problems`](super::lint::upstream_problems).
pub fn next_step(
    state: &EntityState,
    causes: &[Cause],
    problems: &[(EntityId, UpstreamProblem)],
    input: NextInput<'_>,
) -> Option<NextStep> {
    from_state(state, causes, problems, &input).or(match input {
        NextInput::Doc { .. } => None,
        NextInput::Spec { cm_due: CmDue::Due } => Some(NextStep::Measure),
        NextInput::Spec {
            cm_due: CmDue::NotDue,
        } => None,
        NextInput::Item { integration, .. } => match integration {
            Integration::ApprovedPending { .. } => Some(NextStep::AwaitIntegration),
            // Nothing to do about a baseline this clone cannot see: a clone that has
            // it says more.
            Integration::Unapproved
            | Integration::Integrated { .. }
            | Integration::Unknown { .. } => None,
        },
    })
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::*;
    use crate::domain::approval::test_support::{approve, cm_pass, cs};
    use crate::domain::classification::fold;
    use crate::domain::event::DocKind;
    use crate::domain::event::test_support::register_doc;
    use crate::domain::finding::test_support::finding;
    use crate::domain::hash::ContentHash;
    use crate::domain::id::test_support::{eid, sid};
    use crate::domain::nonempty::NonEmptyVec;
    use crate::domain::revision::RevisionId;
    use crate::domain::spec::test_support::spec;
    use crate::domain::state::{
        BlockReason, Completeness, InvalidReason, StaleReason, UnresolvedReason,
    };

    fn state(approval: Approval, upstream: Upstream) -> EntityState {
        EntityState::new(approval, upstream, Completeness::Complete)
    }

    fn invalid() -> Approval {
        Approval::Invalid(NonEmptyVec::new(
            InvalidReason::DanglingRef { upstream: eid(9) },
            vec![],
        ))
    }

    fn modified() -> Approval {
        Approval::Modified {
            approved: ContentHash::raw(b"old"),
        }
    }

    fn blocked() -> Upstream {
        Upstream::Blocked(NonEmptyVec::new(
            BlockReason::UpstreamUnapproved { upstream: eid(1) },
            vec![],
        ))
    }

    fn stale() -> Upstream {
        Upstream::Stale(NonEmptyVec::new(
            StaleReason::UpstreamStale { upstream: eid(1) },
            vec![],
        ))
    }

    fn pins(names: &[&str]) -> BTreeSet<crate::domain::pin::PinId> {
        names.iter().map(|n| n.parse().unwrap()).collect()
    }

    fn rev() -> RevisionId {
        RevisionId::from_validated("r1".into()).unwrap()
    }

    /// A chain of causes for a block that goes through upstream 1 and starts at 7.
    fn causes_through_one() -> Vec<Cause> {
        vec![
            Cause::for_test(eid(7), eid(1)),
            Cause::for_test(eid(1), eid(1)),
        ]
    }

    const DOC: NextInput<'static> = NextInput::Doc { gate: Gate::Cr };

    #[test]
    fn invalid_comes_first_whatever_else_is_true() {
        let problems = [(eid(1), UpstreamProblem::Unresolved)];
        for upstream in [Upstream::Ok, blocked(), stale()] {
            assert_eq!(
                next_step(&state(invalid(), upstream), &[], &problems, DOC),
                Some(NextStep::Fix)
            );
        }
    }

    #[test]
    fn a_store_that_cannot_be_read_comes_before_every_other_step() {
        let problems = [
            (eid(1), UpstreamProblem::Unresolved),
            (eid(2), UpstreamProblem::NotApproved),
        ];
        for approval in [Approval::Draft, Approval::Approved, modified()] {
            for upstream in [Upstream::Ok, blocked(), stale()] {
                assert_eq!(
                    next_step(&state(approval.clone(), upstream), &[], &problems, DOC),
                    Some(NextStep::OpenWithPrivateStore)
                );
            }
        }
    }

    #[test]
    fn a_blocked_entity_names_the_root_of_the_chain_through_the_first_problem() {
        let causes = causes_through_one();
        let problems = [(eid(1), UpstreamProblem::NotApproved)];
        assert_eq!(
            next_step(&state(Approval::Draft, blocked()), &causes, &problems, DOC),
            Some(NextStep::ApproveUpstream { root: eid(7) })
        );
        // A chain through another upstream is not the one to follow.
        let elsewhere = [(eid(2), UpstreamProblem::NotApproved)];
        assert_eq!(
            next_step(&state(Approval::Draft, blocked()), &causes, &elsewhere, DOC),
            None,
            "blocked by something no entity names: nothing to point at"
        );
        assert_eq!(
            next_step(&state(Approval::Draft, blocked()), &[], &[], DOC),
            None
        );
    }

    #[test]
    fn an_upstream_problem_alone_does_not_block_an_entity_that_is_not_blocked() {
        // Modified or Stale: the entity itself is to be approved again, and a
        // Modified upstream is the reason, not something to approve first.
        let problems = [(eid(1), UpstreamProblem::NotApproved)];
        assert_eq!(
            next_step(&state(modified(), Upstream::Ok), &[], &problems, DOC),
            Some(NextStep::Reapprove(Gate::Cr))
        );
        assert_eq!(
            next_step(&state(Approval::Approved, stale()), &[], &[], DOC),
            Some(NextStep::Reapprove(Gate::Cr))
        );
        assert_eq!(
            next_step(&state(Approval::Draft, Upstream::Ok), &[], &[], DOC),
            Some(NextStep::Approve(Gate::Cr))
        );
        assert_eq!(
            next_step(&state(Approval::Approved, Upstream::Ok), &[], &[], DOC),
            None
        );
    }

    #[test]
    fn the_gate_of_each_kind_is_the_one_asked_for() {
        let draft = state(Approval::Draft, Upstream::Ok);
        let progress = ItemProgress::new(pins(&["PIN-A"]), pins(&["PIN-A"]));
        let unapproved = Integration::Unapproved;
        for (input, gate) in [
            (NextInput::Doc { gate: Gate::Ci }, Gate::Ci),
            (
                NextInput::Spec {
                    cm_due: CmDue::NotDue,
                },
                Gate::Cs,
            ),
            (
                NextInput::Item {
                    progress: &progress,
                    integration: &unapproved,
                },
                Gate::Ca,
            ),
        ] {
            assert_eq!(
                next_step(&draft, &[], &[], input),
                Some(NextStep::Approve(gate))
            );
        }
    }

    #[test]
    fn an_item_whose_tests_lack_a_pin_is_implementing_not_ready_to_approve() {
        let draft = state(Approval::Draft, Upstream::Ok);
        let unapproved = Integration::Unapproved;
        let lacking = ItemProgress::new(pins(&[]), pins(&["PIN-A"]));
        let carried = ItemProgress::new(pins(&["PIN-A"]), pins(&["PIN-A"]));
        let item = |progress| NextInput::Item {
            progress,
            integration: &unapproved,
        };
        assert_eq!(
            next_step(&draft, &[], &[], item(&lacking)),
            Some(NextStep::Implement)
        );
        assert_eq!(
            next_step(&draft, &[], &[], item(&carried)),
            Some(NextStep::Approve(Gate::Ca))
        );
        // Only a Draft is "implementing": a stale item is approved again as it is.
        assert_eq!(
            next_step(
                &state(Approval::Approved, stale()),
                &[],
                &[],
                item(&lacking)
            ),
            Some(NextStep::Reapprove(Gate::Ca))
        );
    }

    #[test]
    fn an_approved_item_waits_for_the_baseline_until_its_revision_reaches_it() {
        let approved = state(Approval::Approved, Upstream::Ok);
        let progress = ItemProgress::new(pins(&["PIN-A"]), pins(&["PIN-A"]));
        let ask = |integration: &Integration| {
            next_step(
                &approved,
                &[],
                &[],
                NextInput::Item {
                    progress: &progress,
                    integration,
                },
            )
        };
        assert_eq!(
            ask(&Integration::ApprovedPending { revision: rev() }),
            Some(NextStep::AwaitIntegration)
        );
        assert_eq!(ask(&Integration::Integrated { revision: rev() }), None);
        assert_eq!(ask(&Integration::Unapproved), None);
    }

    #[test]
    fn an_approved_spec_waits_for_its_measurement_only_when_it_is_due() {
        let approved = state(Approval::Approved, Upstream::Ok);
        let ask = |cm_due| next_step(&approved, &[], &[], NextInput::Spec { cm_due });
        assert_eq!(ask(CmDue::Due), Some(NextStep::Measure));
        assert_eq!(ask(CmDue::NotDue), None);
        // A Spec that is itself waiting for something else says that first.
        assert_eq!(
            next_step(
                &state(Approval::Approved, stale()),
                &[],
                &[],
                NextInput::Spec { cm_due: CmDue::Due }
            ),
            Some(NextStep::Reapprove(Gate::Cs))
        );
    }

    // --- when a measurement is due -------------------------------------------

    fn integrated() -> Integration {
        Integration::Integrated { revision: rev() }
    }

    fn pending() -> Integration {
        Integration::ApprovedPending { revision: rev() }
    }

    #[test]
    fn a_measurement_is_due_when_every_item_is_integrated_and_none_was_recorded() {
        let s = spec(11, &[1], &["PIN-A"]);
        let none = ApprovalSet::default();
        assert_eq!(
            CmDue::of(&[integrated(), integrated()], &none, &s),
            CmDue::Due
        );
        assert_eq!(
            CmDue::of(&[integrated(), pending()], &none, &s),
            CmDue::NotDue
        );
        assert_eq!(
            CmDue::of(&[integrated(), Integration::Unapproved], &none, &s),
            CmDue::NotDue
        );
        assert_eq!(
            CmDue::of(&[], &none, &s),
            CmDue::NotDue,
            "no item, nothing to wait for"
        );
    }

    fn ledger(events: &[crate::domain::event::Event]) -> ApprovalSet {
        ApprovalSet::from_events(events, &fold(events))
    }

    #[test]
    fn a_recorded_measurement_ends_the_wait_whether_it_passed_or_not() {
        let f1 = finding(1, "one");
        let s = spec(11, &[1], &["PIN-A"]);
        let a_cs = approve(
            cs(11, s.contract_hash(), s.hyp_hash(), &[(1, *f1.hash())]),
            &[],
        );
        let before = ledger(std::slice::from_ref(&a_cs));
        assert_eq!(CmDue::of(&[integrated()], &before, &s), CmDue::Due);
        let recorded = approve(cm_pass(11, s.hyp_hash(), a_cs.id()), &[]);
        let after = ledger(&[a_cs.clone(), recorded]);
        assert_eq!(CmDue::of(&[integrated()], &after, &s), CmDue::NotDue);
    }

    /// A measurement recorded against the duplicate identifier of a Spec that two
    /// branches registered separately still counts for the one that survived.
    #[test]
    fn a_measurement_recorded_before_two_registrations_merged_ends_the_wait() {
        let s = spec(1, &[1], &["PIN-A"]);
        let first = register_doc(1, DocKind::Spec, "docs/spec/a.md", None, 1);
        let second = register_doc(2, DocKind::Spec, "docs/spec/a.md", None, 1);
        let f1 = finding(1, "one");
        let a_cs = approve(
            cs(2, s.contract_hash(), s.hyp_hash(), &[(1, *f1.hash())]),
            &[second.id()],
        );
        let recorded = approve(cm_pass(2, s.hyp_hash(), a_cs.id()), &[a_cs.id()]);
        let l = ledger(&[first, second, a_cs, recorded]);
        assert_eq!(l.canonical(sid(2).entity()), sid(1).entity());
        assert_eq!(CmDue::of(&[integrated()], &l, &s), CmDue::NotDue);
    }

    #[test]
    fn a_measurement_of_other_hypotheses_does_not_end_the_wait() {
        let f1 = finding(1, "one");
        let s = spec(11, &[1], &["PIN-A"]);
        let a_cs = approve(
            cs(11, s.contract_hash(), s.hyp_hash(), &[(1, *f1.hash())]),
            &[],
        );
        let other_hash = ContentHash::raw(b"other hypotheses");
        let recorded = approve(cm_pass(11, &other_hash, a_cs.id()), &[]);
        let l = ledger(&[a_cs, recorded]);
        assert_eq!(CmDue::of(&[integrated()], &l, &s), CmDue::Due);
    }

    #[test]
    fn a_partial_draft_is_asked_to_open_the_store_not_to_approve() {
        let partial =
            Completeness::from_reasons(BTreeSet::from([UnresolvedReason::new(eid(5), eid(5))]));
        let draft = EntityState::new(Approval::Draft, Upstream::Ok, partial);
        assert_eq!(
            next_step(
                &draft,
                &[],
                &[(eid(5), UpstreamProblem::Unresolved)],
                NextInput::Spec {
                    cm_due: CmDue::NotDue
                }
            ),
            Some(NextStep::OpenWithPrivateStore)
        );
    }
}
