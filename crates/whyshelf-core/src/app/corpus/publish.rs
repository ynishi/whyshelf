//! Publishing a private document, and undoing a publication that stopped part way.
//!
//! # `publish`
//!
//! The steps are the plan's ([`plan_publication`]): which of them there are, and in what
//! order, is decided by what the corpus shows now, so there is no flag for "resume". A
//! new publication has all four, one that stopped after the file was made has three,
//! and one that stopped after the commit point has the last.
//!
//! ```text
//!   plan = plan_publication(..)              from what the ports show now
//!   plan.confirmation() is Some  -->  ConfirmPort::ask --> HumanConfirmation
//!   plan again just before the run, HumanConfirmation::holds_for(new digest)
//!
//!   1 CopyRecordsAndObjects  the texts first (scan_copied -> store), then the records
//!                            (append_copied): a copy of a record is inert until 3
//!   2 CreatePublicFile       FileWriter::create_new, never over another file
//!   3 WritePublished         one record naming the copied ones: the commit point
//!   4 RemovePrivateFile      FileWriter::remove, against the bytes the plan saw
//! ```
//!
//! Every step is safe to run again, and the plan is made again each time from what is
//! there: records the public ledger holds already are skipped, a text with the same hash
//! is the same file, the public file that exists makes the plan start after the step
//! that makes it, and a `Published` that is written makes it start after the commit
//! point. A run that stopped anywhere reaches the same end when it is run again.
//!
//! # `abort_publish`
//!
//! Undoes a publication that stopped before its commit point: the plan is
//! [`plan_abort`]'s, the one step removes the public file, and the document is private
//! again. It writes no record. Whether it asks a person is the plan's to say.

use std::collections::BTreeMap;

use crate::app::commit::record::{ChainError, append_copied, scan_copied, store};
use crate::app::config::Config;
use crate::app::corpus::flow::{Flow, finish};
use crate::app::corpus::name::home_of;
use crate::app::corpus::snapshot::Snapshot;
use crate::app::corpus::write::{Mint, actor, ask, read_bytes, record_plain, still_holds, written};
use crate::app::error::AppError;
use crate::app::outcome::{Outcome, Refusal};
use crate::app::ports::{
    ConfirmPrompt, CorpusPorts, CorpusRead, FileWriter, ObjectRead, PortName, Write,
};
use crate::app::raw::NormalizedText;
use crate::domain::boundary::ObjectText;
use crate::domain::corpus::{Location, Visibility};
use crate::domain::error::PublicationError;
use crate::domain::hash::TextHash;
use crate::domain::id::DocId;
use crate::domain::judge::Judge;
use crate::domain::judgement::Judgement;
use crate::domain::knowable::Knowable;
use crate::domain::location::RelPath;
use crate::domain::naming::Ref;
use crate::domain::policy::GatePolicy;
use crate::domain::publication::{
    AbortEvidence, AbortPlan, AbortStep, ObjectTexts, PublicationPlan, PublicationStep,
    PublishEvidence, plan_abort, plan_publication,
};

/// Publish a private document: the command of [`publish`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Publish {
    /// The document, as a person names it (an item stands for its Design).
    pub doc: Ref,
    /// Where the public file goes. A publication that stopped has its public file
    /// already, and `None` finishes it where it is; for a new one it must be given.
    pub to: Option<RelPath>,
}

/// What a publication did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Published {
    doc: DocId,
    destination: RelPath,
    steps: Vec<PublicationStep>,
    records: usize,
}

impl Published {
    /// The document.
    pub fn doc(&self) -> DocId {
        self.doc
    }

    /// The public file.
    pub fn destination(&self) -> &RelPath {
        &self.destination
    }

    /// The steps this run did, in order.
    pub fn steps(&self) -> &[PublicationStep] {
        &self.steps
    }

    /// How many records the publication carries.
    pub fn records(&self) -> usize {
        self.records
    }
}

/// What the plan reads beyond the corpus and the ledgers: the texts of the versions and
/// the bytes of the file.
struct Evidence {
    texts: BTreeMap<TextHash, NormalizedText>,
    document: Vec<u8>,
}

/// Read the texts of the versions the ledgers keep, and the bytes of the file that is or
/// will be public.
async fn read_evidence<P: CorpusRead>(
    ports: &P,
    snapshot: &Snapshot,
    doc: DocId,
) -> Result<Evidence, Flow> {
    let corpus = snapshot.corpus();
    let file = match corpus.publishing_of(&doc) {
        Some(open) => open.public(),
        None => corpus
            .by_id(&doc)
            .first()
            .map(|file| file.facts())
            .ok_or(PublicationError::NotFound { doc })?,
    };
    let document = read_bytes(ports, file).await?;
    let mut texts = BTreeMap::new();
    for record in snapshot.ledgers().judgements() {
        let Some(key) = record.object() else { continue };
        let hash = *key.text();
        if snapshot.ledgers().is_purged(&hash) || texts.contains_key(&hash) {
            continue;
        }
        // A version of a private document lives in the private store; one a copy already
        // reached, in the public one too.
        for side in [Visibility::Private, Visibility::Public] {
            let found = ports
                .objects()
                .get(side, hash)
                .await
                .map_err(AppError::port(PortName::Objects))?;
            if let Some(text) = found {
                texts.insert(hash, NormalizedText::new(text));
                break;
            }
        }
    }
    Ok(Evidence { texts, document })
}

/// The plan, from what the ports show now.
fn plan_now(
    snapshot: &Snapshot,
    evidence: &Evidence,
    doc: DocId,
    to: &RelPath,
    judge: &Judge,
) -> Result<PublicationPlan, PublicationError> {
    let objects = ObjectTexts::of(
        evidence
            .texts
            .values()
            .map(|text| ObjectText::new(text.as_str().to_owned())),
    );
    plan_publication(
        doc,
        snapshot.corpus(),
        snapshot.ledgers(),
        to.clone(),
        judge,
        &PublishEvidence {
            objects: &objects,
            document: &evidence.document,
            private_root: snapshot.private_root(),
        },
    )
}

/// Where the public file goes.
fn destination(
    snapshot: &Snapshot,
    doc: DocId,
    given: Option<RelPath>,
) -> Result<RelPath, PublicationError> {
    match given {
        Some(to) => Ok(to),
        None => snapshot
            .corpus()
            .publishing_of(&doc)
            .map(|open| open.public().location().path().clone())
            .ok_or(PublicationError::NoneOpen { doc }),
    }
}

/// Publish the document, after a person confirms it where the plan says one must. See
/// the module documentation.
pub async fn publish<P: CorpusPorts>(
    ports: &P,
    config: &Config,
    command: Publish,
) -> Result<Outcome<Published>, AppError> {
    finish(publish_flow(ports, config, command).await)
}

async fn publish_flow<P: CorpusPorts>(
    ports: &P,
    config: &Config,
    command: Publish,
) -> Result<Published, Flow> {
    let policy = config.gate_policy();
    let snapshot = Snapshot::read(ports, policy).await?;
    let doc = home_of(&snapshot.named(&command.doc)?);
    let judge = actor(ports).await?;
    let to = destination(&snapshot, doc, command.to)?;
    let evidence = read_evidence(ports, &snapshot, doc).await?;
    let plan = plan_now(&snapshot, &evidence, doc, &to, &judge)?;
    let confirmation = ask(ports, &judge, ConfirmPrompt::of_publication(&plan)).await?;

    // The plan is made again just before the run: a file edited, or a record added,
    // since the question is not what the person confirmed.
    let (snapshot, evidence, plan) = if confirmation.is_some() {
        let snapshot = Snapshot::read(ports, policy).await?;
        let evidence = read_evidence(ports, &snapshot, doc).await?;
        let plan = plan_now(&snapshot, &evidence, doc, &to, &judge)?;
        still_holds(confirmation.as_ref(), plan.confirmation())?;
        (snapshot, evidence, plan)
    } else {
        (snapshot, evidence, plan)
    };

    let private = private_location(&snapshot, doc)?;
    for step in plan.steps() {
        match step {
            PublicationStep::CopyRecordsAndObjects => {
                copy_records_and_objects(ports, &snapshot, &plan, &evidence).await?;
            }
            PublicationStep::CreatePublicFile => {
                create_public_file(ports, &plan, &evidence).await?;
            }
            PublicationStep::WritePublished => {
                write_published(ports, policy, &plan, &judge).await?;
            }
            PublicationStep::RemovePrivateFile => {
                remove_private_file(ports, &plan, &private).await?;
            }
        }
    }
    Ok(Published {
        doc,
        destination: plan.destination().clone(),
        steps: plan.steps().to_vec(),
        records: plan.copied_count(),
    })
}

/// The private file the publication ends by removing: the private copy of one that
/// stopped, else the document's own file.
fn private_location(snapshot: &Snapshot, doc: DocId) -> Result<Location, PublicationError> {
    let corpus = snapshot.corpus();
    match corpus.publishing_of(&doc) {
        Some(open) => Ok(open.private_copy().facts().location().clone()),
        None => corpus
            .by_id(&doc)
            .first()
            .map(|file| file.location().clone())
            .ok_or(PublicationError::NotFound { doc }),
    }
}

/// Step 1: the texts the copied records name, and then the records.
///
/// The texts go first, so that no record in the public ledger names a text that is not
/// there. A record the public ledger holds is skipped, and a text with the hash of one
/// stored is the same file.
async fn copy_records_and_objects<P: CorpusPorts>(
    ports: &P,
    snapshot: &Snapshot,
    plan: &PublicationPlan,
    evidence: &Evidence,
) -> Result<(), Flow> {
    for key in plan.objects() {
        let text = evidence
            .texts
            .get(key.text())
            .ok_or(PublicationError::ObjectUnavailable { text: *key.text() })?;
        let scanned = scan_copied(ports.secrets(), text, *key, plan).await?;
        store(ports.put_texts(), &scanned).await?;
    }
    let private = snapshot.ledgers().private();
    for record in plan.records_to_write() {
        let held = private.get(&record.id()).ok_or(ChainError::NotAsRead {
            record: record.id(),
        })?;
        append_copied(ports.appender(), held, plan, private).await?;
    }
    Ok(())
}

/// Step 2: the public file, byte for byte, never over another.
async fn create_public_file<P: CorpusPorts>(
    ports: &P,
    plan: &PublicationPlan,
    evidence: &Evidence,
) -> Result<(), Flow> {
    let at = Location::new(
        Visibility::Public,
        plan.destination().clone(),
        Knowable::Unknown,
    );
    let result = ports
        .files()
        .create_new(&at, &evidence.document)
        .await
        .map_err(AppError::port(PortName::Files))?;
    match result {
        Write::Exists => Err(Refusal::PublishTargetExists(plan.destination().clone()).into()),
        other => Ok(written(other, plan.destination())?),
    }
}

/// Step 3: the one record that names the copied ones, the commit point. The ledgers are
/// read again, because the records it names must be in the public ledger now.
async fn write_published<P: CorpusPorts>(
    ports: &P,
    policy: GatePolicy,
    plan: &PublicationPlan,
    judge: &Judge,
) -> Result<(), Flow> {
    let snapshot = Snapshot::read(ports, policy).await?;
    let mint = Mint::of(ports, judge.clone()).await?;
    let judgement = Judgement::published(
        mint.id,
        mint.at,
        mint.judge,
        plan.doc(),
        plan.published_records(),
        snapshot.ledgers(),
    )?;
    record_plain(ports, snapshot.corpus(), snapshot.ledgers(), judgement).await
}

/// Step 4: the private file, last, against the bytes the plan saw.
async fn remove_private_file<P: CorpusPorts>(
    ports: &P,
    plan: &PublicationPlan,
    private: &Location,
) -> Result<(), Flow> {
    let result = ports
        .files()
        .remove(private, plan.private_file())
        .await
        .map_err(AppError::port(PortName::Files))?;
    Ok(written(result, private.path())?)
}

/// Undo a publication that stopped before its commit point: the command of
/// [`abort_publish`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AbortPublish {
    /// The document, as a person names it (an item stands for its Design).
    pub doc: Ref,
}

/// What an abort did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Aborted {
    doc: DocId,
    removed: RelPath,
}

impl Aborted {
    /// The document, which is private again.
    pub fn doc(&self) -> DocId {
        self.doc
    }

    /// The public file that was removed.
    pub fn removed(&self) -> &RelPath {
        &self.removed
    }
}

async fn abort_plan_now<P: CorpusRead>(
    ports: &P,
    snapshot: &Snapshot,
    doc: DocId,
) -> Result<(AbortPlan, Location), Flow> {
    let open = snapshot
        .corpus()
        .publishing_of(&doc)
        .ok_or(PublicationError::NoneOpen { doc })?;
    let public = open.public();
    let bytes = read_bytes(ports, public).await?;
    let plan = plan_abort(
        doc,
        snapshot.corpus(),
        snapshot.ledgers(),
        &AbortEvidence {
            public_file: &bytes,
        },
    )?;
    Ok((plan, public.location().clone()))
}

/// Undo the publication of the document: the public file goes, and the document is
/// private again. See the module documentation.
pub async fn abort_publish<P: CorpusPorts>(
    ports: &P,
    config: &Config,
    command: AbortPublish,
) -> Result<Outcome<Aborted>, AppError> {
    finish(abort_flow(ports, config, command).await)
}

async fn abort_flow<P: CorpusPorts>(
    ports: &P,
    config: &Config,
    command: AbortPublish,
) -> Result<Aborted, Flow> {
    let policy = config.gate_policy();
    let snapshot = Snapshot::read(ports, policy).await?;
    let doc = home_of(&snapshot.named(&command.doc)?);
    let judge = actor(ports).await?;
    let (plan, public) = abort_plan_now(ports, &snapshot, doc).await?;
    let confirmation = ask(ports, &judge, ConfirmPrompt::of_abort(&plan)).await?;

    let (plan, public) = if confirmation.is_some() {
        let snapshot = Snapshot::read(ports, policy).await?;
        let (plan, public) = abort_plan_now(ports, &snapshot, doc).await?;
        still_holds(confirmation.as_ref(), plan.confirmation())?;
        (plan, public)
    } else {
        (plan, public)
    };

    for step in plan.steps() {
        match step {
            AbortStep::RemovePublicFile => {
                // The bytes the plan saw (the plan's `expect`, which is not
                // `Option::expect`, and called by path so that it reads as it is).
                let result = ports
                    .files()
                    .remove(&public, AbortPlan::expect(&plan))
                    .await
                    .map_err(AppError::port(PortName::Files))?;
                written(result, plan.public_file())?;
            }
        }
    }
    Ok(Aborted {
        doc,
        removed: plan.public_file().clone(),
    })
}

#[cfg(test)]
mod tests;
