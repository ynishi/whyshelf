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
//!   DocReading ──► 0  Unparsed?            ──► InvalidDoc (Unreadable)
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
//!      │                                   path of a link to artifacts made an
//!      │                                   Artifact from the ArtifactReadings (A1)
//!      └────────►  7  link cycles          (strongly connected components)
//! ```
//!
//! The paths a link to artifacts writes are read before the build, by
//! [`Shelf::artifact_asks`] (each with the comparisons of the links that write it), so
//! that an adapter can look at those files first: a path is read once, by one reader,
//! for both, and each link takes the hash of its own comparison. A path that has no file is an
//! [`Artifact`] that is absent, not a link that cannot be followed (rule A1); a text
//! that is no path is a [`DocProblem::ArtifactPathInvalid`].
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
//! above, the same code, on the file it would be.

use std::collections::btree_map::Entry;
use std::collections::{BTreeMap, BTreeSet};

use thiserror::Error;

use crate::domain::error::ValueError;
use crate::domain::id::DocId;
use crate::domain::location::RelPath;
use crate::domain::nonempty::{Count, NonEmptyVec, TwoOrMore};

use super::artifact::{Artifact, ArtifactAsks, ArtifactReadings, ArtifactState};
use super::doc::{
    ArtifactLink, Doc, DocDraft, DocHash, DocKey, FieldValue, Link, LinkState, ResolvedLink,
    UnresolvedLink, UnresolvedWhy,
};
use super::graph::{Cycle, CycleSource, cycles_of};
use super::schema::{
    ArtifactLinkKind, BookName, BookSchema, Cardinality, EntryName, FieldPresence, FieldType,
    FieldWord, ID_ENTRY, KEY_ENTRY, KeyForm, KeyProblem, LinkKind, Membership, Propagation, Schema,
};

mod new;

pub use new::{ClaimKeyProblem, NewDoc, NewDocProblem, NewKey, NextKeyProblem};

/// A value of the front matter, as the adapter's YAML parser gave it: not yet
/// checked against a field's type.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RawValue {
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
    /// A text.
    Text,
    /// A whole number.
    Integer,
    /// A truth value.
    Bool,
    /// A list of texts.
    List,
}

impl RawValue {
    /// The kind of the value.
    pub fn kind(&self) -> RawKind {
        match self {
            Self::Text(_) => RawKind::Text,
            Self::Integer(_) => RawKind::Integer,
            Self::Bool(_) => RawKind::Bool,
            Self::List(_) => RawKind::List,
        }
    }
}

/// Why a file's front matter could not be read as a mapping of values.
///
/// The adapter's YAML parser finds these, because a mapping built in memory cannot
/// show a key that was written twice.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum RawProblem {
    /// The file does not start with a `---` line, so it has no front matter.
    #[error("the file has no front matter: it must start with a line `---`")]
    NoFrontMatter,
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
    /// A value that is none of a text, a whole number, a truth value and a list of
    /// texts, with the kind it was read as.
    #[error("the value of `{key}` is {kind}, which a front matter does not hold{}", kind.hint())]
    UnsupportedValue {
        /// The key.
        key: String,
        /// What the value was read as.
        kind: UnheldKind,
    },
}

/// What a value of a front matter was read as when it is none of the kinds a front
/// matter holds (a text, a whole number, a truth value, a list of texts).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnheldKind {
    /// No value (`key:` with nothing after it, or `null`).
    Null,
    /// A number written with a decimal point or an exponent (`1.5`, `1.0`, `1e3`).
    Float,
    /// A whole number too large for the integers a front matter holds (64-bit,
    /// signed).
    TooLarge,
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
            Self::Null => "no value".to_owned(),
            Self::Float => "a number written with a decimal point or an exponent".to_owned(),
            Self::TooLarge => "a whole number above the largest one (2^63 - 1)".to_owned(),
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
            Self::TooLarge => ": put the number in quotes as a text",
            Self::Null => ": write a value, or leave the key out",
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
    /// The file is under no book's directory.
    #[error("the file is under no book")]
    OutsideBooks,
    /// There is no `id:`.
    #[error("there is no `id:`")]
    IdMissing,
    /// The `id:` is not a text.
    #[error("`id:` is not a text")]
    IdNotText,
    /// The `id:` is not an identifier.
    #[error("`id:` is not an identifier")]
    IdInvalid(#[source] ValueError),
    /// There is no `key:`.
    #[error("there is no `key:`")]
    KeyMissing,
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
    #[error("`{name}` is neither a field nor a link of the book")]
    UnknownEntry {
        /// The entry.
        name: String,
    },
    /// A required field is missing.
    #[error("the required field `{field}` is missing")]
    FieldMissing {
        /// The field.
        field: EntryName,
    },
    /// A field of a kind of value other than its type.
    #[error("the field `{field}` is {found:?}, not {expected:?}")]
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
    #[error("the link `{link}` is {found:?}, not a list of keys")]
    LinkNotList {
        /// The link.
        link: EntryName,
        /// What was written.
        found: RawKind,
    },
    /// A text written in a link to artifacts that is no path under the root (an
    /// absolute path, one that climbs out with `..`, an empty segment).
    #[error("`{written}` in the link `{link}` is not a path under the root of the project")]
    ArtifactPathInvalid {
        /// The link.
        link: EntryName,
        /// What was written.
        written: String,
        /// Why it is no path.
        #[source]
        source: ValueError,
    },
    /// A link with a number of keys its cardinality does not admit.
    #[error("the link `{link}` has {found} key(s), which {cardinality:?} does not admit")]
    LinkCount {
        /// The link.
        link: EntryName,
        /// What the book declares.
        cardinality: Cardinality,
        /// How many keys were written.
        found: usize,
    },
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

impl DocFailure {
    /// Which of the three it is.
    pub fn tag(&self) -> FailureTag {
        match self {
            Self::Invalid(_) => FailureTag::Invalid,
            Self::Gone => FailureTag::Gone,
            Self::Unknown(_) => FailureTag::Unknown,
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
    fn id(self) -> Option<DocId> {
        match self {
            Self::Read(id) => Some(id),
            Self::Absent | Self::Unreadable => None,
        }
    }

    /// What the `id:` entry of a parsed front matter says.
    fn of_entry(entry: Option<&RawValue>) -> Self {
        match read_id(entry) {
            Ok(id) => Self::Read(id),
            Err(fault) => fault.seen(),
        }
    }
}

/// Why an `id:` entry gives no identity.
enum IdFault {
    /// There is no `id:` entry.
    Missing,
    /// The entry is not a text.
    NotText,
    /// The entry is a text that is no identifier.
    Invalid(ValueError),
}

impl IdFault {
    /// What a file with this entry is known to say about its identity.
    fn seen(&self) -> IdSeen {
        match self {
            Self::Missing => IdSeen::Absent,
            Self::NotText | Self::Invalid(_) => IdSeen::Unreadable,
        }
    }

    fn into_problem(self) -> DocProblem {
        match self {
            Self::Missing => DocProblem::IdMissing,
            Self::NotText => DocProblem::IdNotText,
            Self::Invalid(e) => DocProblem::IdInvalid(e),
        }
    }
}

/// The `id:` entry of a parsed front matter: an identity, or why there is none.
fn read_id(entry: Option<&RawValue>) -> Result<DocId, IdFault> {
    match entry {
        None => Err(IdFault::Missing),
        Some(RawValue::Text(text)) => text.parse::<DocId>().map_err(IdFault::Invalid),
        Some(_) => Err(IdFault::NotText),
    }
}

/// The `key:` entry of a parsed front matter, in the grammar of its book: a key, or
/// what is wrong with it.
fn read_key(book: &BookSchema, entry: Option<&RawValue>) -> Result<DocKey, DocProblem> {
    match entry {
        None => Err(DocProblem::KeyMissing),
        Some(RawValue::Text(text)) => book
            .key()
            .parse(text)
            .map(|form| DocKey {
                book: book.name().clone(),
                form,
            })
            .map_err(DocProblem::KeyInvalid),
        Some(_) => Err(DocProblem::KeyNotText),
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
        Self::Broken {
            id,
            faults: NonEmptyVec::new(problem, Vec::new()),
        }
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
/// paths written in it, each already read as a path.
struct WrittenArtifacts {
    kind: ArtifactLinkKind,
    paths: Vec<RelPath>,
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
    /// Every link to artifacts the book declares, with the paths the file wrote.
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
        Self {
            origin,
            identity: Identity::unusable(id, problem),
            written_key: None,
            problems: Vec::new(),
            fields: BTreeMap::new(),
            links: BTreeMap::new(),
            artifact_links: BTreeMap::new(),
            body: String::new(),
        }
    }

    fn read(schema: &Schema, reading: DocReading) -> Self {
        let (origin, entries, body) = match reading {
            DocReading::Unparsed { origin, id, why } => {
                let id = match id {
                    ReadId::Read(id) => IdSeen::Read(id),
                    ReadId::Unreadable => IdSeen::Unreadable,
                };
                return Self::unusable(origin, id, DocProblem::Unreadable(why));
            }
            DocReading::Parsed {
                origin,
                entries,
                body,
            } => (origin, entries, body),
        };
        match schema.membership(&origin) {
            Membership::In(book) => Self::read_in(book, origin, &entries, body),
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
    ) -> Self {
        let key = read_key(book, entries.get(KEY_ENTRY));
        let written_key = key.as_ref().ok().cloned();
        let identity = Identity::of(read_id(entries.get(ID_ENTRY)), key);
        let mut problems = Vec::new();
        let Entries {
            fields,
            links: written,
            artifacts,
        } = read_entries(book, entries, &mut problems);
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
    /// The paths written in each link to artifacts.
    artifacts: BTreeMap<EntryName, Vec<RelPath>>,
}

/// A text written in a link to artifacts, read as a path under the root: the one
/// reader of it, which both [`Shelf::artifact_asks`] and the build use.
fn artifact_path(text: &str) -> Result<RelPath, ValueError> {
    text.parse::<RelPath>()
}

/// The fields of a file, the keys written in each link and the paths written in each
/// link to artifacts, from every entry but `id` and `key`; what is wrong with an entry
/// goes to `problems`.
fn read_entries(
    book: &BookSchema,
    entries: &BTreeMap<String, RawValue>,
    problems: &mut Vec<DocProblem>,
) -> Entries {
    let mut fields = BTreeMap::new();
    let mut written = BTreeMap::new();
    let mut artifacts = BTreeMap::new();
    for (name, raw) in entries {
        if name == ID_ENTRY || name == KEY_ENTRY {
            continue;
        }
        if let Some((field, decl)) = book.fields().get_key_value(name.as_str()) {
            match field_value(field, &decl.ty, raw) {
                Ok(value) => {
                    fields.insert(field.clone(), value);
                }
                Err(problem) => problems.push(problem),
            }
        } else if let Some((link, kind)) = book.links().get_key_value(name.as_str()) {
            match raw {
                RawValue::List(keys) if kind.cardinality.admits(keys.len()) => {
                    written.insert(link.clone(), keys.clone());
                }
                RawValue::List(keys) => problems.push(DocProblem::LinkCount {
                    link: link.clone(),
                    cardinality: kind.cardinality,
                    found: keys.len(),
                }),
                other => problems.push(DocProblem::LinkNotList {
                    link: link.clone(),
                    found: other.kind(),
                }),
            }
        } else if let Some((link, kind)) = book.artifact_links().get_key_value(name.as_str()) {
            match raw {
                RawValue::List(texts) if kind.cardinality.admits(texts.len()) => {
                    let mut paths = Vec::new();
                    for text in texts {
                        match artifact_path(text) {
                            Ok(path) => paths.push(path),
                            Err(source) => problems.push(DocProblem::ArtifactPathInvalid {
                                link: link.clone(),
                                written: text.clone(),
                                source,
                            }),
                        }
                    }
                    artifacts.insert(link.clone(), paths);
                }
                RawValue::List(texts) => problems.push(DocProblem::LinkCount {
                    link: link.clone(),
                    cardinality: kind.cardinality,
                    found: texts.len(),
                }),
                other => problems.push(DocProblem::LinkNotList {
                    link: link.clone(),
                    found: other.kind(),
                }),
            }
        } else {
            problems.push(DocProblem::UnknownEntry { name: name.clone() });
        }
    }
    Entries {
        fields,
        links: written,
        artifacts,
    }
}

/// What the book requires and the entries leave out: a required field, and a link
/// whose cardinality does not admit none.
fn check_required(
    book: &BookSchema,
    entries: &BTreeMap<String, RawValue>,
    problems: &mut Vec<DocProblem>,
) {
    for (field, decl) in book.fields() {
        if decl.presence == FieldPresence::Required && !entries.contains_key(field.as_str()) {
            problems.push(DocProblem::FieldMissing {
                field: field.clone(),
            });
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

/// Every link to artifacts the book declares, with the paths the file wrote in it.
fn written_artifacts(
    book: &BookSchema,
    mut written: BTreeMap<EntryName, Vec<RelPath>>,
) -> BTreeMap<EntryName, WrittenArtifacts> {
    book.artifact_links()
        .iter()
        .map(|(name, kind)| {
            let link = WrittenArtifacts {
                kind: kind.clone(),
                paths: written.remove(name).unwrap_or_default(),
            };
            (name.clone(), link)
        })
        .collect()
}

/// The links to artifacts of a valid document, each path made an [`Artifact`] from
/// what the adapter found there (rule A1): a path with no reading is a path with no
/// file, never a link that cannot be followed.
fn resolve_artifacts(
    found: &ArtifactReadings,
    written: BTreeMap<EntryName, WrittenArtifacts>,
) -> BTreeMap<EntryName, ArtifactLink> {
    written
        .into_iter()
        .map(|(name, WrittenArtifacts { kind, paths })| {
            let artifacts = paths
                .into_iter()
                .map(|path| {
                    let state = found
                        .get(&path)
                        .map_or(ArtifactState::Absent, |r| r.state(&kind.comparison));
                    Artifact::new(path, state)
                })
                .collect();
            (name, ArtifactLink::new(kind, artifacts))
        })
        .collect()
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

fn field_value(
    field: &EntryName,
    ty: &FieldType,
    raw: &RawValue,
) -> Result<FieldValue, DocProblem> {
    match (ty, raw) {
        (FieldType::Text, RawValue::Text(s)) => Ok(FieldValue::Text(s.clone())),
        (FieldType::Integer, RawValue::Integer(n)) => Ok(FieldValue::Integer(*n)),
        (FieldType::Bool, RawValue::Bool(b)) => Ok(FieldValue::Bool(*b)),
        (FieldType::TextList, RawValue::List(items)) => Ok(FieldValue::TextList(items.clone())),
        (FieldType::Enum(words), RawValue::Text(s)) => FieldWord::new(s)
            .ok()
            .filter(|word| words.contains(word))
            .map(FieldValue::Enum)
            .ok_or_else(|| DocProblem::FieldNotInEnum {
                field: field.clone(),
                found: s.clone(),
            }),
        _ => Err(DocProblem::FieldMismatch {
            field: field.clone(),
            expected: ty.clone(),
            found: raw.kind(),
        }),
    }
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
    index: KeyIndex,
    /// Every key a file of a book writes in the book's grammar, valid file or not,
    /// identity read or not: the keys a new document cannot take, and the numbers
    /// the next key counts from.
    written_keys: BTreeSet<DocKey>,
    cycles: Vec<Cycle>,
}

impl Shelf {
    /// The paths the files write in their links to artifacts, each with the
    /// comparisons of the links that write it: what an adapter is to read before
    /// [`Shelf::build`], so that each becomes an [`Artifact`].
    ///
    /// A text that is no path is left out here (the build makes it a
    /// [`DocProblem::ArtifactPathInvalid`]); a file under no book writes none.
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
            for (name, kind) in book.artifact_links() {
                if let Some(RawValue::List(texts)) = entries.get(name.as_str()) {
                    for path in texts.iter().filter_map(|t| artifact_path(t).ok()) {
                        asks.entry(path)
                            .or_default()
                            .insert(kind.comparison.clone());
                    }
                }
            }
        }
        asks
    }

    /// A shelf from the schema, what an adapter read of the documents, and what it
    /// found at the paths [`Shelf::artifact_asks`] named.
    ///
    /// The one place a [`Doc`] is made. It never fails; see the module page for the
    /// order of the steps. A path written in a link to artifacts with no entry in
    /// `artifacts` is a path with no file.
    pub fn build(schema: &Schema, readings: Vec<DocReading>, artifacts: &ArtifactReadings) -> Self {
        let candidates = flag_duplicates(
            readings
                .into_iter()
                .map(|reading| Candidate::read(schema, reading))
                .collect(),
        );
        let keys = keys_of(&candidates);
        let written_keys = candidates
            .iter()
            .filter_map(|c| c.written_key.clone())
            .collect();
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
            .map(|(draft, written, paths)| {
                let links = resolve_links(&index, written);
                let artifact_links = resolve_artifacts(artifacts, paths);
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
            index,
            written_keys,
            cycles,
        }
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
    /// paths written in their links to artifacts.
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
mod new_tests;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod unheld_tests;
