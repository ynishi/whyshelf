//! Who decides.
//!
//! The name that goes into a judgement's `by`. It is taken as declared, like a
//! Git author: from the configuration or from the command line. There is no
//! authentication in the model.

use std::fmt;

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
}
