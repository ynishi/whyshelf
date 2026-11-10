//! A document of a shelf, and the parts of it.
//!
//! These are types only. A [`Doc`] is put together by [`Shelf::build`] and by
//! nothing else: its fields are private and its constructor is visible to the
//! shelf's own modules alone. The same goes for the two values that are *computed*
//! from a document and then *kept* by a record of the ledger, [`DocHash`] and
//! [`FramePin`]: each has one name for the path that computes it (`computed`,
//! `taken`) and another for the path that restores it from a record (`restore`), so
//! that a reader of the code sees which of the two a call is. Visibility cannot
//! tell them apart, so the rule is written in the table of the
//! [module page](super#who-decides-what): `computed` and `taken` are called where
//! a document is read and where an approval is framed, `restore` only where a
//! record is read back.
//!
//! The grammar of a key ([`KeyForm`] and its parts) is in [`schema`](super::schema),
//! because the pattern that makes a form is part of the declaration of a book.
//!
//! [`Shelf::build`]: super::build::Shelf::build

use std::collections::BTreeMap;
use std::fmt;

use crate::domain::canon::Canon;
use crate::domain::hash::ContentHash;
use crate::domain::id::DocId;
use crate::domain::location::RelPath;
use crate::domain::nonempty::TwoOrMore;

use super::schema::{BookName, EntryName, FieldWord, KeyForm, KeyProblem};

/// The key of a document: its book, and its key in the grammar of that book.
///
/// A key is unique within a book, not across books; `spec:FR-1` and `design:FR-1`
/// are two keys.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct DocKey {
    /// The book.
    pub book: BookName,
    /// The key inside it.
    pub form: KeyForm,
}

impl fmt::Display for DocKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}", self.book, self.form)
    }
}

/// The value of a field, checked against the type the book declares for it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FieldValue {
    /// Free text.
    Text(String),
    /// A whole number.
    Integer(i64),
    /// A truth value.
    Bool(bool),
    /// A list of texts.
    TextList(Vec<String>),
    /// A word of the enum the book declares.
    Enum(FieldWord),
}

impl FieldValue {
    /// The canonical form of a value, tagged with its type, so that a field that
    /// changes type changes a hash even when the text stays.
    pub(super) fn to_canon(&self) -> Canon {
        let (ty, value) = match self {
            Self::Text(s) => ("text", Canon::str(s.as_str())),
            Self::Integer(n) => ("integer", Canon::Int(*n)),
            Self::Bool(b) => ("bool", Canon::Bool(*b)),
            Self::TextList(items) => (
                "text-list",
                Canon::list(items.iter().map(|s| Canon::str(s.as_str()))),
            ),
            Self::Enum(word) => ("enum", Canon::str(word.as_str())),
        };
        Canon::map([("type", Canon::str(ty)), ("value", value)])
    }
}

/// Why a link could not be followed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UnresolvedWhy {
    /// The text is not a key in the grammar of the book the link points into.
    Malformed(KeyProblem),
    /// No document of that book has the key.
    Missing,
    /// More than one document of that book has the key.
    Ambiguous(TwoOrMore<DocId>),
    /// The one document with the key is invalid, so it cannot be stood on.
    TargetInvalid(DocId),
}

/// One link that could not be followed: what was written, and why.
///
/// [`LinkState`], the refusal of a frame, a changed set of links and a notice all
/// carry this one struct, so that none of them spreads it over fields of its own.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnresolvedLink {
    written: String,
    why: UnresolvedWhy,
}

impl UnresolvedLink {
    pub(super) fn new(written: String, why: UnresolvedWhy) -> Self {
        Self { written, why }
    }

    /// The key as the document wrote it.
    pub fn written(&self) -> &str {
        &self.written
    }

    /// Why it could not be followed.
    pub fn why(&self) -> &UnresolvedWhy {
        &self.why
    }
}

/// What one link of a document comes to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LinkState {
    /// The link names a valid document.
    Resolved {
        /// The document it names.
        target: DocId,
    },
    /// The link names nothing that can be stood on.
    Unresolved(UnresolvedLink),
}

/// The hash of a document, which says whether it changed.
///
/// It covers the name of the book, the fields and the body. It does not cover the
/// key, the file's place or the links: renaming a key or moving a file does not
/// make an approval stale, and a link's target is read through the shelf, so
/// counting it here would let an upstream's rename leak into the downstream's own
/// change. A change of a link is told by comparing with the frame of an approval
/// instead (see [`freshness`](super::freshness)). A document that moves to another
/// book does change, because the book is in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct DocHash(ContentHash);

impl DocHash {
    /// The hash a shelf computed for a document it read.
    pub(super) fn computed(hash: ContentHash) -> Self {
        Self(hash)
    }

    /// The hash a record of the ledger kept.
    pub(super) fn restore(hash: ContentHash) -> Self {
        Self(hash)
    }

    /// The digest.
    pub fn hash(&self) -> &ContentHash {
        &self.0
    }
}

impl fmt::Display for DocHash {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(&self.0, f)
    }
}

/// What an approval stood on for one link: the upstream it named, the key it had
/// then and the hash it had then.
///
/// The key is kept so that an upstream that has since gone can be told from a link
/// that is written wrong now: both are a miss in the shelf, and only the key says
/// whether they are the same one. It is not part of any hash.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct FramePin {
    link: EntryName,
    target: DocId,
    key: DocKey,
    hash: DocHash,
}

impl FramePin {
    /// The pin an approval takes of an upstream it stands on.
    pub(super) fn taken(link: EntryName, target: DocId, key: DocKey, hash: DocHash) -> Self {
        Self {
            link,
            target,
            key,
            hash,
        }
    }

    /// The pin a record of the ledger kept.
    pub(super) fn restore(link: EntryName, target: DocId, key: DocKey, hash: DocHash) -> Self {
        Self {
            link,
            target,
            key,
            hash,
        }
    }

    /// The link of the approved document the pin was taken for.
    pub fn link(&self) -> &EntryName {
        &self.link
    }

    /// The upstream.
    pub fn target(&self) -> DocId {
        self.target
    }

    /// The key the upstream had when it was pinned.
    pub fn key(&self) -> &DocKey {
        &self.key
    }

    /// The hash the upstream had when it was pinned.
    pub fn hash(&self) -> &DocHash {
        &self.hash
    }
}

/// A valid document.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Doc {
    id: DocId,
    key: DocKey,
    origin: RelPath,
    fields: BTreeMap<EntryName, FieldValue>,
    links: BTreeMap<EntryName, Vec<LinkState>>,
    body: String,
    hash: DocHash,
}

impl Doc {
    pub(super) fn new(
        id: DocId,
        key: DocKey,
        origin: RelPath,
        fields: BTreeMap<EntryName, FieldValue>,
        body: String,
        hash: DocHash,
    ) -> Self {
        Self {
            id,
            key,
            origin,
            fields,
            links: BTreeMap::new(),
            body,
            hash,
        }
    }

    pub(super) fn set_links(&mut self, links: BTreeMap<EntryName, Vec<LinkState>>) {
        self.links = links;
    }

    /// The identity of the document.
    pub fn id(&self) -> DocId {
        self.id
    }

    /// The key of the document.
    pub fn key(&self) -> &DocKey {
        &self.key
    }

    /// The file the document was read from.
    pub fn origin(&self) -> &RelPath {
        &self.origin
    }

    /// The fields, by name.
    pub fn fields(&self) -> &BTreeMap<EntryName, FieldValue> {
        &self.fields
    }

    /// The links, by name: every link the book declares is there, with no entry in
    /// its list when the document wrote none.
    pub fn links(&self) -> &BTreeMap<EntryName, Vec<LinkState>> {
        &self.links
    }

    /// The body, as the adapter normalized it.
    pub fn body(&self) -> &str {
        &self.body
    }

    /// The hash of the document.
    pub fn hash(&self) -> &DocHash {
        &self.hash
    }
}
