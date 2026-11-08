//! What a use case gives back: a value, or one of three reasons it did not.
//!
//! A use case that works on the corpus and the ledgers does not fail with an
//! error when the answer is "no". A gate that finds something wrong, a
//! precondition that does not hold and a name that matches nothing are results the
//! caller has to show and act on, so they are values of [`Outcome`]; an error
//! ([`AppError`]) is for a port that failed, and an argument
//! that could not be read is an error before any use case runs.
//!
//! ```text
//!   Outcome<T>
//!     Done(T)                      wrote, or read, what was asked
//!     Violations(NonEmptyVec<..>)  a gate's machine check found these; nothing written
//!     Refused(Refusal)             a precondition does not hold; nothing written
//!     NotFound { query, miss }     the name given matches nothing, or several (see [`Miss`])
//! ```
//!
//! A name that cannot be looked up here at all (a `private:` document that may be
//! someone else's, another project's document, a URL) is not a miss: it is a refusal
//! ([`Refusal::CannotNameHere`]), and
//! [`corpus::outcome_of`](super::corpus::outcome_of) is the one place that tells the
//! four answers of a lookup apart.
//!
//! # One table for the exit
//!
//! The command line's exit code and the tool server's `isError` are decided from
//! what a use case returned, in the one function [`exit_class`], so the two surfaces
//! cannot disagree on what a refusal is. A use case returns
//! `Result<Outcome<T>, AppError>`: an outcome is an answer, an error is a failure of
//! the tool.
//!
//! | what the use case returned | [`ExitClass`] | [`is_error`](ExitClass::is_error) |
//! |---|---|---|
//! | `Ok(`[`Outcome::Done`]`)` | [`Success`](ExitClass::Success) | no |
//! | `Ok(`[`Outcome::Violations`]`)` | [`Violations`](ExitClass::Violations) | yes |
//! | `Ok(`[`Outcome::Refused`]`)` | [`Refused`](ExitClass::Refused) | yes |
//! | `Ok(`[`Outcome::NotFound`]`)` | [`NotFound`](ExitClass::NotFound) | yes |
//! | `Err(`[`AppError::Internal`]`)` | [`Internal`](ExitClass::Internal) | yes |
//! | `Err(` any other error `)` | [`Port`](ExitClass::Port) | yes |
//!
//! # Refusals and the domain's own
//!
//! The domain has refusals of its own ([`RevokeError`], [`JudgementError`],
//! [`AdoptionRefusal`], [`LeakRefusal`], [`PublicationError`]). A [`Refusal`] does not
//! restate them: it **wraps** them (`From`), and its message is the domain's. The
//! ones a caller branches on to choose the next step have a variant of their own, with
//! what the domain carried, and a `From` that picks it:
//!
//! | the domain says | the refusal is |
//! |---|---|
//! | [`RevokeError::RevokeOthers`] | [`Refusal::RevokeOthers`] |
//! | [`RevokeError::NotRevocable`], [`JudgementError::NotRevocable`] | [`Refusal::NotRevocable`] |
//! | [`LeakRefusal::Leaks`] | [`Refusal::Leaks`] |
//! | [`LeakRefusal::Uncheckable`], [`PublicationError::LeakUncheckable`] | [`Refusal::LeakUncheckable`] |
//! | [`PublicationError::DestinationTaken`] | [`Refusal::PublishTargetExists`] |
//! | [`PublicationError::EvidenceStale`] | [`Refusal::EvidenceStale`] |
//! | any other [`RevokeError`], [`JudgementError`], [`AdoptionRefusal`], [`PublicationError`] | [`Refusal::Revoke`], [`Refusal::Judgement`], [`Refusal::Adoption`], [`Refusal::Publication`] |
//!
//! The leaks of a publication stay inside [`Refusal::Publication`], because that
//! error also says what to do about them
//! ([`LeakRemedy`](crate::domain::publication::LeakRemedy)): taking the remedy
//! away would leave the next step without its choice between fixing and aborting.

use std::convert::Infallible;

use thiserror::Error;

use crate::domain::approval::Gate;
use crate::domain::error::{
    AdoptionRefusal, JudgementError, LeakRefusal, PublicationError, RevokeError,
};
use crate::domain::hash::{BytesHash, TextHash};
use crate::domain::id::DocId;
use crate::domain::judge::Judge;
use crate::domain::judgement::{JudgementId, Target};
use crate::domain::knowable::Knowable;
use crate::domain::lint::CheckViolation;
use crate::domain::location::RelPath;
use crate::domain::naming::{Named, Ref, Rename, UnresolvedWhy};
use crate::domain::nonempty::NonEmptyVec;
use crate::domain::publication::ConfirmDigest;

use crate::domain::boundary::Leak;

use super::error::AppError;
use super::ports::SecretHit;

/// What a use case gives back. See the module documentation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome<T> {
    /// What was asked for: the new state and the files written for a write, the
    /// value read for a read.
    Done(T),
    /// A gate's machine check found these. Nothing was written.
    Violations(NonEmptyVec<CheckViolation>),
    /// A precondition does not hold. Nothing was written.
    Refused(Refusal),
    /// The name given matches nothing, or several.
    NotFound {
        /// The reference as given.
        query: Ref,
        /// How it missed.
        miss: Miss,
    },
}

/// How a name that could be looked up here did not name one thing.
///
/// The two are different answers for the person: a name that matches nothing may be a
/// name that used to belong to something (the hint), and a name that matches several
/// is a choice to make (the candidates).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Miss {
    /// Nothing has that name here.
    Unmatched {
        /// What the name used to belong to, when an earlier version says. `Unknown`
        /// when the history that would say could not be read here: that is no
        /// statement that no earlier name fits.
        hint: Knowable<Option<Rename>>,
    },
    /// Several fit, and none is chosen.
    Ambiguous(NonEmptyVec<Named>),
}

/// An outcome that stopped a use case: any outcome but [`Outcome::Done`], which has no
/// value to carry. [`widen`](Outcome::widen) makes it the outcome of a use case of any
/// value, so a step that can stop a use case does not have to know what it returns.
pub type Stopped = Outcome<Infallible>;

impl Outcome<Infallible> {
    /// The same outcome as the outcome of a use case that returns a `T`.
    pub fn widen<T>(self) -> Outcome<T> {
        match self {
            Self::Done(never) => match never {},
            Self::Violations(violations) => Outcome::Violations(violations),
            Self::Refused(refusal) => Outcome::Refused(refusal),
            Self::NotFound { query, miss } => Outcome::NotFound { query, miss },
        }
    }
}

impl<T> Outcome<T> {
    /// Whether the use case did what was asked.
    pub fn is_done(&self) -> bool {
        matches!(self, Self::Done(_))
    }

    /// The value, when the use case did what was asked.
    pub fn done(self) -> Option<T> {
        match self {
            Self::Done(value) => Some(value),
            Self::Violations(_) | Self::Refused(_) | Self::NotFound { .. } => None,
        }
    }

    /// The same outcome with the value changed; the other cases are untouched.
    pub fn map<U>(self, change: impl FnOnce(T) -> U) -> Outcome<U> {
        match self {
            Self::Done(value) => Outcome::Done(change(value)),
            Self::Violations(violations) => Outcome::Violations(violations),
            Self::Refused(refusal) => Outcome::Refused(refusal),
            Self::NotFound { query, miss } => Outcome::NotFound { query, miss },
        }
    }
}

impl<T> From<Refusal> for Outcome<T> {
    fn from(refusal: Refusal) -> Self {
        Self::Refused(refusal)
    }
}

/// The class of an [`Outcome`]: the only thing a surface decides its exit from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ExitClass {
    /// The use case did what was asked.
    Success,
    /// A gate's check found violations.
    Violations,
    /// A precondition did not hold.
    Refused,
    /// A name matched nothing, or several.
    NotFound,
    /// The tool is wrong, not the person: the parts of a record did not belong
    /// together ([`AppError::Internal`]).
    Internal,
    /// A port failed ([`AppError::Port`]), or another error of the tool.
    Port,
}

impl ExitClass {
    /// Whether a surface reports this as a failure (the tool server's `isError`; a
    /// non-zero exit of the command line).
    pub fn is_error(self) -> bool {
        match self {
            Self::Success => false,
            Self::Violations | Self::Refused | Self::NotFound | Self::Internal | Self::Port => true,
        }
    }
}

/// The one table from what a use case returned to its class. See the module
/// documentation.
pub fn exit_class<T>(result: &Result<Outcome<T>, AppError>) -> ExitClass {
    match result {
        Ok(Outcome::Done(_)) => ExitClass::Success,
        Ok(Outcome::Violations(_)) => ExitClass::Violations,
        Ok(Outcome::Refused(_)) => ExitClass::Refused,
        Ok(Outcome::NotFound { .. }) => ExitClass::NotFound,
        Err(AppError::Internal(_)) => ExitClass::Internal,
        // The other errors are the event-log use cases' own (a value that was
        // refused, a reference that names nothing there). None of them is an answer,
        // so a surface reports each as the tool's failure.
        Err(_) => ExitClass::Port,
    }
}

/// Why a use case did not start: a precondition that does not hold.
///
/// The domain's own refusals are wrapped, not restated (see the module
/// documentation); the variants without a domain source are the application's.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum Refusal {
    /// The working tree has uncommitted changes. Fix: commit first.
    #[error("the working tree is not clean: commit first")]
    DirtyTree,
    /// The record is another judge's, and the revocation was not forced.
    #[error("{}", RevokeError::RevokeOthers { owner: owner.clone() })]
    RevokeOthers {
        /// Whose record it is.
        owner: Judge,
    },
    /// Text that is to be written where others read it names private documents.
    #[error("{}", LeakRefusal::Leaks(.0.clone()))]
    Leaks(NonEmptyVec<Leak>),
    /// The leak check cannot run, so nothing is written to the public side.
    #[error("{}", LeakRefusal::Uncheckable)]
    LeakUncheckable,
    /// A secret was found in a text about to be stored. The report names the rule
    /// and the line, never the secret.
    #[error("a secret was found in text about to be stored ({} hit(s))", .0.len())]
    Secret(NonEmptyVec<SecretHit>),
    /// The record cannot be revoked: only an approval, an adoption or a retirement
    /// can.
    #[error("{}", RevokeError::NotRevocable { record: *.0 })]
    NotRevocable(JudgementId),
    /// A file stands at the public destination already.
    #[error("{}", PublicationError::DestinationTaken { to: (.0).clone() })]
    PublishTargetExists(RelPath),
    /// The person's settings name a private root, and it cannot be read.
    #[error("the private root is set and cannot be read")]
    PrivateRootAbsent,
    /// An agent may not take an earlier decision in as an approval.
    #[error("an agent cannot adopt: taking a decision in is a person's act")]
    AgentCannotAdopt,
    /// An agent may not stand in for the person who confirms an operation that
    /// cannot be undone.
    #[error("an agent cannot confirm an operation that cannot be undone")]
    AgentCannotConfirm,
    /// The operation cannot be undone and needs a person's confirmation, and this
    /// surface has no person to ask.
    #[error("this operation cannot be undone and needs a person's confirmation at a terminal")]
    NeedsHumanConfirm,
    /// The person was asked, and said no.
    #[error("the person asked did not confirm")]
    ConfirmDeclined,
    /// The plan changed between the confirmation and the run. Confirm again.
    #[error("the plan changed since it was confirmed: confirmed {confirmed}, now {now}")]
    ConfirmationStale {
        /// The digest the person confirmed.
        confirmed: ConfirmDigest,
        /// The digest of the plan made again just before the run.
        now: ConfirmDigest,
    },
    /// A publication of this document stopped part way and is open.
    #[error("a publication of {0} is in progress: resume it or abort it")]
    PublishInProgress(DocId),
    /// The version was purged on purpose; its text is not put back.
    #[error("the text {0} was purged and is not stored again")]
    ObjectPurged(TextHash),
    /// The file has more than one hard link, and replacing it would cut them.
    #[error("{0} has more than one hard link")]
    MultipleLinks(RelPath),
    /// The path is a symbolic link or not a regular file.
    #[error("{0} is not a regular file")]
    NotRegularFile(RelPath),
    /// The file's group cannot be kept on the replacement, and the group has access.
    #[error("the group of {0} cannot be kept on a replacement")]
    CannotKeepGroup(RelPath),
    /// The file's owner cannot be kept on the replacement. A person can fix it (the
    /// file is another user's, and this one may not give a file to its owner), so it is
    /// a refusal, like the group's.
    #[error("the owner of {0} cannot be kept on a replacement")]
    CannotKeepOwner(RelPath),
    /// Whether the working tree is clean cannot be told here (not a work tree, or the
    /// tool that would say failed to answer), so it is not said to be dirty either.
    #[error("whether the working tree is clean cannot be told here")]
    WorkTreeUnknown,
    /// The name given cannot be looked up here, which is not the same as nothing
    /// having that name.
    #[error("`{query}` cannot be looked up here: {why}")]
    CannotNameHere {
        /// The reference as given.
        query: Ref,
        /// Why it cannot be looked up.
        why: UnresolvedWhy,
    },
    /// The gate does not judge what the name points at: CR judges a Finding, CS and
    /// CM a Spec, CI a Design and CA an item.
    #[error("{} does not judge {target}", gate.name())]
    WrongTarget {
        /// The gate.
        gate: Gate,
        /// What the name points at.
        target: Target,
    },
    /// No place for an `id:` was found in the file, so it is left as it was.
    #[error("no place for an `id:` was found in {0}")]
    CannotPlaceId(RelPath),
    /// The file's content when it was written is not the content it had when it was
    /// compared. Read it again and plan again.
    #[error("{0} changed while it was being written")]
    ChangedUnderfoot(RelPath),
    /// The bytes checked are not the bytes of the file that would be copied.
    #[error("{}", PublicationError::EvidenceStale { expected: *expected, found: *found })]
    EvidenceStale {
        /// The bytes the corpus has for the file.
        expected: BytesHash,
        /// The bytes that were given to be checked.
        found: BytesHash,
    },
    /// A revocation was refused for a reason with no variant of its own.
    #[error(transparent)]
    Revoke(RevokeError),
    /// A judgement could not be built or placed.
    #[error(transparent)]
    Judgement(JudgementError),
    /// A past decision could not be taken in.
    #[error(transparent)]
    Adoption(AdoptionRefusal),
    /// A publication cannot be planned.
    #[error(transparent)]
    Publication(PublicationError),
}

impl Refusal {
    /// The refusal for the secrets found in a text; `None` when there are none.
    pub fn secret(hits: Vec<SecretHit>) -> Option<Self> {
        NonEmptyVec::from_vec(hits).ok().map(Self::Secret)
    }
}

impl From<RevokeError> for Refusal {
    fn from(error: RevokeError) -> Self {
        match error {
            RevokeError::RevokeOthers { owner } => Self::RevokeOthers { owner },
            RevokeError::NotRevocable { record } => Self::NotRevocable(record),
            RevokeError::NothingToRevoke
            | RevokeError::UnknownRecord { .. }
            | RevokeError::AlreadyRevoked { .. } => Self::Revoke(error),
        }
    }
}

impl From<JudgementError> for Refusal {
    fn from(error: JudgementError) -> Self {
        match error {
            JudgementError::NotRevocable { record } => Self::NotRevocable(record),
            JudgementError::ObjectNotOfTarget { .. }
            | JudgementError::ObjectNotChecked { .. }
            | JudgementError::ProofRestored
            | JudgementError::TargetPresent { .. }
            | JudgementError::RecordNotInPublicLedger { .. }
            | JudgementError::RecordNotOfDoc { .. }
            | JudgementError::RecordRevoked { .. }
            | JudgementError::UnknownTarget { .. }
            | JudgementError::AmbiguousTarget { .. }
            | JudgementError::UnknownRecord { .. }
            | JudgementError::RevokeSpansTargets => Self::Judgement(error),
        }
    }
}

impl From<AdoptionRefusal> for Refusal {
    fn from(error: AdoptionRefusal) -> Self {
        Self::Adoption(error)
    }
}

impl From<LeakRefusal> for Refusal {
    fn from(error: LeakRefusal) -> Self {
        match error {
            LeakRefusal::Uncheckable => Self::LeakUncheckable,
            LeakRefusal::Leaks(leaks) => Self::Leaks(leaks),
        }
    }
}

impl From<PublicationError> for Refusal {
    fn from(error: PublicationError) -> Self {
        match error {
            PublicationError::LeakUncheckable => Self::LeakUncheckable,
            PublicationError::DestinationTaken { to } => Self::PublishTargetExists(to),
            PublicationError::EvidenceStale { expected, found } => {
                Self::EvidenceStale { expected, found }
            }
            PublicationError::NotFound { .. }
            | PublicationError::NoneOpen { .. }
            | PublicationError::AlreadyRecorded { .. }
            | PublicationError::AlreadyPublic { .. }
            | PublicationError::DocInvalid { .. }
            | PublicationError::DestinationDiffers { .. }
            | PublicationError::ObjectUnavailable { .. }
            | PublicationError::Leaks { .. } => Self::Publication(error),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::num::NonZeroU32;

    use super::*;
    use crate::app::commit::record::ChainError;
    use crate::app::ports::PortName;
    use crate::domain::boundary::{CrossingField, CrossingText, Leak, PrivateNames, PrivateRoot};
    use crate::domain::id::test_support::docid;
    use crate::domain::judge::test_support::judge;
    use crate::domain::judgement::test_support::jid;
    use crate::domain::nonempty::NonBlank;
    use crate::domain::publication::LeakRemedy;

    fn violation() -> NonEmptyVec<CheckViolation> {
        NonEmptyVec::new(CheckViolation::EmptyBody, Vec::new())
    }

    fn query() -> Ref {
        Ref::Id(docid(1))
    }

    fn unmatched() -> Miss {
        Miss::Unmatched {
            hint: Knowable::Unknown,
        }
    }

    #[test]
    fn each_result_has_one_class_and_only_success_is_not_an_error() {
        let internal = AppError::Internal(ChainError::NoObject);
        let port = AppError::Port {
            port: PortName::Ledger,
            source: Box::new(std::fmt::Error),
        };
        let table: [(Result<Outcome<u8>, AppError>, ExitClass, bool); 6] = [
            (Ok(Outcome::Done(1)), ExitClass::Success, false),
            (
                Ok(Outcome::Violations(violation())),
                ExitClass::Violations,
                true,
            ),
            (
                Ok(Outcome::Refused(Refusal::DirtyTree)),
                ExitClass::Refused,
                true,
            ),
            (
                Ok(Outcome::NotFound {
                    query: query(),
                    miss: unmatched(),
                }),
                ExitClass::NotFound,
                true,
            ),
            (Err(internal), ExitClass::Internal, true),
            (Err(port), ExitClass::Port, true),
        ];
        for (result, class, error) in table {
            assert_eq!(exit_class(&result), class);
            assert_eq!(class.is_error(), error);
        }
    }

    #[test]
    fn an_error_of_the_event_log_use_cases_is_the_tools_failure_too() {
        let result: Result<Outcome<u8>, AppError> = Err(AppError::NoActor);
        assert_eq!(exit_class(&result), ExitClass::Port);
    }

    #[test]
    fn widening_a_stopped_outcome_keeps_what_stopped_it() {
        let violations: Stopped = Outcome::Violations(violation());
        assert_eq!(violations.widen::<u8>(), Outcome::Violations(violation()));
        let refused: Stopped = Outcome::Refused(Refusal::DirtyTree);
        assert_eq!(
            refused.widen::<String>(),
            Outcome::Refused(Refusal::DirtyTree)
        );
        let missing: Stopped = Outcome::NotFound {
            query: query(),
            miss: unmatched(),
        };
        assert_eq!(
            missing.widen::<u8>(),
            Outcome::NotFound {
                query: query(),
                miss: unmatched()
            }
        );
    }

    #[test]
    fn a_refusal_can_stand_for_an_outcome_of_any_value() {
        let outcome: Outcome<String> = Refusal::NeedsHumanConfirm.into();
        assert_eq!(outcome, Outcome::Refused(Refusal::NeedsHumanConfirm));
        assert!(!outcome.is_done());
    }

    #[test]
    fn map_changes_the_value_and_leaves_the_other_cases() {
        assert_eq!(Outcome::Done(2).map(|n| n * 2), Outcome::Done(4));
        let refused: Outcome<u8> = Outcome::Refused(Refusal::DirtyTree);
        assert_eq!(refused.map(|n| n + 1), Outcome::Refused(Refusal::DirtyTree));
        assert_eq!(Outcome::Done(5).done(), Some(5));
        assert_eq!(Outcome::<u8>::Refused(Refusal::DirtyTree).done(), None);
    }

    #[test]
    fn revocation_refusals_pick_their_own_variant_and_wrap_the_rest() {
        let owner = judge("bob");
        assert_eq!(
            Refusal::from(RevokeError::RevokeOthers {
                owner: owner.clone()
            }),
            Refusal::RevokeOthers { owner }
        );
        assert_eq!(
            Refusal::from(RevokeError::NotRevocable { record: jid(1) }),
            Refusal::NotRevocable(jid(1))
        );
        assert_eq!(
            Refusal::from(JudgementError::NotRevocable { record: jid(2) }),
            Refusal::NotRevocable(jid(2))
        );
        assert_eq!(
            Refusal::from(RevokeError::NothingToRevoke),
            Refusal::Revoke(RevokeError::NothingToRevoke)
        );
        assert_eq!(
            Refusal::from(JudgementError::RevokeSpansTargets),
            Refusal::Judgement(JudgementError::RevokeSpansTargets)
        );
    }

    #[test]
    fn the_message_is_the_domains_own() {
        let record = jid(3);
        let domain = RevokeError::NotRevocable { record };
        assert_eq!(
            Refusal::from(domain.clone()).to_string(),
            domain.to_string()
        );
        let to: RelPath = "docs/a.md".parse().expect("a path");
        let taken = PublicationError::DestinationTaken { to };
        assert_eq!(Refusal::from(taken.clone()).to_string(), taken.to_string());
        let wrapped = Refusal::from(RevokeError::NothingToRevoke);
        assert_eq!(
            wrapped.to_string(),
            RevokeError::NothingToRevoke.to_string()
        );
    }

    #[test]
    fn a_publication_keeps_its_remedy_when_it_refuses_for_leaks() {
        let leaks = one_leak();
        let error = PublicationError::Leaks {
            leaks: leaks.clone(),
            remedy: LeakRemedy::AbortPublish,
        };
        assert_eq!(
            Refusal::from(error.clone()),
            Refusal::Publication(error.clone())
        );
        assert_eq!(
            Refusal::from(LeakRefusal::Leaks(leaks.clone())),
            Refusal::Leaks(leaks)
        );
    }

    #[test]
    fn a_leak_check_that_cannot_run_is_one_refusal_whoever_says_so() {
        assert_eq!(
            Refusal::from(LeakRefusal::Uncheckable),
            Refusal::LeakUncheckable
        );
        assert_eq!(
            Refusal::from(PublicationError::LeakUncheckable),
            Refusal::LeakUncheckable
        );
    }

    #[test]
    fn publication_errors_with_a_variant_of_their_own_lose_nothing() {
        let to: RelPath = "docs/a.md".parse().expect("a path");
        assert_eq!(
            Refusal::from(PublicationError::DestinationTaken { to: to.clone() }),
            Refusal::PublishTargetExists(to)
        );
        let expected = BytesHash::of(b"a");
        let found = BytesHash::of(b"b");
        assert_eq!(
            Refusal::from(PublicationError::EvidenceStale { expected, found }),
            Refusal::EvidenceStale { expected, found }
        );
        let other = PublicationError::NotFound { doc: docid(9) };
        assert_eq!(Refusal::from(other.clone()), Refusal::Publication(other));
    }

    #[test]
    fn a_publication_with_nothing_open_to_abort_is_wrapped_as_it_is() {
        let none_open = PublicationError::NoneOpen { doc: docid(9) };
        assert_eq!(
            Refusal::from(none_open.clone()),
            Refusal::Publication(none_open)
        );
    }

    #[test]
    fn what_a_person_can_fix_in_a_file_or_a_tree_has_a_refusal_that_says_so() {
        let at: RelPath = "docs/a.md".parse().expect("a path");
        assert_eq!(
            Refusal::CannotKeepOwner(at.clone()).to_string(),
            "the owner of docs/a.md cannot be kept on a replacement"
        );
        // A tree that cannot be asked about is not said to be dirty.
        assert_ne!(
            Refusal::WorkTreeUnknown.to_string(),
            Refusal::DirtyTree.to_string()
        );
        assert!(
            !Refusal::WorkTreeUnknown
                .to_string()
                .contains("commit first")
        );
        // A name that cannot be looked up says why, and is not a miss.
        let query: Ref = "private:0192f3a4-5b6c-7d8e-8f90-000000000001"
            .parse()
            .expect("a reference");
        let refusal = Refusal::CannotNameHere {
            query: query.clone(),
            why: UnresolvedWhy::PrivateNotHere,
        };
        assert!(refusal.to_string().contains(&query.to_string()));
        assert!(
            refusal
                .to_string()
                .contains(&UnresolvedWhy::PrivateNotHere.to_string())
        );
    }

    #[test]
    fn a_gate_that_does_not_judge_the_target_and_a_file_with_no_place_for_an_id_say_so() {
        use crate::domain::id::test_support::docid;
        let wrong = Refusal::WrongTarget {
            gate: Gate::Cs,
            target: Target::Doc(docid(1)),
        };
        assert!(wrong.to_string().starts_with("CS does not judge"));
        let at: RelPath = "docs/a.md".parse().expect("a path");
        assert_eq!(
            Refusal::CannotPlaceId(at).to_string(),
            "no place for an `id:` was found in docs/a.md"
        );
    }

    #[test]
    fn a_confirmation_that_went_stale_names_both_digests() {
        use crate::domain::publication::ConfirmDigest;
        let refusal = Refusal::ConfirmationStale {
            confirmed: ConfirmDigest::Abort(
                crate::app::commit::fixtures::Aborting::of(b"x")
                    .plan
                    .digest(),
            ),
            now: ConfirmDigest::Abort(
                crate::app::commit::fixtures::Aborting::of(b"other")
                    .plan
                    .digest(),
            ),
        };
        assert!(refusal.to_string().starts_with("the plan changed since"));
    }

    #[test]
    fn secrets_make_a_refusal_only_when_there_are_some() {
        assert_eq!(Refusal::secret(Vec::new()), None);
        let hit = SecretHit::new(
            NonBlank::new("rule").expect("a rule"),
            NonZeroU32::new(1).expect("a line"),
        );
        let got = Refusal::secret(vec![hit.clone()]);
        assert_eq!(
            got,
            Some(Refusal::Secret(NonEmptyVec::new(hit, Vec::new())))
        );
    }

    fn one_leak() -> NonEmptyVec<Leak> {
        use std::collections::BTreeSet;

        use crate::domain::corpus::Entity;
        use crate::domain::finding::test_support::finding;
        use crate::domain::judgement::test_support::{Filed, corpus_of};
        use crate::domain::knowable::Knowable;

        let corpus = corpus_of(vec![Filed::private(Entity::Finding(finding(1, "body")))]);
        let Knowable::Known(names) =
            PrivateNames::of(&corpus, PrivateRoot::Present, &BTreeSet::new())
        else {
            panic!("the private root is present");
        };
        let mut text = CrossingText::default();
        text.push(CrossingField::Body, "see slow login");
        NonEmptyVec::from_vec(names.leaks_in(&text)).expect("a private title is a leak")
    }
}
