//! Who decides, and who ran the command.
//!
//! The name that goes into a judgement's `by`. It is taken as declared, like a
//! Git author: from the configuration or from the command line. There is no
//! authentication in the model, so who may decide
//! ([`Authority`](super::shelf::schema::Authority)) is a rule about this name as it
//! is written, not about who wrote it.
//!
//! A record names one who answers for it (the actor) and, when another ran the
//! command, that one too ([`Attribution`]).

use std::fmt;

use thiserror::Error;

use super::error::ValueError;

const ACTOR_MAX: usize = 128;

/// A declared name: of a person who approves, reviews or revokes, or of the program
/// that ran the command for one.
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

/// Who answers for a record, and, when it is another, who ran the command.
///
/// Both names are as declared: nothing here tells that the one who ran the command
/// is who it says. The actor is the name every rule reads (who may decide, who has
/// read, whose run it was); `ran_by` is only told, so that a record made by a
/// program for a person keeps both. A name is never kept twice: when the one who ran
/// the command is the one who answers for it, the record has one name, which is
/// the only form a record of that case has, written and read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Attribution {
    actor: Actor,
    ran_by: Option<Actor>,
}

/// `ran_by` and the actor are one name, which an [`Attribution`] holds once.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[error("`{0}` ran the command and answers for it: one name")]
pub struct SameName(pub Actor);

impl Attribution {
    /// A record whose actor ran the command too.
    pub fn one(actor: Actor) -> Self {
        Self {
            actor,
            ran_by: None,
        }
    }

    /// A record `actor` answers for and `ran_by` ran, who is another: the one place
    /// two names are compared (as the names are, trimmed by [`Actor::new`]).
    pub fn ran_for(actor: Actor, ran_by: Actor) -> Result<Self, SameName> {
        if actor == ran_by {
            return Err(SameName(actor));
        }
        Ok(Self {
            actor,
            ran_by: Some(ran_by),
        })
    }

    /// The attribution of a command that `ran_by` ran, if anyone was declared to:
    /// one name when there is none, or when it is the actor's own.
    pub fn new(actor: Actor, ran_by: Option<Actor>) -> Self {
        match ran_by {
            None => Self::one(actor),
            Some(ran_by) => {
                Self::ran_for(actor, ran_by).unwrap_or_else(|SameName(actor)| Self::one(actor))
            }
        }
    }

    /// Who answers for the record.
    pub fn actor(&self) -> &Actor {
        &self.actor
    }

    /// Who ran the command, when it is not the actor.
    pub fn ran_by(&self) -> Option<&Actor> {
        self.ran_by.as_ref()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn name(text: &str) -> Actor {
        Actor::new(text).unwrap()
    }

    #[test]
    fn actor_is_one_short_trimmed_line() {
        assert_eq!(Actor::new("  alice ").unwrap().as_str(), "alice");
        assert_eq!(Actor::new("  "), Err(ValueError::ActorEmpty));
        assert_eq!(Actor::new("a\nb"), Err(ValueError::ActorBadShape));
        assert_eq!(Actor::new(&"a".repeat(129)), Err(ValueError::ActorBadShape));
    }

    #[test]
    fn another_who_ran_the_command_is_kept_beside_the_actor() {
        let by = Attribution::ran_for(name("ana"), name("bot")).unwrap();
        assert_eq!(by.actor(), &name("ana"));
        assert_eq!(by.ran_by(), Some(&name("bot")));
        assert_eq!(Attribution::new(name("ana"), Some(name("bot"))), by);
    }

    #[test]
    fn the_same_name_is_one_name() {
        assert_eq!(
            Attribution::ran_for(name("ana"), name("ana")),
            Err(SameName(name("ana")))
        );
        let one = Attribution::one(name("ana"));
        assert_eq!(one.ran_by(), None);
        assert_eq!(Attribution::new(name("ana"), Some(name("ana"))), one);
        assert_eq!(Attribution::new(name("ana"), None), one);
    }

    #[test]
    fn names_are_compared_trimmed() {
        // `Actor::new` trims, so a name with spaces around it is the same name.
        assert_eq!(
            Attribution::new(name("ana"), Some(name("  ana "))),
            Attribution::one(name("ana"))
        );
    }
}
