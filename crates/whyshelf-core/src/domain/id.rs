//! Identities: entity identifiers, item numbers and aliases.
//!
//! Every Finding, Spec, Design and Item has a surrogate identifier, a UUID v7,
//! that never changes: not when the file is renamed, not when the content is
//! revised. The domain never makes one (no clock, no randomness); the caller
//! passes it in as a value. A path is not an identity: the store keeps the
//! mapping from path to identifier, and a move is an event.
//!
//! A human-readable name is separate. An [`Alias`] such as `spec/cache-timeout`
//! is unique within a store (two entities with one alias are both invalid, see
//! the classification fold) and is resolved to an identifier by the application
//! before the domain sees it. An item is also known by its number inside its
//! Design (`#1`), see [`ItemNumber`].

use std::fmt;
use std::num::NonZeroU32;
use std::str::FromStr;

use uuid::Uuid;

use super::canon::{Canon, ToCanon};
use super::error::ValueError;

/// The identifier of any entity: a UUID v7.
///
/// Why v7: it sorts roughly by creation time, so listings are stable, and it is
/// the surrogate key the architecture baseline asks of a persisted entity. The
/// constructor checks the version so that a random v4 cannot slip in.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct EntityId(Uuid);

impl EntityId {
    /// Wrap a UUID, checking that it is version 7.
    pub fn new(uuid: Uuid) -> Result<Self, ValueError> {
        match uuid.get_version_num() {
            7 => Ok(Self(uuid)),
            version => Err(ValueError::IdNotV7 { version }),
        }
    }

    /// The UUID inside.
    pub fn as_uuid(&self) -> Uuid {
        self.0
    }
}

impl fmt::Display for EntityId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0.hyphenated())
    }
}

impl fmt::Debug for EntityId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "EntityId({self})")
    }
}

impl FromStr for EntityId {
    type Err = ValueError;

    /// Only the form [`Display`](fmt::Display) writes: lowercase and hyphenated.
    ///
    /// A UUID has several spellings (without hyphens, in braces, as a URN, in
    /// capitals) and the parser of the `uuid` crate takes them all. An identifier
    /// has one, because the leak lint looks for that one in a public text: a private
    /// entity named in another spelling would pass it.
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let uuid = Uuid::parse_str(s).map_err(ValueError::IdNotUuid)?;
        if s != uuid.hyphenated().to_string() {
            return Err(ValueError::IdNotCanonical);
        }
        Self::new(uuid)
    }
}

impl ToCanon for EntityId {
    fn to_canon(&self) -> Canon {
        Canon::str(self.to_string())
    }
}

macro_rules! typed_id {
    ($(#[$meta:meta])* $name:ident) => {
        $(#[$meta])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
        pub struct $name(EntityId);

        impl $name {
            /// Wrap an entity identifier as this kind of identifier.
            pub fn new(id: EntityId) -> Self {
                Self(id)
            }

            /// The identifier without its kind.
            pub fn entity(&self) -> EntityId {
                self.0
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                fmt::Display::fmt(&self.0, f)
            }
        }

        impl FromStr for $name {
            type Err = ValueError;

            /// Only the form [`Display`](fmt::Display) writes, as for [`EntityId`].
            fn from_str(s: &str) -> Result<Self, Self::Err> {
                s.parse::<EntityId>().map(Self)
            }
        }

        impl From<$name> for EntityId {
            fn from(id: $name) -> Self {
                id.0
            }
        }

        impl ToCanon for $name {
            fn to_canon(&self) -> Canon {
                self.0.to_canon()
            }
        }
    };
}

typed_id!(
    /// The identifier of a Finding.
    ///
    /// Why a type of its own: a Spec lists Finding identifiers and a Design
    /// names a Spec identifier; the compiler keeps them from being swapped.
    FindingId
);
typed_id!(
    /// The identifier of a Spec.
    SpecId
);
typed_id!(
    /// The identifier of a Design.
    DesignId
);
typed_id!(
    /// The identifier of a work item, stable across revisions of its Design.
    ///
    /// A Design file may write it as the `id:` of an item; the item then keeps its
    /// identity when it is renumbered. An item with no `id:` has the identifier a
    /// registration event gave it, or none.
    ItemId
);
typed_id!(
    /// The identity of a document file, written as `id:` in its front matter.
    ///
    /// A document is one of the three kinds that have a file (Finding, Spec,
    /// Design). The same UUID v7 is the identifier of the entity the file holds
    /// ([`FindingId`], [`SpecId`] or [`DesignId`]); this type names it before the
    /// kind is settled, which is when files are collected into a corpus.
    DocId
);

/// The number of a work item inside its Design, starting at 1.
///
/// Displayed as `#1`. The number is the human-facing key of an item and the key of
/// the item map of a Design. It is part of no hash: renumbering items changes
/// neither the hash of an item nor that of the Design, as retagging does not. The
/// identity of an item is its [`ItemId`], which a Design file writes as the `id:` of
/// the item; for an item with none, the registration event records the identifier
/// that goes with a `(Design, number)` pair.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ItemNumber(NonZeroU32);

impl ItemNumber {
    /// A number from a positive integer; zero is refused.
    pub fn new(n: u32) -> Result<Self, ValueError> {
        NonZeroU32::new(n)
            .map(Self)
            .ok_or(ValueError::ItemNumberZero)
    }

    /// The number as an integer.
    pub fn get(&self) -> u32 {
        self.0.get()
    }
}

impl fmt::Display for ItemNumber {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "#{}", self.0)
    }
}

impl FromStr for ItemNumber {
    type Err = ValueError;

    /// Accepts `1` and `#1`.
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let digits = s.strip_prefix('#').unwrap_or(s);
        let n: u32 = digits.parse().map_err(ValueError::ItemNumberNotNumber)?;
        Self::new(n)
    }
}

impl ToCanon for ItemNumber {
    fn to_canon(&self) -> Canon {
        Canon::Int(i64::from(self.0.get()))
    }
}

/// The short form of an identifier: the last 8 or more hexadecimal characters of a
/// UUID, which is its random part.
///
/// The first characters of a UUID v7 are a time in milliseconds, so the first few
/// of two identifiers made together are the same; the end is not. A short id is
/// therefore matched against the end of an identifier, never the start. Eight
/// characters are 32 bits, so two identifiers of one project ending alike is rare, and
/// when it happens the lookup lists the candidates instead of choosing
/// ([`naming`](super::naming)).
///
/// A short id is a convenience for a person typing a command. It is not an identity,
/// and no file keeps one outside a Design's `<design>#id:<short>` (where it is
/// accepted is in [`naming`](super::naming)).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ShortId(String);

impl ShortId {
    /// The fewest characters of a short id.
    pub const MIN_LEN: usize = 8;

    /// The most characters of a short id: the 32 hexadecimal digits of a UUID.
    pub const MAX_LEN: usize = 32;

    /// Whether `text` has the shape of a short id: 8 to 32 hexadecimal digits, of
    /// either case. Uppercase is refused by parsing, but it is still this shape, so it
    /// is not read as an alias or a path.
    pub fn looks_like(text: &str) -> bool {
        (Self::MIN_LEN..=Self::MAX_LEN).contains(&text.len())
            && text.bytes().all(|b| b.is_ascii_hexdigit())
    }

    /// The short form of an identifier, as it is shown: its last 8 characters.
    pub fn of(id: EntityId) -> Self {
        let mut buffer = Uuid::encode_buffer();
        let hex = id.as_uuid().simple().encode_lower(&mut buffer);
        Self(hex[hex.len() - Self::MIN_LEN..].to_owned())
    }

    /// The short id as text: lowercase hexadecimal.
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Whether the identifier ends with these characters.
    pub fn matches(&self, id: EntityId) -> bool {
        // Called for every document of a lookup: no allocation.
        let mut buffer = Uuid::encode_buffer();
        id.as_uuid()
            .simple()
            .encode_lower(&mut buffer)
            .ends_with(self.0.as_str())
    }
}

impl fmt::Display for ShortId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl FromStr for ShortId {
    type Err = ValueError;

    /// 8 to 32 characters of lowercase hexadecimal. A shorter text is refused rather
    /// than guessed at, so that a lookup cannot match half the project.
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if let Some(ch) = s.chars().find(|c| !matches!(c, '0'..='9' | 'a'..='f')) {
            return Err(ValueError::ShortIdBadChar { ch });
        }
        let len = s.len();
        if len < Self::MIN_LEN {
            return Err(ValueError::ShortIdTooShort {
                len,
                min: Self::MIN_LEN,
            });
        }
        if len > Self::MAX_LEN {
            return Err(ValueError::ShortIdTooLong {
                len,
                max: Self::MAX_LEN,
            });
        }
        Ok(Self(s.to_owned()))
    }
}

const ALIAS_MAX: usize = 128;

/// A human-readable name such as `spec/cache-timeout`.
///
/// Lowercase letters, digits and `. _ - /`, at most 128 characters, no empty
/// segment. The `:` is not allowed, so an alias can never be confused with a
/// `private:` or `ext:` reference.
///
/// A text of 8 to 32 hexadecimal characters (either case) is refused
/// ([`ValueError::AliasLooksLikeShortId`]): that is how a [`ShortId`] is written, and
/// a bare text must never be ambiguous between the two. `cafe-babe` and `v20240101`
/// are aliases.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Alias(String);

impl Alias {
    /// The alias as text.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for Alias {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl FromStr for Alias {
    type Err = ValueError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if s.is_empty() {
            return Err(ValueError::AliasEmpty);
        }
        // Either case: `CAFEBABE` is not an alias either, and not a short id.
        if ShortId::looks_like(s) {
            return Err(ValueError::AliasLooksLikeShortId { len: s.len() });
        }
        let len = s.chars().count();
        if len > ALIAS_MAX {
            return Err(ValueError::AliasTooLong {
                len,
                max: ALIAS_MAX,
            });
        }
        if let Some(ch) = s
            .chars()
            .find(|c| !(c.is_ascii_lowercase() || c.is_ascii_digit() || ".-_/".contains(*c)))
        {
            return Err(ValueError::AliasBadChar { ch });
        }
        if s.split('/').any(str::is_empty) {
            return Err(ValueError::AliasBadShape);
        }
        Ok(Self(s.to_owned()))
    }
}

impl ToCanon for Alias {
    fn to_canon(&self) -> Canon {
        Canon::str(self.0.as_str())
    }
}

#[cfg(test)]
pub(crate) mod test_support {
    use super::*;

    /// A valid UUID v7 built from a small number, for tests.
    pub(crate) fn eid(n: u128) -> EntityId {
        let bits = (n & 0x0fff_ffff_ffff_ffff) | (0x7u128 << 76) | (0b10u128 << 62);
        EntityId::new(Uuid::from_u128(bits)).unwrap()
    }

    pub(crate) fn fid(n: u128) -> FindingId {
        FindingId::new(eid(n))
    }

    pub(crate) fn sid(n: u128) -> SpecId {
        SpecId::new(eid(n))
    }

    pub(crate) fn did(n: u128) -> DesignId {
        DesignId::new(eid(n))
    }

    pub(crate) fn iid(n: u128) -> ItemId {
        ItemId::new(eid(n))
    }

    pub(crate) fn docid(n: u128) -> DocId {
        DocId::new(eid(n))
    }
}

#[cfg(test)]
mod tests {
    use super::test_support::*;
    use super::*;

    #[test]
    fn entity_id_requires_version_7() {
        let v4 = Uuid::from_u128(0x1234_5678_9abc_4def_8123_4567_89ab_cdef);
        assert_eq!(v4.get_version_num(), 4);
        assert_eq!(EntityId::new(v4), Err(ValueError::IdNotV7 { version: 4 }));
        assert!(EntityId::new(eid(1).as_uuid()).is_ok());
    }

    #[test]
    fn entity_id_text_round_trip() {
        let id = eid(42);
        assert_eq!(id.to_string().parse::<EntityId>().unwrap(), id);
        assert!(matches!(
            "not-a-uuid".parse::<EntityId>(),
            Err(ValueError::IdNotUuid(_))
        ));
    }

    #[test]
    fn an_entity_id_is_read_only_in_its_one_written_form() {
        let id = eid(42);
        let hyphenated = id.to_string();
        let simple = hyphenated.replace('-', "");
        for other in [
            simple.clone(),
            hyphenated.to_uppercase(),
            format!("{{{hyphenated}}}"),
            format!("urn:uuid:{hyphenated}"),
        ] {
            assert_eq!(
                other.parse::<EntityId>(),
                Err(ValueError::IdNotCanonical),
                "{other}"
            );
        }
        // A text that is no UUID at all stays what it was.
        assert!(matches!(
            "42".parse::<EntityId>(),
            Err(ValueError::IdNotUuid(_))
        ));
        assert_eq!(hyphenated.parse::<EntityId>(), Ok(id));
    }

    #[test]
    fn a_typed_id_is_read_in_the_one_form_it_is_written() {
        let id = iid(0xabc);
        assert_eq!(id.to_string().parse::<ItemId>(), Ok(id));
        assert_eq!(
            id.to_string().to_uppercase().parse::<ItemId>(),
            Err(ValueError::IdNotCanonical)
        );
        assert!(matches!(
            "nonsense".parse::<ItemId>(),
            Err(ValueError::IdNotUuid(_))
        ));
    }

    #[test]
    fn typed_ids_convert_back_to_the_entity_id() {
        let f = fid(5);
        assert_eq!(EntityId::from(f), eid(5));
        assert_eq!(f.to_string(), eid(5).to_string());
    }

    #[test]
    fn item_number_starts_at_one() {
        assert_eq!(ItemNumber::new(0), Err(ValueError::ItemNumberZero));
        assert_eq!("#3".parse::<ItemNumber>().unwrap().get(), 3);
        assert_eq!("3".parse::<ItemNumber>().unwrap().to_string(), "#3");
        assert!(matches!(
            "x".parse::<ItemNumber>(),
            Err(ValueError::ItemNumberNotNumber(_))
        ));
        assert_eq!("0".parse::<ItemNumber>(), Err(ValueError::ItemNumberZero));
    }

    #[test]
    fn a_short_id_is_the_end_of_an_identifier_and_not_its_start() {
        let id = eid(0xabcd_ef01_2345);
        let hex = id.to_string().replace('-', "");
        let short = ShortId::of(id);
        assert_eq!(short.as_str().len(), 8);
        assert!(hex.ends_with(short.as_str()));
        assert!(short.matches(id));
        // The end tells two identifiers made together apart.
        assert!(!short.matches(eid(0xabcd_ef01_2346)));
        // A longer end still matches; the start does not.
        assert!(hex[20..].parse::<ShortId>().unwrap().matches(id));
        assert!(!hex[..12].parse::<ShortId>().unwrap().matches(id));
    }

    #[test]
    fn a_short_id_has_a_length_and_an_alphabet() {
        assert_eq!(
            "9f3c21a".parse::<ShortId>(),
            Err(ValueError::ShortIdTooShort { len: 7, min: 8 })
        );
        assert_eq!(
            "9F3C21A7".parse::<ShortId>(),
            Err(ValueError::ShortIdBadChar { ch: 'F' })
        );
        assert_eq!(
            "9f3c21ag".parse::<ShortId>(),
            Err(ValueError::ShortIdBadChar { ch: 'g' })
        );
        assert!(matches!(
            "0".repeat(33).parse::<ShortId>(),
            Err(ValueError::ShortIdTooLong { len: 33, max: 32 })
        ));
        assert_eq!(
            "12345678".parse::<ShortId>().unwrap().to_string(),
            "12345678"
        );
    }

    #[test]
    fn alias_accepts_the_documented_shape() {
        for ok in [
            "spec/cache-timeout",
            "finding/slow-login",
            "a",
            "x.y_z-1/2",
            "cafe-babe",
            "v20240101",
            "cafebab",
            "cafebabe/x",
        ] {
            assert_eq!(ok.parse::<Alias>().unwrap().as_str(), ok);
        }
        // One more than a short id may be.
        let long = "a".repeat(33);
        assert_eq!(long.parse::<Alias>().unwrap().as_str(), long);
    }

    #[test]
    fn an_alias_may_not_look_like_a_short_id() {
        for (text, len) in [
            ("cafebabe", 8),
            ("CAFEBABE", 8),
            ("12345678", 8),
            ("9f3c21a7", 8),
            ("0123456789abcdef0123456789abcdef", 32),
        ] {
            assert_eq!(
                text.parse::<Alias>(),
                Err(ValueError::AliasLooksLikeShortId { len }),
                "{text}"
            );
        }
    }

    #[test]
    fn alias_rejects_each_broken_rule() {
        assert_eq!("".parse::<Alias>(), Err(ValueError::AliasEmpty));
        assert_eq!(
            "Spec/a".parse::<Alias>(),
            Err(ValueError::AliasBadChar { ch: 'S' })
        );
        assert_eq!(
            "private:a".parse::<Alias>(),
            Err(ValueError::AliasBadChar { ch: ':' })
        );
        assert_eq!("/a".parse::<Alias>(), Err(ValueError::AliasBadShape));
        assert_eq!("a//b".parse::<Alias>(), Err(ValueError::AliasBadShape));
        assert_eq!("a/".parse::<Alias>(), Err(ValueError::AliasBadShape));
        assert!(matches!(
            "a".repeat(129).parse::<Alias>(),
            Err(ValueError::AliasTooLong { len: 129, max: 128 })
        ));
    }
}
