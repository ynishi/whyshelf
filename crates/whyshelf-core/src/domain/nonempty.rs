//! Values that cannot be empty: a list with at least one element, a text with
//! at least one visible character.
//!
//! The rule "a Spec without grounds is refused" is held by a type, not by a
//! check somebody must remember to call.

use std::fmt;

use super::canon::{Canon, ToCanon};
use super::error::ValueError;

/// A list with at least one element.
///
/// Why: a Spec must name its Findings and a Crux must name a rejected option.
/// With this type the empty case is impossible to represent.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct NonEmptyVec<T> {
    head: T,
    tail: Vec<T>,
}

impl<T> NonEmptyVec<T> {
    /// A list of one element followed by more.
    pub fn new(head: T, tail: Vec<T>) -> Self {
        Self { head, tail }
    }

    /// A list from a vector, refused when the vector is empty.
    pub fn from_vec(items: Vec<T>) -> Result<Self, ValueError> {
        let mut iter = items.into_iter();
        let head = iter.next().ok_or(ValueError::EmptyList)?;
        Ok(Self {
            head,
            tail: iter.collect(),
        })
    }

    /// The first element.
    pub fn first(&self) -> &T {
        &self.head
    }

    /// The number of elements; at least 1.
    pub fn len(&self) -> usize {
        1 + self.tail.len()
    }

    /// Always `false`; present so the type reads like a collection.
    pub fn is_empty(&self) -> bool {
        false
    }

    /// The elements in order.
    pub fn iter(&self) -> impl Iterator<Item = &T> {
        std::iter::once(&self.head).chain(self.tail.iter())
    }

    /// The elements as a vector.
    pub fn into_vec(self) -> Vec<T> {
        let mut out = Vec::with_capacity(1 + self.tail.len());
        out.push(self.head);
        out.extend(self.tail);
        out
    }
}

impl<T: ToCanon> ToCanon for NonEmptyVec<T> {
    fn to_canon(&self) -> Canon {
        Canon::list(self.iter().map(ToCanon::to_canon))
    }
}

/// A free text that has at least one non-white-space character.
///
/// Used for the explanatory fields of an entity (an environment, a reason, a
/// summary), where any wording is allowed but silence is not. The text is kept
/// as given, including inner line breaks.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct NonBlank(String);

impl NonBlank {
    /// A text, refused when it is empty or only white space.
    pub fn new(text: impl Into<String>) -> Result<Self, ValueError> {
        let text = text.into();
        if text.trim().is_empty() {
            Err(ValueError::BlankText)
        } else {
            Ok(Self(text))
        }
    }

    /// The text.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for NonBlank {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl ToCanon for NonBlank {
    fn to_canon(&self) -> Canon {
        Canon::str(self.0.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_vec_is_refused() {
        assert_eq!(
            NonEmptyVec::<u8>::from_vec(vec![]),
            Err(ValueError::EmptyList)
        );
    }

    #[test]
    fn order_and_length_are_kept() {
        let v = NonEmptyVec::from_vec(vec![3, 1, 2]).unwrap();
        assert_eq!(v.len(), 3);
        assert!(!v.is_empty());
        assert_eq!(*v.first(), 3);
        assert_eq!(v.iter().copied().collect::<Vec<_>>(), vec![3, 1, 2]);
        assert_eq!(v.into_vec(), vec![3, 1, 2]);
    }

    #[test]
    fn a_blank_text_is_refused() {
        assert_eq!(NonBlank::new(""), Err(ValueError::BlankText));
        assert_eq!(NonBlank::new(" \n\t"), Err(ValueError::BlankText));
        assert_eq!(NonBlank::new(" a ").unwrap().as_str(), " a ");
    }
}
