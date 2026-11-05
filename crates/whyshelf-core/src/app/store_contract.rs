//! What every [`StorePort`] keeps about events, as a check an implementation runs.
//!
//! The contract of [`append`](StorePort::append) and [`events`](StorePort::events)
//! is a matter of meaning, not of a file layout: which two appends are the same
//! record, what a reader gets back when one event was recorded at two times, and
//! that a judgement by two people is two events. An in-memory fake and a store on a
//! disk must agree on it, or a use case tested against the fake says nothing about
//! the store it runs against. So the contract is written once, here, as
//! [`check_append_and_events`], and each implementation runs that same function on a
//! store with nothing in it: this crate's tests run it on the fake, and the storage
//! adapter's tests run it on the store on disk.
//!
//! A broken rule is returned as a [`ContractError`], not raised as a panic, because
//! this is library code that another crate's tests call.
//!
//! ```text
//!   fresh store
//!     append(e, t1)        Written          the record is new
//!     append(e, t1)        AlreadyPresent   same identifier, same time
//!     append(e, t2)        Written          same identifier, another time: beside it
//!     events()             [ e at {t1, t2} ]   one event, the times in digest order
//!     other scope          untouched
//!     two approvals        two events       one per person
//! ```

use std::collections::BTreeSet;
use std::fmt::Display;

use crate::domain::actor::{Actor, Timestamp};
use crate::domain::approval::{Attestation, Pinned};
use crate::domain::event::{
    DocKind, DocRegistration, Event, EventBody, EventId, Recorded, Registration, StoreKind, Version,
};
use crate::domain::hash::ContentHash;
use crate::domain::id::{EntityId, FindingId};

use super::ports::{AppendOutcome, StorePort};

/// Why a store did not pass the contract.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ContractError {
    /// The store was not empty when the check started.
    #[error("the check needs a store with no events in it")]
    NotFresh,
    /// A rule of the contract does not hold; the text says which.
    #[error("the store breaks its contract: {0}")]
    Broken(&'static str),
    /// The store failed while the check was using it.
    #[error("the store failed during the check: {0}")]
    Store(String),
    /// A value the check builds for itself was refused; this is a fault of the check.
    #[error("the check could not build its own data: {0}")]
    Fixture(String),
}

fn ensure(holds: bool, rule: &'static str) -> Result<(), ContractError> {
    if holds {
        Ok(())
    } else {
        Err(ContractError::Broken(rule))
    }
}

fn store<T, E: Display>(result: Result<T, E>) -> Result<T, ContractError> {
    result.map_err(|error| ContractError::Store(error.to_string()))
}

fn fixture<T, E: Display>(result: Result<T, E>) -> Result<T, ContractError> {
    result.map_err(|error| ContractError::Fixture(error.to_string()))
}

fn entity(text: &str) -> Result<EntityId, ContractError> {
    fixture(text.parse())
}

fn at(text: &str) -> Result<Timestamp, ContractError> {
    fixture(Timestamp::from_validated(text.to_owned()))
}

fn registration(entity_id: EntityId, name: &str) -> Result<Event, ContractError> {
    let body = EventBody::Registered(Registration::Doc(DocRegistration::new(
        DocKind::Spec,
        fixture(format!("docs/spec/{name}.md").parse())?,
        None,
        None,
        BTreeSet::new(),
        Version::Unparsed {
            raw: ContentHash::raw(name.as_bytes()),
        },
    )));
    fixture(Event::new(entity_id, BTreeSet::new(), body))
}

fn approval(finding: EntityId, by: &str, heads: &[EventId]) -> Result<Event, ContractError> {
    let attestation = Attestation::Cr {
        finding: Pinned::new(FindingId::new(finding), ContentHash::raw(b"finding")),
    };
    let by = fixture(Actor::new(by))?;
    fixture(Event::new(
        finding,
        heads.iter().copied().collect(),
        EventBody::Approved { by, attestation },
    ))
}

async fn read<S: StorePort>(
    store_under_test: &S,
    scope: StoreKind,
) -> Result<Vec<Recorded>, ContractError> {
    store(store_under_test.events(scope).await)
}

async fn put<S: StorePort>(
    store_under_test: &S,
    scope: StoreKind,
    event: &Event,
    time: &Timestamp,
) -> Result<AppendOutcome, ContractError> {
    store(store_under_test.append(scope, event, time).await)
}

/// Run the contract of `append` and `events` on a store that has no events.
///
/// The store keeps the contract when every append and every read below answers as
/// the picture in the module documentation says. It returns the first rule that does
/// not hold. Nothing here depends on how the store keeps its records, only on what
/// [`StorePort`] promises.
pub async fn check_append_and_events<S: StorePort>(
    store_under_test: &S,
) -> Result<(), ContractError> {
    let repo = StoreKind::Repo;
    let private = StoreKind::Private;
    ensure(
        read(store_under_test, repo).await?.is_empty()
            && read(store_under_test, private).await?.is_empty(),
        "a store with nothing written has no events",
    )
    .map_err(|_| ContractError::NotFresh)?;

    let event = registration(entity("0192e3a4-7b5c-7d3e-8f01-23456789abcd")?, "a")?;
    // One instant written with two offsets: two texts, so two records. A store that
    // read the times as instants would take them for one.
    let first = at("2026-11-04T12:00:00Z")?;
    let second = at("2026-11-04T21:00:00+09:00")?;

    ensure(
        put(store_under_test, repo, &event, &first).await? == AppendOutcome::Written,
        "a record that is new is written",
    )?;
    ensure(
        put(store_under_test, repo, &event, &first).await? == AppendOutcome::AlreadyPresent,
        "the same identifier at the same time is already present",
    )?;
    ensure(
        read(store_under_test, repo).await? == vec![Recorded::new(event.clone(), first.clone())],
        "the event is read back once, with its one time",
    )?;

    ensure(
        put(store_under_test, repo, &event, &second).await? == AppendOutcome::Written,
        "the same identifier at another time is a further record",
    )?;
    ensure(
        put(store_under_test, repo, &event, &first).await? == AppendOutcome::AlreadyPresent,
        "a record stays present after another is added",
    )?;
    let mut both = Recorded::new(event.clone(), first.clone());
    fixture(both.merge(Recorded::new(event.clone(), second.clone())))?;
    ensure(
        read(store_under_test, repo).await? == vec![both.clone()],
        "one event with both times, in the order of the digests of their records",
    )?;

    ensure(
        read(store_under_test, private).await?.is_empty(),
        "the other store is not touched",
    )?;
    ensure(
        put(store_under_test, private, &event, &first).await? == AppendOutcome::Written,
        "the same record in the other store is new there",
    )?;
    ensure(
        read(store_under_test, repo).await? == vec![both],
        "a write to the other store does not change this one",
    )?;

    let other = registration(entity("0192e3a4-7b5c-7d3e-8f01-23456789abce")?, "b")?;
    ensure(
        put(store_under_test, repo, &other, &first).await? == AppendOutcome::Written,
        "another event is written",
    )?;
    ensure(
        read(store_under_test, repo).await?.len() == 2,
        "two events are read as two",
    )?;

    // A judgement is a fact about its person: the same attestation by two people is
    // two events, each with its record.
    let finding = entity("0192e3a4-7b5c-7d3e-8f01-23456789abcf")?;
    let by_alice = approval(finding, "alice", &[])?;
    let by_bob = approval(finding, "bob", &[])?;
    ensure(
        by_alice.id() != by_bob.id(),
        "two people approving the same attestation make two events",
    )?;
    for approved in [&by_alice, &by_bob] {
        ensure(
            put(store_under_test, repo, approved, &first).await? == AppendOutcome::Written,
            "each person's approval is written",
        )?;
    }
    let ids: BTreeSet<EventId> = read(store_under_test, repo)
        .await?
        .iter()
        .map(|recorded| recorded.event().id())
        .collect();
    ensure(
        ids.contains(&by_alice.id()) && ids.contains(&by_bob.id()) && ids.len() == 4,
        "both approvals are read back",
    )?;
    Ok(())
}
