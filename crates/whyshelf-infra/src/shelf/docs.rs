//! The documents: Markdown files with a YAML front matter, under the directories of
//! the books.
//!
//! ```text
//!   ---
//!   id: 019a6f3e-...-...        the identity, a UUID v7, never changed
//!   key: S-3                    the key a person writes
//!   severity: high              a field
//!   grounds: [F-1, F-2]         a link: the name of a kind of link and keys
//!   ---
//!   The body.
//! ```
//!
//! The front matter is a flat mapping. This module turns it into
//! [`RawValue`]s and hands every entry, `id` and `key` included, to
//! [`Shelf::build`](whyshelf_core::domain::shelf::build::Shelf::build), which
//! decides which entry is a field and which a link. A file whose front matter
//! cannot be read is a [`DocReading::Unparsed`], and one that is read as a mapping
//! but holds values of kinds it does not hold is a [`DocReading::Unheld`] with every
//! such value; each has its `id:` line if that line can still be read, so that a
//! broken file is an invalid document with an identity and not a hole.
//!
//! # Writing a new document
//!
//! [`MarkdownDocs`] is also the [`DocWrite`] of a new document: [`render`] makes the
//! text from the entries (a flat front matter, every text in double quotes, `id` and
//! `key` first, the whole text after the normalization rule), and the file is
//! created only where nothing is (`create_new`: a directory or a link there, even one
//! to nothing, is in the way too), with its directories; a write that fails after
//! the file was made removes it, and only it. What [`render`]
//! writes, [`read`](fn@read) reads back into the same entries; a test holds the two together.
//!
//! # Writing a document in the form
//!
//! The text [`render`] makes is the one form of a document, so what `new` writes is
//! in it. The adapter judges nothing of the form: [`DocSource::forms`] gives each file
//! as it was read, with its bytes and the line of the first comment in its front
//! matter ([`comment_line`]; the form has no place for one), and
//! [`DocSource::render`] gives the text of entries and a body the use case chose in
//! the form, the whole text after the normalization rule ([`normalize`]), with what
//! that text reads as when it is read as the file. So the form also normalizes a
//! body: its line endings become `\n`, white space at the end of its lines and blank
//! lines at its end go, and it ends with one newline. [`read`](fn@read) normalizes the whole
//! text first, so this changes nothing it reads. Whether a file may be written over
//! is [`DocSource::replaceable`]: not when the user cannot write it or it has more
//! than one link ([`Replaceable::ReadOnly`], [`Replaceable::Linked`]).
//!
//! [`MarkdownDocs`] is also the [`DocReplace`] that writes that text over the file:
//! when the file still holds the bytes that were read, just before, and whole (a
//! new file beside it, renamed over it), so the file is either as it was or in the
//! form. It is the one write here that replaces a file; [`DocWrite::create`] never
//! does. [`DocReplace::restore`] writes the bytes a file had back over the text that
//! was written, in the same way and whether they are in the form or not, and
//! [`DocRemove`] removes a file that still holds the bytes it is expected to: what
//! `rekey` needs to move a document to a file of its own and to undo that when it stops.
//! The text [`DocSource::render`] gives is the text [`DocWrite::create`] writes, byte for
//! byte (a test holds the two together), so a file the latter made is told from another
//! by comparing it with the former.
//!
//! # Where the content of a document is kept
//!
//! [`MarkdownDocs`] names the blob a document's file is (or would be) in, for an
//! approval to keep, and reads that content back, through the project's Git
//! ([`git`](super::git)); outside a repository there is none.
//!
//! # Normalization
//!
//! Before anything is read, the whole text is normalized by the domain's one rule,
//! [`normalize`]; the body is hashed by the domain as it is after it.
//!
//! # Where each part is
//!
//! The reading of the documents is `docs/read.rs` ([`DocSource`]), the writing of them
//! `docs/write.rs` ([`DocWrite`], [`DocReplace`], [`DocRemove`], and whether a file may
//! be written over), and the reading back of a content by its address
//! `docs/read_back.rs`. The text of a document, read and rendered, is here.
//!
//! [`DocSource`]: whyshelf_core::app::shelf::DocSource
//! [`DocSource::forms`]: whyshelf_core::app::shelf::DocSource::forms
//! [`DocSource::render`]: whyshelf_core::app::shelf::DocSource::render
//! [`DocSource::replaceable`]: whyshelf_core::app::shelf::DocSource::replaceable
//! [`DocWrite`]: whyshelf_core::app::shelf::DocWrite
//! [`DocWrite::create`]: whyshelf_core::app::shelf::DocWrite::create
//! [`DocReplace`]: whyshelf_core::app::shelf::DocReplace
//! [`DocReplace::restore`]: whyshelf_core::app::shelf::DocReplace::restore
//! [`DocRemove`]: whyshelf_core::app::shelf::DocRemove
//! [`Replaceable::ReadOnly`]: whyshelf_core::app::shelf::Replaceable::ReadOnly
//! [`Replaceable::Linked`]: whyshelf_core::app::shelf::Replaceable::Linked

mod read;
mod read_back;
mod write;

use std::collections::{BTreeMap, BTreeSet};
use std::io;
use std::path::PathBuf;

use std::fmt;

use serde::Deserialize;
use serde::de::{
    self, Deserializer, EnumAccess, IgnoredAny, MapAccess, SeqAccess, VariantAccess, Visitor,
};
use whyshelf_core::app::shelf::FileRead;
use whyshelf_core::domain::error::ValueError;
use whyshelf_core::domain::id::DocId;
use whyshelf_core::domain::location::RelPath;
use whyshelf_core::domain::nonempty::NonEmptyVec;
use whyshelf_core::domain::shelf::build::{
    DocReading, RawProblem, RawValue, ReadId, UnheldItem, UnheldKind, UnheldValue,
};
use whyshelf_core::domain::shelf::doc::normalize;

use super::git::{Git, GitError};
use super::state::files::FileStates;
use super::state::{ProjectTree, StateError};

/// Why the documents could not be read at all, or a document's file could not be
/// written. A file that is not a document is not an error: it is a reading that
/// says why.
#[derive(Debug, thiserror::Error)]
pub enum DocsError {
    /// A directory or a file could not be read.
    #[error("cannot read {}", .path.display())]
    Io {
        /// The path.
        path: PathBuf,
        /// The operating system's error.
        #[source]
        source: io::Error,
    },
    /// A step of a write failed: what it was, with the path it failed on.
    #[error("cannot {step}")]
    Write {
        /// The step, with its path (`create the temporary file <path>`, `rename
        /// <path> over <path>`).
        step: String,
        /// The operating system's error.
        #[source]
        source: io::Error,
    },
    /// A file's name is not a path the project can name (not UTF-8, or a segment
    /// a relative path refuses).
    #[error("{} has a name that is not a relative path", .path.display())]
    Name {
        /// The path.
        path: PathBuf,
        /// Why the domain refused it.
        #[source]
        source: Option<ValueError>,
    },
    /// Git gave no usable name for a document's file that is there.
    #[error(transparent)]
    Git(#[from] GitError),
    /// What is at the directory of a range, or at a document's file, cannot be told by
    /// the state of the project (a kind of file that is neither a file, a directory
    /// nor a link), and is not read. The reading stops, as for a file that cannot be
    /// read.
    #[error("cannot tell what {} is: {why}", .path.display())]
    Untold {
        /// The path.
        path: PathBuf,
        /// Why, as the state said it.
        why: String,
    },
    /// The state of the project could not be read at the side asked.
    #[error(transparent)]
    State(#[from] StateError),
}

/// The Markdown files of a project, and where their content is kept: the blob each
/// is (or would be) in, in the project's Git repository when it is in one.
#[derive(Debug, Clone)]
pub struct MarkdownDocs {
    root: PathBuf,
    git: Git,
    /// The tree of the project the documents are read from.
    state: ProjectTree,
}

impl MarkdownDocs {
    /// The documents of the project at `root`, read at one side (the root as it is).
    pub fn new(root: PathBuf) -> Self {
        Self::on(ProjectTree::files(FileStates::new(root.clone())), root)
    }

    /// The documents of the project `state` reads; they are written, and read back by
    /// their addresses, under `root`.
    pub fn on(state: ProjectTree, root: PathBuf) -> Self {
        Self {
            git: Git::new(root.clone()),
            root,
            state,
        }
    }
}

/// The reading of a file that is not UTF-8 text.
fn not_text(origin: RelPath) -> DocReading {
    DocReading::Unparsed {
        origin,
        id: ReadId::Unreadable,
        why: RawProblem::Syntax { line: None },
    }
}

/// How a text stands to a front matter: the one place "no front matter" is defined.
///
/// A front matter is opened by a line that is exactly `---` as the very first line
/// of the text (after the normalization rule, so a CRLF, and spaces or tabs at the
/// end of the line, do not matter). The rule, on the text after the normalization
/// rule:
///
/// 1. It begins with the line `---`: a front matter opens
///    ([`FrontMatterStart::Opens`]); one never closed is
///    [`RawProblem::Unclosed`].
/// 2. Otherwise, set aside a leading byte order mark (U+FEFF) and every blank line
///    before the first line with anything on it. When there is no such line (the
///    empty text, a text of blank lines, a lone mark), or that line neither
///    begins with `---` nor starts a merge conflict (4.), the text holds *no front
///    matter at all*
///    ([`FrontMatterStart::None`]): a Markdown file that begins with a heading or
///    a paragraph, with `---` only later (a thematic break), is one.
/// 3. Otherwise that line begins with `---` and is not the exact opening at the
///    first byte (a mark or blank lines come before it, or it is `----`, `---x`,
///    `--- x`): the text tried to open a front matter, and did not
///    ([`FrontMatterStart::BadOpening`], [`RawProblem::BadOpening`]). It is
///    invalid, and a book that skips files with no front matter never skips it, so
///    a mistyped delimiter is not passed over. In a book that does not skip, the
///    shelf tells it as [`RawProblem::NoFrontMatter`], as before a book could skip
///    (`Shelf::told`).
/// 4. Otherwise that line is the start of a merge conflict (`<<<<<<<`,
///    [`Marker::Start`]): the text may be a file whose two sides each open a front
///    matter ([`FrontMatterStart::MarkerOpens`]). Only the shape of the line is
///    judged here; whether the text is a conflict of front matters is
///    [`conflict`]'s.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FrontMatterStart<'a> {
    /// The first line is `---`; `rest` is the text after it.
    Opens {
        /// What follows the opening line.
        rest: &'a str,
    },
    /// The first line with anything on it begins with `---`, but is not the exact
    /// opening at the first byte.
    BadOpening,
    /// The first line with anything on it starts a merge conflict.
    MarkerOpens,
    /// No front matter at all.
    None,
}

/// Whether a line is the delimiter of a front matter, opening or closing: exactly
/// `---`, without its line break. The one place the question is answered.
fn is_delimiter(line: &str) -> bool {
    line.trim_end_matches('\n') == "---"
}

/// Whether a line tries to be the delimiter that opens a front matter: it begins
/// with `---`. One that is not exactly the delimiter ([`is_delimiter`]) is a
/// mistyped opening. The one place the question is answered.
fn tries_to_open(line: &str) -> bool {
    line.starts_with("---")
}

/// A line of the markers a merge conflict leaves in a text.
///
/// ```text
///   <<<<<<< ours        Start    the side that was here
///   ||||||| base        Base     the common ancestor (diff3 and zdiff3 only)
///   =======             Middle   between the sides
///   >>>>>>> theirs      End      the side that came
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Marker {
    Start,
    Base,
    Middle,
    End,
}

/// The marker a line is, if it is one: seven of the marker's character, then white
/// space or the end of the line. The one place the shape of a marker is known. The
/// length is fixed at seven; the `conflict-marker-size` attribute is not read.
fn marker(line: &str) -> Option<Marker> {
    let line = line.trim_end_matches('\n');
    [
        ("<<<<<<<", Marker::Start),
        ("|||||||", Marker::Base),
        ("=======", Marker::Middle),
        (">>>>>>>", Marker::End),
    ]
    .into_iter()
    .find_map(|(seven, marker)| {
        let rest = line.strip_prefix(seven)?;
        rest.chars()
            .next()
            .is_none_or(char::is_whitespace)
            .then_some(marker)
    })
}

/// A text without the byte order mark it may begin with: what the first line with
/// anything on it is looked for in, and what the lines of a conflict are read from.
/// The one place the mark is set aside.
fn without_mark(text: &str) -> &str {
    text.strip_prefix('\u{feff}').unwrap_or(text)
}

/// How a text, already normalized by [`normalize`], stands to a front matter
/// ([`FrontMatterStart`]).
fn front_matter_start(normalized: &str) -> FrontMatterStart<'_> {
    if let Some((first, rest)) = normalized.split_once('\n')
        && is_delimiter(first)
    {
        return FrontMatterStart::Opens { rest };
    }
    let first_with_content = without_mark(normalized)
        .lines()
        .find(|line| !line.is_empty());
    match first_with_content {
        Some(line) if tries_to_open(line) => FrontMatterStart::BadOpening,
        Some(line) if marker(line) == Some(Marker::Start) => FrontMatterStart::MarkerOpens,
        _ => FrontMatterStart::None,
    }
}

/// A file's text as a reading.
///
/// A file whose front matter cannot be read is also asked whether it is a merge
/// conflict that touches its front matter (`conflict`); if it is, that is what it
/// is said to be, and its identity is the one the sides agree on.
pub fn read(origin: RelPath, text: &str) -> DocReading {
    let text = normalize(text);
    let unparsed = |why, id| DocReading::Unparsed {
        origin: origin.clone(),
        id,
        why,
    };
    // The reading of a file whose front matter opens with a line both sides share
    // and could not be read: a conflict if it is one, else for `why`, with the id
    // `lines` show.
    let unread = |why, lines| match conflict(&text, Opened::Common) {
        Some(id) => unparsed(RawProblem::ConflictMarkers, id),
        None => unparsed(why, seen_id(lines)),
    };
    let rest = match front_matter_start(&text) {
        FrontMatterStart::Opens { rest } => rest,
        FrontMatterStart::BadOpening => {
            return unparsed(RawProblem::BadOpening, ReadId::Unreadable);
        }
        FrontMatterStart::MarkerOpens => {
            return match conflict(&text, Opened::None) {
                Some(id) => unparsed(RawProblem::ConflictMarkers, id),
                None => unparsed(RawProblem::NoFrontMatter, ReadId::Unreadable),
            };
        }
        FrontMatterStart::None => return unparsed(RawProblem::NoFrontMatter, ReadId::Unreadable),
    };
    let Some((yaml, body)) = split_closing(rest) else {
        return unread(RawProblem::Unclosed, rest.lines());
    };
    match entries(yaml) {
        Ok(entries) => DocReading::Parsed {
            origin,
            entries,
            body: body.to_owned(),
        },
        Err(Unread::Whole(why)) => unread(why, yaml.lines()),
        Err(Unread::Values(values)) => DocReading::Unheld {
            origin,
            id: seen_id(yaml.lines()),
            values,
        },
    }
}

/// The YAML up to the closing `---` line, and the body after it.
fn split_closing(rest: &str) -> Option<(&str, &str)> {
    let mut offset = 0;
    for line in rest.split_inclusive('\n') {
        if is_delimiter(line) {
            return Some((&rest[..offset], &rest[offset + line.len()..]));
        }
        offset += line.len();
    }
    None
}

/// A file as `fmt` is given it: its reading, its bytes, and the line of the first
/// comment of its front matter when it is read as a mapping and holds one (the module
/// page, "Writing a document in the form"). Nothing of the form is judged here.
pub fn file_read(origin: RelPath, bytes: Vec<u8>) -> FileRead {
    let Ok(text) = std::str::from_utf8(&bytes) else {
        return FileRead {
            reading: not_text(origin),
            read: bytes,
            comment: None,
        };
    };
    let reading = read(origin, text);
    let comment = match &reading {
        DocReading::Parsed { .. } => comment_in(text),
        DocReading::Unparsed { .. } | DocReading::Unheld { .. } => None,
    };
    FileRead {
        reading,
        read: bytes,
        comment,
    }
}

/// The line of the file the first comment of its front matter is on, if it holds
/// one ([`comment_line`] of its front matter after the normalization rule).
fn comment_in(text: &str) -> Option<u32> {
    let text = normalize(text);
    let (yaml, _) = split_closing(text.strip_prefix("---\n")?)?;
    // The front matter starts on the second line of the file.
    comment_line(yaml).map(|line| line.saturating_add(1))
}

/// The line (counted from 1) of the first YAML comment in a front matter, if it
/// holds one: the one place the rule is written.
///
/// The rule goes by meaning, not by a guess at the grammar of YAML. A candidate is
/// a `#` that begins a line or follows white space (a space or a tab). It is a
/// comment when cutting the front matter from it to the end of its line leaves the
/// reading unchanged: the same entries, with the same values. Cutting inside a
/// quoted string leaves the string unclosed or shorter, and inside a block scalar
/// (`|` or `>`) changes its text, so such a `#` is no comment. A front matter that
/// cannot be read has none.
pub fn comment_line(yaml: &str) -> Option<u32> {
    let whole = entries(yaml).ok()?;
    let mut offset = 0;
    for (n, line) in yaml.split_inclusive('\n').enumerate() {
        let end = line.trim_end_matches('\n').len();
        let mut before: Option<char> = None;
        for (at, c) in line.char_indices() {
            if c == '#' && before.is_none_or(|b| b == ' ' || b == '\t') {
                let cut = format!("{}{}", &yaml[..offset + at], &yaml[offset + end..]);
                if entries(&cut).as_ref() == Ok(&whole) {
                    return Some(u32::try_from(n + 1).unwrap_or(u32::MAX));
                }
            }
            before = Some(c);
        }
        offset += line.len();
    }
    None
}

/// Why a front matter gives no entries.
///
/// The one place the two kinds of fault are told apart ([`entries`]): a fault of the
/// whole, which ends the reading, or the faults of values, which are independent of
/// each other.
#[derive(Debug, PartialEq, Eq)]
enum Unread {
    /// The front matter is no mapping of text keys, or is not YAML.
    Whole(RawProblem),
    /// The front matter is a mapping, and these values (every one, in the order
    /// written) are of kinds a front matter does not hold.
    Values(NonEmptyVec<UnheldValue>),
}

/// The entries of a front matter.
///
/// The order of the judgement, the one place it is made: the parser's error; a front
/// matter that is no mapping; a text key written twice at the top level (the first
/// such key); a key that is no text; and only then the values, each in the order
/// written, every one of a kind a front matter does not hold collected.
fn entries(yaml: &str) -> Result<BTreeMap<String, RawValue>, Unread> {
    if yaml.trim().is_empty() {
        return Ok(BTreeMap::new());
    }
    let front: Front = serde_norway::from_str(yaml).map_err(|error| {
        Unread::Whole(RawProblem::Syntax {
            // The parser counts from the first line of the YAML, which is the second
            // line of the file.
            line: error
                .location()
                .and_then(|at| u32::try_from(at.line()).ok())
                .map(|line| line + 1),
        })
    })?;
    let pairs = match front {
        Front::Mapping(pairs) => pairs,
        Front::Duplicate(key) => return Err(Unread::Whole(RawProblem::DuplicateKey { key })),
        Front::NotMapping => return Err(Unread::Whole(RawProblem::NotAMapping)),
    };
    let mut out = BTreeMap::new();
    let mut unheld = Vec::new();
    for (key, Held(value)) in pairs {
        let Key::Text(key) = key else {
            return Err(Unread::Whole(RawProblem::NotAMapping));
        };
        match value {
            Ok(raw) => {
                out.insert(key, raw);
            }
            Err(kind) => unheld.push(UnheldValue::new(key, kind)),
        }
    }
    match NonEmptyVec::from_vec(unheld) {
        Ok(values) => Err(Unread::Values(values)),
        Err(_) => Ok(out),
    }
}

/// The top level of a front matter as the parser streams it.
///
/// A key written twice is found here, in the parser's own stream of keys, and not in
/// a mapping built from it (which keeps one value per key) and not in the wording of
/// an error. Only the top level is looked at: the contents of a value that is a
/// mapping, a list in a list or a tagged value are skipped as they stream, so a key
/// written twice inside a nested mapping is no [`RawProblem::DuplicateKey`], and the
/// value is read as a mapping ([`UnheldKind::Mapping`]).
enum Front {
    /// A mapping, with every entry in the order written.
    Mapping(Vec<(Key, Held)>),
    /// A mapping with a text key written twice: the first such key.
    Duplicate(String),
    /// A scalar, a sequence, or a tagged value.
    NotMapping,
}

impl<'de> Deserialize<'de> for Front {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct FrontVisitor;

        impl<'de> Visitor<'de> for FrontVisitor {
            type Value = Front;

            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("a YAML document")
            }

            fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Front, A::Error> {
                let mut pairs: Vec<(Key, Held)> = Vec::new();
                let mut seen: BTreeSet<String> = BTreeSet::new();
                let mut duplicate = None;
                while let Some((key, value)) = map.next_entry::<Key, Held>()? {
                    if let Key::Text(text) = &key
                        && !seen.insert(text.clone())
                        && duplicate.is_none()
                    {
                        duplicate = Some(text.clone());
                    }
                    pairs.push((key, value));
                }
                Ok(match duplicate {
                    Some(key) => Front::Duplicate(key),
                    None => Front::Mapping(pairs),
                })
            }

            fn visit_bool<E: de::Error>(self, _: bool) -> Result<Front, E> {
                Ok(Front::NotMapping)
            }

            fn visit_i64<E: de::Error>(self, _: i64) -> Result<Front, E> {
                Ok(Front::NotMapping)
            }

            fn visit_u64<E: de::Error>(self, _: u64) -> Result<Front, E> {
                Ok(Front::NotMapping)
            }

            fn visit_u128<E: de::Error>(self, _: u128) -> Result<Front, E> {
                Ok(Front::NotMapping)
            }

            fn visit_i128<E: de::Error>(self, _: i128) -> Result<Front, E> {
                Ok(Front::NotMapping)
            }

            fn visit_f64<E: de::Error>(self, _: f64) -> Result<Front, E> {
                Ok(Front::NotMapping)
            }

            fn visit_str<E: de::Error>(self, _: &str) -> Result<Front, E> {
                Ok(Front::NotMapping)
            }

            fn visit_unit<E: de::Error>(self) -> Result<Front, E> {
                Ok(Front::NotMapping)
            }

            fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Front, A::Error> {
                while seq.next_element::<IgnoredAny>()?.is_some() {}
                Ok(Front::NotMapping)
            }

            fn visit_enum<A: EnumAccess<'de>>(self, data: A) -> Result<Front, A::Error> {
                skip_tagged(data)?;
                Ok(Front::NotMapping)
            }
        }

        deserializer.deserialize_any(FrontVisitor)
    }
}

/// A scalar as the parser gives it, with the number translated to what a front
/// matter holds: the one place a number of the parser meets the range of the
/// integers a front matter holds.
enum Scalar {
    Text(String),
    Bool(bool),
    Integer(i64),
    TooLarge,
    TooSmall,
    Float,
    Null,
}

/// What a node of the YAML is read as, in one of the places it stands: a key
/// ([`Key`]), the value of a key ([`Held`]), an item of a list ([`HeldItem`]). The
/// one visitor ([`Node`]) shows the parser's node as a [`Scalar`], a list, a mapping
/// or a tagged value; each place says what it makes of them. The contents of a
/// mapping and of a tagged value are skipped as they stream.
trait Place: Sized {
    fn scalar(scalar: Scalar) -> Self;

    fn list<'de, A: SeqAccess<'de>>(items: A) -> Result<Self, A::Error>;

    fn mapping() -> Self;

    fn tagged() -> Self;
}

/// The visitor of a [`Place`].
struct Node<P>(std::marker::PhantomData<P>);

impl<'de, P: Place> Visitor<'de> for Node<P> {
    type Value = P;

    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("a YAML node")
    }

    fn visit_str<E: de::Error>(self, text: &str) -> Result<P, E> {
        Ok(P::scalar(Scalar::Text(text.to_owned())))
    }

    fn visit_bool<E: de::Error>(self, value: bool) -> Result<P, E> {
        Ok(P::scalar(Scalar::Bool(value)))
    }

    fn visit_i64<E: de::Error>(self, value: i64) -> Result<P, E> {
        Ok(P::scalar(Scalar::Integer(value)))
    }

    fn visit_u64<E: de::Error>(self, value: u64) -> Result<P, E> {
        Ok(P::scalar(
            i64::try_from(value).map_or(Scalar::TooLarge, Scalar::Integer),
        ))
    }

    fn visit_u128<E: de::Error>(self, _: u128) -> Result<P, E> {
        Ok(P::scalar(Scalar::TooLarge))
    }

    fn visit_i128<E: de::Error>(self, _: i128) -> Result<P, E> {
        Ok(P::scalar(Scalar::TooSmall))
    }

    /// A number with a decimal point or an exponent. The parser also gives a whole
    /// number beyond its 128-bit integers as a float, and the text it was written
    /// as is not seen here, so such a number is told as a float too: a known limit.
    fn visit_f64<E: de::Error>(self, _: f64) -> Result<P, E> {
        Ok(P::scalar(Scalar::Float))
    }

    fn visit_unit<E: de::Error>(self) -> Result<P, E> {
        Ok(P::scalar(Scalar::Null))
    }

    fn visit_none<E: de::Error>(self) -> Result<P, E> {
        Ok(P::scalar(Scalar::Null))
    }

    fn visit_some<D: Deserializer<'de>>(self, deserializer: D) -> Result<P, D::Error> {
        deserializer.deserialize_any(self)
    }

    fn visit_seq<A: SeqAccess<'de>>(self, items: A) -> Result<P, A::Error> {
        P::list(items)
    }

    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<P, A::Error> {
        while map.next_entry::<IgnoredAny, IgnoredAny>()?.is_some() {}
        Ok(P::mapping())
    }

    fn visit_enum<A: EnumAccess<'de>>(self, data: A) -> Result<P, A::Error> {
        skip_tagged(data)?;
        Ok(P::tagged())
    }
}

/// Skips a tagged value: its tag and what it tags.
fn skip_tagged<'de, A: EnumAccess<'de>>(data: A) -> Result<(), A::Error> {
    let (IgnoredAny, contents) = data.variant::<IgnoredAny>()?;
    contents.newtype_variant::<IgnoredAny>()?;
    Ok(())
}

/// Skips the rest of a list.
fn skip_list<'de, A: SeqAccess<'de>>(mut items: A) -> Result<(), A::Error> {
    while items.next_element::<IgnoredAny>()?.is_some() {}
    Ok(())
}

/// A key of the top level: a text, or something else.
enum Key {
    Text(String),
    NotText,
}

impl Place for Key {
    fn scalar(scalar: Scalar) -> Self {
        match scalar {
            Scalar::Text(text) => Self::Text(text),
            _ => Self::NotText,
        }
    }

    fn list<'de, A: SeqAccess<'de>>(items: A) -> Result<Self, A::Error> {
        skip_list(items)?;
        Ok(Self::NotText)
    }

    fn mapping() -> Self {
        Self::NotText
    }

    fn tagged() -> Self {
        Self::NotText
    }
}

impl<'de> Deserialize<'de> for Key {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        deserializer.deserialize_any(Node(std::marker::PhantomData))
    }
}

/// The value of a key: what a front matter holds, or the kind it was read as. No value
/// (`key:`, `null`, `~`) is held as it is, [`RawValue::Null`]; what it comes to is
/// not decided here.
struct Held(Result<RawValue, UnheldKind>);

impl Place for Held {
    fn scalar(scalar: Scalar) -> Self {
        Self(match scalar {
            Scalar::Null => Ok(RawValue::Null),
            Scalar::Text(text) => Ok(RawValue::Text(text)),
            Scalar::Bool(value) => Ok(RawValue::Bool(value)),
            Scalar::Integer(value) => Ok(RawValue::Integer(value)),
            Scalar::TooLarge => Err(UnheldKind::TooLarge),
            Scalar::TooSmall => Err(UnheldKind::TooSmall),
            Scalar::Float => Err(UnheldKind::Float),
        })
    }

    /// A list of texts, or the kind of its first item that is no text.
    fn list<'de, A: SeqAccess<'de>>(mut items: A) -> Result<Self, A::Error> {
        let mut texts = Vec::new();
        let mut unheld = None;
        while let Some(item) = items.next_element::<HeldItem>()? {
            match item {
                HeldItem::Text(text) => texts.push(text),
                HeldItem::Item(kind) => {
                    unheld.get_or_insert(kind);
                }
            }
        }
        Ok(Self(match unheld {
            Some(kind) => Err(UnheldKind::Item(kind)),
            None => Ok(RawValue::List(texts)),
        }))
    }

    fn mapping() -> Self {
        Self(Err(UnheldKind::Mapping))
    }

    fn tagged() -> Self {
        Self(Err(UnheldKind::Tagged))
    }
}

impl<'de> Deserialize<'de> for Held {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        deserializer.deserialize_any(Node(std::marker::PhantomData))
    }
}

/// An item of a list: a text, or the kind it was read as.
enum HeldItem {
    Text(String),
    Item(UnheldItem),
}

impl Place for HeldItem {
    fn scalar(scalar: Scalar) -> Self {
        match scalar {
            Scalar::Text(text) => Self::Text(text),
            Scalar::Bool(_) => Self::Item(UnheldItem::Bool),
            Scalar::Integer(_) | Scalar::TooLarge | Scalar::TooSmall | Scalar::Float => {
                Self::Item(UnheldItem::Number)
            }
            Scalar::Null => Self::Item(UnheldItem::Null),
        }
    }

    fn list<'de, A: SeqAccess<'de>>(items: A) -> Result<Self, A::Error> {
        skip_list(items)?;
        Ok(Self::Item(UnheldItem::List))
    }

    fn mapping() -> Self {
        Self::Item(UnheldItem::Mapping)
    }

    fn tagged() -> Self {
        Self::Item(UnheldItem::Tagged)
    }
}

impl<'de> Deserialize<'de> for HeldItem {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        deserializer.deserialize_any(Node(std::marker::PhantomData))
    }
}

/// The text of a document: the front matter, then the body.
///
/// The front matter is the flat form [`read`](fn@read) reads: one `name: value` line per
/// entry, `id` first, then `key`, then the rest by name. A text is always in double
/// quotes, with `\` escapes for a quote, a backslash and every control or line-break
/// character, so it is one line and is never read as a number, a truth value or
/// null; a list is a flow sequence of such texts; a whole number and a truth value
/// are written bare. A name is written bare when it is a plain lowercase word YAML
/// would not read as anything but a text, and quoted otherwise. The whole text is
/// then put through the normalization rule ([`normalize`]): `\n` line endings, no
/// white space at the end of a line, no blank lines at the end, one final newline.
/// So it is the one form of a document, whatever the body given, and reads back
/// into the same entries and the body after the same rule.
pub fn render(entries: &BTreeMap<String, RawValue>, body: &str) -> String {
    normalize(&format!("{}{body}", render_front(entries)))
}

/// The front matter [`render`] writes, its two lines `---` included.
fn render_front(entries: &BTreeMap<String, RawValue>) -> String {
    let mut out = String::from("---\n");
    let first = ["id", "key"];
    let ordered = first
        .iter()
        .filter_map(|name| entries.get_key_value(*name))
        .chain(
            entries
                .iter()
                .filter(|(name, _)| !first.contains(&name.as_str())),
        );
    for (name, value) in ordered {
        out.push_str(&yaml_name(name));
        out.push_str(": ");
        out.push_str(&yaml_value(value));
        out.push('\n');
    }
    out.push_str("---\n");
    out
}

/// The words YAML may read as something other than a text.
const YAML_WORDS: [&str; 10] = [
    "y", "n", "yes", "no", "on", "off", "true", "false", "null", "~",
];

fn yaml_name(name: &str) -> String {
    let plain = name.starts_with(|c: char| c.is_ascii_lowercase())
        && name
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '_')
        && !YAML_WORDS.contains(&name);
    if plain { name.to_owned() } else { quoted(name) }
}

fn yaml_value(value: &RawValue) -> String {
    match value {
        RawValue::Null => "null".to_owned(),
        RawValue::Text(text) => quoted(text),
        RawValue::Integer(n) => n.to_string(),
        RawValue::Bool(b) => b.to_string(),
        RawValue::List(items) => {
            let items: Vec<String> = items.iter().map(|item| quoted(item)).collect();
            format!("[{}]", items.join(", "))
        }
    }
}

/// A text as a YAML double-quoted scalar on one line.
fn quoted(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 2);
    out.push('"');
    for c in text.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\t' => out.push_str("\\t"),
            '\r' => out.push_str("\\r"),
            c if c.is_control() || matches!(c, '\u{2028}' | '\u{2029}' | '\u{feff}') => {
                out.push_str(&format!("\\u{:04X}", u32::from(c)));
            }
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// The `id:` a line gives, if it is an `id:` line with a value that is a [`DocId`]:
/// the one place a line is read for an identity.
fn id_of(line: &str) -> Option<DocId> {
    let value = line.strip_prefix("id:")?;
    let value = value.split(" #").next().unwrap_or(value).trim();
    let Ok(id) = value.trim_matches(['"', '\'']).parse::<DocId>() else {
        return None;
    };
    Some(id)
}

/// The identity in `ids`: the one, when they are exactly one.
fn the_one(ids: BTreeSet<DocId>) -> Option<DocId> {
    let mut ids = ids.into_iter();
    match (ids.next(), ids.next()) {
        (Some(id), None) => Some(id),
        _ => None,
    }
}

/// The identity a file that could not be read shows, found by looking at the lines
/// it is given only: [`ReadId::Read`] when they give exactly one `id:`,
/// [`ReadId::Unreadable`] when none gives one and when they give two (a file does
/// not say which is the document's). Which lines are the front matter's is the
/// caller's.
fn seen_id<'a>(lines: impl Iterator<Item = &'a str>) -> ReadId {
    the_one(lines.filter_map(id_of).collect()).map_or(ReadId::Unreadable, ReadId::Read)
}

/// How the front matter of a file that holds merge conflict markers was opened.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Opened {
    /// By the line `---` the sides share, outside every conflict: the first line of
    /// the text ([`FrontMatterStart::Opens`]).
    Common,
    /// By nothing outside the conflict: the first line of the text starts it
    /// ([`FrontMatterStart::MarkerOpens`]), so a side opens its own front matter
    /// or has none.
    None,
}

/// Where a line is in a merge conflict.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Span {
    /// Outside every conflict: both sides hold the line.
    Common,
    /// After `<<<<<<<`: ours.
    Ours,
    /// After `|||||||`: the common ancestor, which is no side's.
    Base,
    /// After `=======`: theirs.
    Theirs,
}

/// How a side stands to its front matter.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Matter {
    /// The side has none, or has not opened it yet.
    NotOpened,
    Open,
    Closed,
    /// The side's part begins with a line that begins with `---` and is not the
    /// opening: a mistyped delimiter.
    Mistyped,
}

/// What `conflict` has seen of a side of a conflict, ours or theirs: its front
/// matter and the identities in it.
#[derive(Debug)]
struct ConflictSide {
    matter: Matter,
    ids: BTreeSet<DocId>,
}

/// Whether a file that could not be read is a merge conflict that touches its front
/// matter, and the identity it shows if it is: the one place that is decided.
///
/// The file holds two documents, ours and theirs, one on each side of a conflict,
/// and each side has the front matter its own lines give: what the sides share is
/// both's, a line between `<<<<<<<` and `=======` ours only, between `=======` and
/// `>>>>>>>` theirs only, and the common ancestor (`|||||||`) is neither's. A side
/// opens a front matter as a text does (`opened` says how): by the line `---`
/// shared, or, for a text that starts with the conflict, by `---` as the first line
/// of its part of the conflict the text starts with. A side whose part begins with
/// a line that begins with `---` and is not exactly that (`----`, `--- x`) tried to
/// open one and did not, as [`FrontMatterStart::BadOpening`]: it touches the front
/// matter and shows no identity, so a mistyped delimiter is not passed over as a
/// file with no front matter. One that opens with anything else has none, a `---`
/// later in it is a thematic break, and so is one that begins a side of a later
/// conflict, which is in the body. A `---` after the opening closes it, and an
/// `id:` is the side's only while its front matter is open.
///
/// The conflict touches the front matter when a conflict that ends (`>>>>>>>`)
/// starts while a side's front matter is open, or opens one. `None` when none does,
/// and when no side opened a front matter, so the file is read as it would be
/// without this. Otherwise `Some`: when every side that opened one shows a single
/// identity, what [`ReadId::of_sides`] makes of them ([`ReadId::Read`] when they are
/// the same, [`ReadId::OneOf`] when they differ: the file holds one of them, and
/// which is not said); [`ReadId::Unreadable`] when one shows none or several, or
/// opened its front matter with a mistyped delimiter.
fn conflict(text: &str, opened: Opened) -> Option<ReadId> {
    let start = match opened {
        Opened::Common => Matter::Open,
        Opened::None => Matter::NotOpened,
    };
    let mut sides = [
        ConflictSide {
            matter: start,
            ids: BTreeSet::new(),
        },
        ConflictSide {
            matter: start,
            ids: BTreeSet::new(),
        },
    ];
    let mut span = Span::Common;
    // The line is the first of the part of a side, which opens its front matter.
    let mut first = false;
    // The conflict is the one the text starts with: only there does a side open a
    // front matter of its own.
    let mut opening = opened == Opened::None;
    let mut hunk_touches = false;
    let mut touched = false;
    // The line that opens a front matter is the first of the text. A byte order mark
    // before a marker that starts the text is set aside, as where the first line is
    // read, so that line is the marker here too.
    let lines = without_mark(text)
        .lines()
        .skip(usize::from(opened == Opened::Common));
    for line in lines {
        let first_here = std::mem::take(&mut first);
        match (marker(line), span) {
            (Some(Marker::Start), Span::Common) => {
                span = Span::Ours;
                first = true;
                hunk_touches = sides.iter().any(|side| side.matter == Matter::Open);
                continue;
            }
            (Some(Marker::Base), Span::Ours) => {
                span = Span::Base;
                continue;
            }
            (Some(Marker::Middle), Span::Ours | Span::Base) => {
                span = Span::Theirs;
                first = true;
                continue;
            }
            (Some(Marker::End), Span::Ours | Span::Base | Span::Theirs) => {
                span = Span::Common;
                touched |= hunk_touches;
                opening = false;
                continue;
            }
            _ => {}
        }
        let held: &[usize] = match span {
            Span::Common => &[0, 1],
            Span::Ours => &[0],
            Span::Base => &[],
            Span::Theirs => &[1],
        };
        for &at in held {
            let side = &mut sides[at];
            match side.matter {
                Matter::NotOpened if opening && first_here && tries_to_open(line) => {
                    side.matter = if is_delimiter(line) {
                        Matter::Open
                    } else {
                        Matter::Mistyped
                    };
                    hunk_touches = true;
                }
                Matter::Open if is_delimiter(line) => side.matter = Matter::Closed,
                Matter::Open => side.ids.extend(id_of(line)),
                Matter::NotOpened | Matter::Closed | Matter::Mistyped => {}
            }
        }
    }
    if !touched {
        return None;
    }
    let mut shown = BTreeSet::new();
    let mut any_opened = false;
    for side in sides {
        if side.matter == Matter::NotOpened {
            continue;
        }
        any_opened = true;
        if side.matter == Matter::Mistyped {
            return Some(ReadId::Unreadable);
        }
        let Some(id) = the_one(side.ids) else {
            return Some(ReadId::Unreadable);
        };
        shown.insert(id);
    }
    if !any_opened {
        return None;
    }
    Some(ReadId::of_sides(shown))
}

#[cfg(test)]
mod tests {
    use super::*;
    use whyshelf_core::app::shelf::{
        Creation, DocRemove, DocReplace, DocSource, DocWrite, Removal, Rendered, Replaceable,
        Replacement, Side,
    };
    use whyshelf_core::domain::shelf::build::FormEntries;
    use whyshelf_core::domain::shelf::schema::{BookDir, BookRange};

    const ID: &str = "019a6f3e-0000-7000-8000-000000000001";

    fn origin() -> RelPath {
        "docs/a.md".parse().unwrap()
    }

    #[test]
    fn a_front_matter_becomes_entries_and_a_body() {
        let text = format!(
            "---\r\nid: {ID}\nkey: S-1\nsev: 3\nok: true\ngrounds: [F-1, F-2]\n---\nBody.  \n\n\n"
        );
        let DocReading::Parsed { entries, body, .. } = read(origin(), &text) else {
            panic!("parsed");
        };
        assert_eq!(entries["id"], RawValue::Text(ID.to_owned()));
        assert_eq!(entries["key"], RawValue::Text("S-1".to_owned()));
        assert_eq!(entries["sev"], RawValue::Integer(3));
        assert_eq!(entries["ok"], RawValue::Bool(true));
        assert_eq!(
            entries["grounds"],
            RawValue::List(vec!["F-1".to_owned(), "F-2".to_owned()])
        );
        assert_eq!(body, "Body.\n");
    }

    #[test]
    fn no_value_is_read_as_null_whatever_way_it_is_written() {
        let text = format!("---\nid: {ID}\na:\nb: null\nc: ~\nd: Null\ne: \"\"\n---\n");
        let DocReading::Parsed { entries, .. } = read(origin(), &text) else {
            panic!("parsed");
        };
        for name in ["a", "b", "c", "d"] {
            assert_eq!(entries[name], RawValue::Null, "{name}");
        }
        // A text that is empty is a text: what it says is not decided here.
        assert_eq!(entries["e"], RawValue::Text(String::new()));
    }

    #[test]
    fn a_null_in_a_list_is_still_a_value_a_front_matter_does_not_hold() {
        let text = format!("---\nid: {ID}\nv: [a, ~]\n---\n");
        assert!(matches!(read(origin(), &text), DocReading::Unheld { .. }));
    }

    #[test]
    fn no_value_is_written_as_null_and_reads_back() {
        let mut entries = BTreeMap::new();
        entries.insert("id".to_owned(), RawValue::Text(ID.to_owned()));
        entries.insert("key".to_owned(), RawValue::Null);
        let text = render(&entries, "");
        assert!(text.contains("\nkey: null\n"), "{text}");
        let DocReading::Parsed { entries: back, .. } = read(origin(), &text) else {
            panic!("parsed");
        };
        assert_eq!(back, entries);
    }

    #[test]
    fn a_key_written_twice_is_reported_with_the_identity() {
        let text = format!("---\nid: {ID}\nkey: a\nkey: b\n---\n");
        let DocReading::Unparsed { id, why, .. } = read(origin(), &text) else {
            panic!("unparsed");
        };
        assert_eq!(id, ReadId::Read(ID.parse().unwrap()));
        assert_eq!(
            why,
            RawProblem::DuplicateKey {
                key: "key".to_owned()
            }
        );
    }

    #[test]
    fn a_value_a_front_matter_does_not_hold_is_reported() {
        let text = format!("---\nid: {ID}\nnested: {{a: 1}}\n---\n");
        let DocReading::Unheld { values, .. } = read(origin(), &text) else {
            panic!("unheld");
        };
        assert_eq!(
            values,
            NonEmptyVec::new(
                UnheldValue::new("nested".to_owned(), UnheldKind::Mapping),
                Vec::new()
            )
        );
    }

    #[test]
    fn each_kind_a_front_matter_does_not_hold_is_named() {
        let cases = [
            ("v: 1.5", UnheldKind::Float),
            ("v: 1.0", UnheldKind::Float),
            ("v: 1e3", UnheldKind::Float),
            ("v: 18446744073709551615", UnheldKind::TooLarge),
            ("v: 9223372036854775808", UnheldKind::TooLarge),
            ("v: -99999999999999999999999", UnheldKind::TooSmall),
            ("v: {a: 1}", UnheldKind::Mapping),
            ("v: !x a", UnheldKind::Tagged),
            ("v: [a, ~]", UnheldKind::Item(UnheldItem::Null)),
            ("v: [a, true]", UnheldKind::Item(UnheldItem::Bool)),
            ("v: [a, 3]", UnheldKind::Item(UnheldItem::Number)),
            (
                "v: [a, 18446744073709551616]",
                UnheldKind::Item(UnheldItem::Number),
            ),
            (
                "v: [JSON Lines: one record per line]",
                UnheldKind::Item(UnheldItem::Mapping),
            ),
            ("v: [a, [b]]", UnheldKind::Item(UnheldItem::List)),
            ("v: [a, !x b]", UnheldKind::Item(UnheldItem::Tagged)),
        ];
        for (line, kind) in cases {
            let text = format!("---\nid: {ID}\n{line}\n---\n");
            let DocReading::Unheld { values, .. } = read(origin(), &text) else {
                panic!("unheld: {line}");
            };
            assert_eq!(
                values,
                NonEmptyVec::new(UnheldValue::new("v".to_owned(), kind), Vec::new()),
                "{line}"
            );
        }
    }

    #[test]
    fn an_item_read_as_a_mapping_is_told_to_be_quoted() {
        let text = format!("---\nid: {ID}\nrejected: [JSON Lines: one record per line]\n---\n");
        let DocReading::Unheld { values, .. } = read(origin(), &text) else {
            panic!("unheld");
        };
        let message = values.first().to_string();
        assert!(
            message.contains("a list with an item that is a mapping"),
            "{message}"
        );
        assert!(message.contains("put it in quotes"), "{message}");
    }

    #[test]
    fn a_file_without_a_front_matter_is_said_to_have_none() {
        assert!(matches!(
            read(origin(), "Just text.\n"),
            DocReading::Unparsed {
                why: RawProblem::NoFrontMatter,
                ..
            }
        ));
    }

    #[test]
    fn a_front_matter_that_is_not_closed_is_said_so_and_keeps_its_identity() {
        assert!(matches!(
            read(origin(), &format!("---\nid: {ID}\n")),
            DocReading::Unparsed {
                why: RawProblem::Unclosed,
                id: ReadId::Read(_),
                ..
            }
        ));
    }

    #[test]
    fn a_front_matter_that_is_no_mapping_is_said_so() {
        for yaml in [
            "- a\n- b",
            "just: [",
            "plain",
            "18446744073709551616: a",
            "!x a",
            "18446744073709551616",
        ] {
            let DocReading::Unparsed { why, .. } = read(origin(), &format!("---\n{yaml}\n---\n"))
            else {
                panic!("unparsed");
            };
            let expected_mapping = yaml != "just: [";
            assert_eq!(why == RawProblem::NotAMapping, expected_mapping, "{yaml}");
        }
    }

    #[test]
    fn a_key_written_twice_inside_a_nested_mapping_is_no_top_level_duplicate() {
        let text = format!("---\nid: {ID}\nnested:\n  a: 1\n  a: 2\n---\n");
        let DocReading::Unheld { values, .. } = read(origin(), &text) else {
            panic!("unheld");
        };
        assert_eq!(
            values,
            NonEmptyVec::new(
                UnheldValue::new("nested".to_owned(), UnheldKind::Mapping),
                Vec::new()
            ),
            "a nested mapping is not a value a front matter holds"
        );
    }

    #[test]
    fn the_first_of_several_duplicate_keys_is_named() {
        let text = format!("---\nid: {ID}\nb: 1\na: 1\na: 2\nb: 2\n---\n");
        let DocReading::Unparsed { why, .. } = read(origin(), &text) else {
            panic!("unparsed");
        };
        assert_eq!(
            why,
            RawProblem::DuplicateKey {
                key: "a".to_owned()
            }
        );
    }

    /// Texts YAML could take for something else, or that need an escape.
    const TRICKY: [&str; 22] = [
        "",
        "plain",
        "with \"quotes\" and \\ backslash",
        "two\nlines\r\nand a tab\t",
        " leading and trailing ",
        "key: value # not a comment",
        "---",
        "- a list?",
        "[not, a, list]",
        "{not: a map}",
        "null",
        "~",
        "true",
        "no",
        "123",
        "-7",
        "1e3",
        "0x1F",
        "日本語と emoji 🦀",
        "\u{1}\u{7f}\u{85}\u{2028}\u{2029}\u{feff}",
        "'single'",
        "&anchor *alias !tag %directive @at `tick`",
    ];

    #[test]
    fn what_is_rendered_reads_back_into_the_same_entries() {
        let mut entries = BTreeMap::new();
        entries.insert("id".to_owned(), RawValue::Text(ID.to_owned()));
        entries.insert("key".to_owned(), RawValue::Text("V-4".to_owned()));
        for (i, text) in TRICKY.iter().enumerate() {
            entries.insert(format!("t{i}"), RawValue::Text((*text).to_owned()));
        }
        entries.insert(
            "all".to_owned(),
            RawValue::List(TRICKY.iter().map(|t| (*t).to_owned()).collect()),
        );
        entries.insert("none".to_owned(), RawValue::List(Vec::new()));
        entries.insert("min".to_owned(), RawValue::Integer(i64::MIN));
        entries.insert("max".to_owned(), RawValue::Integer(i64::MAX));
        entries.insert("zero".to_owned(), RawValue::Integer(0));
        entries.insert("yes".to_owned(), RawValue::Bool(true));
        entries.insert("off".to_owned(), RawValue::Bool(false));
        entries.insert(
            "123".to_owned(),
            RawValue::Text("a name that is a number".to_owned()),
        );
        entries.insert("Odd Name".to_owned(), RawValue::Text("x".to_owned()));
        for body in [
            "",
            "Body.",
            "Body.  \n\n\nMore.\t\n\n",
            "---\nnot front matter\n",
        ] {
            let text = render(&entries, body);
            assert!(text.starts_with(&format!("---\nid: \"{ID}\"\nkey: \"V-4\"\n")));
            let DocReading::Parsed {
                entries: back,
                body: back_body,
                ..
            } = read(origin(), &text)
            else {
                panic!("parsed: {text}");
            };
            assert_eq!(back, entries, "{text}");
            assert_eq!(back_body, normalize(body), "{text}");
        }
    }

    /// Front matters written every way a person writes them, each with no comment.
    const WRITTEN: [&str; 21] = [
        "id: ID\nkey: S-1\ntext: |\n  a literal block\n  # with a hash line, which is text\n",
        "id: ID\nkey: S-1\ntext: >-\n  a folded\n  block # and a hash\n",
        "{id: ID, key: S-1, title: a flow mapping}\n",
        "id: ID\nkey: S-1\ntitle: &t shared\nalso: *t\n",
        "id: ID\nkey: S-1\ntitle: !!str 123\n",
        "key: S-1\nid: ID\ntitle: Plain words, unquoted\n",
        "id: ID\nkey: S-1\nserves: [REQ-2, REQ-5]\ngrounds: []\n",
        "id: 'ID'\nkey: 'S-1'\ntitle: 'single, with ''quotes'''\n",
        "id: ID\nkey: S-1\nlist:\n  - a\n  - b c\n",
        "id: ID\nkey: S-1\nlist:\n- a\n- \"b\"\n",
        "id: ID\nkey: S-1\ncount: 3\nok: true\nno: false\nneg: -7\n",
        "id: ID\nkey: S-1\nversion: \"3\"\nword: \"true\"\nyes: yes\n",
        "\"id\": ID\n'key': S-1\n\"Odd Name\": x\n",
        "id: ID\nkey: S-1\nissues: [\"#30\", '#31']\ntitle: \"a # inside quotes\"\n",
        "id: ID\nkey: S-1\ntitle: it's a#b, not a comment\n",
        "id: ID\nkey: S-1\ntitle: folded\n  over two lines\n",
        "id: ID\nkey: S-1\ntitle: \"a double\n  quoted # over lines\"\n",
        "id:   ID   \nkey:\tS-1\ntitle:    spaced   \n",
        "id: ID\nkey: S-1\nrejected: [\"JSON Lines: one record per line\", plain]\n",
        "id: ID\nkey: S-1\nescaped: \"tab\\tand \\u00e9\"\n",
        "id: ID\nkey: S-1\nflow: [a,b ,  c]\n",
    ];

    /// The bodies a file can have after its front matter.
    const BODIES: [&str; 5] = [
        "",
        "Body.\n",
        "Body.  \n\n\nMore.\t\n\n",
        "No final newline",
        "---\nnot front matter\n",
    ];

    #[test]
    fn every_front_matter_renders_to_a_text_that_reads_as_the_file() {
        for yaml in WRITTEN {
            let yaml = yaml.replace("ID", ID);
            for body in BODIES {
                for text in [
                    format!("---\n{yaml}---\n{body}"),
                    format!("---\n{yaml}---\n{body}").replace('\n', "\r\n"),
                    format!("--- \n{yaml}---\t\n{body}"),
                ] {
                    let FileRead { reading, .. } = file_read(origin(), text.clone().into_bytes());
                    assert!(
                        matches!(reading, DocReading::Parsed { .. }),
                        "parsed: {text:?}"
                    );
                    let Rendered {
                        text: formed,
                        reads_as,
                    } = rendered_of(&reading);
                    assert_eq!(reads_as, reading, "{text:?}");
                    let formed = String::from_utf8(formed).unwrap();
                    // The whole text is normalized, the body with it.
                    assert_eq!(formed, normalize(&formed), "{text:?}");
                    assert!(!formed.contains('\r'), "{formed:?}");
                    assert!(formed.ends_with(&normalize(body)), "{formed:?}");
                    assert!(formed.starts_with(&format!("---\nid: \"{ID}\"\nkey: \"S-1\"\n")));
                    // And the text in the form is in the form.
                    assert_eq!(
                        rendered_of(&read(origin(), &formed)).text,
                        formed.clone().into_bytes(),
                        "{formed:?}"
                    );
                }
            }
        }
    }

    /// Every entry of a reading as the entries of a form: a book that declares nothing
    /// leaves nothing out, so these are the entries as they are.
    fn all_of(entries: &BTreeMap<String, RawValue>) -> FormEntries {
        use whyshelf_core::domain::shelf::build::{DropNames, Shelf};
        use whyshelf_core::domain::shelf::schema::{
            BookName, BookSchemaDecl, KeyPatternDecl, Schema,
        };
        let decls = vec![BookSchemaDecl {
            name: "spec".to_owned(),
            dir: "docs".to_owned(),
            scope: Default::default(),
            key: KeyPatternDecl::Prefixed {
                prefix: "S".to_owned(),
            },
            description: None,
            fields: Vec::new(),
            links: Vec::new(),
        }];
        let schema = Schema::validate(decls, &BTreeSet::new()).expect("a sound declaration");
        let form = Shelf::form_entries(
            &schema,
            &BookName::new("spec").unwrap(),
            entries,
            &DropNames::none(),
        );
        assert_eq!(form.as_map(), entries, "a book that declares nothing");
        form
    }

    /// The text in the form of a parsed reading, as the adapter renders it.
    fn rendered_of(reading: &DocReading) -> Rendered {
        let DocReading::Parsed {
            origin,
            entries,
            body,
        } = reading
        else {
            panic!("not parsed: {reading:?}");
        };
        MarkdownDocs::new(PathBuf::new()).render(origin, &all_of(entries), body)
    }

    #[test]
    fn a_crlf_file_and_a_body_with_trailing_white_space_are_written_normalized() {
        let in_form = format!("---\nid: \"{ID}\"\nkey: \"S-1\"\n---\nBody.\n\nMore.\n");
        for text in [
            in_form.replace('\n', "\r\n"),
            format!("---\nid: \"{ID}\"\nkey: \"S-1\"\n---\nBody.  \n\t\nMore.\t\n\n\n"),
        ] {
            let got = file_read(origin(), text.clone().into_bytes());
            let Rendered {
                text: formed,
                reads_as,
            } = rendered_of(&got.reading);
            assert_ne!(formed, got.read, "{text:?}");
            assert_eq!(reads_as, got.reading, "{text:?}");
            assert_eq!(String::from_utf8(formed.clone()).unwrap(), in_form);
            let before = hash_of(got.reading);
            assert_eq!(hash_of(read(origin(), &in_form)), before, "{text:?}");
            assert_eq!(rendered_of(&reads_as).text, formed);
        }
    }

    /// The hash the domain gives the document of this reading, in a book of `docs/`.
    fn hash_of(reading: DocReading) -> whyshelf_core::domain::shelf::doc::DocHash {
        use whyshelf_core::domain::shelf::artifact::ArtifactReadings;
        use whyshelf_core::domain::shelf::build::Shelf;
        use whyshelf_core::domain::shelf::schema::{BookSchemaDecl, KeyPatternDecl, Schema};
        let decls = vec![BookSchemaDecl {
            name: "spec".to_owned(),
            dir: "docs".to_owned(),
            scope: Default::default(),
            key: KeyPatternDecl::Prefixed {
                prefix: "S".to_owned(),
            },
            description: None,
            fields: Vec::new(),
            links: Vec::new(),
        }];
        let schema = Schema::validate(decls, &BTreeSet::new()).expect("a sound declaration");
        let shelf = Shelf::build(&schema, vec![reading], &ArtifactReadings::new());
        let doc = shelf.docs().values().next().expect("one valid document");
        *doc.hash()
    }

    #[test]
    fn what_new_writes_is_in_the_form() {
        let mut entries = BTreeMap::new();
        entries.insert("id".to_owned(), RawValue::Text(ID.to_owned()));
        entries.insert("key".to_owned(), RawValue::Text("V-4".to_owned()));
        for (i, text) in TRICKY.iter().enumerate() {
            entries.insert(format!("t{i}"), RawValue::Text((*text).to_owned()));
        }
        for body in [
            "Body.\n",
            "",
            "No final newline",
            "Trailing white space.  \n\t\nMore.\t\n\n\n",
            "Carriage\r\nreturns\r\n",
            "Lone\rreturn\n",
            "  \n\t\n",
            "\n\n\n",
        ] {
            let text = render(&entries, body);
            let read_back = file_read(origin(), text.clone().into_bytes());
            assert_eq!(
                rendered_of(&read_back.reading).text,
                text.clone().into_bytes(),
                "{body:?}: {text:?}"
            );
            assert_eq!(read_back.comment, None, "{body:?}: {text:?}");
        }
    }

    #[test]
    fn a_file_that_cannot_be_read_has_no_comment_to_tell() {
        for bytes in [
            b"no front matter\n".to_vec(),
            format!("---\nid: {ID}\n").into_bytes(),
            format!("---\nid: {ID}\nkey: a\nkey: b\n---\n").into_bytes(),
            vec![
                b'-', b'-', b'-', b'\n', 0xff, b'\n', b'-', b'-', b'-', b'\n',
            ],
        ] {
            let got = file_read(origin(), bytes.clone());
            assert!(matches!(got.reading, DocReading::Unparsed { .. }));
            assert_eq!(got.comment, None);
            assert_eq!(got.read, bytes);
        }
    }

    #[test]
    fn a_comment_is_found_and_a_hash_in_quotes_is_not() {
        let found = [
            ("# a whole line\nid: x\n", 1),
            ("id: x\nkey: y # after a value\n", 2),
            ("id: x\nlist: [a, b]  # after a list\n", 2),
            ("id: x\ntitle: it's plain # and a comment\n", 2),
            ("id: x\ntitle: \"quoted\" # after the quotes\n", 2),
            ("id: x\ntitle: 'it''s' # after the quotes\n", 2),
            ("id: x\n  # indented\n", 2),
            ("id: x\ntitle:\tx\t#after a tab\n", 2),
            ("id: x\nlist:\n  - a # in a block list\n", 3),
            // A quote inside a plain text opens no string.
            ("id: x\ntitle: a:'b # c'\n", 2),
            ("id: x\ntitle: a-'b # c'\n", 2),
            ("id: x\ntitle: x-\"y # z\"\n", 2),
            ("id: x\ntitle: f['x # c']\n", 2),
            // On a continuation line of a plain text over several lines.
            ("id: x\ntitle: folded\n  over # two lines\n", 3),
            // After a block scalar, at the indentation of the mapping.
            ("id: x\ntext: |\n  a\n# after the block\n", 4),
            // A real comment after a `#` that is not one.
            ("id: x\ntitle: \"Issue #24\" # real\n", 2),
        ];
        for (yaml, line) in found {
            assert_eq!(comment_line(yaml), Some(line), "{yaml:?}");
        }
        let none = [
            "id: x\ntitle: \"a # b\"\n",
            "id: x\ntitle: 'a # b'\n",
            "id: x\ntitle: a#b\n",
            "id: x\nissues: [\"#30\", '#31']\n",
            "id: x\ntitle: \"over\n  # two lines\"\n",
            "id: x\ntitle: \"an \\\" escaped # quote\"\n",
            "id: x\ntitle: 'a ''quoted'' # word'\n",
            "id: x\ntitle: \"Issue #24\"\n",
            "id: x\ntitle: 'Issue #24'\n",
            // Inside a block scalar a `#` is text.
            "id: x\ntext: |\n  a\n  # not a comment\n",
            "id: x\ntext: >\n  folded # text\n  more\n",
            "id: x\ntext: |-\n  ends with #\n",
            // A front matter that cannot be read has none.
            "id: x\nid: y # twice\n",
        ];
        for yaml in none {
            assert_eq!(comment_line(yaml), None, "{yaml:?}");
        }
    }

    #[test]
    fn a_file_with_a_comment_is_told_with_its_line() {
        let text = format!("---\nid: {ID}\nkey: S-1 # the key\n---\nBody.\n");
        assert_eq!(file_read(origin(), text.into_bytes()).comment, Some(3));
    }

    #[tokio::test]
    async fn replace_replaces_only_the_file_as_read_and_create_still_never_overwrites() {
        let root = tempfile::tempdir().unwrap();
        let docs = MarkdownDocs::new(root.path().to_path_buf());
        let at: RelPath = "books/a.md".parse().unwrap();
        let file = root.path().join(at.as_str());
        tokio::fs::create_dir_all(file.parent().unwrap())
            .await
            .unwrap();
        tokio::fs::write(&file, "old").await.unwrap();

        assert_eq!(
            docs.replace_read(&at, b"other", b"new").await.unwrap(),
            Replacement::Changed
        );
        assert_eq!(tokio::fs::read(&file).await.unwrap(), b"old");

        assert_eq!(
            docs.replace_read(&at, b"old", b"new").await.unwrap(),
            Replacement::Replaced
        );
        assert_eq!(tokio::fs::read(&file).await.unwrap(), b"new");
        // Nothing is left beside it.
        let mut entries = tokio::fs::read_dir(file.parent().unwrap()).await.unwrap();
        let mut names = Vec::new();
        while let Some(entry) = entries.next_entry().await.unwrap() {
            names.push(entry.file_name());
        }
        assert_eq!(names, ["a.md"]);

        let gone: RelPath = "books/gone.md".parse().unwrap();
        assert_eq!(
            docs.replace_read(&gone, b"", b"new").await.unwrap(),
            Replacement::Changed
        );
        assert!(docs.current(Side::After, &gone).await.unwrap().is_none());

        assert_eq!(
            docs.create(&at, &all_of(&BTreeMap::new()), "Other.\n")
                .await
                .unwrap(),
            Creation::AlreadyThere
        );
        assert_eq!(tokio::fs::read(&file).await.unwrap(), b"new");
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn a_read_only_file_and_a_hard_link_are_kept_from_being_replaced() {
        let root = tempfile::tempdir().unwrap();
        let docs = MarkdownDocs::new(root.path().to_path_buf());
        let dir = root.path().join("docs");
        tokio::fs::create_dir_all(&dir).await.unwrap();
        let unquoted = |n: u32| format!("---\nid: {ID}\nkey: S-{n}\n---\n");
        for n in 1..=3 {
            tokio::fs::write(dir.join(format!("{n}.md")), unquoted(n))
                .await
                .unwrap();
        }
        let read_only = dir.join("2.md");
        let mut permissions = tokio::fs::metadata(&read_only).await.unwrap().permissions();
        permissions.set_readonly(true);
        tokio::fs::set_permissions(&read_only, permissions)
            .await
            .unwrap();
        tokio::fs::hard_link(dir.join("3.md"), root.path().join("elsewhere"))
            .await
            .unwrap();
        // A user the permissions do not hold back (root) can write it after all.
        let writable_anyway = tokio::fs::OpenOptions::new()
            .write(true)
            .open(&read_only)
            .await
            .is_ok();

        let forms = docs
            .forms(
                Side::After,
                &[BookRange::of_dir(BookDir::new("docs").unwrap())],
            )
            .await
            .unwrap();
        let mut got = Vec::new();
        for form in &forms {
            let DocReading::Parsed { origin, .. } = &form.reading else {
                panic!("not parsed: {form:?}");
            };
            got.push(docs.replaceable(origin).await.unwrap());
        }
        assert_eq!(got[0], Replaceable::Yes, "{got:?}");
        if !writable_anyway {
            assert_eq!(got[1], Replaceable::ReadOnly);
        }
        assert_eq!(got[2], Replaceable::Linked);
        // Telling changed nothing.
        for n in 1..=3 {
            assert_eq!(
                tokio::fs::read_to_string(dir.join(format!("{n}.md")))
                    .await
                    .unwrap(),
                unquoted(n)
            );
        }
    }

    #[tokio::test]
    async fn a_write_that_fails_names_its_step_and_path() {
        let root = tempfile::tempdir().unwrap();
        let docs = MarkdownDocs::new(root.path().to_path_buf());
        // A directory where the file would be: the document's directory is a file.
        tokio::fs::write(root.path().join("books"), "a file")
            .await
            .unwrap();
        let at: RelPath = "books/a.md".parse().unwrap();
        let error = docs
            .create(&at, &all_of(&BTreeMap::new()), "")
            .await
            .unwrap_err();
        let message = error.to_string();
        assert!(
            message.starts_with("cannot create the directory ") && message.ends_with("books"),
            "{message}"
        );
    }

    #[tokio::test]
    async fn a_new_file_is_created_with_its_directories_and_never_replaced() {
        let root = tempfile::tempdir().unwrap();
        let docs = MarkdownDocs::new(root.path().to_path_buf());
        let at: RelPath = "books/design/cache/timeout.md".parse().unwrap();
        let mut entries = BTreeMap::new();
        entries.insert("id".to_owned(), RawValue::Text(ID.to_owned()));
        entries.insert("key".to_owned(), RawValue::Text("cache/timeout".to_owned()));
        entries.insert("title".to_owned(), RawValue::Text("T".to_owned()));
        assert_eq!(
            docs.create(&at, &all_of(&entries), "Body.\n")
                .await
                .unwrap(),
            Creation::Created
        );
        let file = root.path().join(at.as_str());
        let written = tokio::fs::read_to_string(&file).await.unwrap();
        assert_eq!(
            written,
            format!("---\nid: \"{ID}\"\nkey: \"cache/timeout\"\ntitle: \"T\"\n---\nBody.\n")
        );

        let other = BTreeMap::new();
        assert_eq!(
            docs.create(&at, &all_of(&other), "Other.\n").await.unwrap(),
            Creation::AlreadyThere
        );
        assert_eq!(tokio::fs::read_to_string(&file).await.unwrap(), written);

        let dirs = [BookRange::of_dir(BookDir::new("books/design").unwrap())];
        let readings = docs.read_docs(Side::After, &dirs).await.unwrap();
        let [DocReading::Parsed { entries: back, .. }] = readings.as_slice() else {
            panic!("one parsed file");
        };
        assert_eq!(*back, entries);
    }

    #[tokio::test]
    async fn the_bytes_create_writes_are_the_text_render_gives() {
        let root = tempfile::tempdir().unwrap();
        let docs = MarkdownDocs::new(root.path().to_path_buf());
        let mut entries = BTreeMap::new();
        entries.insert("id".to_owned(), RawValue::Text(ID.to_owned()));
        entries.insert("key".to_owned(), RawValue::Text("V-4".to_owned()));
        for (i, text) in TRICKY.iter().enumerate() {
            entries.insert(format!("t{i}"), RawValue::Text((*text).to_owned()));
        }
        for (n, body) in [
            "Body.\n",
            "",
            "No final newline",
            "Trailing white space.  \n\t\nMore.\t\n\n\n",
            "Carriage\r\nreturns\r\n",
        ]
        .into_iter()
        .enumerate()
        {
            let at: RelPath = format!("books/{n}.md").parse().unwrap();
            let form = all_of(&entries);
            // Taken before the file is made, as `rekey` takes it.
            let rendered = docs.render(&at, &form, body);
            assert_eq!(
                docs.create(&at, &form, body).await.unwrap(),
                Creation::Created
            );
            assert_eq!(
                tokio::fs::read(root.path().join(at.as_str()))
                    .await
                    .unwrap(),
                rendered.text,
                "{body:?}"
            );
        }
    }

    #[tokio::test]
    async fn a_file_is_put_back_over_what_was_written_and_removed_only_as_it_was_read() {
        let root = tempfile::tempdir().unwrap();
        let docs = MarkdownDocs::new(root.path().to_path_buf());
        let at: RelPath = "books/a.md".parse().unwrap();
        let file = root.path().join(at.as_str());
        tokio::fs::create_dir_all(file.parent().unwrap())
            .await
            .unwrap();
        // Not in the form, as a person left it.
        let before = b"id: x\nkey:   y   # a comment\n";
        tokio::fs::write(&file, before).await.unwrap();

        assert_eq!(
            docs.replace_read(&at, before, b"written").await.unwrap(),
            Replacement::Replaced
        );
        // A file that is not what was written is left as it is.
        assert_eq!(
            docs.restore(&at, b"other", before).await.unwrap(),
            Replacement::Changed
        );
        assert_eq!(tokio::fs::read(&file).await.unwrap(), b"written");
        assert_eq!(
            docs.restore(&at, b"written", before).await.unwrap(),
            Replacement::Replaced
        );
        assert_eq!(tokio::fs::read(&file).await.unwrap(), before);
        // Nothing is left beside it.
        let mut entries = tokio::fs::read_dir(file.parent().unwrap()).await.unwrap();
        let mut names = Vec::new();
        while let Some(entry) = entries.next_entry().await.unwrap() {
            names.push(entry.file_name());
        }
        assert_eq!(names, ["a.md"]);

        assert_eq!(
            docs.remove_read(&at, b"other").await.unwrap(),
            Removal::Changed
        );
        assert_eq!(tokio::fs::read(&file).await.unwrap(), before);
        assert_eq!(
            docs.remove_read(&at, before).await.unwrap(),
            Removal::Removed
        );
        assert!(docs.current(Side::After, &at).await.unwrap().is_none());
        // Gone is told apart from changed, and is no error.
        assert_eq!(docs.remove_read(&at, before).await.unwrap(), Removal::Gone);
        assert_eq!(
            docs.restore(&at, b"written", before).await.unwrap(),
            Replacement::Changed
        );
    }

    #[tokio::test]
    async fn the_markdown_files_under_the_books_are_read_and_nothing_else() {
        let root = tempfile::tempdir().unwrap();
        let dir = root.path().join("docs/specs/sub");
        tokio::fs::create_dir_all(&dir).await.unwrap();
        tokio::fs::write(dir.join("a.md"), format!("---\nid: {ID}\n---\n"))
            .await
            .unwrap();
        tokio::fs::write(dir.join("b.txt"), "no").await.unwrap();
        tokio::fs::write(root.path().join("outside.md"), "---\n---\n")
            .await
            .unwrap();
        let docs = MarkdownDocs::new(root.path().to_path_buf());
        let dirs = [
            BookRange::of_dir(BookDir::new("docs/specs").unwrap()),
            BookRange::of_dir(BookDir::new("docs/missing").unwrap()),
        ];
        let readings = docs.read_docs(Side::After, &dirs).await.unwrap();
        assert_eq!(readings.len(), 1);
        let DocReading::Parsed { origin, .. } = &readings[0] else {
            panic!("parsed");
        };
        assert_eq!(origin.as_str(), "docs/specs/sub/a.md");
    }

    /// The range of a book `spec` over `docs/specs` with these settings.
    fn ranges_of(include: Option<&[&str]>, exclude: &[&str]) -> Vec<BookRange> {
        use std::collections::BTreeSet;
        use whyshelf_core::domain::shelf::schema::{
            BookSchemaDecl, KeyPatternDecl, Schema, ScopeDecl,
        };
        let decl = BookSchemaDecl {
            name: "spec".to_owned(),
            dir: "docs/specs".to_owned(),
            scope: ScopeDecl {
                include: include.map(|l| l.iter().map(|s| (*s).to_owned()).collect()),
                exclude: (!exclude.is_empty())
                    .then(|| exclude.iter().map(|s| (*s).to_owned()).collect()),
                without_front_matter: None,
            },
            key: KeyPatternDecl::slug(),
            description: None,
            fields: Vec::new(),
            links: Vec::new(),
        };
        Schema::validate(vec![decl], &BTreeSet::new())
            .unwrap()
            .ranges()
    }

    #[tokio::test]
    async fn only_the_files_the_range_takes_are_listed_and_read() {
        let root = tempfile::tempdir().unwrap();
        let base = root.path().join("docs/specs");
        for (file, text) in [
            ("a.md", format!("---\nid: {ID}\n---\n")),
            ("b.md", "no front matter\n".to_owned()),
            ("archive/c.md", "---\n".to_owned()),
            ("archive/deep/d.md", "plain\n".to_owned()),
            ("keep/e.md", format!("---\nid: {ID}\n---\n")),
        ] {
            let path = base.join(file);
            tokio::fs::create_dir_all(path.parent().unwrap())
                .await
                .unwrap();
            tokio::fs::write(path, text).await.unwrap();
        }
        // A file that is not UTF-8, out of the range: never read, so never invalid.
        tokio::fs::write(base.join("archive/bin.md"), [0xff, 0xfe, 0x00])
            .await
            .unwrap();
        let docs = MarkdownDocs::new(root.path().to_path_buf());
        let listed = |ranges: Vec<BookRange>| {
            let docs = docs.clone();
            async move {
                let mut names: Vec<String> = docs
                    .read_docs(Side::After, &ranges)
                    .await
                    .unwrap()
                    .into_iter()
                    .map(|r| match r {
                        DocReading::Parsed { origin, .. }
                        | DocReading::Unparsed { origin, .. }
                        | DocReading::Unheld { origin, .. } => origin.to_string(),
                    })
                    .collect();
                names.sort();
                names
            }
        };
        // No setting: every .md file, as before.
        assert_eq!(listed(ranges_of(None, &[])).await.len(), 6);
        // An exclude leaves a directory out, at any depth below it.
        assert_eq!(
            listed(ranges_of(None, &["archive/**"])).await,
            ["docs/specs/a.md", "docs/specs/b.md", "docs/specs/keep/e.md"]
        );
        // An include narrows to what it names.
        assert_eq!(
            listed(ranges_of(Some(&["keep/*.md"]), &[])).await,
            ["docs/specs/keep/e.md"]
        );
        // `forms` (`fmt`) lists the same files.
        let forms = docs
            .forms(Side::After, &ranges_of(None, &["archive/**"]))
            .await
            .unwrap();
        assert_eq!(forms.len(), 3);
    }

    #[tokio::test]
    async fn a_name_that_is_no_path_fails_in_the_range_and_not_outside_it() {
        let root = tempfile::tempdir().unwrap();
        let base = root.path().join("docs/specs");
        tokio::fs::create_dir_all(base.join("archive"))
            .await
            .unwrap();
        tokio::fs::write(base.join("a.md"), format!("---\nid: {ID}\n---\n"))
            .await
            .unwrap();
        // A backslash: a name, on Unix, that no relative path of the project is.
        tokio::fs::write(base.join("archive/a\\b.md"), "x\n")
            .await
            .unwrap();
        let docs = MarkdownDocs::new(root.path().to_path_buf());
        let out = docs
            .read_docs(Side::After, &ranges_of(None, &["archive/**"]))
            .await
            .unwrap();
        assert_eq!(out.len(), 1);
        let out = docs
            .read_docs(Side::After, &ranges_of(None, &["archive/*.md"]))
            .await
            .unwrap();
        assert_eq!(out.len(), 1, "a file the range does not take is not named");
        assert!(matches!(
            docs.read_docs(Side::After, &ranges_of(None, &[])).await,
            Err(DocsError::Name { .. })
        ));
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn a_directory_the_range_leaves_out_is_not_opened() {
        use std::os::unix::fs::PermissionsExt;
        let root = tempfile::tempdir().unwrap();
        let base = root.path().join("docs/specs");
        let locked = base.join("archive/locked");
        tokio::fs::create_dir_all(&locked).await.unwrap();
        tokio::fs::write(base.join("a.md"), format!("---\nid: {ID}\n---\n"))
            .await
            .unwrap();
        tokio::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o000))
            .await
            .unwrap();
        // Root, or a file system that ignores modes, can read it anyway.
        let readable = tokio::fs::read_dir(&locked).await.is_ok();
        let docs = MarkdownDocs::new(root.path().to_path_buf());
        for exclude in ["archive/**", "**", "*/locked/**", "**/locked/**"] {
            let got = docs
                .read_docs(Side::After, &ranges_of(Some(&["*.md"]), &[exclude]))
                .await;
            if exclude == "**" {
                assert!(got.unwrap().is_empty());
            } else {
                assert_eq!(got.unwrap().len(), 1, "{exclude}");
            }
        }
        if !readable {
            // In the range, a directory that cannot be read fails, as it did.
            assert!(matches!(
                docs.read_docs(Side::After, &ranges_of(None, &[])).await,
                Err(DocsError::Io { .. })
            ));
        }
        tokio::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o755))
            .await
            .unwrap();
    }

    #[test]
    fn what_is_no_front_matter_is_stated_once_and_its_edges_are_exact() {
        let none =
            |text: &str| matches!(front_matter_start(&normalize(text)), FrontMatterStart::None);
        let opens = |text: &str| {
            matches!(
                front_matter_start(&normalize(text)),
                FrontMatterStart::Opens { .. }
            )
        };
        let bad_opening = |text: &str| {
            matches!(
                front_matter_start(&normalize(text)),
                FrontMatterStart::BadOpening
            )
        };
        // An empty file, blank lines and white space only: none.
        assert!(none(""));
        assert!(none("\n\n"));
        assert!(none("  \t\n \n"));
        // Plain text, a heading, and a `---` that is a thematic break mid-file: none.
        assert!(none("Just text.\n"));
        assert!(none("# Title\n\ntext\n\n---\n\nmore\n"));
        assert!(none("# Title\n---\nid: x\n---\n"));
        // A first line that does not begin with `---`: none.
        assert!(none("-- -\n"));
        assert!(none("--\nid: x\n--\n"));
        assert!(none("+++\ntitle = \"x\"\n+++\n"));
        // A first line that begins with `---` but is not the line `---`: a mistyped
        // delimiter, never none.
        assert!(bad_opening("----\nid: x\n----\n"));
        assert!(bad_opening("--- x\n"));
        assert!(bad_opening("---x\n"));
        // The line `---`, however the line ends: a front matter opens (CRLF, a lone
        // CR, spaces and tabs at the end of the line, and with nothing after it).
        assert!(opens("---\nid: x\n---\n"));
        assert!(opens("---\r\nid: x\r\n---\r\n"));
        assert!(opens("---\rid: x\r---\r"));
        assert!(opens("---  \t\nid: x\n---\n"));
        assert!(opens("---"));
        // A byte order mark or blank lines before the line `---`: the file tried to
        // have a front matter, so it is not "none".
        assert!(bad_opening("\u{feff}---\nid: x\n---\n"));
        assert!(bad_opening("\n---\nid: x\n---\n"));
        assert!(bad_opening("\n\n  \n---\nid: x\n---\n"));
        assert!(bad_opening("\u{feff}\n---\nid: x\n---\n"));
        assert!(bad_opening("\u{feff}----\n"));
        // A byte order mark before anything else is not a front matter either.
        assert!(none("\u{feff}Just text.\n"));
        assert!(none("\u{feff}"));
    }

    #[test]
    fn a_file_is_read_as_none_misplaced_unclosed_or_broken_by_the_same_rule() {
        let why = |text: &str| match read(origin(), text) {
            DocReading::Unparsed { why, .. } => Some(why),
            DocReading::Parsed { .. } => None,
            DocReading::Unheld { values, .. } => {
                panic!("no case holds an unheld value: {values:?}")
            }
        };
        assert_eq!(why(""), Some(RawProblem::NoFrontMatter));
        assert_eq!(why("\n"), Some(RawProblem::NoFrontMatter));
        assert_eq!(why("text\n"), Some(RawProblem::NoFrontMatter));
        assert_eq!(why("# T\n\n---\n\nbody\n"), Some(RawProblem::NoFrontMatter));
        assert_eq!(
            why("\u{feff}---\nid: x\n---\n"),
            Some(RawProblem::BadOpening)
        );
        assert_eq!(why("\n---\nid: x\n---\n"), Some(RawProblem::BadOpening));
        assert_eq!(why("----\nid: x\n----\n"), Some(RawProblem::BadOpening));
        // Opened and never closed, and not YAML: broken, never "none".
        assert_eq!(why("---\nid: x\n"), Some(RawProblem::Unclosed));
        assert_eq!(why("---"), Some(RawProblem::Unclosed));
        assert!(matches!(
            why("---\njust: [\n---\n"),
            Some(RawProblem::Syntax { .. })
        ));
        assert_eq!(why("---\n- a\n---\n"), Some(RawProblem::NotAMapping));
        // A CRLF file with a front matter is read.
        assert_eq!(why(&format!("---\r\nid: {ID}\r\n---\r\nbody\r\n")), None);
    }

    /// What every invalid file of a book `spec` over `docs` is told as, with this
    /// `without_front_matter`.
    fn told(without_front_matter: Option<&str>, text: &str) -> String {
        use std::collections::BTreeSet;
        use whyshelf_core::domain::shelf::artifact::ArtifactReadings;
        use whyshelf_core::domain::shelf::build::Shelf;
        use whyshelf_core::domain::shelf::schema::{
            BookSchemaDecl, KeyPatternDecl, Schema, ScopeDecl,
        };
        let schema = Schema::validate(
            vec![BookSchemaDecl {
                name: "spec".to_owned(),
                dir: "docs".to_owned(),
                scope: ScopeDecl {
                    include: None,
                    exclude: None,
                    without_front_matter: without_front_matter.map(str::to_owned),
                },
                key: KeyPatternDecl::slug(),
                description: None,
                fields: Vec::new(),
                links: Vec::new(),
            }],
            &BTreeSet::new(),
        )
        .unwrap();
        let shelf = Shelf::build(
            &schema,
            vec![read(origin(), text)],
            &ArtifactReadings::new(),
        );
        shelf
            .invalid_files()
            .iter()
            .flat_map(|doc| doc.problems().iter().map(ToString::to_string))
            .collect::<Vec<_>>()
            .join("; ")
    }

    #[test]
    fn with_nothing_declared_every_file_is_told_as_on_main() {
        // The message of origin/main for a file that does not open with `---`.
        const MAIN: &str = "the file could not be read: the file has no front matter: it must start with a line `---`";
        for text in [
            "",
            "plain\n",
            "\u{feff}---\nid: x\n---\n",
            "\n---\nid: x\n---\n",
            "----\nid: x\n----\n",
            "--- x\n",
            "---x\n",
        ] {
            assert_eq!(told(None, text), MAIN, "{text:?}");
            assert_eq!(told(Some("invalid"), text), MAIN, "{text:?}");
        }
        assert_eq!(
            told(None, "---\nid: x\n"),
            "the file could not be read: the front matter is not closed: add a line `---` after the last entry"
        );
        // Under `skip`, a mistyped opening is what keeps the file from being skipped,
        // and is told so.
        assert_eq!(
            told(Some("skip"), "----\nid: x\n----\n"),
            "the file could not be read: the front matter must open with a line that is exactly `---` at the start of the file"
        );
        assert_eq!(told(Some("skip"), "plain\n"), "");
    }

    #[test]
    fn a_skipping_book_skips_none_and_only_none_by_the_readers_own_answer() {
        use std::collections::BTreeSet;
        use whyshelf_core::domain::shelf::build::Shelf;
        use whyshelf_core::domain::shelf::schema::{
            BookSchemaDecl, KeyPatternDecl, Schema, ScopeDecl,
        };
        let schema = Schema::validate(
            vec![BookSchemaDecl {
                name: "spec".to_owned(),
                dir: "docs".to_owned(),
                scope: ScopeDecl {
                    include: None,
                    exclude: None,
                    without_front_matter: Some("skip".to_owned()),
                },
                key: KeyPatternDecl::slug(),
                description: None,
                fields: Vec::new(),
                links: Vec::new(),
            }],
            &BTreeSet::new(),
        )
        .unwrap();
        let skipped = |text: &str| Shelf::skip_of(&schema, &read(origin(), text)).is_some();
        assert!(skipped(""));
        assert!(skipped("Just text.\n"));
        assert!(skipped("# T\n\n---\n\nmore\n"));
        // Broken in any way: not skipped.
        assert!(!skipped("---\nid: x\n"));
        assert!(!skipped("\u{feff}---\nid: x\n---\n"));
        assert!(!skipped("\n---\nid: x\n---\n"));
        assert!(!skipped("----\nid: x\n----\n"));
        assert!(!skipped("---\njust: [\n---\n"));
        assert!(!skipped(&format!("---\nid: {ID}\n---\n")));
    }

    const ID_B: &str = "019a6f3e-0000-7000-8000-000000000002";
    const ID_C: &str = "019a6f3e-0000-7000-8000-000000000003";

    /// Why a file that could not be read could not, and the identity it shows.
    fn unread(text: &str) -> (RawProblem, ReadId) {
        let DocReading::Unparsed { why, id, .. } = read(origin(), text) else {
            panic!("unparsed: {text:?}");
        };
        (why, id)
    }

    fn identity(id: &str) -> ReadId {
        ReadId::Read(id.parse().unwrap())
    }

    /// The identity of a file one of whose sides shows each of `ids`.
    fn one_of(ids: &[&str]) -> ReadId {
        ReadId::of_sides(ids.iter().map(|id| id.parse().unwrap()))
    }

    /// A file both of whose sides are a whole document, as Git leaves an add/add.
    fn both_sides(ours: &str, theirs: &str) -> String {
        format!(
            "---\n<<<<<<< ours\nid: {ours}\nkey: S-1\n---\nOurs.\n=======\nid: {theirs}\nkey: S-2\n---\nTheirs.\n>>>>>>> theirs\n"
        )
    }

    #[test]
    fn a_file_that_holds_both_sides_of_two_documents_is_a_conflict_of_one_of_the_two() {
        let (why, id) = unread(&both_sides(ID, ID_B));
        assert_eq!(why, RawProblem::ConflictMarkers);
        assert_eq!(id, one_of(&[ID, ID_B]));
        assert!(matches!(id, ReadId::OneOf(_)), "{id:?}");
    }

    #[test]
    fn a_conflict_inside_the_front_matter_of_one_document_keeps_its_identity() {
        let text = format!(
            "---\nid: {ID}\n<<<<<<< ours\ntitle: x\n=======\ntitle: y\n>>>>>>> theirs\n---\nBody.\n"
        );
        assert_eq!(unread(&text), (RawProblem::ConflictMarkers, identity(ID)));
    }

    #[test]
    fn two_sides_that_show_the_same_identity_are_a_conflict_of_that_identity() {
        assert_eq!(
            unread(&both_sides(ID, ID)),
            (RawProblem::ConflictMarkers, identity(ID))
        );
    }

    #[test]
    fn a_file_that_starts_with_a_marker_reads_the_front_matter_each_side_opens() {
        let text = format!(
            "<<<<<<< ours\n---\nid: {ID}\n---\n=======\n---\nid: {ID_B}\n---\n>>>>>>> theirs\n"
        );
        assert_eq!(
            unread(&text),
            (RawProblem::ConflictMarkers, one_of(&[ID, ID_B]))
        );
    }

    #[test]
    fn a_byte_order_mark_before_the_marker_that_starts_the_text_is_set_aside() {
        for before in ["\u{feff}", "\u{feff}\n", "\n"] {
            let text = format!(
                "{before}<<<<<<< ours\n---\nid: {ID}\nkey: S-1\n---\n=======\n---\nid: {ID}\nkey: S-2\n---\n>>>>>>> theirs\n"
            );
            assert_eq!(
                unread(&text),
                (RawProblem::ConflictMarkers, identity(ID)),
                "{before:?}"
            );
        }
    }

    #[test]
    fn a_side_that_is_empty_leaves_the_identity_of_the_other() {
        let text = format!("<<<<<<< ours\n---\nid: {ID}\nkey: F-3\n---\n=======\n>>>>>>> theirs\n");
        assert_eq!(unread(&text), (RawProblem::ConflictMarkers, identity(ID)));
    }

    #[test]
    fn a_side_whose_id_cannot_be_read_leaves_the_file_with_no_one_identity() {
        let text = both_sides(ID, ID_B).replace(&format!("id: {ID_B}"), "id: not-an-id");
        assert_eq!(
            unread(&text),
            (RawProblem::ConflictMarkers, ReadId::Unreadable)
        );
    }

    #[test]
    fn an_id_in_a_conflict_of_the_body_is_not_the_identity() {
        let text = format!(
            "---\nid: {ID}\n<<<<<<< ours\ntitle: x\n=======\ntitle: y\n>>>>>>> theirs\n---\nBody.\n<<<<<<< ours\nid: {ID_C}\n=======\nid: {ID_C}\n>>>>>>> theirs\n"
        );
        assert_eq!(unread(&text), (RawProblem::ConflictMarkers, identity(ID)));
    }

    /// A file with the common ancestor of a conflict (diff3, zdiff3) in it.
    fn with_ancestor(ours: &str, theirs: &str) -> String {
        format!(
            "---\n<<<<<<< ours\nid: {ours}\n---\nOurs.\n||||||| base\nid: {ID_C}\n---\nBase.\n=======\nid: {theirs}\n---\nTheirs.\n>>>>>>> theirs\n"
        )
    }

    #[test]
    fn a_common_ancestor_between_two_different_sides_is_no_side() {
        assert_eq!(
            unread(&with_ancestor(ID, ID_B)),
            (RawProblem::ConflictMarkers, one_of(&[ID, ID_B]))
        );
    }

    #[test]
    fn the_identity_of_a_common_ancestor_is_not_counted() {
        assert_eq!(
            unread(&with_ancestor(ID, ID)),
            (RawProblem::ConflictMarkers, identity(ID))
        );
    }

    #[test]
    fn a_front_matter_that_is_not_yaml_with_a_conflict_only_in_its_body_is_a_syntax_error() {
        let text = format!(
            "---\nid: {ID}\njust: [\n---\nBody.\n<<<<<<< ours\nx\n=======\ny\n>>>>>>> theirs\n"
        );
        let (why, id) = unread(&text);
        assert!(matches!(why, RawProblem::Syntax { .. }), "{why:?}");
        assert_eq!(id, identity(ID));
    }

    #[test]
    fn a_conflict_that_never_ends_is_no_conflict() {
        let text = format!("---\n<<<<<<< ours\nid: {ID}\nkey: x\n=======\nkey: y\n");
        assert_eq!(unread(&text), (RawProblem::Unclosed, identity(ID)));
    }

    #[test]
    fn a_text_that_starts_with_a_marker_and_has_no_front_matter_on_either_side_has_none() {
        assert_eq!(
            unread("<<<<<<< ours\nplain\n=======\nother\n>>>>>>> theirs\n"),
            (RawProblem::NoFrontMatter, ReadId::Unreadable)
        );
    }

    #[test]
    fn a_line_with_more_or_other_than_seven_marker_characters_is_no_marker() {
        for line in ["<<<<<<<<", "<<<<<<<x", "=======x", "======", "||||||||"] {
            assert_eq!(marker(line), None, "{line}");
        }
        for (line, expected) in [
            ("<<<<<<<", Marker::Start),
            ("<<<<<<< ours", Marker::Start),
            ("|||||||", Marker::Base),
            ("=======", Marker::Middle),
            (">>>>>>> theirs", Marker::End),
        ] {
            assert_eq!(marker(line), Some(expected), "{line}");
        }
        let text =
            format!("<<<<<<<<\n---\nid: {ID}\n---\n=======\n---\nid: {ID_B}\n---\n>>>>>>>>\n");
        assert_eq!(
            unread(&text),
            (RawProblem::NoFrontMatter, ReadId::Unreadable)
        );
    }

    #[test]
    fn a_side_that_starts_with_a_mistyped_opening_is_a_conflict_of_no_one_identity() {
        for opening in ["----", "--- x"] {
            let text = format!(
                "<<<<<<< ours\n{opening}\nid: {ID}\n{opening}\n=======\nplain\n>>>>>>> theirs\n"
            );
            assert_eq!(
                unread(&text),
                (RawProblem::ConflictMarkers, ReadId::Unreadable),
                "{text:?}"
            );
        }
    }

    #[test]
    fn a_thematic_break_after_a_marker_does_not_open_a_front_matter() {
        let text = "<<<<<<< ours\n# Readme\ntext\n---\nmore\n=======\n# Readme\ntext\n---\nmore\n>>>>>>> theirs\n";
        assert_eq!(
            unread(text),
            (RawProblem::NoFrontMatter, ReadId::Unreadable)
        );
    }

    #[test]
    fn a_side_of_a_later_conflict_that_begins_with_a_thematic_break_opens_no_front_matter() {
        let text = format!(
            "<<<<<<< ours\n# Readme\n=======\n# Read me\n>>>>>>> theirs\ntext\n<<<<<<< ours\n---\nid: {ID}\n---\n=======\nmore\n>>>>>>> theirs\n"
        );
        assert_eq!(
            unread(&text),
            (RawProblem::NoFrontMatter, ReadId::Unreadable)
        );
    }

    #[test]
    fn a_front_matter_that_is_read_stays_read_whatever_its_body_holds() {
        let text = format!(
            "---\nid: {ID}\nkey: S-1\n---\nBody.\n<<<<<<< ours\nx\n=======\ny\n>>>>>>> theirs\n"
        );
        let DocReading::Parsed { entries, body, .. } = read(origin(), &text) else {
            panic!("parsed");
        };
        assert_eq!(entries["key"], RawValue::Text("S-1".to_owned()));
        assert!(body.contains("<<<<<<< ours"), "{body}");
    }

    #[test]
    fn a_front_matter_that_is_not_closed_and_has_no_marker_shows_the_id_of_any_line() {
        let text = format!("---\nkey: x\nbody: y\nid: {ID}\n");
        assert_eq!(unread(&text), (RawProblem::Unclosed, identity(ID)));
    }

    #[test]
    fn a_mistyped_opening_is_said_so_whatever_markers_follow() {
        for text in [
            format!("----\nid: {ID}\n<<<<<<< ours\nx\n=======\ny\n>>>>>>> theirs\n----\n"),
            format!(
                "\n---\n<<<<<<< ours\nid: {ID}\n---\n=======\nid: {ID_B}\n---\n>>>>>>> theirs\n"
            ),
        ] {
            assert_eq!(
                unread(&text),
                (RawProblem::BadOpening, ReadId::Unreadable),
                "{text:?}"
            );
        }
    }
}
