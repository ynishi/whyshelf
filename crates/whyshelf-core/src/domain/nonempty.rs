//! Values that cannot be empty: a list with at least one element, a text with
//! at least one visible character.
//!
//! The rule "a Spec without grounds is refused" is held by a type, not by a
//! check somebody must remember to call.

use std::collections::BTreeSet;
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

/// A set with at least one element.
///
/// Why: a fingerprint of a change that holds no hunk says nothing, and a set of
/// pins is only worth naming while it is not empty. Like [`NonEmptyVec`], the empty
/// case cannot be represented; unlike it, each element is held once, in its own
/// order.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct NonEmptySet<T: Ord>(BTreeSet<T>);

impl<T: Ord> NonEmptySet<T> {
    /// A set from the elements given, refused when there are none.
    pub fn from_set(items: BTreeSet<T>) -> Result<Self, ValueError> {
        if items.is_empty() {
            Err(ValueError::EmptyList)
        } else {
            Ok(Self(items))
        }
    }

    /// The number of elements; at least 1.
    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// Always `false`; present so the type reads like a collection.
    pub fn is_empty(&self) -> bool {
        false
    }

    /// The elements, smallest first.
    pub fn iter(&self) -> impl Iterator<Item = &T> {
        self.0.iter()
    }

    /// Whether `item` is in the set.
    pub fn contains(&self, item: &T) -> bool {
        self.0.contains(item)
    }

    /// The elements as a plain set.
    pub fn as_set(&self) -> &BTreeSet<T> {
        &self.0
    }
}

impl<T: Ord + ToCanon> ToCanon for NonEmptySet<T> {
    fn to_canon(&self) -> Canon {
        Canon::list(self.0.iter().map(ToCanon::to_canon))
    }
}

/// A list with at least two elements.
///
/// Why: a choice between alternatives is only a choice when there are two; one
/// alternative is the step itself. The type says so, so a view never has to print
/// "choose one of: a".
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct TwoOrMore<T> {
    first: T,
    second: T,
    rest: Vec<T>,
}

impl<T> TwoOrMore<T> {
    /// A list of two elements followed by more.
    pub fn new(first: T, second: T, rest: Vec<T>) -> Self {
        Self {
            first,
            second,
            rest,
        }
    }

    /// The number of elements; at least 2.
    pub fn len(&self) -> usize {
        2 + self.rest.len()
    }

    /// Always `false`; present so the type reads like a collection.
    pub fn is_empty(&self) -> bool {
        false
    }

    /// The elements in order.
    pub fn iter(&self) -> impl Iterator<Item = &T> {
        [&self.first, &self.second]
            .into_iter()
            .chain(self.rest.iter())
    }

    /// The elements as a vector.
    pub fn into_vec(self) -> Vec<T> {
        let mut out = Vec::with_capacity(2 + self.rest.len());
        out.push(self.first);
        out.push(self.second);
        out.extend(self.rest);
        out
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
    fn an_empty_set_is_refused_and_the_elements_are_kept_once_in_order() {
        assert_eq!(
            NonEmptySet::<u8>::from_set(BTreeSet::new()),
            Err(ValueError::EmptyList)
        );
        let set = NonEmptySet::from_set(BTreeSet::from([3, 1, 3, 2])).unwrap();
        assert_eq!(set.len(), 3);
        assert!(!set.is_empty());
        assert!(set.contains(&2) && !set.contains(&9));
        assert_eq!(set.iter().copied().collect::<Vec<_>>(), vec![1, 2, 3]);
    }

    #[test]
    fn two_or_more_keeps_the_order_and_has_no_shorter_form() {
        let two = TwoOrMore::new('a', 'b', vec![]);
        assert_eq!(two.len(), 2);
        assert!(!two.is_empty());
        assert_eq!(two.iter().copied().collect::<String>(), "ab");
        let three = TwoOrMore::new('a', 'b', vec!['c']);
        assert_eq!(three.len(), 3);
        assert_eq!(three.into_vec(), vec!['a', 'b', 'c']);
    }

    #[test]
    fn a_blank_text_is_refused() {
        assert_eq!(NonBlank::new(""), Err(ValueError::BlankText));
        assert_eq!(NonBlank::new(" \n\t"), Err(ValueError::BlankText));
        assert_eq!(NonBlank::new(" a ").unwrap().as_str(), " a ");
    }
}
