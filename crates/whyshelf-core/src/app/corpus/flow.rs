//! What can end a use case before it has a value, and the one place that turns it
//! into what the caller sees.
//!
//! A step of a use case can stop it in three ways: the answer is "no" (a refusal, a
//! violation, a name that matches nothing), a port failed, or the parts of a record did
//! not belong together. The first is an [`Outcome`], the other two are an
//! [`AppError`]. Writing each use case so that every step returns `Result<_, Flow>`
//! lets `?` carry all of them, and [`finish`] is the one function that says which is
//! which:
//!
//! | a step stopped with | the use case returns |
//! |---|---|
//! | a [`Refusal`], a domain refusal that has one, [`Violations`], a [`Stopped`] | `Ok(`[`Outcome`]`)` |
//! | [`Stop::Refused`] | `Ok(`[`Outcome::Refused`]`)` |
//! | [`Stop::Port`], an [`AppError`] | `Err(`[`AppError`]`)` |
//! | [`Stop::Broken`], a [`ChainError`] | `Err(`[`AppError::Internal`]`)` |

use crate::app::commit::Stop;
use crate::app::commit::record::ChainError;
use crate::app::error::AppError;
use crate::app::outcome::{Outcome, Refusal, Stopped};
use crate::domain::error::{AdoptionRefusal, JudgementError, PublicationError, RevokeError};
use crate::domain::gate::Violations;

/// Why a step of a use case stopped it.
#[derive(Debug)]
pub(super) enum Flow {
    /// The answer is not a value: it is an outcome to give back.
    Stopped(Box<Stopped>),
    /// The tool failed.
    Failed(AppError),
}

/// The result of a use case, from the result of its steps.
pub(super) fn finish<T>(steps: Result<T, Flow>) -> Result<Outcome<T>, AppError> {
    match steps {
        Ok(value) => Ok(Outcome::Done(value)),
        Err(Flow::Stopped(stopped)) => Ok(stopped.widen()),
        Err(Flow::Failed(error)) => Err(error),
    }
}

impl From<Stopped> for Flow {
    fn from(stopped: Stopped) -> Self {
        Self::Stopped(Box::new(stopped))
    }
}

impl From<AppError> for Flow {
    fn from(error: AppError) -> Self {
        Self::Failed(error)
    }
}

impl From<Refusal> for Flow {
    fn from(refusal: Refusal) -> Self {
        Self::from(Stopped::Refused(refusal))
    }
}

impl From<Violations> for Flow {
    fn from(violations: Violations) -> Self {
        Self::from(Stopped::Violations(violations))
    }
}

impl From<Stop> for Flow {
    fn from(stop: Stop) -> Self {
        match stop {
            Stop::Refused(refusal) => Self::from(refusal),
            Stop::Port(error) => Self::Failed(error),
            Stop::Broken(broken) => Self::from(broken),
        }
    }
}

impl From<ChainError> for Flow {
    fn from(broken: ChainError) -> Self {
        Self::Failed(AppError::Internal(broken))
    }
}

impl From<JudgementError> for Flow {
    fn from(error: JudgementError) -> Self {
        Self::from(Refusal::from(error))
    }
}

impl From<RevokeError> for Flow {
    fn from(error: RevokeError) -> Self {
        Self::from(Refusal::from(error))
    }
}

impl From<AdoptionRefusal> for Flow {
    fn from(error: AdoptionRefusal) -> Self {
        Self::from(Refusal::from(error))
    }
}

impl From<PublicationError> for Flow {
    fn from(error: PublicationError) -> Self {
        Self::from(Refusal::from(error))
    }
}

#[cfg(test)]
mod tests;
