//! Identities: entity identifiers, document identifiers and their short forms.
//!
//! Every document has a surrogate identifier, a UUID v7, that never changes: not
//! when the file is renamed, not when the content is revised. The domain never
//! makes one (no clock, no randomness); the caller passes it in as a value. A path
//! is not an identity.
//!
//! A person typing a command may name a document by a [`ShortId`], the end of its
//! identifier.

use std::fmt;
use std::str::FromStr;

use uuid::Uuid;

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
    /// has one, so that equal identifiers are equal texts.
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let uuid = Uuid::parse_str(s).map_err(ValueError::IdNotUuid)?;
        if s != uuid.hyphenated().to_string() {
            return Err(ValueError::IdNotCanonical);
        }
        Self::new(uuid)
    }
}

/// The identity of a document file, written as `id:` in its front matter.
///
/// The same UUID v7 as an [`EntityId`]; a type of its own so that a document's
/// identifier cannot be swapped for another identifier by mistake.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct DocId(EntityId);

impl DocId {
    /// Wrap an entity identifier as the identifier of a document.
    pub fn new(id: EntityId) -> Self {
        Self(id)
    }

    /// The identifier without its kind.
    pub fn entity(&self) -> EntityId {
        self.0
    }
}

impl fmt::Display for DocId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(&self.0, f)
    }
}

impl FromStr for DocId {
    type Err = ValueError;

    /// Only the form [`Display`](fmt::Display) writes, as for [`EntityId`].
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        s.parse::<EntityId>().map(Self)
    }
}

impl From<DocId> for EntityId {
    fn from(id: DocId) -> Self {
        id.0
    }
}

/// The short form of an identifier: the last 8 or more hexadecimal characters of a
/// UUID, which is its random part.
///
/// The first characters of a UUID v7 are a time in milliseconds, so the first few
/// of two identifiers made together are the same; the end is not. A short id is
/// therefore matched against the end of an identifier, never the start. Eight
/// characters are 32 bits, so two identifiers of one project ending alike is rare, and
/// when it happens the lookup lists the candidates instead of choosing.
///
/// A short id is a convenience for a person typing a command. It is not an identity,
/// and no file keeps one.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ShortId(String);

impl ShortId {
    /// The fewest characters of a short id.
    pub const MIN_LEN: usize = 8;

    /// The most characters of a short id: the 32 hexadecimal digits of a UUID.
    pub const MAX_LEN: usize = 32;

    /// Whether `text` has the shape of a short id: 8 to 32 hexadecimal digits, of
    /// either case. Uppercase is refused by parsing, but it is still this shape, so it
    /// is not read as a key or a path.
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

#[cfg(test)]
mod tests {
    use super::*;

    /// A valid UUID v7 built from a small number.
    fn eid(n: u128) -> EntityId {
        let bits = (n & 0x0fff_ffff_ffff_ffff) | (0x7u128 << 76) | (0b10u128 << 62);
        EntityId::new(Uuid::from_u128(bits)).unwrap()
    }

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
    fn a_doc_id_is_read_in_the_one_form_it_is_written() {
        let id = DocId::new(eid(0xabc));
        assert_eq!(id.to_string().parse::<DocId>(), Ok(id));
        assert_eq!(
            id.to_string().to_uppercase().parse::<DocId>(),
            Err(ValueError::IdNotCanonical)
        );
        assert!(matches!(
            "nonsense".parse::<DocId>(),
            Err(ValueError::IdNotUuid(_))
        ));
    }

    #[test]
    fn a_doc_id_converts_back_to_the_entity_id() {
        let d = DocId::new(eid(5));
        assert_eq!(EntityId::from(d), eid(5));
        assert_eq!(d.entity(), eid(5));
        assert_eq!(d.to_string(), eid(5).to_string());
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
}
