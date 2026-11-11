//! The templates `whyshelf init` writes: a declaration of books and documents
//! beside it, as data.
//!
//! whyshelf knows no kind of document (the core declares none), so what a new
//! project starts with is text kept here, in the formats of the adapters that read
//! it back: the declaration in the TOML of [`books`](super::books), each document
//! in the Markdown of [`docs`] with no `id` (one is given when it is
//! made). [`minimal`] and [`sample`] turn them into the [`InitRequest`] the use case
//! [`init`](whyshelf_core::app::shelf::ShelfApp::init) takes, which reads the
//! declaration with the reader of the project's file, checks it, and makes each
//! document as `new` makes one.
//!
//! | template | what it is |
//! |---|---|
//! | [`MINIMAL`] | one book, `note` (keys `N-<n>`, a required `title`), with comments that show how to add a field, a link and a link to artifacts with a preset; no document |
//! | [`SAMPLE`] | four books, `finding`, `requirement`, `decision` and `verification`, linked by `grounds` (informs, `cover = "warn"`), `serves` and `checks` (depends, `cover = true`), a verification's `method` and its `tests`, files by the default preset `file` |
//! | `init/sample/*.md` | one document of each book of [`SAMPLE`], in one chain `F-1 -> REQ-1 -> DEC-1 -> V-1`, about a small command that must not lose a user's file; `V-1` is a `review` and names no file |
//!
//! A test of this module makes a project of each in an empty directory, through
//! the real adapters, and holds it clean: `check` finds nothing wrong and no gap,
//! every document is in the one form, and the chain reaches from `V-1` to `F-1`.
//!
#![doc = concat!("```toml\n", include_str!("init/sample.toml"), "```")]

use whyshelf_core::app::shelf::{DeclarationText, InitRequest, NewDocRequest};
use whyshelf_core::domain::error::ValueError;
use whyshelf_core::domain::location::RelPath;
use whyshelf_core::domain::shelf::build::{DocReading, RawProblem, RawValue};
use whyshelf_core::domain::shelf::schema::{BookName, NameProblem};

use super::docs;

/// The entry of a document's front matter that holds its key, as the documents'
/// adapter writes it.
const KEY_ENTRY: &str = "key";

/// The declaration of `whyshelf init`: one book of notes, commented.
pub const MINIMAL: &str = include_str!("init/minimal.toml");

/// The one book [`MINIMAL`] declares, for a command line to name what to make first.
pub const MINIMAL_BOOK: &str = "note";

/// The declaration of `whyshelf init --sample`: four books in a chain of reasons.
pub const SAMPLE: &str = include_str!("init/sample.toml");

/// The documents of the sample, in the order they are made (each links to the one
/// before it): the book, the template's name, and its text.
const SAMPLE_DOCS: [(&str, &str, &str); 4] = [
    ("finding", "F-1.md", include_str!("init/sample/F-1.md")),
    (
        "requirement",
        "REQ-1.md",
        include_str!("init/sample/REQ-1.md"),
    ),
    ("decision", "DEC-1.md", include_str!("init/sample/DEC-1.md")),
    ("verification", "V-1.md", include_str!("init/sample/V-1.md")),
];

/// Why a template of a document is not a request for one: a mistake of this
/// program, which the test of this module keeps from being shipped. The command
/// line answers it as a failed run (exit 3), before anything is written. (A
/// declaration template that is no TOML declaration is a failed run too, an error
/// of the port that reads it; one that reads and is not sound is a request the
/// use case refuses, exit 2, as every declaration that cannot be used is.)
#[derive(Debug, thiserror::Error)]
pub enum TemplateError {
    /// The name of its book is refused.
    #[error("the template {template} names a book with a name that is refused")]
    Book {
        /// The template.
        template: &'static str,
        /// Why.
        #[source]
        source: NameProblem,
    },
    /// The name of the template is no relative path.
    #[error("the template {template} has a name that is no relative path")]
    Name {
        /// The template.
        template: &'static str,
        /// Why.
        #[source]
        source: ValueError,
    },
    /// Its front matter cannot be read.
    #[error("the template {template} cannot be read")]
    Unread {
        /// The template.
        template: &'static str,
        /// Why.
        #[source]
        why: RawProblem,
    },
    /// It writes no key as a text.
    #[error("the template {template} writes no key")]
    NoKey {
        /// The template.
        template: &'static str,
    },
}

/// What `whyshelf init` makes: [`MINIMAL`], and no document.
pub fn minimal() -> InitRequest {
    InitRequest {
        declaration: DeclarationText::new(MINIMAL),
        docs: Vec::new(),
    }
}

/// What `whyshelf init --sample` makes: [`SAMPLE`], and its four documents.
pub fn sample() -> Result<InitRequest, TemplateError> {
    Ok(InitRequest {
        declaration: DeclarationText::new(SAMPLE),
        docs: SAMPLE_DOCS
            .iter()
            .map(|(book, template, text)| doc_request(book, template, text))
            .collect::<Result<_, _>>()?,
    })
}

/// A template of a document as the request `new` takes: its key, its entries as
/// text (a list as one item each, commas kept), its body.
fn doc_request(
    book: &str,
    template: &'static str,
    text: &str,
) -> Result<NewDocRequest, TemplateError> {
    let book = BookName::new(book).map_err(|source| TemplateError::Book { template, source })?;
    let origin: RelPath = template
        .parse()
        .map_err(|source| TemplateError::Name { template, source })?;
    let (mut entries, body) = match docs::read(origin, text) {
        DocReading::Parsed { entries, body, .. } => (entries, body),
        DocReading::Unparsed { why, .. } => return Err(TemplateError::Unread { template, why }),
    };
    let Some(RawValue::Text(key)) = entries.remove(KEY_ENTRY) else {
        return Err(TemplateError::NoKey { template });
    };
    let mut texts = Vec::new();
    let mut items = Vec::new();
    for (entry, value) in entries {
        match value {
            RawValue::Text(text) => texts.push((entry, text)),
            RawValue::Integer(n) => texts.push((entry, n.to_string())),
            RawValue::Bool(b) => texts.push((entry, b.to_string())),
            RawValue::List(list) => items.extend(list.into_iter().map(|i| (entry.clone(), i))),
        }
    }
    Ok(NewDocRequest {
        book,
        key: Some(key),
        entries: texts,
        items,
        body,
    })
}

#[cfg(test)]
mod tests;
