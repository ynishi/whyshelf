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
//!      │              same id twice, same key twice in a book
//!      │           3  index of keys        (valid and readable-invalid files)
//!      │           4  links resolved       through Shelf::find_key
//!      │           5  hash                 (book, fields, body)
//!      └────────►  6  link cycles          (strongly connected components)
//! ```
//!
//! Two questions about a shelf have one entry each, and everything else asks them:
//! [`Shelf::find_key`] answers what a key in a book comes to (the build of a link and
//! the lookup of what a person typed both call it), and [`Shelf::state_of`] answers
//! what an identity comes to now (a valid document, an invalid one, gone, or not
//! knowable because some file could not even be read for its id).

use std::collections::{BTreeMap, BTreeSet};

use thiserror::Error;

use crate::domain::canon::Canon;
use crate::domain::error::ValueError;
use crate::domain::hash::{ContentHash, HashKind};
use crate::domain::id::DocId;
use crate::domain::location::RelPath;
use crate::domain::nonempty::{NonEmptyVec, TwoOrMore};

use super::doc::{Doc, DocHash, DocKey, FieldValue, LinkState, UnresolvedLink, UnresolvedWhy};
use super::schema::{
    BookName, BookSchema, Cardinality, EntryName, FieldPresence, FieldType, FieldWord, KeyForm,
    KeyProblem, Membership, Propagation, Schema,
};

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
    /// texts (a nested mapping, a floating point number).
    #[error("the value of `{key}` is of a kind a front matter does not hold")]
    UnsupportedValue {
        /// The key.
        key: String,
    },
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

/// What a key in a book comes to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KeyHit {
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

/// What [`DocState`] is when it is a failure, owned so that a refusal or a cause
/// can keep it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DocStateKind {
    /// Invalid, with every problem of every file that carries the identity.
    Invalid(NonEmptyVec<DocProblem>),
    /// Not there.
    Gone,
    /// Not knowable.
    Unknown(NonEmptyVec<RelPath>),
}

impl<'a> DocState<'a> {
    /// The valid document, or the owned form of why there is none.
    ///
    /// The one place a [`DocState`] becomes a [`DocStateKind`]; [`to_kind`] and the
    /// frames of the ledger take it from here.
    ///
    /// [`to_kind`]: Self::to_kind
    // The error is the answer (why the document is not valid), and it is not a hot path.
    #[allow(clippy::result_large_err)]
    pub fn valid_or_kind(&self) -> Result<&'a Doc, DocStateKind> {
        match self {
            Self::Valid(doc) => Ok(doc),
            Self::Invalid(docs) => {
                let head = docs.first().problems().first().clone();
                let rest = docs
                    .iter()
                    .enumerate()
                    .flat_map(|(i, d)| d.problems().iter().skip(usize::from(i == 0)).cloned())
                    .collect();
                Err(DocStateKind::Invalid(NonEmptyVec::new(head, rest)))
            }
            Self::Gone => Err(DocStateKind::Gone),
            Self::Unknown { unreadable } => Err(DocStateKind::Unknown(unreadable.clone())),
        }
    }

    /// The owned form of a state that is not valid; `None` is a valid document,
    /// which has nothing to carry.
    pub fn to_kind(&self) -> Option<DocStateKind> {
        self.valid_or_kind().err()
    }
}

/// Which graph a cycle was found in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum CycleSource {
    /// The `Depends` links the shelf holds now.
    Links,
    /// The frames of the approvals in the ledger (see
    /// [`FrameGraph`](super::ledger::FrameGraph)).
    Frames,
}

/// The identity of a cycle: where it was found and the smallest identifier in it.
///
/// The source is part of it, so that two cycles of different graphs that share a
/// smallest document are two ids.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct CycleId {
    /// The graph.
    pub source: CycleSource,
    /// The smallest identifier among the members.
    pub min: DocId,
}

/// The documents of a cycle.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CycleMembers {
    /// A document that stands on itself.
    SelfLoop(DocId),
    /// Two or more documents that stand on one another, smallest first.
    Ring(TwoOrMore<DocId>),
}

/// A cycle among documents.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cycle {
    id: CycleId,
    members: CycleMembers,
}

impl Cycle {
    /// The identity of the cycle.
    pub fn id(&self) -> CycleId {
        self.id
    }

    /// The members.
    pub fn members(&self) -> &CycleMembers {
        &self.members
    }

    /// Whether `id` is in the cycle.
    pub fn contains(&self, id: DocId) -> bool {
        self.ids().contains(&id)
    }

    /// The members, smallest first.
    pub fn ids(&self) -> Vec<DocId> {
        match &self.members {
            CycleMembers::SelfLoop(m) => vec![*m],
            CycleMembers::Ring(ms) => ms.iter().copied().collect(),
        }
    }
}

/// The strongly connected components of a directed graph, in an order where a
/// component comes after every component its members point at.
///
/// An edge points from a document to what it stands on, so the order is upstream
/// first. Both graphs of the shelf (the links, the frames of the ledger) are taken
/// apart here, once: this is the only place a cycle is searched for. The walk keeps
/// its own stack, so a long chain of documents cannot overflow the call stack.
pub(super) fn components(edges: &BTreeMap<DocId, BTreeSet<DocId>>) -> Vec<Vec<DocId>> {
    let mut nodes: BTreeSet<DocId> = edges.keys().copied().collect();
    for targets in edges.values() {
        nodes.extend(targets.iter().copied());
    }
    let ids: Vec<DocId> = nodes.into_iter().collect();
    let index_of: BTreeMap<DocId, usize> = ids.iter().enumerate().map(|(i, id)| (*id, i)).collect();
    let adjacent: Vec<Vec<usize>> = ids
        .iter()
        .map(|id| {
            edges
                .get(id)
                .map(|targets| {
                    targets
                        .iter()
                        .filter_map(|t| index_of.get(t).copied())
                        .collect()
                })
                .unwrap_or_default()
        })
        .collect();

    let n = ids.len();
    let unvisited = usize::MAX;
    let mut order = vec![unvisited; n];
    let mut low = vec![0usize; n];
    let mut on_stack = vec![false; n];
    let mut stack: Vec<usize> = Vec::new();
    let mut next = 0usize;
    let mut out: Vec<Vec<DocId>> = Vec::new();

    for root in 0..n {
        if order[root] != unvisited {
            continue;
        }
        order[root] = next;
        low[root] = next;
        next += 1;
        stack.push(root);
        on_stack[root] = true;
        let mut calls: Vec<(usize, usize)> = vec![(root, 0)];
        while let Some(&(v, i)) = calls.last() {
            if let Some(&w) = adjacent[v].get(i) {
                if let Some(top) = calls.last_mut() {
                    top.1 += 1;
                }
                if order[w] == unvisited {
                    order[w] = next;
                    low[w] = next;
                    next += 1;
                    stack.push(w);
                    on_stack[w] = true;
                    calls.push((w, 0));
                } else if on_stack[w] {
                    low[v] = low[v].min(order[w]);
                }
            } else {
                calls.pop();
                if low[v] == order[v] {
                    let mut component = Vec::new();
                    while let Some(w) = stack.pop() {
                        on_stack[w] = false;
                        component.push(ids[w]);
                        if w == v {
                            break;
                        }
                    }
                    component.sort();
                    out.push(component);
                }
                if let Some(&(parent, _)) = calls.last() {
                    low[parent] = low[parent].min(low[v]);
                }
            }
        }
    }
    out
}

/// Every cycle of a graph: each component of two or more, and each document that
/// points at itself. Ordered by identity.
pub(super) fn cycles_of(
    source: CycleSource,
    edges: &BTreeMap<DocId, BTreeSet<DocId>>,
) -> Vec<Cycle> {
    let mut cycles: Vec<Cycle> = components(edges)
        .into_iter()
        .filter_map(|component| {
            let mut ids = component.into_iter();
            let first = ids.next()?;
            let members = match ids.next() {
                Some(second) => CycleMembers::Ring(TwoOrMore::new(first, second, ids.collect())),
                None if edges.get(&first).is_some_and(|t| t.contains(&first)) => {
                    CycleMembers::SelfLoop(first)
                }
                None => return None,
            };
            Some(Cycle {
                id: CycleId { source, min: first },
                members,
            })
        })
        .collect();
    cycles.sort_by_key(Cycle::id);
    cycles
}

/// The hash of a document: its book, fields and body.
///
/// The one place the scope of a [`DocHash`] is decided and computed.
fn hash_doc(book: &BookName, fields: &BTreeMap<EntryName, FieldValue>, body: &str) -> DocHash {
    let canon = Canon::map([
        ("book", Canon::str(book.as_str())),
        (
            "fields",
            Canon::map(
                fields
                    .iter()
                    .map(|(name, value)| (name.as_str(), value.to_canon())),
            ),
        ),
        ("body", Canon::str(body)),
    ]);
    DocHash::computed(ContentHash::of(HashKind::ShelfDoc, &canon))
}

/// A file after its form is read, before the shelf looks at the files together.
struct Candidate {
    origin: RelPath,
    id: Option<DocId>,
    key: Option<DocKey>,
    problems: Vec<DocProblem>,
    fields: BTreeMap<EntryName, FieldValue>,
    written: BTreeMap<EntryName, Vec<String>>,
    body: String,
}

impl Candidate {
    fn unusable(origin: RelPath, id: Option<DocId>, problem: DocProblem) -> Self {
        Self {
            origin,
            id,
            key: None,
            problems: vec![problem],
            fields: BTreeMap::new(),
            written: BTreeMap::new(),
            body: String::new(),
        }
    }

    fn read(schema: &Schema, reading: DocReading) -> Self {
        let (origin, entries, body) = match reading {
            DocReading::Unparsed { origin, id, why } => {
                let id = match id {
                    ReadId::Read(id) => Some(id),
                    ReadId::Unreadable => None,
                };
                return Self::unusable(origin, id, DocProblem::Unreadable(why));
            }
            DocReading::Parsed {
                origin,
                entries,
                body,
            } => (origin, entries, body),
        };
        let book = match schema.membership(&origin) {
            Membership::In(name) => schema.book(&name),
            Membership::Outside => None,
        };
        let Some(book) = book else {
            let id = match entries.get("id") {
                Some(RawValue::Text(text)) => text.parse::<DocId>().ok(),
                _ => None,
            };
            return Self::unusable(origin, id, DocProblem::OutsideBooks);
        };
        Self::read_in(book, origin, &entries, body)
    }

    fn read_in(
        book: &BookSchema,
        origin: RelPath,
        entries: &BTreeMap<String, RawValue>,
        body: String,
    ) -> Self {
        let mut problems = Vec::new();

        let id = match entries.get("id") {
            None => {
                problems.push(DocProblem::IdMissing);
                None
            }
            Some(RawValue::Text(text)) => match text.parse::<DocId>() {
                Ok(id) => Some(id),
                Err(e) => {
                    problems.push(DocProblem::IdInvalid(e));
                    None
                }
            },
            Some(_) => {
                problems.push(DocProblem::IdNotText);
                None
            }
        };
        let key = match entries.get("key") {
            None => {
                problems.push(DocProblem::KeyMissing);
                None
            }
            Some(RawValue::Text(text)) => match book.key().parse(text) {
                Ok(form) => Some(DocKey {
                    book: book.name().clone(),
                    form,
                }),
                Err(e) => {
                    problems.push(DocProblem::KeyInvalid(e));
                    None
                }
            },
            Some(_) => {
                problems.push(DocProblem::KeyNotText);
                None
            }
        };

        let mut fields = BTreeMap::new();
        let mut written = BTreeMap::new();
        for (name, raw) in entries {
            if name == "id" || name == "key" {
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
            } else {
                problems.push(DocProblem::UnknownEntry { name: name.clone() });
            }
        }
        for (field, decl) in book.fields() {
            if decl.presence == FieldPresence::Required && !entries.contains_key(field.as_str()) {
                problems.push(DocProblem::FieldMissing {
                    field: field.clone(),
                });
            }
        }
        for (link, kind) in book.links() {
            if !entries.contains_key(link.as_str()) && !kind.cardinality.admits(0) {
                problems.push(DocProblem::LinkCount {
                    link: link.clone(),
                    cardinality: kind.cardinality,
                    found: 0,
                });
            }
        }

        Self {
            origin,
            id,
            key,
            problems,
            fields,
            written,
            body,
        }
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
/// A shelf keeps the [`Schema`] it was built with, because what a link means and
/// what a key may be are asked of it later (by the lookup, by freshness).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Shelf {
    schema: Schema,
    docs: BTreeMap<DocId, Doc>,
    invalid: BTreeMap<DocId, NonEmptyVec<InvalidDoc>>,
    unreadable: Vec<InvalidDoc>,
    keys: BTreeMap<DocKey, BTreeSet<DocId>>,
    cycles: Vec<Cycle>,
}

impl Shelf {
    /// A shelf from the schema and what an adapter read.
    ///
    /// The one place a [`Doc`] is made. It never fails; see the module page for the
    /// order of the steps.
    pub fn build(schema: &Schema, readings: Vec<DocReading>) -> Self {
        let candidates: Vec<Candidate> = readings
            .into_iter()
            .map(|reading| Candidate::read(schema, reading))
            .collect();
        let candidates = flag_duplicates(candidates);

        let mut docs = BTreeMap::new();
        let mut pending: Vec<(DocId, BookName, BTreeMap<EntryName, Vec<String>>)> = Vec::new();
        let mut invalid: BTreeMap<DocId, Vec<InvalidDoc>> = BTreeMap::new();
        let mut unreadable = Vec::new();
        let mut keys: BTreeMap<DocKey, BTreeSet<DocId>> = BTreeMap::new();

        for candidate in candidates {
            if let (Some(id), Some(key)) = (candidate.id, &candidate.key) {
                keys.entry(key.clone()).or_default().insert(id);
            }
            match (candidate.problems.is_empty(), candidate.id, candidate.key) {
                (true, Some(id), Some(key)) => {
                    let hash = hash_doc(&key.book, &candidate.fields, &candidate.body);
                    pending.push((id, key.book.clone(), candidate.written));
                    docs.insert(
                        id,
                        Doc::new(
                            id,
                            key,
                            candidate.origin,
                            candidate.fields,
                            candidate.body,
                            hash,
                        ),
                    );
                }
                (_, id, _) => {
                    let mut problems = candidate.problems.into_iter();
                    let Some(head) = problems.next() else {
                        // A candidate without problems has an id and a key: the arm
                        // above took it. Nothing reaches here.
                        continue;
                    };
                    let entry = InvalidDoc {
                        origin: candidate.origin,
                        id,
                        problems: NonEmptyVec::new(head, problems.collect()),
                    };
                    match id {
                        Some(id) => invalid.entry(id).or_default().push(entry),
                        None => unreadable.push(entry),
                    }
                }
            }
        }

        let invalid = invalid
            .into_iter()
            .filter_map(|(id, docs)| NonEmptyVec::from_vec(docs).ok().map(|docs| (id, docs)))
            .collect();

        let mut shelf = Self {
            schema: schema.clone(),
            docs,
            invalid,
            unreadable,
            keys,
            cycles: Vec::new(),
        };

        let resolved: Vec<(DocId, BTreeMap<EntryName, Vec<LinkState>>)> = pending
            .into_iter()
            .map(|(id, book, written)| (id, shelf.resolve_links(&book, &written)))
            .collect();
        for (id, links) in resolved {
            if let Some(doc) = shelf.docs.get_mut(&id) {
                doc.set_links(links);
            }
        }

        let mut edges: BTreeMap<DocId, BTreeSet<DocId>> = BTreeMap::new();
        for doc in shelf.docs.values() {
            for (_, states) in shelf.links_of(doc, Propagation::Depends) {
                for state in states {
                    if let LinkState::Resolved { target } = state {
                        edges.entry(doc.id()).or_default().insert(*target);
                    }
                }
            }
        }
        shelf.cycles = cycles_of(CycleSource::Links, &edges);
        shelf
    }

    fn resolve_links(
        &self,
        book: &BookName,
        written: &BTreeMap<EntryName, Vec<String>>,
    ) -> BTreeMap<EntryName, Vec<LinkState>> {
        let Some(book) = self.schema.book(book) else {
            return BTreeMap::new();
        };
        book.links()
            .iter()
            .map(|(name, kind)| {
                let states = written
                    .get(name)
                    .map(|keys| {
                        keys.iter()
                            .map(|text| self.link_state(&kind.target, text))
                            .collect()
                    })
                    .unwrap_or_default();
                (name.clone(), states)
            })
            .collect()
    }

    fn link_state(&self, target_book: &BookName, written: &str) -> LinkState {
        let unresolved = |why| LinkState::Unresolved(UnresolvedLink::new(written.to_owned(), why));
        let Some(book) = self.schema.book(target_book) else {
            return unresolved(UnresolvedWhy::Missing);
        };
        let form = match book.key().parse(written) {
            Ok(form) => form,
            Err(problem) => return unresolved(UnresolvedWhy::Malformed(problem)),
        };
        match self.find_key(target_book, &form) {
            KeyHit::Valid(target) => LinkState::Resolved { target },
            KeyHit::Invalid(target) => unresolved(UnresolvedWhy::TargetInvalid(target)),
            KeyHit::Ambiguous(targets) => unresolved(UnresolvedWhy::Ambiguous(targets)),
            KeyHit::Missing => unresolved(UnresolvedWhy::Missing),
        }
    }

    /// What a key in a book comes to.
    ///
    /// The one place a key is looked up in a book: the build of a link and the
    /// lookup of what a person typed both call it. More than one document with the
    /// key is [`Ambiguous`](KeyHit::Ambiguous) whether they are valid or not (a key
    /// written twice makes every holder invalid).
    pub fn find_key(&self, book: &BookName, form: &KeyForm) -> KeyHit {
        let key = DocKey {
            book: book.clone(),
            form: form.clone(),
        };
        let mut ids = self.keys.get(&key).into_iter().flatten().copied();
        match (ids.next(), ids.next()) {
            (None, _) => KeyHit::Missing,
            (Some(id), None) if self.docs.contains_key(&id) => KeyHit::Valid(id),
            (Some(id), None) => KeyHit::Invalid(id),
            (Some(first), Some(second)) => {
                KeyHit::Ambiguous(TwoOrMore::new(first, second, ids.collect()))
            }
        }
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
                self.links_of(doc, Propagation::Replaces)
                    .into_iter()
                    .flat_map(|(_, states)| states)
                    .any(|state| matches!(state, LinkState::Resolved { target } if *target == id))
            })
            .map(Doc::id)
            .collect()
    }

    /// The links of a document that have a meaning for freshness, by name.
    ///
    /// The one place the schema's `Propagation` of a link is looked up for a
    /// document. A name the schema no longer declares has no entry.
    pub fn links_of<'d>(
        &self,
        doc: &'d Doc,
        propagation: Propagation,
    ) -> Vec<(&'d EntryName, &'d [LinkState])> {
        let Some(book) = self.schema.book(&doc.key().book) else {
            return Vec::new();
        };
        doc.links()
            .iter()
            .filter(|(name, _)| {
                book.links()
                    .get(*name)
                    .is_some_and(|kind| kind.propagation == propagation)
            })
            .map(|(name, states)| (name, states.as_slice()))
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

    /// The invalid files whose identity could not be read.
    pub fn unreadable(&self) -> &[InvalidDoc] {
        &self.unreadable
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

/// Marks as invalid every file that shares its `id:` with another, and every file
/// of a book that shares its key with another file of the book.
fn flag_duplicates(mut candidates: Vec<Candidate>) -> Vec<Candidate> {
    let mut by_id: BTreeMap<DocId, Vec<usize>> = BTreeMap::new();
    let mut by_key: BTreeMap<DocKey, Vec<usize>> = BTreeMap::new();
    for (i, c) in candidates.iter().enumerate() {
        if let Some(id) = c.id {
            by_id.entry(id).or_default().push(i);
            if let Some(key) = &c.key {
                by_key.entry(key.clone()).or_default().push(i);
            }
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
mod tests;
