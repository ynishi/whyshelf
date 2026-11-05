//! A fact that may not be possible to tell in this environment.
//!
//! Some facts depend on a tool or a place the program may not have: whether a
//! path is tracked by Git, say, when the project is not in a work tree. Such a
//! fact is not "false" and not "absent"; it is *unknown here*. [`Knowable`] keeps
//! that apart from `Option`, where `None` reads as "there is nothing", and from a
//! guessed default, which would turn an unanswered question into an answer.

/// A value, or the statement that it cannot be told here.
///
/// Why not `Option`: `None` says there is no value; [`Knowable::Unknown`] says
/// there is one and it could not be read. A rule that depends on the fact must
/// decide what an unknown answer means, and the type makes it visible that it has
/// to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Knowable<T> {
    /// The fact is known.
    Known(T),
    /// The fact cannot be told in this environment.
    Unknown,
}

impl<T> Knowable<T> {
    /// The value, if it is known.
    pub fn known(&self) -> Option<&T> {
        match self {
            Self::Known(value) => Some(value),
            Self::Unknown => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_gives_the_value_and_unknown_gives_none() {
        assert_eq!(Knowable::Known(3).known(), Some(&3));
        assert_eq!(Knowable::<u8>::Unknown.known(), None);
    }
}
