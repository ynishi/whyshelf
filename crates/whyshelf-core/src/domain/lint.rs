//! Machine checks: what can be decided without judgement.
//!
//! Three families, all pure functions of values:
//!
//! - **CHECK, layer 1.** One function per gate ([`check_cr`], [`check_cs`],
//!   [`check_ci`], [`check_ca`], [`check_cm`]) returns every violation found, not
//!   the first. An approval (layer 2) may be recorded only when the list is empty.
//!   The machine looks at form: links resolve and can be read here, upstream is
//!   approved and current, pins match in both directions, tests pass. Whether a claim follows from its
//!   evidence is for the human who approves.
//! - **Leak lint.** [`leak_lint`] stops a public document from carrying the alias
//!   or the title of a private entity. A public document may point at a private
//!   entity only by its opaque `private:` identifier.
//! - **Drift of generated files.** [`export_drift`] tells whether a generated file
//!   was edited by hand or has fallen behind its sources.
//!
//! # The violations of a gate, and how to clear each
//!
//! A check returns a list of [`CheckViolation`]s; the command line prints each as one
//! line (the third column) and exits with status 1 when there is any. The list is
//! complete: fix them together, then run the check again.
//!
//! | violation | gate | the command line says | what it means | how to clear it |
//! |---|---|---|---|---|
//! | `EmptyBody` | CR | the Finding has no body | nothing is written under the front matter | write what was seen and how it is known |
//! | `MeasuredDataNotLabelledMeasured` | CR | a measured_data Finding must be labelled `evidence: measured` | a measurement carries a weaker label | set `evidence: measured`, or change `finding_type` if it was not measured |
//! | `Upstream` | CS, CI, CA | upstream *x* is not ready: *why* | an entity this one stands on cannot be depended on yet; *why* is an [`UpstreamProblem`] (next table) | clear the problem upstream first, nearest to the Finding first |
//! | `NoPinsDeclared` | CS | the Spec declares no pin | `accept.pins` is empty | declare at least one `PIN-*` or `UT-*` with its contract sentence |
//! | `NoHypsDeclared` | CS | the Spec declares no hypothesis | `accept.hyp` is empty | declare at least one `HYP-*` with a metric and a target |
//! | `PinCoverage` | CI | pin coverage: *issue* | the Design and the Spec disagree about a pin; *issue* is a [`CoverageIssue`] | assign the pin to an item, or declare it in the Spec |
//! | `PinMissingInTests` | CA | the item declares *pin* but no test carries it | no `@pin: ...` tag for it in the scanned directories, at the revision | add the tag to the test that holds the contract, commit, check again |
//! | `PinUndeclaredInTests` | CA | a test carries *pin* but the Spec never declared it | a tag in the scanned directories at the revision that the Spec does not list (it counts wherever it is, not only on this item's tests) | declare it in the Spec, or take the tag off the test |
//! | `TestsFailed` | CA | the tests did not pass | `test.command` exited with a non-zero status at the revision | make the tests pass at that revision |
//! | `ItemNotInDesign` | CA | the Design no longer has this item | the Design's item at that number is not the item that was handed to the check (a guard: the application takes the item from the same Design, so it is not expected) | run the command again |
//! | `CsApprovalNotFound` | CM | no CS approval fixed the hypotheses of this Spec | there is no unrevoked CS approval to compare with | approve the Spec (CS) first |
//! | `HypothesisChanged` | CM | the hypotheses changed after CS fixed them | `accept.hyp` is not what the CS approval fixed | put the hypotheses back, or approve CS again and measure against the new ones |
//!
//! What can be wrong with an upstream ([`UpstreamProblem`]), in the order in which
//! [`upstream_problems`] puts them (the first is the one to look at first):
//!
//! | problem | the command line says | what it means | how to clear it |
//! |---|---|---|---|
//! | `Missing` | it does not exist | the reference names no entity | correct the alias or identifier, or register the file it should name |
//! | `Invalid` | it is invalid | the upstream file does not parse, or breaks a rule | fix the upstream file (`whyshelf status` shows the reasons) |
//! | `Unresolved` | it cannot be read here | the upstream, or something above it, is in a store this environment cannot read (a `private:` reference in a clone), so it cannot be checked or pinned | open the project where the private store is, and check there |
//! | `NotApproved` | it is not approved | the upstream is a Draft, or was edited after its approval (Modified) | run the upstream's own gate and approve it |
//! | `Stale` | it is stale | the upstream is approved, but something above it changed | run the gates again from the top: the first stale one, then the next |
//! | `Blocked` | it is blocked | the upstream is approved, but something above it is not ready | follow the NEXT column of `whyshelf status` from the top |
//!
//! One problem is reported per upstream, the first that applies: an upstream that is
//! Approved, Stale and Partial is `Unresolved`, because staleness cannot be cleared
//! where the entity cannot be read.
//!
//! # Generated files
//!
//! A generated file (the map, a status or changelog Markdown) carries a marker with
//! its own `export` hash: the hash of its content with the marker removed, rendered
//! deterministically. Two facts can then be told apart. If the content found in the
//! file no longer hashes to the marker, somebody edited the file. If it does, but a
//! fresh render from today's events and files does not, the file is out of date
//! (after a merge, say). Extracting the content without the marker and rendering
//! afresh are the storage adapter's work; the comparison is here.
//!
//! The marker deliberately does not mention the events it was built from, so an
//! event unrelated to the file does not change the file.

use std::collections::BTreeMap;
use std::num::NonZeroU32;

use super::approval::TestOutcome;
use super::approval::{ApprovalSet, Attestation};
use super::design::{CoverageIssue, Design, WorkItem};
use super::evaluate::{Evaluated, ImplView, ItemKey};
use super::event::EventId;
use super::finding::{EvidenceLabel, Finding, FindingSource};
use super::hash::ContentHash;
use super::id::{Alias, DesignId, EntityId, FindingId, SpecId};
use super::nonempty::NonBlank;
use super::pin::PinId;
use super::spec::Spec;
use super::state::{Approval, Completeness, EntityState, Upstream};

/// What is wrong with an upstream entity a gate depends on.
///
/// The variants are in the order of [`upstream_problems`]: the first that applies
/// to an upstream wins, and the problems of several upstreams are listed by it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum UpstreamProblem {
    /// The entity is not there (a dangling reference).
    ///
    /// Fix: correct the alias or identifier, or register the file it should name.
    Missing,
    /// The entity is invalid or did not parse.
    ///
    /// Fix: repair the upstream file.
    Invalid,
    /// The entity, or something it stands on, is in a store this environment
    /// cannot read, so it can be neither checked nor pinned here.
    ///
    /// Fix: open the project where the private store is, and check there.
    Unresolved,
    /// The entity has no approval for its current content (a Draft, or edited after
    /// its approval).
    ///
    /// Fix: run the upstream's own gate and approve it.
    NotApproved,
    /// The entity is approved but stale.
    ///
    /// Fix: approve again from the top, the first stale entity first.
    Stale,
    /// The entity is approved but blocked.
    ///
    /// Fix: follow the next command of the status from the top.
    Blocked,
}

/// One reason a gate's machine check fails. The module documentation has a table
/// of the violations with the command line's sentence and how to clear each.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CheckViolation {
    /// CR: the Finding has no body.
    ///
    /// Fix: write the body under the front matter.
    EmptyBody,
    /// CR: a measured-data Finding is not labelled as measured.
    ///
    /// Fix: set `evidence: measured`, or use another `finding_type`.
    MeasuredDataNotLabelledMeasured,
    /// An upstream entity is not ready to be depended on.
    ///
    /// Fix: see [`UpstreamProblem`] for the problem named.
    Upstream {
        /// The upstream entity.
        upstream: EntityId,
        /// What is wrong with it.
        problem: UpstreamProblem,
    },
    /// CS: the Spec declares no PIN or unit-test tag.
    ///
    /// Fix: declare at least one under `accept.pins`.
    NoPinsDeclared,
    /// CS: the Spec declares no hypothesis.
    ///
    /// Fix: declare at least one under `accept.hyp`.
    NoHypsDeclared,
    /// CI: the Design and the Spec disagree about a pin.
    ///
    /// Fix: assign every pin of the Spec to an item, and assign no pin the Spec
    /// does not declare.
    PinCoverage(CoverageIssue),
    /// CA: the item's pin is not carried by any test at the revision.
    ///
    /// Fix: add the `@pin: ...` tag to the test, commit, and check again.
    PinMissingInTests {
        /// The pin.
        pin: PinId,
    },
    /// CA: a test carries a tag the Spec never declared.
    ///
    /// Fix: declare the tag in the Spec, or remove it from the test.
    PinUndeclaredInTests {
        /// The tag.
        pin: PinId,
    },
    /// CA: the tests did not pass.
    ///
    /// Fix: make `test.command` pass at the revision.
    TestsFailed,
    /// CA: the item is not (or no longer) the one the Design has at that number.
    ///
    /// Fix: run the command again.
    ItemNotInDesign,
    /// CM: the CS approval that should have fixed the hypotheses is not there.
    ///
    /// Fix: approve the Spec (CS) first.
    CsApprovalNotFound,
    /// CM: the hypotheses were changed after the CS approval fixed them.
    ///
    /// Fix: restore the hypotheses, or approve CS again and measure against the
    /// new ones.
    HypothesisChanged {
        /// The hash the CS approval fixed.
        fixed: ContentHash,
        /// The hash today.
        current: ContentHash,
    },
}

/// The one problem of an upstream that is there, the first that applies.
fn problem_of(state: &EntityState) -> Option<UpstreamProblem> {
    match state.approval() {
        Approval::Invalid(_) => Some(UpstreamProblem::Invalid),
        _ if state.completeness().is_partial() => Some(UpstreamProblem::Unresolved),
        Approval::Draft | Approval::Modified { .. } => Some(UpstreamProblem::NotApproved),
        Approval::Approved => match state.upstream() {
            Upstream::Stale(_) => Some(UpstreamProblem::Stale),
            Upstream::Blocked(_) => Some(UpstreamProblem::Blocked),
            Upstream::Ok => None,
        },
    }
}

/// What stands in the way of depending on each upstream of an entity.
///
/// `target` is the completeness of the entity that depends on them; each upstream
/// is its identifier and its state, or `None` when no entity of that identifier is
/// in the model. An upstream that is not there is `Unresolved` when the target
/// wrote it with a mark into a store this environment cannot read (it names the
/// upstream in its completeness), else `Missing`.
///
/// One problem is returned per upstream, the first that applies in the order of
/// [`UpstreamProblem`]: Invalid, then Unresolved (an upstream that is itself
/// partial counts, so a gate stops on whatever it cannot read further up), then
/// NotApproved, Stale, Blocked. Across upstreams the result is ordered by that same
/// order, the order the upstreams were given breaking ties. The gates and the
/// `NEXT` column both read this and keep no order of their own.
pub fn upstream_problems(
    target: &Completeness,
    upstreams: &[(EntityId, Option<&EntityState>)],
) -> Vec<(EntityId, UpstreamProblem)> {
    let mut out: Vec<(EntityId, UpstreamProblem)> = upstreams
        .iter()
        .filter_map(|(id, found)| {
            let problem = match found {
                None if target.names_upstream(*id) => Some(UpstreamProblem::Unresolved),
                None => Some(UpstreamProblem::Missing),
                Some(state) => problem_of(state),
            };
            problem.map(|problem| (*id, problem))
        })
        .collect();
    out.sort_by_key(|(_, problem)| *problem);
    out
}

fn upstream_violations(
    target: &Completeness,
    upstreams: &[(EntityId, Option<&EntityState>)],
) -> Vec<CheckViolation> {
    upstream_problems(target, upstreams)
        .into_iter()
        .map(|(upstream, problem)| CheckViolation::Upstream { upstream, problem })
        .collect()
}

/// CR, layer 1: the Finding is complete as a record.
///
/// The metadata each type requires is already held by the type. What remains is
/// the body, and the label a measurement must carry.
pub fn check_cr(finding: &Finding) -> Vec<CheckViolation> {
    let mut out = Vec::new();
    if finding.body().trim().is_empty() {
        out.push(CheckViolation::EmptyBody);
    }
    if matches!(finding.source(), FindingSource::MeasuredData { .. })
        && finding.evidence() != EvidenceLabel::Measured
    {
        out.push(CheckViolation::MeasuredDataNotLabelledMeasured);
    }
    out
}

/// CS, layer 1: every ground exists and can be read here, passed CR and is
/// current; pins and hypotheses are declared.
///
/// `state` is the Spec's own state: its completeness says which grounds were
/// written with a mark into a store this environment cannot read. Such a ground is
/// a violation: it cannot be checked, and the hash of it cannot be recorded.
pub fn check_cs(
    spec: &Spec,
    state: &EntityState,
    findings: &BTreeMap<FindingId, Evaluated<Finding>>,
) -> Vec<CheckViolation> {
    let upstreams: Vec<_> = spec
        .grounds()
        .iter()
        .map(|g| (g.entity(), findings.get(g).map(Evaluated::state)))
        .collect();
    let mut out = upstream_violations(state.completeness(), &upstreams);
    if spec.pins().is_empty() {
        out.push(CheckViolation::NoPinsDeclared);
    }
    if spec.hyps().is_empty() {
        out.push(CheckViolation::NoHypsDeclared);
    }
    out
}

/// CI, layer 1: the Spec passed CS, is current and can be read in full here; pins
/// are covered in both directions.
///
/// An approved Spec that is Partial (something it stands on cannot be read here) is
/// a violation: whether it is still current cannot be told. `state` is the
/// Design's own state, for the Spec it wrote with a mark into a store this
/// environment cannot read.
///
/// That the Crux gives a reason for each rejected option is held by its type, so
/// there is nothing to check for it here.
pub fn check_ci(
    design: &Design,
    state: &EntityState,
    specs: &BTreeMap<SpecId, Evaluated<Spec>>,
) -> Vec<CheckViolation> {
    let found = specs.get(&design.spec());
    let mut out = upstream_violations(
        state.completeness(),
        &[(design.spec().entity(), found.map(Evaluated::state))],
    );
    if let Some(spec) = found.and_then(Evaluated::entity) {
        out.extend(
            design
                .check_pin_coverage(spec)
                .into_iter()
                .map(CheckViolation::PinCoverage),
        );
    }
    out
}

/// CA, layer 1: the Design passed CI, is current and can be read in full here; the
/// item's pins and the tests' tags match in both directions at the revision; the
/// tests passed.
///
/// An approved Design that is Partial, through its Spec or further up, is a
/// violation, for the same reason as at CI. An item is named by its number inside
/// its Design, never by a mark, so it has nothing of its own that cannot be read.
pub fn check_ca(
    key: &ItemKey,
    item: &WorkItem,
    designs: &BTreeMap<DesignId, Evaluated<Design>>,
    specs: &BTreeMap<SpecId, Evaluated<Spec>>,
    impl_view: &ImplView,
    tests: TestOutcome,
) -> Vec<CheckViolation> {
    let found = designs.get(&key.design);
    let mut out = upstream_violations(
        &Completeness::Complete,
        &[(key.design.entity(), found.map(Evaluated::state))],
    );
    let design = found.and_then(Evaluated::entity);
    if let Some(design) = design
        && design.items().get(&key.number) != Some(item)
    {
        out.push(CheckViolation::ItemNotInDesign);
    }
    for pin in item.assigned_pins() {
        if !impl_view.has_pin(pin) {
            out.push(CheckViolation::PinMissingInTests { pin: pin.clone() });
        }
    }
    let declared = design
        .and_then(|d| specs.get(&d.spec()))
        .and_then(Evaluated::entity);
    if let Some(spec) = declared {
        for pin in impl_view.pins_at_revision().keys() {
            if !spec.pins().contains_key(pin) {
                out.push(CheckViolation::PinUndeclaredInTests { pin: pin.clone() });
            }
        }
    }
    if tests == TestOutcome::Failed {
        out.push(CheckViolation::TestsFailed);
    }
    out
}

/// CM, layer 1: the CS approval named exists and fixed the hypotheses as they are.
pub fn check_cm(spec: &Spec, cs: &EventId, ledger: &ApprovalSet) -> Vec<CheckViolation> {
    let fixed = ledger
        .record(cs)
        .filter(|r| !ledger.is_revoked(cs) && r.subject() == ledger.canonical(spec.id().entity()))
        .and_then(|r| match r.attestation() {
            Attestation::Cs { hyp, .. } => Some(*hyp),
            _ => None,
        });
    match fixed {
        None => vec![CheckViolation::CsApprovalNotFound],
        Some(hyp) if &hyp != spec.hyp_hash() => vec![CheckViolation::HypothesisChanged {
            fixed: hyp,
            current: *spec.hyp_hash(),
        }],
        Some(_) => Vec::new(),
    }
}

/// A name that a public document must not carry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PrivateName {
    /// The alias of a private entity.
    Alias(Alias),
    /// The title of a private entity.
    Title(NonBlank),
}

/// A private name found in a public text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LeakFinding {
    name: PrivateName,
    line: NonZeroU32,
}

impl LeakFinding {
    /// The private name that appeared.
    pub fn name(&self) -> &PrivateName {
        &self.name
    }

    /// The line it starts on, from 1.
    pub fn line(&self) -> u32 {
        self.line.get()
    }
}

fn is_alias_char(c: char) -> bool {
    c.is_ascii_lowercase() || c.is_ascii_digit() || ".-_/".contains(c)
}

/// Find the private names that a public text carries.
///
/// Matching is ASCII case-insensitive. An alias counts only as a whole token, so
/// `spec/auth` is not found inside `spec/auth-v2`. A title is found anywhere,
/// across line breaks too. Text that is not ASCII is compared exactly. A
/// `private:` identifier is not a name and never matches.
pub fn leak_lint(public_text: &str, private_names: &[PrivateName]) -> Vec<LeakFinding> {
    let haystack = public_text.to_ascii_lowercase();
    let mut out = Vec::new();
    for name in private_names {
        let (needle, whole_token) = match name {
            PrivateName::Alias(alias) => (alias.as_str().to_ascii_lowercase(), true),
            PrivateName::Title(title) => (title.as_str().trim().to_ascii_lowercase(), false),
        };
        if needle.is_empty() {
            continue;
        }
        for (at, _) in haystack.match_indices(&needle) {
            if whole_token {
                let before = haystack[..at].chars().next_back();
                let mut rest = haystack[at + needle.len()..].chars();
                let after = rest.next();
                // A dot ends a sentence unless a token character follows it.
                let continues = match after {
                    Some('.') => rest.next().is_some_and(is_alias_char),
                    Some(c) => is_alias_char(c),
                    None => false,
                };
                if before.is_some_and(is_alias_char) || continues {
                    continue;
                }
            }
            let line = haystack[..at].matches('\n').count() + 1;
            if let Some(line) = u32::try_from(line).ok().and_then(NonZeroU32::new) {
                out.push(LeakFinding {
                    name: name.clone(),
                    line,
                });
            }
        }
    }
    out.sort_by_key(|f| f.line);
    out
}

/// How a generated file stands against its marker and its sources.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExportDrift {
    /// The content matches its marker and a fresh render.
    InSync,
    /// The content does not match its marker: someone edited the file.
    EditedByHand,
    /// The content matches its marker but a fresh render differs: the file is out
    /// of date (after a merge, for example).
    Outdated,
}

/// Compare a generated file with its marker and with a fresh render.
///
/// `body_without_marker` is the file as found with the marker removed;
/// `rebuilt_without_marker` is what rendering today's sources gives, also without a
/// marker. A hand edit is reported before staleness, since it needs the human first.
pub fn export_drift(
    marker: &ContentHash,
    body_without_marker: &[u8],
    rebuilt_without_marker: &[u8],
) -> ExportDrift {
    if &ContentHash::export(body_without_marker) != marker {
        ExportDrift::EditedByHand
    } else if &ContentHash::export(rebuilt_without_marker) != marker {
        ExportDrift::Outdated
    } else {
        ExportDrift::InSync
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::approval::test_support::{approve, cr, cs};
    use crate::domain::classification::fold;
    use crate::domain::design::test_support::{design, item, num};
    use crate::domain::evaluate::Evaluator;
    use crate::domain::finding::test_support::{finding, usage_source};
    use crate::domain::id::test_support::{did, eid, fid, iid, sid};
    use crate::domain::nonempty::NonEmptyVec;
    use crate::domain::spec::test_support::{hyps, spec};
    use crate::domain::state::{BlockReason, InvalidReason, StaleReason, UnresolvedReason};
    use std::collections::BTreeSet;

    fn ledger(events: &[crate::domain::event::Event]) -> ApprovalSet {
        ApprovalSet::from_events(events, &fold(events))
    }

    /// Any state that has nothing the target could not read, for the gates whose
    /// own state does not matter to the case.
    fn complete() -> EntityState {
        EntityState::new(Approval::Draft, Upstream::Ok, Completeness::Complete)
    }

    // --- CR ---------------------------------------------------------------

    #[test]
    fn cr_accepts_a_complete_finding() {
        assert_eq!(check_cr(&finding(1, "Login takes 8 s.")), vec![]);
    }

    #[test]
    fn cr_rejects_an_empty_body_and_an_unlabelled_measurement() {
        assert_eq!(
            check_cr(&finding(1, " \n")),
            vec![CheckViolation::EmptyBody]
        );
        let measured = |label| {
            Finding::new(
                fid(1),
                NonBlank::new("slow login").unwrap(),
                FindingSource::MeasuredData {
                    measured_at: crate::domain::actor::MeasuredAt::from_validated(
                        "2026-11-04T12:40:00Z".into(),
                    )
                    .unwrap(),
                    revision: crate::domain::revision::RevisionId::from_validated("a1".into())
                        .unwrap(),
                    environment: NonBlank::new("4 cores").unwrap(),
                },
                label,
                "8 s".into(),
            )
        };
        assert_eq!(check_cr(&measured(EvidenceLabel::Measured)), vec![]);
        for label in [EvidenceLabel::Theoretical, EvidenceLabel::Estimated] {
            assert_eq!(
                check_cr(&measured(label)),
                vec![CheckViolation::MeasuredDataNotLabelledMeasured]
            );
        }
        // Other types do not need the label.
        let usage = Finding::new(
            fid(1),
            NonBlank::new("t").unwrap(),
            usage_source(),
            EvidenceLabel::Estimated,
            "b".into(),
        );
        assert_eq!(check_cr(&usage), vec![]);
    }

    // --- CS ---------------------------------------------------------------

    fn findings_map(
        entries: Vec<(u128, Finding, Vec<crate::domain::event::Event>)>,
    ) -> BTreeMap<FindingId, Evaluated<Finding>> {
        entries
            .into_iter()
            .map(|(n, f, events)| (fid(n), Evaluator::finding(&f, &ledger(&events), &[])))
            .collect()
    }

    #[test]
    fn cs_requires_grounds_that_exist_and_passed_cr() {
        let f1 = finding(1, "one");
        let approved = [approve(cr(1, f1.hash()), &[])];
        let s = spec(11, &[1, 2], &["PIN-A"]);

        // Ground 1 approved; ground 2 absent.
        let map = findings_map(vec![(1, f1.clone(), approved.to_vec())]);
        assert_eq!(
            check_cs(&s, &complete(), &map),
            vec![CheckViolation::Upstream {
                upstream: eid(2),
                problem: UpstreamProblem::Missing
            }]
        );

        // Ground 2 present but a draft.
        let f2 = finding(2, "two");
        let map = findings_map(vec![
            (1, f1.clone(), approved.to_vec()),
            (2, f2, Vec::new()),
        ]);
        assert_eq!(
            check_cs(&s, &complete(), &map),
            vec![CheckViolation::Upstream {
                upstream: eid(2),
                problem: UpstreamProblem::NotApproved
            }]
        );

        // Both approved: clean.
        let f2 = finding(2, "two");
        let both = [
            approve(cr(1, f1.hash()), &[]),
            approve(cr(2, f2.hash()), &[]),
        ];
        let map = findings_map(vec![(1, f1, both.to_vec()), (2, f2, both.to_vec())]);
        assert_eq!(check_cs(&s, &complete(), &map), vec![]);
    }

    #[test]
    fn cs_requires_declared_pins_and_hypotheses() {
        let f1 = finding(1, "one");
        let approved = [approve(cr(1, f1.hash()), &[])];
        let map = findings_map(vec![(1, f1, approved.to_vec())]);
        let bare = Spec::new(
            sid(11),
            NonEmptyVec::new(fid(1), vec![]),
            NonBlank::new("c").unwrap(),
            BTreeMap::new(),
            BTreeMap::new(),
        )
        .unwrap();
        assert_eq!(
            check_cs(&bare, &complete(), &map),
            vec![
                CheckViolation::NoPinsDeclared,
                CheckViolation::NoHypsDeclared
            ]
        );
        let no_hyp = Spec::new(
            sid(11),
            NonEmptyVec::new(fid(1), vec![]),
            NonBlank::new("c").unwrap(),
            crate::domain::spec::test_support::pins(&["PIN-A"]),
            hyps(&[]),
        )
        .unwrap();
        assert_eq!(
            check_cs(&no_hyp, &complete(), &map),
            vec![CheckViolation::NoHypsDeclared]
        );
    }

    // --- CI ---------------------------------------------------------------

    fn spec_map(
        s: &Spec,
        events: &[crate::domain::event::Event],
    ) -> BTreeMap<SpecId, Evaluated<Spec>> {
        let l = ledger(events);
        let f = finding(1, "one");
        let ef = Evaluator::finding(&f, &l, &[]);
        let es = Evaluator::spec(
            s,
            &BTreeMap::from([(fid(1), ef)]),
            &l,
            &BTreeSet::new(),
            &[],
        );
        BTreeMap::from([(s.id(), es)])
    }

    #[test]
    fn ci_checks_the_spec_state_and_pin_coverage() {
        let f1 = finding(1, "one");
        let s = spec(11, &[1], &["PIN-A", "UT-B"]);
        let d = design(21, 11, vec![(1, item("a", &["PIN-A"]))]);

        // Spec is a draft and UT-B is unassigned.
        let drafts = spec_map(&s, &[]);
        assert_eq!(
            check_ci(&d, &complete(), &drafts),
            vec![
                CheckViolation::Upstream {
                    upstream: eid(11),
                    problem: UpstreamProblem::NotApproved
                },
                CheckViolation::PinCoverage(CoverageIssue::Unassigned("UT-B".parse().unwrap())),
            ]
        );

        // Spec approved; design assigns everything.
        let events = [
            approve(cr(1, f1.hash()), &[]),
            approve(
                cs(11, s.contract_hash(), s.hyp_hash(), &[(1, *f1.hash())]),
                &[],
            ),
        ];
        let ok_design = design(
            21,
            11,
            vec![(1, item("a", &["PIN-A"])), (2, item("b", &["UT-B"]))],
        );
        assert_eq!(
            check_ci(&ok_design, &complete(), &spec_map(&s, &events)),
            vec![]
        );

        // Missing spec.
        assert_eq!(
            check_ci(&ok_design, &complete(), &BTreeMap::new()),
            vec![CheckViolation::Upstream {
                upstream: eid(11),
                problem: UpstreamProblem::Missing
            }]
        );
    }

    // --- CA ---------------------------------------------------------------

    struct CaWorld {
        designs: BTreeMap<DesignId, Evaluated<Design>>,
        specs: BTreeMap<SpecId, Evaluated<Spec>>,
        design: Design,
    }

    fn ca_world(approved: bool) -> CaWorld {
        let f1 = finding(1, "one");
        let s = spec(11, &[1], &["PIN-A", "UT-B"]);
        let d = design(
            21,
            11,
            vec![(1, item("a", &["PIN-A"])), (2, item("b", &["UT-B"]))],
        );
        let mut events = vec![
            approve(cr(1, f1.hash()), &[]),
            approve(
                cs(11, s.contract_hash(), s.hyp_hash(), &[(1, *f1.hash())]),
                &[],
            ),
        ];
        if approved {
            events.push(approve(
                crate::domain::approval::test_support::ci(
                    21,
                    d.design_hash(),
                    11,
                    s.contract_hash(),
                ),
                &[],
            ));
        }
        let l = ledger(&events);
        let ef = Evaluator::finding(&f1, &l, &[]);
        let es = Evaluator::spec(
            &s,
            &BTreeMap::from([(fid(1), ef)]),
            &l,
            &BTreeSet::new(),
            &[],
        );
        let ed = Evaluator::design(
            &d,
            &BTreeMap::from([(sid(11), es.clone())]),
            &l,
            &BTreeSet::new(),
            &[],
        );
        CaWorld {
            designs: BTreeMap::from([(did(21), ed)]),
            specs: BTreeMap::from([(sid(11), es)]),
            design: d,
        }
    }

    fn view(pins: &[&str]) -> ImplView {
        ImplView::new(
            crate::domain::revision::RevisionId::from_validated("head".into()).unwrap(),
            None,
            pins.iter()
                .map(|p| (p.parse().unwrap(), vec!["tests/a.rs:3".parse().unwrap()]))
                .collect(),
            Default::default(),
        )
    }

    fn run_ca(w: &CaWorld, number: u32, pins: &[&str], tests: TestOutcome) -> Vec<CheckViolation> {
        let key = ItemKey {
            id: iid(31),
            design: did(21),
            number: num(number),
        };
        let it = w.design.items()[&num(number)].clone();
        check_ca(&key, &it, &w.designs, &w.specs, &view(pins), tests)
    }

    #[test]
    fn ca_passes_when_everything_lines_up() {
        let w = ca_world(true);
        assert_eq!(run_ca(&w, 1, &["PIN-A"], TestOutcome::Passed), vec![]);
    }

    #[test]
    fn ca_checks_both_directions_of_the_pin_match() {
        let w = ca_world(true);
        // Item 1 is assigned PIN-A; the tests have PIN-Z (undeclared) and not PIN-A.
        assert_eq!(
            run_ca(&w, 1, &["PIN-Z"], TestOutcome::Passed),
            vec![
                CheckViolation::PinMissingInTests {
                    pin: "PIN-A".parse().unwrap()
                },
                CheckViolation::PinUndeclaredInTests {
                    pin: "PIN-Z".parse().unwrap()
                },
            ]
        );
        // A tag of a sibling item in the tests is declared, so it is fine.
        assert_eq!(
            run_ca(&w, 1, &["PIN-A", "UT-B"], TestOutcome::Passed),
            vec![]
        );
    }

    #[test]
    fn ca_requires_passing_tests_and_a_ready_design() {
        let w = ca_world(true);
        assert_eq!(
            run_ca(&w, 1, &["PIN-A"], TestOutcome::Failed),
            vec![CheckViolation::TestsFailed]
        );
        let draft = ca_world(false);
        assert_eq!(
            run_ca(&draft, 1, &["PIN-A"], TestOutcome::Passed),
            vec![CheckViolation::Upstream {
                upstream: eid(21),
                problem: UpstreamProblem::NotApproved
            }]
        );
        let key = ItemKey {
            id: iid(31),
            design: did(21),
            number: num(1),
        };
        let it = item("a", &["PIN-A"]);
        assert_eq!(
            check_ca(
                &key,
                &it,
                &BTreeMap::new(),
                &BTreeMap::new(),
                &view(&["PIN-A"]),
                TestOutcome::Passed
            ),
            vec![CheckViolation::Upstream {
                upstream: eid(21),
                problem: UpstreamProblem::Missing
            }]
        );
    }

    #[test]
    fn ca_rejects_an_item_that_is_not_the_one_in_the_design() {
        let w = ca_world(true);
        let key = ItemKey {
            id: iid(31),
            design: did(21),
            number: num(1),
        };
        let other = item("a, but reworded", &["PIN-A"]);
        assert_eq!(
            check_ca(
                &key,
                &other,
                &w.designs,
                &w.specs,
                &view(&["PIN-A"]),
                TestOutcome::Passed
            ),
            vec![CheckViolation::ItemNotInDesign]
        );
    }

    // --- upstreams that cannot be read here ---------------------------------

    fn partial(root: u128, through: u128) -> Completeness {
        Completeness::from_reasons(BTreeSet::from([UnresolvedReason::new(
            eid(root),
            eid(through),
        )]))
    }

    fn stale_upstream() -> Upstream {
        Upstream::Stale(NonEmptyVec::new(
            StaleReason::UpstreamStale { upstream: eid(9) },
            vec![],
        ))
    }

    #[test]
    fn upstream_problems_come_in_one_order_and_one_per_upstream() {
        let invalid = EntityState::new(
            Approval::Invalid(NonEmptyVec::new(
                InvalidReason::DanglingRef { upstream: eid(9) },
                vec![],
            )),
            Upstream::Ok,
            Completeness::Complete,
        );
        let unresolved = EntityState::new(Approval::Approved, Upstream::Ok, partial(5, 5));
        let draft = complete();
        let stale = EntityState::new(Approval::Approved, stale_upstream(), Completeness::Complete);
        let blocked = EntityState::new(
            Approval::Approved,
            Upstream::Blocked(NonEmptyVec::new(
                BlockReason::UpstreamUnapproved { upstream: eid(9) },
                vec![],
            )),
            Completeness::Complete,
        );
        let ready = EntityState::new(Approval::Approved, Upstream::Ok, Completeness::Complete);
        // Given back to front, and with one that has no problem among them.
        let given = [
            (eid(7), Some(&ready)),
            (eid(6), Some(&blocked)),
            (eid(5), Some(&stale)),
            (eid(4), Some(&draft)),
            (eid(3), Some(&unresolved)),
            (eid(2), Some(&invalid)),
            (eid(1), None),
        ];
        assert_eq!(
            upstream_problems(&Completeness::Complete, &given),
            vec![
                (eid(1), UpstreamProblem::Missing),
                (eid(2), UpstreamProblem::Invalid),
                (eid(3), UpstreamProblem::Unresolved),
                (eid(4), UpstreamProblem::NotApproved),
                (eid(5), UpstreamProblem::Stale),
                (eid(6), UpstreamProblem::Blocked),
            ]
        );
    }

    #[test]
    fn an_approved_stale_and_partial_upstream_is_unresolved() {
        // Not Stale: a clone cannot approve again, whatever changed.
        let state = EntityState::new(Approval::Approved, stale_upstream(), partial(5, 5));
        assert_eq!(
            upstream_problems(&Completeness::Complete, &[(eid(1), Some(&state))]),
            vec![(eid(1), UpstreamProblem::Unresolved)]
        );
    }

    #[test]
    fn a_partial_upstream_and_an_unrelated_draft_upstream_are_both_reported() {
        let partial_state = EntityState::new(Approval::Approved, Upstream::Ok, partial(5, 1));
        let draft = complete();
        // The Draft is given first: the order of the problems decides, not the list.
        assert_eq!(
            upstream_problems(
                &Completeness::Complete,
                &[(eid(2), Some(&draft)), (eid(1), Some(&partial_state))]
            ),
            vec![
                (eid(1), UpstreamProblem::Unresolved),
                (eid(2), UpstreamProblem::NotApproved),
            ]
        );
    }

    #[test]
    fn an_upstream_that_is_not_there_is_unresolved_only_when_the_target_names_it() {
        let named = partial(1, 1);
        assert_eq!(
            upstream_problems(&named, &[(eid(1), None), (eid(2), None)]),
            vec![
                (eid(2), UpstreamProblem::Missing),
                (eid(1), UpstreamProblem::Unresolved),
            ]
        );
    }

    #[test]
    fn cs_stops_on_a_ground_it_cannot_read_and_pins_nothing() {
        let s = spec(11, &[1], &["PIN-A"]);
        let unreadable = Evaluator::spec(
            &s,
            &BTreeMap::new(),
            &ledger(&[]),
            &BTreeSet::from([eid(1)]),
            &[],
        );
        assert_eq!(
            check_cs(&s, unreadable.state(), &BTreeMap::new()),
            vec![CheckViolation::Upstream {
                upstream: eid(1),
                problem: UpstreamProblem::Unresolved
            }]
        );
        // Without the mark the same missing ground is only missing.
        let dangling = Evaluator::spec(&s, &BTreeMap::new(), &ledger(&[]), &BTreeSet::new(), &[]);
        assert_eq!(
            check_cs(&s, dangling.state(), &BTreeMap::new()),
            vec![CheckViolation::Upstream {
                upstream: eid(1),
                problem: UpstreamProblem::Missing
            }]
        );
    }

    /// A Spec that was approved where the private store is, looked at where it is not.
    fn approved_partial_spec() -> (Spec, Evaluated<Spec>, Vec<crate::domain::event::Event>) {
        let f1 = finding(1, "one");
        let s = spec(11, &[1], &["PIN-A", "UT-B"]);
        let events = vec![
            approve(cr(1, f1.hash()), &[]),
            approve(
                cs(11, s.contract_hash(), s.hyp_hash(), &[(1, *f1.hash())]),
                &[],
            ),
        ];
        let es = Evaluator::spec(
            &s,
            &BTreeMap::new(),
            &ledger(&events),
            &BTreeSet::from([eid(1)]),
            &[],
        );
        (s, es, events)
    }

    #[test]
    fn ci_stops_on_an_approved_spec_that_is_partial() {
        let (s, es, _) = approved_partial_spec();
        assert_eq!(es.state().approval(), &Approval::Approved);
        let d = design(
            21,
            11,
            vec![(1, item("a", &["PIN-A"])), (2, item("b", &["UT-B"]))],
        );
        assert_eq!(
            check_ci(&d, &complete(), &BTreeMap::from([(s.id(), es)])),
            vec![CheckViolation::Upstream {
                upstream: eid(11),
                problem: UpstreamProblem::Unresolved
            }]
        );
    }

    #[test]
    fn ca_stops_on_an_approved_design_that_is_partial_further_up() {
        let (s, es, mut events) = approved_partial_spec();
        let d = design(
            21,
            11,
            vec![(1, item("a", &["PIN-A"])), (2, item("b", &["UT-B"]))],
        );
        events.push(approve(
            crate::domain::approval::test_support::ci(21, d.design_hash(), 11, s.contract_hash()),
            &[],
        ));
        let ed = Evaluator::design(
            &d,
            &BTreeMap::from([(s.id(), es.clone())]),
            &ledger(&events),
            &BTreeSet::new(),
            &[],
        );
        assert_eq!(ed.state().approval(), &Approval::Approved);
        let key = ItemKey {
            id: iid(31),
            design: did(21),
            number: num(1),
        };
        assert_eq!(
            check_ca(
                &key,
                &d.items()[&num(1)],
                &BTreeMap::from([(did(21), ed)]),
                &BTreeMap::from([(s.id(), es)]),
                &view(&["PIN-A"]),
                TestOutcome::Passed
            ),
            vec![CheckViolation::Upstream {
                upstream: eid(21),
                problem: UpstreamProblem::Unresolved
            }]
        );
    }

    // --- CM ---------------------------------------------------------------

    #[test]
    fn cm_needs_the_cs_approval_that_fixed_the_current_hypotheses() {
        let f1 = finding(1, "one");
        let s = spec(11, &[1], &["PIN-A"]);
        let a_cs = approve(
            cs(11, s.contract_hash(), s.hyp_hash(), &[(1, *f1.hash())]),
            &[],
        );
        let events = vec![a_cs.clone()];
        let l = ledger(&events);
        assert_eq!(check_cm(&s, &a_cs.id(), &l), vec![]);

        // A CS event that does not exist.
        let ghost = approve(cr(1, f1.hash()), &[]).id();
        assert_eq!(
            check_cm(&s, &ghost, &l),
            vec![CheckViolation::CsApprovalNotFound]
        );

        // The event named is not a CS approval.
        let a_cr = approve(cr(1, f1.hash()), &[]);
        let l2 = ledger(&[a_cs.clone(), a_cr.clone()]);
        assert_eq!(
            check_cm(&s, &a_cr.id(), &l2),
            vec![CheckViolation::CsApprovalNotFound]
        );

        // Withdrawn.
        let rev = crate::domain::approval::test_support::revoke(
            sid(11).entity(),
            a_cs.id(),
            &[a_cs.id()],
        );
        let l3 = ledger(&[a_cs.clone(), rev]);
        assert_eq!(
            check_cm(&s, &a_cs.id(), &l3),
            vec![CheckViolation::CsApprovalNotFound]
        );

        // The hypotheses changed after the approval.
        let changed = Spec::new(
            sid(11),
            NonEmptyVec::new(fid(1), vec![]),
            NonBlank::new("c").unwrap(),
            crate::domain::spec::test_support::pins(&["PIN-A"]),
            BTreeMap::from([(
                "HYP-OTHER".parse().unwrap(),
                crate::domain::pin::Hypothesis::new("m", "1").unwrap(),
            )]),
        )
        .unwrap();
        assert_eq!(
            check_cm(&changed, &a_cs.id(), &l),
            vec![CheckViolation::HypothesisChanged {
                fixed: *s.hyp_hash(),
                current: *changed.hyp_hash()
            }]
        );

        // An approval of another spec does not count.
        let other = spec(12, &[1], &["PIN-A"]);
        assert_eq!(
            check_cm(&other, &a_cs.id(), &l),
            vec![CheckViolation::CsApprovalNotFound]
        );
    }

    // --- leak lint ----------------------------------------------------------

    fn alias(s: &str) -> PrivateName {
        PrivateName::Alias(s.parse().unwrap())
    }

    fn title(s: &str) -> PrivateName {
        PrivateName::Title(NonBlank::new(s).unwrap())
    }

    #[test]
    fn leak_lint_finds_an_alias_with_its_line() {
        let text = "intro\nsee spec/secret-plan for details\n";
        let found = leak_lint(text, &[alias("spec/secret-plan")]);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].line(), 2);
        assert_eq!(found[0].name(), &alias("spec/secret-plan"));
    }

    #[test]
    fn leak_lint_matches_whole_alias_tokens_only() {
        assert!(leak_lint("spec/auth-v2 is public", &[alias("spec/auth")]).is_empty());
        assert!(leak_lint("myspec/auth", &[alias("spec/auth")]).is_empty());
        assert_eq!(leak_lint("(spec/auth)", &[alias("spec/auth")]).len(), 1);
        assert_eq!(
            leak_lint("see spec/auth.", &[alias("spec/auth")]).len(),
            1,
            "a final dot ends a sentence"
        );
        assert!(
            leak_lint("spec/auth.v2", &[alias("spec/auth")]).is_empty(),
            "a dot between token characters extends it"
        );
        assert_eq!(leak_lint("spec/auth, next", &[alias("spec/auth")]).len(), 1);
    }

    #[test]
    fn leak_lint_finds_titles_case_insensitively_and_across_lines() {
        let text = "We follow the Quarterly Pricing Experiment\nresults.\n";
        assert_eq!(
            leak_lint(text, &[title("quarterly pricing experiment")]).len(),
            1
        );
        let split = "Quarterly Pricing\nExperiment";
        assert_eq!(
            leak_lint(split, &[title("Quarterly Pricing\nExperiment")]).len(),
            1
        );
    }

    #[test]
    fn leak_lint_ignores_opaque_private_references_and_clean_text() {
        let id = eid(5);
        let text = format!("depends on private:{id}\n");
        assert!(leak_lint(&text, &[alias("spec/secret"), title("Secret roadmap")]).is_empty());
        assert!(leak_lint("", &[alias("spec/secret")]).is_empty());
        assert!(leak_lint("anything", &[]).is_empty());
    }

    #[test]
    fn leak_lint_reports_each_occurrence_in_line_order() {
        let text = "a spec/x\nb spec/y\nc spec/x\n";
        let found = leak_lint(text, &[alias("spec/x"), alias("spec/y")]);
        assert_eq!(
            found.iter().map(LeakFinding::line).collect::<Vec<_>>(),
            vec![1, 2, 3]
        );
    }

    // --- export drift ------------------------------------------------------

    #[test]
    fn export_drift_tells_hand_edits_from_staleness() {
        let body = b"{\n  \"entities\": {}\n}\n";
        let marker = ContentHash::export(body);
        assert_eq!(export_drift(&marker, body, body), ExportDrift::InSync);
        // The file was edited: its content no longer hashes to the marker.
        assert_eq!(
            export_drift(&marker, b"{ \"edited\": true }\n", body),
            ExportDrift::EditedByHand
        );
        // The file is intact but the sources moved on.
        assert_eq!(
            export_drift(&marker, body, b"{\n  \"entities\": {\"x\": 1}\n}\n"),
            ExportDrift::Outdated
        );
        // Both: the hand edit is reported first.
        assert_eq!(
            export_drift(&marker, b"edited", b"also different"),
            ExportDrift::EditedByHand
        );
    }

    // Golden vector: the content of a generated file without its marker, and its hash.
    #[test]
    fn golden_export_marker() {
        let body = b"{\n  \"entities\": {},\n  \"version\": 1\n}\n";
        assert_eq!(
            ContentHash::export(body).to_string(),
            "sha256:cdbfb1fb5ac0c505d3ec2595dcf5fed3948c6a1cf9f0553977bb976378ce2f53"
        );
    }
}
