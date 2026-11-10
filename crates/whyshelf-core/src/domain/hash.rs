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
//!
//! # Keyed hashes
//!
//! The hash of a private document must not be written where others can read it: for
//! a short document it is a fingerprint that can be checked by guessing. A pin of a
//! private upstream is therefore a [`PinMac`], an HMAC-SHA256 of the upstream's
//! identifier and current hash under a [`PinKey`]. The key is a value the caller
//! passes in; the domain reads no file for it. Which key goes with which pin
//! (`kid`), and what a missing key means, is in [`keyring`](super::keyring); the
//! computation is here, with every other hash.
//!
//! The HMAC is the RustCrypto `hmac` over SHA-256, tested here against the vectors of
//! RFC 4231. A pin is checked by comparing MACs in constant time
//! ([`PinMac::verifies`]), and a [`PinKey`]'s keyed state is wiped when it is dropped.

use std::fmt;
use std::str::FromStr;

use hmac::{Hmac, KeyInit, Mac};
use sha2::{Digest, Sha256};

use super::canon::Canon;
use super::error::ValueError;
use super::id::EntityId;
use super::location::RelPath;

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
    /// The message a [`PinMac`] authenticates.
    PinMac,
    /// What a publication will do: see [`PlanDigest`](super::publication::PlanDigest).
    Plan,
    /// One hunk of a change: see [`HunkId`].
    Hunk,
    /// What taking an earlier decision in will do: see
    /// [`AdoptDigest`](super::adoption::AdoptDigest).
    Adopt,
    /// What aborting a publication will do: see
    /// [`AbortDigest`](super::publication::AbortDigest).
    Abort,
    /// The content of a shelf document: the book it is in, its fields and its body
    /// (see [`shelf`](super::shelf)).
    ShelfDoc,
    /// The canonical form of a record in a shelf's ledger of judgements (see
    /// [`shelf`](super::shelf)).
    ShelfJudgement,
    /// The bytes of a file of a project that is no document of a shelf (code, a
    /// test, data), as an approval pins it (see
    /// [`shelf::artifact`](super::shelf::artifact)).
    ShelfArtifact,
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
            Self::PinMac => "pin-mac",
            Self::Plan => "plan",
            Self::Hunk => "hunk",
            Self::Adopt => "adopt",
            Self::Abort => "abort",
            Self::ShelfDoc => "shelf-doc",
            Self::ShelfJudgement => "shelf-judgement",
            Self::ShelfArtifact => "shelf-artifact",
        }
    }

    /// The bytes prepended to every payload of this kind.
    pub fn tag(self) -> Vec<u8> {
        format!("whyshelf/{}/v1\0", self.name()).into_bytes()
    }
}

/// The methods of a digest a person types back: a `struct $name(ContentHash)`, kept
/// apart from the others by its type (and its [`HashKind`]), with its short form,
/// the whole hash, a `Debug` that shows the short form under the type's name, and a
/// `Display` of the whole hash.
macro_rules! confirm_digest {
    ($name:ident) => {
        impl $name {
            /// The first 8 characters of the digest: what a person types back.
            pub fn short(&self) -> String {
                self.0.hex().chars().take(8).collect()
            }

            /// The whole digest.
            pub fn hash(&self) -> &$crate::domain::hash::ContentHash {
                &self.0
            }
        }

        impl ::std::fmt::Debug for $name {
            fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
                write!(f, concat!(stringify!($name), "({})"), self.short())
            }
        }

        impl ::std::fmt::Display for $name {
            fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
                ::std::fmt::Display::fmt(&self.0, f)
            }
        }
    };
}
pub(crate) use confirm_digest;

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

        impl FromStr for $name {
            type Err = ValueError;

            /// Only the form [`Display`](fmt::Display) writes: `sha256:` and 64
            /// lowercase hex digits. The text carries no kind, so any digest parses;
            /// where the digest came from is for the reader of the record to know.
            fn from_str(s: &str) -> Result<Self, Self::Err> {
                s.parse::<ContentHash>().map(Self)
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

/// The identity of one hunk of a change, independent of where in the file it sits
/// and of how the lines are spaced (`hunk` tag).
///
/// A hunk here is one piece of a diff cut with no lines of context: a run of lines
/// taken out and a run of lines put in, in one file. Its identity is the hash of
/// the path and of those two runs. The line numbers are left out, so the hunk is the
/// same one when the lines around it move; each line is read with its white space
/// collapsed (runs of white space become one space, the ends are trimmed) and a
/// line with nothing else on it is dropped, so re-indenting or changing the
/// spacing of a line does not make it another hunk.
///
/// Why it is here and not with the code that cuts a diff: it is a hash, and the
/// rule for what a hash covers is written where every hash is computed. Cutting
/// the diff is an adapter's work; what to do with the pieces is
/// [`integration`](super::integration).
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct HunkId(ContentHash);

impl HunkId {
    /// The identity of the hunk of `path` that takes out `removed` and puts in
    /// `added` (one text per line, without its line break).
    pub fn of(path: &RelPath, removed: &[&str], added: &[&str]) -> Self {
        let lines = |lines: &[&str]| {
            Canon::list(
                lines
                    .iter()
                    .map(|line| line.split_whitespace().collect::<Vec<_>>().join(" "))
                    .filter(|line| !line.is_empty())
                    .map(Canon::Str),
            )
        };
        Self(ContentHash::of(
            HashKind::Hunk,
            &Canon::map([
                ("path", Canon::str(path.as_str())),
                ("removed", lines(removed)),
                ("added", lines(added)),
            ]),
        ))
    }

    /// The 64 lowercase hex characters of the digest.
    pub fn hex(&self) -> String {
        self.0.hex()
    }
}

impl fmt::Display for HunkId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(&self.0, f)
    }
}

impl fmt::Debug for HunkId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "HunkId({})", self.0)
    }
}

impl FromStr for HunkId {
    type Err = ValueError;

    /// Only the form [`Display`](fmt::Display) writes.
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        s.parse::<ContentHash>().map(Self)
    }
}

impl super::canon::ToCanon for HunkId {
    fn to_canon(&self) -> Canon {
        self.0.to_canon()
    }
}

/// The fewest bytes a [`PinKey`] may have. A shorter key is a mistake (a password
/// typed in, a truncated file), and HMAC would accept it all the same.
const PIN_KEY_MIN: usize = 16;

type HmacSha256 = Hmac<Sha256>;

/// The secret that keyed pins are computed under.
///
/// A value passed in by the caller (the application reads it from the person's own
/// data, outside the project). It prints as nothing and has no equality, so it
/// cannot end up in a log or a comparison by accident.
///
/// It keeps the HMAC already keyed, not the bytes it was made from; that state is
/// wiped when the key is dropped.
#[derive(Clone)]
pub struct PinKey(HmacSha256);

impl PinKey {
    /// A key from its bytes; at least 16 of them.
    pub fn new(bytes: &[u8]) -> Result<Self, ValueError> {
        let too_short = ValueError::PinKeyTooShort { len: bytes.len() };
        if bytes.len() < PIN_KEY_MIN {
            return Err(too_short);
        }
        // HMAC takes a key of any length (RFC 2104 hashes one longer than a block),
        // so this refuses nothing the check above lets through.
        <HmacSha256 as KeyInit>::new_from_slice(bytes)
            .map(Self)
            .map_err(|_| too_short)
    }

    /// The HMAC under this key, having read an upstream's identifier and hash.
    fn keyed(&self, upstream: EntityId, hash: &ContentHash) -> HmacSha256 {
        let mut mac = self.0.clone();
        mac.update(&HashKind::PinMac.tag());
        mac.update(upstream.as_uuid().as_bytes());
        mac.update(&hash.0);
        mac
    }
}

impl fmt::Debug for PinKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("PinKey(<hidden>)")
    }
}

/// The HMAC-SHA256 of an upstream's identifier and hash under a [`PinKey`]: what a
/// pin of a private upstream records in place of the hash.
///
/// Why the identifier is in it: two private documents with equal content then have
/// different pins, so a reader of the ledger cannot tell that they are equal.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PinMac([u8; 32]);

impl PinMac {
    /// The MAC of `hash`, the content of the upstream `upstream`, under `key`.
    pub fn of(key: &PinKey, upstream: EntityId, hash: &ContentHash) -> Self {
        Self(key.keyed(upstream, hash).finalize().into_bytes().into())
    }

    /// Whether this is the MAC of `hash`, the content of `upstream`, under `key`.
    ///
    /// The comparison takes the same time wherever the MACs differ, so how long a
    /// check takes says nothing of how much of a pin was right.
    pub fn verifies(&self, key: &PinKey, upstream: EntityId, hash: &ContentHash) -> bool {
        key.keyed(upstream, hash).verify_slice(&self.0).is_ok()
    }

    /// The 64 lowercase hex characters of the MAC.
    pub fn hex(&self) -> String {
        ContentHash(self.0).hex()
    }
}

impl fmt::Display for PinMac {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.hex())
    }
}

impl fmt::Debug for PinMac {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "PinMac({self})")
    }
}

impl FromStr for PinMac {
    type Err = ValueError;

    /// Only the form [`Display`](fmt::Display) writes: 64 lowercase hex characters.
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if s.len() != 64 {
            return Err(ValueError::HashLength { len: s.len() });
        }
        format!("{SCHEME}{s}")
            .parse::<ContentHash>()
            .map(|h| Self(h.0))
    }
}

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

    fn path(text: &str) -> RelPath {
        text.parse().unwrap()
    }

    #[test]
    fn what_a_person_confirms_is_separated_by_what_it_is_the_digest_of() {
        let canon = Canon::map([("doc", Canon::str("x"))]);
        assert_eq!(HashKind::Adopt.tag(), b"whyshelf/adopt/v1\0");
        assert_eq!(HashKind::Abort.tag(), b"whyshelf/abort/v1\0");
        let digests = [HashKind::Plan, HashKind::Adopt, HashKind::Abort]
            .map(|kind| ContentHash::of(kind, &canon));
        assert_ne!(digests[0], digests[1]);
        assert_ne!(digests[1], digests[2]);
        assert_ne!(digests[0], digests[2]);
    }

    #[test]
    fn a_hunk_id_has_its_own_tag_and_round_trips() {
        assert_eq!(HashKind::Hunk.tag(), b"whyshelf/hunk/v1\0");
        let id = HunkId::of(&path("src/a.rs"), &["old"], &["new"]);
        assert_eq!(id.to_string().parse::<HunkId>().unwrap(), id);
        assert_eq!(id.hex().len(), 64);
    }

    #[test]
    fn a_hunk_id_is_made_of_the_path_and_the_lines_and_no_position() {
        let base = HunkId::of(&path("src/a.rs"), &["let a = 1;"], &["let a = 2;"]);
        assert_ne!(
            base,
            HunkId::of(&path("src/b.rs"), &["let a = 1;"], &["let a = 2;"]),
            "another file"
        );
        assert_ne!(
            base,
            HunkId::of(&path("src/a.rs"), &["let a = 1;"], &["let a = 3;"]),
            "other lines put in"
        );
        assert_ne!(
            base,
            HunkId::of(&path("src/a.rs"), &["let a = 2;"], &["let a = 1;"]),
            "the two runs are not interchangeable"
        );
    }

    #[test]
    fn a_hunk_id_ignores_how_the_lines_are_spaced() {
        let base = HunkId::of(&path("src/a.rs"), &["let a = 1;"], &["let a = 2;"]);
        let spaced = HunkId::of(
            &path("src/a.rs"),
            &["    let   a =\t1;  "],
            &["\tlet a = 2;", "   "],
        );
        assert_eq!(base, spaced);
        // The spacing inside a line is one space or none, not nothing: two words do
        // not become one.
        assert_ne!(
            HunkId::of(&path("src/a.rs"), &[], &["a b"]),
            HunkId::of(&path("src/a.rs"), &[], &["ab"])
        );
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

    /// HMAC-SHA256 of the concatenation of `parts` under `key`, of any length.
    fn hmac_sha256(key: &[u8], parts: &[&[u8]]) -> [u8; 32] {
        let mut mac = <HmacSha256 as KeyInit>::new_from_slice(key).unwrap();
        for part in parts {
            mac.update(part);
        }
        mac.finalize().into_bytes().into()
    }

    fn mac_hex(key: &[u8], data: &[u8]) -> String {
        ContentHash(hmac_sha256(key, &[data])).hex()
    }

    // RFC 4231, test cases 1, 2, 3, 4, 6 and 7 (case 5 truncates the output).
    // The keys, data and expected values are the published test vectors of that
    // IETF RFC (https://www.rfc-editor.org/rfc/rfc4231), quoted as test data.
    #[test]
    fn hmac_sha256_matches_the_vectors_of_rfc_4231() {
        assert_eq!(
            mac_hex(&[0x0b; 20], b"Hi There"),
            "b0344c61d8db38535ca8afceaf0bf12b881dc200c9833da726e9376c2e32cff7"
        );
        assert_eq!(
            mac_hex(b"Jefe", b"what do ya want for nothing?"),
            "5bdcc146bf60754e6a042426089575c75a003f089d2739839dec58b964ec3843"
        );
        assert_eq!(
            mac_hex(&[0xaa; 20], &[0xdd; 50]),
            "773ea91e36800e46854db8ebd09181a72959098b3ef8c122d9635514ced565fe"
        );
        let key: Vec<u8> = (1..=25).collect();
        assert_eq!(
            mac_hex(&key, &[0xcd; 50]),
            "82558a389a443c0ea4cc819899f2083a85f0faa3e578f8077a2e3ff46729665b"
        );
        // A key longer than the block is hashed first.
        assert_eq!(
            mac_hex(
                &[0xaa; 131],
                b"Test Using Larger Than Block-Size Key - Hash Key First"
            ),
            "60e431591ee0b67f0d8a26aacbf5b77f8e0bc6213728c5140546040f0ee37f54"
        );
        assert_eq!(
            mac_hex(
                &[0xaa; 131],
                b"This is a test using a larger than block-size key and a larger than block-size data. The key needs to be hashed before being used by the HMAC algorithm."
            ),
            "9b09ffa71b942fcb27635fbcd5b0e944bfdc63644f0713938a7f51535c3a35e2"
        );
    }

    #[test]
    fn a_message_in_parts_is_the_message_in_one() {
        let key = b"0123456789abcdef";
        assert_eq!(
            hmac_sha256(key, &[b"abcdef"]),
            hmac_sha256(key, &[b"abc", b"def"])
        );
    }

    #[test]
    fn a_pin_key_is_not_too_short_and_never_printed() {
        assert_eq!(
            PinKey::new(&[1; 15]).unwrap_err(),
            ValueError::PinKeyTooShort { len: 15 }
        );
        let key = PinKey::new(&[7; 16]).unwrap();
        assert!(!format!("{key:?}").contains('7'));
    }

    fn id(n: u128) -> EntityId {
        crate::domain::id::test_support::eid(n)
    }

    #[test]
    fn a_pin_mac_is_stable_for_one_key_and_differs_for_another() {
        let key = PinKey::new(b"0123456789abcdef").unwrap();
        let other = PinKey::new(b"fedcba9876543210").unwrap();
        let hash = ContentHash::raw(b"a private finding\n");
        assert_eq!(
            PinMac::of(&key, id(1), &hash),
            PinMac::of(&key, id(1), &hash)
        );
        assert_ne!(
            PinMac::of(&key, id(1), &hash),
            PinMac::of(&other, id(1), &hash)
        );
        assert_ne!(
            PinMac::of(&key, id(1), &hash),
            PinMac::of(&key, id(1), &ContentHash::raw(b"edited\n"))
        );
        // The same content under another identifier is another pin.
        assert_ne!(
            PinMac::of(&key, id(1), &hash),
            PinMac::of(&key, id(2), &hash)
        );
    }

    #[test]
    fn a_pin_mac_verifies_only_for_its_key_upstream_and_hash() {
        let key = PinKey::new(b"0123456789abcdef").unwrap();
        let other = PinKey::new(b"fedcba9876543210").unwrap();
        let hash = ContentHash::raw(b"a private finding\n");
        let mac = PinMac::of(&key, id(1), &hash);
        assert!(mac.verifies(&key, id(1), &hash));
        assert!(!mac.verifies(&other, id(1), &hash));
        assert!(!mac.verifies(&key, id(2), &hash));
        assert!(!mac.verifies(&key, id(1), &ContentHash::raw(b"edited\n")));
    }

    #[test]
    fn a_pin_mac_is_not_the_hash_it_authenticates() {
        let key = PinKey::new(b"0123456789abcdef").unwrap();
        let hash = ContentHash::raw(b"x");
        assert_ne!(PinMac::of(&key, id(1), &hash).hex(), hash.hex());
    }

    #[test]
    fn a_pin_mac_parses_what_it_prints_and_nothing_else() {
        let key = PinKey::new(b"0123456789abcdef").unwrap();
        let mac = PinMac::of(&key, id(1), &ContentHash::raw(b"x"));
        assert_eq!(mac.to_string().parse::<PinMac>().unwrap(), mac);
        assert_eq!(
            "abc".parse::<PinMac>(),
            Err(ValueError::HashLength { len: 3 })
        );
        assert_eq!(
            "A".repeat(64).parse::<PinMac>(),
            Err(ValueError::HashNotLowerHex)
        );
    }
}
