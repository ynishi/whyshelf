//! The four answers of a lookup, each to its one outcome.

use super::*;
use crate::domain::id::test_support::docid;
use crate::domain::knowable::Knowable;
use crate::domain::naming::{Rename, UnresolvedWhy};
use crate::domain::nonempty::NonEmptyVec;

fn query() -> Ref {
    Ref::Alias("spec/cache".parse().expect("an alias"))
}

#[test]
fn a_name_that_is_resolved_goes_on() {
    let named = Named::Doc(docid(1));
    assert_eq!(
        outcome_of(&query(), Resolution::Resolved(named.clone())),
        Ok(named)
    );
}

#[test]
fn a_name_that_matches_nothing_is_not_found_with_the_hint_the_lookup_gave() {
    for hint in [
        Knowable::Unknown,
        Knowable::Known(None),
        Knowable::Known(Some(Rename::Alias {
            was: "spec/cache".parse().expect("an alias"),
            now: "spec/cache-v2".parse().expect("an alias"),
            doc: docid(1),
        })),
    ] {
        assert_eq!(
            outcome_of(&query(), Resolution::Dangling { hint: hint.clone() }),
            Err(Outcome::NotFound {
                query: query(),
                miss: Miss::Unmatched { hint },
            })
        );
    }
}

#[test]
fn a_name_that_matches_several_lists_them_and_chooses_none() {
    let candidates = NonEmptyVec::new(Named::Doc(docid(1)), vec![Named::Doc(docid(2))]);
    assert_eq!(
        outcome_of(&query(), Resolution::Ambiguous(candidates.clone())),
        Err(Outcome::NotFound {
            query: query(),
            miss: Miss::Ambiguous(candidates),
        })
    );
}

#[test]
fn a_name_that_cannot_be_looked_up_here_is_a_refusal_not_a_miss() {
    for why in [
        UnresolvedWhy::PrivateNotHere,
        UnresolvedWhy::External,
        UnresolvedWhy::Url,
        UnresolvedWhy::ItemWithoutId,
    ] {
        assert_eq!(
            outcome_of(&query(), Resolution::Unresolved(why)),
            Err(Outcome::Refused(Refusal::CannotNameHere {
                query: query(),
                why,
            }))
        );
    }
}

#[test]
fn a_document_belongs_to_itself() {
    // An item belongs to the Design that holds it; the snapshot's tests look an item up
    // and check it.
    assert_eq!(home_of(&Named::Doc(docid(5))), docid(5));
}
