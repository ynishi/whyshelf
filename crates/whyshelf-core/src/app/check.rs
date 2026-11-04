//! The gates CR, CS, CI, CA and CM, and `revoke`.
//!
//! Each gate has two calls. `check_*` is the machine layer: it reads, runs what must
//! be run (the tests, for CA), and returns every violation it finds in a
//! [`CheckReport`]. `approve_*` is the human layer: it runs the same check and, only
//! when nothing is found, records an `Approved` event.
//!
//! The check itself is the domain's ([`gate`]): this module collects what the check
//! reads from the model and the ports into the gate's input, and calls it. What the
//! check returns when nothing is found, a [`Passed`], is the only thing an approval
//! can be recorded from, so no path records an approval over a violation. Which hash
//! an approval pins, and what it records of the upstream, is the attestation the
//! check builds (see [`approval`](crate::domain::approval)).
//!
//! An Invalid target is a violation of every gate (the measurement included), shown
//! like any other and with the same exit status; it is not an error of the tool.
//!
//! Two rules are worth stating again here:
//!
//! - CA records the **revision the tests and the pin check ran at**, and refuses a
//!   working tree that is not clean.
//! - CM's failure is a choice of two (see [`CmInput`]), and each carries what the
//!   follow-up needs, so a failure cannot be recorded without saying which kind.

use std::collections::BTreeMap;

use crate::domain::actor::Actor;
use crate::domain::approval::{Attestation, CmResult, Gate};
use crate::domain::evaluate::Evaluated;
use crate::domain::event::{EventId, RecordBody};
use crate::domain::gate::{
    self, CaInput, Checked, CiInput, CrInput, CsInput, Passed, Violations, ca, ci, cm, cr, cs,
    invalid_target,
};
use crate::domain::id::{DesignId, EntityId, FindingId, SpecId};
use crate::domain::lint::CheckViolation;
use crate::domain::nonempty::NonBlank;
use crate::domain::reference::{ItemRef, Ref};
use crate::domain::spec::DeclaredPins;

use super::Workspace;
use super::commit::Committed;
use super::error::AppError;
use super::model::{Model, impl_view_at};
use super::ports::{PortName, Ports, RevisionPort, TestRunPort};
use super::raw::RawRevision;

/// What a machine check found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CheckReport {
    gate: Gate,
    target: EntityId,
    violations: Vec<CheckViolation>,
}

impl CheckReport {
    /// A report of what a check of `gate` found on `target`; no violation is a pass.
    pub fn new(gate: Gate, target: EntityId, violations: Vec<CheckViolation>) -> Self {
        Self {
            gate,
            target,
            violations,
        }
    }

    /// The gate.
    pub fn gate(&self) -> Gate {
        self.gate
    }

    /// The entity checked.
    pub fn target(&self) -> EntityId {
        self.target
    }

    /// Everything found; empty when the check passes.
    pub fn violations(&self) -> &[CheckViolation] {
        &self.violations
    }

    /// Whether an approval may be recorded.
    pub fn passed(&self) -> bool {
        self.violations.is_empty()
    }
}

/// What was recorded by an approval.
#[derive(Debug)]
pub struct Approved {
    event: EventId,
    attestation: Attestation,
    committed: Committed,
}

impl Approved {
    /// The `Approved` event, which `revoke` takes.
    pub fn event(&self) -> EventId {
        self.event
    }

    /// What was recorded: the entity, the hashes, and for a CA the revision. This,
    /// and not the revision the working tree is at now, is what the approval says.
    pub fn attestation(&self) -> &Attestation {
        &self.attestation
    }

    /// The write and its subscribers' warnings.
    pub fn committed(&self) -> &Committed {
        &self.committed
    }
}

/// The result a person reads from a measurement.
///
/// Exactly one of three, and a failure names what the follow-up needs: a wiring
/// gap says which item seems to be missing (add it to the Design); a wrong
/// hypothesis names the new `measured_data` Finding that records the measurement
/// (revise the Spec).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CmInput {
    /// The measurement met the hypotheses.
    Pass,
    /// A wiring or implementation gap.
    FailWiring {
        /// A hint at the missing item, free text.
        item_hint: NonBlank,
    },
    /// The hypothesis was wrong.
    FailHyp {
        /// The Finding that records the measurement.
        finding: Ref,
    },
}

/// A check of a gate: which, on what, and what it came to.
struct Prepared {
    gate: Gate,
    target: EntityId,
    checked: Checked,
}

impl Prepared {
    fn report(&self) -> CheckReport {
        CheckReport {
            gate: self.gate,
            target: self.target,
            violations: match &self.checked {
                Ok(_) => Vec::new(),
                Err(violations) => violations.clone().into_vec(),
            },
        }
    }
}

/// What the check of a target that has no evaluation is: the target is a document in
/// conflict, which is Invalid, or its file is missing.
fn without_evaluation(model: &Model, gate: Gate, id: EntityId) -> Result<Prepared, AppError> {
    match model
        .conflicted_doc(id)
        .and_then(|entry| invalid_target(entry.doc().state()))
    {
        Some(violations) => Ok(Prepared {
            gate,
            target: id,
            checked: Err(violations),
        }),
        None => Err(AppError::TargetFileMissing { id }),
    }
}

fn prepare_cr(model: &Model, target: &Ref) -> Result<Prepared, AppError> {
    let gate = Gate::Cr;
    let id = model.resolve_as(target, gate.target_kind())?;
    let Some(finding) = model.findings().get(&FindingId::new(id)) else {
        return without_evaluation(model, gate, id);
    };
    Ok(Prepared {
        gate,
        target: id,
        checked: cr::check(CrInput { finding }),
    })
}

fn prepare_cs(model: &Model, target: &Ref) -> Result<Prepared, AppError> {
    let gate = Gate::Cs;
    let id = model.resolve_as(target, gate.target_kind())?;
    let Some(spec) = model.specs().get(&SpecId::new(id)) else {
        return without_evaluation(model, gate, id);
    };
    let states = model.states();
    Ok(Prepared {
        gate,
        target: id,
        checked: cs::check(CsInput {
            spec,
            grounds: model.findings().iter().map(|(id, ev)| (*id, ev)).collect(),
            states: &states,
        }),
    })
}

fn prepare_ci(model: &Model, target: &Ref) -> Result<Prepared, AppError> {
    let gate = Gate::Ci;
    let id = model.resolve_as(target, gate.target_kind())?;
    let Some(design) = model.designs().get(&DesignId::new(id)) else {
        return without_evaluation(model, gate, id);
    };
    let states = model.states();
    Ok(Prepared {
        gate,
        target: id,
        checked: ci::check(CiInput {
            design,
            specs: model.specs().iter().map(|(id, ev)| (*id, ev)).collect(),
            states: &states,
        }),
    })
}

fn prepare_cm(model: &Model, target: &Ref, input: &CmInput) -> Result<Prepared, AppError> {
    let gate = Gate::Cm;
    let id = model.resolve_as(target, gate.target_kind())?;
    let Some(spec) = model.specs().get(&SpecId::new(id)) else {
        return without_evaluation(model, gate, id);
    };
    let result = match input {
        CmInput::Pass => CmResult::Pass,
        CmInput::FailWiring { item_hint } => CmResult::FailWiring {
            item_hint: item_hint.clone(),
        },
        CmInput::FailHyp { finding } => CmResult::FailHyp {
            finding: FindingId::new(model.resolve_as(finding, Gate::Cr.target_kind())?),
        },
    };
    let hyp_finding = match &result {
        CmResult::FailHyp { finding } => model.findings().get(finding),
        CmResult::Pass | CmResult::FailWiring { .. } => None,
    };
    Ok(Prepared {
        gate,
        target: id,
        checked: cm::check(gate::CmInput {
            spec,
            ledger: model.ledger(),
            result,
            hyp_finding,
        }),
    })
}

impl<P: Ports> Workspace<P> {
    async fn prepare_ca(
        &self,
        model: &Model,
        target: &ItemRef,
        revision: Option<&RawRevision>,
    ) -> Result<Prepared, AppError> {
        let item_id = model.resolve_item(target)?;
        let id = item_id.entity();
        let item = model.item(item_id).ok_or(AppError::ItemNotFound {
            item: target.clone(),
        })?;
        let revision = match revision {
            None => self
                .ports
                .revision()
                .current()
                .await
                .map_err(AppError::port(PortName::Revision))?,
            Some(text) => self
                .ports
                .revision()
                .validate(text)
                .await
                .map_err(AppError::port(PortName::Revision))?
                .ok_or_else(|| AppError::RevisionRejected {
                    text: text.as_str().to_owned(),
                })?,
        };
        // The pins and the tests are read at the revision that is recorded.
        let impl_view =
            impl_view_at(&self.ports, &self.config, revision.clone(), model.ledger()).await?;
        let tests = self
            .ports
            .test_run()
            .run(
                &revision,
                self.config.test().command(),
                self.config.test().timeout(),
            )
            .await
            .map_err(AppError::port(PortName::TestRun))?;
        let states = model.states();
        let designs: BTreeMap<_, _> = model.designs().iter().map(|(id, ev)| (*id, ev)).collect();
        // A tag in a test is undeclared when no Spec of the project declares it.
        let declared = DeclaredPins::of(model.specs().values().filter_map(Evaluated::entity));
        Ok(Prepared {
            gate: Gate::Ca,
            target: id,
            checked: ca::check(CaInput {
                key: item.key(),
                item: item.item(),
                evaluation: item.evaluation(),
                designs,
                declared: &declared,
                states: &states,
                impl_view: &impl_view,
                revision,
                tests,
            }),
        })
    }

    /// Record an approval, over a check that passed.
    async fn approve_checked(
        &self,
        model: &Model,
        prepared: Prepared,
        by: Option<Actor>,
    ) -> Result<Approved, AppError> {
        let by = by
            .or_else(|| self.config.user().cloned())
            .ok_or(AppError::NoActor)?;
        let passed: Passed = match prepared.checked {
            Ok(passed) => passed,
            Err(violations) => return Err(check_failed(prepared.gate, violations)),
        };
        let store = model.home(prepared.target).ok_or(AppError::NotFound {
            reference: Ref::Local(crate::domain::reference::LocalRef::Id(prepared.target)),
        })?;
        let mut draft = self.draft().await?;
        let event = draft.push_approval(model, store, &passed, by)?;
        let committed = self.commit(model, draft).await?;
        Ok(Approved {
            event,
            attestation: passed.attestation().clone(),
            committed,
        })
    }

    /// CR, machine layer.
    pub async fn check_cr(&self, finding: &Ref) -> Result<CheckReport, AppError> {
        let model = Model::load(&self.ports, &self.config).await?;
        Ok(prepare_cr(&model, finding)?.report())
    }

    /// CR, human layer: record the approval if the check passes.
    pub async fn approve_cr(
        &mut self,
        finding: &Ref,
        by: Option<Actor>,
    ) -> Result<Approved, AppError> {
        let model = Model::load(&self.ports, &self.config).await?;
        let prepared = prepare_cr(&model, finding)?;
        self.approve_checked(&model, prepared, by).await
    }

    /// CS, machine layer.
    pub async fn check_cs(&self, spec: &Ref) -> Result<CheckReport, AppError> {
        let model = Model::load(&self.ports, &self.config).await?;
        Ok(prepare_cs(&model, spec)?.report())
    }

    /// CS, human layer.
    pub async fn approve_cs(
        &mut self,
        spec: &Ref,
        by: Option<Actor>,
    ) -> Result<Approved, AppError> {
        let model = Model::load(&self.ports, &self.config).await?;
        let prepared = prepare_cs(&model, spec)?;
        self.approve_checked(&model, prepared, by).await
    }

    /// CI, machine layer.
    pub async fn check_ci(&self, design: &Ref) -> Result<CheckReport, AppError> {
        let model = Model::load(&self.ports, &self.config).await?;
        Ok(prepare_ci(&model, design)?.report())
    }

    /// CI, human layer.
    pub async fn approve_ci(
        &mut self,
        design: &Ref,
        by: Option<Actor>,
    ) -> Result<Approved, AppError> {
        let model = Model::load(&self.ports, &self.config).await?;
        let prepared = prepare_ci(&model, design)?;
        self.approve_checked(&model, prepared, by).await
    }

    /// CA, machine layer: the pins in both directions and the tests, at the
    /// given revision or the current one.
    pub async fn check_ca(
        &self,
        item: &ItemRef,
        revision: Option<&RawRevision>,
    ) -> Result<CheckReport, AppError> {
        let model = Model::load(&self.ports, &self.config).await?;
        Ok(self.prepare_ca(&model, item, revision).await?.report())
    }

    /// CA, human layer. Refuses a working tree that is not clean ("commit
    /// first"); there is no way around it. Records the revision the check ran
    /// at.
    pub async fn approve_ca(
        &mut self,
        item: &ItemRef,
        revision: Option<&RawRevision>,
        by: Option<Actor>,
    ) -> Result<Approved, AppError> {
        let clean = self
            .ports
            .revision()
            .is_clean()
            .await
            .map_err(AppError::port(PortName::Revision))?;
        if !clean {
            return Err(AppError::DirtyWorktree);
        }
        let model = Model::load(&self.ports, &self.config).await?;
        let prepared = self.prepare_ca(&model, item, revision).await?;
        self.approve_checked(&model, prepared, by).await
    }

    /// CM: record what a measurement showed, against the hypotheses the CS
    /// approval fixed.
    pub async fn record_cm(
        &mut self,
        spec: &Ref,
        result: &CmInput,
        by: Option<Actor>,
    ) -> Result<Approved, AppError> {
        let model = Model::load(&self.ports, &self.config).await?;
        let prepared = prepare_cm(&model, spec, result)?;
        self.approve_checked(&model, prepared, by).await
    }

    /// Withdraw an approval, with the reason. The approval stays in the log; the
    /// set of approvals no longer counts it.
    pub async fn revoke(
        &mut self,
        approval: EventId,
        reason: NonBlank,
        by: Option<Actor>,
    ) -> Result<Committed, AppError> {
        let by = by
            .or_else(|| self.config.user().cloned())
            .ok_or(AppError::NoActor)?;
        let model = Model::load(&self.ports, &self.config).await?;
        let record = model
            .ledger()
            .record(&approval)
            .ok_or(AppError::NotAnApproval { event: approval })?;
        if model.ledger().is_revoked(&approval) {
            return Err(AppError::AlreadyRevoked { event: approval });
        }
        // The revocation goes where the approval is, so each store is complete
        // about the approvals it holds.
        let store = model
            .store_of_event(&approval)
            .ok_or(AppError::NotAnApproval { event: approval })?;
        let mut draft = self.draft().await?;
        draft.push(
            &model,
            store,
            record.subject(),
            RecordBody::Revoked {
                revokes: approval,
                reason,
                by,
            },
        )?;
        self.commit(&model, draft).await
    }
}

fn check_failed(gate: Gate, violations: Violations) -> AppError {
    AppError::CheckFailed {
        gate,
        violations: violations.into_vec(),
    }
}
