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
//! `key` first), and the file is created only where none is (`create_new`), with
//! its directories. What [`render`] writes, [`read`] reads back into the same
//! entries; a test holds the two together.
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
use whyshelf_core::app::shelf::{Creation, DocSource, DocWrite};
use whyshelf_core::domain::error::ValueError;
use whyshelf_core::domain::id::DocId;
use whyshelf_core::domain::location::RelPath;
use whyshelf_core::domain::shelf::artifact::ContentAddress;
use whyshelf_core::domain::shelf::build::{
    DocReading, RawProblem, RawValue, ReadId, UnheldItem, UnheldKind,
};
use whyshelf_core::domain::shelf::doc::normalize;
use whyshelf_core::domain::shelf::schema::BookDir;

use super::git::{Git, GitError};

/// Why the documents could not be read at all. A file that is not a document is not
/// an error: it is a reading that says why.
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
    /// read back.
    pub async fn read_back(&self, address: &ContentAddress) -> Option<Vec<u8>> {
        self.git.read_blob(address).await
    }
}

impl DocSource for MarkdownDocs {
    type Error = DocsError;

    async fn read_docs(&self, dirs: &[BookDir]) -> Result<Vec<DocReading>, DocsError> {
        let mut readings = Vec::new();
        for dir in dirs {
            for file in markdown_files(&self.root.join(dir.as_rel().as_str())).await? {
                let origin = relative(&self.root, &file)?;
                let text = tokio::fs::read(&file)
                    .await
                    .map_err(|source| DocsError::Io {
                        path: file.clone(),
                        source,
                    })?;
                readings.push(match String::from_utf8(text) {
                    Ok(text) => read(origin, &text),
                    Err(_) => DocReading::Unparsed {
                        origin,
                        id: ReadId::Unreadable,
                        why: RawProblem::Syntax { line: None },
                    },
                });
            }
        }
        Ok(readings)
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

/// Every `.md` file under `dir`, in a stable order. A directory that is not there
/// has none.
async fn markdown_files(dir: &Path) -> Result<Vec<PathBuf>, DocsError> {
    let mut found = Vec::new();
    let mut pending = vec![dir.to_path_buf()];
    while let Some(next) = pending.pop() {
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
            let path = entry.path();
            // arch-lint: allow(no-sync-io) reason="`kind` is a FileType fetched with `.await` above: a pure accessor, no I/O"
            if kind.is_dir() {
                pending.push(path);
            // arch-lint: allow(no-sync-io) reason="`kind` is a FileType fetched with `.await` above: a pure accessor, no I/O"
            } else if kind.is_file() && path.extension().is_some_and(|e| e == "md") {
                found.push(path);
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

/// A file's text as a reading.
pub fn read(origin: RelPath, text: &str) -> DocReading {
    let text = normalize(text);
    let unparsed = |why| DocReading::Unparsed {
        origin: origin.clone(),
        id: seen_id(&text).map_or(ReadId::Unreadable, ReadId::Read),
        why,
    };
    let Some(rest) = text.strip_prefix("---\n") else {
        return unparsed(RawProblem::NoFrontMatter);
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

    async fn create(
        &self,
        path: &RelPath,
        entries: &BTreeMap<String, RawValue>,
        body: &str,
    ) -> Result<Creation, DocsError> {
        let file = self.root.join(path.as_str());
        let io = |path: &Path| {
            let path = path.to_path_buf();
            move |source| DocsError::Io { path, source }
        };
        if let Some(dir) = file.parent() {
            tokio::fs::create_dir_all(dir).await.map_err(io(dir))?;
        }
        let mut out = match tokio::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&file)
            .await
        {
            Ok(out) => out,
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
                return Ok(Creation::AlreadyThere);
            }
            Err(source) => return Err(DocsError::Io { path: file, source }),
        };
        out.write_all(render(entries, body).as_bytes())
            .await
            .map_err(io(&file))?;
        out.sync_all().await.map_err(io(&file))?;
        Ok(Creation::Created)
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
/// would not read as anything but a text, and quoted otherwise. What this writes
/// reads back into the same entries, and the body into itself after the
/// normalization rule.
pub fn render(entries: &BTreeMap<String, RawValue>, body: &str) -> String {
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
    out.push_str(body);
    if !body.is_empty() && !body.ends_with('\n') {
        out.push('\n');
    }
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

        let dirs = [BookDir::new("books/design").unwrap()];
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
            BookDir::new("docs/specs").unwrap(),
            BookDir::new("docs/missing").unwrap(),
        ];
        let readings = docs.read_docs(&dirs).await.unwrap();
        assert_eq!(readings.len(), 1);
        let DocReading::Parsed { origin, .. } = &readings[0] else {
            panic!("parsed");
        };
        assert_eq!(origin.as_str(), "docs/specs/sub/a.md");
    }
}
