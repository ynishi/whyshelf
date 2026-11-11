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
//! cannot be read is a [`DocReading::Unparsed`], with its `id:` line if that line
//! can still be read, so that a broken file is an invalid document with an identity
//! and not a hole.
//!
//! # Writing a new document
//!
//! [`MarkdownDocs`] is also the [`DocWrite`] of a new document: [`render`] makes the
//! text from the entries (a flat front matter, every text in double quotes, `id` and
//! `key` first, the whole text after the normalization rule), and the file is
//! created only where nothing is (`create_new`: a directory or a link there, even one
//! to nothing, is in the way too), with its directories; a write that fails after
//! the file was made removes it, and only it. What [`render`]
//! writes, [`read`] reads back into the same entries; a test holds the two together.
//!
//! # Writing a document in the form
//!
//! The text [`render`] makes is the one form of a document, so what `new` writes is
//! in it. [`form`] sets a file beside it ([`DocSource::forms`]): [`render`] of the
//! entries and the body it reads, the whole text after the normalization rule
//! ([`normalize`]). So the form also normalizes a body: its line endings become
//! `\n`, white space at the end of its lines and blank lines at its end go, and it
//! ends with one newline. [`read`] normalizes the whole text first, so this changes
//! nothing it reads. A file whose text is that already is in the form; a file whose
//! front matter cannot be read has no text in it; a file whose front matter holds a
//! comment is told so ([`comment_line`]), because the form has no place for one;
//! every other file is given its text in the form, with what that text reads as, for
//! the use case to hold against the file's own reading, unless the user cannot
//! write it or it has more than one link ([`Form::ReadOnly`], [`Form::Linked`]).
//!
//! [`MarkdownDocs`] is also the [`DocReplace`] that writes that text over the file:
//! when the file still holds the bytes that were read, just before, and whole (a
//! new file beside it, renamed over it), so the file is either as it was or in the
//! form. It is the one write here that replaces a file; [`DocWrite::create`] never
//! does.
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

use std::collections::{BTreeMap, BTreeSet};
use std::io;
use std::path::{Path, PathBuf};

use std::fmt;

use serde::Deserialize;
use serde::de::{self, Deserializer, IgnoredAny, MapAccess, SeqAccess, Visitor};
use serde_norway::Value;
use tokio::io::AsyncWriteExt;
use whyshelf_core::app::shelf::{
    Creation, DocReplace, DocSource, DocWrite, FileForm, Form, Replacement,
};
use whyshelf_core::domain::error::ValueError;
use whyshelf_core::domain::id::DocId;
use whyshelf_core::domain::location::RelPath;
use whyshelf_core::domain::shelf::artifact::ContentAddress;
use whyshelf_core::domain::shelf::build::{
    DocReading, RawProblem, RawValue, ReadId, UnheldItem, UnheldKind,
};
use whyshelf_core::domain::shelf::doc::normalize;
use whyshelf_core::domain::shelf::schema::BookRange;

use super::create;
use super::git::{Git, GitError};

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
}

/// The Markdown files of a project, and where their content is kept: the blob each
/// is (or would be) in, in the project's Git repository when it is in one.
#[derive(Debug, Clone)]
pub struct MarkdownDocs {
    root: PathBuf,
    git: Git,
}

impl MarkdownDocs {
    /// The documents of the project at `root`.
    pub fn new(root: PathBuf) -> Self {
        Self {
            git: Git::new(root.clone()),
            root,
        }
    }

    /// The content a document's file had at `address`, or none when it cannot be
    /// read back; an error when Git could not be run or took too long.
    pub async fn read_back(&self, address: &ContentAddress) -> Result<Option<Vec<u8>>, DocsError> {
        Ok(self.git.read_blob(address).await?)
    }
}

impl DocSource for MarkdownDocs {
    type Error = DocsError;

    async fn read_docs(&self, ranges: &[BookRange]) -> Result<Vec<DocReading>, DocsError> {
        Ok(self
            .files_of(ranges)
            .await?
            .into_iter()
            .map(|(origin, bytes)| match std::str::from_utf8(&bytes) {
                Ok(text) => read(origin, text),
                Err(_) => not_text(origin),
            })
            .collect())
    }

    async fn forms(&self, ranges: &[BookRange]) -> Result<Vec<FileForm>, DocsError> {
        let mut forms = Vec::new();
        for (origin, bytes) in self.files_of(ranges).await? {
            let file = self.root.join(origin.as_str());
            let mut found = form(origin, bytes);
            if matches!(found.form, Form::Other { .. })
                && let Some(kept) = kept_from_replacing(&file).await?
            {
                found.form = kept;
            }
            forms.push(found);
        }
        Ok(forms)
    }

    async fn current(&self, file: &RelPath) -> Result<Option<Vec<u8>>, DocsError> {
        let full = self.root.join(file.as_str());
        match tokio::fs::read(&full).await {
            Ok(bytes) => Ok(Some(bytes)),
            Err(error)
                if matches!(
                    error.kind(),
                    io::ErrorKind::NotFound
                        | io::ErrorKind::NotADirectory
                        | io::ErrorKind::IsADirectory
                ) =>
            {
                Ok(None)
            }
            Err(source) => Err(DocsError::Io { path: full, source }),
        }
    }

    async fn addresses(
        &self,
        files: &BTreeSet<RelPath>,
    ) -> Result<BTreeMap<RelPath, ContentAddress>, DocsError> {
        let mut there = Vec::new();
        for file in files {
            if self.current(file).await?.is_some() {
                there.push(file);
            }
        }
        let Some(names) = self.git.blob_names(&there).await? else {
            return Ok(BTreeMap::new());
        };
        Ok(there.into_iter().cloned().zip(names).collect())
    }
}

impl MarkdownDocs {
    /// Every `.md` file the ranges take, with its bytes, in the order of the books
    /// and, under each, a stable order. Whether a file is in a range is asked of
    /// the range and of nothing else here ([`range_files`]); a file out of range is
    /// not read.
    async fn files_of(&self, ranges: &[BookRange]) -> Result<Vec<(RelPath, Vec<u8>)>, DocsError> {
        let mut files = Vec::new();
        for range in ranges {
            for file in range_files(&self.root.join(range.dir().as_rel().as_str()), range).await? {
                let origin = relative(&self.root, &file)?;
                let bytes = tokio::fs::read(&file)
                    .await
                    .map_err(|source| DocsError::Io {
                        path: file.clone(),
                        source,
                    })?;
                files.push((origin, bytes));
            }
        }
        Ok(files)
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

/// Why a file that is not in the form must still not be written over, if it must
/// not, from the file's metadata:
///
/// - [`Form::Linked`]: it has more than one link (a hard link). The rename that
///   replaces it would give this path a new file and leave the other links on the
///   old one, so the two would part; and the new file would be the writer's, with
///   none of the old one's owner or extended attributes. (Only on Unix, where the
///   number of links is told.)
/// - [`Form::ReadOnly`]: the user cannot write it. It is opened for writing, which
///   changes nothing in it (no byte, no time); a refusal (no permission, or a file
///   system mounted read-only) is the answer.
async fn kept_from_replacing(file: &Path) -> Result<Option<Form>, DocsError> {
    let failed = |source| DocsError::Io {
        path: file.to_path_buf(),
        source,
    };
    let meta = tokio::fs::metadata(file).await.map_err(failed)?;
    if links(&meta) > 1 {
        return Ok(Some(Form::Linked));
    }
    match tokio::fs::OpenOptions::new().write(true).open(file).await {
        Ok(_) => Ok(None),
        Err(error)
            if matches!(
                error.kind(),
                io::ErrorKind::PermissionDenied | io::ErrorKind::ReadOnlyFilesystem
            ) =>
        {
            Ok(Some(Form::ReadOnly))
        }
        Err(source) => Err(failed(source)),
    }
}

/// How many links the file has.
#[cfg(unix)]
fn links(meta: &std::fs::Metadata) -> u64 {
    use std::os::unix::fs::MetadataExt;
    meta.nlink()
}

/// How many links the file has: not told here, so one.
#[cfg(not(unix))]
fn links(_meta: &std::fs::Metadata) -> u64 {
    1
}

/// Every file under `dir`, the directory of `range`, that the range takes, in a
/// stable order. A directory that is not there has none.
///
/// The range is asked before anything else is done with a name: a directory it
/// takes nothing under (an `exclude` of `<dir>/**`) is not opened, and a file it
/// does not take is not made a path, so a name that is no path, or a directory
/// that cannot be read, out of the range is no error. In the range, both are, as
/// they were. A name is asked of the range as its text; one that is not UTF-8 is
/// asked with its bad bytes replaced, and when the range takes that, it fails
/// where every name that is no path fails ([`relative`]).
async fn range_files(dir: &Path, range: &BookRange) -> Result<Vec<PathBuf>, DocsError> {
    let mut found = Vec::new();
    let mut pending = vec![(dir.to_path_buf(), String::new())];
    while let Some((next, below)) = pending.pop() {
        let mut entries = match tokio::fs::read_dir(&next).await {
            Ok(entries) => entries,
            Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
            Err(source) => return Err(DocsError::Io { path: next, source }),
        };
        let io = |source| DocsError::Io {
            path: next.clone(),
            source,
        };
        while let Some(entry) = entries.next_entry().await.map_err(io)? {
            let kind = entry.file_type().await.map_err(io)?;
            let name = entry.file_name();
            let name = name.to_string_lossy();
            let at = if below.is_empty() {
                name.into_owned()
            } else {
                format!("{below}/{name}")
            };
            // arch-lint: allow(no-sync-io) reason="`kind` is a FileType fetched with `.await` above: a pure accessor, no I/O"
            if kind.is_dir() {
                if !range.leaves_out_dir(&at) {
                    pending.push((entry.path(), at));
                }
            // arch-lint: allow(no-sync-io) reason="`kind` is a FileType fetched with `.await` above: a pure accessor, no I/O"
            } else if kind.is_file() && range.takes_below(&at) {
                found.push(entry.path());
            }
        }
    }
    found.sort();
    Ok(found)
}

/// The path of `file` relative to `root`, with `/` between segments.
fn relative(root: &Path, file: &Path) -> Result<RelPath, DocsError> {
    let name = |source| DocsError::Name {
        path: file.to_path_buf(),
        source,
    };
    let rel = file.strip_prefix(root).map_err(|_| name(None))?;
    let mut segments = Vec::new();
    for part in rel.components() {
        segments.push(part.as_os_str().to_str().ok_or_else(|| name(None))?);
    }
    segments.join("/").parse().map_err(|e| name(Some(e)))
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
///    empty text, a text of blank lines, a lone mark), or that line does not begin
///    with `---`, the text holds *no front matter at all*
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
    /// No front matter at all.
    None,
}

/// How a text, already normalized by [`normalize`], stands to a front matter
/// ([`FrontMatterStart`]).
fn front_matter_start(normalized: &str) -> FrontMatterStart<'_> {
    if let Some(rest) = normalized.strip_prefix("---\n") {
        return FrontMatterStart::Opens { rest };
    }
    let first_with_content = normalized
        .strip_prefix('\u{feff}')
        .unwrap_or(normalized)
        .lines()
        .find(|line| !line.is_empty());
    if first_with_content.is_some_and(|line| line.starts_with("---")) {
        FrontMatterStart::BadOpening
    } else {
        FrontMatterStart::None
    }
}

/// A file's text as a reading.
pub fn read(origin: RelPath, text: &str) -> DocReading {
    let text = normalize(text);
    let unparsed = |why| DocReading::Unparsed {
        origin: origin.clone(),
        id: seen_id(&text).map_or(ReadId::Unreadable, ReadId::Read),
        why,
    };
    let rest = match front_matter_start(&text) {
        FrontMatterStart::Opens { rest } => rest,
        FrontMatterStart::BadOpening => return unparsed(RawProblem::BadOpening),
        FrontMatterStart::None => return unparsed(RawProblem::NoFrontMatter),
    };
    let Some((yaml, body)) = split_closing(rest) else {
        return unparsed(RawProblem::Unclosed);
    };
    match entries(yaml) {
        Ok(entries) => DocReading::Parsed {
            origin,
            entries,
            body: body.to_owned(),
        },
        Err(why) => unparsed(why),
    }
}

/// The YAML up to the closing `---` line, and the body after it.
fn split_closing(rest: &str) -> Option<(&str, &str)> {
    let mut offset = 0;
    for line in rest.split_inclusive('\n') {
        if line.trim_end_matches('\n') == "---" {
            return Some((&rest[..offset], &rest[offset + line.len()..]));
        }
        offset += line.len();
    }
    None
}

/// A file beside the one form ([`render`]): its reading, its bytes, and how it
/// stands to the form (the module page, "Writing a document in the form").
pub fn form(origin: RelPath, bytes: Vec<u8>) -> FileForm {
    let Ok(text) = std::str::from_utf8(&bytes) else {
        return FileForm {
            reading: not_text(origin),
            read: bytes,
            form: Form::Unwritable,
        };
    };
    let reading = read(origin.clone(), text);
    let form = match &reading {
        DocReading::Parsed { entries, body, .. } => {
            let formed = render(entries, body);
            if formed == text {
                Form::InForm
            } else if let Some(line) = comment_in(text) {
                Form::Comment { line }
            } else {
                Form::Other {
                    reads_as: read(origin, &formed),
                    text: formed.into_bytes(),
                }
            }
        }
        DocReading::Unparsed { .. } => Form::Unwritable,
    };
    FileForm {
        reading,
        read: bytes,
        form,
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

/// The entries of a front matter.
fn entries(yaml: &str) -> Result<BTreeMap<String, RawValue>, RawProblem> {
    if yaml.trim().is_empty() {
        return Ok(BTreeMap::new());
    }
    let front: Front = serde_norway::from_str(yaml).map_err(|error| RawProblem::Syntax {
        // The parser counts from the first line of the YAML, which is the second
        // line of the file.
        line: error
            .location()
            .and_then(|at| u32::try_from(at.line()).ok())
            .map(|line| line + 1),
    })?;
    let pairs = match front {
        Front::Mapping(pairs) => pairs,
        Front::Duplicate(key) => return Err(RawProblem::DuplicateKey { key }),
        Front::NotMapping => return Err(RawProblem::NotAMapping),
    };
    let mut out = BTreeMap::new();
    for (key, value) in pairs {
        let Value::String(key) = key else {
            return Err(RawProblem::NotAMapping);
        };
        let raw = raw_value(value).map_err(|kind| RawProblem::UnsupportedValue {
            key: key.clone(),
            kind,
        })?;
        out.insert(key, raw);
    }
    Ok(out)
}

/// The top level of a front matter as the parser streams it.
///
/// A key written twice is found here, in the parser's own stream of keys, and not in
/// a mapping built from it (which keeps one value per key) and not in the wording of
/// an error. Only the top level is looked at: a key written twice inside a nested
/// mapping is not a [`RawProblem::DuplicateKey`], because a nested value is not a
/// kind a front matter holds and is reported as that
/// ([`RawProblem::UnsupportedValue`]), or, when the parser itself refuses the
/// nested mapping, as a [`RawProblem::Syntax`].
enum Front {
    /// A mapping, with every entry in the order written.
    Mapping(Vec<(Value, Value)>),
    /// A mapping with a text key written twice: the first such key.
    Duplicate(String),
    /// A scalar, or a sequence.
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
                let mut pairs: Vec<(Value, Value)> = Vec::new();
                let mut duplicate = None;
                while let Some((key, value)) = map.next_entry::<Value, Value>()? {
                    if duplicate.is_none()
                        && let Value::String(text) = &key
                        && pairs.iter().any(|(seen, _)| *seen == key)
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
        }

        deserializer.deserialize_any(FrontVisitor)
    }
}

/// A value of a front matter, if it is a kind a front matter holds.
fn raw_value(value: Value) -> Result<RawValue, UnheldKind> {
    match value {
        Value::String(text) => Ok(RawValue::Text(text)),
        Value::Bool(b) => Ok(RawValue::Bool(b)),
        Value::Number(n) => match n.as_i64() {
            Some(i) => Ok(RawValue::Integer(i)),
            None if n.is_f64() => Err(UnheldKind::Float),
            None => Err(UnheldKind::TooLarge),
        },
        Value::Sequence(items) => items
            .into_iter()
            .map(|item| match item {
                Value::String(text) => Ok(text),
                Value::Null => Err(UnheldKind::Item(UnheldItem::Null)),
                Value::Bool(_) => Err(UnheldKind::Item(UnheldItem::Bool)),
                Value::Number(_) => Err(UnheldKind::Item(UnheldItem::Number)),
                Value::Mapping(_) => Err(UnheldKind::Item(UnheldItem::Mapping)),
                Value::Sequence(_) => Err(UnheldKind::Item(UnheldItem::List)),
                Value::Tagged(_) => Err(UnheldKind::Item(UnheldItem::Tagged)),
            })
            .collect::<Result<Vec<_>, _>>()
            .map(RawValue::List),
        Value::Null => Err(UnheldKind::Null),
        Value::Mapping(_) => Err(UnheldKind::Mapping),
        Value::Tagged(_) => Err(UnheldKind::Tagged),
    }
}

impl DocWrite for MarkdownDocs {
    type Error = DocsError;

    async fn in_the_way(&self, path: &RelPath) -> Result<Option<RelPath>, DocsError> {
        create::in_the_way(&self.root, path)
            .await
            .map_err(|source| DocsError::Io {
                path: self.root.join(path.as_str()),
                source,
            })
    }

    async fn dir_in_the_way(&self, dir: &RelPath) -> Result<Option<RelPath>, DocsError> {
        create::dir_in_the_way(&self.root, dir)
            .await
            .map_err(|source| DocsError::Io {
                path: self.root.join(dir.as_str()),
                source,
            })
    }

    /// Created `create_new` with its directories; a write that fails removes the
    /// file it made, and only it.
    async fn create(
        &self,
        path: &RelPath,
        entries: &BTreeMap<String, RawValue>,
        body: &str,
    ) -> Result<Creation, DocsError> {
        let file = self.root.join(path.as_str());
        create::create_new(&file, render(entries, body).as_bytes())
            .await
            .map_err(|failed| DocsError::Write {
                step: failed.step,
                source: failed.source,
            })
    }
}

/// The error of a step of a write, told as the step with its path.
fn write_step(step: String) -> impl FnOnce(io::Error) -> DocsError {
    move |source| DocsError::Write { step, source }
}

/// Writes over a document's file after comparing it with the bytes read: the text goes
/// into a new file beside it (a name starting with `.` and ending in `.tmp`, so it
/// is never read as a document), with the file's permissions, and is renamed over
/// it. A new file that cannot be written is removed.
///
/// The comparison and the rename are two steps: an edit saved to the file in the
/// short time between them is overwritten, and a file removed in it is written
/// again. A read-only file and one with more than one link are never given here:
/// [`DocSource::forms`] leaves them alone ([`Form::ReadOnly`], [`Form::Linked`]).
impl DocReplace for MarkdownDocs {
    type Error = DocsError;

    async fn replace_read(
        &self,
        path: &RelPath,
        read: &[u8],
        text: &[u8],
    ) -> Result<Replacement, DocsError> {
        let file = self.root.join(path.as_str());
        match tokio::fs::read(&file).await {
            Ok(now) if now == read => {}
            Ok(_) => return Ok(Replacement::Changed),
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                return Ok(Replacement::Changed);
            }
            Err(source) => {
                return Err(DocsError::Io { path: file, source });
            }
        }
        let permissions = tokio::fs::metadata(&file)
            .await
            .map_err(|source| DocsError::Io {
                path: file.clone(),
                source,
            })?
            .permissions();
        let name = path.as_str().rsplit('/').next().unwrap_or(path.as_str());
        let beside = file.with_file_name(format!(".{name}.{}.tmp", uuid::Uuid::now_v7()));
        let shown = beside.display();
        let written = async {
            let mut out = tokio::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&beside)
                .await
                .map_err(write_step(format!("create the temporary file {shown}")))?;
            out.write_all(text)
                .await
                .map_err(write_step(format!("write the temporary file {shown}")))?;
            out.set_permissions(permissions)
                .await
                .map_err(write_step(format!(
                    "give the temporary file {shown} the permissions of {}",
                    file.display()
                )))?;
            out.sync_all().await.map_err(write_step(format!(
                "write the temporary file {shown} to disk"
            )))?;
            drop(out);
            tokio::fs::rename(&beside, &file)
                .await
                .map_err(write_step(format!(
                    "rename {shown} over {}",
                    file.display()
                )))
        }
        .await;
        if let Err(error) = written {
            // The new file is not the document; whether it can be removed changes
            // nothing of the error told.
            let _ = tokio::fs::remove_file(&beside).await;
            return Err(error);
        }
        Ok(Replacement::Replaced)
    }
}

/// The text of a document: the front matter, then the body.
///
/// The front matter is the flat form [`read`] reads: one `name: value` line per
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

/// The `id:` a text shows, found by looking at its lines only: for a file that could
/// not be read. `None` when no line gives one, and when the lines give two (a merge
/// conflict that left both sides does not say which is the document's).
fn seen_id(text: &str) -> Option<DocId> {
    let mut lines = text.lines();
    if lines.next() != Some("---") {
        return None;
    }
    let mut found = BTreeSet::new();
    for line in lines.take_while(|line| *line != "---") {
        let Some(value) = line.strip_prefix("id:") else {
            continue;
        };
        let value = value.split(" #").next().unwrap_or(value).trim();
        if let Ok(id) = value.trim_matches(['"', '\'']).parse::<DocId>() {
            found.insert(id);
        }
    }
    let mut ids = found.into_iter();
    match (ids.next(), ids.next()) {
        (Some(id), None) => Some(id),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use whyshelf_core::domain::shelf::schema::BookDir;

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
        let DocReading::Unparsed { why, .. } = read(origin(), &text) else {
            panic!("unparsed");
        };
        assert_eq!(
            why,
            RawProblem::UnsupportedValue {
                key: "nested".to_owned(),
                kind: UnheldKind::Mapping,
            }
        );
    }

    #[test]
    fn each_kind_a_front_matter_does_not_hold_is_named() {
        let cases = [
            ("v:", UnheldKind::Null),
            ("v: 1.5", UnheldKind::Float),
            ("v: 1.0", UnheldKind::Float),
            ("v: 1e3", UnheldKind::Float),
            ("v: 18446744073709551615", UnheldKind::TooLarge),
            ("v: {a: 1}", UnheldKind::Mapping),
            ("v: !x a", UnheldKind::Tagged),
            ("v: [a, ~]", UnheldKind::Item(UnheldItem::Null)),
            ("v: [a, true]", UnheldKind::Item(UnheldItem::Bool)),
            ("v: [a, 3]", UnheldKind::Item(UnheldItem::Number)),
            (
                "v: [JSON Lines: one record per line]",
                UnheldKind::Item(UnheldItem::Mapping),
            ),
            ("v: [a, [b]]", UnheldKind::Item(UnheldItem::List)),
            ("v: [a, !x b]", UnheldKind::Item(UnheldItem::Tagged)),
        ];
        for (line, kind) in cases {
            let text = format!("---\nid: {ID}\n{line}\n---\n");
            let DocReading::Unparsed { why, .. } = read(origin(), &text) else {
                panic!("unparsed: {line}");
            };
            assert_eq!(
                why,
                RawProblem::UnsupportedValue {
                    key: "v".to_owned(),
                    kind,
                },
                "{line}"
            );
        }
    }

    #[test]
    fn an_item_read_as_a_mapping_is_told_to_be_quoted() {
        let text = format!("---\nid: {ID}\nrejected: [JSON Lines: one record per line]\n---\n");
        let DocReading::Unparsed { why, .. } = read(origin(), &text) else {
            panic!("unparsed");
        };
        let message = why.to_string();
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
        for yaml in ["- a\n- b", "just: [", "plain"] {
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
        let DocReading::Unparsed { why, .. } = read(origin(), &text) else {
            panic!("unparsed");
        };
        assert!(
            !matches!(why, RawProblem::DuplicateKey { .. }),
            "a nested mapping is not a value a front matter holds: {why:?}"
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
                    let FileForm {
                        reading, form: got, ..
                    } = form(origin(), text.clone().into_bytes());
                    assert!(
                        matches!(reading, DocReading::Parsed { .. }),
                        "parsed: {text:?}"
                    );
                    let Form::Other {
                        text: formed,
                        reads_as,
                    } = got
                    else {
                        panic!("not in the form: {text:?}: {got:?}");
                    };
                    assert_eq!(reads_as, reading, "{text:?}");
                    let formed = String::from_utf8(formed).unwrap();
                    // The whole text is normalized, the body with it.
                    assert_eq!(formed, normalize(&formed), "{text:?}");
                    assert!(!formed.contains('\r'), "{formed:?}");
                    assert!(formed.ends_with(&normalize(body)), "{formed:?}");
                    assert!(formed.starts_with(&format!("---\nid: \"{ID}\"\nkey: \"S-1\"\n")));
                    // And the text in the form is in the form.
                    assert_eq!(
                        form(origin(), formed.clone().into_bytes()).form,
                        Form::InForm,
                        "{formed:?}"
                    );
                }
            }
        }
    }

    #[test]
    fn a_crlf_file_and_a_body_with_trailing_white_space_are_written_normalized() {
        let in_form = format!("---\nid: \"{ID}\"\nkey: \"S-1\"\n---\nBody.\n\nMore.\n");
        for text in [
            in_form.replace('\n', "\r\n"),
            format!("---\nid: \"{ID}\"\nkey: \"S-1\"\n---\nBody.  \n\t\nMore.\t\n\n\n"),
        ] {
            let got = form(origin(), text.clone().into_bytes());
            let Form::Other {
                text: formed,
                reads_as,
            } = got.form
            else {
                panic!("not in the form: {text:?}");
            };
            assert_eq!(reads_as, got.reading, "{text:?}");
            assert_eq!(String::from_utf8(formed.clone()).unwrap(), in_form);
            let before = hash_of(got.reading);
            assert_eq!(hash_of(read(origin(), &in_form)), before, "{text:?}");
            assert_eq!(form(origin(), formed).form, Form::InForm);
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
            assert_eq!(
                form(origin(), text.clone().into_bytes()).form,
                Form::InForm,
                "{body:?}: {text:?}"
            );
        }
    }

    #[test]
    fn a_file_that_cannot_be_read_has_no_text_in_the_form() {
        for bytes in [
            b"no front matter\n".to_vec(),
            format!("---\nid: {ID}\n").into_bytes(),
            format!("---\nid: {ID}\nkey: a\nkey: b\n---\n").into_bytes(),
            vec![
                b'-', b'-', b'-', b'\n', 0xff, b'\n', b'-', b'-', b'-', b'\n',
            ],
        ] {
            let got = form(origin(), bytes.clone());
            assert!(matches!(got.reading, DocReading::Unparsed { .. }));
            assert_eq!(got.form, Form::Unwritable);
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
        assert_eq!(
            form(origin(), text.into_bytes()).form,
            Form::Comment { line: 3 }
        );
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
        assert!(docs.current(&gone).await.unwrap().is_none());

        assert_eq!(
            docs.create(&at, &BTreeMap::new(), "Other.\n")
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
            .forms(&[BookRange::of_dir(BookDir::new("docs").unwrap())])
            .await
            .unwrap();
        let got: Vec<&Form> = forms.iter().map(|f| &f.form).collect();
        assert!(matches!(got[0], Form::Other { .. }), "{got:?}");
        if !writable_anyway {
            assert_eq!(*got[1], Form::ReadOnly);
        }
        assert_eq!(*got[2], Form::Linked);
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
        let error = docs.create(&at, &BTreeMap::new(), "").await.unwrap_err();
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
            docs.create(&at, &entries, "Body.\n").await.unwrap(),
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
            docs.create(&at, &other, "Other.\n").await.unwrap(),
            Creation::AlreadyThere
        );
        assert_eq!(tokio::fs::read_to_string(&file).await.unwrap(), written);

        let dirs = [BookRange::of_dir(BookDir::new("books/design").unwrap())];
        let readings = docs.read_docs(&dirs).await.unwrap();
        let [DocReading::Parsed { entries: back, .. }] = readings.as_slice() else {
            panic!("one parsed file");
        };
        assert_eq!(*back, entries);
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
        let readings = docs.read_docs(&dirs).await.unwrap();
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
            key: KeyPatternDecl::Slug,
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
                let mut names: Vec<String> =
                    docs.read_docs(&ranges)
                        .await
                        .unwrap()
                        .into_iter()
                        .map(|r| match r {
                            DocReading::Parsed { origin, .. }
                            | DocReading::Unparsed { origin, .. } => origin.to_string(),
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
        let forms = docs.forms(&ranges_of(None, &["archive/**"])).await.unwrap();
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
            .read_docs(&ranges_of(None, &["archive/**"]))
            .await
            .unwrap();
        assert_eq!(out.len(), 1);
        let out = docs
            .read_docs(&ranges_of(None, &["archive/*.md"]))
            .await
            .unwrap();
        assert_eq!(out.len(), 1, "a file the range does not take is not named");
        assert!(matches!(
            docs.read_docs(&ranges_of(None, &[])).await,
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
                .read_docs(&ranges_of(Some(&["*.md"]), &[exclude]))
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
                docs.read_docs(&ranges_of(None, &[])).await,
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
                key: KeyPatternDecl::Slug,
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
            .unreadable()
            .iter()
            .chain(shelf.anonymous())
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
                key: KeyPatternDecl::Slug,
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
}
