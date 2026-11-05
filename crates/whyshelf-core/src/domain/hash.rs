//! Content hashes, and the one place where a hash is computed.
//!
//! # Domain separation
//!
//! Every digest starts with a tag `whyshelf/<kind>/v1\0` followed by the payload
//! bytes. The kind says what the hash is *of*, so a Finding hash can never equal
//! a Spec hash even for equal bytes, and an event identifier can never be
//! mistaken for a content hash. The kinds are listed by [`HashKind`].
//!
//! # Hash scope
//!
//! What goes into each hash is decided where the value is defined: a Finding's
//! hash covers its reserved keys and body but not its classification (tags);
//! a Spec's `contract_hash` covers grounds, the accepted pins and hypotheses and
//! the Contract section; its `hyp_hash` covers the hypotheses only; a Design's
//! hash covers its spec reference, its Crux and its items taken as a set; an item's
//! hash covers its design, summary and assigned pins. Supplementary sections,
//! classification and the numbers and ids of items are never hashed, so correcting
//! a typo in a note, retagging or renumbering the items does not remove an
//! approval.
//!
//! Four further kinds hash bytes rather than a [`Canon`]:
//! `raw` is the hash of an entity's normalized source text (the key of the
//! stored object), `export` is the hash of a generated file's content, and the
//! two below are types of their own, so that one cannot stand where the other is
//! asked for:
//!
//! | type | of | for |
//! |---|---|---|
//! | [`TextHash`] | the normalized full text of a document | the key the object of that version of the text is to have, in place of `raw`, once objects are keyed by document |
//! | [`BytesHash`] | the bytes of a file as they are on disk, before normalization | telling whether two files are byte-for-byte copies |
//!
//! A [`BytesHash`] sees a difference in line breaks or in trailing white space that
//! normalization removes, so two files that read as one text but were written
//! differently are not copies of one another.

use std::fmt;
use std::str::FromStr;

use sha2::{Digest, Sha256};

use super::canon::Canon;
use super::error::ValueError;

const SCHEME: &str = "sha256:";

/// What a hash is of. Part of every digest as a domain separation tag.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HashKind {
    /// A Finding's content.
    Finding,
    /// A Spec's contract scope (`contract_hash`).
    SpecContract,
    /// A Spec's hypothesis scope (`hyp_hash`).
    SpecHyp,
    /// A Design's content.
    Design,
    /// A work item's content.
    Item,
    /// An event identifier.
    Event,
    /// The record of an event: its identifier and the time it was written, which
    /// names the file of that record.
    EventRecord,
    /// The normalized source text of an entity.
    Raw,
    /// The content of a generated file.
    Export,
    /// The normalized full text of a document: see [`TextHash`].
    Text,
    /// The raw bytes of a file before any normalization: see [`BytesHash`].
    Bytes,
}

impl HashKind {
    /// The name that appears in the domain separation tag.
    pub fn name(self) -> &'static str {
        match self {
            Self::Finding => "finding",
            Self::SpecContract => "spec-contract",
            Self::SpecHyp => "spec-hyp",
            Self::Design => "design",
            Self::Item => "item",
            Self::Event => "event",
            Self::EventRecord => "event-record",
            Self::Raw => "raw",
            Self::Export => "export",
            Self::Text => "text",
            Self::Bytes => "bytes",
        }
    }

    /// The bytes prepended to every payload of this kind.
    pub fn tag(self) -> Vec<u8> {
        format!("whyshelf/{}/v1\0", self.name()).into_bytes()
    }
}

/// A SHA-256 digest written `sha256:<64 lowercase hex>`.
///
/// Why a type: a hash is compared and stored everywhere (approvals, event
/// identifiers, object keys), and a plain string would accept any text.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ContentHash([u8; 32]);

impl ContentHash {
    /// The hash of a canonical value, separated by `kind`.
    pub fn of(kind: HashKind, canon: &Canon) -> Self {
        Self::of_bytes(kind, &canon.to_bytes())
    }

    /// The hash of raw bytes, separated by `kind`.
    pub fn of_bytes(kind: HashKind, payload: &[u8]) -> Self {
        let mut hasher = Sha256::new();
        hasher.update(kind.tag());
        hasher.update(payload);
        Self(hasher.finalize().into())
    }

    /// The `raw` hash of a normalized source text: the key of its stored object.
    pub fn raw(normalized_text: &[u8]) -> Self {
        Self::of_bytes(HashKind::Raw, normalized_text)
    }

    /// The `export` hash of a generated file's content without its marker.
    pub fn export(content: &[u8]) -> Self {
        Self::of_bytes(HashKind::Export, content)
    }

    /// The 64 lowercase hex characters of the digest.
    pub fn hex(&self) -> String {
        const DIGITS: &[u8; 16] = b"0123456789abcdef";
        let mut out = String::with_capacity(64);
        for byte in self.0 {
            out.push(char::from(DIGITS[usize::from(byte >> 4)]));
            out.push(char::from(DIGITS[usize::from(byte & 0x0f)]));
        }
        out
    }
}

impl fmt::Display for ContentHash {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{SCHEME}{}", self.hex())
    }
}

impl fmt::Debug for ContentHash {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "ContentHash({self})")
    }
}

impl FromStr for ContentHash {
    type Err = ValueError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let digest = s.strip_prefix(SCHEME).ok_or(ValueError::HashPrefix)?;
        if digest.len() != 64 {
            return Err(ValueError::HashLength { len: digest.len() });
        }
        let mut bytes = [0u8; 32];
        for (slot, pair) in bytes.iter_mut().zip(digest.as_bytes().chunks(2)) {
            *slot = (nibble(pair[0])? << 4) | nibble(pair[1])?;
        }
        Ok(Self(bytes))
    }
}

fn nibble(c: u8) -> Result<u8, ValueError> {
    match c {
        b'0'..=b'9' => Ok(c - b'0'),
        b'a'..=b'f' => Ok(c - b'a' + 10),
        _ => Err(ValueError::HashNotLowerHex),
    }
}

impl super::canon::ToCanon for ContentHash {
    fn to_canon(&self) -> Canon {
        Canon::Str(self.to_string())
    }
}

macro_rules! bytes_hash {
    ($(#[$meta:meta])* $name:ident, $kind:ident, $input:ty) => {
        $(#[$meta])*
        #[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
        pub struct $name(ContentHash);

        impl $name {
            /// The hash of `input` under this type's domain separation tag.
            pub fn of(input: &$input) -> Self {
                Self(ContentHash::of_bytes(HashKind::$kind, input.as_ref()))
            }

            /// The 64 lowercase hex characters of the digest.
            pub fn hex(&self) -> String {
                self.0.hex()
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                fmt::Display::fmt(&self.0, f)
            }
        }

        impl fmt::Debug for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                write!(f, concat!(stringify!($name), "({})"), self.0)
            }
        }
    };
}

bytes_hash!(
    /// The hash of the normalized full text of a document (`text` tag).
    ///
    /// Why a type of its own: it will be the key of the object that keeps a
    /// version of the text. The adapter that reads a file normalizes the text
    /// (line breaks to LF, trailing spaces and tabs removed, exactly one LF at the
    /// end) before the domain sees it, so this hash is of that form. It cannot be
    /// confused with a [`BytesHash`] of the same characters.
    TextHash,
    Text,
    str
);

bytes_hash!(
    /// The hash of a file's bytes as they are on disk, before normalization
    /// (`bytes` tag).
    ///
    /// Why a type of its own: two files are copies of one another only when they
    /// agree byte for byte; the normalized text would hide a difference in line
    /// breaks or in white space at the end of a line. A file that is a copy in
    /// this sense cannot be told apart from the original by anything it says.
    BytesHash,
    Bytes,
    [u8]
);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_and_parse_round_trip() {
        let h = ContentHash::raw(b"hello\n");
        let text = h.to_string();
        assert!(text.starts_with("sha256:"));
        assert_eq!(text.len(), 7 + 64);
        assert_eq!(text.parse::<ContentHash>().unwrap(), h);
    }

    #[test]
    fn parse_rejects_each_malformed_shape() {
        assert_eq!("abc".parse::<ContentHash>(), Err(ValueError::HashPrefix));
        assert_eq!(
            "sha256:abc".parse::<ContentHash>(),
            Err(ValueError::HashLength { len: 3 })
        );
        let upper = format!("sha256:{}", "A".repeat(64));
        assert_eq!(
            upper.parse::<ContentHash>(),
            Err(ValueError::HashNotLowerHex)
        );
    }

    #[test]
    fn kinds_separate_equal_payloads() {
        let a = ContentHash::of_bytes(HashKind::Finding, b"same");
        let b = ContentHash::of_bytes(HashKind::Design, b"same");
        assert_ne!(a, b);
    }

    #[test]
    fn tag_has_the_documented_shape() {
        assert_eq!(HashKind::SpecContract.tag(), b"whyshelf/spec-contract/v1\0");
        assert_eq!(HashKind::Raw.tag(), b"whyshelf/raw/v1\0");
        assert_eq!(HashKind::EventRecord.tag(), b"whyshelf/event-record/v1\0");
    }

    #[test]
    fn raw_and_export_are_separated() {
        assert_ne!(ContentHash::raw(b"x"), ContentHash::export(b"x"));
    }

    #[test]
    fn text_and_bytes_hashes_have_their_own_tags() {
        assert_eq!(HashKind::Text.tag(), b"whyshelf/text/v1\0");
        assert_eq!(HashKind::Bytes.tag(), b"whyshelf/bytes/v1\0");
    }

    #[test]
    fn a_text_hash_and_a_bytes_hash_of_the_same_input_differ() {
        let text = TextHash::of("same\n");
        let bytes = BytesHash::of(b"same\n");
        assert_ne!(text.hex(), bytes.hex());
        // Neither is the `raw` hash of the same payload either.
        assert_ne!(text.hex(), ContentHash::raw(b"same\n").hex());
        assert_ne!(bytes.hex(), ContentHash::raw(b"same\n").hex());
    }

    #[test]
    fn bytes_hash_sees_a_line_break_that_normalization_removes() {
        assert_ne!(BytesHash::of(b"a\r\nb\r\n"), BytesHash::of(b"a\nb\n"));
        assert_eq!(BytesHash::of(b"a\nb\n"), BytesHash::of(b"a\nb\n"));
    }

    #[test]
    fn text_hash_follows_the_text() {
        assert_eq!(TextHash::of("a\n"), TextHash::of("a\n"));
        assert_ne!(TextHash::of("a\n"), TextHash::of("b\n"));
        assert!(TextHash::of("a\n").to_string().starts_with("sha256:"));
    }

    // Golden vectors: fixed input, fixed expected digest. These must never
    // change; if one fails, a hash that approvals depend on has moved.
    #[test]
    fn golden_raw() {
        assert_eq!(
            ContentHash::raw(b"hello\n").to_string(),
            "sha256:e088bfd1850ff6d188730b1c1f448cd05ece8cd953495feaf7f441ac2f548523"
        );
    }

    #[test]
    fn golden_text() {
        assert_eq!(
            TextHash::of("hello\n").to_string(),
            "sha256:fe5c3d05a7f8e0fa67e291a48f186d7d96eb1d9efe6d1611379ff891f84f0784"
        );
    }

    #[test]
    fn golden_bytes() {
        assert_eq!(
            BytesHash::of(b"hello\r\n").to_string(),
            "sha256:fbb6a4d2567bf784b93943ef1595e3d96681080c7f02de29586d20a4cc08cdd9"
        );
    }

    #[test]
    fn golden_export() {
        assert_eq!(
            ContentHash::export(b"{\"a\":1}\n").to_string(),
            "sha256:7c82fac34314a42032309f907ce699524d0a672b00983b85a3985df6ea15d0e7"
        );
    }
}
