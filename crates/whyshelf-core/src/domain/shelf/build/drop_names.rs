//! The names `fmt --drop` takes out of the documents, and whether they fit the
//! declaration.
//!
//! What a person asked is a [`DropAsk`]: names, each a text that says something, as
//! given. [`DropNames::new`] checks them against the declaration once and answers a
//! [`DropNames`] or every way they do not fit ([`DropProblem`]);
//! [`Shelf::form_entries`](super::Shelf::form_entries) then leaves out, from the front
//! matter of a file of a book, the entries it lists that the book does not declare. A caller reads arguments into an ask and passes the
//! checked names on; it decides neither.
//!
//! ```text
//!   DropAsk ──► DropNames::new(&Schema) ──► DropNames ──► Shelf::form_entries(.., &DropNames)
//!    (names,         │                                       ──► FormEntries (and which names
//!     as given)      └─► every DropProblem                         it left out: dropped())
//! ```
//!
//! # The rule
//!
//! - **A name is dropped from a file only where its book does not declare it.** The
//!   test is the one the build reads an entry by (`Declared::of`, private): the name
//!   is none of the book's fields, links and links to artifacts, so the build tells the
//!   entry as a [`DocProblem::UnknownEntry`](super::DocProblem::UnknownEntry). Such an entry is dropped whatever kind of entry the
//!   name was, and whatever it holds. An entry of a book that declares the name stays,
//!   and so does every entry the ask does not list.
//! - **What fits the declaration**: a name that is not `id` or `key`
//!   ([`DropProblem::Reserved`]: every document has them, and the build tells a file
//!   without either), and that some book does not declare (a name every book declares
//!   would drop nothing anywhere and is the sign of a declaration not taken out yet,
//!   [`DropProblem::DeclaredEverywhere`]). A name no book declares is no problem: it
//!   is what `fmt --drop` is for, and the documents that do not hold it lose nothing.
//!   Every problem is collected.

use std::collections::BTreeSet;

use thiserror::Error;

use crate::domain::nonempty::{NonBlank, NonEmptyVec};

use super::Declared;
use crate::domain::shelf::schema::{ID_ENTRY, KEY_ENTRY, Schema};

/// The names a person asked `fmt` to drop, as given and before they are checked
/// against the declaration. A caller that reads arguments makes this and nothing more,
/// so the rule of what they mean is one, in [`DropNames`].
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct DropAsk {
    /// The names, each a text that says something, each once.
    pub names: BTreeSet<NonBlank>,
}

/// A [`DropAsk`] checked against the declaration: the names [`Shelf::form_entries`]
/// leaves out of the files of the books that do not declare them.
///
/// It can be made by [`DropNames::new`] and [`DropNames::none`] only, so that no `id`
/// or `key` is ever dropped, and no caller drops a name it did not check.
///
/// [`Shelf::form_entries`]: super::Shelf::form_entries
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DropNames(BTreeSet<NonBlank>);

/// Why the names asked to be dropped do not fit the declaration.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum DropProblem {
    /// The name is `id` or `key`. Every document has them, and a file that holds
    /// neither is invalid, so there is nothing to retire.
    #[error("`{0}` cannot be dropped: every document has an `id` and a `key`")]
    Reserved(NonBlank),
    /// Every book declares the name, so it would be dropped from no file. A name is
    /// dropped once the declarations that hold it are taken out.
    #[error(
        "every book declares `{0}`, so nothing would be dropped: take it out of the declaration of the books first"
    )]
    DeclaredEverywhere(NonBlank),
}

impl DropNames {
    /// No name: what `new` asks of [`Shelf::form_entries`](super::Shelf::form_entries),
    /// and `fmt` without `--drop`.
    pub fn none() -> Self {
        Self(BTreeSet::new())
    }

    /// The names, or every way the ask does not fit the declaration (the page says
    /// what fits). All the problems are collected, in the order of the names, so one
    /// run tells a person all they have to mend.
    pub fn new(schema: &Schema, ask: DropAsk) -> Result<Self, NonEmptyVec<DropProblem>> {
        let mut problems = Vec::new();
        for name in &ask.names {
            if name.as_str() == ID_ENTRY || name.as_str() == KEY_ENTRY {
                problems.push(DropProblem::Reserved(name.clone()));
            } else if !schema.books().is_empty()
                && schema
                    .books()
                    .values()
                    .all(|book| Declared::of(book, name.as_str()).is_some())
            {
                problems.push(DropProblem::DeclaredEverywhere(name.clone()));
            }
        }
        match NonEmptyVec::from_vec(problems) {
            Ok(problems) => Err(problems),
            Err(_) => Ok(Self(ask.names)),
        }
    }

    /// The name as asked, when `name` is one of those to drop.
    pub(super) fn listed(&self, name: &str) -> Option<&NonBlank> {
        self.0.get(name)
    }

    /// The names to drop.
    pub fn names(&self) -> &BTreeSet<NonBlank> {
        &self.0
    }

    /// Whether no name is to be dropped.
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}
