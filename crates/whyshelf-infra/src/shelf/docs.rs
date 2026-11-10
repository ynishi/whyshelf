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
//! # The one normalization rule
//!
//! Before anything is read, line breaks become LF, the spaces and tabs at the end of
//! each line are removed, and the text ends in exactly one LF. There is no Unicode
//! normalization. The body is hashed by the domain as it is after this rule.

use std::collections::{BTreeMap, BTreeSet};
use std::io;
use std::path::{Path, PathBuf};

use serde_norway::Value;
use whyshelf_core::app::shelf::DocSource;
use whyshelf_core::domain::error::ValueError;
use whyshelf_core::domain::id::DocId;
use whyshelf_core::domain::location::RelPath;
use whyshelf_core::domain::shelf::build::{DocReading, RawProblem, RawValue, ReadId};
use whyshelf_core::domain::shelf::schema::BookDir;

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
}

/// The Markdown files of a project.
#[derive(Debug, Clone)]
pub struct MarkdownDocs {
    root: PathBuf,
}

impl MarkdownDocs {
    /// The documents of the project at `root`.
    pub fn new(root: PathBuf) -> Self {
        Self { root }
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

/// Normalize a text by the rule in the module documentation.
pub fn normalize(text: &str) -> String {
    let unified = text.replace("\r\n", "\n").replace('\r', "\n");
    let mut out = String::with_capacity(unified.len() + 1);
    for line in unified.split('\n') {
        out.push_str(line.trim_end_matches([' ', '\t']));
        out.push('\n');
    }
    let kept = out.trim_end_matches('\n').len();
    out.truncate(kept);
    if !out.is_empty() {
        out.push('\n');
    }
    out
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
        return unparsed(RawProblem::NotAMapping);
    };
    let Some((yaml, body)) = split_closing(rest) else {
        return unparsed(RawProblem::Syntax { line: None });
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
    let value: Value = serde_norway::from_str(yaml).map_err(|error| {
        let message = error.to_string();
        match duplicate_key(&message) {
            Some(key) => RawProblem::DuplicateKey { key },
            None => RawProblem::Syntax {
                // The parser counts from the first line of the YAML, which is the
                // second line of the file.
                line: error
                    .location()
                    .and_then(|at| u32::try_from(at.line()).ok())
                    .map(|line| line + 1),
            },
        }
    })?;
    let Value::Mapping(map) = value else {
        return Err(RawProblem::NotAMapping);
    };
    let mut out = BTreeMap::new();
    for (key, value) in map {
        let Value::String(key) = key else {
            return Err(RawProblem::NotAMapping);
        };
        let raw =
            raw_value(value).ok_or_else(|| RawProblem::UnsupportedValue { key: key.clone() })?;
        out.insert(key, raw);
    }
    Ok(out)
}

/// The key a duplicate-key message of the YAML parser names.
fn duplicate_key(message: &str) -> Option<String> {
    let rest = message.strip_prefix("duplicate entry with key \"")?;
    let end = rest.find('"')?;
    Some(rest[..end].to_owned())
}

/// A value of a front matter, if it is a kind a front matter holds.
fn raw_value(value: Value) -> Option<RawValue> {
    match value {
        Value::String(text) => Some(RawValue::Text(text)),
        Value::Bool(b) => Some(RawValue::Bool(b)),
        Value::Number(n) => n.as_i64().map(RawValue::Integer),
        Value::Sequence(items) => items
            .into_iter()
            .map(|item| match item {
                Value::String(text) => Some(text),
                _ => None,
            })
            .collect::<Option<Vec<_>>>()
            .map(RawValue::List),
        Value::Null | Value::Mapping(_) | Value::Tagged(_) => None,
    }
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
                key: "nested".to_owned()
            }
        );
    }

    #[test]
    fn a_file_without_a_front_matter_or_its_end_is_unparsed() {
        assert!(matches!(
            read(origin(), "Just text.\n"),
            DocReading::Unparsed {
                why: RawProblem::NotAMapping,
                ..
            }
        ));
        assert!(matches!(
            read(origin(), &format!("---\nid: {ID}\n")),
            DocReading::Unparsed {
                why: RawProblem::Syntax { .. },
                id: ReadId::Read(_),
                ..
            }
        ));
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
