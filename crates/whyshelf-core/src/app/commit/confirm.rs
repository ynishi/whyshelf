//! A person's confirmation of what cannot be undone, as a value that can be had only
//! from the person.
//!
//! Some operations cannot be undone, and each needs a person to confirm the plan
//! before it runs: the first step of a publication (it puts the document's id and the
//! hashes of the versions it approved where Git keeps them), taking earlier
//! decisions in, removing a private copy whose bytes differ from the public file, and
//! aborting a publication whose public file says what the private one does not. Whether
//! a plan asks is the plan's to say (its `confirmation()`); what is asked is a
//! [`ConfirmDigest`].
//!
//! ```text
//!   ConfirmPort::ask(&ConfirmPrompt)
//!        |  Yes          --> HumanConfirmation { digest asked about }
//!        |  No           --> Refusal::ConfirmDeclined
//!        |  Unavailable  --> Refusal::NeedsHumanConfirm
//!
//!   an agent is refused before it is asked: Refusal::AgentCannotConfirm
//!
//!   just before the run: the plan is made again, and
//!   HumanConfirmation::holds_for(&new digest)  a plan that changed needs a new answer
//! ```
//!
//! A flag, an argument or a [`Principal`] never stands in for the answer. The
//! [`Principal`] is a declaration kept for the record and the display; it does not
//! make a confirmation, and a person who is not declared (`Undeclared`) still has to
//! answer.
//!
//! ```compile_fail,E0451
//! use whyshelf_core::app::commit::confirm::HumanConfirmation;
//! use whyshelf_core::domain::publication::ConfirmDigest;
//!
//! // The confirmation has a private field: it is not made by naming a digest.
//! fn forged(digest: ConfirmDigest) -> HumanConfirmation {
//!     HumanConfirmation { digest }
//! }
//! ```
//!
//! ```compile_fail,E0624
//! use whyshelf_core::app::commit::confirm::HumanConfirmation;
//! use whyshelf_core::domain::publication::ConfirmDigest;
//!
//! fn forged(digest: ConfirmDigest) {
//!     let _ = HumanConfirmation::new(digest);
//! }
//! ```
//!
//! Known limitation: [`ConfirmPort`] is a public trait, so a library user can write an
//! implementation that always says yes. That is choosing to skip the question, and it
//! is what the confirmation is built against (a slip), not an agent that is set on
//! getting through.

use crate::app::commit::Stop;
use crate::app::error::AppError;
use crate::app::outcome::Refusal;
use crate::app::ports::{ConfirmPort, ConfirmPrompt, Confirmed, PortName};
use crate::domain::judge::{Judge, Principal};
use crate::domain::publication::ConfirmDigest;

/// A person confirmed this plan.
///
/// Made only by [`confirm`], from [`Confirmed::Yes`] to a prompt about the plan. It
/// holds the digest the person was asked about, and it holds for a run only if the
/// plan made again just before the run has the same digest ([`holds_for`](Self::holds_for)).
/// The digest is a [`ConfirmDigest`], so a confirmation of a publication does not hold for
/// an adoption or an abort, whatever the bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HumanConfirmation {
    digest: ConfirmDigest,
}

impl HumanConfirmation {
    /// A confirmation of the plan with this digest.
    pub(in crate::app::commit) fn new(digest: ConfirmDigest) -> Self {
        Self { digest }
    }

    /// The digest the person confirmed.
    pub fn digest(&self) -> ConfirmDigest {
        self.digest
    }

    /// Whether the confirmation still holds for the plan as it is now.
    ///
    /// Call it with the digest of the plan made again just before the run: a plan that
    /// changed since the question (a file edited in between, a record added) is not
    /// the one the person confirmed.
    pub fn holds_for(&self, now: &ConfirmDigest) -> Result<(), Refusal> {
        if self.digest == *now {
            Ok(())
        } else {
            Err(Refusal::ConfirmationStale {
                confirmed: self.digest,
                now: *now,
            })
        }
    }
}

/// The step that asks a person and makes a [`HumanConfirmation`] of a yes.
///
/// `by` is who is acting. An agent is refused before anything is asked
/// ([`Refusal::AgentCannotConfirm`]): it is not there to answer.
pub async fn confirm<C: ConfirmPort>(
    port: &C,
    by: &Judge,
    prompt: &ConfirmPrompt,
) -> Result<HumanConfirmation, Stop> {
    if matches!(by.principal(), Principal::Agent { .. }) {
        return Err(Stop::Refused(Refusal::AgentCannotConfirm));
    }
    let answer = port
        .ask(prompt)
        .await
        .map_err(AppError::port(PortName::Confirm))?;
    match answer {
        Confirmed::Yes => Ok(HumanConfirmation::new(prompt.digest())),
        Confirmed::No => Err(Stop::Refused(Refusal::ConfirmDeclined)),
        Confirmed::Unavailable => Err(Stop::Refused(Refusal::NeedsHumanConfirm)),
    }
}

#[cfg(test)]
mod tests;
