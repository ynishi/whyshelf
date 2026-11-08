//! The use cases of the corpus and the ledgers: what a person asks of the tool, as free
//! functions over the ports.
//!
//! Each is `async fn use_case(ports: &P, ..command) -> Result<Outcome<T>, AppError>`.
//! `P` is [`CorpusRead`](super::ports::CorpusRead) for the ones that only read and
//! [`CorpusPorts`](super::ports::CorpusPorts) for the ones that write; neither is the
//! earlier model's [`Ports`](super::ports::Ports). The command is a type, the answer is
//! an [`Outcome`](super::outcome::Outcome) (done, the violations a gate found, a refusal,
//! or a name that matches nothing), and an `Err` is a failure of a port or of the tool.
//!
//! ```text
//!                       read only: P: CorpusRead             writes: P: CorpusPorts
//!   status ..........  Snapshot
//!   check ...........  Snapshot -> gate machine layer
//!
//!   approve (CR CS CI CA CM)  Snapshot -> gate -> Judgement::approve ---+
//!   adopt ................... Snapshot -> adoption::check -> confirm ----+
//!   revoke  retire  purge ... Snapshot -> Ledgers::revoke | Judgement::* +--> split -> scan -> store -> append
//!   import .................. Snapshot -> FileWriter::rewrite (+ adopt)
//!   publish  abort_publish .. Snapshot -> plan_publication | plan_abort -> confirm -> steps
//! ```
//!
//! # Where each judgement is made
//!
//! A use case collects and calls. It decides nothing that has an owner:
//!
//! | judgement | owner |
//! |---|---|
//! | what a reference names | [`naming`](crate::domain::naming)'s index and resolver, through [`Snapshot::resolve`] |
//! | what a lookup's answer means to the person | [`outcome_of`], the one table |
//! | the version of the text a gate read | [`CorpusDocument::text`](crate::domain::corpus::CorpusDocument::text) |
//! | the state of everything | [`evaluate_corpus`](crate::domain::corpus_eval::evaluate_corpus) |
//! | which CS approval a measurement is set against | [`Ledgers::cs_fixing`](crate::domain::ledger::Ledgers::cs_fixing) |
//! | what an approval records of the change | [`gate::ca::check`](crate::domain::gate::ca::check) |
//! | whether the working tree is clean | `gates::clean_tree` (one place, in the CA approval) |
//! | which ledger a record goes to | [`JudgementKind::ledger`](crate::domain::judgement::JudgementKind::ledger) |
//! | whether a person must confirm | the plan's `confirmation()` ([`Adoptable`](crate::domain::adoption::Adoptable), [`PublicationPlan`](crate::domain::publication::PublicationPlan), [`AbortPlan`](crate::domain::publication::AbortPlan)) |
//! | what a surface exits with | [`exit_class`](super::outcome::exit_class) |
//! | which use case does a next step | [`use_case_for`] |
//!
//! # What stops a use case
//!
//! A step that stops a use case does so with a `Flow`, and the one function that turns it
//! into what the caller sees keeps the three kinds apart: a refusal, a violation or a
//! name that matches nothing is an `Ok` outcome; a failed port is an
//! [`AppError::Port`](super::error::AppError::Port); and parts of a record that do not
//! belong together ([`Stop::Broken`](super::commit::Stop::Broken)) are an
//! [`AppError::Internal`](super::error::AppError::Internal), never an outcome, because
//! the person cannot fix them.
//!
//! # Known limitations
//!
//! - The histories that explain a rename (the aliases a document had, the numbers an
//!   item had) are not read by any port, so a name that matches nothing carries a hint
//!   that is `Unknown`, never "no earlier name fits" ([`Miss::Unmatched`](super::outcome::Miss::Unmatched)).
//! - The keys of a person are an empty keyring where no place to look for them exists.
//! - A revocation and the retirement of an item are not held back by a publication cut
//!   short: neither has a document this layer can look up (a record does not name its
//!   document, and a retired item has no file, so no Design leads to it). Every other
//!   judgement on a document is held back.

mod adopt;
mod flow;
mod gates;
mod idline;
mod judgements;
mod name;
mod publish;
mod snapshot;
mod usecase;
mod write;

#[cfg(test)]
#[path = "corpus/fixtures_tests.rs"]
pub(crate) mod fixtures;

pub use adopt::{Adopt, AdoptAfter, Adopted, Import, Imported, adopt, import};
pub use gates::{Approve, Approved, Check, CheckPassed, GateCommand, Measured, approve, check};
pub use judgements::{
    Purge, Purged, Retire, Retired, Revoke, RevokeBy, Revoked, purge, retire, revoke,
};
pub use name::{home_of, outcome_of};
pub use publish::{AbortPublish, Aborted, Publish, Published, abort_publish, publish};
pub use snapshot::{Snapshot, status};
pub use usecase::{UseCase, edit_path, use_case_for};
