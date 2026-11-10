//! Content hashes, and the one place where a hash is computed.
//!
//! # Domain separation
//!
//! Every digest starts with a tag `whyshelf/<kind>/v1\0` followed by the payload
//! bytes. The kind says what the hash is *of*, so the hash of a document can never
//! equal the hash of a record or of a file's bytes, even for equal payloads. The
//! kinds are listed by [`HashKind`].
//!
//! # Hash scope
//!
//! What goes into each hash is decided where the value is defined, in
//! [`shelf`](super::shelf): a document's hash covers its book, its fields and its
//! body; a record's hash covers its canonical form; an artifact's hash covers what
//! its link's comparison counts of the bytes its adapter read (every byte, its lines,
//! or only that it is there).

use std::fmt;
use std::str::FromStr;

use sha2::{Digest, Sha256};

use super::canon::Canon;
use super::error::ValueError;

const SCHEME: &str = "sha256:";

/// What a hash is of. Part of every digest as a domain separation tag.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HashKind {
    /// The content of a shelf document: the book it is in, its fields and its body
    /// (see [`shelf`](super::shelf)).
    ShelfDoc,
    /// The canonical form of a record in a shelf's ledger of judgements (see
    /// [`shelf`](super::shelf)).
    ShelfJudgement,
    /// The bytes of an artifact, a thing outside the documents of a shelf (code, a
    /// test, data), as its adapter read them and an approval pins them (see
    /// [`shelf::artifact`](super::shelf::artifact)).
    ShelfArtifact,
    /// The lines of the bytes of an artifact, as a `lines` comparison keeps them (see
    /// [`shelf::artifact::Comparison`](super::shelf::artifact::Comparison)).
    ShelfArtifactLines,
    /// That an artifact is there, as a `presence` comparison keeps it: one hash for
    /// every artifact that is there, whatever its content (see
    /// [`shelf::artifact::Comparison`](super::shelf::artifact::Comparison)).
    ShelfArtifactPresence,
}

impl HashKind {
    /// The name that appears in the domain separation tag.
    pub fn name(self) -> &'static str {
        match self {
            Self::ShelfDoc => "shelf-doc",
            Self::ShelfJudgement => "shelf-judgement",
            Self::ShelfArtifact => "shelf-artifact",
            Self::ShelfArtifactLines => "shelf-artifact-lines",
            Self::ShelfArtifactPresence => "shelf-artifact-presence",
        }
    }

    /// The bytes prepended to every payload of this kind.
    pub fn tag(self) -> Vec<u8> {
        format!("whyshelf/{}/v1\0", self.name()).into_bytes()
    }
}

/// A SHA-256 digest written `sha256:<64 lowercase hex>`.
///
/// Why a type: a hash is compared and stored everywhere (documents, records,
/// artifacts), and a plain string would accept any text.
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_and_parse_round_trip() {
        let h = ContentHash::of_bytes(HashKind::ShelfDoc, b"hello\n");
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
        let a = ContentHash::of_bytes(HashKind::ShelfDoc, b"same");
        let b = ContentHash::of_bytes(HashKind::ShelfArtifact, b"same");
        assert_ne!(a, b);
    }

    #[test]
    fn tag_has_the_documented_shape() {
        assert_eq!(HashKind::ShelfDoc.tag(), b"whyshelf/shelf-doc/v1\0");
        assert_eq!(
            HashKind::ShelfJudgement.tag(),
            b"whyshelf/shelf-judgement/v1\0"
        );
        assert_eq!(
            HashKind::ShelfArtifact.tag(),
            b"whyshelf/shelf-artifact/v1\0"
        );
    }

    // Golden vector: fixed input, fixed expected digest. It must never change; if
    // it fails, the way every hash is computed has moved.
    #[test]
    fn golden_shelf_doc() {
        assert_eq!(
            ContentHash::of_bytes(HashKind::ShelfDoc, b"{\"a\":1}\n").to_string(),
            "sha256:2a1a9a41a5fe8a3ba277d08c1a8f3dd1f4fc362413d150daa8b263f4847b0c5b"
        );
    }
}
