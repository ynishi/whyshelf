//! Machine checks: what can be decided without judgement.
//!
//! Three families, all pure functions of values:
//!
//! - **CHECK, layer 1.** One function per gate (in [`gate`](super::gate)) returns
//!   every violation found, not the first, or the proof that none was
//!   ([`Passed`](super::gate::Passed)). An approval (layer 2) may be recorded only
//!   from that proof. The machine looks at form: links resolve and can be read
//!   here, upstream is approved and current, pins match in both directions, tests
//!   pass. Whether a claim follows from its evidence is for the human who approves.
//!   This module holds the violations those checks name, and the order in which an
//!   entity's upstream problems are shown ([`upstream_problems`]).
//! - **Leak lint.** [`leak_lint`] stops a public document from carrying the alias
//!   or the title of a private entity, or the identifier of one without the mark.
//!   A public document may point at a private entity only by its opaque
//!   `private:` identifier.
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
//! | `TargetInvalid` | all | the target is invalid: *why* | the entity the gate is run on is Invalid, so there is nothing to approve; *why* are its [`InvalidReason`]s | fix the file (`whyshelf status` shows the reasons); nothing else is checked until then |
//! | `EmptyBody` | CR | the Finding has no body | nothing is written under the front matter | write what was seen and how it is known |
//! | `MeasuredDataNotLabelledMeasured` | CR | a measured_data Finding must be labelled `evidence: measured` | a measurement carries a weaker label | set `evidence: measured`, or change `finding_type` if it was not measured |
//! | `Upstream` | CS, CI, CA | upstream *x* is not ready: *why* | an entity this one stands on cannot be depended on yet; *why* is an [`UpstreamProblem`] (next table) | clear the problem upstream first, nearest to the Finding first |
//! | `NoPinsDeclared` | CS | the Spec declares no pin | `accept.pins` is empty, and the [`GatePolicy`](super::policy::GatePolicy) requires pins (it does by default) | declare at least one `PIN-*` or `UT-*` with its contract sentence |
//! | `NoHypsDeclared` | CS | the Spec declares no hypothesis | `accept.hyp` is empty, and the policy requires hypotheses (it does by default) | declare at least one `HYP-*` with a metric and a target |
//! | `PinCoverage` | CI | pin coverage: *issue* | the Design and the Spec disagree about a pin; *issue* is a [`CoverageIssue`] | assign the pin to an item, or declare it in the Spec |
//! | `PinMissingInTests` | CA | the item declares *pin* but no test carries it | no `@pin: ...` tag for it in the scanned directories, at the revision | add the tag to the test that holds the contract, commit, check again |
//! | `PinUndeclaredInTests` | CA | a test carries *pin* but no Spec of the project declares it | a tag in a test file (`test.files`, inside the scan directories) at the revision that no Spec lists ([`DeclaredPins`](super::spec::DeclaredPins)); it counts wherever it is, not only on this item's tests, and whichever Spec declares it | declare it in a Spec, or take the tag off the test |
//! | `MalformedPin` | CA | *file:line*: `@pin: text` is not a pin name | a pin tag whose name is not a prefix of the pin vocabulary (by default `PIN-` or `UT-`) followed by capitals, digits, `_` and `-` (`PIN-foo`) | correct the name, or take the tag off |
//! | `TestsFailed` | CA | the tests did not pass | `test.command` exited with a non-zero status at the revision | make the tests pass at that revision |
//! | `TestsTimedOut` | CA | the tests did not finish within *time* | `test.command` was still running when `test.timeout` ended and was stopped; no result exists, so nothing is recorded | make the tests faster, or raise `test.timeout` |
//! | `ItemNotInDesign` | CA | the Design no longer has this item | the Design's item at that number is not the item that was handed to the check (a guard: the application takes the item from the same Design, so it is not expected) | run the command again |
//! | `MeasurementNotMeasuredData` | CM | *finding* is not a measured_data Finding | a `fail-hyp` names a Finding that does not record a measurement (or that is not there) | name the `measured_data` Finding that records it |
//! | `CsApprovalNotFound` | CM | no CS approval fixed the hypotheses of this Spec | there is no unrevoked CS approval to compare with | approve the Spec (CS) first |
//! | `HypothesisChanged` | CM | the hypotheses changed after CS fixed them | `accept.hyp` is not what the CS approval fixed | put the hypotheses back, or approve CS again and measure against the new ones |
//!
//! What can be wrong with an upstream ([`UpstreamProblem`]), in the order in which
//! [`upstream_problems`] puts them (the first is the one to look at first):
//!
//! | problem | the command line says | what it means | how to clear it |
//! |---|---|---|---|
//! | `Invalid` | it is invalid | the upstream file does not parse, or breaks a rule, or is a document whose kind or path is in conflict | fix the upstream file (`whyshelf status` shows the reasons) |
//! | `Unresolved` | it cannot be read here | the upstream, or something above it, is in a store this environment cannot read (a `private:` reference in a clone), so it cannot be checked or pinned | open the project where the private store is, and check there |
//! | `NotApproved` | it is not approved | the upstream is a Draft, or was edited after its approval (Modified) | run the upstream's own gate and approve it |
//! | `Stale` | it is stale | the upstream is approved, but something above it changed | run the gates again from the top: the first stale one, then the next |
//! | `Blocked` | it is blocked | the upstream is approved, but something above it is not ready | follow the NEXT column of `whyshelf status` from the top |
//!
//! One problem is reported per upstream, the first that applies: an upstream that is
//! Approved, Stale and Partial is `Unresolved`, because staleness cannot be cleared
//! where the entity cannot be read. The upstream is still Stale, and so is the
//! entity that stands on it; this order only says which to show first.
//!
//! An upstream that no entity of the model is, and that the target did not write
//! with a mark into a store it cannot read, is a dangling reference, which makes
//! the *target* Invalid: that is its own violation, found before any upstream is
//! looked at, so there is no problem for it here.
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

use std::num::NonZeroU32;
use std::time::Duration;

use super::design::CoverageIssue;
use super::hash::ContentHash;
use super::id::{Alias, EntityId, FindingId};
use super::location::CodeLocation;
use super::nonempty::{NonBlank, NonEmptyVec};
use super::pin::PinId;
use super::reference::Ref;
use super::state::{
    ApprovalProblem, Completeness, EntityState, HealthProblem, InvalidReason, UpstreamFacts,
};

/// What is wrong with an upstream entity a gate depends on.
///
/// The variants are in the order of [`upstream_problems`]: the first that applies
/// to an upstream wins, and the problems of several upstreams are listed by it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum UpstreamProblem {
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
    /// Any gate: the entity the gate is run on is Invalid, so there is nothing to
    /// approve and nothing else is checked.
    ///
    /// Fix: repair the file; the reasons say what is wrong with it.
    TargetInvalid {
        /// Why the entity is Invalid.
        reasons: NonEmptyVec<InvalidReason>,
    },
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
    /// CS: the Spec declares no PIN or unit-test tag, and the policy requires one.
    ///
    /// Fix: declare at least one under `accept.pins`.
    NoPinsDeclared,
    /// CS: the Spec declares no hypothesis, and the policy requires one.
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
    /// CA: a tag in a test file is shaped like a pin tag but is not a pin name.
    ///
    /// Fix: correct the tag (a prefix of the pin vocabulary, `PIN-` or `UT-` by
    /// default, then capitals, digits, `_` and `-`), or
    /// take it off the test.
    MalformedPin {
        /// Where the tag was found.
        location: CodeLocation,
        /// The text the tag named.
        text: NonBlank,
    },
    /// CA: the tests did not pass.
    ///
    /// Fix: make `test.command` pass at the revision.
    TestsFailed,
    /// CA: the tests did not finish within `test.timeout`, so there is no result.
    ///
    /// Fix: make the tests faster, or raise `test.timeout`; then check again.
    TestsTimedOut {
        /// How long the command was given.
        after: Duration,
    },
    /// CA: the item is not (or no longer) the one the Design has at that number.
    ///
    /// Fix: run the command again.
    ItemNotInDesign,
    /// CM: a `fail-hyp` names a Finding that does not record a measurement.
    ///
    /// Fix: name the `measured_data` Finding that records the measurement.
    MeasurementNotMeasuredData {
        /// The Finding named.
        finding: FindingId,
    },
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

/// The one problem of an upstream that is there, the first that applies: the order
/// is the whole rule, over the facts the upstream states about itself.
fn problem_of(facts: UpstreamFacts) -> Option<UpstreamProblem> {
    match (facts.approval(), facts.is_partial(), facts.health()) {
        (Some(ApprovalProblem::Invalid), _, _) => Some(UpstreamProblem::Invalid),
        (_, true, _) => Some(UpstreamProblem::Unresolved),
        (Some(ApprovalProblem::NotApproved), false, _) => Some(UpstreamProblem::NotApproved),
        (None, false, Some(HealthProblem::Stale)) => Some(UpstreamProblem::Stale),
        (None, false, Some(HealthProblem::Blocked)) => Some(UpstreamProblem::Blocked),
        (None, false, None) => None,
    }
}

/// What stands in the way of depending on each upstream of an entity.
///
/// `target` is the completeness of the entity that depends on them; each upstream
/// is its identifier and its state, or `None` when no entity of that identifier has
/// one. An upstream with no state is `Unresolved` when the target wrote it with a
/// mark into a store this environment cannot read (it names the upstream in its
/// completeness); otherwise it is a dangling reference, which makes the target
/// Invalid and is reported as that, so there is no problem for it here.
///
/// The state of an upstream is read as [`UpstreamFacts`] (the target is taken as
/// not yet approved, so a Modified upstream counts as not approved), and one problem
/// is returned per upstream, the first that applies in the order of
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
                None => None,
                Some(state) => problem_of(UpstreamFacts::of(state, false, false)),
            };
            problem.map(|problem| (*id, problem))
        })
        .collect();
    out.sort_by_key(|(_, problem)| *problem);
    out
}

/// A name that a public document must not carry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PrivateName {
    /// The alias of a private entity.
    Alias(Alias),
    /// The title of a private entity.
    Title(NonBlank),
}

/// A private name, or an identifier without its mark, found in a public text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LeakFinding {
    /// The alias or the title of a private entity.
    Name {
        /// The private name that appeared.
        name: PrivateName,
        /// The line it starts on.
        line: NonZeroU32,
    },
    /// The identifier of a private entity written without the `private:` mark.
    ///
    /// A document of the repo store reaches the private store only by the opaque
    /// `private:<id>`; written plain, the identifier names nothing there (a clone
    /// cannot see it) and gives away that the entity exists.
    UnmarkedPrivateRef {
        /// The reference as written.
        reference: Ref,
        /// The line it starts on.
        line: NonZeroU32,
    },
}

impl LeakFinding {
    /// The private name that appeared; none for an identifier without its mark.
    pub fn name(&self) -> Option<&PrivateName> {
        match self {
            Self::Name { name, .. } => Some(name),
            Self::UnmarkedPrivateRef { .. } => None,
        }
    }

    /// The reference that should have been marked; none for a name.
    pub fn unmarked_reference(&self) -> Option<&Ref> {
        match self {
            Self::UnmarkedPrivateRef { reference, .. } => Some(reference),
            Self::Name { .. } => None,
        }
    }

    /// The line it starts on, from 1.
    pub fn line(&self) -> u32 {
        match self {
            Self::Name { line, .. } | Self::UnmarkedPrivateRef { line, .. } => line.get(),
        }
    }
}

fn is_alias_char(c: char) -> bool {
    c.is_ascii_lowercase() || c.is_ascii_digit() || ".-_/".contains(c)
}

fn line_of(haystack: &str, at: usize) -> Option<NonZeroU32> {
    let line = haystack[..at].matches('\n').count() + 1;
    u32::try_from(line).ok().and_then(NonZeroU32::new)
}

/// Find what a public text must not carry of the private entities.
///
/// `private_names` are the aliases and titles of the private entities, and
/// `private_ids` their identifiers.
///
/// Matching is ASCII case-insensitive. An alias counts only as a whole token, so
/// `spec/auth` is not found inside `spec/auth-v2`. A title is found across line
/// breaks too, and where it begins (ends) with an ASCII letter or digit it is a word
/// there: it is not found when the character just before (after) it is an ASCII
/// letter or digit, so the title `Cache` does not block `cached`. A title that begins
/// (ends) with anything else, as a script written without word spacing does, is found
/// by plain substring at that end, and so is a title that stands next to such text
/// (`Cacheの設計`). Text that is not ASCII is compared exactly. An
/// identifier counts when it is not written `private:<id>`: that is the only way a
/// public text may name a private entity, and the one mark is
/// [`StoreKind::mark_for`](super::event::StoreKind::mark_for). The text is the whole
/// file, front matter included, so a reference in a key and one in the body are
/// seen alike.
pub fn leak_lint(
    public_text: &str,
    private_names: &[PrivateName],
    private_ids: &[EntityId],
) -> Vec<LeakFinding> {
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
        // A title that starts (ends) in an ASCII letter or digit is a word at that end:
        // the character beside it must not continue the word. One that starts (ends)
        // in anything else, a script written without word spacing for one, is found
        // by plain substring at that end.
        let word_start = !whole_token && needle.starts_with(|c: char| c.is_ascii_alphanumeric());
        let word_end = !whole_token && needle.ends_with(|c: char| c.is_ascii_alphanumeric());
        for (at, _) in haystack.match_indices(&needle) {
            if word_start && haystack[..at].ends_with(|c: char| c.is_ascii_alphanumeric()) {
                continue;
            }
            if word_end
                && haystack[at + needle.len()..].starts_with(|c: char| c.is_ascii_alphanumeric())
            {
                continue;
            }
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
            if let Some(line) = line_of(&haystack, at) {
                out.push(LeakFinding::Name {
                    name: name.clone(),
                    line,
                });
            }
        }
    }
    for id in private_ids {
        let needle = id.to_string();
        let marked = Ref::Private(*id).to_string();
        for (at, _) in haystack.match_indices(&needle) {
            if haystack[..at + needle.len()].ends_with(&marked) {
                continue;
            }
            if let Some(line) = line_of(&haystack, at) {
                out.push(LeakFinding::UnmarkedPrivateRef {
                    reference: Ref::Local(super::reference::LocalRef::Id(*id)),
                    line,
                });
            }
        }
    }
    out.sort_by_key(LeakFinding::line);
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
    use crate::domain::id::test_support::eid;
    use crate::domain::state::{Approval, BlockReason, StaleReason, UnresolvedReason, Upstream};
    use std::collections::BTreeSet;

    /// A Draft with nothing it could not read.
    fn complete() -> EntityState {
        EntityState::new(Approval::Draft, Upstream::Ok, Completeness::Complete)
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
        ];
        assert_eq!(
            upstream_problems(&Completeness::Complete, &given),
            vec![
                (eid(2), UpstreamProblem::Invalid),
                (eid(3), UpstreamProblem::Unresolved),
                (eid(4), UpstreamProblem::NotApproved),
                (eid(5), UpstreamProblem::Stale),
                (eid(6), UpstreamProblem::Blocked),
            ]
        );
    }

    /// The order table: a state that holds several facts at once, and the problem
    /// shown first for it. "Approved, Stale and Partial" is Stale and Partial for the
    /// entity above it (the evaluator keeps both), and `Unresolved` here.
    #[test]
    fn the_order_of_problems_is_decided_over_the_facts_that_hold_together() {
        let invalid = Approval::Invalid(NonEmptyVec::new(
            InvalidReason::DanglingRef { upstream: eid(9) },
            vec![],
        ));
        let blocked = Upstream::Blocked(NonEmptyVec::new(
            BlockReason::UpstreamUnapproved { upstream: eid(9) },
            vec![],
        ));
        let modified = Approval::Modified {
            approved: ContentHash::raw(b"old"),
        };
        let rows = [
            // (approval, upstream axis, completeness, the first problem)
            (
                Approval::Approved,
                stale_upstream(),
                partial(5, 5),
                Some(UpstreamProblem::Unresolved),
            ),
            (
                Approval::Approved,
                Upstream::Ok,
                partial(5, 5),
                Some(UpstreamProblem::Unresolved),
            ),
            (
                Approval::Draft,
                blocked.clone(),
                partial(5, 5),
                Some(UpstreamProblem::Unresolved),
            ),
            (
                invalid.clone(),
                blocked.clone(),
                partial(5, 5),
                Some(UpstreamProblem::Invalid),
            ),
            (
                Approval::Draft,
                blocked.clone(),
                Completeness::Complete,
                Some(UpstreamProblem::NotApproved),
            ),
            (
                modified,
                Upstream::Ok,
                Completeness::Complete,
                Some(UpstreamProblem::NotApproved),
            ),
            (
                Approval::Approved,
                stale_upstream(),
                Completeness::Complete,
                Some(UpstreamProblem::Stale),
            ),
            (
                Approval::Approved,
                blocked,
                Completeness::Complete,
                Some(UpstreamProblem::Blocked),
            ),
            (
                Approval::Approved,
                Upstream::Ok,
                Completeness::Complete,
                None,
            ),
        ];
        for (approval, upstream, completeness, first) in rows {
            let state = EntityState::new(approval.clone(), upstream.clone(), completeness);
            let got = upstream_problems(&Completeness::Complete, &[(eid(1), Some(&state))])
                .first()
                .map(|(_, problem)| *problem);
            assert_eq!(got, first, "{approval:?} / {upstream:?}");
        }
    }

    #[test]
    fn an_approved_stale_and_partial_upstream_is_unresolved_and_the_entity_above_it_keeps_both() {
        // Not Stale first: a clone cannot approve again, whatever changed.
        let state = EntityState::new(Approval::Approved, stale_upstream(), partial(5, 5));
        assert_eq!(
            upstream_problems(&Completeness::Complete, &[(eid(1), Some(&state))]),
            vec![(eid(1), UpstreamProblem::Unresolved)]
        );
        // The facts it states hold together: stale, and partial.
        let facts = UpstreamFacts::of(&state, true, false);
        assert_eq!(facts.health(), Some(HealthProblem::Stale));
        assert!(facts.is_partial());
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
    fn an_upstream_with_no_state_is_unresolved_only_when_the_target_names_it() {
        let named = partial(1, 1);
        // The other one is dangling: the target is Invalid for it, and says so itself.
        assert_eq!(
            upstream_problems(&named, &[(eid(1), None), (eid(2), None)]),
            vec![(eid(1), UpstreamProblem::Unresolved)]
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
        let found = leak_lint(text, &[alias("spec/secret-plan")], &[]);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].line(), 2);
        assert_eq!(found[0].name(), Some(&alias("spec/secret-plan")));
    }

    #[test]
    fn leak_lint_matches_whole_alias_tokens_only() {
        let lint = |text: &str| leak_lint(text, &[alias("spec/auth")], &[]);
        assert!(lint("spec/auth-v2 is public").is_empty());
        assert!(lint("myspec/auth").is_empty());
        assert_eq!(lint("(spec/auth)").len(), 1);
        assert_eq!(
            lint("see spec/auth.").len(),
            1,
            "a final dot ends a sentence"
        );
        assert!(
            lint("spec/auth.v2").is_empty(),
            "a dot between token characters extends it"
        );
        assert_eq!(lint("spec/auth, next").len(), 1);
    }

    #[test]
    fn leak_lint_finds_titles_case_insensitively_and_across_lines() {
        let text = "We follow the Quarterly Pricing Experiment\nresults.\n";
        assert_eq!(
            leak_lint(text, &[title("quarterly pricing experiment")], &[]).len(),
            1
        );
        let split = "Quarterly Pricing\nExperiment";
        assert_eq!(
            leak_lint(split, &[title("Quarterly Pricing\nExperiment")], &[]).len(),
            1
        );
    }

    #[test]
    fn a_title_that_starts_or_ends_in_a_letter_is_a_whole_word() {
        let lint = |text: &str| leak_lint(text, &[title("Cache")], &[]).len();
        assert_eq!(lint("the cache, and Cache."), 2);
        assert_eq!(lint("(CACHE)"), 1);
        assert_eq!(lint("cached values and precache and cache2"), 0);
        // A neighbour that is not an ASCII letter or digit does not extend the word, so a
        // title next to text in a script without word spacing is found.
        assert_eq!(lint("Cacheの設計"), 1);
        assert_eq!(lint("設計Cache"), 1);
        // Only the side that starts (ends) in a letter or digit is bounded.
        let both = |text: &str| leak_lint(text, &[title("-cache-")], &[]).len();
        assert_eq!(both("a-cache-b"), 1);
    }

    #[test]
    fn a_title_in_a_script_without_word_spacing_is_found_as_a_substring() {
        let lint = |text: &str| leak_lint(text, &[title("認証キャッシュ")], &[]).len();
        assert_eq!(lint("これは認証キャッシュの話"), 1);
        assert_eq!(lint("認証キャッシュ"), 1);
        let mixed = |text: &str| leak_lint(text, &[title("キャッシュ設計 v2")], &[]).len();
        assert_eq!(
            mixed("あのキャッシュ設計 v2です"),
            1,
            "the title ends in a digit, and a CJK character is no part of a word"
        );
        assert_eq!(mixed("キャッシュ設計 v22"), 0);
    }

    #[test]
    fn leak_lint_ignores_opaque_private_references_and_clean_text() {
        let id = eid(5);
        let text = format!("depends on private:{id}\n");
        assert!(
            leak_lint(
                &text,
                &[alias("spec/secret"), title("Secret roadmap")],
                &[id]
            )
            .is_empty()
        );
        assert!(leak_lint("", &[alias("spec/secret")], &[id]).is_empty());
        assert!(leak_lint("anything", &[], &[]).is_empty());
    }

    #[test]
    fn leak_lint_reports_each_occurrence_in_line_order() {
        let text = "a spec/x\nb spec/y\nc spec/x\n";
        let found = leak_lint(text, &[alias("spec/x"), alias("spec/y")], &[]);
        assert_eq!(
            found.iter().map(LeakFinding::line).collect::<Vec<_>>(),
            vec![1, 2, 3]
        );
    }

    #[test]
    fn an_identifier_of_a_private_entity_without_its_mark_is_found_in_a_key_and_in_the_body() {
        let id = eid(5);
        let other = eid(6);
        let text = format!(
            "---\ngrounds:\n  - {id}\n  - private:{other}\n---\nSee {id} and private:{id}.\n"
        );
        let found = leak_lint(&text, &[], &[id, other]);
        assert_eq!(
            found
                .iter()
                .map(|f| (f.line(), f.unmarked_reference().cloned()))
                .collect::<Vec<_>>(),
            vec![
                (
                    3,
                    Some(Ref::Local(crate::domain::reference::LocalRef::Id(id)))
                ),
                (
                    6,
                    Some(Ref::Local(crate::domain::reference::LocalRef::Id(id)))
                ),
            ],
            "the bare identifier of the key and of the body; the marked ones are fine"
        );
        assert!(found.iter().all(|f| f.name().is_none()));
    }

    #[test]
    fn the_mark_that_makes_an_identifier_fine_is_the_one_the_domain_writes() {
        let id = eid(5);
        let written = Ref::Private(id).to_string();
        assert!(leak_lint(&format!("x {written} y"), &[], &[id]).is_empty());
        assert_eq!(leak_lint(&format!("x {id} y"), &[], &[id]).len(), 1);
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
