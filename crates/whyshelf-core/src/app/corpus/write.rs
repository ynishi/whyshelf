//! What the use cases that write have in common: who, when and which id; the evidence
//! chain driven end to end; the question put to a person.
//!
//! Nothing here decides. Which ledger a record goes to is
//! [`JudgementKind::ledger`](crate::domain::judgement::JudgementKind::ledger), whether a
//! secret is in a text is the secret scan's, whether a person must confirm is the
//! plan's (`confirmation()`), and whether a confirmation still holds is
//! [`HumanConfirmation::holds_for`]. This module only calls them in the order the
//! chain of [`commit::record`](crate::app::commit::record) needs.

use crate::app::commit::confirm::{HumanConfirmation, confirm};
use crate::app::commit::record::{
    ChainError, append_with_object, append_without_object, scan, store,
};
use crate::app::corpus::flow::Flow;
use crate::app::corpus::snapshot::Snapshot;
use crate::app::error::AppError;
use crate::app::outcome::Refusal;
use crate::app::ports::{
    ActorSource, ClockPort, ConfirmPrompt, CorpusPorts, CorpusRead, DocumentSource, FileBytes,
    IdPort, PortName, Write,
};
use crate::domain::actor::Timestamp;
use crate::domain::corpus::{Corpus, FileFacts};
use crate::domain::id::DocId;
use crate::domain::judge::Judge;
use crate::domain::judgement::{Judgement, JudgementId, Split};
use crate::domain::ledger::Ledgers;
use crate::domain::location::RelPath;
use crate::domain::publication::ConfirmDigest;

/// Who is acting, when, and the identity of the record: what a judgement is made with
/// and the domain never makes.
pub(super) struct Mint {
    pub(super) id: JudgementId,
    pub(super) at: Timestamp,
    pub(super) judge: Judge,
}

impl Mint {
    /// Ask the ports, who is acting included.
    pub(super) async fn take<P: CorpusPorts>(ports: &P) -> Result<Self, AppError> {
        let judge = actor(ports).await?;
        Self::of(ports, judge).await
    }

    /// Ask the ports for the time and the identity, for `judge`, who is known already.
    pub(super) async fn of<P: CorpusPorts>(ports: &P, judge: Judge) -> Result<Self, AppError> {
        let at = ports
            .time()
            .now()
            .await
            .map_err(AppError::port(PortName::Clock))?;
        let id = JudgementId::new(
            ports
                .ids()
                .new_entity_id()
                .await
                .map_err(AppError::port(PortName::Id))?,
        );
        Ok(Self { id, at, judge })
    }
}

/// Who is acting.
pub(super) async fn actor<P: CorpusPorts>(ports: &P) -> Result<Judge, AppError> {
    ports
        .actor_source()
        .actor()
        .await
        .map_err(AppError::port(PortName::Actor))
}

/// Write an approval or an adoption: scan its text, store it, append the record.
///
/// The text is the one the gate (or the adoption's check) read, which `snapshot` kept.
pub(super) async fn record_with_object<P: CorpusPorts>(
    ports: &P,
    snapshot: &Snapshot,
    judgement: Judgement,
) -> Result<(), Flow> {
    let Split::WithObject(view) = judgement.split() else {
        return Err(ChainError::NoObject.into());
    };
    let text = snapshot.text(*view.checked_text())?;
    let scanned = scan(
        ports.secrets(),
        snapshot.corpus(),
        snapshot.ledgers(),
        text,
        &view,
    )
    .await?;
    let stored = store(ports.put_texts(), &scanned).await?;
    append_with_object(ports.appender(), view, &stored).await?;
    Ok(())
}

/// Write a revocation, a retirement, a purge or a publication.
pub(super) async fn record_plain<P: CorpusPorts>(
    ports: &P,
    corpus: &Corpus,
    ledgers: &Ledgers,
    judgement: Judgement,
) -> Result<(), Flow> {
    let Split::Plain(view) = judgement.split() else {
        return Err(ChainError::KeepsObject.into());
    };
    append_without_object(ports.appender(), view, corpus, ledgers).await?;
    Ok(())
}

/// Put the question to a person if the plan says one is needed.
///
/// `prompt` is the plan's own ([`ConfirmPrompt::of_publication`] and its siblings give
/// `None` exactly when the plan's `confirmation()` is `None`), so whether to ask is
/// decided by the plan and by nothing else. An agent is refused before it is asked.
pub(super) async fn ask<P: CorpusPorts>(
    ports: &P,
    by: &Judge,
    prompt: Option<ConfirmPrompt>,
) -> Result<Option<HumanConfirmation>, Flow> {
    match prompt {
        Some(prompt) => Ok(Some(confirm(ports.confirm(), by, &prompt).await?)),
        None => Ok(None),
    }
}

/// Whether what was confirmed still holds for the plan made again just before the run.
///
/// | confirmed | the plan now asks | |
/// |---|---|---|
/// | yes | the same | goes on |
/// | yes | something else | [`Refusal::ConfirmationStale`] |
/// | yes | nothing | goes on: nothing is left that cannot be taken back |
/// | no | nothing | goes on |
/// | no | something | [`Refusal::NeedsHumanConfirm`]: the plan changed under the run and now needs a person |
pub(super) fn still_holds(
    confirmation: Option<&HumanConfirmation>,
    now: Option<ConfirmDigest>,
) -> Result<(), Refusal> {
    match (confirmation, now) {
        (Some(confirmed), Some(digest)) => confirmed.holds_for(&digest),
        (Some(_), None) | (None, None) => Ok(()),
        (None, Some(_)) => Err(Refusal::NeedsHumanConfirm),
    }
}

/// A document whose publication is cut short takes no new judgement: finish it or
/// abort it first.
pub(super) fn not_publishing(snapshot: &Snapshot, doc: DocId) -> Result<(), Refusal> {
    use crate::domain::corpus::PublishForm;
    match snapshot.corpus().publishing_of(&doc) {
        Some(open) if open.form() == PublishForm::FileCopied => {
            Err(Refusal::PublishInProgress(doc))
        }
        Some(_) | None => Ok(()),
    }
}

/// The bytes a file has now. A file that is not there any more is the file having
/// changed under the run ([`Refusal::ChangedUnderfoot`]), not a failure of the port.
pub(super) async fn read_bytes<P: CorpusRead>(
    ports: &P,
    file: &FileFacts,
) -> Result<Vec<u8>, Flow> {
    match ports
        .documents()
        .bytes(file)
        .await
        .map_err(AppError::port(PortName::Documents))?
    {
        FileBytes::Read(bytes) => Ok(bytes),
        FileBytes::Gone => Err(Refusal::ChangedUnderfoot(file.location().path().clone()).into()),
    }
}

/// What writing a file came to, as a refusal where it did not write.
///
/// `Exists` is not handled here: for a creation it is the caller's refusal.
pub(super) fn written(result: Write, at: &RelPath) -> Result<(), Refusal> {
    match result.refusal(at) {
        Some(refusal) => Err(refusal),
        None => Ok(()),
    }
}

#[cfg(test)]
mod tests;
