//! What stops a use case is told apart in one place.

use super::*;
use crate::app::commit::Stop;
use crate::app::outcome::{ExitClass, Miss, exit_class};
use crate::app::ports::PortName;
use crate::domain::error::{AdoptionRefusal, JudgementError, PublicationError, RevokeError};
use crate::domain::id::test_support::docid;
use crate::domain::judgement::Target;
use crate::domain::knowable::Knowable;
use crate::domain::lint::CheckViolation;
use crate::domain::naming::Ref;
use crate::domain::nonempty::NonEmptyVec;

fn violations() -> Violations {
    NonEmptyVec::new(CheckViolation::EmptyBody, Vec::new())
}

fn port_error() -> AppError {
    AppError::port(PortName::Ledger)(std::fmt::Error)
}

fn finished(steps: Result<u8, Flow>) -> Result<Outcome<u8>, AppError> {
    finish(steps)
}

#[test]
fn a_step_that_went_through_is_done() {
    assert_eq!(finished(Ok(3)).expect("an outcome"), Outcome::Done(3));
}

#[test]
fn a_refusal_a_violation_and_a_miss_are_outcomes_not_errors() {
    let refused = finished(Err(Refusal::DirtyTree.into())).expect("an outcome");
    assert_eq!(refused, Outcome::Refused(Refusal::DirtyTree));

    let found = finished(Err(violations().into())).expect("an outcome");
    assert_eq!(found, Outcome::Violations(violations()));

    let query = Ref::Id(docid(1));
    let missing = finished(Err(Stopped::NotFound {
        query: query.clone(),
        miss: Miss::Unmatched {
            hint: Knowable::Unknown,
        },
    }
    .into()))
    .expect("an outcome");
    assert_eq!(
        missing,
        Outcome::NotFound {
            query,
            miss: Miss::Unmatched {
                hint: Knowable::Unknown
            }
        }
    );
}

#[test]
fn a_stop_is_told_by_what_it_is() {
    // A precondition that does not hold is an outcome.
    let refused = finished(Err(Stop::Refused(Refusal::NeedsHumanConfirm).into()));
    assert_eq!(
        refused.as_ref().expect("an outcome"),
        &Outcome::Refused(Refusal::NeedsHumanConfirm)
    );
    assert_eq!(exit_class(&refused), ExitClass::Refused);

    // A port that failed stays an error, with the cause kept.
    let failed = finished(Err(Stop::Port(port_error()).into()));
    assert!(matches!(failed, Err(AppError::Port { .. })));
    assert_eq!(exit_class(&failed), ExitClass::Port);

    // The parts of a record that do not belong together are the tool's mistake: an
    // internal error, never an outcome.
    let broken = finished(Err(Stop::Broken(ChainError::NoObject).into()));
    assert!(matches!(
        broken,
        Err(AppError::Internal(ChainError::NoObject))
    ));
    assert_eq!(exit_class(&broken), ExitClass::Internal);
}

#[test]
fn an_error_of_the_tool_is_carried_as_it_is() {
    let failed = finished(Err(port_error().into()));
    assert!(matches!(failed, Err(AppError::Port { .. })));
    let internal = finished(Err(ChainError::KeepsObject.into()));
    assert!(matches!(
        internal,
        Err(AppError::Internal(ChainError::KeepsObject))
    ));
}

#[test]
fn what_the_domain_refused_is_a_refusal_with_its_own_variant() {
    let target = Target::Doc(docid(1));
    let judgement = finished(Err(JudgementError::UnknownTarget { target }.into()));
    assert_eq!(
        judgement.expect("an outcome"),
        Outcome::Refused(Refusal::Judgement(JudgementError::UnknownTarget { target }))
    );
    let revoke = finished(Err(RevokeError::NothingToRevoke.into()));
    assert_eq!(
        revoke.expect("an outcome"),
        Outcome::Refused(Refusal::Revoke(RevokeError::NothingToRevoke))
    );
    let adoption = finished(Err(AdoptionRefusal::TargetUnknown { target }.into()));
    assert_eq!(
        adoption.expect("an outcome"),
        Outcome::Refused(Refusal::Adoption(AdoptionRefusal::TargetUnknown { target }))
    );
    let publication = finished(Err(PublicationError::NotFound { doc: docid(1) }.into()));
    assert_eq!(
        publication.expect("an outcome"),
        Outcome::Refused(Refusal::Publication(PublicationError::NotFound {
            doc: docid(1)
        }))
    );
}
