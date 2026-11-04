//! Machine checks: what can be decided without judgement.
//!
//! Three families, all pure functions of values:
//!
//! - **CHECK, layer 1.** One function per gate ([`check_cr`], [`check_cs`],
//!   [`check_ci`], [`check_ca`], [`check_cm`]) returns every violation found, not
//!   the first. An approval (layer 2) may be recorded only when the list is empty.
//!   The machine looks at form: links resolve, upstream is approved and current,
//!   pins match in both directions, tests pass. Whether a claim follows from its
//!   evidence is for the human who approves.
//! - **Leak lint.** [`leak_lint`] stops a public document from carrying the alias
//!   or the title of a private entity. A public document may point at a private
//!   entity only by its opaque `private:` identifier.
//! - **Drift of generated files.** [`export_drift`] tells whether a generated file
//!   was edited by hand or has fallen behind its sources.
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
use super::state::{Approval, EntityState, Upstream};

/// What is wrong with an upstream entity a gate depends on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum UpstreamProblem {
    /// The entity is not there (a dangling reference).
    Missing,
    /// The entity is invalid or did not parse.
    Invalid,
    /// The entity has no approval for its current content.
    NotApproved,
    /// The entity is approved but stale.
    Stale,
    /// The entity is approved but blocked.
    Blocked,
}

/// One reason a gate's machine check fails.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CheckViolation {
    /// CR: the Finding has no body.
    EmptyBody,
    /// CR: a measured-data Finding is not labelled as measured.
    MeasuredDataNotLabelledMeasured,
    /// An upstream entity is not ready to be depended on.
    Upstream {
        /// The upstream entity.
        upstream: EntityId,
        /// What is wrong with it.
        problem: UpstreamProblem,
    },
    /// CS: the Spec declares no PIN or unit-test tag.
    NoPinsDeclared,
    /// CS: the Spec declares no hypothesis.
    NoHypsDeclared,
    /// CI: the Design and the Spec disagree about a pin.
    PinCoverage(CoverageIssue),
    /// CA: the item's pin is not carried by any test at the revision.
    PinMissingInTests {
        /// The pin.
        pin: PinId,
    },
    /// CA: a test carries a tag the Spec never declared.
    PinUndeclaredInTests {
        /// The tag.
        pin: PinId,
    },
    /// CA: the tests did not pass.
    TestsFailed,
    /// CA: the item is not (or no longer) the one the Design has at that number.
    ItemNotInDesign,
    /// CM: the CS approval that should have fixed the hypotheses is not there.
    CsApprovalNotFound,
    /// CM: the hypotheses were changed after the CS approval fixed them.
    HypothesisChanged {
        /// The hash the CS approval fixed.
        fixed: ContentHash,
        /// The hash today.
        current: ContentHash,
    },
}

fn upstream_problem(state: &EntityState) -> Option<UpstreamProblem> {
    match (state.approval(), state.upstream()) {
        (Approval::Invalid(_), _) => Some(UpstreamProblem::Invalid),
        (Approval::Draft | Approval::Modified { .. }, _) => Some(UpstreamProblem::NotApproved),
        (Approval::Approved, Upstream::Stale(_)) => Some(UpstreamProblem::Stale),
        (Approval::Approved, Upstream::Blocked(_)) => Some(UpstreamProblem::Blocked),
        (Approval::Approved, Upstream::Ok) => None,
    }
}

fn upstream_violation<T>(id: EntityId, found: Option<&Evaluated<T>>) -> Option<CheckViolation> {
    let problem = match found {
        None => Some(UpstreamProblem::Missing),
        Some(ev) => upstream_problem(ev.state()),
    };
    problem.map(|problem| CheckViolation::Upstream {
        upstream: id,
        problem,
    })
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

/// CS, layer 1: every ground exists, passed CR and is current; pins and
/// hypotheses are declared.
pub fn check_cs(
    spec: &Spec,
    findings: &BTreeMap<FindingId, Evaluated<Finding>>,
) -> Vec<CheckViolation> {
    let mut out: Vec<CheckViolation> = spec
        .grounds()
        .iter()
        .filter_map(|g| upstream_violation(g.entity(), findings.get(g)))
        .collect();
    if spec.pins().is_empty() {
        out.push(CheckViolation::NoPinsDeclared);
    }
    if spec.hyps().is_empty() {
        out.push(CheckViolation::NoHypsDeclared);
    }
    out
}

/// CI, layer 1: the Spec passed CS and is current; pins are covered in both
/// directions.
///
/// That the Crux gives a reason for each rejected option is held by its type, so
/// there is nothing to check for it here.
pub fn check_ci(design: &Design, specs: &BTreeMap<SpecId, Evaluated<Spec>>) -> Vec<CheckViolation> {
    let found = specs.get(&design.spec());
    let mut out: Vec<CheckViolation> = upstream_violation(design.spec().entity(), found)
        .into_iter()
        .collect();
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

/// CA, layer 1: the Design passed CI and is current; the item's pins and the
/// tests' tags match in both directions at the revision; the tests passed.
pub fn check_ca(
    key: &ItemKey,
    item: &WorkItem,
    designs: &BTreeMap<DesignId, Evaluated<Design>>,
    specs: &BTreeMap<SpecId, Evaluated<Spec>>,
    impl_view: &ImplView,
    tests: TestOutcome,
) -> Vec<CheckViolation> {
    let found = designs.get(&key.design);
    let mut out: Vec<CheckViolation> = upstream_violation(key.design.entity(), found)
        .into_iter()
        .collect();
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

    fn ledger(events: &[crate::domain::event::Event]) -> ApprovalSet {
        ApprovalSet::from_events(events, &fold(events))
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
            check_cs(&s, &map),
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
            check_cs(&s, &map),
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
        assert_eq!(check_cs(&s, &map), vec![]);
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
            check_cs(&bare, &map),
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
            check_cs(&no_hyp, &map),
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
        let es = Evaluator::spec(s, &BTreeMap::from([(fid(1), ef)]), &l, &[]);
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
            check_ci(&d, &drafts),
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
        assert_eq!(check_ci(&ok_design, &spec_map(&s, &events)), vec![]);

        // Missing spec.
        assert_eq!(
            check_ci(&ok_design, &BTreeMap::new()),
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
        let es = Evaluator::spec(&s, &BTreeMap::from([(fid(1), ef)]), &l, &[]);
        let ed = Evaluator::design(&d, &BTreeMap::from([(sid(11), es.clone())]), &l, &[]);
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
