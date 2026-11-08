//! The gates CR, CS, CI, CA and CM: `check` (the machine layer) and `approve` (the
//! person's judgement on top of it).
//!
//! The check is the domain's ([`gate`]): this module collects what a gate reads from
//! the snapshot and the ports into the gate's input and calls it. What a check returns
//! when it finds nothing, a [`Passed`], is the only thing an approval can be made from,
//! so no path records an approval over a violation.
//!
//! ```text
//!   Snapshot --- named(target) --> Named        (outcome_of: not found / cannot name here)
//!       |
//!       +--- CR CS CI CM: the evaluated entity, the states, the text hash it was read from
//!       +--- CA: work_tree (approve only) -> Dirty: DirtyTree, Unknown: WorkTreeUnknown
//!                change_fingerprint, code scan, test run at the revision
//!       v
//!   gate::{cr, cs, ci, ca, cm}::check*  ----- violations --> Outcome::Violations
//!       | Passed
//!   check:   Done(CheckPassed)                         (nothing is written)
//!   approve: Judgement::approve (ledger: JudgementKind::ledger) -> split -> scan -> store -> append
//! ```
//!
//! # The one mapping of the work tree
//!
//! A CA approval refuses a tree that is not clean, and says so in the one place
//! `clean_tree` below: [`WorkTree::Dirty`] is [`Refusal::DirtyTree`] (commit first), and
//! a tree whose state cannot be told here is [`Refusal::WorkTreeUnknown`], which does not
//! tell the person to commit when nothing says there is anything to commit. The
//! machine check alone does not need a clean tree; the approval records the revision
//! the check ran at, so it must be one that is committed.

use std::collections::BTreeMap;

use crate::app::config::Config;
use crate::app::corpus::flow::{Flow, finish};
use crate::app::corpus::name::home_of;
use crate::app::corpus::snapshot::Snapshot;
use crate::app::corpus::write::{Mint, not_publishing, record_with_object};
use crate::app::error::AppError;
use crate::app::outcome::{Outcome, Refusal};
use crate::app::ports::{
    CodeScanPort, CorpusPorts, CorpusRead, PinScan, PortName, RevisionFactsPort, TestRunPort,
    WorkTree,
};
use crate::domain::approval::{CmResult, Gate};
use crate::domain::evaluate::{Evaluated, ImplView, ItemKey};
use crate::domain::finding::Finding;
use crate::domain::gate::{
    self, CaInput, CiInput, CrInput, CsInput, FingerprintAsked, Passed, ca, ci, cm, cr, cs,
    invalid_target,
};
use crate::domain::hash::TextHash;
use crate::domain::id::{DesignId, DocId, FindingId, SpecId};
use crate::domain::judgement::{Judgement, JudgementId, JudgementKind, ObjectKey, Target};
use crate::domain::knowable::Knowable;
use crate::domain::ledger::CsFixing;
use crate::domain::naming::{Named, Ref};
use crate::domain::nonempty::NonBlank;
use crate::domain::revision::{Revision, RevisionId};
use crate::domain::spec::DeclaredPins;

/// A gate to run, with what only that gate needs beyond the target.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GateCommand {
    /// CR: the Finding is complete as a record.
    Cr,
    /// CS: the Contract and its acceptance declaration.
    Cs,
    /// CI: the split into items and the assignment of pins.
    Ci,
    /// CA: the implementation of an item. The tests and the pin scan run at
    /// `revision`, and the approval records it.
    Ca {
        /// The revision the tests and the pin scan run at. No port of the corpus says
        /// which revision the tree is at, so the caller names it.
        revision: RevisionId,
    },
    /// CM: what a measurement of the hypotheses showed.
    Cm {
        /// The result.
        result: Measured,
    },
}

impl GateCommand {
    /// The gate this runs.
    pub fn gate(&self) -> Gate {
        match self {
            Self::Cr => Gate::Cr,
            Self::Cs => Gate::Cs,
            Self::Ci => Gate::Ci,
            Self::Ca { .. } => Gate::Ca,
            Self::Cm { .. } => Gate::Cm,
        }
    }
}

/// What a measurement showed, with the Finding it names written as a person gives it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Measured {
    /// The measurement met the hypotheses.
    Pass,
    /// A wiring or implementation gap.
    FailWiring {
        /// A hint at the missing item, free text.
        item_hint: NonBlank,
    },
    /// The hypothesis was wrong; the Finding records the measurement.
    FailHyp {
        /// The Finding.
        finding: Ref,
    },
}

/// Run a gate and approve: the command of [`approve`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Approve {
    /// What is approved, as a person names it.
    pub target: Ref,
    /// The gate, with what only it needs.
    pub gate: GateCommand,
}

/// Run a gate's machine check and record nothing: the command of [`check`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Check {
    /// What is checked, as a person names it.
    pub target: Ref,
    /// The gate, with what only it needs.
    pub gate: GateCommand,
}

/// What an approval wrote.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Approved {
    id: JudgementId,
    gate: Gate,
    target: Target,
    ledger: crate::domain::corpus::Visibility,
}

impl Approved {
    /// The record, which a revocation names.
    pub fn id(&self) -> JudgementId {
        self.id
    }

    /// The gate that let it through.
    pub fn gate(&self) -> Gate {
        self.gate
    }

    /// The document or item approved.
    pub fn target(&self) -> Target {
        self.target
    }

    /// The ledger the record was written to.
    pub fn ledger(&self) -> crate::domain::corpus::Visibility {
        self.ledger
    }
}

/// What a check that found nothing was about.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CheckPassed {
    gate: Gate,
    target: Target,
}

impl CheckPassed {
    /// The gate that found nothing.
    pub fn gate(&self) -> Gate {
        self.gate
    }

    /// The document or item checked.
    pub fn target(&self) -> Target {
        self.target
    }
}

/// Run the gate's machine check on the target. Nothing is written: this takes the
/// read-only ports, so there is no port to write with.
///
/// What the gate finds is [`Outcome::Violations`]; a check that finds nothing is
/// [`Outcome::Done`].
pub async fn check<P: CorpusRead>(
    ports: &P,
    config: &Config,
    command: Check,
) -> Result<Outcome<CheckPassed>, AppError> {
    finish(check_flow(ports, config, command).await)
}

async fn check_flow<P: CorpusRead>(
    ports: &P,
    config: &Config,
    command: Check,
) -> Result<CheckPassed, Flow> {
    let snapshot = Snapshot::read(ports, config.gate_policy()).await?;
    let named = snapshot.named(&command.target)?;
    let passed = gate_passed(ports, config, &snapshot, &command.gate, named).await?;
    Ok(CheckPassed {
        gate: passed.gate(),
        target: Target::of_subject(passed.gate(), passed.subject()),
    })
}

/// Run the gate on the target and, only when it finds nothing, record the approval.
///
/// ```text
///   CA: the working tree must be clean (a person commits first)
///   a document whose publication is cut short takes no new judgement
///   check -> Passed -> Judgement::approve -> split -> scan -> store -> append
/// ```
pub async fn approve<P: CorpusPorts>(
    ports: &P,
    config: &Config,
    command: Approve,
) -> Result<Outcome<Approved>, AppError> {
    finish(approve_flow(ports, config, command).await)
}

async fn approve_flow<P: CorpusPorts>(
    ports: &P,
    config: &Config,
    command: Approve,
) -> Result<Approved, Flow> {
    let snapshot = Snapshot::read(ports, config.gate_policy()).await?;
    let named = snapshot.named(&command.target)?;
    not_publishing(&snapshot, home_of(&named))?;
    if matches!(command.gate, GateCommand::Ca { .. }) {
        clean_tree(ports).await?;
    }
    let passed = gate_passed(ports, config, &snapshot, &command.gate, named).await?;

    let mint = Mint::take(ports).await?;
    let object = ObjectKey::new(*passed.attestation().target_hash(), *passed.text());
    let kind = JudgementKind::Approve {
        passed: passed.clone(),
        object,
    };
    let origin = kind.ledger(snapshot.corpus(), snapshot.ledgers())?;
    let judgement =
        Judgement::approve(mint.id, mint.at, mint.judge, origin, passed.clone(), object)?;
    record_with_object(ports, &snapshot, judgement).await?;
    Ok(Approved {
        id: mint.id,
        gate: passed.gate(),
        target: Target::of_subject(passed.gate(), passed.subject()),
        ledger: origin,
    })
}

/// The one place the state of the working tree is read for an approval of CA. See the
/// module documentation.
async fn clean_tree<P: CorpusRead>(ports: &P) -> Result<(), Flow> {
    match ports
        .facts()
        .work_tree()
        .await
        .map_err(AppError::port(PortName::RevisionFacts))?
    {
        Knowable::Known(WorkTree::Clean) => Ok(()),
        Knowable::Known(WorkTree::Dirty) => Err(Refusal::DirtyTree.into()),
        Knowable::Unknown => Err(Refusal::WorkTreeUnknown.into()),
    }
}

/// The gate's check on the named thing: what it finds, or the proof that it found
/// nothing.
async fn gate_passed<P: CorpusRead>(
    ports: &P,
    config: &Config,
    snapshot: &Snapshot,
    command: &GateCommand,
    named: Named,
) -> Result<Passed, Flow> {
    match command {
        GateCommand::Cr => cr_passed(snapshot, doc_for(Gate::Cr, &named)?),
        GateCommand::Cs => cs_passed(snapshot, doc_for(Gate::Cs, &named)?),
        GateCommand::Ci => ci_passed(snapshot, doc_for(Gate::Ci, &named)?),
        GateCommand::Ca { revision } => ca_passed(ports, config, snapshot, &named, revision).await,
        GateCommand::Cm { result } => cm_passed(snapshot, doc_for(Gate::Cm, &named)?, result),
    }
}

/// The document a document gate judges, or the refusal that the name points at an
/// item.
fn doc_for(gate: Gate, named: &Named) -> Result<DocId, Flow> {
    match named {
        Named::Doc(doc) => Ok(*doc),
        Named::Item(item) => Err(Refusal::WrongTarget {
            gate,
            target: Target::Item(item.id()),
        }
        .into()),
    }
}

/// Why a document has no evaluation of the kind the gate judges: it is a document that
/// cannot be judged (Invalid, with its reasons), or it is of another kind.
fn not_judged(snapshot: &Snapshot, gate: Gate, doc: DocId) -> Flow {
    let states = snapshot.evaluation().states();
    match states
        .get(&doc.entity())
        .and_then(|state| invalid_target(state))
    {
        Some(violations) => violations.into(),
        None => Refusal::WrongTarget {
            gate,
            target: Target::Doc(doc),
        }
        .into(),
    }
}

/// The hash of the text of the first file that carries `doc`: the version the
/// evaluation was made of.
fn text_of(snapshot: &Snapshot, gate: Gate, doc: DocId) -> Result<TextHash, Flow> {
    snapshot
        .corpus()
        .by_id(&doc)
        .first()
        .map(|file| file.text())
        .ok_or_else(|| not_judged(snapshot, gate, doc))
}

fn cr_passed(snapshot: &Snapshot, doc: DocId) -> Result<Passed, Flow> {
    let finding: &Evaluated<Finding> = snapshot
        .evaluation()
        .findings()
        .get(&FindingId::new(doc.entity()))
        .ok_or_else(|| not_judged(snapshot, Gate::Cr, doc))?;
    let text = text_of(snapshot, Gate::Cr, doc)?;
    Ok(cr::check(CrInput { finding, text })?)
}

fn cs_passed(snapshot: &Snapshot, doc: DocId) -> Result<Passed, Flow> {
    let evaluation = snapshot.evaluation();
    let spec = evaluation
        .specs()
        .get(&SpecId::new(doc.entity()))
        .ok_or_else(|| not_judged(snapshot, Gate::Cs, doc))?;
    let states = evaluation.states();
    let input = CsInput {
        spec,
        grounds: evaluation
            .findings()
            .iter()
            .map(|(id, found)| (*id, found))
            .collect(),
        states: &states,
        policy: snapshot.policy(),
        text: text_of(snapshot, Gate::Cs, doc)?,
    };
    Ok(cs::check_in(input, snapshot.pins())?)
}

fn ci_passed(snapshot: &Snapshot, doc: DocId) -> Result<Passed, Flow> {
    let evaluation = snapshot.evaluation();
    let design = evaluation
        .designs()
        .get(&DesignId::new(doc.entity()))
        .ok_or_else(|| not_judged(snapshot, Gate::Ci, doc))?;
    let states = evaluation.states();
    let input = CiInput {
        design,
        specs: evaluation
            .specs()
            .iter()
            .map(|(id, spec)| (*id, spec))
            .collect(),
        states: &states,
        text: text_of(snapshot, Gate::Ci, doc)?,
    };
    Ok(ci::check_in(input, snapshot.pins())?)
}

fn cm_passed(snapshot: &Snapshot, doc: DocId, measured: &Measured) -> Result<Passed, Flow> {
    let evaluation = snapshot.evaluation();
    let spec = evaluation
        .specs()
        .get(&SpecId::new(doc.entity()))
        .ok_or_else(|| not_judged(snapshot, Gate::Cm, doc))?;
    let result = match measured {
        Measured::Pass => CmResult::Pass,
        Measured::FailWiring { item_hint } => CmResult::FailWiring {
            item_hint: item_hint.clone(),
        },
        Measured::FailHyp { finding } => {
            let found = snapshot.named(finding)?;
            CmResult::FailHyp {
                finding: FindingId::new(doc_for(Gate::Cr, &found)?.entity()),
            }
        }
    };
    let hyp_finding = match &result {
        CmResult::FailHyp { finding } => evaluation.findings().get(finding),
        CmResult::Pass | CmResult::FailWiring { .. } => None,
    };
    // A Spec that is not an entity (it cannot be read) is stopped by the check as
    // Invalid before the approval it is set against is looked at.
    let fixing = match spec.entity() {
        Some(entity) => snapshot.ledgers().cs_fixing(entity.id(), entity.hyp_hash()),
        None => CsFixing::Missing,
    };
    let input = gate::CmInput {
        spec,
        fixing,
        result,
        hyp_finding,
        text: text_of(snapshot, Gate::Cm, doc)?,
    };
    Ok(cm::check(input)?)
}

async fn ca_passed<P: CorpusRead>(
    ports: &P,
    config: &Config,
    snapshot: &Snapshot,
    named: &Named,
    revision: &RevisionId,
) -> Result<Passed, Flow> {
    let item = match named {
        Named::Item(item) => item,
        Named::Doc(doc) => {
            return Err(Refusal::WrongTarget {
                gate: Gate::Ca,
                target: Target::Doc(*doc),
            }
            .into());
        }
    };
    let evaluation = snapshot.evaluation();
    let wrong = || Refusal::WrongTarget {
        gate: Gate::Ca,
        target: Target::Item(item.id()),
    };
    let design_id = DesignId::new(item.design().entity());
    let design = evaluation
        .designs()
        .get(&design_id)
        .and_then(Evaluated::entity)
        .ok_or_else(wrong)?;
    let work_item = design.items().get(&item.number()).ok_or_else(wrong)?;
    let report = evaluation.item(&item.id()).ok_or_else(wrong)?;
    let key = ItemKey {
        id: item.id(),
        design: design_id,
        number: item.number(),
    };

    let fingerprint = ports
        .facts()
        .change_fingerprint(&Revision::Commit(revision.clone()))
        .await
        .map_err(AppError::port(PortName::RevisionFacts))?;
    // No scan directory and no `test.files` leave no file that could carry a pin.
    let scan = match config.test_files() {
        Some(files) => ports
            .scanner()
            .pins_at(
                revision,
                config.paths().scan_dirs(),
                &files,
                config.test().pin_search(),
            )
            .await
            .map_err(AppError::port(PortName::CodeScan))?,
        None => PinScan::default(),
    };
    let tests = ports
        .runner()
        .run(revision, config.test().command(), config.test().timeout())
        .await
        .map_err(AppError::port(PortName::TestRun))?;
    let impl_view = ImplView::new(revision.clone(), None, scan.pins, BTreeMap::new())
        .with_malformed(scan.malformed);
    // A tag in a test is undeclared when no Spec of the project declares it.
    let declared = DeclaredPins::of(evaluation.specs().values().filter_map(Evaluated::entity));
    let states = evaluation.states();
    let input = CaInput {
        key: &key,
        item: work_item,
        state: report.state(),
        designs: evaluation
            .designs()
            .iter()
            .map(|(id, design)| (*id, design))
            .collect(),
        declared: &declared,
        states: &states,
        impl_view: &impl_view,
        revision: revision.clone(),
        tests,
        fingerprint: FingerprintAsked::Asked(fingerprint),
        // The item is read from its Design's file.
        text: text_of(snapshot, Gate::Ca, item.design())?,
    };
    Ok(ca::check_in(input, snapshot.pins())?)
}

#[cfg(test)]
mod tests;
