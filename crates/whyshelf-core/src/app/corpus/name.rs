//! What a name given as an argument comes to: the one table.
//!
//! A lookup ([`Resolution`]) has four answers, and a use case must not answer the
//! person differently for the same one. This is the single mapping from the answer to
//! what a use case does next:
//!
//! | the lookup said | the use case |
//! |---|---|
//! | `Resolved(named)` | goes on with `named` |
//! | `Dangling { hint }` | stops with [`Outcome::NotFound`] and [`Miss::Unmatched`] (the hint is `Unknown` when no history could be read) |
//! | `Ambiguous(named)` | stops with [`Outcome::NotFound`] and [`Miss::Ambiguous`], the candidates listed |
//! | `Unresolved(why)` | stops with [`Refusal::CannotNameHere`]: the name cannot be looked up here, which is not the same as nothing having it |

use crate::app::outcome::{Miss, Outcome, Refusal, Stopped};
use crate::domain::id::DocId;
use crate::domain::naming::{Named, Ref, Resolution};

/// The document a thing belongs to: the document itself, or the Design that holds the
/// item. What a judgement about the thing is held back by (a publication cut short) and
/// what a publication moves are the document's.
pub fn home_of(named: &Named) -> DocId {
    match named {
        Named::Doc(doc) => *doc,
        Named::Item(item) => item.design(),
    }
}

/// The thing a reference names, or the outcome that stops the use case. See the module
/// documentation.
pub fn outcome_of(query: &Ref, resolution: Resolution<Named>) -> Result<Named, Stopped> {
    match resolution {
        Resolution::Resolved(named) => Ok(named),
        Resolution::Dangling { hint } => Err(Outcome::NotFound {
            query: query.clone(),
            miss: Miss::Unmatched { hint },
        }),
        Resolution::Ambiguous(candidates) => Err(Outcome::NotFound {
            query: query.clone(),
            miss: Miss::Ambiguous(candidates),
        }),
        Resolution::Unresolved(why) => Err(Outcome::Refused(Refusal::CannotNameHere {
            query: query.clone(),
            why,
        })),
    }
}

#[cfg(test)]
mod tests;
