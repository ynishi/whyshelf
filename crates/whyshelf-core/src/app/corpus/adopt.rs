//! Taking an earlier decision in (`adopt`), and giving files the identity they lack
//! (`import`).
//!
//! # `adopt`
//!
//! An adoption is stronger than a gate (no test and no clean tree stood behind it) and
//! cannot be taken back, so it is never recorded on a flag or an argument: a person
//! confirms the adoption's own digest, and the run checks the confirmation against the
//! adoption made again just before it writes.
//!
//! ```text
//!   Snapshot -> named(target) -> adoption::check -> Adoptable        (an agent: AgentCannotAdopt)
//!        Adoptable::confirmation() is always Some -> ConfirmPort::ask -> HumanConfirmation
//!   Snapshot again -> adoption::check again -> HumanConfirmation::holds_for(new digest)
//!        Judgement::adopt (ledger: JudgementKind::ledger) -> split -> scan -> store -> append
//! ```
//!
//! # `import`
//!
//! Importing writes an `id:` where there is none: into a file that has none, or into
//! the items of a Design that have none. That is all it does; it takes nothing in as an
//! approval, so on its own it asks nobody. The one step in this module's reach that
//! cannot be taken back is an adoption, and an import that is asked to adopt what it
//! has just identified does it through `adopt`, whose own confirmation decides:
//!
//! ```text
//!   Import::File { adopt: None }      write the id                         -> no question
//!   Import::File { adopt: Some(..) }  write the id, then adopt it          -> the adoption asks
//!   Import::Items { design }          write the ids of the items           -> no question
//! ```
//!
//! The file is rewritten against the bytes the listing saw, by the one way a user's file
//! is changed ([`FileWriter::rewrite`](crate::app::ports::FileWriter::rewrite)); a file that
//! changed since is [`Refusal::ChangedUnderfoot`]. The id is written first and stays if
//! the adoption that follows is refused: the file is then a document, and `adopt` can be
//! run on it again.

use std::collections::BTreeMap;

use crate::app::config::Config;
use crate::app::corpus::flow::{Flow, finish};
use crate::app::corpus::idline::{with_doc_id, with_item_ids};
use crate::app::corpus::name::home_of;
use crate::app::corpus::snapshot::Snapshot;
use crate::app::corpus::write::{
    Mint, actor, ask, not_publishing, read_bytes, record_with_object, still_holds, written,
};
use crate::app::error::AppError;
use crate::app::outcome::{Outcome, Refusal};
use crate::app::ports::{ConfirmPrompt, CorpusPorts, FileWriter, IdPort, PortName};
use crate::domain::adoption::{self, Adoptable, AdoptionInput};
use crate::domain::approval::Gate;
use crate::domain::corpus::{Entity, FileFacts, Location, Visibility};
use crate::domain::design::ItemIssue;
use crate::domain::error::AdoptionRefusal;
use crate::domain::hash::BytesHash;
use crate::domain::id::{DocId, ItemId, ItemNumber};
use crate::domain::judge::Principal;
use crate::domain::judgement::{Judgement, JudgementId, JudgementKind, ObjectKey, Target};
use crate::domain::naming::{Named, Ref};

/// Take an earlier decision in as an approval: the command of [`adopt`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Adopt {
    /// The gate whose approval is taken in.
    pub gate: Gate,
    /// The document or item, as a person names it.
    pub target: Ref,
    /// Where the decision came from: an address, or a citation in free text.
    pub source: String,
}

/// What an adoption wrote.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Adopted {
    id: JudgementId,
    gate: Gate,
    target: Target,
    ledger: Visibility,
}

impl Adopted {
    /// The record, which a revocation names.
    pub fn id(&self) -> JudgementId {
        self.id
    }

    /// The gate whose approval was taken in.
    pub fn gate(&self) -> Gate {
        self.gate
    }

    /// The document or item.
    pub fn target(&self) -> Target {
        self.target
    }

    /// The ledger the record was written to.
    pub fn ledger(&self) -> Visibility {
        self.ledger
    }
}

/// Take the decision in, after a person confirms it. See the module documentation.
pub async fn adopt<P: CorpusPorts>(
    ports: &P,
    config: &Config,
    command: Adopt,
) -> Result<Outcome<Adopted>, AppError> {
    finish(adopt_flow(ports, config, command).await)
}

/// What the adoption is about, for the check: the target and the document it belongs
/// to.
fn target_of(named: &Named) -> (Target, crate::domain::id::DocId) {
    let target = match named {
        Named::Doc(doc) => Target::Doc(*doc),
        Named::Item(item) => Target::Item(item.id()),
    };
    (target, home_of(named))
}

/// The adoption's check on the snapshot, and the document the target belongs to.
fn adoptable_for(
    snapshot: &Snapshot,
    command: &Adopt,
) -> Result<(Adoptable, crate::domain::id::DocId), Flow> {
    let named = snapshot.named(&command.target)?;
    let (target, home) = target_of(&named);
    // The version checked is the one of the file that holds the target (an item's Design).
    let text = snapshot
        .corpus()
        .by_id(&home)
        .first()
        .map(|file| file.text())
        .ok_or(AdoptionRefusal::TargetInvalid { target })?;
    let adoptable = adoption::check(AdoptionInput {
        corpus: snapshot.corpus(),
        gate: command.gate,
        target,
        source: &command.source,
        keyring: snapshot.keyring(),
        text,
    })?;
    Ok((adoptable, home))
}

async fn adopt_flow<P: CorpusPorts>(
    ports: &P,
    config: &Config,
    command: Adopt,
) -> Result<Adopted, Flow> {
    let snapshot = Snapshot::read(ports, config.gate_policy()).await?;
    let judge = actor(ports).await?;
    if matches!(judge.principal(), Principal::Agent { .. }) {
        return Err(Refusal::AgentCannotAdopt.into());
    }
    let (adoptable, home) = adoptable_for(&snapshot, &command)?;
    not_publishing(&snapshot, home)?;
    let confirmation = ask(ports, &judge, ConfirmPrompt::of_adoption(&adoptable)).await?;

    // The adoption is made again just before it is written: a file edited since the
    // question is not the one the person confirmed.
    let snapshot = Snapshot::read(ports, config.gate_policy()).await?;
    let (adoptable, home) = adoptable_for(&snapshot, &command)?;
    not_publishing(&snapshot, home)?;
    still_holds(confirmation.as_ref(), adoptable.confirmation())?;

    let mint = Mint::of(ports, judge).await?;
    let object = ObjectKey::new(*adoptable.attestation().hash(), *adoptable.text());
    let target = Target::of_subject(
        adoptable.attestation().gate(),
        adoptable.attestation().subject(),
    );
    let kind = JudgementKind::Adopt {
        adoptable: adoptable.clone(),
        object,
    };
    let origin = kind.ledger(snapshot.corpus(), snapshot.ledgers())?;
    let judgement = Judgement::adopt(mint.id, mint.at, mint.judge, origin, adoptable, object)?;
    record_with_object(ports, &snapshot, judgement).await?;
    Ok(Adopted {
        id: mint.id,
        gate: command.gate,
        target,
        ledger: origin,
    })
}

/// A decision to take in once a file has its identity.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AdoptAfter {
    /// The gate whose approval is taken in.
    pub gate: Gate,
    /// Where the decision came from.
    pub source: String,
}

/// Give files, or the items of a Design, the identity they lack: the command of
/// [`import`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Import {
    /// A file with no `id:` (the next step names it by where it is), and whether to
    /// take an earlier decision in for it once it is a document.
    File {
        /// Where the file is.
        at: Location,
        /// The adoption that follows, which asks for a confirmation of its own.
        adopt: Option<AdoptAfter>,
    },
    /// A Design with items that have no `id:`.
    Items {
        /// The Design.
        design: DocId,
    },
}

/// What an import wrote, one variant for each [`Import`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Imported {
    /// A file is a document now.
    File {
        /// The identity written into the file.
        doc: DocId,
        /// The adoption that followed, when the command asked for one.
        adopted: Option<Adopted>,
    },
    /// Items of a Design were given an identity.
    Items {
        /// The Design.
        design: DocId,
        /// The items whose `id:` was written; empty when every item had one.
        items: Vec<ItemId>,
    },
}

impl Imported {
    /// The document that has an identity now (the Design, for items).
    pub fn doc(&self) -> DocId {
        match self {
            Self::File { doc, .. } => *doc,
            Self::Items { design, .. } => *design,
        }
    }
}

/// Write the `id:` that is lacking, and adopt if asked. See the module documentation.
pub async fn import<P: CorpusPorts>(
    ports: &P,
    config: &Config,
    command: Import,
) -> Result<Outcome<Imported>, AppError> {
    finish(import_flow(ports, config, command).await)
}

async fn import_flow<P: CorpusPorts>(
    ports: &P,
    config: &Config,
    command: Import,
) -> Result<Imported, Flow> {
    let snapshot = Snapshot::read(ports, config.gate_policy()).await?;
    match command {
        Import::File { at, adopt: after } => {
            let candidate = snapshot
                .corpus()
                .candidates()
                .iter()
                .find(|candidate| {
                    candidate.facts().location().visibility() == at.visibility()
                        && candidate.facts().location().path() == at.path()
                })
                .ok_or_else(|| Refusal::ChangedUnderfoot(at.path().clone()))?;
            let facts = candidate.facts();
            let doc = DocId::new(new_id(ports).await?);
            let bytes = read_bytes(ports, facts).await?;
            rewrite(ports, facts, &with_doc_id(&bytes, doc)).await?;
            let adopted = match after {
                Some(AdoptAfter { gate, source }) => {
                    let next = Adopt {
                        gate,
                        target: Ref::Id(doc),
                        source,
                    };
                    Some(adopt_flow(ports, config, next).await?)
                }
                None => None,
            };
            Ok(Imported::File { doc, adopted })
        }
        Import::Items { design } => {
            // Rewriting the file of a publication cut short would make its two files
            // differ, and neither would be the document any more.
            not_publishing(&snapshot, design)?;
            let file =
                snapshot
                    .corpus()
                    .by_id(&design)
                    .first()
                    .ok_or(AdoptionRefusal::TargetUnknown {
                        target: Target::Doc(design),
                    })?;
            let Entity::Design(held) = file.document().entity() else {
                return Err(AdoptionRefusal::TargetInvalid {
                    target: Target::Doc(design),
                }
                .into());
            };
            let mut ids: BTreeMap<ItemNumber, ItemId> = BTreeMap::new();
            for issue in held.item_issues() {
                if let ItemIssue::UnidentifiedItem { number } = issue {
                    ids.insert(number, ItemId::new(new_id(ports).await?));
                }
            }
            if ids.is_empty() {
                // Every item has its identity already: running an import again after
                // it went through has nothing to do.
                return Ok(Imported::Items {
                    design,
                    items: Vec::new(),
                });
            }
            let facts = file.facts();
            let path = facts.location().path();
            let bytes = read_bytes(ports, facts).await?;
            let (rewritten, placed) =
                with_item_ids(&bytes, &ids).ok_or_else(|| Refusal::CannotPlaceId(path.clone()))?;
            rewrite(ports, facts, &rewritten).await?;
            // An entry the shape does not cover is left as it was: its minted id was
            // not written, so it is not reported.
            ids.retain(|number, _| placed.contains(number));
            Ok(Imported::Items {
                design,
                items: ids.into_values().collect(),
            })
        }
    }
}

async fn new_id<P: CorpusPorts>(ports: &P) -> Result<crate::domain::id::EntityId, AppError> {
    ports
        .ids()
        .new_entity_id()
        .await
        .map_err(AppError::port(PortName::Id))
}

/// Replace the file the listing saw with `bytes`, against the hash of what the listing
/// saw.
async fn rewrite<P: CorpusPorts>(ports: &P, facts: &FileFacts, bytes: &[u8]) -> Result<(), Flow> {
    // The bytes were just read; the writer compares them again as it replaces the file.
    let expect: BytesHash = facts.bytes();
    let result = ports
        .files()
        .rewrite(facts.location(), expect, bytes)
        .await
        .map_err(AppError::port(PortName::Files))?;
    written(result, facts.location().path())?;
    Ok(())
}

#[cfg(test)]
mod tests;
