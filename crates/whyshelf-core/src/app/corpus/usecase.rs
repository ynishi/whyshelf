//! From a next step to the use case that does it: the one table the command line and
//! the tool server share.
//!
//! The domain says what waits on a person as a [`NextAction`], and holds no command
//! text. A surface turns the action into its own grammar through [`UseCase`], which is
//! the use cases of this module one for one, and [`use_case_for`] is the `match` that
//! has to name a use case for every action: a new action that no use case answers does
//! not compile.
//!
//! | [`NextAction`] | [`UseCase`] | the function |
//! |---|---|---|
//! | `Edit(for)` | `EditFile { hint }` | none: the person edits the file, the tool does not run it |
//! | `Import` | `Import` | [`import`](super::import) |
//! | `Approve(gate)`, `Reapprove(gate)` | `Approve(gate)` | [`approve`](super::approve) |
//! | `Measure` | `Approve(Gate::Cm)` | [`approve`](super::approve) |
//! | `Adopt` | `Adopt` | [`adopt`](fn@super::adopt) |
//! | `Retire` | `Retire` | [`retire`](super::retire) |
//! | `ResumePublish` | `Publish` | [`publish`](fn@super::publish) |
//! | `AbortPublish` | `AbortPublish` | [`abort_publish`](super::abort_publish) |
//!
//! `Publish` has no flag for "resume": whether a run starts a publication or finishes
//! one that stopped is the plan's, made from what the corpus shows.
//!
//! That a use case, run from the state that produced its action, succeeds is not a thing
//! a type can promise. The tests of this module run each action's use case from such a
//! state and require [`Outcome::Done`](crate::app::outcome::Outcome::Done).
//!
//! # The file of an edit
//!
//! An [`NextAction::Edit`] is about a file, and the action does not carry it: the step's
//! target does. [`edit_path`] is the one place that turns a target into the path the
//! person is to edit.

use crate::domain::approval::Gate;
use crate::domain::corpus::{Corpus, Entity};
use crate::domain::location::RelPath;
use crate::domain::plan_next::{EditFor, EntityRef, NextAction};

/// What does a next step: a use case of this module, or the person's own edit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UseCase {
    /// The person edits a file. The tool does not run it.
    EditFile {
        /// What the edit is for.
        hint: EditFor,
    },
    /// Write an `id:` where there is none.
    Import,
    /// Run a gate and approve.
    Approve(Gate),
    /// Take an earlier decision in.
    Adopt,
    /// Record that something whose file is gone is gone on purpose.
    Retire,
    /// Publish a document, or finish a publication that stopped.
    Publish,
    /// Undo a publication that stopped before its commit point.
    AbortPublish,
}

/// The use case that does a next step's action. See the module documentation.
pub fn use_case_for(action: &NextAction) -> UseCase {
    match action {
        NextAction::Edit(hint) => UseCase::EditFile { hint: hint.clone() },
        NextAction::Import => UseCase::Import,
        NextAction::Approve(gate) | NextAction::Reapprove(gate) => UseCase::Approve(*gate),
        NextAction::Measure => UseCase::Approve(Gate::Cm),
        NextAction::Adopt => UseCase::Adopt,
        NextAction::Retire => UseCase::Retire,
        NextAction::ResumePublish => UseCase::Publish,
        NextAction::AbortPublish => UseCase::AbortPublish,
    }
}

/// The file a step's target is in, which an edit is made to.
///
/// `None` when the corpus has no file for the target (a document whose file is gone is
/// restored from where it was, not edited).
pub fn edit_path(target: &EntityRef, corpus: &Corpus) -> Option<RelPath> {
    match target {
        EntityRef::Doc(doc) => corpus
            .by_id(doc)
            .first()
            .map(|file| file.location().path().clone())
            .or_else(|| {
                corpus
                    .unparsed()
                    .iter()
                    .find(|file| file.id() == Some(*doc))
                    .map(|file| file.facts().location().path().clone())
            }),
        EntityRef::Item(item) => {
            corpus
                .documents()
                .iter()
                .find_map(|file| match file.document().entity() {
                    Entity::Design(design)
                        if design.items().values().any(|held| held.id() == Some(*item)) =>
                    {
                        Some(file.location().path().clone())
                    }
                    Entity::Design(_) | Entity::Finding(_) | Entity::Spec(_) => None,
                })
        }
        EntityRef::File(location) => Some(location.path().clone()),
    }
}

#[cfg(test)]
mod tests;
