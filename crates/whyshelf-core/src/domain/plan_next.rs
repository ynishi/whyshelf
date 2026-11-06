//! The next step, as a step with a target, what it needs, and room for a choice.
//!
//! [`next`] says *what* waits on a person. This module says what to
//! do about it in a form a command line, a tool server and a report can each turn
//! into their own words without deciding anything: which action, on what, because of
//! which upstream, needing what, and whether it is one step or a choice between two.
//! The domain holds no command text; a surface maps a [`NextAction`] to its own
//! grammar.
//!
//! ```text
//!   NextStep
//!     Do(DoStep)            one step: action + target (+ root) + needs
//!     Choose(TwoOrMore)     two or more DoSteps, one to pick; never nested
//!     Advise(Advice)        not a use case: a thing about the environment
//!     Wait(WaitFor)         nothing a person can do, something outside to wait for
//! ```
//!
//! # What a step is made of
//!
//! | part | meaning |
//! |---|---|
//! | [`NextAction`] | what to do: an edit of a file ([`EditFor`] says what for), an import, an approval, a measurement, a retirement, the two ends of a publication cut short |
//! | `target` | the [`EntityRef`] the step is about: the one asked |
//! | `root` | when the action must be run on an upstream first, that upstream; the action's subject is then the root and not the target ([`DoStep::subject`]) |
//! | `needs` | what the step cannot be done without, apart from the tool ([`Need`]) |
//!
//! `needs` says what is not the tool's to give: a person's judgement at a gate, a
//! clean working tree, tests that pass, a measurement, a person's confirmation of
//! something that cannot be taken back. It is the action's own list
//! ([`NextAction::needs`]), with the one addition the situation can make (a
//! confirmation) by [`DoStep::needing`]. Whether the step then *succeeds* is not a
//! thing a type can promise; the application's tests run each action from a state
//! that produces it.
//!
//! # Where the steps come from
//!
//! One function for what an entity asks for, and one for each other thing the
//! corpus can show:
//!
//! | function | reads | gives |
//! |---|---|---|
//! | [`next_for_entity`] | the state, its causes, its [`Readiness`], its upstreams, the naming lints of its references | what the document or item waits on |
//! | [`next_for_presence`] | the [`Presence`] of what has a judgement | a choice between bringing the file back and retiring it |
//! | [`import_steps`] | the files with no `id:` and the Designs with an item that has none | one `Import` each |
//! | [`publication_steps`] | the publications cut short | what finishes or undoes each |
//!
//! The order for an entity is the one of [`next`], read from its `state_step` and
//! not written again; this module translates its answer and adds what the old model had no place for:
//!
//! ```text
//!   Invalid                                    -> Edit(Fix reasons)
//!     (only items with no id)                  -> Import
//!   a store out of reach, further up           -> Advise(OpenWithPrivateRoot)
//!   Blocked by an upstream                     -> Approve(gate of the root), run on the root
//!     (the kind of the root not known)         -> Advise(UpstreamKindUnknown), never nothing
//!   Modified, Stale, or Draft, but the gate
//!   would refuse for what the entity lacks     -> Edit(Fill lacks)
//!   Modified, Stale                            -> Reapprove(gate)
//!   Draft                                      -> Approve(gate)
//!     (an item whose tests lack pins)          -> Edit(Implement pins)
//!   Approved and current
//!     a pin of its own with no key here,
//!     or a keyed pin of what is public now     -> Reapprove(gate)
//!     a Spec due for its measurement           -> Measure
//!     an item approved, not yet landed         -> Wait(Integration)
//!     a reference to repair                    -> Edit(DropMark) or Edit(StableItemRef)
//!
//!   file gone, with judgements                 -> Choose[Edit(Restore), Retire]
//!   file in both roots, not recorded published-> Choose[ResumePublish, AbortPublish]
//!   file in both roots, recorded published     -> ResumePublish
//!   file with no id                            -> Import
//! ```
//!
//! # What is never produced here
//!
//! [`NextAction::Adopt`] takes an earlier decision in as an approval without a gate
//! and is stronger than any gate, so it is not offered to whoever follows the next
//! step: it belongs to the plan of an import, which a person asks for. It is in the
//! type because a plan names it, and nothing here builds it.
//!
//! # Choosing
//!
//! A choice is two or more steps ([`TwoOrMore`]) and each of them is a step, not
//! another choice, so a choice cannot be nested.

use std::collections::{BTreeMap, BTreeSet};

use super::approval::Gate;
use super::corpus::{Corpus, Entity, Invalidity, Location, PublishForm, PublishInProgress};
use super::design::ItemIssue;
use super::evaluate::Cause;
use super::event::DocKind;
use super::id::{DocId, EntityId, ItemId};
use super::integration::Integration;
use super::judgement::Target;
use super::lint::upstream_problems;
use super::naming::NamingLint;
use super::next::{self, CmDue};
use super::nonempty::{NonEmptyVec, TwoOrMore};
use super::pin::PinId;
use super::state::{
    Approval, EntityState, InvalidReason, ItemProgress, Lack, LackField, LackNeed, Presence,
    Readiness,
};

/// What a step is about: a document, an item of a Design, or a file that is not
/// a document yet.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EntityRef {
    /// A document.
    Doc(DocId),
    /// An item of a Design.
    Item(ItemId),
    /// A file with no `id:`, which is only known by where it is.
    File(Box<Location>),
}

impl From<Target> for EntityRef {
    fn from(target: Target) -> Self {
        match target {
            Target::Doc(id) => Self::Doc(id),
            Target::Item(id) => Self::Item(id),
        }
    }
}

/// What waits on a person, for one thing. See the module documentation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NextStep {
    /// One step.
    Do(DoStep),
    /// Two or more steps, of which one is to be picked.
    Choose(TwoOrMore<DoStep>),
    /// Something about the environment, which no use case does.
    Advise(Advice),
    /// Nothing for a person to do, but something outside to wait for.
    Wait(WaitFor),
}

/// A step: an action on a target.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DoStep {
    action: NextAction,
    target: EntityRef,
    root: Option<EntityRef>,
    needs: Vec<Need>,
}

impl DoStep {
    /// A step of `action` on `target`, with the needs of the action.
    pub fn new(action: NextAction, target: EntityRef) -> Self {
        let needs = action.needs();
        Self {
            action,
            target,
            root: None,
            needs,
        }
    }

    /// The same step, run on `root`, an upstream of the target, first.
    pub fn on_root(mut self, root: EntityRef) -> Self {
        self.root = Some(root);
        self
    }

    /// The same step, with a need the situation adds to the action's own.
    pub fn needing(mut self, need: Need) -> Self {
        if !self.needs.contains(&need) {
            self.needs.push(need);
        }
        self
    }

    /// What to do.
    pub fn action(&self) -> &NextAction {
        &self.action
    }

    /// The thing the step is about: the one that was asked.
    pub fn target(&self) -> &EntityRef {
        &self.target
    }

    /// The upstream the action must be run on first, if it is not the target.
    pub fn root(&self) -> Option<&EntityRef> {
        self.root.as_ref()
    }

    /// What the action is run on: the root when there is one, the target otherwise.
    pub fn subject(&self) -> &EntityRef {
        self.root.as_ref().unwrap_or(&self.target)
    }

    /// What it cannot be done without.
    pub fn needs(&self) -> &[Need] {
        &self.needs
    }
}

/// What to do. One variant is one use case, or an edit that the person makes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NextAction {
    /// Edit a file: the tool does not.
    Edit(EditFor),
    /// Write an `id:` into a file that has none, or into an item that has none.
    Import,
    /// Run the gate and approve, for the first time.
    Approve(Gate),
    /// Run the gate and approve again: the approval no longer holds.
    Reapprove(Gate),
    /// Take an earlier decision in as an approval.
    ///
    /// Only the plan of an import names it; see the module documentation.
    Adopt,
    /// Record the measurement of the hypotheses.
    Measure,
    /// Record that something whose file is gone is gone on purpose.
    Retire,
    /// Finish a publication that stopped part way.
    ResumePublish,
    /// Undo a publication that stopped part way, before it was recorded.
    AbortPublish,
}

impl NextAction {
    /// What the action cannot be done without.
    ///
    /// | action | needs |
    /// |---|---|
    /// | `Approve`, `Reapprove` | a person's judgement at the gate; for CA also a clean working tree and passing tests |
    /// | `Measure` | a measurement |
    /// | `Adopt` | a person's confirmation: it cannot be taken back |
    /// | `ResumePublish`, `AbortPublish` | a confirmation if the plan has a step that cannot be taken back: the plan decides ([`Need::ConfirmationIfIrreversible`]) |
    /// | `Import` | nothing: the step only writes an `id:`, and an adoption is in the plan of an import a person asks for, whose own confirmation decides |
    /// | the others | nothing the tool does not have |
    pub fn needs(&self) -> Vec<Need> {
        match self {
            Self::Approve(gate) | Self::Reapprove(gate) => {
                let mut needs = vec![Need::Judgement(*gate)];
                if *gate == Gate::Ca {
                    needs.extend([Need::CleanTree, Need::TestsPass]);
                }
                needs
            }
            Self::Measure => vec![Need::Measurement],
            Self::Adopt => vec![Need::HumanConfirmation],
            Self::ResumePublish | Self::AbortPublish => vec![Need::ConfirmationIfIrreversible],
            Self::Edit(_) | Self::Import | Self::Retire => Vec::new(),
        }
    }
}

/// What an edit of a file is for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EditFor {
    /// Repair what makes the file Invalid.
    Fix(NonEmptyVec<InvalidReason>),
    /// Add what the next gate would refuse the entity for lacking.
    Fill(NonEmptyVec<Lack>),
    /// Make the tests carry these pins.
    Implement {
        /// The pins of the item that no test carries yet.
        pins: NonEmptyVec<PinId>,
    },
    /// Put a file back that is gone.
    Restore,
    /// Take the `private:` mark off the reference to this document, which is public
    /// now.
    DropMark(DocId),
    /// Name an item by its id and not its number in a reference.
    StableItemRef,
}

/// Something about the environment, which is not a use case.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Advice {
    /// Something it stands on is in a private root this environment does not have:
    /// open the project where that root is.
    OpenWithPrivateRoot,
    /// Something upstream must be approved first, and this environment does not
    /// know what kind of document `root` is, so it cannot say which gate approves
    /// it. Not expected: a document the corpus does not hold would be a dangling
    /// reference (Invalid) or a private root out of reach
    /// ([`OpenWithPrivateRoot`](Self::OpenWithPrivateRoot)) before it is the root of a
    /// chain. It is a step that must not vanish, so it is advice that names the
    /// document.
    UpstreamKindUnknown {
        /// The upstream to look at first.
        root: DocId,
    },
}

/// What there is to wait for, with nothing to do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WaitFor {
    /// The approved change to reach the baseline.
    Integration,
}

/// What a step cannot be done without.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Need {
    /// A person's judgement at this gate: the tool runs the machine check, and a
    /// person approves.
    Judgement(Gate),
    /// A working tree with nothing uncommitted.
    CleanTree,
    /// Tests that pass at the revision.
    TestsPass,
    /// A measurement to record.
    Measurement,
    /// A person's confirmation, from outside the process, of something that cannot
    /// be taken back.
    HumanConfirmation,
    /// A person's confirmation, from outside the process, if the plan of the step has
    /// a part that cannot be taken back; the plan says (its `confirmation`), and the
    /// step does not, since it is made before the plan is.
    ConfirmationIfIrreversible,
}

/// What an entity is asked about, beside its state: its kind decides its gate and
/// what else it can wait for.
#[derive(Debug, Clone, Copy)]
pub enum Subject<'a> {
    /// A Finding (gate CR).
    Finding(DocId),
    /// A Spec (gate CS).
    Spec {
        /// The document.
        id: DocId,
        /// Whether its measurement is due.
        cm_due: CmDue,
    },
    /// A Design (gate CI).
    Design(DocId),
    /// An item of a Design (gate CA).
    Item {
        /// The item.
        id: ItemId,
        /// Which of its pins the tests carry.
        progress: &'a ItemProgress,
        /// How its approved change has landed.
        integration: &'a Integration,
    },
}

impl Subject<'_> {
    fn gate(&self) -> Gate {
        match self {
            Self::Finding(_) => Gate::Cr,
            Self::Spec { .. } => Gate::Cs,
            Self::Design(_) => Gate::Ci,
            Self::Item { .. } => Gate::Ca,
        }
    }

    fn target(&self) -> EntityRef {
        match self {
            Self::Finding(id) | Self::Design(id) | Self::Spec { id, .. } => EntityRef::Doc(*id),
            Self::Item { id, .. } => EntityRef::Item(*id),
        }
    }

    /// The pins of an item that the tests do not carry; none for anything else, or
    /// when they all are.
    fn missing_pins(&self) -> Option<NonEmptyVec<PinId>> {
        match self {
            Self::Item { progress, .. } => NonEmptyVec::from_vec(
                progress
                    .declared()
                    .difference(progress.found())
                    .cloned()
                    .collect(),
            )
            .ok(),
            Self::Finding(_) | Self::Spec { .. } | Self::Design(_) => None,
        }
    }
}

/// Everything [`next_for_entity`] reads about one document or item.
#[derive(Debug, Clone, Copy)]
pub struct EntityAsk<'a> {
    subject: Subject<'a>,
    state: &'a EntityState,
    causes: &'a [Cause],
    readiness: &'a Readiness,
    upstreams: &'a [(EntityId, Option<&'a EntityState>)],
    kinds: &'a BTreeMap<EntityId, DocKind>,
    lints: &'a [NamingLint],
}

impl<'a> EntityAsk<'a> {
    /// A question about `subject`, whose evaluation gave `state`, `causes` and
    /// `readiness`.
    ///
    /// `kinds` is the kind of every document there is: it names the gate of the
    /// root of a chain that blocks the entity. It is asked for here and not left to
    /// be added, so that a caller cannot forget it and have the step to approve the
    /// root disappear (see [`Advice::UpstreamKindUnknown`] for a root that is not in
    /// it).
    pub fn new(
        subject: Subject<'a>,
        state: &'a EntityState,
        causes: &'a [Cause],
        readiness: &'a Readiness,
        kinds: &'a BTreeMap<EntityId, DocKind>,
    ) -> Self {
        Self {
            subject,
            state,
            causes,
            readiness,
            upstreams: &[],
            kinds,
            lints: &[],
        }
    }

    /// With what the entity stands on: each direct upstream and its state (none
    /// when it has none).
    pub fn standing_on(mut self, upstreams: &'a [(EntityId, Option<&'a EntityState>)]) -> Self {
        self.upstreams = upstreams;
        self
    }

    /// With the warnings the references of the document gave
    /// ([`document_lints`](super::naming::document_lints)).
    pub fn with_lints(mut self, lints: &'a [NamingLint]) -> Self {
        self.lints = lints;
        self
    }
}

fn is_unidentified_item(reason: &InvalidReason) -> bool {
    matches!(
        reason,
        InvalidReason::Corpus(invalidity)
            if matches!(**invalidity, Invalidity::Item(ItemIssue::UnidentifiedItem { .. }))
    )
}

fn do_step(action: NextAction, ask: &EntityAsk<'_>) -> NextStep {
    NextStep::Do(DoStep::new(action, ask.subject.target()))
}

/// What the document or item waits on, if it waits on anything.
///
/// The order is [`next`]'s: see the module documentation for what is
/// added to it.
pub fn next_for_entity(ask: &EntityAsk<'_>) -> Option<NextStep> {
    let problems = upstream_problems(ask.state.completeness(), ask.upstreams);
    let implementing = matches!(
        ask.subject,
        Subject::Item { progress, .. } if progress.is_implementing()
    );
    let base = next::state_step(
        ask.state,
        ask.causes,
        &problems,
        ask.subject.gate(),
        implementing,
    );
    match base {
        Some(step) => translate(step, ask),
        None if next::is_current(ask.state) => rest(ask),
        // Blocked by something no entity names: nothing to point at.
        None => None,
    }
}

/// The step of the old order, in the words of this model.
fn translate(step: next::NextStep, ask: &EntityAsk<'_>) -> Option<NextStep> {
    match step {
        next::NextStep::Fix => Some(fix(ask)),
        next::NextStep::OpenWithPrivateStore => Some(NextStep::Advise(Advice::OpenWithPrivateRoot)),
        next::NextStep::ApproveUpstream { root } => Some(match ask.kinds.get(&root) {
            Some(kind) => NextStep::Do(
                DoStep::new(
                    NextAction::Approve(Gate::of_doc(*kind)),
                    ask.subject.target(),
                )
                .on_root(EntityRef::Doc(DocId::new(root))),
            ),
            // The step cannot be built without the gate, and it is not dropped:
            // the root is named, and the person is told what is missing.
            None => NextStep::Advise(Advice::UpstreamKindUnknown {
                root: DocId::new(root),
            }),
        }),
        next::NextStep::Reapprove(gate) => Some(approve_or_fill(NextAction::Reapprove(gate), ask)),
        next::NextStep::Approve(gate) => Some(approve_or_fill(NextAction::Approve(gate), ask)),
        next::NextStep::Implement => {
            let pins = ask.subject.missing_pins()?;
            Some(do_step(NextAction::Edit(EditFor::Implement { pins }), ask))
        }
        next::NextStep::Measure => Some(do_step(NextAction::Measure, ask)),
        next::NextStep::AwaitIntegration => Some(NextStep::Wait(WaitFor::Integration)),
    }
}

/// An Invalid entity is to be fixed, or, when the only thing wrong is an item with
/// no id, imported.
fn fix(ask: &EntityAsk<'_>) -> NextStep {
    let reasons: Vec<InvalidReason> = match ask.state.approval() {
        Approval::Invalid(reasons) => reasons
            .iter()
            .filter(|reason| !is_unidentified_item(reason))
            .cloned()
            .collect(),
        Approval::Draft | Approval::Approved | Approval::Modified { .. } => Vec::new(),
    };
    match NonEmptyVec::from_vec(reasons) {
        Ok(reasons) => do_step(NextAction::Edit(EditFor::Fix(reasons)), ask),
        Err(_) => do_step(NextAction::Import, ask),
    }
}

/// Run the gate, unless the gate would refuse for what the entity lacks.
///
/// A keyed pin of an upstream that is public now is a lack too, but approving
/// again is what mends it, so it is not something to fill.
fn approve_or_fill(action: NextAction, ask: &EntityAsk<'_>) -> NextStep {
    let lacks: Vec<Lack> = ask
        .readiness
        .lacks()
        .iter()
        .filter(|lack| {
            lack.need() == LackNeed::Required && !matches!(lack.field(), LackField::PlainPin { .. })
        })
        .copied()
        .collect();
    match NonEmptyVec::from_vec(lacks) {
        Ok(lacks) => do_step(NextAction::Edit(EditFor::Fill(lacks)), ask),
        Err(_) => do_step(action, ask),
    }
}

/// What an approved and current entity waits on when its state asks for nothing.
fn rest(ask: &EntityAsk<'_>) -> Option<NextStep> {
    let public_now = ask
        .readiness
        .lacks()
        .iter()
        .any(|lack| matches!(lack.field(), LackField::PlainPin { .. }));
    if public_now {
        // The same gate as any approval again, so a lack it would refuse for is
        // filled first.
        return Some(approve_or_fill(
            NextAction::Reapprove(ask.subject.gate()),
            ask,
        ));
    }
    match ask.subject {
        Subject::Spec {
            cm_due: CmDue::Due, ..
        } => return Some(do_step(NextAction::Measure, ask)),
        Subject::Item {
            integration: Integration::Pending { .. },
            ..
        } => return Some(NextStep::Wait(WaitFor::Integration)),
        Subject::Finding(_) | Subject::Spec { .. } | Subject::Design(_) | Subject::Item { .. } => {}
    }
    ask.lints.first().map(|lint| {
        let edit = match lint {
            NamingLint::MarkStale { doc, .. } => EditFor::DropMark(*doc),
            NamingLint::FragileItemRef { .. } => EditFor::StableItemRef,
        };
        do_step(NextAction::Edit(edit), ask)
    })
}

/// What something that has judgements waits on, by whether its file is there.
///
/// A file that is gone is brought back or retired: two steps and a choice.
pub fn next_for_presence(target: Target, presence: Presence) -> Option<NextStep> {
    match presence {
        Presence::Missing => {
            let target = EntityRef::from(target);
            Some(NextStep::Choose(TwoOrMore::new(
                DoStep::new(NextAction::Edit(EditFor::Restore), target.clone()),
                DoStep::new(NextAction::Retire, target),
                Vec::new(),
            )))
        }
        Presence::Present | Presence::Retired => None,
    }
}

/// The files and items that are to be given an `id:`, one step each.
///
/// A file with no `id:` is a candidate whole; a Design with an item that has none is
/// one step on the Design, whatever the number of its items.
pub fn import_steps(corpus: &Corpus) -> Vec<DoStep> {
    let files = corpus.candidates().iter().map(|candidate| {
        DoStep::new(
            NextAction::Import,
            EntityRef::File(Box::new(candidate.facts().location().clone())),
        )
    });
    let designs: BTreeSet<DocId> = corpus
        .documents()
        .iter()
        .filter(|file| match file.document().entity() {
            Entity::Design(design) => design
                .item_issues()
                .iter()
                .any(|issue| matches!(issue, ItemIssue::UnidentifiedItem { .. })),
            Entity::Finding(_) | Entity::Spec(_) => false,
        })
        .map(|file| file.id())
        .collect();
    let items = designs
        .into_iter()
        .map(|id| DoStep::new(NextAction::Import, EntityRef::Doc(id)));
    files.chain(items).collect()
}

/// What a publication that stopped part way waits on.
///
/// A copy not yet recorded as published is finished or undone: a choice. Once the
/// publication is recorded there is one way on, finishing.
///
/// Whether either asks a person to confirm is the plan's to say, not this step's:
/// finishing a copy writes the records into the public ledger again from the start,
/// which cannot be taken back, and finishing a recorded one takes back a private copy
/// that may say something the public file does not. The step says only that the plan
/// decides ([`Need::ConfirmationIfIrreversible`]), so it cannot contradict the plan
/// ([`PublicationPlan::confirmation`](super::publication::PublicationPlan::confirmation),
/// [`AbortPlan::confirmation`](super::publication::AbortPlan::confirmation)).
pub fn publication_step(publication: &PublishInProgress) -> NextStep {
    let target = EntityRef::Doc(publication.id());
    let resume = DoStep::new(NextAction::ResumePublish, target.clone());
    match publication.form() {
        PublishForm::FileCopied => NextStep::Choose(TwoOrMore::new(
            resume,
            DoStep::new(NextAction::AbortPublish, target),
            Vec::new(),
        )),
        PublishForm::PrivateCopyLeft => NextStep::Do(resume),
    }
}

/// [`publication_step`] for every publication the corpus shows stopped part way.
pub fn publication_steps(corpus: &Corpus) -> Vec<NextStep> {
    corpus.publishing().iter().map(publication_step).collect()
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::*;
    use crate::domain::corpus::{Document, FileFacts, GitPlace, Header, Reading, Visibility};
    use crate::domain::design::test_support::{design, identified, item, num};
    use crate::domain::hash::BytesHash;
    use crate::domain::id::test_support::{docid, eid, iid};
    use crate::domain::knowable::Knowable;
    use crate::domain::naming::Ref;
    use crate::domain::revision::{Revision, RevisionId};
    use crate::domain::spec::test_support::spec as spec_of;
    use crate::domain::state::{
        BlockReason, Completeness, StaleReason, UnresolvedReason, Upstream,
    };

    // --- the states ---------------------------------------------------------------

    fn state(approval: Approval, upstream: Upstream) -> EntityState {
        EntityState::new(approval, upstream, Completeness::Complete)
    }

    fn invalid_with(reason: InvalidReason) -> Approval {
        Approval::Invalid(NonEmptyVec::new(reason, vec![]))
    }

    fn invalid() -> Approval {
        invalid_with(InvalidReason::DanglingRef { upstream: eid(9) })
    }

    fn unidentified() -> InvalidReason {
        InvalidReason::Corpus(Box::new(Invalidity::Item(ItemIssue::UnidentifiedItem {
            number: num(1),
        })))
    }

    fn modified() -> Approval {
        Approval::Modified {
            approved: crate::domain::hash::ContentHash::raw(b"old"),
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

    fn pins(names: &[&str]) -> BTreeSet<PinId> {
        names.iter().map(|n| n.parse().unwrap()).collect()
    }

    fn rev() -> Revision {
        Revision::Commit(RevisionId::from_validated("r1".into()).unwrap())
    }

    fn no_lacks() -> Readiness {
        Readiness::default()
    }

    /// No document has a kind known here: for the questions that never reach the
    /// root of a chain.
    static NO_KINDS: BTreeMap<EntityId, DocKind> = BTreeMap::new();

    fn ask_of<'a>(
        subject: Subject<'a>,
        state: &'a EntityState,
        causes: &'a [Cause],
        readiness: &'a Readiness,
    ) -> EntityAsk<'a> {
        EntityAsk::new(subject, state, causes, readiness, &NO_KINDS)
    }

    fn lacking(field: LackField, need: LackNeed) -> Readiness {
        Readiness::new(vec![Lack::new(field, need)])
    }

    fn doc_target(n: u128) -> EntityRef {
        EntityRef::Doc(docid(n))
    }

    fn step(action: NextAction, target: EntityRef) -> Option<NextStep> {
        Some(NextStep::Do(DoStep::new(action, target)))
    }

    /// The kind of every action, written out so that a new action has to be named
    /// here, and so in the coverage test below.
    fn kind(action: &NextAction) -> &'static str {
        match action {
            NextAction::Edit(EditFor::Fix(_)) => "edit-fix",
            NextAction::Edit(EditFor::Fill(_)) => "edit-fill",
            NextAction::Edit(EditFor::Implement { .. }) => "edit-implement",
            NextAction::Edit(EditFor::Restore) => "edit-restore",
            NextAction::Edit(EditFor::DropMark(_)) => "edit-drop-mark",
            NextAction::Edit(EditFor::StableItemRef) => "edit-stable-item-ref",
            NextAction::Import => "import",
            NextAction::Approve(_) => "approve",
            NextAction::Reapprove(_) => "reapprove",
            NextAction::Adopt => "adopt",
            NextAction::Measure => "measure",
            NextAction::Retire => "retire",
            NextAction::ResumePublish => "resume-publish",
            NextAction::AbortPublish => "abort-publish",
        }
    }

    const ALL_KINDS: [&str; 14] = [
        "edit-fix",
        "edit-fill",
        "edit-implement",
        "edit-restore",
        "edit-drop-mark",
        "edit-stable-item-ref",
        "import",
        "approve",
        "reapprove",
        "adopt",
        "measure",
        "retire",
        "resume-publish",
        "abort-publish",
    ];

    fn actions_of(step: &NextStep) -> Vec<&NextAction> {
        match step {
            NextStep::Do(step) => vec![step.action()],
            NextStep::Choose(steps) => steps.iter().map(DoStep::action).collect(),
            NextStep::Advise(_) | NextStep::Wait(_) => Vec::new(),
        }
    }

    // --- the corpus ---------------------------------------------------------------

    fn rel(path: &str) -> crate::domain::location::RelPath {
        path.parse().unwrap()
    }

    fn public_facts(path: &str, bytes: &[u8]) -> FileFacts {
        FileFacts::new(
            Location::new(
                Visibility::Public,
                rel(path),
                Knowable::Known(GitPlace::Tracked),
            ),
            BytesHash::of(bytes),
        )
    }

    fn private_facts(path: &str, bytes: &[u8]) -> FileFacts {
        FileFacts::new(
            Location::new(
                Visibility::Private,
                rel(path),
                Knowable::Known(GitPlace::Ignored),
            ),
            BytesHash::of(bytes),
        )
    }

    fn spec_document(n: u128) -> Reading {
        let header = Header::new(docid(n), DocKind::Spec, None, BTreeSet::new(), None);
        Reading::resolved(
            Document::new(header, Entity::Spec(spec_of(n, &[900], &["PIN-A"]))).unwrap(),
        )
    }

    fn design_document(n: u128, items: Vec<(u32, crate::domain::design::WorkItem)>) -> Reading {
        let header = Header::new(docid(n), DocKind::Design, None, BTreeSet::new(), None);
        Reading::resolved(Document::new(header, Entity::Design(design(n, 900, items))).unwrap())
    }

    fn corpus_cut_short(published: bool, private_bytes: &[u8]) -> Corpus {
        let published = if published {
            BTreeSet::from([docid(1)])
        } else {
            BTreeSet::new()
        };
        Corpus::build(
            [
                (public_facts("spec/a.md", b"same"), spec_document(1)),
                (private_facts("spec/a.md", private_bytes), spec_document(1)),
            ],
            &published,
        )
    }

    // --- every action has a state that produces it --------------------------------

    #[test]
    fn each_action_but_adopt_has_a_state_that_produces_it() {
        let doc = Subject::Finding(docid(1));
        let design_subject = Subject::Design(docid(1));
        let spec_due = Subject::Spec {
            id: docid(1),
            cm_due: CmDue::Due,
        };
        let lacking_progress = ItemProgress::new(pins(&[]), pins(&["PIN-A"]));
        let carried = ItemProgress::new(pins(&["PIN-A"]), pins(&["PIN-A"]));
        let pending = Integration::Pending { approved: rev() };
        let implementing = Subject::Item {
            id: iid(5),
            progress: &lacking_progress,
            integration: &Integration::Unapproved,
        };
        let waiting = Subject::Item {
            id: iid(5),
            progress: &carried,
            integration: &pending,
        };

        let draft = state(Approval::Draft, Upstream::Ok);
        let approved = state(Approval::Approved, Upstream::Ok);
        let invalid_state = state(invalid(), Upstream::Ok);
        let unidentified_state = state(invalid_with(unidentified()), Upstream::Ok);
        let modified_state = state(modified(), Upstream::Ok);
        let nothing = no_lacks();
        let missing_pins = lacking(LackField::Pins, LackNeed::Required);
        let public_now = lacking(LackField::PlainPin { upstream: eid(2) }, LackNeed::Required);
        let lints = [NamingLint::MarkStale {
            reference: Ref::Private(Box::new(Ref::Id(docid(3)))),
            doc: docid(3),
        }];
        let fragile = [NamingLint::FragileItemRef {
            reference: Ref::Item(Box::new(Ref::Id(docid(3))), num(1)),
        }];

        let mut produced: BTreeSet<&'static str> = BTreeSet::new();
        let mut collect = |step: Option<NextStep>| {
            for action in actions_of(&step.expect("a step")) {
                produced.insert(kind(action));
            }
        };

        collect(next_for_entity(&ask_of(doc, &invalid_state, &[], &nothing)));
        collect(next_for_entity(&ask_of(
            design_subject,
            &unidentified_state,
            &[],
            &nothing,
        )));
        collect(next_for_entity(&ask_of(
            Subject::Spec {
                id: docid(1),
                cm_due: CmDue::NotDue,
            },
            &draft,
            &[],
            &missing_pins,
        )));
        collect(next_for_entity(&ask_of(
            implementing,
            &draft,
            &[],
            &nothing,
        )));
        collect(next_for_entity(&ask_of(doc, &draft, &[], &nothing)));
        collect(next_for_entity(&ask_of(
            doc,
            &modified_state,
            &[],
            &nothing,
        )));
        collect(next_for_entity(&ask_of(spec_due, &approved, &[], &nothing)));
        collect(next_for_entity(&ask_of(waiting, &approved, &[], &nothing)));
        collect(next_for_entity(&ask_of(doc, &approved, &[], &public_now)));
        collect(next_for_entity(
            &ask_of(doc, &approved, &[], &nothing).with_lints(&lints),
        ));
        collect(next_for_entity(
            &ask_of(doc, &approved, &[], &nothing).with_lints(&fragile),
        ));
        collect(next_for_presence(Target::Doc(docid(1)), Presence::Missing));
        collect(Some(publication_step(
            &corpus_cut_short(false, b"same").publishing()[0],
        )));
        collect(Some(publication_step(
            &corpus_cut_short(true, b"same").publishing()[0],
        )));
        let imports = import_steps(&Corpus::build(
            [(
                public_facts("note.md", b"x"),
                Reading::Unidentified { kind: None },
            )],
            &BTreeSet::new(),
        ));
        for step in imports {
            collect(Some(NextStep::Do(step)));
        }

        for name in ALL_KINDS {
            if name == "adopt" {
                assert!(!produced.contains(name), "adopt is never produced here");
            } else {
                assert!(produced.contains(name), "no state produced `{name}`");
            }
        }
        assert!(produced.iter().all(|name| ALL_KINDS.contains(name)));
    }

    // --- the order of an entity -----------------------------------------------------

    #[test]
    fn an_invalid_entity_is_fixed_before_anything_else() {
        let reasons = NonEmptyVec::new(InvalidReason::DanglingRef { upstream: eid(9) }, vec![]);
        for upstream in [Upstream::Ok, blocked(), stale()] {
            let s = state(invalid(), upstream);
            let n = no_lacks();
            let ask = ask_of(Subject::Finding(docid(1)), &s, &[], &n);
            assert_eq!(
                next_for_entity(&ask),
                step(
                    NextAction::Edit(EditFor::Fix(reasons.clone())),
                    doc_target(1)
                )
            );
        }
    }

    #[test]
    fn a_design_whose_only_fault_is_an_item_with_no_id_is_imported_and_fixed_otherwise() {
        let n = no_lacks();
        let only = state(invalid_with(unidentified()), Upstream::Ok);
        let ask = ask_of(Subject::Design(docid(1)), &only, &[], &n);
        assert_eq!(
            next_for_entity(&ask),
            step(NextAction::Import, doc_target(1))
        );

        // With another fault, that one is to be fixed first, and the missing id is
        // not among what is to be fixed.
        let both = state(
            Approval::Invalid(NonEmptyVec::new(
                unidentified(),
                vec![InvalidReason::DanglingRef { upstream: eid(9) }],
            )),
            Upstream::Ok,
        );
        let ask = ask_of(Subject::Design(docid(1)), &both, &[], &n);
        assert_eq!(
            next_for_entity(&ask),
            step(
                NextAction::Edit(EditFor::Fix(NonEmptyVec::new(
                    InvalidReason::DanglingRef { upstream: eid(9) },
                    vec![]
                ))),
                doc_target(1)
            )
        );
    }

    #[test]
    fn a_store_out_of_reach_is_advice_to_open_the_private_root() {
        let partial =
            Completeness::from_reasons(BTreeSet::from([UnresolvedReason::new(eid(5), eid(5))]));
        let s = EntityState::new(Approval::Draft, Upstream::Ok, partial);
        let n = no_lacks();
        let upstream_state = state(Approval::Draft, Upstream::Ok);
        let upstreams = [(eid(5), Some(&upstream_state))];
        let kinds = BTreeMap::from([(eid(5), DocKind::Finding)]);
        let ask =
            EntityAsk::new(Subject::Design(docid(1)), &s, &[], &n, &kinds).standing_on(&upstreams);
        // The upstream here is read and not partial; with no upstream problem the
        // Draft is simply to be approved.
        assert_eq!(
            next_for_entity(&ask),
            step(NextAction::Approve(Gate::Ci), doc_target(1))
        );
        // An upstream that cannot be read is the problem, and comes first.
        let unread = [(eid(5), None)];
        let ask =
            EntityAsk::new(Subject::Design(docid(1)), &s, &[], &n, &kinds).standing_on(&unread);
        assert_eq!(
            next_for_entity(&ask),
            Some(NextStep::Advise(Advice::OpenWithPrivateRoot))
        );
    }

    #[test]
    fn a_key_that_is_missing_asks_to_approve_again_and_a_store_asks_to_open_it() {
        let key = Completeness::from_reasons(BTreeSet::from([UnresolvedReason::key_missing(
            eid(5),
            "k1".parse().unwrap(),
        )]));
        let n = no_lacks();
        let lacks_key = EntityState::new(Approval::Approved, Upstream::Ok, key);
        let ask = ask_of(Subject::Design(docid(1)), &lacks_key, &[], &n);
        assert_eq!(
            next_for_entity(&ask),
            step(NextAction::Reapprove(Gate::Ci), doc_target(1)),
            "the upstream is there to be read; the pin is the entity's own"
        );

        let store =
            Completeness::from_reasons(BTreeSet::from([UnresolvedReason::new(eid(5), eid(5))]));
        let in_store = EntityState::new(Approval::Approved, Upstream::Ok, store);
        let unread = [(eid(5), None)];
        let kinds = BTreeMap::new();
        let ask = EntityAsk::new(Subject::Design(docid(1)), &in_store, &[], &n, &kinds)
            .standing_on(&unread);
        assert_eq!(
            next_for_entity(&ask),
            Some(NextStep::Advise(Advice::OpenWithPrivateRoot))
        );
    }

    #[test]
    fn a_blocked_entity_is_sent_to_the_root_of_the_chain_with_that_gate() {
        let s = state(Approval::Draft, blocked());
        let n = no_lacks();
        let up = state(Approval::Draft, Upstream::Ok);
        let upstreams = [(eid(1), Some(&up))];
        let kinds = BTreeMap::from([(eid(1), DocKind::Spec), (eid(7), DocKind::Finding)]);
        let causes = [
            Cause::for_test(eid(7), eid(1)),
            Cause::for_test(eid(1), eid(1)),
        ];
        let ask = EntityAsk::new(Subject::Design(docid(2)), &s, &causes, &n, &kinds)
            .standing_on(&upstreams);
        let got = next_for_entity(&ask).unwrap();
        let NextStep::Do(step) = got else {
            panic!("a step");
        };
        assert_eq!(step.action(), &NextAction::Approve(Gate::Cr));
        assert_eq!(step.target(), &doc_target(2));
        assert_eq!(step.root(), Some(&EntityRef::Doc(DocId::new(eid(7)))));
        assert_eq!(step.subject(), &EntityRef::Doc(DocId::new(eid(7))));
        assert_eq!(step.needs(), &[Need::Judgement(Gate::Cr)]);

        // A root whose kind is not known has no gate to name, and the step does
        // not vanish: it is advice that names the root.
        let none = BTreeMap::new();
        let ask = EntityAsk::new(Subject::Design(docid(2)), &s, &causes, &n, &none)
            .standing_on(&upstreams);
        assert_eq!(
            next_for_entity(&ask),
            Some(NextStep::Advise(Advice::UpstreamKindUnknown {
                root: DocId::new(eid(7))
            }))
        );
    }

    #[test]
    fn a_hunks_integrated_item_makes_the_measurement_of_its_spec_due() {
        use crate::domain::approval::ApprovalSet;
        use crate::domain::integration::{
            ApprovedChange, BaselineFacts, ChangeFingerprint, Hunk, HunkScheme, UnknownWhy,
        };

        let fingerprint = ChangeFingerprint::of_hunks(
            rev(),
            rev(),
            &[Hunk::new(
                rel("src/a.rs"),
                vec![],
                vec!["let timeout = Duration::from_secs(30);".to_owned()],
            )],
            HunkScheme::CURRENT,
        )
        .unwrap();
        // Squash-merged: the approved commit is not in the history, its hunks are.
        let landed = |hunks: bool| {
            Integration::decide([ApprovedChange::checked(
                rev(),
                crate::domain::approval::CaChange::Fingerprinted(fingerprint.clone()),
                BaselineFacts::new(Knowable::Known(false), Knowable::Known(hunks)),
                UnknownWhy::Shallow,
            )])
        };

        let spec = spec_of(1, &[900], &["PIN-A"]);
        let ledger = ApprovalSet::default();
        let s = state(Approval::Approved, Upstream::Ok);
        let n = no_lacks();
        let next_of = |integration: &Integration| {
            let cm_due = CmDue::of([integration.is_integrated()], &ledger, &spec);
            next_for_entity(&ask_of(
                Subject::Spec {
                    id: docid(1),
                    cm_due,
                },
                &s,
                &[],
                &n,
            ))
        };

        let got = next_of(&landed(true)).expect("a step");
        let NextStep::Do(measure) = got else {
            panic!("a step");
        };
        assert_eq!(measure.action(), &NextAction::Measure);
        assert_eq!(measure.needs(), &[Need::Measurement]);
        // Not landed (a known no on both), or not told: not due.
        assert_eq!(next_of(&landed(false)), None);
        assert_eq!(next_of(&Integration::Unapproved), None);
    }

    #[test]
    fn what_the_gate_would_refuse_for_is_filled_before_the_gate_is_run() {
        let n = lacking(LackField::Pins, LackNeed::Required);
        let spec = Subject::Spec {
            id: docid(1),
            cm_due: CmDue::NotDue,
        };
        let fill = || {
            EditFor::Fill(NonEmptyVec::new(
                Lack::new(LackField::Pins, LackNeed::Required),
                vec![],
            ))
        };
        for approval in [Approval::Draft, modified()] {
            let s = state(approval, Upstream::Ok);
            assert_eq!(
                next_for_entity(&ask_of(spec, &s, &[], &n)),
                step(NextAction::Edit(fill()), doc_target(1))
            );
        }
        // A lack the gate does not refuse for is not filled.
        let informational = lacking(LackField::Pins, LackNeed::Informational);
        let s = state(Approval::Draft, Upstream::Ok);
        assert_eq!(
            next_for_entity(&ask_of(spec, &s, &[], &informational)),
            step(NextAction::Approve(Gate::Cs), doc_target(1))
        );
    }

    #[test]
    fn a_draft_approves_a_modified_or_stale_one_approves_again() {
        let n = no_lacks();
        let finding = Subject::Finding(docid(1));
        for (approval, upstream, action) in [
            (Approval::Draft, Upstream::Ok, NextAction::Approve(Gate::Cr)),
            (modified(), Upstream::Ok, NextAction::Reapprove(Gate::Cr)),
            (Approval::Approved, stale(), NextAction::Reapprove(Gate::Cr)),
        ] {
            let s = state(approval, upstream);
            assert_eq!(
                next_for_entity(&ask_of(finding, &s, &[], &n)),
                step(action, doc_target(1))
            );
        }
        let s = state(Approval::Approved, Upstream::Ok);
        assert_eq!(next_for_entity(&ask_of(finding, &s, &[], &n)), None);
    }

    #[test]
    fn an_item_whose_tests_lack_pins_implements_exactly_those() {
        let progress = ItemProgress::new(pins(&["PIN-A"]), pins(&["PIN-A", "PIN-B", "PIN-C"]));
        let subject = Subject::Item {
            id: iid(5),
            progress: &progress,
            integration: &Integration::Unapproved,
        };
        let s = state(Approval::Draft, Upstream::Ok);
        let n = no_lacks();
        assert_eq!(
            next_for_entity(&ask_of(subject, &s, &[], &n)),
            step(
                NextAction::Edit(EditFor::Implement {
                    pins: NonEmptyVec::new(
                        "PIN-B".parse().unwrap(),
                        vec!["PIN-C".parse().unwrap()]
                    )
                }),
                EntityRef::Item(iid(5))
            )
        );
        // Approving a Draft item needs the tests, the tree and a judgement.
        let carried = ItemProgress::new(pins(&["PIN-A"]), pins(&["PIN-A"]));
        let subject = Subject::Item {
            id: iid(5),
            progress: &carried,
            integration: &Integration::Unapproved,
        };
        let got = next_for_entity(&ask_of(subject, &s, &[], &n)).unwrap();
        let NextStep::Do(do_step) = got else {
            panic!("a step");
        };
        assert_eq!(do_step.action(), &NextAction::Approve(Gate::Ca));
        assert_eq!(
            do_step.needs(),
            &[Need::Judgement(Gate::Ca), Need::CleanTree, Need::TestsPass]
        );
    }

    #[test]
    fn an_approved_item_waits_while_pending_and_not_otherwise() {
        let carried = ItemProgress::new(pins(&["PIN-A"]), pins(&["PIN-A"]));
        let s = state(Approval::Approved, Upstream::Ok);
        let n = no_lacks();
        let ask = |integration: &Integration| {
            next_for_entity(&ask_of(
                Subject::Item {
                    id: iid(5),
                    progress: &carried,
                    integration,
                },
                &s,
                &[],
                &n,
            ))
        };
        assert_eq!(
            ask(&Integration::Pending { approved: rev() }),
            Some(NextStep::Wait(WaitFor::Integration))
        );
        assert_eq!(ask(&Integration::Unapproved), None);
        assert_eq!(
            ask(&Integration::Unknown {
                approved: rev(),
                why: crate::domain::integration::UnknownWhy::Shallow
            }),
            None
        );
        assert_eq!(
            ask(&Integration::Integrated {
                evidence: crate::domain::integration::IntegrationEvidence::HunksContained
            }),
            None
        );
    }

    #[test]
    fn an_approved_spec_is_measured_when_due() {
        let s = state(Approval::Approved, Upstream::Ok);
        let n = no_lacks();
        let ask = |cm_due| {
            next_for_entity(&ask_of(
                Subject::Spec {
                    id: docid(1),
                    cm_due,
                },
                &s,
                &[],
                &n,
            ))
        };
        let got = ask(CmDue::Due).unwrap();
        let NextStep::Do(measure) = got else {
            panic!("a step");
        };
        assert_eq!(measure.action(), &NextAction::Measure);
        assert_eq!(measure.needs(), &[Need::Measurement]);
        assert_eq!(ask(CmDue::NotDue), None);
    }

    #[test]
    fn a_keyed_pin_of_what_is_public_now_asks_to_approve_again() {
        let s = state(Approval::Approved, Upstream::Ok);
        let n = lacking(LackField::PlainPin { upstream: eid(2) }, LackNeed::Required);
        // It is no lack to fill, even before the entity is approved again.
        assert_eq!(
            next_for_entity(&ask_of(Subject::Design(docid(1)), &s, &[], &n)),
            step(NextAction::Reapprove(Gate::Ci), doc_target(1))
        );
        // Approving again is the gate like any other: what it would refuse for is
        // filled first.
        let both = Readiness::new(vec![
            Lack::new(LackField::PlainPin { upstream: eid(2) }, LackNeed::Required),
            Lack::new(LackField::Pins, LackNeed::Required),
        ]);
        let spec = Subject::Spec {
            id: docid(1),
            cm_due: CmDue::NotDue,
        };
        assert_eq!(
            next_for_entity(&ask_of(spec, &s, &[], &both)),
            step(
                NextAction::Edit(EditFor::Fill(NonEmptyVec::new(
                    Lack::new(LackField::Pins, LackNeed::Required),
                    vec![]
                ))),
                doc_target(1)
            )
        );
    }

    #[test]
    fn a_blocked_entity_with_no_root_to_name_waits_on_nothing_else() {
        // Blocked by something no entity names: neither a measurement nor the
        // baseline is asked for, and no reference repair either.
        let s = state(Approval::Approved, blocked());
        let n = no_lacks();
        let carried = ItemProgress::new(pins(&["PIN-A"]), pins(&["PIN-A"]));
        let pending = Integration::Pending { approved: rev() };
        let lints = [NamingLint::FragileItemRef {
            reference: Ref::Item(Box::new(Ref::Id(docid(3))), num(1)),
        }];
        for subject in [
            Subject::Spec {
                id: docid(1),
                cm_due: CmDue::Due,
            },
            Subject::Item {
                id: iid(5),
                progress: &carried,
                integration: &pending,
            },
            Subject::Design(docid(1)),
        ] {
            let ask = ask_of(subject, &s, &[], &n).with_lints(&lints);
            assert_eq!(next_for_entity(&ask), None, "{subject:?}");
        }
    }

    #[test]
    fn a_reference_to_repair_is_the_last_thing_asked() {
        let s = state(Approval::Approved, Upstream::Ok);
        let n = no_lacks();
        let lints = [
            NamingLint::MarkStale {
                reference: Ref::Private(Box::new(Ref::Id(docid(3)))),
                doc: docid(3),
            },
            NamingLint::FragileItemRef {
                reference: Ref::Item(Box::new(Ref::Id(docid(3))), num(1)),
            },
        ];
        let ask = ask_of(Subject::Design(docid(1)), &s, &[], &n).with_lints(&lints);
        assert_eq!(
            next_for_entity(&ask),
            step(NextAction::Edit(EditFor::DropMark(docid(3))), doc_target(1))
        );
        // Something else to do comes first.
        let draft = state(Approval::Draft, Upstream::Ok);
        let ask = ask_of(Subject::Design(docid(1)), &draft, &[], &n).with_lints(&lints);
        assert_eq!(
            next_for_entity(&ask),
            step(NextAction::Approve(Gate::Ci), doc_target(1))
        );
        let fragile = &lints[1..];
        let ask = ask_of(Subject::Design(docid(1)), &s, &[], &n).with_lints(fragile);
        assert_eq!(
            next_for_entity(&ask),
            step(NextAction::Edit(EditFor::StableItemRef), doc_target(1))
        );
    }

    // --- presence --------------------------------------------------------------------

    #[test]
    fn a_missing_file_is_a_choice_between_restoring_and_retiring() {
        let target = Target::Item(iid(5));
        let want = NextStep::Choose(TwoOrMore::new(
            DoStep::new(NextAction::Edit(EditFor::Restore), EntityRef::Item(iid(5))),
            DoStep::new(NextAction::Retire, EntityRef::Item(iid(5))),
            vec![],
        ));
        assert_eq!(next_for_presence(target, Presence::Missing), Some(want));
        assert_eq!(next_for_presence(target, Presence::Present), None);
        assert_eq!(next_for_presence(target, Presence::Retired), None);
    }

    // --- publication ----------------------------------------------------------------

    #[test]
    fn a_copy_not_recorded_as_published_is_resumed_or_aborted() {
        let corpus = corpus_cut_short(false, b"same");
        let steps = publication_steps(&corpus);
        assert_eq!(steps.len(), 1);
        let NextStep::Choose(choice) = &steps[0] else {
            panic!("a choice");
        };
        let steps: Vec<&DoStep> = choice.iter().collect();
        assert_eq!(steps.len(), 2);
        assert_eq!(steps[0].action(), &NextAction::ResumePublish);
        assert_eq!(steps[1].action(), &NextAction::AbortPublish);
        assert!(steps.iter().all(|s| s.target() == &doc_target(1)));
        // Whether a person is asked is the plan's to say, for both.
        assert_eq!(steps[0].needs(), &[Need::ConfirmationIfIrreversible]);
        assert_eq!(steps[1].needs(), &[Need::ConfirmationIfIrreversible]);
    }

    #[test]
    fn a_publication_recorded_as_published_is_resumed_and_the_plan_decides_what_it_asks() {
        for private in [&b"same"[..], &b"edited"[..]] {
            let steps = publication_steps(&corpus_cut_short(true, private));
            let NextStep::Do(resume) = &steps[0] else {
                panic!("a step");
            };
            assert_eq!(resume.action(), &NextAction::ResumePublish);
            // A step is made before the plan, so it cannot know whether the copy
            // differs; it never says more than the plan will.
            assert_eq!(resume.needs(), &[Need::ConfirmationIfIrreversible]);
        }
    }

    #[test]
    fn two_files_with_one_id_and_different_bytes_are_no_publication() {
        let corpus = corpus_cut_short(false, b"edited");
        assert!(publication_steps(&corpus).is_empty());
    }

    // --- import -----------------------------------------------------------------------

    #[test]
    fn a_file_with_no_id_and_a_design_with_an_item_with_no_id_are_imported() {
        let corpus = Corpus::build(
            [
                (
                    public_facts("note.md", b"x"),
                    Reading::Unidentified {
                        kind: Some(DocKind::Finding),
                    },
                ),
                (
                    public_facts("design/a.md", b"d"),
                    design_document(
                        2,
                        vec![
                            (1, identified(40, "has an id", &["PIN-A"])),
                            (2, item("has none", &[])),
                        ],
                    ),
                ),
                (
                    public_facts("design/b.md", b"e"),
                    design_document(3, vec![(1, identified(41, "has an id", &["PIN-A"]))]),
                ),
            ],
            &BTreeSet::new(),
        );
        let steps = import_steps(&corpus);
        assert_eq!(steps.len(), 2);
        assert!(steps.iter().all(|s| s.action() == &NextAction::Import));
        assert!(
            matches!(steps[0].target(), EntityRef::File(location) if location.path().as_str() == "note.md")
        );
        assert_eq!(steps[1].target(), &doc_target(2));
        assert_eq!(steps[1].needs(), &[]);
    }

    // --- the shape of a step ----------------------------------------------------------

    #[test]
    fn a_choice_has_at_least_two_steps_and_none_is_a_choice() {
        let mut choices =
            vec![next_for_presence(Target::Doc(docid(1)), Presence::Missing).expect("a choice")];
        choices.push(publication_step(
            &corpus_cut_short(false, b"same").publishing()[0],
        ));
        for choice in choices {
            let NextStep::Choose(steps) = choice else {
                panic!("a choice");
            };
            assert!(steps.len() >= 2);
            // The type of a member is DoStep: a choice cannot hold a choice.
            for step in steps.iter() {
                let _: &DoStep = step;
            }
        }
    }

    #[test]
    fn the_needs_of_each_action() {
        let table = [
            (
                NextAction::Approve(Gate::Cr),
                vec![Need::Judgement(Gate::Cr)],
            ),
            (
                NextAction::Reapprove(Gate::Ci),
                vec![Need::Judgement(Gate::Ci)],
            ),
            (
                NextAction::Approve(Gate::Ca),
                vec![Need::Judgement(Gate::Ca), Need::CleanTree, Need::TestsPass],
            ),
            (NextAction::Measure, vec![Need::Measurement]),
            (NextAction::Adopt, vec![Need::HumanConfirmation]),
            (NextAction::Import, vec![]),
            (NextAction::Retire, vec![]),
            (
                NextAction::ResumePublish,
                vec![Need::ConfirmationIfIrreversible],
            ),
            (
                NextAction::AbortPublish,
                vec![Need::ConfirmationIfIrreversible],
            ),
            (NextAction::Edit(EditFor::Restore), vec![]),
        ];
        for (action, needs) in table {
            assert_eq!(action.needs(), needs, "{}", kind(&action));
        }
        let step = DoStep::new(NextAction::Retire, doc_target(1))
            .needing(Need::HumanConfirmation)
            .needing(Need::HumanConfirmation);
        assert_eq!(step.needs(), &[Need::HumanConfirmation], "said once");
    }
}
