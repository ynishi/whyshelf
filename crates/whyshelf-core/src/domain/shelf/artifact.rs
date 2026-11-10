//! What a document links to outside the documents: an *artifact*, a name in a
//! *space*.
//!
//! A document may stand on such a thing (`depends`) or point at it (`informs`), as it
//! does on another document. What an approval wants to keep of it is the same as of
//! an upstream document: *what it was when the approval was made*, so that a later
//! change makes the approval stale.
//!
//! The model knows no kind of artifact. A [`Space`] is only a name to it (a book
//! declares one per link to artifacts), and an [`ArtifactName`] is opaque: which texts
//! are names of a space, and what a name comes to now, is the answer of the space's
//! adapter, through a port of the application. Every space goes through the same
//! values and rules.
//!
//! | value | what it is | who makes it |
//! |---|---|---|
//! | [`Space`] | the name of a space of names | a book's declaration of a link, checked against the spaces the adapters offer |
//! | [`ArtifactName`] | a name in a space, as a link writes it; opaque | the build, from the text written (rule A1) |
//! | [`ArtifactReading`] | the adapter's answer for one name: there (a hash per comparison asked, an address?), absent, cannot tell now (why), or not a name of the space (why) | the adapter, through [`ArtifactReading::of_bytes`] and its siblings |
//! | [`Artifact`] | a space, a name, and whether the thing is there now with which [`ArtifactHash`], absent, or unknown | the build of a shelf, from an [`ArtifactReading`] |
//! | [`ArtifactHash`] | the hash of the content as its link's [`Comparison`] counts it: the only thing freshness compares (A3, A4) | [`Comparison::fingerprint`], over the bytes the adapter read |
//! | [`Comparison`] | what counts: that the thing is there, every byte, or the lines with their white space collapsed, without blank lines and without the lines a prefix marks | a book's declaration of the link; kept in each pin |
//! | [`ContentAddress`] | where the content can be read back later; opaque, only compared | an adapter, after its own check ([`ContentAddress::from_validated`], A7) |
//! | [`Revision`] | one point of the project's history, kept as a hint; opaque | the same ([`Revision::from_validated`]) |
//!
//! ```text
//!   Shelf::artifact_asks ──► ArtifactAsks (space -> name -> comparisons)
//!   adapter of the space ──► ArtifactReading: present (a hash per comparison, address?)
//!                                             | absent | cannot tell (why) | not a name (why)
//!                        │
//!   Shelf::build ────────┴──► Artifact { space, name, ArtifactState }   (not a name: the
//!                                                                       document is invalid)
//!   frame_approve ──────────► ArtifactPin { link, space, name, PinnedArtifact, Comparison }
//!                              (refused when a name cannot be told now)
//!   assess ─────────────────► compares ArtifactHash of one comparison only; addresses never
//!                              count; a name that cannot be told now is a notice
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
use crate::domain::nonempty::NonBlank;

/// The name of a space of names outside the documents, such as `file`.
///
/// A space is only a name to the domain: what its names are, and what each comes to,
/// is its adapter's. A declaration names the space of each link to artifacts, and
/// [`Schema::validate`](super::schema::Schema::validate) refuses one that no adapter
/// offers; there is no space a link has when it names none.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Space(String);

/// The most characters a [`Space`] has.
pub const SPACE_MAX: usize = 40;

impl Space {
    /// A space: a lowercase ASCII letter, then lowercase ASCII letters, digits and
    /// `-`, at most [`SPACE_MAX`] characters.
    pub fn new(text: &str) -> Result<Self, SpaceProblem> {
        let mut chars = text.chars();
        let first = chars.next().is_some_and(|c| c.is_ascii_lowercase());
        let rest = chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-');
        if first && rest && text.len() <= SPACE_MAX {
            Ok(Self(text.to_owned()))
        } else {
            Err(SpaceProblem {
                found: text.to_owned(),
            })
        }
    }

    /// The name of the space.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for Space {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// A text that is no [`Space`].
#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[error(
    "`{found}` is no space: a lowercase letter, then lowercase letters, digits and `-`, at most {SPACE_MAX}"
)]
pub struct SpaceProblem {
    /// What was written.
    pub found: String,
}

/// A name in a space, as a link to artifacts writes it.
///
/// Opaque: the domain checks only what every name has (not empty, no control
/// character, at most [`NAME_MAX`] bytes) and compares it as written. Whether a text
/// is a name of its space is the answer of the space's adapter
/// ([`ArtifactReading::not_a_name`]).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ArtifactName(String);

/// The most bytes an [`ArtifactName`] has.
pub const NAME_MAX: usize = 4096;

impl ArtifactName {
    /// A name, as above.
    pub fn new(text: &str) -> Result<Self, ArtifactNameProblem> {
        if text.is_empty() {
            return Err(ArtifactNameProblem::Empty);
        }
        if text.len() > NAME_MAX {
            return Err(ArtifactNameProblem::TooLong { bytes: text.len() });
        }
        if text.chars().any(char::is_control) {
            return Err(ArtifactNameProblem::ControlChar);
        }
        Ok(Self(text.to_owned()))
    }

    /// The name as written.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for ArtifactName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::str::FromStr for ArtifactName {
    type Err = ArtifactNameProblem;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        Self::new(text)
    }
}

/// Why a text is no [`ArtifactName`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum ArtifactNameProblem {
    /// The text is empty.
    #[error("a name is not empty")]
    Empty,
    /// The text is longer than a name can be.
    #[error("a name has at most {NAME_MAX} bytes, and this has {bytes}")]
    TooLong {
        /// How many bytes it has.
        bytes: usize,
    },
    /// The text holds a control character.
    #[error("a name holds no control character")]
    ControlChar,
}

/// One artifact: a name in a space. The same name in two spaces is two artifacts.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ArtifactKey {
    space: Space,
    name: ArtifactName,
}

impl ArtifactKey {
    /// The name `name` in the space `space`.
    pub fn new(space: Space, name: ArtifactName) -> Self {
        Self { space, name }
    }

    /// The space.
    pub fn space(&self) -> &Space {
        &self.space
    }

    /// The name.
    pub fn name(&self) -> &ArtifactName {
        &self.name
    }
}

impl fmt::Display for ArtifactKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "`{}` in `{}`", self.name, self.space)
    }
}

/// The hash of the content of an artifact: what an approval pins and what freshness
/// compares.
///
/// It is made by [`Comparison::fingerprint`] from the bytes an adapter read: which
/// part of them counts is the link's comparison, a value of the domain, and two
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

/// What of an artifact a link to it compares: the one place the hash of an artifact
/// is made.
///
/// A book declares it per link to artifacts (`compare` in the declaration), and an
/// approval keeps it with each artifact it pins, so the hash pinned and the hash now
/// are always made the same way. The adapter reads the bytes and asks
/// [`Comparison::fingerprint`]; which part of them counts is decided here, as a
/// value, and not by the adapter.
///
/// | comparison | what counts | what a change of it does not see |
/// |---|---|---|
/// | `Bytes` (the default) | every byte | nothing |
/// | `Lines` | the lines, each with its white space collapsed, without blank lines and without the lines that start with one of `ignore` | indentation, spacing, blank lines, and the lines it ignores (such as `//` comments) |
/// | `Presence` | only that the artifact is there | every change of its content |
///
/// `Lines` reads the bytes as UTF-8, each byte that is not UTF-8 kept as its value
/// (`\xNN`), so no change of such a byte is lost; a line ends at `\n`, `\r\n` or a
/// lone `\r`. It knows no language: a prefix is compared with the start of the line after its white
/// space, so `//` ignores `// a`, `/// a` and `//! a`, and not `x // a`. Its hashes
/// are of another [`HashKind`] and cover the prefixes, so they never equal a hash of
/// the bytes, nor one made with other prefixes. `Presence` has one hash, of a
/// [`HashKind`] of its own, for every artifact that is there: an artifact that came
/// or went is told, an edit is not.
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
    /// Only that the artifact is there.
    Presence,
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
            Self::Presence => ArtifactHash(ContentHash::of(
                HashKind::ShelfArtifactPresence,
                &Canon::str(PRESENCE),
            )),
        }
    }

    /// The word a declaration and a record write for its kind.
    pub fn kind_word(&self) -> &'static str {
        match self {
            Self::Bytes => BYTES,
            Self::Lines { .. } => LINES,
            Self::Presence => PRESENCE,
        }
    }

    /// The prefixes of the lines it leaves out; none for `Bytes` and `Presence`.
    pub fn ignored(&self) -> impl Iterator<Item = &LinePrefix> {
        match self {
            Self::Bytes | Self::Presence => None,
            Self::Lines { ignore } => Some(ignore.iter()),
        }
        .into_iter()
        .flatten()
    }

    /// The comparison a declaration or a record names: `kind` (`bytes`, `lines` or
    /// `presence`) and the prefixes, which only `lines` takes. The one reader of the
    /// words.
    pub fn parse(kind: &str, ignore: &[String]) -> Result<Self, ComparisonProblem> {
        match kind {
            BYTES if ignore.is_empty() => Ok(Self::Bytes),
            PRESENCE if ignore.is_empty() => Ok(Self::Presence),
            BYTES => Err(ComparisonProblem::IgnoreWithBytes),
            PRESENCE => Err(ComparisonProblem::IgnoreWithPresence),
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
/// The word of [`Comparison::Presence`].
const PRESENCE: &str = "presence";

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
    /// A kind that is not `bytes`, `lines` or `presence`.
    #[error("`{found}` is no comparison (`bytes`, `lines` or `presence`)")]
    UnknownKind {
        /// What was written.
        found: String,
    },
    /// Prefixes to ignore with `bytes`, which counts every byte.
    #[error("`ignore` is for `compare = \"lines\"`: `bytes` counts every byte")]
    IgnoreWithBytes,
    /// Prefixes to ignore with `presence`, which reads no line.
    #[error("`ignore` is for `compare = \"lines\"`: `presence` counts only that it is there")]
    IgnoreWithPresence,
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
    /// Where the content of an artifact or of a document's file, as it was, can be
    /// read back later.
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

/// What a name of an artifact comes to now.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ArtifactState {
    /// The thing is there, with the hash of its content.
    Present(ArtifactHash),
    /// Nothing is there. A link to a name with nothing there is still an artifact
    /// (rule A1): the evidence may not be written yet.
    Absent,
    /// It cannot be told now, for the reason [`Untold`] gives. An approval cannot
    /// stand on it, and freshness does not change for it: it is a notice.
    Unknown(Untold),
}

/// Why what a name comes to cannot be told now.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Untold {
    /// The adapter said it cannot tell, for this reason.
    Said(NonBlank),
    /// The adapter was asked and gave no answer for the name.
    NoAnswer,
    /// The adapter answered for the name, and was not asked for this comparison.
    NotAsked,
    /// The adapter said the text is not a name of its space, for this reason (the
    /// document that writes it is invalid).
    NotAName(NonBlank),
}

impl Untold {
    /// The reason, as a person reads it.
    pub fn as_str(&self) -> &str {
        match self {
            Self::Said(why) | Self::NotAName(why) => why.as_str(),
            Self::NoAnswer => "the adapter gave no answer for it",
            Self::NotAsked => "the adapter was not asked for this comparison",
        }
    }
}

impl fmt::Display for Untold {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl ArtifactState {
    /// The hash, when the thing is there.
    pub fn hash(&self) -> Option<&ArtifactHash> {
        match self {
            Self::Present(hash) => Some(hash),
            Self::Absent | Self::Unknown(_) => None,
        }
    }
}

/// What an adapter answered for one name of its space: there (the hash under each
/// comparison it was asked for, and where its content can be read back), absent,
/// cannot tell now, or not a name of the space. Every name asked about has one of
/// the four.
///
/// Opaque: the hashes of a thing that is there are made only by
/// [`ArtifactReading::of_bytes`], from the bytes the adapter read, so no adapter can
/// bring a hash made by a rule of its own.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArtifactReading(Answer);

#[derive(Debug, Clone, PartialEq, Eq)]
enum Answer {
    Present(Found),
    Absent,
    CannotTell(NonBlank),
    NotAName(NonBlank),
}

/// A thing that is there: its hashes, and its address when the adapter can say.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Found {
    hashes: BTreeMap<Comparison, ArtifactHash>,
    address: Option<ContentAddress>,
}

impl ArtifactReading {
    /// The reading of a thing whose bytes the adapter read: its hash under each of
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
        Self(Answer::Present(Found { hashes, address }))
    }

    /// Nothing is there.
    pub fn absent() -> Self {
        Self(Answer::Absent)
    }

    /// The adapter cannot tell now, for this reason (what it needs is not there or
    /// cannot be read now).
    pub fn cannot_tell(why: NonBlank) -> Self {
        Self(Answer::CannotTell(why))
    }

    /// The text is not a name of the space, for this reason: the document that writes
    /// it is invalid.
    pub fn not_a_name(why: NonBlank) -> Self {
        Self(Answer::NotAName(why))
    }

    /// A reading of a thing with the hashes given, for a test that has no bytes.
    #[cfg(test)]
    pub(crate) fn of_hashes(
        hashes: BTreeMap<Comparison, ArtifactHash>,
        address: Option<ContentAddress>,
    ) -> Self {
        Self(Answer::Present(Found { hashes, address }))
    }

    /// Whether the thing is there.
    pub fn is_present(&self) -> bool {
        matches!(self.0, Answer::Present(_))
    }

    /// Why the text is not a name of the space, when the adapter said it is not.
    pub fn not_a_name_why(&self) -> Option<&NonBlank> {
        match &self.0 {
            Answer::NotAName(why) => Some(why),
            Answer::Present(_) | Answer::Absent | Answer::CannotTell(_) => None,
        }
    }

    /// What the shelf keeps of it under `comparison`: the hash, its absence, or that
    /// it cannot be told, without the address, which no judgement of freshness reads
    /// (rule A4). A comparison the adapter was not asked for cannot be told
    /// ([`Untold::NotAsked`]), and neither can a text that is not a name
    /// ([`Untold::NotAName`]; the build makes the document that writes it invalid).
    /// Nothing is read as absent that the adapter did not say is absent.
    pub fn state(&self, comparison: &Comparison) -> ArtifactState {
        match &self.0 {
            Answer::Present(found) => found
                .hashes
                .get(comparison)
                .map_or(ArtifactState::Unknown(Untold::NotAsked), |h| {
                    ArtifactState::Present(*h)
                }),
            Answer::Absent => ArtifactState::Absent,
            Answer::NotAName(why) => ArtifactState::Unknown(Untold::NotAName(why.clone())),
            Answer::CannotTell(why) => ArtifactState::Unknown(Untold::Said(why.clone())),
        }
    }

    /// Where the content can be read back, when the thing is there and the adapter
    /// could say.
    pub fn address(&self) -> Option<&ContentAddress> {
        match &self.0 {
            Answer::Present(found) => found.address.as_ref(),
            Answer::Absent | Answer::CannotTell(_) | Answer::NotAName(_) => None,
        }
    }

    /// Give a thing that is there the address its content can be read back at, once
    /// the adapter has found it; any other reading takes none.
    pub fn set_address(&mut self, address: ContentAddress) {
        if let Answer::Present(found) = &mut self.0 {
            found.address = Some(address);
        }
    }
}

/// The names of one space an adapter is asked about, each with the comparisons its
/// hash is wanted under ([`Comparison::Bytes`] is always given).
pub type SpaceAsks = BTreeMap<ArtifactName, BTreeSet<Comparison>>;

/// What an adapter answered for each name of one space it was asked about.
pub type SpaceReadings = BTreeMap<ArtifactName, ArtifactReading>;

/// What the adapters are asked to resolve: per space, each name with its
/// comparisons.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ArtifactAsks(BTreeMap<Space, SpaceAsks>);

impl ArtifactAsks {
    /// Nothing asked.
    pub fn new() -> Self {
        Self::default()
    }

    /// Ask for `name` in `space` under `comparison` (and the comparisons already
    /// asked).
    pub fn ask(&mut self, space: Space, name: ArtifactName, comparison: Comparison) {
        self.0
            .entry(space)
            .or_default()
            .entry(name)
            .or_default()
            .insert(comparison);
    }

    /// Ask for `name` in `space`, under no comparison but [`Comparison::Bytes`].
    pub fn ask_name(&mut self, space: Space, name: ArtifactName) {
        self.0.entry(space).or_default().entry(name).or_default();
    }

    /// Whether nothing is asked.
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// The comparisons asked for `name` in `space`, if it is asked about.
    pub fn get(&self, space: &Space, name: &ArtifactName) -> Option<&BTreeSet<Comparison>> {
        self.0.get(space).and_then(|names| names.get(name))
    }

    /// What is asked of each space.
    pub fn spaces(&self) -> impl Iterator<Item = (&Space, &SpaceAsks)> {
        self.0.iter()
    }

    /// Every name asked, with its space and its comparisons.
    pub fn iter(&self) -> impl Iterator<Item = (&Space, &ArtifactName, &BTreeSet<Comparison>)> {
        self.0.iter().flat_map(|(space, names)| {
            names
                .iter()
                .map(move |(name, comparisons)| (space, name, comparisons))
        })
    }
}

/// What the adapters answered, per space and name.
///
/// A name with no answer here cannot be told ([`Untold::NoAnswer`]): only an answer
/// of the adapter says that nothing is there ([`ArtifactReadings::state_of`]).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ArtifactReadings(BTreeMap<Space, SpaceReadings>);

impl ArtifactReadings {
    /// No answer.
    pub fn new() -> Self {
        Self::default()
    }

    /// The answer for `name` in `space`.
    pub fn insert(&mut self, space: Space, name: ArtifactName, reading: ArtifactReading) {
        self.0.entry(space).or_default().insert(name, reading);
    }

    /// The answers of one space, added to those already there.
    pub fn extend_space(&mut self, space: Space, readings: SpaceReadings) {
        self.0.entry(space).or_default().extend(readings);
    }

    /// The answer for `name` in `space`, if there is one.
    pub fn get(&self, space: &Space, name: &ArtifactName) -> Option<&ArtifactReading> {
        self.0.get(space).and_then(|names| names.get(name))
    }

    /// The answer for one artifact, if there is one.
    pub fn of(&self, key: &ArtifactKey) -> Option<&ArtifactReading> {
        self.get(key.space(), key.name())
    }

    /// What one artifact comes to under `comparison`: the state of its answer
    /// ([`ArtifactReading::state`]), or [`Untold::NoAnswer`] when there is none. The
    /// one place a missing answer is read.
    pub fn state_of(&self, key: &ArtifactKey, comparison: &Comparison) -> ArtifactState {
        self.of(key)
            .map_or(ArtifactState::Unknown(Untold::NoAnswer), |r| {
                r.state(comparison)
            })
    }

    /// Whether there is no answer.
    pub fn is_empty(&self) -> bool {
        self.0.values().all(BTreeMap::is_empty)
    }

    /// Every answer, with its space and its name.
    pub fn iter(&self) -> impl Iterator<Item = (&Space, &ArtifactName, &ArtifactReading)> {
        self.0.iter().flat_map(|(space, names)| {
            names
                .iter()
                .map(move |(name, reading)| (space, name, reading))
        })
    }
}

impl FromIterator<(Space, ArtifactName, ArtifactReading)> for ArtifactReadings {
    fn from_iter<I: IntoIterator<Item = (Space, ArtifactName, ArtifactReading)>>(iter: I) -> Self {
        let mut readings = Self::new();
        for (space, name, reading) in iter {
            readings.insert(space, name, reading);
        }
        readings
    }
}

/// An artifact a document links to: its space, its name, and what it comes to now.
///
/// Made by the build only, from what the adapter answered, so what it says is what
/// the project has: an approval pins it as it is.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Artifact {
    key: ArtifactKey,
    state: ArtifactState,
}

impl Artifact {
    pub(super) fn new(key: ArtifactKey, state: ArtifactState) -> Self {
        Self { key, state }
    }

    /// The space and the name.
    pub fn key(&self) -> &ArtifactKey {
        &self.key
    }

    /// The space.
    pub fn space(&self) -> &Space {
        self.key.space()
    }

    /// The name, as written.
    pub fn name(&self) -> &ArtifactName {
        self.key.name()
    }

    /// What it comes to now.
    pub fn state(&self) -> &ArtifactState {
        &self.state
    }
}

#[cfg(test)]
mod comparison_tests;

#[cfg(test)]
mod tests {
    use super::*;

    fn why(text: &str) -> NonBlank {
        NonBlank::new(text).unwrap()
    }

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
    fn a_space_is_a_lowercase_word_of_at_most_forty_characters() {
        for good in ["file", "rust-item", "a", "x2", &"a".repeat(SPACE_MAX)] {
            assert_eq!(Space::new(good).unwrap().as_str(), good);
        }
        for bad in [
            "",
            "File",
            "2x",
            "-a",
            "a_b",
            "a b",
            "fíle",
            &"a".repeat(SPACE_MAX + 1),
        ] {
            assert_eq!(
                Space::new(bad),
                Err(SpaceProblem {
                    found: bad.to_owned()
                }),
                "{bad:?}"
            );
        }
    }

    #[test]
    fn a_name_is_opaque_and_checked_only_for_what_every_name_has() {
        for good in ["tests/a.rs", "crate::m::f", "a b", "../x", "/abs", "é"] {
            assert_eq!(ArtifactName::new(good).unwrap().as_str(), good);
        }
        assert_eq!(ArtifactName::new(""), Err(ArtifactNameProblem::Empty));
        assert_eq!(
            ArtifactName::new("a\nb"),
            Err(ArtifactNameProblem::ControlChar)
        );
        // White space at an end is kept, as a path could have it.
        for edge in [" a", "a ", "a\u{a0}"] {
            assert_eq!(ArtifactName::new(edge).unwrap().as_str(), edge);
        }
        assert!(ArtifactName::new(&"a".repeat(NAME_MAX)).is_ok());
        assert_eq!(
            ArtifactName::new(&"a".repeat(NAME_MAX + 1)),
            Err(ArtifactNameProblem::TooLong {
                bytes: NAME_MAX + 1
            })
        );
    }

    #[test]
    fn the_same_name_in_two_spaces_is_two_artifacts() {
        let name = ArtifactName::new("a").unwrap();
        let file = ArtifactKey::new(Space::new("file").unwrap(), name.clone());
        let other = ArtifactKey::new(Space::new("other").unwrap(), name.clone());
        assert_ne!(file, other);
        let mut readings = ArtifactReadings::new();
        readings.insert(
            file.space().clone(),
            name.clone(),
            ArtifactReading::absent(),
        );
        readings.insert(
            other.space().clone(),
            name,
            ArtifactReading::of_bytes(b"x", &BTreeSet::new(), None),
        );
        assert_eq!(readings.of(&file), Some(&ArtifactReading::absent()));
        assert!(readings.of(&other).is_some_and(ArtifactReading::is_present));
        assert_eq!(
            readings.state_of(&file, &Comparison::Bytes),
            ArtifactState::Absent
        );
    }

    #[test]
    fn a_name_with_no_answer_and_a_comparison_not_asked_cannot_be_told() {
        let key = ArtifactKey::new(Space::new("file").unwrap(), ArtifactName::new("a").unwrap());
        assert_eq!(
            ArtifactReadings::new().state_of(&key, &Comparison::Bytes),
            ArtifactState::Unknown(Untold::NoAnswer)
        );
        let reading = ArtifactReading::of_bytes(b"x", &BTreeSet::new(), None);
        assert_eq!(
            reading.state(&Comparison::Presence),
            ArtifactState::Unknown(Untold::NotAsked)
        );
        assert_eq!(
            Untold::NoAnswer.to_string(),
            "the adapter gave no answer for it"
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
        // An address is set on a thing found, and not on any other answer.
        let mut later = ArtifactReading::of_bytes(b"x", &BTreeSet::new(), None);
        later.set_address(address.clone());
        assert_eq!(later.address(), Some(&address));
        for mut none in [
            ArtifactReading::absent(),
            ArtifactReading::cannot_tell(why("offline")),
            ArtifactReading::not_a_name(why("climbs out")),
        ] {
            none.set_address(address.clone());
            assert_eq!(none.address(), None);
        }
    }

    #[test]
    fn cannot_tell_is_an_unknown_state_and_not_a_name_says_why() {
        let unknown = ArtifactReading::cannot_tell(why("no docs built"));
        assert!(!unknown.is_present());
        assert_eq!(
            unknown.state(&Comparison::Bytes),
            ArtifactState::Unknown(Untold::Said(why("no docs built")))
        );
        assert_eq!(unknown.not_a_name_why(), None);
        assert_eq!(unknown.state(&Comparison::Bytes).hash(), None);

        let no_name = ArtifactReading::not_a_name(why("climbs out"));
        assert_eq!(no_name.not_a_name_why(), Some(&why("climbs out")));
        assert_eq!(
            no_name.state(&Comparison::Bytes),
            ArtifactState::Unknown(Untold::NotAName(why("climbs out")))
        );
        assert_eq!(ArtifactReading::absent().not_a_name_why(), None);
    }
}
