//! The templates `whyshelf init` writes: a declaration of books and documents
//! beside it, as data.
//!
//! whyshelf knows no kind of document (the core declares none), so what a new
//! project starts with is text kept here, in the formats of the adapters that read
//! it back: the declaration in the TOML of [`books`](super::books), each document
//! in the Markdown of [`docs`] with no `id` (one is given when it is
//! made). The templates are one closed set, [`Template`], each a whole set of
//! files with no conditionals; [`Template::request`] turns one into the
//! [`InitRequest`] the use case [`init`](whyshelf_core::app::shelf::ShelfApp::init)
//! takes, which reads the declaration with the reader of the project's file,
//! checks it, and makes each document as `new` makes one.
//!
//! | template | declaration | documents |
//! |---|---|---|
//! | [`Template::Minimal`] | [`MINIMAL`]: one book, `note` (keys `N-<n>`, a required `title`), with comments that show how to add a field, a link and a link to artifacts with a preset | none |
//! | [`Template::Sample`] | [`SAMPLE`]: four books, `finding`, `requirement`, `decision` and `verification`, linked by `grounds` (informs, `cover = "warn"`), `serves` and `checks` (depends, `cover = true`, a verification checks `at-least-one` decision), a verification's `method` and its `tests`, files by the default preset `file` | `init/sample/*.md`: one of each book, in one chain `F-1 -> REQ-1 -> DEC-1 -> V-1`, about a small command that must not lose a user's file; `V-1` is a `review` and names no file |
//! | [`Template::QaFirst`] | [`QA_FIRST`]: the four books of [`SAMPLE`] with two rules loosened, each commented, for a flow that writes the checks before the decisions: `serves` has `cover = "warn"`, and `checks` is `count = "many"` | `init/qa-first/*.md`: `F-1 -> REQ-1` and `V-1`, a `review` that checks no decision yet, about the same command; no decision |
//! | [`Template::Experiment`] | [`EXPERIMENT`]: the four books of [`SAMPLE`], each change commented, for checks that are measurements registered before they are run: a decision's `code` (the preset `code`), a verification's `method` of `plan` or `result` beside `test` and `review`, its `sheet` and `log` (files, by the preset `file`, neither required), and `plans` (a result to its plan, `depends`, optional); `checks` stays `at-least-one`, so a result names its decision too | `init/experiment/*.md`: `F-1 -> REQ-1 -> DEC-1 -> V-1`, about a save that is too slow; `V-1` is a `plan` whose `sheet` names `measurements/save-time.csv`, the one other file, written beside them; no result, since the run has not happened |
//!
//! A template's other files ([`InitFile`]) are what its documents name as
//! artifacts, so a project starts with no name that has nothing behind it.
//!
//! A test of this module makes a project of each in an empty directory, through
//! the real adapters, and holds it clean: `check` finds nothing wrong, no name
//! with nothing behind it, and no gap
//! but the one a template means (under [`QA_FIRST`], REQ-1 reached by no
//! `decision.serves`, a warning), every document is in the one form, and the
//! links reach as the template draws them.
//!
//! The declaration of [`Template::Sample`]:
//!
#![doc = concat!("```toml\n", include_str!("init/sample.toml"), "```")]

use whyshelf_core::app::shelf::{DeclarationText, InitFile, InitRequest, NewDocRequest};
use whyshelf_core::domain::error::ValueError;
use whyshelf_core::domain::location::RelPath;
use whyshelf_core::domain::nonempty::NonEmptyVec;
use whyshelf_core::domain::shelf::build::{DocReading, RawProblem, RawValue, UnheldValue};
use whyshelf_core::domain::shelf::schema::{BookName, NameProblem};

use super::docs;

/// The entry of a document's front matter that holds its key, as the documents'
/// adapter writes it.
const KEY_ENTRY: &str = "key";

/// The declaration of [`Template::Minimal`]: one book of notes, commented.
pub const MINIMAL: &str = include_str!("init/minimal.toml");

/// The one book [`MINIMAL`] declares, for a command line to name what to make first.
const MINIMAL_BOOK: &str = "note";

/// The declaration of [`Template::Sample`]: four books in a chain of reasons.
pub const SAMPLE: &str = include_str!("init/sample.toml");

/// The declaration of [`Template::QaFirst`]: the books of [`SAMPLE`], with a
/// verification that may check no decision yet and a requirement no decision
/// serves listed, not failed, until both rules are tightened at release.
pub const QA_FIRST: &str = include_str!("init/qa-first.toml");

/// The declaration of [`Template::Experiment`]: the books of [`SAMPLE`], with a
/// decision that names its code and a verification that is either half of a
/// measurement registered before it is run, each choice commented.
pub const EXPERIMENT: &str = include_str!("init/experiment.toml");

/// A document of a template: the book, the template's name, and its text.
type DocTemplate = (&'static str, &'static str, &'static str);

/// Another file of a template, neither a document nor the declaration: its path
/// under the root, and its text.
type FileTemplate = (&'static str, &'static str);

/// The documents of [`Template::Sample`], in the order they are made (each links
/// to the one before it).
const SAMPLE_DOCS: [DocTemplate; 4] = [
    ("finding", "F-1.md", include_str!("init/sample/F-1.md")),
    (
        "requirement",
        "REQ-1.md",
        include_str!("init/sample/REQ-1.md"),
    ),
    ("decision", "DEC-1.md", include_str!("init/sample/DEC-1.md")),
    ("verification", "V-1.md", include_str!("init/sample/V-1.md")),
];

/// The documents of [`Template::QaFirst`], in the order they are made: a finding,
/// the requirement it grounds, and a verification written before any decision.
const QA_FIRST_DOCS: [DocTemplate; 3] = [
    ("finding", "F-1.md", include_str!("init/qa-first/F-1.md")),
    (
        "requirement",
        "REQ-1.md",
        include_str!("init/qa-first/REQ-1.md"),
    ),
    (
        "verification",
        "V-1.md",
        include_str!("init/qa-first/V-1.md"),
    ),
];

/// The documents of [`Template::Experiment`], in the order they are made: the
/// chain of [`SAMPLE_DOCS`], ending in a plan, approved before its run, of which
/// there is no result yet.
const EXPERIMENT_DOCS: [DocTemplate; 4] = [
    ("finding", "F-1.md", include_str!("init/experiment/F-1.md")),
    (
        "requirement",
        "REQ-1.md",
        include_str!("init/experiment/REQ-1.md"),
    ),
    (
        "decision",
        "DEC-1.md",
        include_str!("init/experiment/DEC-1.md"),
    ),
    (
        "verification",
        "V-1.md",
        include_str!("init/experiment/V-1.md"),
    ),
];

/// The other files of [`Template::Experiment`]: the table of what is expected,
/// which the plan's `sheet` names, so it is there from the start.
const EXPERIMENT_FILES: [FileTemplate; 1] = [(
    "measurements/save-time.csv",
    include_str!("init/experiment/measurements/save-time.csv"),
)];

/// A template of `whyshelf init`: one of a closed set, each a declaration and the
/// documents beside it, kept as data with no conditionals.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Template {
    /// [`MINIMAL`], and no document: what `init` writes when no template is named.
    #[default]
    Minimal,
    /// [`SAMPLE`], and one pending document of each book, linked in one chain.
    Sample,
    /// [`QA_FIRST`], and a finding, a requirement and a verification, with no
    /// decision yet.
    QaFirst,
    /// [`EXPERIMENT`], the chain of [`Template::Sample`] ending in a plan of a
    /// measurement, and the table of what it expects.
    Experiment,
}

impl Template {
    /// Every template, in the order a list of them is shown.
    pub const ALL: [Self; 4] = [Self::Minimal, Self::Sample, Self::QaFirst, Self::Experiment];

    /// The name a person gives it: `minimal`, `sample`, `qa-first`, `experiment`.
    pub const fn name(self) -> &'static str {
        match self {
            Self::Minimal => "minimal",
            Self::Sample => "sample",
            Self::QaFirst => "qa-first",
            Self::Experiment => "experiment",
        }
    }

    /// What it is, in one line, for a list of the templates.
    pub const fn about(self) -> &'static str {
        match self {
            Self::Minimal => "one book of notes, commented to show how to extend it; no document",
            Self::Sample => {
                "four books, finding -> requirement -> decision -> verification, and one pending document of each, in one chain"
            }
            Self::QaFirst => {
                "the four books of sample, loosened for checks written before decisions, and a finding, a requirement and a verification"
            }
            Self::Experiment => {
                "the four books of sample, for measurements planned before they are run, and one chain ending in a plan with its table"
            }
        }
    }

    /// The template of this name, if there is one.
    pub fn named(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|t| t.name() == name)
    }

    /// Its declaration, as it is written.
    const fn declaration(self) -> &'static str {
        match self {
            Self::Minimal => MINIMAL,
            Self::Sample => SAMPLE,
            Self::QaFirst => QA_FIRST,
            Self::Experiment => EXPERIMENT,
        }
    }

    /// The book a command line names to make a first document of, for a template
    /// that makes none.
    pub const fn first_book(self) -> Option<&'static str> {
        match self {
            Self::Minimal => Some(MINIMAL_BOOK),
            Self::Sample | Self::QaFirst | Self::Experiment => None,
        }
    }

    /// Its documents, in the order they are made.
    const fn docs(self) -> &'static [DocTemplate] {
        match self {
            Self::Minimal => &[],
            Self::Sample => &SAMPLE_DOCS,
            Self::QaFirst => &QA_FIRST_DOCS,
            Self::Experiment => &EXPERIMENT_DOCS,
        }
    }

    /// Its other files, neither documents nor the declaration, in the order they
    /// are written.
    const fn files(self) -> &'static [FileTemplate] {
        match self {
            Self::Minimal | Self::Sample | Self::QaFirst => &[],
            Self::Experiment => &EXPERIMENT_FILES,
        }
    }

    /// What `whyshelf init` makes of it: its declaration, its other files, and its
    /// documents.
    pub fn request(self) -> Result<InitRequest, TemplateError> {
        Ok(InitRequest {
            declaration: DeclarationText::new(self.declaration()),
            files: self
                .files()
                .iter()
                .map(|&(template, text)| {
                    Ok(InitFile {
                        path: template
                            .parse()
                            .map_err(|source| TemplateError::Name { template, source })?,
                        bytes: text.as_bytes().to_vec(),
                    })
                })
                .collect::<Result<_, _>>()?,
            docs: self
                .docs()
                .iter()
                .map(|(book, template, text)| doc_request(book, template, text))
                .collect::<Result<_, _>>()?,
        })
    }
}

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
    /// Its front matter holds values of kinds a front matter does not hold.
    #[error(
        "the template {template} holds values a front matter does not hold: {}",
        UnheldValue::told_all(values)
    )]
    Unheld {
        /// The template.
        template: &'static str,
        /// Every such value.
        values: NonEmptyVec<UnheldValue>,
    },
    /// It writes no key as a text.
    #[error("the template {template} writes no key")]
    NoKey {
        /// The template.
        template: &'static str,
    },
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
        DocReading::Unheld { values, .. } => {
            return Err(TemplateError::Unheld { template, values });
        }
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
