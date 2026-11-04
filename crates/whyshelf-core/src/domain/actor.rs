//! People and time as the domain sees them.
//!
//! # Actor
//!
//! The name that goes into an approval's `by`. It is taken as declared, like a
//! Git author: from the configuration or from the command line. There is no
//! authentication in the model, and nothing checks that the approver is not the
//! implementer.
//!
//! # Time
//!
//! The domain never reads a clock. [`Timestamp`] is the time an event was
//! recorded, supplied by the application from its clock port; [`MeasuredAt`] is
//! the time a measurement was taken, copied from a Finding's metadata. Both are
//! RFC 3339 texts that an adapter has validated with a real date library, so the
//! domain holds them as opaque values. They are compared for equality only: two
//! RFC 3339 texts with different UTC offsets do not sort as text, and the model
//! never needs an order (event order comes from the event graph, and `at` is for
//! display).

use std::fmt;

use super::canon::{Canon, ToCanon};
use super::error::ValueError;

const ACTOR_MAX: usize = 128;

/// The name of a person who approves, reviews or revokes.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Actor(String);

impl Actor {
    /// A name, one short line, trimmed.
    pub fn new(name: &str) -> Result<Self, ValueError> {
        let name = name.trim();
        if name.is_empty() {
            return Err(ValueError::ActorEmpty);
        }
        if name.chars().count() > ACTOR_MAX || name.chars().any(char::is_control) {
            return Err(ValueError::ActorBadShape);
        }
        Ok(Self(name.to_owned()))
    }

    /// The name.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for Actor {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl ToCanon for Actor {
    fn to_canon(&self) -> Canon {
        Canon::str(self.0.as_str())
    }
}

fn check_rfc3339_shape(text: &str) -> Result<(), ValueError> {
    let bytes = text.as_bytes();
    let shaped = text.is_ascii()
        && bytes.len() >= 20
        && bytes[4] == b'-'
        && bytes[7] == b'-'
        && (bytes[10] == b'T' || bytes[10] == b't')
        && !text.contains(char::is_whitespace);
    if shaped {
        Ok(())
    } else {
        Err(ValueError::TimestampBadShape)
    }
}

macro_rules! rfc3339_value {
    ($(#[$meta:meta])* $name:ident) => {
        $(#[$meta])*
        #[derive(Debug, Clone, PartialEq, Eq, Hash)]
        pub struct $name(String);

        impl $name {
            /// Wrap a text that an adapter has validated as RFC 3339.
            ///
            /// Only the outline of the shape is checked here; the calendar
            /// rules are the adapter's.
            pub fn from_validated(text: String) -> Result<Self, ValueError> {
                check_rfc3339_shape(&text)?;
                Ok(Self(text))
            }

            /// The text as given.
            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(&self.0)
            }
        }

        impl ToCanon for $name {
            fn to_canon(&self) -> Canon {
                Canon::str(self.0.as_str())
            }
        }
    };
}

rfc3339_value!(
    /// The time an event was recorded, taken from the clock port.
    ///
    /// Display only: it never enters an event identifier and never decides
    /// anything.
    Timestamp
);
rfc3339_value!(
    /// The time a measurement was taken (`measured_data` Findings).
    ///
    /// Part of the Finding's content, so it is hashed.
    MeasuredAt
);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn actor_is_one_short_trimmed_line() {
        assert_eq!(Actor::new("  alice ").unwrap().as_str(), "alice");
        assert_eq!(Actor::new("  "), Err(ValueError::ActorEmpty));
        assert_eq!(Actor::new("a\nb"), Err(ValueError::ActorBadShape));
        assert_eq!(Actor::new(&"a".repeat(129)), Err(ValueError::ActorBadShape));
    }

    #[test]
    fn timestamps_keep_the_text_as_given() {
        let t = Timestamp::from_validated("2026-11-04T21:40:00+09:00".into()).unwrap();
        assert_eq!(t.as_str(), "2026-11-04T21:40:00+09:00");
        assert!(MeasuredAt::from_validated("2026-11-04T12:40:00Z".into()).is_ok());
    }

    #[test]
    fn timestamps_with_the_wrong_outline_are_refused() {
        for bad in [
            "",
            "yesterday",
            "2026-11-04",
            "2026/11/04T21:40:00Z",
            "2026-11-04 21:40:00Z extra",
        ] {
            assert_eq!(
                Timestamp::from_validated(bad.into()),
                Err(ValueError::TimestampBadShape),
                "{bad}"
            );
        }
    }
}
