//! References between documents, and what a reference can turn out to be.
//!
//! # Kinds of reference
//!
//! A reference carries a mark that says where it points:
//!
//! | written | points to |
//! |---|---|
//! | an alias or an id, no mark | an entity in the same store |
//! | `private:<id>` | an entity in the private store |
//! | `ext:<project>/<id-or-alias>` | an entity of another project |
//! | a URL | an external document |
//!
//! # State of a reference
//!
//! [`RefState`] has three outcomes: **resolved** (the target is there),
//! **dangling** (a reference with no mark whose target does not exist, which
//! makes the referring entity invalid) and **unresolved** (a `private:` or `ext:`
//! reference that cannot be looked up in this environment, as on a CI machine or
//! in a public clone). An unresolved reference is shown, never an error: a public
//! clone must stay usable although it cannot see the private store.
//!
//! # Leak guard
//!
//! A public document may point at a private entity only by its opaque `private:`
//! identifier. Quoting its content, or copying its alias or title, is refused by
//! the leak lint (see `lint`). The reverse direction, private to public, is
//! free. Identifiers in the private store carry no meaning (UUID v7); the human
//! readable alias lives only inside the private store.
//!
//! # Item references
//!
//! An item is named `<design-ref>#<n>`. A Spec reference followed by `#<n>`
//! resolves only when exactly one Design refers to that Spec; with two or more it
//! is ambiguous and must be written with the Design.

use std::fmt;
use std::str::FromStr;

use super::canon::{Canon, ToCanon};
use super::error::{ItemRefError, ValueError};
use super::id::{Alias, DesignId, EntityId, ItemNumber};
use super::location::Uri;

/// The name of another project in an `ext:` reference.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ProjectName(String);

impl ProjectName {
    /// The name as text.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for ProjectName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl FromStr for ProjectName {
    type Err = ValueError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let ok = !s.is_empty()
            && s.chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || ".-_".contains(c));
        if ok {
            Ok(Self(s.to_owned()))
        } else {
            Err(ValueError::ProjectNameBadShape)
        }
    }
}

/// A reference without a mark: an identifier or an alias.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum LocalRef {
    /// A UUID v7.
    Id(EntityId),
    /// A human-readable name.
    Alias(Alias),
}

impl LocalRef {
    /// The identifier, when the reference is one.
    pub fn as_id(&self) -> Option<EntityId> {
        match self {
            Self::Id(id) => Some(*id),
            Self::Alias(_) => None,
        }
    }
}

impl FromStr for LocalRef {
    type Err = ValueError;

    /// Tries an identifier first, then an alias.
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.parse::<EntityId>() {
            Ok(id) => Ok(Self::Id(id)),
            Err(_) => s
                .parse::<Alias>()
                .map(Self::Alias)
                .map_err(|e| ValueError::RefLocalShape(Box::new(e))),
        }
    }
}

impl fmt::Display for LocalRef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Id(id) => fmt::Display::fmt(id, f),
            Self::Alias(alias) => fmt::Display::fmt(alias, f),
        }
    }
}

/// A reference as written in a file or on a command line.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Ref {
    /// An entity in the same store.
    Local(LocalRef),
    /// An entity in the private store, by its opaque identifier.
    Private(EntityId),
    /// An entity of another project.
    External {
        /// The other project.
        project: ProjectName,
        /// The entity inside it.
        target: LocalRef,
    },
    /// An external document.
    Url(Uri),
}

impl Ref {
    /// Whether this reference may fail to resolve without being an error.
    ///
    /// True for `private:` and `ext:` references.
    pub fn may_be_unresolved(&self) -> bool {
        matches!(self, Self::Private(_) | Self::External { .. })
    }
}

impl FromStr for Ref {
    type Err = ValueError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if s.is_empty() {
            return Err(ValueError::RefEmpty);
        }
        if let Some(rest) = s.strip_prefix("private:") {
            return rest
                .parse::<EntityId>()
                .map(Self::Private)
                .map_err(|e| ValueError::RefPrivateId(Box::new(e)));
        }
        if let Some(rest) = s.strip_prefix("ext:") {
            let (project, target) = rest.split_once('/').ok_or(ValueError::RefExternalShape)?;
            return Ok(Self::External {
                project: project.parse()?,
                target: target.parse()?,
            });
        }
        if s.contains("://") {
            return s.parse::<Uri>().map(Self::Url);
        }
        s.parse::<LocalRef>().map(Self::Local)
    }
}

impl fmt::Display for Ref {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Local(r) => fmt::Display::fmt(r, f),
            Self::Private(id) => write!(f, "private:{id}"),
            Self::External { project, target } => write!(f, "ext:{project}/{target}"),
            Self::Url(uri) => fmt::Display::fmt(uri, f),
        }
    }
}

impl ToCanon for Ref {
    fn to_canon(&self) -> Canon {
        Canon::str(self.to_string())
    }
}

/// What looking a reference up gave.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RefState {
    /// The target exists.
    Resolved,
    /// A reference with no mark whose target does not exist. Invalidates the
    /// entity that holds it.
    Dangling,
    /// A `private:` or `ext:` reference that cannot be looked up here. Shown,
    /// not an error.
    Unresolved,
}

impl RefState {
    /// The state of `reference` given whether its target was found.
    ///
    /// A URL is never looked up: it is a pointer to a document outside the
    /// model and counts as resolved.
    pub fn of(reference: &Ref, found: bool) -> Self {
        match (reference, found) {
            (Ref::Url(_), _) | (_, true) => Self::Resolved,
            (Ref::Local(_), false) => Self::Dangling,
            (Ref::Private(_) | Ref::External { .. }, false) => Self::Unresolved,
        }
    }
}

/// A reference to a work item: `<ref>#<number>`.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ItemRef {
    base: LocalRef,
    number: ItemNumber,
}

impl ItemRef {
    /// The Design (or the Spec) the number is counted in.
    pub fn base(&self) -> &LocalRef {
        &self.base
    }

    /// The number of the item.
    pub fn number(&self) -> ItemNumber {
        self.number
    }

    /// Resolve a reference written with a Spec, given the Designs that refer to
    /// that Spec.
    ///
    /// Exactly one Design makes the reference valid. None, or more than one,
    /// is a distinct error: the second case needs the Design named in the
    /// reference.
    pub fn design_of_spec(designs: &[DesignId]) -> Result<DesignId, ItemRefError> {
        match designs {
            [] => Err(ItemRefError::NoDesign),
            [one] => Ok(*one),
            many => Err(ItemRefError::Ambiguous { count: many.len() }),
        }
    }
}

impl FromStr for ItemRef {
    type Err = ItemRefError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let (base, number) = s.rsplit_once('#').ok_or(ItemRefError::Shape)?;
        Ok(Self {
            base: base.parse().map_err(ItemRefError::Base)?,
            number: number.parse().map_err(ItemRefError::Number)?,
        })
    }
}

impl fmt::Display for ItemRef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}{}", self.base, self.number)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::id::test_support::{did, eid};

    #[test]
    fn each_kind_of_reference_parses_and_prints_back() {
        let id = eid(7).to_string();
        for text in [
            "spec/cache-timeout".to_owned(),
            id.clone(),
            format!("private:{id}"),
            "ext:other-project/spec/auth".to_owned(),
            format!("ext:other-project/{id}"),
            "https://example.org/doc".to_owned(),
        ] {
            let parsed: Ref = text.parse().unwrap();
            assert_eq!(parsed.to_string(), text);
        }
    }

    #[test]
    fn the_mark_decides_the_kind() {
        let id = eid(7).to_string();
        assert!(matches!(
            "spec/a".parse::<Ref>().unwrap(),
            Ref::Local(LocalRef::Alias(_))
        ));
        assert!(matches!(
            id.parse::<Ref>().unwrap(),
            Ref::Local(LocalRef::Id(_))
        ));
        assert!(matches!(
            format!("private:{id}").parse::<Ref>().unwrap(),
            Ref::Private(_)
        ));
        assert!(matches!(
            "ext:p/a".parse::<Ref>().unwrap(),
            Ref::External { .. }
        ));
        assert!(matches!(
            "https://x.org/a".parse::<Ref>().unwrap(),
            Ref::Url(_)
        ));
    }

    #[test]
    fn a_private_reference_never_carries_an_alias() {
        assert!(matches!(
            "private:spec/secret".parse::<Ref>(),
            Err(ValueError::RefPrivateId(_))
        ));
    }

    #[test]
    fn malformed_references_are_refused() {
        assert_eq!("".parse::<Ref>(), Err(ValueError::RefEmpty));
        assert_eq!(
            "ext:project-only".parse::<Ref>(),
            Err(ValueError::RefExternalShape)
        );
        assert_eq!(
            "ext:Bad/a".parse::<Ref>(),
            Err(ValueError::ProjectNameBadShape)
        );
        assert!(matches!(
            "Spec A".parse::<Ref>(),
            Err(ValueError::RefLocalShape(_))
        ));
    }

    #[test]
    fn dangling_only_for_unmarked_references() {
        let local: Ref = "spec/a".parse().unwrap();
        let private: Ref = format!("private:{}", eid(1)).parse().unwrap();
        let ext: Ref = "ext:p/a".parse().unwrap();
        let url: Ref = "https://x.org/a".parse().unwrap();
        // found
        for r in [&local, &private, &ext, &url] {
            assert_eq!(RefState::of(r, true), RefState::Resolved);
        }
        // not found
        assert_eq!(RefState::of(&local, false), RefState::Dangling);
        assert_eq!(RefState::of(&private, false), RefState::Unresolved);
        assert_eq!(RefState::of(&ext, false), RefState::Unresolved);
        assert_eq!(RefState::of(&url, false), RefState::Resolved);
        assert!(private.may_be_unresolved() && ext.may_be_unresolved());
        assert!(!local.may_be_unresolved() && !url.may_be_unresolved());
    }

    #[test]
    fn item_reference_splits_at_the_last_hash() {
        let r: ItemRef = "design/cache#2".parse().unwrap();
        assert_eq!(r.number().get(), 2);
        assert_eq!(r.to_string(), "design/cache#2");
        assert_eq!("design/cache".parse::<ItemRef>(), Err(ItemRefError::Shape));
        assert!(matches!(
            "Bad Name#1".parse::<ItemRef>(),
            Err(ItemRefError::Base(_))
        ));
        assert!(matches!(
            "design/a#0".parse::<ItemRef>(),
            Err(ItemRefError::Number(_))
        ));
    }

    #[test]
    fn a_spec_item_reference_needs_exactly_one_design() {
        assert_eq!(ItemRef::design_of_spec(&[]), Err(ItemRefError::NoDesign));
        assert_eq!(ItemRef::design_of_spec(&[did(1)]), Ok(did(1)));
        assert_eq!(
            ItemRef::design_of_spec(&[did(1), did(2)]),
            Err(ItemRefError::Ambiguous { count: 2 })
        );
    }
}
