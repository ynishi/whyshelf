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
//! Looking up a reference that names an entity ([`EntityRef`]) gives a
//! [`Resolution`], and [`EntityRef::resolve`] is the one place that decides which:
//!
//! | reference | looked up as | result |
//! |---|---|---|
//! | no mark | [`Directory::local`] | found / **dangling** (nothing has that name) / **ambiguous** (several do) |
//! | `private:<id>` | [`Directory::by_id`] first, in whichever store holds the id | found |
//! | `private:<id>`, not found, the private store can be read | nothing more is asked | **dangling** (a mistyped id) |
//! | `private:<id>`, not found, the private store cannot be read | nothing more is asked | **unresolved** |
//! | `ext:<project>/<id-or-alias>` | nothing is asked: there is no way to another project | **unresolved** |
//!
//! A URL is not an entity reference ([`Ref::as_entity`] is `None`): it points at a
//! document outside the model and is never looked up.
//!
//! Dangling makes the referring entity invalid. Unresolved is shown, never an
//! error: a public clone must stay usable although it cannot see the private store.
//! Whether the private store can be read is a fact about the environment, [`Reach`],
//! which the application asks of its store port; the rule above only reads it.
//!
//! # Decided
//!
//! - **A mark is not a fact about a store.** A `private:<id>` in a file stays as it
//!   was written after the entity is published, because the tool never rewrites a
//!   user's file. So the identifier is looked up first, in both stores, and a
//!   published Finding that is in the repo store is found from a clone too.
//! - **A `private:` id that names nothing is dangling where the private store can be
//!   read.** A dangling reference is, strictly, one without a mark. Reading the rule
//!   wider here is deliberate: a mistyped id must not hide behind "unresolved".
//! - **`ext:` in `grounds` or `spec` stays a refused value.** An entity that stands on
//!   an `ext:` reference could not be approved in any environment, because nothing
//!   reads the hash of another project's entity. A `private:` reference can be approved
//!   where the private store is, so it is accepted and the entity is *partial* (see
//!   [`Completeness`](crate::domain::state::Completeness)) elsewhere. An `ext:`
//!   reference is unresolved only where it is shown: `get`, and references inside a
//!   text.
//! - **The mark does not reach identity.** A Spec holds the identifiers of its grounds,
//!   not the marks they were written with: a mark is how a file was written and
//!   changes with publication, so it stays out of every hash.
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
    /// The reference as one that names an entity, or `None` for a URL (which
    /// names a document outside the model and is never looked up).
    pub fn as_entity(&self) -> Option<EntityRef> {
        match self {
            Self::Local(local) => Some(EntityRef::Local(local.clone())),
            Self::Private(id) => Some(EntityRef::Private(*id)),
            Self::External { project, target } => Some(EntityRef::External {
                project: project.clone(),
                target: target.clone(),
            }),
            Self::Url(_) => None,
        }
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

/// What a lookup of a local reference found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Lookup<T> {
    /// Exactly one entity has that name or identifier.
    Found(T),
    /// None has.
    Missing,
    /// Several have: an alias held by an entity in each store.
    Ambiguous,
}

/// Whether the private store can be read in this environment.
///
/// The repo store always can (it is the checkout); another project never can (the
/// model has no path to it).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reach {
    /// The private store is here and can be read.
    Available,
    /// The private store is not here (a public clone, a CI machine).
    Absent,
}

/// The entities a reference can be looked up in.
///
/// Implemented by the application's index: the domain holds the rule that turns
/// these answers into a [`Resolution`], not the data.
pub trait Directory<T> {
    /// Look up a reference without a mark, where the referring document sees.
    fn local(&self, reference: &LocalRef) -> Lookup<T>;

    /// Look an entity up by its identifier, in whichever store holds it (an
    /// identifier names the same entity in both stores, after a publication for
    /// one), following the identifiers that a merge made equal.
    fn by_id(&self, id: EntityId) -> Option<T>;

    /// Whether the private store can be read here.
    fn private_reach(&self) -> Reach;
}

/// A reference that names an entity: every [`Ref`] except a URL.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EntityRef {
    /// An alias or an identifier, no mark.
    Local(LocalRef),
    /// `private:<id>`.
    Private(EntityId),
    /// `ext:<project>/<target>`.
    External {
        /// The other project.
        project: ProjectName,
        /// The entity inside it.
        target: LocalRef,
    },
}

/// What looking an entity reference up in this environment gave.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Resolution<T> {
    /// The reference names this entity.
    Found(T),
    /// The store the reference points into can be read here, and nothing in it
    /// has that name: the entity that holds the reference is Invalid.
    Dangling,
    /// A reference without a mark that names several entities.
    Ambiguous,
    /// A `private:` or `ext:` reference whose store cannot be read here. Shown,
    /// not an error.
    Unresolved,
}

impl EntityRef {
    /// Look the reference up. The table in the module documentation is this
    /// function and nothing else decides it.
    ///
    /// A pair that cannot happen (a reference without a mark that cannot be
    /// reached, a `private:` reference that is ambiguous, an `ext:` reference
    /// that is looked up) has no way to be asked.
    pub fn resolve<T>(&self, directory: &impl Directory<T>) -> Resolution<T> {
        match self {
            Self::Local(local) => match directory.local(local) {
                Lookup::Found(found) => Resolution::Found(found),
                Lookup::Missing => Resolution::Dangling,
                Lookup::Ambiguous => Resolution::Ambiguous,
            },
            Self::Private(id) => match directory.by_id(*id) {
                Some(found) => Resolution::Found(found),
                None => match directory.private_reach() {
                    Reach::Available => Resolution::Dangling,
                    Reach::Absent => Resolution::Unresolved,
                },
            },
            Self::External { .. } => Resolution::Unresolved,
        }
    }

    /// The identifier the reference spells out, when it spells one: `private:<id>`
    /// and a local reference written as an identifier. Not an alias, not `ext:`.
    pub fn spelled_id(&self) -> Option<EntityId> {
        match self {
            Self::Local(local) => local.as_id(),
            Self::Private(id) => Some(*id),
            Self::External { .. } => None,
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
    use std::cell::Cell;

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

    /// A directory with fixed answers that counts how often it was asked.
    struct Fixed {
        local: Lookup<u8>,
        by_id: Option<u8>,
        reach: Reach,
        asked: Cell<u32>,
    }

    impl Fixed {
        fn new(local: Lookup<u8>, by_id: Option<u8>, reach: Reach) -> Self {
            Self {
                local,
                by_id,
                reach,
                asked: Cell::new(0),
            }
        }
    }

    impl Directory<u8> for Fixed {
        fn local(&self, _: &LocalRef) -> Lookup<u8> {
            self.asked.set(self.asked.get() + 1);
            self.local.clone()
        }

        fn by_id(&self, _: EntityId) -> Option<u8> {
            self.asked.set(self.asked.get() + 1);
            self.by_id
        }

        fn private_reach(&self) -> Reach {
            self.asked.set(self.asked.get() + 1);
            self.reach
        }
    }

    fn entity(text: &str) -> EntityRef {
        text.parse::<Ref>()
            .unwrap()
            .as_entity()
            .expect("an entity reference")
    }

    #[test]
    fn a_reference_without_a_mark_is_found_dangling_or_ambiguous() {
        let local = entity("spec/a");
        for (lookup, expected) in [
            (Lookup::Found(7), Resolution::Found(7)),
            (Lookup::Missing, Resolution::Dangling),
            (Lookup::Ambiguous, Resolution::Ambiguous),
        ] {
            // Whether the private store can be read does not matter to it.
            for reach in [Reach::Available, Reach::Absent] {
                let directory = Fixed::new(lookup.clone(), None, reach);
                assert_eq!(local.resolve(&directory), expected);
            }
        }
    }

    #[test]
    fn a_private_reference_is_looked_up_by_id_before_the_reach_is_read() {
        let private = entity(&format!("private:{}", eid(1)));
        for reach in [Reach::Available, Reach::Absent] {
            let directory = Fixed::new(Lookup::Missing, Some(9), reach);
            assert_eq!(private.resolve(&directory), Resolution::Found(9));
        }
    }

    #[test]
    fn a_private_id_nothing_has_is_dangling_where_the_store_is_readable() {
        let private = entity(&format!("private:{}", eid(1)));
        let readable = Fixed::new(Lookup::Missing, None, Reach::Available);
        assert_eq!(private.resolve(&readable), Resolution::Dangling);
    }

    #[test]
    fn a_private_id_nothing_has_is_unresolved_where_the_store_is_absent() {
        let private = entity(&format!("private:{}", eid(1)));
        let absent = Fixed::new(Lookup::Missing, None, Reach::Absent);
        assert_eq!(private.resolve(&absent), Resolution::Unresolved);
    }

    #[test]
    fn an_external_reference_asks_nothing_and_is_unresolved() {
        let external = entity("ext:p/a");
        let directory = Fixed::new(Lookup::Found(1), Some(1), Reach::Available);
        assert_eq!(external.resolve(&directory), Resolution::Unresolved);
        assert_eq!(directory.asked.get(), 0);
    }

    #[test]
    fn a_url_names_no_entity() {
        let url: Ref = "https://x.org/a".parse().unwrap();
        assert_eq!(url.as_entity(), None);
    }

    #[test]
    fn only_an_identifier_is_spelled() {
        let id = eid(3);
        assert_eq!(entity(&id.to_string()).spelled_id(), Some(id));
        assert_eq!(entity(&format!("private:{id}")).spelled_id(), Some(id));
        assert_eq!(entity("spec/a").spelled_id(), None);
        assert_eq!(entity("ext:p/a").spelled_id(), None);
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
