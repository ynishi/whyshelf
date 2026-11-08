//! The judgements that keep no object: `revoke`, `retire` and `purge`.
//!
//! Each reads the snapshot, lets the domain build the record and say which ledger it
//! goes to, and writes it through the chain that keeps no object
//! ([`record_plain`](super::write::record_plain)):
//!
//! ```text
//!   Snapshot --> Ledgers::revoke | Judgement::retire | Judgement::purge
//!                  origin = JudgementKind::ledger(corpus, ledgers)   (asked before the record exists)
//!                  split -> append_without_object
//! ```
//!
//! What they name is already an identity, not a name to look up: a revocation names
//! records (the surface shows their identifiers), a retirement and a purge are about
//! what has no file to be found by, so they take the target as the next step names it.

use crate::app::config::Config;
use crate::app::corpus::flow::{Flow, finish};
use crate::app::corpus::snapshot::Snapshot;
use crate::app::corpus::write::{Mint, not_publishing, record_plain};
use crate::app::error::AppError;
use crate::app::outcome::Outcome;
use crate::app::ports::CorpusPorts;
use crate::domain::corpus::Visibility;
use crate::domain::hash::TextHash;
use crate::domain::id::DocId;
use crate::domain::judgement::{FilePresence, Judgement, JudgementId, JudgementKind, Target};
use crate::domain::ledger::{RevokeRequest, RevokeWhat};
use crate::domain::nonempty::{NonBlank, NonEmptyVec};
use crate::domain::state::Presence;

/// What a revocation takes away.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RevokeBy {
    /// These records, named one by one.
    Records(NonEmptyVec<JudgementId>),
    /// The retirements of this target that are visible and unrevoked, the revoking
    /// judge's.
    Retirement(Target),
}

/// Take records back: the command of [`revoke`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Revoke {
    /// What is taken back.
    pub by: RevokeBy,
    /// Why.
    pub reason: NonBlank,
    /// Whether a record of another judge may be revoked.
    pub force: bool,
}

/// What a revocation wrote.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Revoked {
    id: JudgementId,
    records: Vec<JudgementId>,
    ledger: Visibility,
}

impl Revoked {
    /// The revocation.
    pub fn id(&self) -> JudgementId {
        self.id
    }

    /// The records it takes away.
    pub fn records(&self) -> &[JudgementId] {
        &self.records
    }

    /// The ledger it was written to.
    pub fn ledger(&self) -> Visibility {
        self.ledger
    }
}

/// Write a revocation of the records the command names (or the retirements of the
/// target), to the ledger of what they are about.
///
/// The domain chooses which records are taken: only the ones visible in the ledger it
/// is written to, never the ones another branch wrote ([`Ledgers::revoke`](crate::domain::ledger::Ledgers::revoke)).
pub async fn revoke<P: CorpusPorts>(
    ports: &P,
    config: &Config,
    command: Revoke,
) -> Result<Outcome<Revoked>, AppError> {
    finish(revoke_flow(ports, config, command).await)
}

async fn revoke_flow<P: CorpusPorts>(
    ports: &P,
    config: &Config,
    command: Revoke,
) -> Result<Revoked, Flow> {
    let snapshot = Snapshot::read(ports, config.gate_policy()).await?;
    let Revoke { by, reason, force } = command;
    // The ledger is asked of a revocation that names what it takes: a retirement is
    // placed as the target it is about.
    let (what, probe) = match by {
        RevokeBy::Records(records) => (
            RevokeWhat::Records(records.clone()),
            JudgementKind::Revoke {
                targets: records,
                reason: reason.clone(),
            },
        ),
        RevokeBy::Retirement(target) => (
            RevokeWhat::Retire { target, of: None },
            JudgementKind::Retire {
                target,
                reason: reason.clone(),
            },
        ),
    };
    let origin = probe.ledger(snapshot.corpus(), snapshot.ledgers())?;
    let mint = Mint::take(ports).await?;
    let judgement = snapshot.ledgers().revoke(RevokeRequest {
        id: mint.id,
        at: mint.at,
        by: mint.judge,
        origin,
        force,
        reason,
        what,
    })?;
    let records = match judgement.kind() {
        JudgementKind::Revoke { targets, .. } => targets.iter().copied().collect(),
        JudgementKind::Approve { .. }
        | JudgementKind::Adopt { .. }
        | JudgementKind::Retire { .. }
        | JudgementKind::Published { .. }
        | JudgementKind::Purge { .. } => Vec::new(),
    };
    record_plain(ports, snapshot.corpus(), snapshot.ledgers(), judgement).await?;
    Ok(Revoked {
        id: mint.id,
        records,
        ledger: origin,
    })
}

/// Record that something whose file is gone is gone on purpose: the command of
/// [`retire`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Retire {
    /// What is retired.
    pub target: Target,
    /// Why.
    pub reason: NonBlank,
}

/// What a retirement wrote.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Retired {
    id: JudgementId,
    target: Target,
    ledger: Visibility,
}

impl Retired {
    /// The retirement.
    pub fn id(&self) -> JudgementId {
        self.id
    }

    /// What was retired.
    pub fn target(&self) -> Target {
        self.target
    }

    /// The ledger it was written to.
    pub fn ledger(&self) -> Visibility {
        self.ledger
    }
}

/// Retire a document or item whose file is gone.
///
/// Whether the file is gone is the evaluation's ([`presence_of`]): something whose file
/// is there is refused by the domain's constructor.
///
/// [`presence_of`]: crate::domain::corpus_eval::CorpusEvaluation::presence_of
pub async fn retire<P: CorpusPorts>(
    ports: &P,
    config: &Config,
    command: Retire,
) -> Result<Outcome<Retired>, AppError> {
    finish(retire_flow(ports, config, command).await)
}

async fn retire_flow<P: CorpusPorts>(
    ports: &P,
    config: &Config,
    command: Retire,
) -> Result<Retired, Flow> {
    let snapshot = Snapshot::read(ports, config.gate_policy()).await?;
    let Retire { target, reason } = command;
    if let Target::Doc(doc) = target {
        not_publishing(&snapshot, doc)?;
    }
    let presence = match snapshot.evaluation().presence_of(&target) {
        Some(Presence::Present) => FilePresence::Present,
        Some(Presence::Missing | Presence::Retired) | None => FilePresence::Absent,
    };
    let origin = JudgementKind::Retire {
        target,
        reason: reason.clone(),
    }
    .ledger(snapshot.corpus(), snapshot.ledgers())?;
    let mint = Mint::take(ports).await?;
    let judgement = Judgement::retire(
        mint.id, mint.at, mint.judge, origin, target, presence, reason,
    )?;
    record_plain(ports, snapshot.corpus(), snapshot.ledgers(), judgement).await?;
    Ok(Retired {
        id: mint.id,
        target,
        ledger: origin,
    })
}

/// Delete the stored text of a version: the command of [`purge`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Purge {
    /// The document the version belonged to.
    pub doc: DocId,
    /// The text to delete.
    pub text: TextHash,
    /// Why.
    pub reason: NonBlank,
}

/// What a purge wrote.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Purged {
    id: JudgementId,
    text: TextHash,
    ledger: Visibility,
}

impl Purged {
    /// The purge.
    pub fn id(&self) -> JudgementId {
        self.id
    }

    /// The text that was deleted.
    pub fn text(&self) -> TextHash {
        self.text
    }

    /// The ledger it was written to.
    pub fn ledger(&self) -> Visibility {
        self.ledger
    }
}

/// Delete the stored text of a version, and write that it was deleted.
///
/// A purge cannot be taken back. The ledger it is written to is the one of the
/// document's visibility; the appending of the record also takes the text out of the
/// store of the same side ([`LedgerAppend`](crate::app::ports::LedgerAppend)).
pub async fn purge<P: CorpusPorts>(
    ports: &P,
    config: &Config,
    command: Purge,
) -> Result<Outcome<Purged>, AppError> {
    finish(purge_flow(ports, config, command).await)
}

async fn purge_flow<P: CorpusPorts>(
    ports: &P,
    config: &Config,
    command: Purge,
) -> Result<Purged, Flow> {
    let snapshot = Snapshot::read(ports, config.gate_policy()).await?;
    let Purge { doc, text, reason } = command;
    not_publishing(&snapshot, doc)?;
    let origin = JudgementKind::Purge {
        doc,
        object: text,
        reason: reason.clone(),
    }
    .ledger(snapshot.corpus(), snapshot.ledgers())?;
    let mint = Mint::take(ports).await?;
    let judgement = Judgement::purge(mint.id, mint.at, mint.judge, origin, doc, text, reason);
    record_plain(ports, snapshot.corpus(), snapshot.ledgers(), judgement).await?;
    Ok(Purged {
        id: mint.id,
        text,
        ledger: origin,
    })
}

#[cfg(test)]
mod tests;
