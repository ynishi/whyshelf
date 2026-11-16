//! The file `new --from` reads: JSON lines, one object per line, each the request of
//! one new document of a run.
//!
//! ```text
//!   {"book": "note", "as": "a", "entries": {"title": "First"}, "body": "Text."}
//!   {"book": "note", "key": "N-9", "entries": {"title": "Second", "base": [{"as": "a"}, "N-1"]}}
//! ```
//!
//! A line is a [`NewLineIn`]: the `book`, the `key` (the book's next number when left
//! out), the `as` name the other lines of the file link to it by (never written to its
//! file), its `entries` and its `body`. An entry is a text, a truth value, a whole
//! number, or a list of items; an item of a link to documents is a key, or
//! `{"as": "<name>"}`, the document of the line of that name. The use case reads each
//! value by the type its book declares ([`whyshelf_core::app::shelf::EntryInput`]),
//! and the domain judges every line before anything is written.
//!
//! A line that is not an object of this shape is a problem of its own with the number
//! of the line in the file (blank lines are skipped and counted); every such line is
//! told in one answer, and nothing is written. The number is the one the use case
//! gives every problem of the row.
//!
//! An entry written twice in one object is kept twice, so the use case says it is
//! given twice (as it does for `new` with the entry twice on the command line); the
//! reader of the line drops nothing.

use std::borrow::Cow;
use std::fmt;

use schemars::{JsonSchema, Schema, SchemaGenerator, json_schema};
use serde::Deserialize;
use serde::de::{Deserializer, MapAccess, Visitor};
use tokio::io::AsyncReadExt;
use whyshelf_core::app::shelf::{EntryInput, ItemInput, NewDocRequest};
use whyshelf_core::domain::shelf::schema::BookName;

/// One line of the file `new --from` reads: a new document of the run.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct NewLineIn {
    /// The book of the document.
    pub book: String,
    /// The key. Left out, it is the next number of the book (a book of slugs needs
    /// one), passed over for the keys the file asks for and the files that are there.
    pub key: Option<String>,
    /// The name the other lines of the file link to this document by. It is never
    /// written to the file of the document.
    #[serde(rename = "as")]
    pub name: Option<String>,
    /// The entries of the front matter, by name (`id` and `key` are whyshelf's). Left
    /// out, there are none.
    #[serde(default)]
    pub entries: EntriesIn,
    /// The body.
    pub body: Option<String>,
}

/// The entries of a line: an object, kept as the list of its pairs so that a name
/// written twice is not dropped.
#[derive(Debug, Default)]
pub struct EntriesIn(pub Vec<(String, EntryIn)>);

impl<'de> Deserialize<'de> for EntriesIn {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct Pairs;
        impl<'de> Visitor<'de> for Pairs {
            type Value = EntriesIn;

            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("an object of entries")
            }

            fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<EntriesIn, A::Error> {
                let mut pairs = Vec::new();
                while let Some(pair) = map.next_entry::<String, EntryIn>()? {
                    pairs.push(pair);
                }
                Ok(EntriesIn(pairs))
            }
        }
        deserializer.deserialize_map(Pairs)
    }
}

impl JsonSchema for EntriesIn {
    fn schema_name() -> Cow<'static, str> {
        Cow::Borrowed("EntriesIn")
    }

    fn json_schema(generator: &mut SchemaGenerator) -> Schema {
        json_schema!({
            "description": "The entries of the front matter, by name.",
            "type": "object",
            "additionalProperties": generator.subschema_for::<EntryIn>()
        })
    }
}

/// The value of one entry of a line.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(untagged)]
pub enum EntryIn {
    /// A text; for a list field or a link, `a,b` is a list.
    Text(String),
    /// A truth value.
    Bool(bool),
    /// A whole number.
    Integer(i64),
    /// A list: the items of a list field or a link.
    List(Vec<ItemIn>),
}

/// One item of a list.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(untagged)]
pub enum ItemIn {
    /// A text: a key in a link to documents.
    Key(String),
    /// The document of the line of this name (`as`), in a link to documents only.
    Named(NamedIn),
}

/// `{"as": "<name>"}`: the document of the line of that name.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct NamedIn {
    /// The name.
    #[serde(rename = "as")]
    pub name: String,
}

/// Why the file of `--from` gives no run, though it was read: one or more messages for
/// the human, and one problem for each line that is none, with the number of its line.
/// A request that is wrong (exit 2).
#[derive(Debug)]
pub struct LinesRefused {
    /// What is wrong, as a person reads it.
    pub messages: Vec<String>,
    /// One for each line that cannot be read, `row <n>: <why>`.
    pub problems: Vec<String>,
}

/// The file of `--from` could not be read at all (it is not there, may not be read,
/// is not UTF-8, or the standard input failed): a run that failed (exit 3), not a
/// request that is wrong, as a file `init` or another command cannot read is.
#[derive(Debug)]
pub struct Unreadable {
    /// What was being read, as the person gave it: `--from <path>`.
    from: String,
    /// What was wrong.
    source: std::io::Error,
}

impl fmt::Display for Unreadable {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: cannot read it", self.from)
    }
}

impl std::error::Error for Unreadable {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(&self.source)
    }
}

/// Why the file of `--from` gives no run.
#[derive(Debug)]
pub enum FromRefused {
    /// The file could not be read: a failed run.
    Unreadable(Unreadable),
    /// The file was read and is no run: a wrong request.
    Lines(LinesRefused),
}

/// The requests of the file `path` (`-` is the standard input), one per line that is
/// not blank, or why the file gives none.
pub async fn requests_from(path: &str) -> Result<Vec<NewDocRequest>, FromRefused> {
    let unreadable = |source: std::io::Error| {
        FromRefused::Unreadable(Unreadable {
            from: format!("--from {path}"),
            source,
        })
    };
    let text = if path == "-" {
        let mut text = String::new();
        tokio::io::stdin()
            .read_to_string(&mut text)
            .await
            .map_err(unreadable)?;
        text
    } else {
        tokio::fs::read_to_string(path).await.map_err(unreadable)?
    };
    requests_of(&text).map_err(|problems| {
        FromRefused::Lines(LinesRefused {
            messages: vec![format!(
                "--from {path}: {} line{} cannot be read; nothing was written",
                problems.len(),
                if problems.len() == 1 { "" } else { "s" }
            )],
            problems,
        })
    })
}

/// The requests of the lines of `text`: a request for each line that is not blank,
/// numbered by the line (blank lines counted from 1), or every line that cannot be
/// read, with why.
pub fn requests_of(text: &str) -> Result<Vec<NewDocRequest>, Vec<String>> {
    let mut requests = Vec::new();
    let mut problems = Vec::new();
    for (at, line) in text.lines().enumerate() {
        let row = at + 1;
        if line.trim().is_empty() {
            continue;
        }
        let read = serde_json::from_str::<NewLineIn>(line)
            .map_err(|error| error.to_string())
            .and_then(|line| request_of(row, line));
        match read {
            Ok(request) => requests.push(request),
            Err(why) => problems.push(format!("row {row}: {why}")),
        }
    }
    if problems.is_empty() {
        Ok(requests)
    } else {
        Err(problems)
    }
}

/// The request of one line, numbered `row`.
fn request_of(row: usize, line: NewLineIn) -> Result<NewDocRequest, String> {
    let book = BookName::new(&line.book).map_err(|e| format!("book `{}`: {e}", line.book))?;
    Ok(NewDocRequest {
        row,
        book,
        key: line.key,
        name: line.name,
        entries: line
            .entries
            .0
            .into_iter()
            .map(|(name, entry)| (name, input_of(entry)))
            .collect(),
        items: Vec::new(),
        body: line.body.unwrap_or_default(),
    })
}

/// The value of an entry as the use case takes it.
fn input_of(entry: EntryIn) -> EntryInput {
    match entry {
        EntryIn::Text(text) => EntryInput::Text(text),
        EntryIn::Bool(b) => EntryInput::Bool(b),
        EntryIn::Integer(n) => EntryInput::Integer(n),
        EntryIn::List(items) => EntryInput::Items(
            items
                .into_iter()
                .map(|item| match item {
                    ItemIn::Key(key) => ItemInput::Text(key),
                    ItemIn::Named(NamedIn { name }) => ItemInput::Named(name),
                })
                .collect(),
        ),
    }
}
