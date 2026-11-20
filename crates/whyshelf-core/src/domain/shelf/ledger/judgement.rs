//! The identity and the time of a judgement: [`JudgementId`], a UUID v7 the caller
//! supplies, and [`JudgedAt`], unix milliseconds with an order.

use std::fmt;

use crate::domain::id::EntityId;

/// The identity of a record: a UUID v7 the caller supplies.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct JudgementId(EntityId);

impl JudgementId {
    /// An identity from an identifier.
    pub fn new(id: EntityId) -> Self {
        Self(id)
    }

    /// The identifier.
    pub fn entity(&self) -> EntityId {
        self.0
    }
}

impl fmt::Display for JudgementId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(&self.0, f)
    }
}

/// When a judgement was made, in unix milliseconds. The caller supplies it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct JudgedAt(i64);

impl JudgedAt {
    /// A time from unix milliseconds.
    pub fn new(millis: i64) -> Self {
        Self(millis)
    }

    /// The unix milliseconds.
    pub fn millis(&self) -> i64 {
        self.0
    }

    /// An approval that expires at this time has lapsed by `at`: it lapses at the
    /// moment itself. The one rule of when an expiry has passed, read by the check of
    /// a new decision, by freshness and by what a new approval is.
    pub fn lapsed_by(self, at: JudgedAt) -> bool {
        self <= at
    }
}
