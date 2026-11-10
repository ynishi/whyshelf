//! A document of a shelf, and the parts of it.
//!
//! A [`Doc`] is put together by [`Shelf::build`] and by nothing else: its fields are
//! private and its constructors are visible to the shelf's own modules alone. It is
//! made once, whole, in two steps the build needs: `DocDraft::new` computes the
//! [`DocHash`] from what was read (`hash_doc`, private to this module, is the one
//! place the scope of a hash is decided), and `Doc::new` takes the draft with every
//! link the book declares, already resolved. A resolved link carries the hash of its
//! target, which is why every hash is known before any document is made.
//!
//! A [`DocHash`] is computed here and *kept* by a record of the ledger, so it has two
//! doors: `computed`, private to this module, and `restore`, for the path that reads
//! it back from a record. Visibility cannot keep `restore` to that path (the ledger
//! is another module of the shelf), so the rule is written in the table of the
//! [module page](super#who-decides-what): `restore` only where a record is read back.
//!
//! The grammar of a key ([`KeyForm`] and its parts) is in [`schema`](super::schema),
//! because the pattern that makes a form is part of the declaration of a book.
//!
//! [`Shelf::build`]: super::build::Shelf::build

use std::collections::BTreeMap;
use std::fmt;

use crate::domain::canon::Canon;
use crate::domain::hash::{ContentHash, HashKind};
use crate::domain::id::DocId;
use crate::domain::location::RelPath;
use crate::domain::nonempty::TwoOrMore;

use super::schema::{BookName, EntryName, FieldWord, KeyForm, KeyProblem, LinkKind, Propagation};

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

/// One link that could not be followed: what was written, the book it points into,
/// the key it reads as there (when it reads as one), and why.
///
/// [`LinkState`], the refusal of a frame, a changed set of links and a notice all
/// carry this one struct, so that none of them spreads it over fields of its own.
/// The build reads the written text once, in the grammar of the book the link points
/// into; what it read is kept here, so nothing reads the text a second time.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnresolvedLink {
    written: String,
    book: BookName,
    key: Option<DocKey>,
    why: UnresolvedWhy,
}

impl UnresolvedLink {
    /// A link the build could not follow. The key is made here from the book and the
    /// form the text read as, so its book is the link's.
    pub(super) fn new(
        written: String,
        book: BookName,
        form: Option<KeyForm>,
        why: UnresolvedWhy,
    ) -> Self {
        let key = form.map(|form| DocKey {
            book: book.clone(),
            form,
        });
        Self {
            written,
            book,
            key,
            why,
        }
    }

    /// The key as the document wrote it.
    pub fn written(&self) -> &str {
        &self.written
    }

    /// The book the link points into.
    pub fn book(&self) -> &BookName {
        &self.book
    }

    /// The key the text reads as in that book; none when it is no key there
    /// ([`UnresolvedWhy::Malformed`]).
    pub fn key(&self) -> Option<&DocKey> {
        self.key.as_ref()
    }

    /// Why it could not be followed.
    pub fn why(&self) -> &UnresolvedWhy {
        &self.why
    }
}

/// A link that names a valid document: its identity, and the key and hash it has
/// now.
///
/// Made by the build only, from the document the key names, so what it says is what
/// the shelf has: an approval pins it as it is, with no second lookup.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedLink {
    id: DocId,
    key: DocKey,
    hash: DocHash,
}

impl ResolvedLink {
    pub(super) fn new(id: DocId, key: DocKey, hash: DocHash) -> Self {
        Self { id, key, hash }
    }

    /// The document it names.
    pub fn id(&self) -> DocId {
        self.id
    }

    /// The key of that document.
    pub fn key(&self) -> &DocKey {
        &self.key
    }

    /// The hash of that document now.
    pub fn hash(&self) -> &DocHash {
        &self.hash
    }
}

/// What one link of a document comes to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LinkState {
    /// The link names a valid document.
    Resolved(ResolvedLink),
    /// The link names nothing that can be stood on.
    Unresolved(UnresolvedLink),
}

/// One link a book declares, as a document holds it: what kind of link it is, and
/// what each key written in it comes to (none when the document wrote none).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Link {
    kind: LinkKind,
    states: Vec<LinkState>,
}

impl Link {
    pub(super) fn new(kind: LinkKind, states: Vec<LinkState>) -> Self {
        Self { kind, states }
    }

    /// The kind the book declares: the book it points into, how many keys it holds,
    /// and what it means for freshness.
    pub fn kind(&self) -> &LinkKind {
        &self.kind
    }

    /// What each key written in it comes to, in the order written.
    pub fn states(&self) -> &[LinkState] {
        &self.states
    }
}

/// The hash of a document, which says whether it changed.
///
/// It covers the name of the book, the fields and the body. It does not cover the
/// key, the file's place or the links: renaming a key or moving a file does not
/// make an approval stale, and what a link comes to (the key and the hash of the
/// document it names) is another document's, so counting it here would let an
/// upstream's rename or edit leak into the downstream's own change. A change of a
/// link is told by comparing with the frame of an approval instead (see
/// [`freshness`](super::freshness)). A document that moves to another book does
/// change, because the book is in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct DocHash(ContentHash);

impl DocHash {
    /// The hash `hash_doc` computed for a document; private to this module, so the
    /// draft of a [`Doc`] is the only thing that computes one.
    fn computed(hash: ContentHash) -> Self {
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

/// A valid document.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Doc {
    id: DocId,
    key: DocKey,
    origin: RelPath,
    fields: BTreeMap<EntryName, FieldValue>,
    links: BTreeMap<EntryName, Link>,
    body: String,
    hash: DocHash,
}

/// A valid document before its links are resolved: what the build read of it, and
/// the hash computed from that.
///
/// The build needs the hash of every valid document before it makes any, because a
/// resolved link carries the hash of its target; so the hash is computed here, once,
/// and [`Doc::new`] takes it with the rest.
pub(super) struct DocDraft {
    id: DocId,
    key: DocKey,
    origin: RelPath,
    fields: BTreeMap<EntryName, FieldValue>,
    body: String,
    hash: DocHash,
}

impl DocDraft {
    /// A draft of a valid document; its hash is computed from the book, the fields
    /// and the body, so no caller can hand in one that does not belong to it.
    pub(super) fn new(
        id: DocId,
        key: DocKey,
        origin: RelPath,
        fields: BTreeMap<EntryName, FieldValue>,
        body: String,
    ) -> Self {
        let hash = hash_doc(&key.book, &fields, &body);
        Self {
            id,
            key,
            origin,
            fields,
            body,
            hash,
        }
    }

    /// The identity.
    pub(super) fn id(&self) -> DocId {
        self.id
    }

    /// The hash.
    pub(super) fn hash(&self) -> &DocHash {
        &self.hash
    }
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

impl Doc {
    /// A document, whole: the draft (with the hash computed from it) and every link
    /// the book declares, already resolved. The one place a `Doc` is made.
    pub(super) fn new(draft: DocDraft, links: BTreeMap<EntryName, Link>) -> Self {
        let DocDraft {
            id,
            key,
            origin,
            fields,
            body,
            hash,
        } = draft;
        Self {
            id,
            key,
            origin,
            fields,
            links,
            body,
            hash,
        }
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

    /// The links, by name: every link the book declares is there, with its kind, and
    /// with no state in it when the document wrote none.
    pub fn links(&self) -> &BTreeMap<EntryName, Link> {
        &self.links
    }

    /// The links of one meaning for freshness, by name.
    ///
    /// The one place a document's links are sorted by their [`Propagation`]: each
    /// link carries its kind, so no schema is asked.
    pub fn links_of(&self, propagation: Propagation) -> Vec<(&EntryName, &[LinkState])> {
        self.links
            .iter()
            .filter(|(_, link)| link.kind.propagation == propagation)
            .map(|(name, link)| (name, link.states()))
            .collect()
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
