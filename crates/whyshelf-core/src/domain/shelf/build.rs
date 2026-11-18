//! Building a shelf from what an adapter read: the one place a document is made.
//!
//! The file is `build.rs` and the type is [`Shelf`] (a module named like its type
//! trips clippy's `module_inception`).
//!
//! [`Shelf::build`] takes the [`Schema`] and one [`DocReading`] per file, and never
//! fails: a file that cannot be trusted becomes an [`InvalidDoc`] that says why, and
//! the rest of the shelf is built around it. It runs in a fixed order, and each
//! step is the only place its decision is made.
//!
//! ```text
//!   DocReading ──► 0  Unparsed, Unheld?    ──► InvalidDoc (Unreadable, Unheld)
//!      │           1  Schema::membership   ──► InvalidDoc (OutsideBooks)
//!      │           2  the form of the file:  id, key, entries, fields, links
//!      │           3  same id twice, same key twice in a book
//!      │           4  each file judged, once: a valid one has its hash made
//!      │              (DocDraft::new: book, fields, body), any other is an
//!      │              InvalidDoc with every problem found above
//!      │           5  index of keys        (valid and readable-invalid files), with
//!      │                                   the hash of each valid one
//!      │           6  each valid Doc, once: every link the book declares, with its
//!      │                                   kind, resolved through the index; every
//!      │                                   name of a link to artifacts made an
//!      │                                   Artifact from the ArtifactReadings (A1)
//!      └────────►  7  link cycles          (strongly connected components)
//! ```
//!
//! The names a link to artifacts writes are read before the build, by
//! [`Shelf::artifact_asks`] (per space, each with the comparisons of the links that
//! write it), so that the adapter of each space can answer for them first: a name is
//! resolved once, for every link that writes it, and each link takes the hash of its
//! own comparison. A name with nothing there is an [`Artifact`] that is absent, and
//! one the adapter cannot tell about now is one that is unknown, not a link that
//! cannot be followed (rule A1). A text that is no [`ArtifactName`] is a
//! [`DocProblem::ArtifactNameInvalid`], and one its adapter answered is not a name of
//! the space is a [`DocProblem::NotAnArtifactName`]: either makes the document
//! invalid.
//!
//! Two questions about a shelf have one entry each, and everything else asks them:
//! `Shelf::find_key` answers what a key in a book comes to (the build of a link
//! reads the same index before the shelf exists, and the lookup of what a person
//! typed calls it), and [`Shelf::state_of`] answers
//! what an identity comes to now (a valid document, an invalid one, gone, or not
//! knowable because some file could not even be read for its id).
//!
//! A document that is not written yet is asked of a built shelf too
//! ([`Shelf::next_key`], [`Shelf::claim_key`], [`Shelf::check_new`]): the keys it may
//! take are those no file of its book writes, and it is judged by steps 1, 2, 4 and 6
//! above, the same code, on the file it would be. A run of new documents is planned
//! all at once by [`Shelf::plan_new`], which decides their keys, the names they link
//! each other by and their judgement, resolving their links in the shelf's index with
//! the run added to it, and [`Shelf::replan_after_taken`] says what comes of a key
//! whose file was there at the write.

use std::collections::btree_map::Entry;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use thiserror::Error;

use crate::domain::error::ValueError;
use crate::domain::id::DocId;
use crate::domain::location::RelPath;
use crate::domain::nonempty::{Count, NonBlank, NonEmptyVec, OneLine, TwoOrMore};
use crate::domain::words::{Join, Mark, listed};

use super::artifact::{
    Artifact, ArtifactAsks, ArtifactKey, ArtifactName, ArtifactNameProblem, ArtifactReadings, Space,
};
use super::doc::{
    ArtifactLink, Doc, DocDraft, DocHash, DocKey, FieldValue, Link, LinkState, ResolvedLink,
    UnresolvedLink, UnresolvedWhy,
};
use super::graph::{Cycle, CycleSource, cycles_of};
use super::schema::{
    ArtifactLinkKind, BookName, BookSchema, Cardinality, EntryName, FieldDecl, FieldPresence,
    FieldType, ID_ENTRY, KEY_ENTRY, KeyForm, KeyProblem, LinkKind, Membership, Propagation,
    ScalarType, Schema, WithoutFrontMatter,
};

mod new;
mod plan;

pub use new::{
    ClaimKeyProblem, NEXT_KEY_TRIES, NewDoc, NewDocProblem, NewKey, NextKeyProblem, NoFreeKey,
    OutOfRange,
};
pub use plan::{LinkItem, NewPlan, NewRequest, PlanAfterTaken, PlanProblem, PlannedValue, RunName};

/// A value of the front matter, as the adapter's YAML parser gave it: not yet
/// checked against a field's type.
///
/// What a value says is not decided here: an adapter reads `key:`, `null` and `~` as
/// [`Null`](Self::Null) and a `""` as an empty [`Text`](Self::Text), as it found them,
/// and the build reads what any of them comes to for an entry (`Written::of`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RawValue {
    /// No value: `key:` with nothing after it, `null` or `~`. An item of a list that
    /// is none is not this but an [`UnheldItem::Null`].
    Null,
    /// A text.
    Text(String),
    /// A whole number.
    Integer(i64),
    /// A truth value.
    Bool(bool),
    /// A list of texts.
    List(Vec<String>),
}

/// The kind of a [`RawValue`], for a problem to say what was found.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RawKind {
    /// No value.
    Null,
    /// A text.
    Text,
    /// A whole number.
    Integer,
    /// A truth value.
    Bool,
    /// A list of texts.
    List,
}

impl fmt::Display for RawKind {
    /// The kind in words, as a problem says what was found: `no value`, `a text`, `a
    /// whole number`, `a truth value` or `a list`.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Null => "no value",
            Self::Text => "a text",
            Self::Integer => "a whole number",
            Self::Bool => "a truth value",
            Self::List => "a list",
        })
    }
}

impl RawValue {
    /// The kind of the value.
    pub fn kind(&self) -> RawKind {
        match self {
            Self::Null => RawKind::Null,
            Self::Text(_) => RawKind::Text,
            Self::Integer(_) => RawKind::Integer,
            Self::Bool(_) => RawKind::Bool,
            Self::List(_) => RawKind::List,
        }
    }
}

/// Why a file's front matter could not be read as a mapping at all: one reason is
/// enough to read no further, so a file has one. A value of a kind a front matter
/// does not hold is not this: the mapping is read, and each such value is an
/// [`UnheldValue`].
///
/// The adapter's YAML parser finds these, because a mapping built in memory cannot
/// show a key that was written twice.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum RawProblem {
    /// The file has no front matter: no line `---` opens it. A file that has one is
    /// not this, even when the front matter is broken; what counts as none is stated
    /// once, beside the adapter's reader of a front matter.
    #[error("the file has no front matter: it must start with a line `---`")]
    NoFrontMatter,
    /// The first line with anything on it begins with `---`, but the file does not
    /// open with the exact line `---` at its first byte: a byte order mark or blank
    /// lines come before it, or the line is `----`, `---x` or `--- x`. The file
    /// tried to have a front matter, so it is not "no front matter", and a book that
    /// skips files with none does not skip it.
    ///
    /// Only such a book tells it so: for any other the file is told as
    /// [`NoFrontMatter`](Self::NoFrontMatter), as it was before a book could skip
    /// ([`Shelf::told`]).
    #[error(
        "the front matter must open with a line that is exactly `---` at the start of the file"
    )]
    BadOpening,
    /// The front matter is opened by `---` and never closed.
    #[error("the front matter is not closed: add a line `---` after the last entry")]
    Unclosed,
    /// The front matter is not YAML. The line is there when the parser says it.
    #[error("the front matter is not valid YAML")]
    Syntax {
        /// The line of the error, when the parser gave one.
        line: Option<u32>,
    },
    /// A key written twice.
    #[error("the key `{key}` is written twice")]
    DuplicateKey {
        /// The key.
        key: String,
    },
    /// The front matter is YAML, but not a mapping.
    #[error("the front matter is not a mapping")]
    NotAMapping,
}

/// A value of a front matter that is none of no value, a text, a whole number, a
/// truth value and a list of texts, with its key and the kind it was read as.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[error("the value of `{key}` is {kind}, which a front matter does not hold{}", kind.hint())]
pub struct UnheldValue {
    key: String,
    kind: UnheldKind,
}

impl UnheldValue {
    /// The value of `key`, read as `kind`.
    pub fn new(key: String, kind: UnheldKind) -> Self {
        Self { key, kind }
    }

    /// The key.
    pub fn key(&self) -> &str {
        &self.key
    }

    /// What the value was read as.
    pub fn kind(&self) -> UnheldKind {
        self.kind
    }

    /// Every one of `values`, each in its own words, one after the other.
    pub fn told_all(values: &NonEmptyVec<Self>) -> String {
        listed(values.iter(), Mark::Plain, Join::Semicolon)
    }
}

/// What a value of a front matter was read as when it is none of the kinds a front
/// matter holds (no value, a text, a whole number, a truth value, a list of texts).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnheldKind {
    /// A number written with a decimal point or an exponent (`1.5`, `1.0`, `1e3`).
    Float,
    /// A whole number too large for the integers a front matter holds (64-bit,
    /// signed).
    TooLarge,
    /// A whole number too small for the integers a front matter holds (64-bit,
    /// signed).
    TooSmall,
    /// A mapping (`key: {a: 1}`, or an indented block of `a: b`).
    Mapping,
    /// A value with a YAML tag (`!tag`).
    Tagged,
    /// A list with an item that is no text.
    Item(UnheldItem),
}

/// What an item of a list was read as when it is no text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnheldItem {
    /// No value.
    Null,
    /// A truth value.
    Bool,
    /// A number.
    Number,
    /// A mapping: an item written `a: b` without quotes.
    Mapping,
    /// A list in the list.
    List,
    /// A value with a YAML tag.
    Tagged,
}

impl std::fmt::Display for UnheldKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let item = |kind| format!("a list with an item that is {kind}");
        let text = match self {
            Self::Float => "a number written with a decimal point or an exponent".to_owned(),
            Self::TooLarge => "a whole number above the largest one (2^63 - 1)".to_owned(),
            Self::TooSmall => "a whole number below the smallest one (-2^63)".to_owned(),
            Self::Mapping => "a mapping".to_owned(),
            Self::Tagged => "a tagged value".to_owned(),
            Self::Item(UnheldItem::Null) => item("no value"),
            Self::Item(UnheldItem::Bool) => item("a truth value"),
            Self::Item(UnheldItem::Number) => item("a number"),
            Self::Item(UnheldItem::Mapping) => item("a mapping"),
            Self::Item(UnheldItem::List) => item("a list"),
            Self::Item(UnheldItem::Tagged) => item("a tagged value"),
        };
        f.write_str(&text)
    }
}

impl UnheldKind {
    /// How a value of this kind is usually meant to be written, as the end of a
    /// sentence: the usual cause and the way out, or nothing.
    pub fn hint(&self) -> &'static str {
        match self {
            Self::Item(UnheldItem::Mapping) => {
                ": an item written with `: ` is read as a mapping; put it in quotes"
            }
            Self::Item(UnheldItem::Bool | UnheldItem::Number) => {
                ": a list holds texts; put the item in quotes"
            }
            Self::Float => {
                ": write a whole number with digits only, or put the number in quotes as a text"
            }
            Self::TooLarge | Self::TooSmall => ": put the number in quotes as a text",
            Self::Item(UnheldItem::Null) => ": write the item, or take it out",
            Self::Mapping => ": the values are flat; write a text or a list of texts",
            Self::Tagged | Self::Item(UnheldItem::List | UnheldItem::Tagged) => "",
        }
    }
}

/// The identity a file that could not be parsed still shows.
///
/// A merge conflict marker in the body, or a broken mapping, leaves the `id:` line
/// readable more often than not, and then the file is an *invalid document with
/// that identity* and not a hole.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReadId {
    /// An `id:` line was read.
    Read(DocId),
    /// None could be.
    Unreadable,
}

/// A file as an adapter read it, before anything is checked.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DocReading {
    /// The front matter was read as a mapping.
    Parsed {
        /// The file.
        origin: RelPath,
        /// Every entry of the front matter, `id` and `key` included. Which entry is
        /// a field and which a link is decided by [`Shelf::build`] from the schema,
        /// not by the adapter.
        entries: BTreeMap<String, RawValue>,
        /// The body, normalized by the adapter.
        body: String,
    },
    /// The front matter could not be read.
    Unparsed {
        /// The file.
        origin: RelPath,
        /// The identity, if its line could still be read.
        id: ReadId,
        /// What was wrong.
        why: RawProblem,
    },
    /// The front matter was read as a mapping, and holds values of kinds a front
    /// matter does not hold.
    Unheld {
        /// The file.
        origin: RelPath,
        /// The identity, if its line could still be read.
        id: ReadId,
        /// Every such value, in the order written.
        values: NonEmptyVec<UnheldValue>,
    },
}

/// The entries a file of a book is written with: the entries of a document, less those
/// the build reads as not written and the file need not hold.
///
/// It can be made by [`Shelf::form_entries`] only, so that what `new` writes and what
/// `fmt` writes are told by one rule: the writers of a front matter take this type,
/// not a map of raw values.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FormEntries(BTreeMap<String, RawValue>);

impl FormEntries {
    /// Every entry to write, `id` and `key` included, by name.
    pub fn as_map(&self) -> &BTreeMap<String, RawValue> {
        &self.0
    }

    /// The entry written under `name`, if any.
    pub fn get(&self, name: &str) -> Option<&RawValue> {
        self.0.get(name)
    }

    /// Every entry to write, in the order of their names.
    pub fn iter(&self) -> impl Iterator<Item = (&String, &RawValue)> {
        self.0.iter()
    }

    /// How many entries there are.
    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// Whether there is no entry.
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

/// Why a file in the range of a book is not one of its documents.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SkipWhy {
    /// The file has no front matter at all ([`RawProblem::NoFrontMatter`]), and the
    /// book says `without_front_matter = "skip"`.
    NoFrontMatter,
}

impl SkipWhy {
    /// The reason as a word, `no_front_matter`.
    pub fn word(self) -> &'static str {
        match self {
            Self::NoFrontMatter => "no_front_matter",
        }
    }
}

/// A file in the range of a book that is not one of its documents, and why. It is
/// told by `check` and counted nowhere else: it is neither valid nor invalid.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkippedFile {
    origin: RelPath,
    book: BookName,
    why: SkipWhy,
}

impl SkippedFile {
    /// The file.
    pub fn origin(&self) -> &RelPath {
        &self.origin
    }

    /// The book whose range holds it.
    pub fn book(&self) -> &BookName {
        &self.book
    }

    /// Why it is not a document.
    pub fn why(&self) -> SkipWhy {
        self.why
    }
}

/// One thing that makes a document invalid.
///
/// Only things that make the document itself untrustworthy are here. A link that
/// cannot be followed does not: it is a [`LinkState`] of an otherwise valid
/// document.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum DocProblem {
    /// The front matter could not be read.
    #[error("the file could not be read: {0}")]
    Unreadable(#[source] RawProblem),
    /// The front matter was read, and a value in it is of a kind a front matter does
    /// not hold.
    #[error("the file could not be read: {0}")]
    Unheld(#[source] UnheldValue),
    /// The file is under no book's directory.
    #[error("the file is under no book")]
    OutsideBooks,
    /// There is no `id:`.
    #[error("there is no `id:`")]
    IdMissing,
    /// The `id:` is written, and says nothing: no value, a blank text or an empty
    /// list (told at [`FieldPresence::Required`]). It is written, so it is not
    /// [`IdMissing`](Self::IdMissing).
    #[error("`id:` is written empty or blank")]
    IdEmpty,
    /// The `id:` is not a text.
    #[error("`id:` is not a text")]
    IdNotText,
    /// The `id:` is not an identifier.
    #[error("`id:` is not an identifier")]
    IdInvalid(#[source] ValueError),
    /// There is no `key:`.
    #[error("there is no `key:`")]
    KeyMissing,
    /// The `key:` is written, and says nothing, as an `id:` can
    /// ([`IdEmpty`](Self::IdEmpty)).
    #[error("`key:` is written empty or blank")]
    KeyEmpty,
    /// The `key:` is not a text.
    #[error("`key:` is not a text")]
    KeyNotText,
    /// The `key:` does not fit the grammar of the book.
    #[error("`key:` does not fit the book: {0}")]
    KeyInvalid(#[source] KeyProblem),
    /// Another file has the same `id:`: all of them are invalid.
    #[error("the id is also in {} other file(s)", others.len())]
    DuplicateId {
        /// The other files.
        others: NonEmptyVec<RelPath>,
    },
    /// Another file of the book has the same key: all of them are invalid.
    #[error("the key `{key}` is also in {} other file(s)", others.len())]
    DuplicateKey {
        /// The key.
        key: DocKey,
        /// The other files.
        others: NonEmptyVec<RelPath>,
    },
    /// An entry the book declares neither as a field nor as a link.
    #[error("the book `{book}` declares no field or link `{name}`: declare it in the book first")]
    UnknownEntry {
        /// The entry.
        name: String,
        /// The book that declares neither.
        book: BookName,
    },
    /// A required field is missing. The missing entries of a document are told
    /// with these first, in the order the declaration writes the fields, then with
    /// [`DocProblem::LinkCount`]s of no key, for the links into books and then the
    /// links to artifacts, each in the order the declaration writes them.
    #[error("the required field `{field}` is missing")]
    FieldMissing {
        /// The field.
        field: EntryName,
    },
    /// A required field written with a value that says nothing; what counts as a
    /// value is told at [`FieldPresence::Required`].
    #[error("the required field `{field}` is empty or blank")]
    FieldEmpty {
        /// The field.
        field: EntryName,
    },
    /// An item of a list (a list field, a link into a book, a link to artifacts) that
    /// says nothing: a text that is empty or blank. An item has no "not written", so
    /// it is never left out as a field is; a null item is an [`UnheldItem::Null`]
    /// before it gets here.
    #[error("the item {at} of `{entry}` is empty or blank")]
    ItemBlank {
        /// The entry that holds the list.
        entry: EntryName,
        /// Which item, counting from 1.
        at: usize,
    },
    /// A field declared `line` written with more than one line: a line break in it, a
    /// final one included (a `|` block of YAML keeps one; `|-` does not).
    #[error(
        "the field `{field}` is a text of more than one line, not `line` (a `|-` block has no line break at its end)"
    )]
    NotOneLine {
        /// The field.
        field: EntryName,
    },
    /// A field of a kind of value other than its type.
    #[error("the field `{field}` is {found}, not `{}`", expected.word())]
    FieldMismatch {
        /// The field.
        field: EntryName,
        /// Its declared type.
        expected: FieldType,
        /// What was written.
        found: RawKind,
    },
    /// An enum field with a word the book does not list.
    #[error("`{found}` is not a word of the field `{field}`")]
    FieldNotInEnum {
        /// The field.
        field: EntryName,
        /// The word written.
        found: String,
    },
    /// A link that is not a list of texts.
    #[error("the link `{link}` is {found}, not a list of keys")]
    LinkNotList {
        /// The link.
        link: EntryName,
        /// What was written.
        found: RawKind,
    },
    /// A text written in a link to artifacts that is no [`ArtifactName`] (empty, with
    /// a control character, or too long).
    #[error("`{written}` in the link `{link}` is no name of an artifact: {why}")]
    ArtifactNameInvalid {
        /// The link.
        link: EntryName,
        /// What was written.
        written: String,
        /// Why it is no name.
        why: ArtifactNameProblem,
    },
    /// A text written in a link to artifacts that the adapter of its space answered
    /// is not a name there, for the reason it gives.
    #[error("`{written}` in the link `{link}` is not a name in the space `{space}`: {why}")]
    NotAnArtifactName {
        /// The link.
        link: EntryName,
        /// The space of the link.
        space: Space,
        /// What was written.
        written: String,
        /// Why, as the adapter says.
        why: NonBlank,
    },
    /// A link with a number of keys its cardinality does not admit.
    #[error(
        "the link `{link}` has {}, which its count `{}` ({}) does not admit",
        key_count(*found),
        cardinality.word(),
        cardinality.admits_in_words()
    )]
    LinkCount {
        /// The link.
        link: EntryName,
        /// What the book declares.
        cardinality: Cardinality,
        /// How many keys were written.
        found: usize,
    },
}

/// A number of keys, as a problem says it: `1 key`, `2 keys`.
fn key_count(count: usize) -> String {
    if count == 1 {
        "1 key".to_owned()
    } else {
        format!("{count} keys")
    }
}

/// A file that could not be made a valid document, and why.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InvalidDoc {
    origin: RelPath,
    id: Option<DocId>,
    problems: NonEmptyVec<DocProblem>,
}

impl InvalidDoc {
    /// The file.
    pub fn origin(&self) -> &RelPath {
        &self.origin
    }

    /// The identity, when it could be read.
    pub fn id(&self) -> Option<DocId> {
        self.id
    }

    /// Everything that is wrong with it.
    pub fn problems(&self) -> &NonEmptyVec<DocProblem> {
        &self.problems
    }
}

/// What a key in a book comes to: what `Shelf::find_key` answers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum KeyHit {
    /// One valid document has it.
    Valid(DocId),
    /// One document has it, and it is invalid.
    Invalid(DocId),
    /// Several documents have it, valid or not.
    Ambiguous(TwoOrMore<DocId>),
    /// None does.
    Missing,
}

/// What an identity comes to in a shelf now.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DocState<'a> {
    /// A valid document.
    Valid(&'a Doc),
    /// One or more invalid files that carry the identity.
    Invalid(&'a NonEmptyVec<InvalidDoc>),
    /// No file has it, and no file is left whose identity could not be read: it is
    /// not there.
    Gone,
    /// No file is known to have it, but some file's identity could not be read, so
    /// it may be in one of them.
    Unknown {
        /// The files whose identity could not be read.
        unreadable: NonEmptyVec<RelPath>,
    },
}

/// Why an identity is no valid document now: the owned form of a [`DocState`] that
/// is not valid, so that a refusal or a cause can keep it.
///
/// The one type of "not valid": a refusal of a frame, the causes of freshness on the
/// document itself and on an upstream all carry it, and a retirement keeps its
/// [`tag`](Self::tag).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DocFailure {
    /// Invalid, with every problem of every file that carries the identity.
    Invalid(NonEmptyVec<DocProblem>),
    /// Not there.
    Gone,
    /// Not knowable: the files whose identity could not be read.
    Unknown(NonEmptyVec<RelPath>),
}

/// Which of the three a [`DocFailure`] is, without what it carries: what a record
/// keeps of a document it retired.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FailureTag {
    /// [`DocFailure::Invalid`].
    Invalid,
    /// [`DocFailure::Gone`].
    Gone,
    /// [`DocFailure::Unknown`].
    Unknown,
}

impl FailureTag {
    /// What the tag says of `subject`, as a clause: `{subject} is invalid`, `no file
    /// in the books' ranges carries {subject}` (a file a book's range leaves out, or
    /// none at all: the shelf reads no file outside the ranges, so it cannot tell the
    /// two apart), `whether a file carries {subject} cannot be told`.
    ///
    /// The one place the three are worded: the clause of a document that is not
    /// valid, however it is named (`it`, `the document`, a key).
    pub fn said_of(self, subject: &dyn std::fmt::Display) -> String {
        match self {
            Self::Invalid => format!("{subject} is invalid"),
            Self::Gone => format!("no file in the books' ranges carries {subject}"),
            Self::Unknown => format!("whether a file carries {subject} cannot be told"),
        }
    }
}

/// What the tag says of a document that is not named: [`FailureTag::said_of`] `it`.
impl std::fmt::Display for FailureTag {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.said_of(&"it"))
    }
}

/// How much of a [`DocFailure`] a sentence about it tells.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Detail {
    /// All of it: the problems of an invalid document, and the files that could not be
    /// read.
    Full,
    /// Without the problems of an invalid document, for a sentence about a document
    /// that is not the subject of the answer (an upstream): the files that could not
    /// be read are still told, since they are the whole of what is known.
    Short,
}

impl DocFailure {
    /// Which of the three it is.
    pub fn tag(&self) -> FailureTag {
        match self {
            Self::Invalid(_) => FailureTag::Invalid,
            Self::Gone => FailureTag::Gone,
            Self::Unknown(_) => FailureTag::Unknown,
        }
    }

    /// What it says of `subject`: the clause of [`FailureTag::said_of`], and what the
    /// failure carries as `detail` allows.
    ///
    /// | failure | [`Detail::Full`] | [`Detail::Short`] |
    /// |---|---|---|
    /// | [`DocFailure::Invalid`] | `s is invalid: p1; p2` | `s is invalid` |
    /// | [`DocFailure::Gone`] | `no file in the books' ranges carries s` | the same |
    /// | [`DocFailure::Unknown`] | `whether a file carries s cannot be told; unreadable: a; b` | the same |
    ///
    /// The one sentence of why a document is not valid: a refusal and an answer of
    /// A refused approval and the notices of `stale` and `status` say it through this,
    /// so they say it alike.
    pub fn said_of(&self, subject: &dyn std::fmt::Display, detail: Detail) -> String {
        let clause = self.tag().said_of(subject);
        match (self, detail) {
            (Self::Invalid(problems), Detail::Full) => {
                format!(
                    "{clause}: {}",
                    listed(problems.iter(), Mark::Plain, Join::Semicolon)
                )
            }
            (Self::Unknown(unreadable), Detail::Full | Detail::Short) => {
                format!(
                    "{clause}; unreadable: {}",
                    listed(unreadable.iter(), Mark::Plain, Join::Semicolon)
                )
            }
            (Self::Invalid(_), Detail::Short) | (Self::Gone, Detail::Full | Detail::Short) => {
                clause
            }
        }
    }
}

/// What an identity comes to now, owned and with the files it was found in: what an
/// answer shows of a document.
///
/// The owned form of a whole [`DocState`] (where [`DocFailure`] is the owned form of
/// a failure only), made by [`DocState::presence`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DocPresence {
    /// A valid document.
    Valid {
        /// Its key.
        key: DocKey,
        /// Its file.
        origin: RelPath,
        /// Its hash now.
        hash: DocHash,
    },
    /// One or more files carry the identity, and they are invalid.
    Invalid {
        /// The files.
        files: NonEmptyVec<RelPath>,
        /// Every problem of every file.
        problems: NonEmptyVec<DocProblem>,
    },
    /// No file carries it.
    Gone,
    /// No file is known to carry it, but some file's identity could not be read.
    Unknown {
        /// The files whose identity could not be read.
        unreadable: NonEmptyVec<RelPath>,
    },
}

/// The files of one book, counted ([`Shelf::tally_by_book`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct BookTally {
    /// Valid documents.
    pub valid: usize,
    /// Files under the book that are no valid document.
    pub invalid: usize,
}

/// Every problem of every file that carries one identity, first file first.
fn all_problems(docs: &NonEmptyVec<InvalidDoc>) -> NonEmptyVec<DocProblem> {
    docs.flat_map(InvalidDoc::problems)
}

impl<'a> DocState<'a> {
    /// The valid document, or the owned form of why there is none.
    ///
    /// The one place a [`DocState`] becomes a [`DocFailure`]; the frames of the
    /// ledger and freshness take it from here. A caller that only asks which of the
    /// four a state is (and would drop what a failure carries) matches the
    /// [`DocState`] itself, so no problem is cloned for nothing.
    // `assess` asks this for every approved document and every pin, so the cost was
    // weighed: the failure is the answer, kept as it is by a refusal or a cause, and
    // boxing it would add an allocation to each failure and a deref to each match, to
    // save copying a few words on the way out. A failure is the rare case.
    #[allow(clippy::result_large_err)]
    pub fn valid_or_failure(&self) -> Result<&'a Doc, DocFailure> {
        match self {
            Self::Valid(doc) => Ok(doc),
            Self::Invalid(docs) => Err(DocFailure::Invalid(all_problems(docs))),
            Self::Gone => Err(DocFailure::Gone),
            Self::Unknown { unreadable } => Err(DocFailure::Unknown(unreadable.clone())),
        }
    }

    /// The owned form of the whole state, with the files: the one place an answer
    /// takes what to show of a document from.
    pub fn presence(&self) -> DocPresence {
        match self {
            Self::Valid(doc) => DocPresence::Valid {
                key: doc.key().clone(),
                origin: doc.origin().clone(),
                hash: *doc.hash(),
            },
            Self::Invalid(docs) => DocPresence::Invalid {
                files: docs.map(|d| d.origin().clone()),
                problems: all_problems(docs),
            },
            Self::Gone => DocPresence::Gone,
            Self::Unknown { unreadable } => DocPresence::Unknown {
                unreadable: unreadable.clone(),
            },
        }
    }
}

/// What a file says about its identity.
///
/// A file with no `id:` entry is known to carry none; a file whose `id:` could not
/// be read (or that could not be read at all) may carry any. Only the second makes
/// an identity no readable file has [`DocState::Unknown`] instead of
/// [`DocState::Gone`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum IdSeen {
    /// The identity was read.
    Read(DocId),
    /// The file has no `id:` entry.
    Absent,
    /// The file may have one, and it could not be read.
    Unreadable,
}

impl IdSeen {
    /// What a file that could not be read shows of its identity.
    fn of_read(id: ReadId) -> Self {
        match id {
            ReadId::Read(id) => Self::Read(id),
            ReadId::Unreadable => Self::Unreadable,
        }
    }

    fn id(self) -> Option<DocId> {
        match self {
            Self::Read(id) => Some(id),
            Self::Absent | Self::Unreadable => None,
        }
    }

    /// What the `id:` entry of a parsed front matter says.
    fn of_entry(entry: Option<&RawValue>) -> Self {
        match read_id(Written::of(entry)) {
            Ok(id) => Self::Read(id),
            Err(fault) => fault.seen(),
        }
    }
}

/// What a parsed front matter wrote for one entry, told once for the entry whatever
/// the type the book declares for it: nothing written, something written that says
/// nothing, or a value.
///
/// The one place it is decided whether a written value says something: no value
/// (`key:`, `null`), a text that is empty or white space only ([`NonBlank::new`] refuses
/// it), and a list with no item say nothing, for a field of any type (an integer, a bool or
/// an enum written `""` says nothing too), for a link, and for the `id:` and the
/// `key:`. What nothing means is left to the reader of the entry: a field left out when
/// it is optional and a problem when it is required ([`declared_field`]), a link with
/// no key, an `id:` or a `key:` that is written empty.
///
/// A text that says something travels as the [`NonBlank`] that proves it, so no reader
/// of an entry asks again whether it is blank. An item of a list is not judged here but
/// by [`read_items`]: it has no "not written" to read as.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Written<'a> {
    /// The entry is not in the front matter.
    Absent,
    /// The entry is there and says nothing.
    Nothing,
    /// The entry is a text that says something, as it was written (white space around
    /// it kept). Built here and nowhere else.
    Text(NonBlank),
    /// The entry says something that is not a text: a whole number, a truth value or a
    /// list with an item, as the adapter read it.
    Value(&'a RawValue),
}

impl<'a> Written<'a> {
    /// What an entry of a front matter, or none, comes to.
    fn of(raw: Option<&'a RawValue>) -> Self {
        match raw {
            None => Self::Absent,
            Some(RawValue::Null) => Self::Nothing,
            Some(RawValue::Text(text)) => {
                NonBlank::new(text.as_str()).map_or(Self::Nothing, Self::Text)
            }
            Some(RawValue::List(items)) if items.is_empty() => Self::Nothing,
            Some(value) => Self::Value(value),
        }
    }
}

/// The items of a list written in `entry`: those that say something, each as a
/// [`NonBlank`], and a problem for each that does not.
struct Items {
    /// The items that say something, in the order they are written.
    said: Vec<NonBlank>,
    /// An [`DocProblem::ItemBlank`] for every item that is empty or blank, in the
    /// order they are written.
    blank: Vec<DocProblem>,
}

impl Items {
    /// The items when every one says something, or every problem of the list.
    fn whole(self) -> Result<NonEmptyVec<NonBlank>, Vec<DocProblem>> {
        match NonEmptyVec::from_vec(self.said) {
            Ok(said) if self.blank.is_empty() => Ok(said),
            _ => Err(self.blank),
        }
    }
}

/// The items of a list written in `entry`, each made a text that says something: the
/// one reader of an item, for a list field, a link into a book and a link to
/// artifacts (before a text is read as a name). Every item that is empty or blank is a
/// [`DocProblem::ItemBlank`] with its place, counting from 1, and the items that say
/// something are still read, so that the rest of the list is judged as well.
///
/// A list with no item says nothing and is left to [`Written::of`], so the callers
/// never give it here; were it given, it is told as its first item missing.
fn read_items(entry: &EntryName, items: &[String]) -> Items {
    let blank = |at: usize| DocProblem::ItemBlank {
        entry: entry.clone(),
        at,
    };
    let mut read = Items {
        said: Vec::new(),
        blank: Vec::new(),
    };
    for (at, item) in items.iter().enumerate() {
        match NonBlank::new(item.as_str()) {
            Ok(item) => read.said.push(item),
            Err(_) => read.blank.push(blank(at + 1)),
        }
    }
    if items.is_empty() {
        read.blank.push(blank(1));
    }
    read
}

/// Why an `id:` entry gives no identity.
enum IdFault {
    /// There is no `id:` entry.
    Missing,
    /// The entry is written and says nothing.
    Empty,
    /// The entry is not a text.
    NotText,
    /// The entry is a text that is no identifier.
    Invalid(ValueError),
}

impl IdFault {
    /// What a file with this entry is known to say about its identity: one that is
    /// written empty may still have meant one, so it is not known to carry none.
    fn seen(&self) -> IdSeen {
        match self {
            Self::Missing => IdSeen::Absent,
            Self::Empty | Self::NotText | Self::Invalid(_) => IdSeen::Unreadable,
        }
    }

    fn into_problem(self) -> DocProblem {
        match self {
            Self::Missing => DocProblem::IdMissing,
            Self::Empty => DocProblem::IdEmpty,
            Self::NotText => DocProblem::IdNotText,
            Self::Invalid(e) => DocProblem::IdInvalid(e),
        }
    }
}

/// The `id:` entry of a parsed front matter: an identity, or why there is none.
fn read_id(entry: Written<'_>) -> Result<DocId, IdFault> {
    match entry {
        Written::Absent => Err(IdFault::Missing),
        Written::Nothing => Err(IdFault::Empty),
        Written::Text(text) => text.as_str().parse::<DocId>().map_err(IdFault::Invalid),
        Written::Value(_) => Err(IdFault::NotText),
    }
}

/// The `key:` entry of a parsed front matter, in the grammar of its book: a key, or
/// what is wrong with it.
fn read_key(book: &BookSchema, entry: Written<'_>) -> Result<DocKey, DocProblem> {
    match entry {
        Written::Absent => Err(DocProblem::KeyMissing),
        Written::Nothing => Err(DocProblem::KeyEmpty),
        Written::Text(text) => book
            .key()
            .parse(text.as_str())
            .map(|form| DocKey {
                book: book.name().clone(),
                form,
            })
            .map_err(DocProblem::KeyInvalid),
        Written::Value(_) => Err(DocProblem::KeyNotText),
    }
}

/// What the `id:` and the `key:` of a file come to, read together: the two things a
/// valid document must have, so that a file is either whole here or has at least one
/// problem with them.
enum Identity {
    /// Both were read.
    Whole { id: DocId, key: DocKey },
    /// One or both were not (or the file could not be read at all): what is known of
    /// its identity, and what is wrong.
    Broken {
        id: IdSeen,
        faults: NonEmptyVec<DocProblem>,
    },
}

impl Identity {
    fn of(id: Result<DocId, IdFault>, key: Result<DocKey, DocProblem>) -> Self {
        match (id, key) {
            (Ok(id), Ok(key)) => Self::Whole { id, key },
            (Ok(id), Err(key)) => Self::unusable(IdSeen::Read(id), key),
            (Err(id), Ok(_)) => Self::unusable(id.seen(), id.into_problem()),
            (Err(id), Err(key)) => Self::Broken {
                id: id.seen(),
                faults: NonEmptyVec::new(id.into_problem(), vec![key]),
            },
        }
    }

    /// A file whose identity is what it is, and which is invalid for `problem`.
    fn unusable(id: IdSeen, problem: DocProblem) -> Self {
        Self::unusable_all(id, NonEmptyVec::new(problem, Vec::new()))
    }

    /// A file whose identity is what it is, and which is invalid for every one of
    /// `faults`.
    fn unusable_all(id: IdSeen, faults: NonEmptyVec<DocProblem>) -> Self {
        Self::Broken { id, faults }
    }

    /// The identity, when it could be read.
    fn id(&self) -> Option<DocId> {
        match self {
            Self::Whole { id, .. } => Some(*id),
            Self::Broken { id, .. } => id.id(),
        }
    }
}

/// One link a book declares, as a file wrote it: its kind, and the keys written in
/// it (none when the file wrote none).
struct WrittenLink {
    kind: LinkKind,
    keys: Vec<String>,
}

/// One link to artifacts a book declares, as a file wrote it: its kind, and the
/// names written in it, each already read as a name.
struct WrittenArtifacts {
    kind: ArtifactLinkKind,
    names: Vec<ArtifactName>,
}

/// A file after its form is read, before the shelf looks at the files together.
struct Candidate {
    origin: RelPath,
    identity: Identity,
    /// The key the file writes, when it fits the book's grammar, whether or not the
    /// identity could be read: what makes a key taken for a new document.
    written_key: Option<DocKey>,
    /// What is wrong beyond the identity and the key; the check of the files together
    /// adds to it.
    problems: Vec<DocProblem>,
    fields: BTreeMap<EntryName, FieldValue>,
    /// Every link into a book the book declares, with what the file wrote in it.
    links: BTreeMap<EntryName, WrittenLink>,
    /// Every link to artifacts the book declares, with the names the file wrote.
    artifact_links: BTreeMap<EntryName, WrittenArtifacts>,
    body: String,
}

/// A file judged, once: a valid document still to be made whole, or an invalid
/// file with the identity it showed.
enum Judged {
    Valid {
        draft: DocDraft,
        links: BTreeMap<EntryName, WrittenLink>,
        artifact_links: BTreeMap<EntryName, WrittenArtifacts>,
    },
    Invalid {
        id: IdSeen,
        file: InvalidDoc,
    },
}

impl Candidate {
    fn unusable(origin: RelPath, id: IdSeen, problem: DocProblem) -> Self {
        Self::unusable_all(origin, id, NonEmptyVec::new(problem, Vec::new()))
    }

    /// A file invalid for every one of `faults`, whatever else it holds.
    fn unusable_all(origin: RelPath, id: IdSeen, faults: NonEmptyVec<DocProblem>) -> Self {
        Self {
            origin,
            identity: Identity::unusable_all(id, faults),
            written_key: None,
            problems: Vec::new(),
            fields: BTreeMap::new(),
            links: BTreeMap::new(),
            artifact_links: BTreeMap::new(),
            body: String::new(),
        }
    }

    /// The form of a file. `artifacts` are what the adapters answered for the names
    /// of the links to artifacts, read for one answer only: that a text is not a name.
    fn read(schema: &Schema, reading: DocReading, artifacts: &ArtifactReadings) -> Self {
        let (origin, entries, body) = match reading {
            DocReading::Unparsed { origin, id, why } => {
                let why = Shelf::told(schema, &origin, why);
                return Self::unusable(origin, IdSeen::of_read(id), DocProblem::Unreadable(why));
            }
            DocReading::Unheld { origin, id, values } => {
                let faults = values.map(|value| DocProblem::Unheld(value.clone()));
                return Self::unusable_all(origin, IdSeen::of_read(id), faults);
            }
            DocReading::Parsed {
                origin,
                entries,
                body,
            } => (origin, entries, body),
        };
        match schema.membership(&origin) {
            Membership::In(book) => Self::read_in(book, origin, &entries, body, artifacts),
            Membership::Outside => {
                let id = IdSeen::of_entry(entries.get(ID_ENTRY));
                Self::unusable(origin, id, DocProblem::OutsideBooks)
            }
        }
    }

    /// The form of a file under `book`: its identity and key, its fields and the keys
    /// written in its links, and every problem with them.
    fn read_in(
        book: &BookSchema,
        origin: RelPath,
        entries: &BTreeMap<String, RawValue>,
        body: String,
        found: &ArtifactReadings,
    ) -> Self {
        let key = read_key(book, Written::of(entries.get(KEY_ENTRY)));
        let written_key = key.as_ref().ok().cloned();
        let identity = Identity::of(read_id(Written::of(entries.get(ID_ENTRY))), key);
        let mut problems = Vec::new();
        let Entries {
            fields,
            links: written,
            artifacts,
        } = read_entries(book, entries, found, &mut problems);
        check_required(book, entries, &mut problems);
        Self {
            origin,
            identity,
            written_key,
            problems,
            fields,
            links: written_links(book, written),
            artifact_links: written_artifacts(book, artifacts),
            body,
        }
    }

    /// Judges the file, once: valid only when its identity and key were read and
    /// nothing else is wrong with it.
    fn judge(self) -> Judged {
        let Self {
            origin,
            identity,
            written_key: _,
            problems,
            fields,
            links,
            artifact_links,
            body,
        } = self;
        match identity {
            Identity::Whole { id, key } => match NonEmptyVec::from_vec(problems) {
                Err(_) => Judged::Valid {
                    draft: DocDraft::new(id, key, origin, fields, body),
                    links,
                    artifact_links,
                },
                Ok(problems) => Judged::Invalid {
                    id: IdSeen::Read(id),
                    file: InvalidDoc {
                        origin,
                        id: Some(id),
                        problems,
                    },
                },
            },
            Identity::Broken { id, faults } => Judged::Invalid {
                id,
                file: InvalidDoc {
                    origin,
                    id: id.id(),
                    problems: faults.extended(problems),
                },
            },
        }
    }
}

/// What the entries of a front matter come to, beside the identity and the key.
struct Entries {
    fields: BTreeMap<EntryName, FieldValue>,
    /// The keys written in each link into a book.
    links: BTreeMap<EntryName, Vec<String>>,
    /// The names written in each link to artifacts.
    artifacts: BTreeMap<EntryName, Vec<ArtifactName>>,
}

/// A text written in a link to artifacts, read as a name: the one reader of it,
/// which both [`Shelf::artifact_asks`] and the build use.
fn artifact_name(text: &str) -> Result<ArtifactName, ArtifactNameProblem> {
    ArtifactName::new(text)
}

/// What a book declares under the name of an entry that is neither `id` nor `key`.
enum Declared<'b> {
    /// A field.
    Field(&'b EntryName, &'b FieldDecl),
    /// A link into a book.
    Link(&'b EntryName, &'b LinkKind),
    /// A link to artifacts.
    Artifacts(&'b EntryName, &'b ArtifactLinkKind),
}

impl<'b> Declared<'b> {
    /// What `book` declares under `name`, or none.
    fn of(book: &'b BookSchema, name: &str) -> Option<Self> {
        if let Some((field, decl)) = book.fields().get_key_value(name) {
            Some(Self::Field(field, decl))
        } else if let Some((link, kind)) = book.links().get_key_value(name) {
            Some(Self::Link(link, kind))
        } else {
            book.artifact_links()
                .get_key_value(name)
                .map(|(link, kind)| Self::Artifacts(link, kind))
        }
    }
}

/// Whether a file of `book` need not hold the entry `name`: it says nothing, and the
/// build reads it as not written without a problem. The rule of [`Shelf::form_entries`],
/// beside the readers of an entry it follows ([`declared_field`], [`read_link`],
/// [`check_required`]).
fn need_not_hold(book: &BookSchema, name: &str, raw: &RawValue) -> bool {
    if Written::of(Some(raw)) != Written::Nothing || name == ID_ENTRY || name == KEY_ENTRY {
        return false;
    }
    match Declared::of(book, name) {
        None => false,
        Some(Declared::Field(_, decl)) => decl.presence == FieldPresence::Optional,
        Some(Declared::Link(_, kind)) => kind.cardinality.admits(0),
        Some(Declared::Artifacts(_, kind)) => kind.cardinality.admits(0),
    }
}

/// The fields of a file, the keys written in each link and the names written in each
/// link to artifacts, from every entry but `id` and `key`; what is wrong with an entry
/// goes to `problems`, a name its adapter answered is none included.
///
/// What an entry says is told once, at its head ([`Written::of`]).
fn read_entries(
    book: &BookSchema,
    entries: &BTreeMap<String, RawValue>,
    found: &ArtifactReadings,
    problems: &mut Vec<DocProblem>,
) -> Entries {
    let mut fields = BTreeMap::new();
    let mut written = BTreeMap::new();
    let mut artifacts = BTreeMap::new();
    for (name, raw) in entries {
        if name == ID_ENTRY || name == KEY_ENTRY {
            continue;
        }
        let Some(declared) = Declared::of(book, name) else {
            problems.push(DocProblem::UnknownEntry {
                name: name.clone(),
                book: book.name().clone(),
            });
            continue;
        };
        let entry = Written::of(Some(raw));
        match declared {
            Declared::Field(field, decl) => {
                if let Some(value) = declared_field(field, decl, entry, problems) {
                    fields.insert(field.clone(), value);
                }
            }
            Declared::Link(link, kind) => {
                if let Some(items) = read_link(link, kind.cardinality, entry, problems)
                    && items.whole
                {
                    let keys = items.said.into_iter().map(NonBlank::into_string).collect();
                    written.insert(link.clone(), keys);
                }
            }
            Declared::Artifacts(link, kind) => {
                let Some(items) = read_link(link, kind.cardinality, entry, problems) else {
                    continue;
                };
                let mut names = Vec::new();
                for text in &items.said {
                    match artifact_name(text.as_str()) {
                        Ok(name) => match found
                            .get(&kind.space, &name)
                            .and_then(|r| r.not_a_name_why())
                        {
                            Some(why) => problems.push(DocProblem::NotAnArtifactName {
                                link: link.clone(),
                                space: kind.space.clone(),
                                written: text.as_str().to_owned(),
                                why: why.clone(),
                            }),
                            None => names.push(name),
                        },
                        Err(why) => problems.push(DocProblem::ArtifactNameInvalid {
                            link: link.clone(),
                            written: text.as_str().to_owned(),
                            why,
                        }),
                    }
                }
                if items.whole {
                    artifacts.insert(link.clone(), names);
                }
            }
        }
    }
    Entries {
        fields,
        links: written,
        artifacts,
    }
}

/// What a file wrote in a link, into a book or to artifacts, as a list.
struct LinkItems {
    /// The items that say something ([`read_items`]), in the order they are written.
    said: Vec<NonBlank>,
    /// Whether the list is whole: every item says something and their number is one
    /// the link's count admits. Only a whole list is kept; the items of one that is
    /// not are still read, for the rest of its faults.
    whole: bool,
}

/// The items a file wrote in a link, into a book or to artifacts: each that says
/// something ([`read_items`]), and whether the list is whole. A link that said nothing,
/// or was not written, has no item, and its count is then asked as it always was. Every
/// fault of the list goes to `problems` (each item that says nothing, and a count the
/// link does not admit, which counts the items as they are written); there is no list
/// when the entry is not one.
fn read_link(
    link: &EntryName,
    cardinality: Cardinality,
    written: Written<'_>,
    problems: &mut Vec<DocProblem>,
) -> Option<LinkItems> {
    let (said, written, blank) = match written {
        Written::Absent | Written::Nothing => (Vec::new(), 0, false),
        Written::Value(RawValue::List(texts)) => {
            let Items { said, blank } = read_items(link, texts);
            let any_blank = !blank.is_empty();
            problems.extend(blank);
            (said, texts.len(), any_blank)
        }
        Written::Text(_) => {
            problems.push(DocProblem::LinkNotList {
                link: link.clone(),
                found: RawKind::Text,
            });
            return None;
        }
        Written::Value(other) => {
            problems.push(DocProblem::LinkNotList {
                link: link.clone(),
                found: other.kind(),
            });
            return None;
        }
    };
    let counted = cardinality.admits(written);
    if !counted {
        problems.push(DocProblem::LinkCount {
            link: link.clone(),
            cardinality,
            found: written,
        });
    }
    Some(LinkItems {
        said,
        whole: counted && !blank,
    })
}

/// What the book requires and the entries leave out: a field [`declared_field`]
/// refuses when it is not written, and a link whose cardinality does not admit
/// none. The problems come in this order: every
/// [`DocProblem::FieldMissing`], in the order the declaration writes the fields,
/// then every [`DocProblem::LinkCount`] of a link into a book, then of a link to
/// artifacts, each in the order the declaration writes the links.
fn check_required(
    book: &BookSchema,
    entries: &BTreeMap<String, RawValue>,
    problems: &mut Vec<DocProblem>,
) {
    for (field, decl) in book.fields() {
        if !entries.contains_key(field.as_str()) {
            declared_field(field, decl, Written::Absent, problems);
        }
    }
    let cardinalities = book
        .links()
        .iter()
        .map(|(link, kind)| (link, kind.cardinality))
        .chain(
            book.artifact_links()
                .iter()
                .map(|(link, kind)| (link, kind.cardinality)),
        );
    for (link, cardinality) in cardinalities {
        if !entries.contains_key(link.as_str()) && !cardinality.admits(0) {
            problems.push(DocProblem::LinkCount {
                link: link.clone(),
                cardinality,
                found: 0,
            });
        }
    }
}

/// Every link to artifacts the book declares, with the names the file wrote in it.
fn written_artifacts(
    book: &BookSchema,
    mut written: BTreeMap<EntryName, Vec<ArtifactName>>,
) -> BTreeMap<EntryName, WrittenArtifacts> {
    book.artifact_links()
        .iter()
        .map(|(name, kind)| {
            let link = WrittenArtifacts {
                kind: kind.clone(),
                names: written.remove(name).unwrap_or_default(),
            };
            (name.clone(), link)
        })
        .collect()
}

/// The links to artifacts of a valid document, each name made an [`Artifact`] of
/// its link's space from what the adapter answered (rule A1): a name with no answer
/// is a name with nothing there, never a link that cannot be followed.
fn resolve_artifacts(
    found: &ArtifactReadings,
    written: BTreeMap<EntryName, WrittenArtifacts>,
) -> BTreeMap<EntryName, ArtifactLink> {
    written
        .into_iter()
        .map(|(link, WrittenArtifacts { kind, names })| {
            let artifacts = names
                .into_iter()
                .map(|name| {
                    let key = ArtifactKey::new(kind.space.clone(), name);
                    let state = found.state_of(&key, &kind.comparison);
                    Artifact::new(key, state)
                })
                .collect();
            (link, ArtifactLink::new(kind, artifacts))
        })
        .collect()
}

/// What the links to artifacts of a book ask of the adapters, from the entries one
/// file writes: each name, in its link's space, with its link's comparison. A text
/// that is no name is left out (the build makes it a
/// [`DocProblem::ArtifactNameInvalid`]).
fn ask_of(book: &BookSchema, entries: &BTreeMap<String, RawValue>, asks: &mut ArtifactAsks) {
    for (name, kind) in book.artifact_links() {
        // An item that says nothing is a fault of the list ([`read_items`]) and asks
        // nothing; the names of the other items are asked, for the build judges them.
        if let Written::Value(RawValue::List(texts)) = Written::of(entries.get(name.as_str())) {
            for artifact in read_items(name, texts)
                .said
                .iter()
                .filter_map(|t| artifact_name(t.as_str()).ok())
            {
                asks.ask(kind.space.clone(), artifact, kind.comparison.clone());
            }
        }
    }
}

/// Every link the book declares, with the keys the file wrote in it.
fn written_links(
    book: &BookSchema,
    mut written: BTreeMap<EntryName, Vec<String>>,
) -> BTreeMap<EntryName, WrittenLink> {
    book.links()
        .iter()
        .map(|(name, kind)| {
            let link = WrittenLink {
                kind: kind.clone(),
                keys: written.remove(name).unwrap_or_default(),
            };
            (name.clone(), link)
        })
        .collect()
}

/// Who holds a key, read once: the judgement both [`KeyHit`] and a link's state are
/// made from.
enum Holders {
    /// No file.
    None,
    /// One file, with the hash of its document when the document is valid.
    One(DocId, Option<DocHash>),
    /// Several files, valid or not.
    Many(TwoOrMore<DocId>),
}

/// The keys of every file whose identity and key could be read, valid or not, and
/// the hash of each valid document: what a key in a book comes to is read from here,
/// by the build (for a link) before any document is made, and by
/// `Shelf::find_key` after.
#[derive(Debug, Clone, PartialEq, Eq)]
struct KeyIndex {
    keys: BTreeMap<DocKey, BTreeSet<DocId>>,
    valid: BTreeMap<DocId, DocHash>,
}

impl KeyIndex {
    /// The one reading of a key: more than one file with it is [`Holders::Many`]
    /// whether their documents are valid or not.
    fn holders(&self, key: &DocKey) -> Holders {
        match Count::of(self.keys.get(key).into_iter().flatten().copied()) {
            Count::None => Holders::None,
            Count::One(id) => Holders::One(id, self.valid.get(&id).copied()),
            Count::Many(ids) => Holders::Many(ids),
        }
    }

    fn find(&self, key: &DocKey) -> KeyHit {
        match self.holders(key) {
            Holders::None => KeyHit::Missing,
            Holders::One(id, Some(_)) => KeyHit::Valid(id),
            Holders::One(id, None) => KeyHit::Invalid(id),
            Holders::Many(ids) => KeyHit::Ambiguous(ids),
        }
    }
}

/// The links of a valid document, resolved: every link the book declares (the
/// candidate holds each, written or not), with its kind and what each key written in
/// it comes to.
fn resolve_links(
    index: &KeyIndex,
    written: BTreeMap<EntryName, WrittenLink>,
) -> BTreeMap<EntryName, Link> {
    written
        .into_iter()
        .map(|(name, WrittenLink { kind, keys })| {
            let states = keys
                .iter()
                .map(|text| link_state(index, &kind, text))
                .collect();
            (name, Link::new(kind, states))
        })
        .collect()
}

/// What one key written in a link comes to: read once, in the grammar of the book
/// the link points into (which the kind of the link carries), and looked up in the
/// index.
fn link_state(index: &KeyIndex, kind: &LinkKind, written: &str) -> LinkState {
    let target = &kind.target;
    let unresolved = |form, why| {
        LinkState::Unresolved(UnresolvedLink::new(
            written.to_owned(),
            target.clone(),
            form,
            why,
        ))
    };
    let form = match kind.target_key.parse(written) {
        Ok(form) => form,
        Err(problem) => return unresolved(None, UnresolvedWhy::Malformed(problem)),
    };
    let key = DocKey {
        book: target.clone(),
        form,
    };
    match index.holders(&key) {
        Holders::One(id, Some(hash)) => LinkState::Resolved(ResolvedLink::new(id, key, hash)),
        Holders::One(id, None) => unresolved(Some(key.form), UnresolvedWhy::TargetInvalid(id)),
        Holders::Many(ids) => unresolved(Some(key.form), UnresolvedWhy::Ambiguous(ids)),
        Holders::None => unresolved(Some(key.form), UnresolvedWhy::Missing),
    }
}

/// Whether a field meets its declaration, given what the document wrote of it
/// ([`Written`]): the value it holds, or `None` when it holds none. The one place that
/// says what a declared field requires and what nothing means for it: its type and
/// whether it is there and says something (told at [`FieldPresence::Required`]).
///
/// - Not written, or written with nothing in it (any type, `source: ""` of an integer
///   included): an optional field has no value, as if it were left out, and a
///   required one is [`DocProblem::FieldMissing`] when it is not written and
///   [`DocProblem::FieldEmpty`] when it is.
/// - A value is read by the declared type: a text as it was written (white space
///   around it kept; [`Written::of`] has proved it says something), a `line` that has
///   a line break is a [`DocProblem::NotOneLine`], a list is read item by item
///   ([`read_items`]) and every item that says nothing is a problem of its own.
///
/// What is wrong goes to `problems`, and there is no value then.
fn declared_field(
    field: &EntryName,
    decl: &FieldDecl,
    written: Written<'_>,
    problems: &mut Vec<DocProblem>,
) -> Option<FieldValue> {
    let mut refuse = |problem: DocProblem| {
        problems.push(problem);
        None
    };
    let ty = &decl.ty;
    let value = match (ty, written) {
        (_, Written::Absent) => {
            return match decl.presence {
                FieldPresence::Required => refuse(DocProblem::FieldMissing {
                    field: field.clone(),
                }),
                FieldPresence::Optional => None,
            };
        }
        (_, Written::Nothing) => {
            return match decl.presence {
                FieldPresence::Required => refuse(DocProblem::FieldEmpty {
                    field: field.clone(),
                }),
                FieldPresence::Optional => None,
            };
        }
        (FieldType::Scalar(ScalarType::Text), Written::Text(text)) => FieldValue::Text(text),
        (FieldType::Scalar(ScalarType::Line), Written::Text(text)) => {
            // Not blank is proved; only a line break is left for `OneLine` to refuse.
            match OneLine::new(text.into_string()) {
                Ok(line) => FieldValue::Line(line),
                Err(_) => {
                    return refuse(DocProblem::NotOneLine {
                        field: field.clone(),
                    });
                }
            }
        }
        (FieldType::Scalar(ScalarType::Integer), Written::Value(RawValue::Integer(n))) => {
            FieldValue::Integer(*n)
        }
        (FieldType::Scalar(ScalarType::Bool), Written::Value(RawValue::Bool(b))) => {
            FieldValue::Bool(*b)
        }
        (FieldType::Scalar(ScalarType::TextList), Written::Value(RawValue::List(items))) => {
            match read_items(field, items).whole() {
                Ok(items) => FieldValue::TextList(items),
                Err(blank) => {
                    blank.into_iter().for_each(|problem| {
                        refuse(problem);
                    });
                    return None;
                }
            }
        }
        (FieldType::Enum(_), Written::Text(word)) => match FieldValue::read(ty, &word) {
            Some(value) => value,
            None => {
                return refuse(DocProblem::FieldNotInEnum {
                    field: field.clone(),
                    found: word.into_string(),
                });
            }
        },
        (_, Written::Text(_)) => {
            return refuse(DocProblem::FieldMismatch {
                field: field.clone(),
                expected: ty.clone(),
                found: RawKind::Text,
            });
        }
        (_, Written::Value(raw)) => {
            return refuse(DocProblem::FieldMismatch {
                field: field.clone(),
                expected: ty.clone(),
                found: raw.kind(),
            });
        }
    };
    Some(value)
}

/// The documents of a project, checked: valid ones, invalid ones with their
/// reasons, and the cycles among the links.
///
/// A shelf keeps the [`Schema`] it was built with, because what a key may be is
/// asked of it later (by the lookup, for what a person typed). What a link means is
/// not: each link of a [`Doc`] carries its kind.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Shelf {
    schema: Schema,
    docs: BTreeMap<DocId, Doc>,
    invalid: BTreeMap<DocId, NonEmptyVec<InvalidDoc>>,
    unreadable: Vec<InvalidDoc>,
    anonymous: Vec<InvalidDoc>,
    skipped: Vec<SkippedFile>,
    index: KeyIndex,
    /// Every key a file of a book writes in the book's grammar, valid file or not,
    /// identity read or not: the keys a new document cannot take, and the numbers
    /// the next key counts from; each with the files that write it.
    written_keys: BTreeMap<DocKey, BTreeSet<RelPath>>,
    cycles: Vec<Cycle>,
}

impl Shelf {
    /// The names the files write in their links to artifacts, per space, each with
    /// the comparisons of the links that write it: what the adapters are to resolve
    /// before [`Shelf::build`], so that each becomes an [`Artifact`].
    ///
    /// A text that is no name is left out here (the build makes it a
    /// [`DocProblem::ArtifactNameInvalid`]); a file under no book writes none.
    pub fn artifact_asks(schema: &Schema, readings: &[DocReading]) -> ArtifactAsks {
        let mut asks = ArtifactAsks::new();
        for reading in readings {
            let DocReading::Parsed {
                origin, entries, ..
            } = reading
            else {
                continue;
            };
            let Membership::In(book) = schema.membership(origin) else {
                continue;
            };
            ask_of(book, entries, &mut asks);
        }
        asks
    }

    /// The entries a file of `book` is written with, from the entries of a document:
    /// those the build reads as not written and the file need not hold are left out.
    ///
    /// The one place that is decided, for `new` and for `fmt` alike. An entry that says
    /// nothing (the build's own `Written::of`) is left out only when the build reads it as one that
    /// is not written and tells no problem for it: a field the book declares optional,
    /// or a link (into a book or to artifacts) whose count admits none. It is kept for
    /// a required field, a link whose count does not admit none, the `id` and the `key`,
    /// and an entry the book does not declare, because leaving it out would change
    /// which problems the build tells, or their order. Every entry that says something
    /// is kept. A book the schema does not declare leaves nothing out.
    ///
    /// The build of the result is the build of `entries`: the same document, the same
    /// problems in the same order.
    pub fn form_entries(
        schema: &Schema,
        book: &BookName,
        entries: &BTreeMap<String, RawValue>,
    ) -> FormEntries {
        let book = schema.book(book);
        FormEntries(
            entries
                .iter()
                .filter(|(name, raw)| !book.is_some_and(|book| need_not_hold(book, name, raw)))
                .map(|(name, raw)| (name.clone(), raw.clone()))
                .collect(),
        )
    }

    /// A shelf from the schema, what an adapter read of the documents, and what the
    /// adapters answered for the names [`Shelf::artifact_asks`] asked.
    ///
    /// The one place a [`Doc`] is made. It never fails; see the module page for the
    /// order of the steps. A name written in a link to artifacts with no answer in
    /// `artifacts` is a name with nothing there.
    pub fn build(schema: &Schema, readings: Vec<DocReading>, artifacts: &ArtifactReadings) -> Self {
        let mut skipped = Vec::new();
        let mut kept = Vec::with_capacity(readings.len());
        for reading in readings {
            match Self::skip_of(schema, &reading) {
                Some(skip) => skipped.push(skip),
                None => kept.push(reading),
            }
        }
        let candidates = flag_duplicates(
            kept.into_iter()
                .map(|reading| Candidate::read(schema, reading, artifacts))
                .collect(),
        );
        let keys = keys_of(&candidates);
        let mut written_keys: BTreeMap<DocKey, BTreeSet<RelPath>> = BTreeMap::new();
        for candidate in &candidates {
            if let Some(key) = &candidate.written_key {
                written_keys
                    .entry(key.clone())
                    .or_default()
                    .insert(candidate.origin.clone());
            }
        }
        let Sorted {
            drafts,
            invalid,
            unreadable,
            anonymous,
        } = Sorted::of(candidates);

        let index = KeyIndex {
            keys,
            valid: drafts
                .iter()
                .map(|(draft, _, _)| (draft.id(), *draft.hash()))
                .collect(),
        };
        let docs: BTreeMap<DocId, Doc> = drafts
            .into_iter()
            .map(|(draft, written, names)| {
                let links = resolve_links(&index, written);
                let artifact_links = resolve_artifacts(artifacts, names);
                (draft.id(), Doc::new(draft, links, artifact_links))
            })
            .collect();

        let cycles = cycles_of(CycleSource::Links, &depends_edges(&docs));
        Self {
            schema: schema.clone(),
            docs,
            invalid,
            unreadable,
            anonymous,
            skipped,
            index,
            written_keys,
            cycles,
        }
    }

    /// Whether a file, as the adapter read it, is not a document of its book: the
    /// file has no front matter at all, and its book says
    /// `without_front_matter = "skip"`.
    ///
    /// The one place a file is skipped; [`Shelf::build`] leaves out what it answers,
    /// and `fmt` does not look at it either. Only the reading
    /// [`RawProblem::NoFrontMatter`] can be skipped: a front matter that is opened
    /// and not closed, or that is not YAML, is a file with a front matter that is
    /// broken, and that is invalid whatever the book says, so that a mistyped
    /// delimiter is never passed over.
    pub fn skip_of(schema: &Schema, reading: &DocReading) -> Option<SkippedFile> {
        let DocReading::Unparsed {
            origin,
            why: RawProblem::NoFrontMatter,
            ..
        } = reading
        else {
            return None;
        };
        let Membership::In(book) = schema.membership(origin) else {
            return None;
        };
        (book.scope().without_front_matter() == WithoutFrontMatter::Skip).then(|| SkippedFile {
            origin: origin.clone(),
            book: book.name().clone(),
            why: SkipWhy::NoFrontMatter,
        })
    }

    /// What the front matter of a file that could not be read is told as:
    /// [`RawProblem::BadOpening`] only in a book that skips files with no front
    /// matter, where it is what keeps the file from being skipped; in any other it
    /// is [`RawProblem::NoFrontMatter`], so that a declaration that says nothing of
    /// files with no front matter reads every file as before. Every other problem
    /// is told as it is.
    ///
    /// The one place this is decided; the build and `fmt` both ask it.
    pub fn told(schema: &Schema, origin: &RelPath, why: RawProblem) -> RawProblem {
        let skips = matches!(
            schema.membership(origin),
            Membership::In(book) if book.scope().without_front_matter() == WithoutFrontMatter::Skip
        );
        match why {
            RawProblem::BadOpening if !skips => RawProblem::NoFrontMatter,
            why => why,
        }
    }

    /// The files in the range of a book that are not its documents, in the order
    /// they were read.
    pub fn skipped(&self) -> &[SkippedFile] {
        &self.skipped
    }

    /// What a key in a book comes to.
    ///
    /// The one place a key is looked up in a book: the lookup of what a person typed
    /// calls it, and the build of a link reads the same index before the shelf
    /// exists. More than one document with the key is
    /// [`Ambiguous`](KeyHit::Ambiguous) whether they are valid or not (a key written
    /// twice makes every holder invalid).
    pub(super) fn find_key(&self, book: &BookName, form: &KeyForm) -> KeyHit {
        self.index.find(&DocKey {
            book: book.clone(),
            form: form.clone(),
        })
    }

    /// What an identity comes to now.
    ///
    /// The one place it is decided. An identity no file carries is
    /// [`Gone`](DocState::Gone) only when every file's identity could be read;
    /// otherwise it is [`Unknown`](DocState::Unknown), because it may be in a file
    /// that could not say.
    pub fn state_of(&self, id: DocId) -> DocState<'_> {
        if let Some(doc) = self.docs.get(&id) {
            return DocState::Valid(doc);
        }
        if let Some(invalid) = self.invalid.get(&id) {
            return DocState::Invalid(invalid);
        }
        let origins: Vec<RelPath> = self.unreadable.iter().map(|d| d.origin.clone()).collect();
        match NonEmptyVec::from_vec(origins) {
            Ok(unreadable) => DocState::Unknown { unreadable },
            Err(_) => DocState::Gone,
        }
    }

    /// The valid documents that point at `id` with a `Replaces` link.
    ///
    /// A fact about links, not a verdict: whether such a document is a successor is
    /// decided by [`Successors::of`](super::freshness::Successors::of).
    pub fn replacers_of(&self, id: DocId) -> Vec<DocId> {
        self.docs
            .values()
            .filter(|doc| {
                doc.links_of(Propagation::Replaces)
                    .into_iter()
                    .flat_map(|(_, states)| states)
                    .any(|state| matches!(state, LinkState::Resolved(target) if target.id() == id))
            })
            .map(Doc::id)
            .collect()
    }

    /// The cycles among the `Depends` links of the valid documents.
    pub fn cycles(&self) -> &[Cycle] {
        &self.cycles
    }

    /// The schema the shelf was built with.
    pub fn schema(&self) -> &Schema {
        &self.schema
    }

    /// A valid document.
    pub fn doc(&self, id: DocId) -> Option<&Doc> {
        self.docs.get(&id)
    }

    /// The valid documents.
    pub fn docs(&self) -> &BTreeMap<DocId, Doc> {
        &self.docs
    }

    /// The invalid files whose identity could be read, grouped by identity.
    pub fn invalid(&self) -> &BTreeMap<DocId, NonEmptyVec<InvalidDoc>> {
        &self.invalid
    }

    /// The invalid files whose identity could not be read: any of them may carry an
    /// identity no readable file has, which is why such an identity is
    /// [`DocState::Unknown`].
    pub fn unreadable(&self) -> &[InvalidDoc] {
        &self.unreadable
    }

    /// The invalid files that have no `id:` entry: they carry no identity, so they
    /// leave every other identity's state as it is.
    pub fn anonymous(&self) -> &[InvalidDoc] {
        &self.anonymous
    }

    /// How many valid documents and how many invalid files each book holds.
    ///
    /// Every file is counted in the book `Schema::membership` puts it in, valid or
    /// not, readable identity or not: one rule for both counts. Every declared book is
    /// listed, an empty one with zeros.
    pub fn tally_by_book(&self) -> BTreeMap<BookName, BookTally> {
        let mut tally: BTreeMap<BookName, BookTally> = self
            .schema
            .books()
            .keys()
            .map(|name| (name.clone(), BookTally::default()))
            .collect();
        let mut count = |origin: &RelPath, valid: bool| {
            if let Membership::In(book) = self.schema.membership(origin)
                && let Some(entry) = tally.get_mut(book.name())
            {
                if valid {
                    entry.valid += 1;
                } else {
                    entry.invalid += 1;
                }
            }
        };
        for doc in self.docs.values() {
            count(doc.origin(), true);
        }
        for invalid in self.invalid.values().flat_map(NonEmptyVec::iter) {
            count(invalid.origin(), false);
        }
        for no_id in self.unreadable.iter().chain(&self.anonymous) {
            count(no_id.origin(), false);
        }
        tally
    }

    /// Every identity some file carries, valid or not.
    pub fn ids(&self) -> BTreeSet<DocId> {
        self.docs
            .keys()
            .chain(self.invalid.keys())
            .copied()
            .collect()
    }
}

/// The files judged, by where each goes in the shelf.
#[derive(Default)]
struct Sorted {
    /// The valid ones, to be made whole, with the keys written in their links and the
    /// names written in their links to artifacts.
    #[allow(clippy::type_complexity)]
    drafts: Vec<(
        DocDraft,
        BTreeMap<EntryName, WrittenLink>,
        BTreeMap<EntryName, WrittenArtifacts>,
    )>,
    /// The invalid ones whose identity was read, grouped by identity.
    invalid: BTreeMap<DocId, NonEmptyVec<InvalidDoc>>,
    /// The invalid ones whose identity could not be read.
    unreadable: Vec<InvalidDoc>,
    /// The invalid ones that have no `id:` entry.
    anonymous: Vec<InvalidDoc>,
}

impl Sorted {
    /// Judges every file once and puts it where it goes.
    fn of(candidates: Vec<Candidate>) -> Self {
        let mut sorted = Self::default();
        for candidate in candidates {
            match candidate.judge() {
                Judged::Valid {
                    draft,
                    links,
                    artifact_links,
                } => sorted.drafts.push((draft, links, artifact_links)),
                Judged::Invalid {
                    id: IdSeen::Read(id),
                    file,
                } => match sorted.invalid.entry(id) {
                    Entry::Occupied(mut held) => held.get_mut().push(file),
                    Entry::Vacant(slot) => {
                        slot.insert(NonEmptyVec::new(file, Vec::new()));
                    }
                },
                Judged::Invalid {
                    id: IdSeen::Absent,
                    file,
                } => sorted.anonymous.push(file),
                Judged::Invalid {
                    id: IdSeen::Unreadable,
                    file,
                } => sorted.unreadable.push(file),
            }
        }
        sorted
    }
}

/// The identities that hold each key: every file whose identity and key could both
/// be read, valid or not.
fn keys_of(candidates: &[Candidate]) -> BTreeMap<DocKey, BTreeSet<DocId>> {
    let mut keys: BTreeMap<DocKey, BTreeSet<DocId>> = BTreeMap::new();
    for candidate in candidates {
        if let Identity::Whole { id, key } = &candidate.identity {
            keys.entry(key.clone()).or_default().insert(*id);
        }
    }
    keys
}

/// The `Depends` links of the valid documents that resolved, as a graph of
/// identities.
fn depends_edges(docs: &BTreeMap<DocId, Doc>) -> BTreeMap<DocId, BTreeSet<DocId>> {
    let mut edges: BTreeMap<DocId, BTreeSet<DocId>> = BTreeMap::new();
    for doc in docs.values() {
        for (_, states) in doc.links_of(Propagation::Depends) {
            for state in states {
                if let LinkState::Resolved(target) = state {
                    edges.entry(doc.id()).or_default().insert(target.id());
                }
            }
        }
    }
    edges
}

/// Marks as invalid every file that shares its `id:` with another, and every file
/// of a book that shares its key with another file of the book.
fn flag_duplicates(mut candidates: Vec<Candidate>) -> Vec<Candidate> {
    let mut by_id: BTreeMap<DocId, Vec<usize>> = BTreeMap::new();
    let mut by_key: BTreeMap<DocKey, Vec<usize>> = BTreeMap::new();
    for (i, c) in candidates.iter().enumerate() {
        if let Some(id) = c.identity.id() {
            by_id.entry(id).or_default().push(i);
        }
        if let Identity::Whole { key, .. } = &c.identity {
            by_key.entry(key.clone()).or_default().push(i);
        }
    }

    let mut flags: Vec<(usize, DocProblem)> = Vec::new();
    for group in by_id.values().filter(|g| g.len() > 1) {
        for &i in group {
            if let Some(others) = origins_but(&candidates, group, i) {
                flags.push((i, DocProblem::DuplicateId { others }));
            }
        }
    }
    for (key, group) in by_key.iter().filter(|(_, g)| g.len() > 1) {
        for &i in group {
            if let Some(others) = origins_but(&candidates, group, i) {
                flags.push((
                    i,
                    DocProblem::DuplicateKey {
                        key: key.clone(),
                        others,
                    },
                ));
            }
        }
    }
    for (i, problem) in flags {
        candidates[i].problems.push(problem);
    }
    candidates
}

fn origins_but(
    candidates: &[Candidate],
    group: &[usize],
    skip: usize,
) -> Option<NonEmptyVec<RelPath>> {
    let origins: Vec<RelPath> = group
        .iter()
        .filter(|&&j| j != skip)
        .map(|&j| candidates[j].origin.clone())
        .collect();
    NonEmptyVec::from_vec(origins).ok()
}

#[cfg(test)]
mod artifact_tests;
#[cfg(test)]
mod comparison_tests;
#[cfg(test)]
mod empty_tests;
#[cfg(test)]
mod form_tests;
#[cfg(test)]
mod new_tests;
#[cfg(test)]
mod plan_tests;
#[cfg(test)]
mod said_tests;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod unheld_tests;
