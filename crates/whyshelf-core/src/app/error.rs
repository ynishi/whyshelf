//! The one error type of the use cases.
//!
//! A caller branches on the reason, so there is one variant per reason and none
//! of them is a message string. A failure of a port keeps its cause behind
//! [`AppError::Port`] (`#[source]`), so a log can print the whole chain; the
//! errors of the domain come in through `#[from]`.

use thiserror::Error;

use crate::domain::approval::Gate;
use crate::domain::error::{EntityError, EventError, ItemRefError, QueryError, ValueError};
use crate::domain::event::{DocKind, EntityKind, EventId, StoreKind};
use crate::domain::hash::ContentHash;
use crate::domain::id::{Alias, EntityId};
use crate::domain::lint::{CheckViolation, LeakFinding};
use crate::domain::location::RelPath;
use crate::domain::nonempty::NonEmptyVec;
use crate::domain::reference::{ItemRef, Ref};
use crate::domain::state::InvalidReason;
use crate::domain::tag::VocabularyError;

use super::ports::{PortName, SecretHit};

/// Why a use case failed.
#[derive(Debug, Error)]
pub enum AppError {
    /// A port failed. The adapter's own error is kept as the source.
    #[error("the {port} port failed")]
    Port {
        /// Which port.
        port: PortName,
        /// The adapter's error.
        #[source]
        source: Box<dyn std::error::Error + Send + Sync>,
    },
    /// A value with rules was refused.
    #[error(transparent)]
    Value(#[from] ValueError),
    /// An entity could not be built from its parts.
    #[error(transparent)]
    Entity(#[from] EntityError),
    /// An event could not be built.
    #[error(transparent)]
    Event(#[from] EventError),
    /// A `find` query could not be parsed.
    #[error(transparent)]
    Query(#[from] QueryError),
    /// An item reference could not be resolved.
    #[error(transparent)]
    ItemRef(#[from] ItemRefError),
    /// A tag is not in the project's vocabulary.
    #[error(transparent)]
    Vocabulary(#[from] VocabularyError),
    /// A reference names nothing in this store.
    #[error("`{reference}` names nothing")]
    NotFound {
        /// The reference as given.
        reference: Ref,
    },
    /// An item number is not registered in the Design.
    #[error("`{item}` names no registered item")]
    ItemNotFound {
        /// The item reference as given.
        item: ItemRef,
    },
    /// A reference names an entity in each store.
    #[error("`{reference}` names an entity in the repo store and another in the private store")]
    Ambiguous {
        /// The reference as given.
        reference: Ref,
    },
    /// A reference names the wrong kind of entity for the command.
    #[error("`{reference}` is a {found:?}, the command needs a {expected:?}")]
    WrongKind {
        /// The reference as given.
        reference: Ref,
        /// What the command needs.
        expected: EntityKind,
        /// What the reference names.
        found: EntityKind,
    },
    /// An external reference (`ext:` or a URL) was used where an entity of
    /// this project is needed.
    #[error("`{reference}` is not an entity of this project")]
    NotLocal {
        /// The reference as given.
        reference: Ref,
    },
    /// The file of the target did not parse, so there is nothing to approve.
    #[error("the file of {id} did not parse")]
    TargetUnparsed {
        /// The target.
        id: EntityId,
    },
    /// The target is Invalid, so nothing can be recorded for it.
    #[error("{id} is invalid")]
    TargetInvalid {
        /// The target.
        id: EntityId,
        /// Why.
        reasons: NonEmptyVec<InvalidReason>,
    },
    /// The target file is missing.
    #[error("the file of {id} is missing")]
    TargetFileMissing {
        /// The target.
        id: EntityId,
    },
    /// The machine check of a gate found violations (layer 1), so layer 2
    /// cannot be recorded.
    #[error("{gate:?} cannot be approved: {} violation(s)", .violations.len())]
    CheckFailed {
        /// The gate.
        gate: Gate,
        /// Everything the check found.
        violations: Vec<CheckViolation>,
    },
    /// The working tree has uncommitted changes.
    #[error("the working tree is not clean: commit first")]
    DirtyWorktree,
    /// The revision adapter did not accept a revision identifier.
    #[error("`{text}` is not a revision")]
    RevisionRejected {
        /// The text as given.
        text: String,
    },
    /// No name was given for `by` and the configuration has none.
    #[error("no name for the approver: pass one or set `user.name`")]
    NoActor,
    /// A secret was found in text about to be written.
    #[error("a secret was found in text about to be written")]
    SecretFound {
        /// Where, without the secret itself.
        hits: Vec<SecretHit>,
    },
    /// A public text carries the alias or title of a private entity.
    #[error("a text for the repo store carries the name of a private entity")]
    LeakFound {
        /// Each name found and where.
        findings: Vec<LeakFinding>,
    },
    /// The alias is already taken in the store.
    #[error("the alias `{alias}` is already taken by {holder}")]
    AliasTaken {
        /// The alias.
        alias: Alias,
        /// Who holds it.
        holder: EntityId,
    },
    /// An alias was given for several files at once.
    #[error("one alias cannot name {count} files")]
    AliasForSeveralFiles {
        /// How many files.
        count: usize,
    },
    /// The file is already registered.
    #[error("{path} is already registered")]
    AlreadyRegistered {
        /// The path.
        path: RelPath,
    },
    /// Another entity sits at the path.
    #[error("{path} is taken by {holder}")]
    PathOccupied {
        /// The path.
        path: RelPath,
        /// The entity there.
        holder: EntityId,
    },
    /// No file at the path.
    #[error("there is no file at {path}")]
    FileMissing {
        /// The path.
        path: RelPath,
    },
    /// The file is of another kind than the one asked for.
    #[error("{path} is a {found}, not a {requested}")]
    KindMismatch {
        /// The path.
        path: RelPath,
        /// What the caller asked for.
        requested: DocKind,
        /// What the file is.
        found: DocKind,
    },
    /// The file does not parse and no kind was given.
    #[error("{path} does not parse, so its kind must be given")]
    KindUnknown {
        /// The path.
        path: RelPath,
    },
    /// The command needs a document (a file), not an item.
    #[error("{id} is not a document")]
    NotADocument {
        /// The entity.
        id: EntityId,
    },
    /// The event is not an `Approved` event.
    #[error("{event} is not an approval")]
    NotAnApproval {
        /// The event as given.
        event: EventId,
    },
    /// The approval was withdrawn already.
    #[error("{event} is already revoked")]
    AlreadyRevoked {
        /// The approval.
        event: EventId,
    },
    /// The entity is in the repo store already.
    #[error("{id} is already in the repo store")]
    AlreadyPublished {
        /// The entity.
        id: EntityId,
    },
    /// A stored object is missing.
    #[error("the object {raw} is missing in the {store:?} store")]
    ObjectMissing {
        /// The key.
        raw: ContentHash,
        /// The store.
        store: StoreKind,
    },
    /// The hash is not a version of the entity.
    #[error("{raw} is not a version of {id}")]
    NotAVersion {
        /// The entity.
        id: EntityId,
        /// The key given.
        raw: ContentHash,
    },
    /// A scaffold would overwrite a file.
    #[error("{path} exists already")]
    PathExists {
        /// The path.
        path: RelPath,
    },
    /// A title gives no usable file name.
    #[error("the title gives no file name; give an alias")]
    NoSlug,
    /// A CM `fail-hyp` names a Finding that is not a measurement.
    #[error("{finding} is not a measured_data finding")]
    CmFindingNotMeasured {
        /// The Finding.
        finding: EntityId,
    },
}

impl AppError {
    /// The constructor of [`AppError::Port`] for use in `map_err`.
    pub(crate) fn port<E>(port: PortName) -> impl FnOnce(E) -> AppError
    where
        E: std::error::Error + Send + Sync + 'static,
    {
        move |source| AppError::Port {
            port,
            source: Box::new(source),
        }
    }
}
