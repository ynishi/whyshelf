//! A file of a project that is no document: code, a test, data.
//!
//! A document may stand on such a file (`depends`) or point at it (`informs`), as it
//! does on another document. What an approval wants to keep of it is the same as of
//! an upstream document: *what the file was when the approval was made*, so that a
//! later change makes the approval stale. The model holds four values for that, none
//! of which knows a version-control system:
//!
//! | value | what it is | who makes it |
//! |---|---|---|
//! | [`Artifact`] | a path under the root, and whether a file is there now with which [`ArtifactHash`] | the build of a shelf, from an [`ArtifactReading`] (rule A1) |
//! | [`ArtifactHash`] | the hash of the file as its link's [`Comparison`] counts it: the only thing freshness compares (A3, A4) | [`Comparison::fingerprint`], over the bytes the adapter read |
//! | [`Comparison`] | what of a file counts: every byte, or the lines without white space and without the lines a prefix marks | a book's declaration of the link; kept in each pin |
//! | [`ContentAddress`] | where the content can be read back later; opaque, only compared | an adapter, after its own check ([`ContentAddress::from_validated`], A7) |
//! | [`Revision`] | one point of the project's history, kept as a hint; opaque | the same ([`Revision::from_validated`]) |
//!
//! ```text
//!   Shelf::artifact_asks ──► ArtifactAsks (path -> comparisons)
//!   adapter ──read bytes──► ArtifactReading::of_bytes (a hash per comparison, address?)  or  Absent
//!                        │
//!   Shelf::build ────────┴──► Artifact { path, ArtifactState }   (the hash of its link's comparison)
//!   frame_approve ──────────► ArtifactPin { link, path, PinnedArtifact, Comparison }   (ledger)
//!   assess ─────────────────► compares ArtifactHash of one comparison only; addresses never count
//! ```
//!
//! # Opaque values
//!
//! A [`ContentAddress`] and a [`Revision`] have no form the domain knows: an adapter
//! checks the text it read against its own rules and only then makes
//! one with `from_validated`, which checks the weakest rules only (not empty, no
//! white space, no control character). The domain compares and shows them and never
//! reads through them; reading content back is a port of the application.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use thiserror::Error;

use crate::domain::canon::Canon;
use crate::domain::hash::{ContentHash, HashKind};
use crate::domain::location::RelPath;

/// The hash of the content of an artifact: what an approval pins and what freshness
/// compares.
///
/// It is made by [`Comparison::fingerprint`] from the bytes an adapter read: which
/// part of a file counts is the link's comparison, a value of the domain, and two
/// hashes are compared only when they were made with the same one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ArtifactHash(ContentHash);

impl ArtifactHash {
    /// The hash of these bytes, under its own [`HashKind`], so it never equals the
    /// hash of a document for the same bytes.
    pub(crate) fn of_content(bytes: &[u8]) -> Self {
        Self(ContentHash::of_bytes(HashKind::ShelfArtifact, bytes))
    }

    /// The hash a record of the ledger kept: only where a record is read back.
    pub(super) fn restore(hash: ContentHash) -> Self {
        Self(hash)
    }

    /// The digest.
    pub fn hash(&self) -> &ContentHash {
        &self.0
    }
}

impl fmt::Display for ArtifactHash {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(&self.0, f)
    }
}

/// What of a file a link to it compares: the one place the hash of a file is made.
///
/// A book declares it per link to artifacts (`compare` in the declaration), and an
/// approval keeps it with each file it pins, so the hash pinned and the hash now are
/// always made the same way. The adapter reads the bytes and asks
/// [`Comparison::fingerprint`]; which part of a file counts is decided here, as a
/// value, and not by the adapter.
///
/// | comparison | what counts | what a change of it does not see |
/// |---|---|---|
/// | `Bytes` (the default) | every byte | nothing |
/// | `Lines` | the lines, each with its white space collapsed, without blank lines and without the lines that start with one of `ignore` | indentation, spacing, blank lines, and the lines it ignores (such as `//` comments) |
///
/// `Lines` reads the bytes as UTF-8, each byte that is not UTF-8 kept as its value
/// (`\xNN`), so no change of such a byte is lost; a line ends at `\n`, `\r\n` or a
/// lone `\r`. It knows no language: a prefix is compared with the start of the line after its white
/// space, so `//` ignores `// a`, `/// a` and `//! a`, and not `x // a`. Its hashes
/// are of another [`HashKind`] and cover the prefixes, so they never equal a hash of
/// the bytes, nor one made with other prefixes.
#[derive(Debug, Clone, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Comparison {
    /// Every byte.
    #[default]
    Bytes,
    /// The lines, as above.
    Lines {
        /// The prefixes of the lines left out.
        ignore: BTreeSet<LinePrefix>,
    },
}

impl Comparison {
    /// The hash of `bytes` as this comparison counts them.
    pub fn fingerprint(&self, bytes: &[u8]) -> ArtifactHash {
        match self {
            Self::Bytes => ArtifactHash::of_content(bytes),
            Self::Lines { ignore } => {
                let text = text_of(bytes);
                let lines = text
                    .split(['\n', '\r'])
                    .map(|line| line.split_whitespace().collect::<Vec<_>>().join(" "))
                    .filter(|line| !line.is_empty())
                    .filter(|line| !ignore.iter().any(|p| line.starts_with(p.as_str())))
                    .map(Canon::str);
                let canon = Canon::map([
                    (
                        "ignore",
                        Canon::list(ignore.iter().map(|p| Canon::str(p.as_str()))),
                    ),
                    ("lines", Canon::list(lines)),
                ]);
                ArtifactHash(ContentHash::of(HashKind::ShelfArtifactLines, &canon))
            }
        }
    }

    /// The word a declaration and a record write for its kind.
    pub fn kind_word(&self) -> &'static str {
        match self {
            Self::Bytes => BYTES,
            Self::Lines { .. } => LINES,
        }
    }

    /// The prefixes of the lines it leaves out; none for `Bytes`.
    pub fn ignored(&self) -> impl Iterator<Item = &LinePrefix> {
        match self {
            Self::Bytes => None,
            Self::Lines { ignore } => Some(ignore.iter()),
        }
        .into_iter()
        .flatten()
    }

    /// The comparison a declaration or a record names: `kind` (`bytes` or `lines`)
    /// and the prefixes, which only `lines` takes. The one reader of the words.
    pub fn parse(kind: &str, ignore: &[String]) -> Result<Self, ComparisonProblem> {
        match kind {
            BYTES if ignore.is_empty() => Ok(Self::Bytes),
            BYTES => Err(ComparisonProblem::IgnoreWithBytes),
            "" => Err(ComparisonProblem::NoKind),
            LINES => Ok(Self::Lines {
                ignore: ignore
                    .iter()
                    .map(|p| LinePrefix::new(p))
                    .collect::<Result<_, _>>()?,
            }),
            other => Err(ComparisonProblem::UnknownKind {
                found: other.to_owned(),
            }),
        }
    }
}

/// The bytes as text for a `lines` comparison: UTF-8, each byte that is not written
/// as its value (`\xNN`) and each `\` of the text written twice, so no two files that
/// differ read alike (the text `\xe9` is `\\xe9`, the byte 0xE9 is `\xe9`).
fn text_of(bytes: &[u8]) -> String {
    let mut text = String::with_capacity(bytes.len());
    for chunk in bytes.utf8_chunks() {
        text.push_str(&chunk.valid().replace('\\', "\\\\"));
        for byte in chunk.invalid() {
            text.push_str(&format!("\\x{byte:02x}"));
        }
    }
    text
}

/// The word of [`Comparison::Bytes`].
const BYTES: &str = "bytes";
/// The word of [`Comparison::Lines`].
const LINES: &str = "lines";

/// The start of a line a `lines` comparison leaves out, such as `//` or `#`.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct LinePrefix(String);

impl LinePrefix {
    /// A prefix: not empty, and no white space or control character in it (a line
    /// is compared after its white space is collapsed, so a prefix with white space
    /// could never match as written).
    pub fn new(text: &str) -> Result<Self, ComparisonProblem> {
        if text.is_empty() || text.chars().any(|c| c.is_whitespace() || c.is_control()) {
            return Err(ComparisonProblem::BadPrefix {
                found: text.to_owned(),
            });
        }
        Ok(Self(text.to_owned()))
    }

    /// The prefix.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Why a comparison cannot be made of what was written.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum ComparisonProblem {
    /// Prefixes to ignore with no kind of comparison (`ignore` without `compare`).
    #[error("`ignore` needs `compare = \"lines\"`")]
    NoKind,
    /// A kind that is neither `bytes` nor `lines`.
    #[error("`{found}` is no comparison (`bytes` or `lines`)")]
    UnknownKind {
        /// What was written.
        found: String,
    },
    /// Prefixes to ignore with `bytes`, which counts every byte.
    #[error("`ignore` is for `compare = \"lines\"`: `bytes` counts every byte")]
    IgnoreWithBytes,
    /// A prefix that is empty or holds white space or a control character.
    #[error("`{found}` is no prefix of a line: it is empty or holds white space")]
    BadPrefix {
        /// What was written.
        found: String,
    },
}

/// Why a text is not an opaque value ([`ContentAddress`], [`Revision`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum OpaqueProblem {
    /// The text is empty.
    #[error("an opaque value is not empty")]
    Empty,
    /// The text holds white space or a control character.
    #[error("an opaque value holds no white space and no control character")]
    BadChar,
}

fn check_opaque(raw: &str) -> Result<(), OpaqueProblem> {
    if raw.is_empty() {
        return Err(OpaqueProblem::Empty);
    }
    if raw.chars().any(|c| c.is_whitespace() || c.is_control()) {
        return Err(OpaqueProblem::BadChar);
    }
    Ok(())
}

macro_rules! opaque {
    ($(#[$meta:meta])* $name:ident) => {
        $(#[$meta])*
        #[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
        pub struct $name(String);

        impl $name {
            /// A value an adapter has already checked against its own form (rule
            /// A7). Only the weakest rules are checked here: not empty, no white
            /// space, no control character.
            pub fn from_validated(raw: String) -> Result<Self, OpaqueProblem> {
                check_opaque(&raw)?;
                Ok(Self(raw))
            }

            /// The value as text.
            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(&self.0)
            }
        }
    };
}

opaque!(
    /// Where the content of a file, as it was, can be read back later.
    ///
    /// Opaque: the domain only compares it and keeps it in a record (rule A5), and
    /// never decides freshness with it (A4). What it is, is the adapter's.
    ContentAddress
);

opaque!(
    /// One point of the history of the project, kept by an approval as a hint of
    /// where it was made.
    ///
    /// Opaque, like [`ContentAddress`], and never part of a freshness (rule A4).
    Revision
);

/// What is at the path of an artifact now.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ArtifactState {
    /// A file, with the hash of its content.
    Present(ArtifactHash),
    /// No file. A link to a path with no file is still an artifact (rule A1): the
    /// evidence may not be written yet.
    Absent,
}

impl ArtifactState {
    /// The hash, when a file is there.
    pub fn hash(&self) -> Option<&ArtifactHash> {
        match self {
            Self::Present(hash) => Some(hash),
            Self::Absent => None,
        }
    }
}

/// What an adapter found at one path: the hash of the file under each comparison it
/// was asked for, and where its content can be read back, or no file.
///
/// Opaque: a file's hashes are made only by [`ArtifactReading::of_bytes`], from the
/// bytes the adapter read, so no adapter can bring a hash made by a rule of its own.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArtifactReading(Option<FileReading>);

/// A file that is there: its hashes, and its address when the adapter can say.
#[derive(Debug, Clone, PartialEq, Eq)]
struct FileReading {
    hashes: BTreeMap<Comparison, ArtifactHash>,
    address: Option<ContentAddress>,
}

impl ArtifactReading {
    /// The reading of a file whose bytes the adapter read: its hash under each of
    /// `comparisons`, and under [`Comparison::Bytes`] always.
    pub fn of_bytes(
        bytes: &[u8],
        comparisons: &BTreeSet<Comparison>,
        address: Option<ContentAddress>,
    ) -> Self {
        let hashes = comparisons
            .iter()
            .chain([&Comparison::Bytes])
            .map(|c| (c.clone(), c.fingerprint(bytes)))
            .collect();
        Self(Some(FileReading { hashes, address }))
    }

    /// No file is there.
    pub fn absent() -> Self {
        Self(None)
    }

    /// A reading of a file with the hashes given, for a test that has no bytes.
    #[cfg(test)]
    pub(crate) fn of_hashes(
        hashes: BTreeMap<Comparison, ArtifactHash>,
        address: Option<ContentAddress>,
    ) -> Self {
        Self(Some(FileReading { hashes, address }))
    }

    /// Whether a file is there.
    pub fn is_present(&self) -> bool {
        self.0.is_some()
    }

    /// What the shelf keeps of it under `comparison`: the hash or its absence,
    /// without the address, which no judgement of freshness reads (rule A4). A
    /// comparison the adapter was not asked for is no file, as a path it was not
    /// asked about is (see [`ArtifactAsks`]).
    pub fn state(&self, comparison: &Comparison) -> ArtifactState {
        self.0
            .as_ref()
            .and_then(|file| file.hashes.get(comparison))
            .map_or(ArtifactState::Absent, |h| ArtifactState::Present(*h))
    }

    /// Where the content can be read back, when a file is there and the adapter
    /// could say.
    pub fn address(&self) -> Option<&ContentAddress> {
        self.0.as_ref().and_then(|file| file.address.as_ref())
    }

    /// Give a file that is there the address its content can be read back at, once
    /// the adapter has found it; a reading of no file takes none.
    pub fn set_address(&mut self, address: ContentAddress) {
        if let Some(file) = self.0.as_mut() {
            file.address = Some(address);
        }
    }
}

/// What an adapter found at each path it was asked about.
///
/// A path it was not asked about, or did not answer for, is no file here: the build
/// asks for every path the documents write, so a hole is a file that is not there.
pub type ArtifactReadings = BTreeMap<RelPath, ArtifactReading>;

/// What an adapter is asked to read: each path, with the comparisons its hash is
/// wanted under ([`Comparison::Bytes`] is always given).
pub type ArtifactAsks = BTreeMap<RelPath, BTreeSet<Comparison>>;

/// A file of the project a document links to: its path, and what is there now.
///
/// Made by the build only, from what the adapter read, so what it says is what the
/// project has: an approval pins it as it is.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Artifact {
    path: RelPath,
    state: ArtifactState,
}

impl Artifact {
    pub(super) fn new(path: RelPath, state: ArtifactState) -> Self {
        Self { path, state }
    }

    /// The path, under the root of the project.
    pub fn path(&self) -> &RelPath {
        &self.path
    }

    /// What is there now.
    pub fn state(&self) -> &ArtifactState {
        &self.state
    }
}

#[cfg(test)]
mod comparison_tests;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_opaque_value_is_not_empty_and_has_no_space() {
        assert_eq!(
            ContentAddress::from_validated(String::new()),
            Err(OpaqueProblem::Empty)
        );
        assert_eq!(
            Revision::from_validated("a b".to_owned()),
            Err(OpaqueProblem::BadChar)
        );
        assert_eq!(
            ContentAddress::from_validated("e69de29".to_owned())
                .unwrap()
                .as_str(),
            "e69de29"
        );
    }

    #[test]
    fn the_hash_of_an_artifact_is_kept_apart_from_other_hashes_of_the_same_bytes() {
        let bytes = b"fn main() {}\n";
        let artifact = ArtifactHash::of_content(bytes);
        assert_eq!(artifact, ArtifactHash::of_content(bytes));
        assert_ne!(
            *artifact.hash(),
            ContentHash::of_bytes(HashKind::ShelfDoc, bytes)
        );
        assert_ne!(artifact, ArtifactHash::of_content(b"fn main() { }\n"));
    }

    #[test]
    fn a_reading_keeps_its_address_apart_from_its_state() {
        let hash = ArtifactHash::of_content(b"x");
        let address = ContentAddress::from_validated("abc".to_owned()).unwrap();
        let reading = ArtifactReading::of_bytes(b"x", &BTreeSet::new(), Some(address.clone()));
        assert_eq!(
            reading.state(&Comparison::Bytes),
            ArtifactState::Present(hash)
        );
        assert_eq!(reading.address(), Some(&address));
        assert!(reading.is_present());
        let absent = ArtifactReading::absent();
        assert!(!absent.is_present());
        assert_eq!(absent.state(&Comparison::Bytes), ArtifactState::Absent);
        assert_eq!(absent.address(), None);
        // An address is set on a file found, and not on no file.
        let mut later = ArtifactReading::of_bytes(b"x", &BTreeSet::new(), None);
        later.set_address(address.clone());
        assert_eq!(later.address(), Some(&address));
        let mut none = ArtifactReading::absent();
        none.set_address(address);
        assert_eq!(none.address(), None);
    }
}
