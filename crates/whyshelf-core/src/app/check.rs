//! The gates CR, CS, CI, CA and CM, and `revoke`.
//!
//! Each gate has two calls. `check_*` is the machine layer: it reads, runs what must
//! be run (the tests, for CA), and returns every violation it finds in a
//! [`CheckReport`]. `approve_*` is the human layer: it runs the same check and, only
//! when nothing is found, records an `Approved` event. The gate in
//! [`commit`](super::commit) refuses the event again if the check had found anything,
//! so no path records an approval over a violation.
//!
//! What an approval pins, and what it records of the upstream, is fixed by
//! [`GateBinding`](crate::domain::approval::GateBinding) in the domain; this module
//! only gathers the hashes it names.
//!
//! | gate | the attestation records |
//! |---|---|
//! | CR | the Finding at its hash |
//! | CS | the Spec at its contract hash, the hypothesis hash it fixes, every ground at the hash it has |
//! | CI | the Design at its hash, the Spec at its contract hash |
//! | CA | the item at its hash, the Design at its hash, the revision, the outcome of the tests |
//! | CM | the Spec at its hypothesis hash, the CS approval that fixed it, the result |
//!
//! Two rules are worth stating again here:
//!
//! - CA records the **revision the tests and the pin check ran at**, and refuses a
//!   working tree that is not clean.
//! - CM's failure is a choice of two (see [`CmInput`]), and each carries what the
//!   follow-up needs, so a failure cannot be recorded without saying which kind.

use crate::domain::actor::Actor;
use crate::domain::approval::{Attestation, CmResult, Gate, Pinned};
use crate::domain::evaluate::Evaluated;
use crate::domain::event::{EntityKind, EventBody, EventId};
use crate::domain::finding::FindingSource;
use crate::domain::id::{DesignId, EntityId, FindingId, SpecId};
use crate::domain::lint::{
    CheckViolation, UpstreamProblem, check_ca, check_ci, check_cm, check_cr, check_cs,
};
use crate::domain::nonempty::{NonBlank, NonEmptyVec};
use crate::domain::reference::{ItemRef, Ref};
use crate::domain::revision::Revision;
use crate::domain::spec::Spec;
use crate::domain::state::Approval;

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
    committed: Committed,
}

impl Approved {
    /// The `Approved` event, which `revoke` takes.
    pub fn event(&self) -> EventId {
        self.event
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

/// A check and, when it passed, the attestation it would record.
struct Checked {
    gate: Gate,
    target: EntityId,
    violations: Vec<CheckViolation>,
    attestation: Option<Attestation>,
}

impl Checked {
    fn report(&self) -> CheckReport {
        CheckReport {
            gate: self.gate,
            target: self.target,
            violations: self.violations.clone(),
        }
    }
}

fn ensure_valid(id: EntityId, approval: &Approval) -> Result<(), AppError> {
    match approval {
        Approval::Invalid(reasons) => Err(AppError::TargetInvalid {
            id,
            reasons: reasons.clone(),
        }),
        _ => Ok(()),
    }
}

fn prepare_cr(model: &Model, target: &Ref) -> Result<Checked, AppError> {
    let id = model.resolve_as(target, EntityKind::Finding)?;
    let ev = model
        .findings()
        .get(&FindingId::new(id))
        .ok_or(AppError::TargetFileMissing { id })?;
    ensure_valid(id, ev.state().approval())?;
    let finding = ev.entity().ok_or(AppError::TargetUnparsed { id })?;
    let violations = check_cr(finding);
    let attestation = violations.is_empty().then(|| Attestation::Cr {
        finding: Pinned::new(finding.id(), *finding.hash()),
    });
    Ok(Checked {
        gate: Gate::Cr,
        target: id,
        violations,
        attestation,
    })
}

/// The grounds of a Spec at the hashes they have, or the grounds that cannot be
/// pinned (not there, or not parsed).
fn pin_grounds(
    model: &Model,
    spec: &Spec,
) -> Result<NonEmptyVec<Pinned<FindingId>>, Vec<EntityId>> {
    let mut pinned = Vec::new();
    let mut absent = Vec::new();
    for ground in spec.grounds().iter() {
        match model.findings().get(ground).and_then(Evaluated::entity) {
            Some(finding) => pinned.push(Pinned::new(*ground, *finding.hash())),
            None => absent.push(ground.entity()),
        }
    }
    match (absent.is_empty(), NonEmptyVec::from_vec(pinned)) {
        (true, Ok(pinned)) => Ok(pinned),
        _ => Err(absent),
    }
}

fn prepare_cs(model: &Model, target: &Ref) -> Result<Checked, AppError> {
    let id = model.resolve_as(target, EntityKind::Spec)?;
    let ev = model
        .specs()
        .get(&SpecId::new(id))
        .ok_or(AppError::TargetFileMissing { id })?;
    ensure_valid(id, ev.state().approval())?;
    let spec = ev.entity().ok_or(AppError::TargetUnparsed { id })?;
    let mut violations = check_cs(spec, ev.state(), model.findings());
    let pinned = pin_grounds(model, spec);
    if let Err(absent) = &pinned {
        // An attestation names every ground at its hash. A ground that cannot be
        // pinned is a violation, never a ground left out.
        for upstream in absent {
            let known = violations.iter().any(
                |v| matches!(v, CheckViolation::Upstream { upstream: u, .. } if u == upstream),
            );
            if !known {
                violations.push(CheckViolation::Upstream {
                    upstream: *upstream,
                    problem: UpstreamProblem::Missing,
                });
            }
        }
    }
    let attestation = match pinned {
        Ok(grounds) if violations.is_empty() => Some(Attestation::Cs {
            spec: Pinned::new(spec.id(), *spec.contract_hash()),
            hyp: *spec.hyp_hash(),
            grounds,
        }),
        _ => None,
    };
    Ok(Checked {
        gate: Gate::Cs,
        target: id,
        violations,
        attestation,
    })
}

fn prepare_ci(model: &Model, target: &Ref) -> Result<Checked, AppError> {
    let id = model.resolve_as(target, EntityKind::Design)?;
    let ev = model
        .designs()
        .get(&DesignId::new(id))
        .ok_or(AppError::TargetFileMissing { id })?;
    ensure_valid(id, ev.state().approval())?;
    let design = ev.entity().ok_or(AppError::TargetUnparsed { id })?;
    let violations = check_ci(design, ev.state(), model.specs());
    let attestation = if violations.is_empty() {
        model
            .specs()
            .get(&design.spec())
            .and_then(|ev| ev.entity())
            .map(|spec| Attestation::Ci {
                design: Pinned::new(design.id(), *design.design_hash()),
                spec: Pinned::new(spec.id(), *spec.contract_hash()),
            })
    } else {
        None
    };
    Ok(Checked {
        gate: Gate::Ci,
        target: id,
        violations,
        attestation,
    })
}

/// The CS approval that fixed the hypotheses of the Spec: one that fixed the
/// current hash if there is one, else any (so the check can say the hypotheses
/// changed). The smallest event identifier decides among equals.
fn cs_approval_for(model: &Model, spec: &Spec) -> Option<EventId> {
    let subject = model.canonical(spec.id().entity());
    let cs: Vec<(EventId, bool)> = model
        .ledger()
        .active()
        .filter(|r| r.subject() == subject)
        .filter_map(|r| match r.attestation() {
            Attestation::Cs { hyp, .. } => Some((r.event(), hyp == spec.hyp_hash())),
            _ => None,
        })
        .collect();
    cs.iter()
        .filter(|(_, fixed_current)| *fixed_current)
        .map(|(e, _)| *e)
        .min()
        .or_else(|| cs.iter().map(|(e, _)| *e).min())
}

fn prepare_cm(model: &Model, target: &Ref, input: &CmInput) -> Result<Checked, AppError> {
    let id = model.resolve_as(target, EntityKind::Spec)?;
    let ev = model
        .specs()
        .get(&SpecId::new(id))
        .ok_or(AppError::TargetFileMissing { id })?;
    let spec = ev.entity().ok_or(AppError::TargetUnparsed { id })?;
    let result = match input {
        CmInput::Pass => CmResult::Pass,
        CmInput::FailWiring { item_hint } => CmResult::FailWiring {
            item_hint: item_hint.clone(),
        },
        CmInput::FailHyp { finding } => {
            let fid = model.resolve_as(finding, EntityKind::Finding)?;
            let measured = model
                .findings()
                .get(&FindingId::new(fid))
                .and_then(|ev| ev.entity())
                .is_some_and(|f| matches!(f.source(), FindingSource::MeasuredData { .. }));
            if !measured {
                return Err(AppError::CmFindingNotMeasured { finding: fid });
            }
            CmResult::FailHyp {
                finding: FindingId::new(fid),
            }
        }
    };
    let (violations, attestation) = match cs_approval_for(model, spec) {
        None => (vec![CheckViolation::CsApprovalNotFound], None),
        Some(cs) => {
            let violations = check_cm(spec, &cs, model.ledger());
            let attestation = violations.is_empty().then(|| Attestation::Cm {
                spec: Pinned::new(spec.id(), *spec.hyp_hash()),
                cs,
                result,
            });
            (violations, attestation)
        }
    };
    Ok(Checked {
        gate: Gate::Cm,
        target: id,
        violations,
        attestation,
    })
}

impl<P: Ports> Workspace<P> {
    async fn prepare_ca(
        &self,
        model: &Model,
        target: &ItemRef,
        revision: Option<&RawRevision>,
    ) -> Result<Checked, AppError> {
        let item_id = model.resolve_item(target)?;
        let id = item_id.entity();
        let item = model.item(item_id).ok_or(AppError::ItemNotFound {
            item: target.clone(),
        })?;
        ensure_valid(id, item.evaluation().state().approval())?;
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
            .run(&revision, self.config.test().command())
            .await
            .map_err(AppError::port(PortName::TestRun))?;
        let key = item.key();
        let violations = check_ca(
            key,
            item.item(),
            model.designs(),
            model.specs(),
            &impl_view,
            tests,
        );
        let attestation = if violations.is_empty() {
            model
                .designs()
                .get(&key.design)
                .and_then(|ev| ev.entity())
                .map(|design| Attestation::Ca {
                    item: Pinned::new(key.id, *item.hash()),
                    design: Pinned::new(design.id(), *design.design_hash()),
                    revision: Revision::Commit(revision),
                    tests,
                })
        } else {
            None
        };
        Ok(Checked {
            gate: Gate::Ca,
            target: id,
            violations,
            attestation,
        })
    }

    /// Record an approval, over a check that passed.
    async fn approve_checked(
        &self,
        model: &Model,
        checked: Checked,
        by: Option<Actor>,
    ) -> Result<Approved, AppError> {
        let by = by
            .or_else(|| self.config.user().cloned())
            .ok_or(AppError::NoActor)?;
        let mut draft = self.draft().await?;
        draft.require(checked.gate, checked.violations);
        let Some(attestation) = checked.attestation else {
            // No attestation means the check found something; the gate says what.
            self.gate(model, &draft).await?;
            return Err(AppError::CheckFailed {
                gate: checked.gate,
                violations: Vec::new(),
            });
        };
        let store = model.home(checked.target).ok_or(AppError::NotFound {
            reference: Ref::Local(crate::domain::reference::LocalRef::Id(checked.target)),
        })?;
        let event = draft.push(
            model,
            store,
            checked.target,
            EventBody::Approved { by, attestation },
        )?;
        let committed = self.commit(model, draft).await?;
        Ok(Approved { event, committed })
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
        let checked = prepare_cr(&model, finding)?;
        self.approve_checked(&model, checked, by).await
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
        let checked = prepare_cs(&model, spec)?;
        self.approve_checked(&model, checked, by).await
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
        let checked = prepare_ci(&model, design)?;
        self.approve_checked(&model, checked, by).await
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
        let checked = self.prepare_ca(&model, item, revision).await?;
        self.approve_checked(&model, checked, by).await
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
        let checked = prepare_cm(&model, spec, result)?;
        self.approve_checked(&model, checked, by).await
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
            EventBody::Revoked {
                revokes: approval,
                reason,
                by,
            },
        )?;
        self.commit(&model, draft).await
    }
}
