//! The chain of evidence between a judgement and a ledger.
//!
//! A [`Judgement`](crate::domain::judgement::Judgement) says which version it keeps an object for ([`ObjectKey`]); it does
//! not know the object was stored. That guarantee is not in the domain's type. It is
//! kept by the one door into a ledger, [`LedgerAppend::append`], taking only a
//! [`Recordable`], and by a [`Recordable`] with an object being made only from
//! [`StoredObject`], which is made only from a text that passed the secret scan and
//! that the object store reported stored.
//!
//! ```text
//!   judgement.split()     Split::WithObject(ObjectJudgement) | Split::Plain(PlainJudgement)
//!        |   whether a judgement keeps an object is decided once, here, by its kind
//!        v
//!   scan(secret, corpus, ledgers, text, &ObjectJudgement)
//!        |   the text is scanned for secrets; its TextHash must be the one the
//!        |   gate read (Passed::text, Adoptable::text); the ledger is the one the
//!        |   rule chooses
//!        v
//!     Scanned ------- ObjectPut::put ------> Put::Stored ----> StoredObject
//!                                              Put::Purged ----> Refusal::ObjectPurged
//!        store(objects, &scanned) does the put and the reading of its answer
//!
//!     append_with_object(ledger, ObjectJudgement, &StoredObject)      Approve, Adopt
//!     append_without_object(ledger, PlainJudgement, corpus, ledgers)  Revoke, Retire, Purge, Published
//!     append_copied(ledger, &StoredJudgement, &PublicationPlan, private)  a publication's copy
//!        |   each makes a Recordable (the checks below) and spends it
//!        v
//!     LedgerAppend::append(&Recordable)
//! ```
//!
//! # Why the constructors are in this module's reach
//!
//! The constructors are `pub(in crate::app::commit)`: callable from
//! [`commit`](super) and its children and from nowhere else. The chain sits in a
//! child of `commit` (not beside it, in `app`) so that the restriction is the one the
//! write path already speaks of, and so that the steps ([`scan`], [`store`]) that do
//! the checking before making a value can be next to the constructors. The reading
//! of a put's answer (`stored`) is in the same reach: only [`store`], which made the
//! put, can turn a [`Put::Stored`] into a [`StoredObject`]. The types are public, to
//! be named and passed; they are not buildable:
//!
//! ```compile_fail,E0451
//! use whyshelf_core::app::commit::record::Recordable;
//! use whyshelf_core::domain::corpus::Visibility;
//! use whyshelf_core::domain::judgement::Judgement;
//!
//! // `Recordable` has private fields: a record cannot be made to say "write this".
//! fn forged(judgement: Judgement) -> Recordable {
//!     Recordable { judgement, ledger: Visibility::Public }
//! }
//! ```
//!
//! ```compile_fail,E0624
//! use whyshelf_core::app::commit::record::{Recordable, StoredObject};
//! use whyshelf_core::domain::judgement::ObjectJudgement;
//!
//! // The constructor is private to the commit step: no stored object, no record.
//! fn forged(judgement: ObjectJudgement, stored: &StoredObject) {
//!     let _ = Recordable::with_object(judgement, stored);
//! }
//! ```
//!
//! ```compile_fail,E0624
//! use whyshelf_core::app::commit::record::Recordable;
//! use whyshelf_core::domain::corpus::Corpus;
//! use whyshelf_core::domain::judgement::PlainJudgement;
//! use whyshelf_core::domain::ledger::Ledgers;
//!
//! fn forged(judgement: PlainJudgement, corpus: &Corpus, ledgers: &Ledgers) {
//!     let _ = Recordable::without_object(judgement, corpus, ledgers);
//! }
//! ```
//!
//! ```compile_fail,E0624
//! use whyshelf_core::app::commit::record::Recordable;
//! use whyshelf_core::domain::ledger::Ledger;
//! use whyshelf_core::domain::publication::PublicationPlan;
//! use whyshelf_core::domain::stored::StoredJudgement;
//!
//! fn forged(record: &StoredJudgement, plan: &PublicationPlan, private: &Ledger) {
//!     let _ = Recordable::copied(record, plan, private);
//! }
//! ```
//!
//! # What a record read back can be written as
//!
//! A [`StoredJudgement`] (a record read from a ledger) has no way into the first two
//! constructors: they take an [`ObjectJudgement`] or a [`PlainJudgement`], which only
//! [`Judgement::split`](crate::domain::judgement::Judgement::split) makes, and a stored record has no `split` (see
//! [`stored`](crate::domain::stored)). Its one write is `Recordable::copied`, a
//! publication's copy of a record of the private ledger: it takes the record as read
//! and writes it to the public ledger byte for byte.
//!
//! ```compile_fail,E0624
//! use whyshelf_core::app::commit::record::Scanned;
//! use whyshelf_core::app::raw::NormalizedText;
//! use whyshelf_core::domain::corpus::Visibility;
//! use whyshelf_core::domain::judgement::ObjectJudgement;
//!
//! // A text that did not pass the secret scan is not a `Scanned`.
//! fn forged(text: &NormalizedText, judgement: &ObjectJudgement) {
//!     let _ = Scanned::new(text, judgement, Visibility::Public);
//! }
//! ```
//!
//! ```compile_fail,E0603
//! use whyshelf_core::app::commit::record::{Scanned, StoredObject, stored};
//! use whyshelf_core::app::ports::Put;
//!
//! // Saying "stored" without a put: the answer is read only by the step that put.
//! fn forged(scanned: &Scanned<'_>) -> StoredObject {
//!     stored(scanned, Put::Stored).unwrap()
//! }
//! ```
//!
//! ```compile_fail,E0451
//! use whyshelf_core::app::commit::record::StoredObject;
//! use whyshelf_core::domain::corpus::Visibility;
//! use whyshelf_core::domain::judgement::ObjectKey;
//!
//! // Nothing was stored, so there is no evidence that something was.
//! fn forged(key: ObjectKey) -> StoredObject {
//!     StoredObject { key, ledger: Visibility::Public }
//! }
//! ```
//!
//! # What is checked where
//!
//! | step | stops with |
//! |---|---|
//! | [`scan`] | [`Refusal::Secret`]; a ledger the rule cannot choose ([`Refusal::Judgement`]); a text that is not the version the gate read ([`ChainError::TextDiffers`]) |
//! | [`store`] | [`Refusal::ObjectPurged`] for [`Put::Purged`] |
//! | `Recordable::with_object` ([`append_with_object`]) | an object or a ledger other than the judgement's ([`ChainError`]) |
//! | `Recordable::without_object` ([`append_without_object`]) | a ledger the rule cannot choose ([`Refusal::Judgement`]) |
//! | `Recordable::copied` ([`append_copied`]) | a record the plan does not carry ([`ChainError::NotInPlan`]); a record other than the private ledger's ([`ChainError::NotAsRead`]) |
//!
//! The text of a version is tied to the approval by the [`TextHash`] the gate set
//! from the text it read ([`Passed::text`](crate::domain::gate::Passed::text),
//! [`Adoptable::text`](crate::domain::adoption::Adoptable::text)), and the domain
//! keeps the judgement's [`ObjectKey`] to that hash: the object stored is the text
//! the gate checked, never one the caller names.

use thiserror::Error;

use crate::app::commit::Stop;
use crate::app::error::AppError;
use crate::app::outcome::Refusal;
use crate::app::ports::{LedgerAppend, ObjectPut, PortName, Put, SecretScanPort};
use crate::app::raw::NormalizedText;
use crate::domain::corpus::{Corpus, Visibility};
use crate::domain::error::JudgementError;
use crate::domain::hash::TextHash;
use crate::domain::judgement::{JudgementId, ObjectJudgement, ObjectKey, PlainJudgement};
use crate::domain::ledger::{Ledger, Ledgers};
use crate::domain::publication::PublicationPlan;
use crate::domain::stored::StoredJudgement;

/// Why the parts handed to a step do not belong together.
///
/// Every variant is a mistake in the code that called the step. It is not a
/// [`Refusal`]: the person cannot act on it.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum ChainError {
    /// The text is not the version the gate read.
    #[error("the text is {found}, and the gate read {expected}")]
    TextDiffers {
        /// The hash of the text the gate read.
        expected: TextHash,
        /// The hash of the text handed over.
        found: TextHash,
    },
    /// The stored object is of another version than the judgement's.
    #[error("the object stored is of {stored:?}, and the judgement keeps {judgement:?}")]
    ObjectDiffers {
        /// The key the judgement keeps (boxed: the error stays small).
        judgement: Box<ObjectKey>,
        /// The key of the object stored.
        stored: Box<ObjectKey>,
    },
    /// The judgement was first written in a ledger other than the one the evidence
    /// is for.
    #[error(
        "the record was made for the {origin:?} ledger, and the evidence is for the {ledger:?}"
    )]
    LedgerDiffers {
        /// The ledger the judgement says it was first written in.
        origin: Visibility,
        /// The ledger the evidence is for.
        ledger: Visibility,
    },
    /// The record is not among those the plan copies.
    #[error("the record {record} is not one the plan copies")]
    NotInPlan {
        /// The record.
        record: JudgementId,
    },
    /// The record to copy is not, byte for byte, the one the private ledger holds.
    #[error("the record {record} is not the one the private ledger holds")]
    NotAsRead {
        /// The record.
        record: JudgementId,
    },
    /// The rule that chooses a ledger could not.
    #[error(transparent)]
    Placement(Box<JudgementError>),
}

impl From<JudgementError> for ChainError {
    fn from(error: JudgementError) -> Self {
        Self::Placement(Box::new(error))
    }
}

/// A text that passed the secret scan and is the version the gate of a judgement read.
/// Made by [`scan`] and nothing else.
#[derive(Debug, Clone, Copy)]
pub struct Scanned<'a> {
    text: &'a str,
    key: ObjectKey,
    ledger: Visibility,
}

impl<'a> Scanned<'a> {
    /// A scanned text.
    ///
    /// The caller has scanned `text`. Its [`TextHash`] must be the one the gate (or
    /// the adoption's check) read ([`ObjectJudgement::checked_text`]), which the domain
    /// keeps the judgement's [`ObjectKey`] to: a text other than the version the gate
    /// looked at is not put under that version's name.
    pub(in crate::app::commit) fn new(
        text: &'a NormalizedText,
        judgement: &ObjectJudgement,
        ledger: Visibility,
    ) -> Result<Self, ChainError> {
        let key = *judgement.object();
        let checked = *judgement.checked_text();
        let found = TextHash::of(text.as_str());
        if found != checked {
            return Err(ChainError::TextDiffers {
                expected: checked,
                found,
            });
        }
        Ok(Self {
            text: text.as_str(),
            key,
            ledger,
        })
    }

    /// The text.
    pub fn text(&self) -> &'a str {
        self.text
    }

    /// The key it is stored under: what was judged, and the hash of this text.
    pub fn key(&self) -> ObjectKey {
        self.key
    }

    /// The ledger of the judgement, which decides the store the text goes to.
    pub fn ledger(&self) -> Visibility {
        self.ledger
    }
}

/// The evidence that a text was stored. Made by [`store`] from a [`Put::Stored`], and
/// nothing else.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StoredObject {
    key: ObjectKey,
    ledger: Visibility,
}

impl StoredObject {
    /// The evidence for the text that was put.
    pub(in crate::app::commit) fn of(scanned: &Scanned<'_>) -> Self {
        Self {
            key: scanned.key,
            ledger: scanned.ledger,
        }
    }

    /// The key it was stored under.
    pub fn key(&self) -> ObjectKey {
        self.key
    }

    /// The ledger of the judgement it was stored for.
    pub fn ledger(&self) -> Visibility {
        self.ledger
    }
}

/// A judgement that may be written to a ledger, with the ledger it is written to.
///
/// The only argument of [`LedgerAppend::append`]:
/// the ledger is inside, so a caller cannot ask for a record in the wrong one.
///
/// The judgement is held as a [`StoredJudgement`], the form it has once written: what
/// an adapter writes is the canonical bytes, and reads it back as one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Recordable {
    judgement: StoredJudgement,
    ledger: Visibility,
}

impl Recordable {
    /// An approval or an adoption, with the evidence that its object was stored.
    ///
    /// The object must be the one the judgement keeps, and the ledger the one the
    /// judgement was first written in.
    pub(in crate::app::commit) fn with_object(
        judgement: ObjectJudgement,
        stored: &StoredObject,
    ) -> Result<Self, ChainError> {
        let key = *judgement.object();
        if key != stored.key {
            return Err(ChainError::ObjectDiffers {
                judgement: Box::new(key),
                stored: Box::new(stored.key),
            });
        }
        let origin = judgement.judgement().origin();
        if origin != stored.ledger {
            return Err(ChainError::LedgerDiffers {
                origin,
                ledger: stored.ledger,
            });
        }
        Ok(Self {
            ledger: stored.ledger,
            judgement: judgement.into_judgement().into(),
        })
    }

    /// A revocation, a retirement, a purge or a publication: a judgement that keeps
    /// no object. The ledger is the one
    /// [`Judgement::ledger`](crate::domain::judgement::Judgement::ledger) chooses.
    pub(in crate::app::commit) fn without_object(
        judgement: PlainJudgement,
        corpus: &Corpus,
        ledgers: &Ledgers,
    ) -> Result<Self, ChainError> {
        let judgement = judgement.into_judgement();
        let ledger = judgement.ledger(corpus, ledgers)?;
        if judgement.origin() != ledger {
            return Err(ChainError::LedgerDiffers {
                origin: judgement.origin(),
                ledger,
            });
        }
        Ok(Self {
            judgement: judgement.into(),
            ledger,
        })
    }

    /// A record of the private ledger, copied to the public one by a publication.
    ///
    /// `held` is the record as it was read; it keeps its origin and its canonical
    /// bytes, so the copy is the same record in both ledgers. It is accepted only if
    /// the plan carries it and `private` (the private ledger the plan was made from)
    /// holds it with the same bytes: a record of the same id with other bytes would
    /// make the id corrupt in both ledgers.
    ///
    /// The one way a record read back is written.
    pub(in crate::app::commit) fn copied(
        held: &StoredJudgement,
        plan: &PublicationPlan,
        private: &Ledger,
    ) -> Result<Self, ChainError> {
        let record = held.id();
        if !plan.records().iter().any(|r| r.id() == record) {
            return Err(ChainError::NotInPlan { record });
        }
        let read = private.get(&record).map(StoredJudgement::canonical_bytes);
        if read != Some(held.canonical_bytes()) {
            return Err(ChainError::NotAsRead { record });
        }
        Ok(Self {
            judgement: held.clone(),
            ledger: Visibility::Public,
        })
    }

    /// The judgement, as it will be read back.
    pub fn judgement(&self) -> &StoredJudgement {
        &self.judgement
    }

    /// The ledger it is written to.
    pub fn ledger(&self) -> Visibility {
        self.ledger
    }

    /// The judgement, given up.
    pub fn into_stored(self) -> StoredJudgement {
        self.judgement
    }
}

/// The step that makes a [`Scanned`]: scan the text for secrets, choose the ledger by
/// the one rule, and tie the text to the judgement.
pub async fn scan<'a, S: SecretScanPort>(
    secret: &S,
    corpus: &Corpus,
    ledgers: &Ledgers,
    text: &'a NormalizedText,
    judgement: &ObjectJudgement,
) -> Result<Scanned<'a>, Stop> {
    let hits = secret
        .scan(text)
        .await
        .map_err(AppError::port(PortName::SecretScan))?;
    if let Some(refusal) = Refusal::secret(hits) {
        return Err(Stop::Refused(refusal));
    }
    let ledger = judgement
        .judgement()
        .ledger(corpus, ledgers)
        .map_err(|error| Stop::Refused(error.into()))?;
    Ok(Scanned::new(text, judgement, ledger)?)
}

/// Read the object store's answer to a put.
///
/// A purged version is not stored again: it is a [`Refusal::ObjectPurged`]. Only
/// [`Put::Stored`] gives a [`StoredObject`]. Private to the commit step: a
/// [`Put::Stored`] that no put answered is not evidence.
pub(in crate::app::commit) fn stored(
    scanned: &Scanned<'_>,
    put: Put,
) -> Result<StoredObject, Refusal> {
    match put {
        Put::Stored => Ok(StoredObject::of(scanned)),
        Put::Purged => Err(Refusal::ObjectPurged(*scanned.key.text())),
    }
}

/// The step that stores a [`Scanned`] and reads the answer: [`ObjectPut::put`], then
/// the evidence for [`Put::Stored`] or [`Refusal::ObjectPurged`] for [`Put::Purged`].
pub async fn store<O: ObjectPut>(objects: &O, scanned: &Scanned<'_>) -> Result<StoredObject, Stop> {
    let put = objects
        .put(scanned)
        .await
        .map_err(AppError::port(PortName::Objects))?;
    Ok(stored(scanned, put)?)
}

/// Write an approval or an adoption to its ledger, once its object is stored.
///
/// The [`Recordable`] is made and spent here; it does not leave the commit step.
pub async fn append_with_object<L: LedgerAppend>(
    ledger: &L,
    judgement: ObjectJudgement,
    stored: &StoredObject,
) -> Result<(), Stop> {
    let record = Recordable::with_object(judgement, stored)?;
    append(ledger, &record).await
}

/// Write a revocation, a retirement, a purge or a publication to the ledger the rule
/// chooses.
///
/// A ledger the rule cannot choose is a refusal, as at [`scan`]: it is a fact of the
/// corpus (the target's files are under both roots, or nowhere), not a mistake of the
/// caller.
pub async fn append_without_object<L: LedgerAppend>(
    ledger: &L,
    judgement: PlainJudgement,
    corpus: &Corpus,
    ledgers: &Ledgers,
) -> Result<(), Stop> {
    let record =
        Recordable::without_object(judgement, corpus, ledgers).map_err(|error| match error {
            ChainError::Placement(error) => Stop::Refused((*error).into()),
            other => Stop::Broken(other),
        })?;
    append(ledger, &record).await
}

/// Write to the public ledger a record that a publication copies from the private
/// one (`private`, as the plan was made from it).
pub async fn append_copied<L: LedgerAppend>(
    ledger: &L,
    held: &StoredJudgement,
    plan: &PublicationPlan,
    private: &Ledger,
) -> Result<(), Stop> {
    let record = Recordable::copied(held, plan, private)?;
    append(ledger, &record).await
}

async fn append<L: LedgerAppend>(ledger: &L, record: &Recordable) -> Result<(), Stop> {
    ledger
        .append(record)
        .await
        .map_err(AppError::port(PortName::Ledger))?;
    Ok(())
}

#[cfg(test)]
mod tests;
