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
use crate::domain::nonempty::{NonBlank, NonEmptyVec, OneLine, TwoOrMore};
use crate::domain::words::{Join, Mark, listed};

use super::artifact::{Artifact, ArtifactKey, ArtifactSeen, ArtifactState, Comparison, Untold};
use super::schema::{
    ArtifactLinkKind, ArtifactPropagation, BookName, EntryName, FieldType, FieldWord, KeyForm,
    KeyProblem, LinkKind, Propagation, ScalarType,
};

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

/// A document as a message names it: by its key, else by its identity.
///
/// A document may have no key a message can know (one that is invalid has none, and
/// one that is gone has none to read), so a message that names a document that may
/// be either of these says it through this value, and so says it one way. A name that
/// is always a key (the document a link resolved to) is a [`DocKey`], not this.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DocLabel {
    id: DocId,
    key: Option<DocKey>,
}

impl DocLabel {
    /// The document `id`, with its key when it has one.
    pub fn new(id: DocId, key: Option<DocKey>) -> Self {
        Self { id, key }
    }

    /// The identity of the document.
    pub fn id(&self) -> DocId {
        self.id
    }

    /// The key of the document, when it has one.
    pub fn key(&self) -> Option<&DocKey> {
        self.key.as_ref()
    }
}

impl fmt::Display for DocLabel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.key {
            Some(key) => fmt::Display::fmt(key, f),
            None => fmt::Display::fmt(&self.id, f),
        }
    }
}

/// The value of a field, checked against the type the book declares for it.
///
/// A value says something: a document that writes nothing for a field (no entry, a
/// null, a text that is blank, a list with no item) has no value of it, so a text is
/// never blank and a list is never empty here, and a reader of a value does not check
/// that again.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FieldValue {
    /// Free text, of any number of lines, not blank.
    Text(NonBlank),
    /// A text of one line, not blank (a field declared `line`).
    Line(OneLine),
    /// A whole number.
    Integer(i64),
    /// A truth value.
    Bool(bool),
    /// A list of texts, with at least one item, none of them blank.
    TextList(NonEmptyVec<NonBlank>),
    /// A word of the enum the book declares.
    Enum(FieldWord),
}

impl FieldValue {
    /// One text as the value of a field of the type `ty`, or none when it is not
    /// one: a whole number for `Integer`, `true` or `false` for `Bool`, a listed word
    /// for `Enum`, the text itself for `Text`, the text itself for `Line` when it has
    /// no line break ([`OneLine::new`]). A `TextList` takes no single text (its items
    /// are read one by one), so it is none.
    ///
    /// The one place a text is read as the value of a type: a caller that must tell
    /// a text of a type from one that is not (the typing of what a person gave, the
    /// check of an enum word, the filter of `status`) asks here and decides only
    /// what a miss means to it. It never judges whether the text says something: the
    /// text is a [`NonBlank`] already, which whoever has the text made once.
    pub fn read(ty: &FieldType, text: &NonBlank) -> Option<FieldValue> {
        match ty {
            FieldType::Scalar(ScalarType::Text) => Some(Self::Text(text.clone())),
            FieldType::Scalar(ScalarType::Line) => OneLine::new(text.as_str()).ok().map(Self::Line),
            FieldType::Scalar(ScalarType::Integer) => text.as_str().parse().ok().map(Self::Integer),
            FieldType::Scalar(ScalarType::Bool) => match text.as_str() {
                "true" => Some(Self::Bool(true)),
                "false" => Some(Self::Bool(false)),
                _ => None,
            },
            FieldType::Scalar(ScalarType::TextList) => None,
            FieldType::Enum(words) => FieldWord::new(text.as_str())
                .ok()
                .filter(|word| words.contains(word))
                .map(Self::Enum),
        }
    }

    /// The canonical form of a value, tagged with its type, so that a field that
    /// changes type changes a hash even when the text stays. A `Line` is tagged
    /// `text`: the declaration became stricter, the content did not, so changing a
    /// field from `text` to `line` does not change the hash of a document.
    pub(super) fn to_canon(&self) -> Canon {
        let (ty, value) = match self {
            Self::Text(s) => ("text", Canon::str(s.as_str())),
            Self::Line(s) => ("text", Canon::str(s.as_str())),
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

/// A file that writes a key: the file, and the identity it carries when that could be
/// read.
///
/// A file writes a key when its `key:` reads in the grammar of its book, whether or
/// not the file is a valid document and whether or not its `id:` could be read: what
/// makes a key taken is the file, not the document. It is the leaf of the one reading
/// of who writes a key (`Shelf::key_writers`), which is why it lives here and not in
/// the build: an [`UnresolvedWhy`] holds it.
///
/// Made by the build only.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeyWriter {
    file: RelPath,
    id: Option<DocId>,
}

impl KeyWriter {
    pub(in crate::domain::shelf) fn new(file: RelPath, id: Option<DocId>) -> Self {
        Self { file, id }
    }

    /// The file.
    pub fn file(&self) -> &RelPath {
        &self.file
    }

    /// The identity the file carries, when it could be read.
    pub fn id(&self) -> Option<DocId> {
        self.id
    }
}

/// A key that two or more files write: its writers, in the order of their files.
///
/// Every file of a collision is invalid (each carries a
/// [`DocProblem::DuplicateKey`](super::build::DocProblem::DuplicateKey) that names
/// the others), so a collision says which files to look at, not which to keep: which
/// of them is to take another key is for a person to say.
///
/// Made by the build only.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeyCollision {
    key: DocKey,
    writers: TwoOrMore<KeyWriter>,
}

impl KeyCollision {
    pub(in crate::domain::shelf) fn new(key: DocKey, writers: TwoOrMore<KeyWriter>) -> Self {
        Self { key, writers }
    }

    /// The key.
    pub fn key(&self) -> &DocKey {
        &self.key
    }

    /// The files that write it, in the order of their paths.
    pub fn writers(&self) -> &TwoOrMore<KeyWriter> {
        &self.writers
    }

    /// The files that write it, in the order of their paths.
    pub fn files(&self) -> impl Iterator<Item = &RelPath> {
        self.writers.iter().map(KeyWriter::file)
    }

    /// The files that write it, as a list that cannot be empty.
    pub fn file_list(&self) -> NonEmptyVec<RelPath> {
        NonEmptyVec::from(self.writers.clone()).map(|writer| writer.file().clone())
    }

    /// The identities of the files whose identity could be read, each once, in the
    /// order of the identities.
    pub fn ids(&self) -> std::collections::BTreeSet<DocId> {
        self.writers.iter().filter_map(KeyWriter::id).collect()
    }
}

/// Why a link could not be followed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UnresolvedWhy {
    /// The text is not a key in the grammar of the book the link points into.
    Malformed(KeyProblem),
    /// No document of that book has the key.
    Missing,
    /// Two or more files of that book write the key, valid or not, whether or not
    /// their identities could be read. (Boxed: the collision names every file, and
    /// the other reasons are small.)
    Ambiguous(Box<KeyCollision>),
    /// The one file of that book that writes the key carries no identity that could
    /// be read, so no document is known to have the key.
    NoIdentity(RelPath),
    /// The one document with the key is invalid, so it cannot be stood on.
    TargetInvalid(DocId),
}

/// Why a document a `Depends` link names is one the link cannot be followed to: the
/// reasons of [`UnresolvedWhy`] that name a document, by the one rule of which
/// documents a link names, which is the [`Doc`]'s.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Unfollowed {
    /// It is the one document of the key, and it is invalid
    /// ([`UnresolvedWhy::TargetInvalid`]).
    TargetInvalid,
    /// It is of a file that writes a key two or more files write
    /// ([`UnresolvedWhy::Ambiguous`]).
    Ambiguous,
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

/// What a link that could not be followed comes to, as a sentence, which is the one
/// way it is told (by the refusal of a frame as by `check` and `stale`):
///
/// | why | the sentence |
/// |---|---|
/// | [`UnresolvedWhy::Malformed`] | `` `W` cannot be followed: not a key: <problem> `` |
/// | [`UnresolvedWhy::Missing`] | `` `W` cannot be followed: no document has this key `` |
/// | [`UnresolvedWhy::Ambiguous`] | `` `W` cannot be followed: the key `K` is written by 2 files: <file> and <file> `` |
/// | [`UnresolvedWhy::NoIdentity`] | `` `W` cannot be followed: the file <file> writes the key `K` and carries no identity that can be read `` |
/// | [`UnresolvedWhy::TargetInvalid`] | `` `W` cannot be followed: the document it names is invalid (K) `` |
///
/// The documents that share a key are told apart by their files, since their key is
/// the same and the identity of one of them may not be readable. The one that is
/// invalid is told by the key the link read as, or by its identity when the link read
/// as no key.
impl fmt::Display for UnresolvedLink {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "`{}` cannot be followed: ", self.written)?;
        match &self.why {
            UnresolvedWhy::Malformed(problem) => write!(f, "not a key: {problem}"),
            UnresolvedWhy::Missing => f.write_str("no document has this key"),
            UnresolvedWhy::Ambiguous(collision) => write!(
                f,
                "the key `{}` is written by {} files: {}",
                self.said_key(),
                collision.writers().len(),
                listed(collision.files(), Mark::Plain, Join::And)
            ),
            UnresolvedWhy::NoIdentity(file) => write!(
                f,
                "the file {file} writes the key `{}` and carries no identity that can be read",
                self.said_key()
            ),
            UnresolvedWhy::TargetInvalid(id) => {
                let label = DocLabel::new(*id, self.key.clone());
                write!(f, "the document it names is invalid ({label})")
            }
        }
    }
}

impl UnresolvedLink {
    /// The key the text reads as, or the text as written when it reads as none.
    fn said_key(&self) -> String {
        self.key
            .as_ref()
            .map_or_else(|| self.written.clone(), ToString::to_string)
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

/// What one key of a link names, as [`Doc::links_named`] reads it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LinkNamed<'a> {
    /// The valid document it resolves to.
    Doc(DocId),
    /// A key that resolves to no document, as written.
    Written(&'a str),
    /// An artifact.
    Artifact(&'a ArtifactKey),
}

/// What one link of a document comes to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LinkState {
    /// The link names a valid document.
    Resolved(ResolvedLink),
    /// The link names nothing that can be stood on.
    Unresolved(UnresolvedLink),
}

/// What one key a `Depends` link writes names.
enum Named<'a> {
    /// The valid document it resolves to.
    Resolved(DocId),
    /// The documents it names and cannot be followed to, by why.
    Unfollowed(Vec<DocId>, Unfollowed),
    /// A key that is no document's.
    Unnamed(&'a DocKey),
    /// Nothing: a text that is no key.
    Nothing,
}

/// What `state` names: the one match over why a link cannot be followed that
/// [`Doc::depends_names`], [`Doc::unfollowed_names`] and [`Doc::unnamed_keys`] read.
fn named(state: &LinkState) -> Named<'_> {
    match state {
        LinkState::Resolved(target) => Named::Resolved(target.id()),
        LinkState::Unresolved(link) => match link.why() {
            UnresolvedWhy::TargetInvalid(id) => {
                Named::Unfollowed(vec![*id], Unfollowed::TargetInvalid)
            }
            UnresolvedWhy::Ambiguous(collision) => {
                let ids: Vec<DocId> = collision.ids().into_iter().collect();
                if ids.is_empty() {
                    // Only files with no identity that could be read write the key: as
                    // with one such file, no document has it.
                    link.key().map_or(Named::Nothing, Named::Unnamed)
                } else {
                    Named::Unfollowed(ids, Unfollowed::Ambiguous)
                }
            }
            UnresolvedWhy::Missing | UnresolvedWhy::NoIdentity(_) => {
                link.key().map_or(Named::Nothing, Named::Unnamed)
            }
            UnresolvedWhy::Malformed(_) => Named::Nothing,
        },
    }
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

/// One link to artifacts a book declares, as a document holds it: what kind of link
/// it is, and the artifact each name written in it comes to (none when the document
/// wrote none).
///
/// Every name is an [`Artifact`], there, absent or unknown (rule A1): there is no
/// "unresolved" artifact, because a thing not written yet is still the evidence a
/// document names. A text that is no name of the space makes the document invalid
/// instead.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArtifactLink {
    kind: ArtifactLinkKind,
    artifacts: Vec<Artifact>,
}

impl ArtifactLink {
    pub(super) fn new(kind: ArtifactLinkKind, artifacts: Vec<Artifact>) -> Self {
        Self { kind, artifacts }
    }

    /// The kind the book declares: its space, how many names it holds and what it
    /// means for freshness.
    pub fn kind(&self) -> &ArtifactLinkKind {
        &self.kind
    }

    /// The artifact each name written in it comes to, in the order written.
    pub fn artifacts(&self) -> &[Artifact] {
        &self.artifacts
    }
}

/// What the names a document's `Depends` links to artifacts write come to now, each
/// under its link ([`Doc::depends_now`]).
#[derive(Debug, Clone)]
pub(in crate::domain::shelf) struct DependsNow<'d> {
    names: BTreeMap<(&'d EntryName, &'d ArtifactKey), PinnedNow<'d>>,
}

impl<'d> DependsNow<'d> {
    /// What the link `link` writing `name` comes to now; none when it does not write it.
    pub(in crate::domain::shelf) fn get(
        &self,
        link: &EntryName,
        name: &ArtifactKey,
    ) -> Option<PinnedNow<'d>> {
        self.names.get(&(link, name)).copied()
    }

    /// The names the link `link` writes.
    pub(in crate::domain::shelf) fn names(
        &self,
        link: &EntryName,
    ) -> impl Iterator<Item = &'d ArtifactKey> {
        self.names
            .keys()
            .filter(move |(of, _)| *of == link)
            .map(|(_, name)| *name)
    }

    /// The links that write a name.
    pub(in crate::domain::shelf) fn links(&self) -> impl Iterator<Item = &'d EntryName> {
        self.names.keys().map(|(link, _)| *link)
    }
}

/// What one name a `Depends` link writes comes to now, and the comparison its link
/// declares.
#[derive(Debug, Clone, Copy)]
pub(in crate::domain::shelf) struct PinnedNow<'d> {
    state: &'d ArtifactState,
    comparison: &'d Comparison,
}

impl<'d> PinnedNow<'d> {
    /// What it comes to now.
    pub(in crate::domain::shelf) fn state(&self) -> &'d ArtifactState {
        self.state
    }

    /// The comparison its link declares now.
    pub(in crate::domain::shelf) fn comparison(&self) -> &'d Comparison {
        self.comparison
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
    artifact_links: BTreeMap<EntryName, ArtifactLink>,
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

/// A document's text by the one rule of normalization: line breaks (CRLF and a lone
/// CR) become LF, the spaces and tabs at the end of each line are removed, and the
/// text ends in exactly one LF, or is empty when nothing but line breaks, spaces
/// and tabs was in it. There is no Unicode normalization. Applying it twice changes
/// nothing.
///
/// The adapter that reads a document applies it to the whole file before reading
/// the front matter, and the hash of a document applies it to the body again, so
/// the body a [`DocHash`] covers is the body after this rule whatever an adapter
/// did: an edit this rule removes changes no hash.
#[must_use]
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
        ("body", Canon::str(normalize(body))),
    ]);
    DocHash::computed(ContentHash::of(HashKind::ShelfDoc, &canon))
}

impl Doc {
    /// A document, whole: the draft (with the hash computed from it) and every link
    /// the book declares, already resolved, into books and to artifacts. The one
    /// place a `Doc` is made.
    pub(super) fn new(
        draft: DocDraft,
        links: BTreeMap<EntryName, Link>,
        artifact_links: BTreeMap<EntryName, ArtifactLink>,
    ) -> Self {
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
            artifact_links,
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

    /// What each of its links names, by name, in the order written: a document a key
    /// resolves to, by its identity; a key that resolves to none, as written; an
    /// artifact, by its space and name. It reads the links alone, not what the
    /// documents and artifacts named hold now, so two readings of a document whose
    /// links name the same are alike here. The one place it is read.
    pub fn links_named(&self) -> BTreeMap<&EntryName, Vec<LinkNamed<'_>>> {
        let docs = self.links.iter().map(|(name, link)| {
            let named = link
                .states()
                .iter()
                .map(|state| match state {
                    LinkState::Resolved(target) => LinkNamed::Doc(target.id()),
                    LinkState::Unresolved(link) => LinkNamed::Written(link.written()),
                })
                .collect();
            (name, named)
        });
        let artifacts = self.artifact_links.iter().map(|(name, link)| {
            let named = link
                .artifacts()
                .iter()
                .map(|artifact| LinkNamed::Artifact(artifact.key()))
                .collect();
            (name, named)
        });
        docs.chain(artifacts).collect()
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

    /// What its `Depends` links resolve to, each with the link that names it, in the
    /// order it holds them: the one rule of which documents its links make it stand
    /// on. A link that cannot be followed leads nowhere.
    pub(crate) fn depends_links(&self) -> impl Iterator<Item = (&EntryName, DocId)> {
        self.links_of(Propagation::Depends)
            .into_iter()
            .flat_map(|(link, states)| {
                states.iter().filter_map(move |state| match state {
                    LinkState::Resolved(target) => Some((link, target.id())),
                    LinkState::Unresolved(_) => None,
                })
            })
    }

    /// The documents its `Depends` links name, for what depends on a document: what a
    /// link resolves to; the one document of the key a link writes when that document
    /// is not valid ([`UnresolvedWhy::TargetInvalid`]: it names it, and cannot stand on
    /// it); and every identity that could be read of the files that write a key two or
    /// more of them write ([`UnresolvedWhy::Ambiguous`]: it names one of them, which
    /// cannot be told). Any other link that cannot be followed names none.
    pub(crate) fn depends_names(&self) -> impl Iterator<Item = DocId> + '_ {
        self.depends_states().flat_map(|state| match named(state) {
            Named::Resolved(id) => vec![id],
            Named::Unfollowed(ids, _) => ids,
            Named::Unnamed(_) | Named::Nothing => Vec::new(),
        })
    }

    /// The documents of [`Doc::depends_names`] that no link resolves to, each with why
    /// its link cannot be followed to it, in the order it holds them: a document two
    /// links name is there for each.
    pub(crate) fn unfollowed_names(&self) -> impl Iterator<Item = (DocId, Unfollowed)> + '_ {
        self.depends_states().flat_map(|state| match named(state) {
            Named::Unfollowed(ids, why) => ids.into_iter().map(|id| (id, why)).collect(),
            Named::Resolved(_) | Named::Unnamed(_) | Named::Nothing => Vec::new(),
        })
    }

    /// The keys its `Depends` links write that are no document's, in the order it holds
    /// them: no file writes the key ([`UnresolvedWhy::Missing`]), or the files that do
    /// carry no identity that could be read ([`UnresolvedWhy::NoIdentity`] for one, an
    /// [`UnresolvedWhy::Ambiguous`] whose writers have no identity for several). A text
    /// that is no key ([`UnresolvedWhy::Malformed`]) is none.
    pub(crate) fn unnamed_keys(&self) -> impl Iterator<Item = &DocKey> + '_ {
        self.depends_states()
            .filter_map(|state| match named(state) {
                Named::Unnamed(key) => Some(key),
                Named::Resolved(_) | Named::Unfollowed(..) | Named::Nothing => None,
            })
    }

    /// What each key written in its `Depends` links comes to, link by link in the order
    /// it holds them.
    fn depends_states(&self) -> impl Iterator<Item = &LinkState> + '_ {
        self.links_of(Propagation::Depends)
            .into_iter()
            .flat_map(|(_, states)| states.iter())
    }

    /// The links to artifacts, by name: every one the book declares is there, with
    /// no artifact in it when the document wrote no name.
    pub fn artifact_links(&self) -> &BTreeMap<EntryName, ArtifactLink> {
        &self.artifact_links
    }

    /// The names its links to artifacts write that have nothing there now, with the
    /// link, by the name of the link and then in the order written. Which names have
    /// something there is what the adapter of the space answered, by its own rule.
    /// The one place such a name is found.
    pub fn absent_artifacts(&self) -> Vec<(&EntryName, &ArtifactKey)> {
        self.artifact_links
            .iter()
            .flat_map(|(name, link)| {
                link.artifacts()
                    .iter()
                    .filter(|artifact| matches!(artifact.state().told(), Ok(ArtifactSeen::Absent)))
                    .map(move |artifact| (name, artifact.key()))
            })
            .collect()
    }

    /// The names its links to artifacts write that cannot be told about now, with the
    /// link and why, in the order of [`Doc::absent_artifacts`]. The one place such a
    /// name is found.
    pub fn unknown_artifacts(&self) -> Vec<(&EntryName, &ArtifactKey, &Untold)> {
        self.artifact_links
            .iter()
            .flat_map(|(name, link)| {
                link.artifacts().iter().filter_map(move |artifact| {
                    let why = artifact.state().told().err()?;
                    Some((name, artifact.key(), why))
                })
            })
            .collect()
    }

    /// The links to artifacts of one meaning for freshness, by name: the one place
    /// they are sorted by their [`ArtifactPropagation`].
    pub fn artifact_links_of(
        &self,
        propagation: ArtifactPropagation,
    ) -> Vec<(&EntryName, &[Artifact])> {
        self.artifact_links_meaning(propagation)
            .map(|(name, link)| (name, link.artifacts()))
            .collect()
    }

    /// The links to artifacts of `propagation`, whole, by name.
    fn artifact_links_meaning(
        &self,
        propagation: ArtifactPropagation,
    ) -> impl Iterator<Item = (&EntryName, &ArtifactLink)> {
        self.artifact_links
            .iter()
            .filter(move |(_, link)| link.kind.propagation == propagation)
    }

    /// What each name its `Depends` links to artifacts write comes to now, with the
    /// comparison its link declares: the one reading of them that what a pin comes to
    /// now, the names a move left behind and the names a link writes are read from.
    pub(in crate::domain::shelf) fn depends_now(&self) -> DependsNow<'_> {
        let mut names = BTreeMap::new();
        for (link, kind) in self.artifact_links_meaning(ArtifactPropagation::Depends) {
            for artifact in kind.artifacts() {
                names.insert(
                    (link, artifact.key()),
                    PinnedNow {
                        state: artifact.state(),
                        comparison: &kind.kind.comparison,
                    },
                );
            }
        }
        DependsNow { names }
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

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use crate::domain::nonempty::{NonBlank, NonEmptySet, OneLine};

    use super::{FieldType, FieldValue, FieldWord, ScalarType};

    fn word(text: &str) -> FieldWord {
        FieldWord::new(text).expect("a word")
    }

    fn colours() -> FieldType {
        let words: BTreeSet<FieldWord> = ["red", "green"].into_iter().map(word).collect();
        FieldType::Enum(NonEmptySet::from_set(words).expect("not empty"))
    }

    fn said(text: &str) -> NonBlank {
        NonBlank::new(text).expect("a text that says something")
    }

    fn read(ty: &FieldType, text: &str) -> Option<FieldValue> {
        FieldValue::read(ty, &said(text))
    }

    #[test]
    fn text_is_read_as_itself_with_its_line_breaks_and_its_spaces() {
        for text in ["a b", "12", "true", " a\nb ", "a\n"] {
            assert_eq!(
                read(&FieldType::Scalar(ScalarType::Text), text),
                Some(FieldValue::Text(said(text))),
                "{text:?}"
            );
        }
    }

    #[test]
    fn a_line_is_read_as_itself_unless_it_has_a_line_break() {
        let line = FieldType::Scalar(ScalarType::Line);
        for text in ["a b", " a ", "12"] {
            assert_eq!(
                read(&line, text),
                Some(FieldValue::Line(OneLine::new(text).expect("one line"))),
                "{text:?}"
            );
        }
        for text in ["a\nb", "a\n", "a\r"] {
            assert_eq!(read(&line, text), None, "{text:?}");
        }
    }

    #[test]
    fn an_integer_is_a_whole_number() {
        assert_eq!(
            read(&FieldType::Scalar(ScalarType::Integer), "12"),
            Some(FieldValue::Integer(12))
        );
        assert_eq!(
            read(&FieldType::Scalar(ScalarType::Integer), "-3"),
            Some(FieldValue::Integer(-3))
        );
        for text in ["x", "1.5", " 1", "#41"] {
            assert_eq!(
                read(&FieldType::Scalar(ScalarType::Integer), text),
                None,
                "{text:?}"
            );
        }
    }

    #[test]
    fn a_bool_is_true_or_false() {
        assert_eq!(
            read(&FieldType::Scalar(ScalarType::Bool), "true"),
            Some(FieldValue::Bool(true))
        );
        assert_eq!(
            read(&FieldType::Scalar(ScalarType::Bool), "false"),
            Some(FieldValue::Bool(false))
        );
        for text in ["yes", "True", "1"] {
            assert_eq!(
                read(&FieldType::Scalar(ScalarType::Bool), text),
                None,
                "{text:?}"
            );
        }
    }

    #[test]
    fn an_enum_takes_only_its_words() {
        assert_eq!(read(&colours(), "red"), Some(FieldValue::Enum(word("red"))));
        for text in ["blue", "Red", "not a word"] {
            assert_eq!(read(&colours(), text), None, "{text:?}");
        }
    }

    #[test]
    fn a_text_list_takes_no_single_text() {
        for text in ["a", "a, b"] {
            assert_eq!(
                read(&FieldType::Scalar(ScalarType::TextList), text),
                None,
                "{text:?}"
            );
        }
    }

    #[test]
    fn the_canonical_form_of_a_value_is_what_it_was_before_a_value_said_something() {
        use crate::domain::canon::Canon;
        use crate::domain::nonempty::NonEmptyVec;

        let tagged =
            |ty: &str, value: Canon| Canon::map([("type", Canon::str(ty)), ("value", value)]);
        assert_eq!(
            FieldValue::Text(said(" a\nb ")).to_canon(),
            tagged("text", Canon::str(" a\nb "))
        );
        assert_eq!(
            FieldValue::TextList(NonEmptyVec::new(said("a"), vec![said(" b ")])).to_canon(),
            tagged(
                "text-list",
                Canon::list([Canon::str("a"), Canon::str(" b ")])
            )
        );
    }

    #[test]
    fn a_line_and_a_text_of_the_same_value_have_the_same_canonical_form() {
        let text = FieldValue::Text(said("same"));
        let line = FieldValue::Line(OneLine::new("same").expect("one line"));
        assert_eq!(text.to_canon(), line.to_canon());
    }
}

#[cfg(test)]
mod wording_tests {
    use crate::domain::nonempty::TwoOrMore;
    use crate::domain::shelf::fixtures_tests::{docid, path};
    use crate::domain::shelf::schema::{KeyPattern, KeyPrefix, KeyProblem};

    use super::{
        BookName, DocKey, DocLabel, KeyCollision, KeyWriter, UnresolvedLink, UnresolvedWhy,
    };

    fn key(book: &str, text: &str) -> DocKey {
        let pattern = KeyPattern::Prefixed(KeyPrefix::new("SP").expect("a prefix"));
        DocKey {
            book: BookName::new(book).expect("a book"),
            form: pattern.parse(text).expect("a key"),
        }
    }

    fn link(why: UnresolvedWhy, key: Option<&DocKey>) -> UnresolvedLink {
        UnresolvedLink::new(
            "SP-1".to_owned(),
            BookName::new("spec").expect("a book"),
            key.map(|k| k.form.clone()),
            why,
        )
    }

    #[test]
    fn a_label_is_the_key_of_the_document_or_else_its_identity() {
        let sp1 = key("spec", "SP-1");
        assert_eq!(
            DocLabel::new(docid(1), Some(sp1.clone())).to_string(),
            "spec:SP-1"
        );
        assert_eq!(
            DocLabel::new(docid(1), None).to_string(),
            docid(1).to_string()
        );
        let label = DocLabel::new(docid(2), Some(sp1.clone()));
        assert_eq!(label.id(), docid(2));
        assert_eq!(label.key(), Some(&sp1));
    }

    #[test]
    fn a_link_that_is_no_key_there_says_what_is_wrong_with_the_text() {
        let problem = KeyProblem::NoSeparator;
        let said = link(UnresolvedWhy::Malformed(problem.clone()), None);
        assert_eq!(
            said.to_string(),
            format!("`SP-1` cannot be followed: not a key: {problem}")
        );
    }

    #[test]
    fn a_link_to_a_key_no_document_has_says_so() {
        let sp1 = key("spec", "SP-1");
        assert_eq!(
            link(UnresolvedWhy::Missing, Some(&sp1)).to_string(),
            "`SP-1` cannot be followed: no document has this key"
        );
    }

    fn writer(file: &str, id: Option<u128>) -> KeyWriter {
        KeyWriter::new(path(file), id.map(docid))
    }

    #[test]
    fn a_link_to_a_key_of_several_files_names_the_files() {
        let sp1 = key("spec", "SP-1");
        let two = KeyCollision::new(
            sp1.clone(),
            TwoOrMore::new(writer("a.md", Some(1)), writer("b.md", None), vec![]),
        );
        assert_eq!(
            link(UnresolvedWhy::Ambiguous(Box::new(two)), Some(&sp1)).to_string(),
            "`SP-1` cannot be followed: the key `spec:SP-1` is written by 2 files: a.md and b.md"
        );
        let three = KeyCollision::new(
            sp1.clone(),
            TwoOrMore::new(
                writer("a.md", Some(1)),
                writer("b.md", Some(2)),
                vec![writer("c.md", None)],
            ),
        );
        assert_eq!(
            link(UnresolvedWhy::Ambiguous(Box::new(three)), Some(&sp1)).to_string(),
            "`SP-1` cannot be followed: the key `spec:SP-1` is written by 3 files: a.md, b.md and c.md"
        );
    }

    #[test]
    fn a_link_to_a_file_with_no_readable_identity_says_so() {
        let sp1 = key("spec", "SP-1");
        let file = path("a.md");
        assert_eq!(
            link(UnresolvedWhy::NoIdentity(file), Some(&sp1)).to_string(),
            "`SP-1` cannot be followed: the file a.md writes the key `spec:SP-1` and carries no identity that can be read"
        );
    }

    #[test]
    fn a_collision_says_the_identities_it_could_read_once_each() {
        let sp1 = key("spec", "SP-1");
        let collision = KeyCollision::new(
            sp1,
            TwoOrMore::new(
                writer("a.md", Some(2)),
                writer("b.md", Some(2)),
                vec![writer("c.md", None)],
            ),
        );
        assert_eq!(collision.ids().into_iter().collect::<Vec<_>>(), [docid(2)]);
        assert_eq!(collision.files().count(), 3);
    }

    #[test]
    fn a_link_to_an_invalid_document_names_it_by_the_key_the_link_read_as() {
        let sp1 = key("spec", "SP-1");
        assert_eq!(
            link(UnresolvedWhy::TargetInvalid(docid(1)), Some(&sp1)).to_string(),
            "`SP-1` cannot be followed: the document it names is invalid (spec:SP-1)"
        );
        // A link that read as no key names it by its identity.
        assert_eq!(
            link(UnresolvedWhy::TargetInvalid(docid(1)), None).to_string(),
            format!(
                "`SP-1` cannot be followed: the document it names is invalid ({})",
                docid(1)
            )
        );
    }
}

#[cfg(test)]
mod names_tests {
    use crate::domain::shelf::build::DocReading;
    use crate::domain::shelf::fixtures_tests::{docid, entries, fr, path, reading, shelf, text};

    use super::Unfollowed;

    /// A finding whose file writes `key` and the identity `id` as written.
    fn writing(file: &str, id: Option<&str>, key: &str) -> DocReading {
        let mut written = vec![("key", text(key)), ("title", text("t"))];
        if let Some(id) = id {
            written.push(("id", text(id)));
        }
        DocReading::Parsed {
            origin: path(file),
            entries: entries(&written),
            body: "body\n".to_owned(),
        }
    }

    #[test]
    fn what_a_link_names_and_cannot_be_followed_to_and_the_keys_that_are_no_documents() {
        // FR-10 links to: FR-1 (valid), FR-2 (not valid), FR-3 (written by the files of
        // 3 and 6), FR-4 (written by a file with no `id:`), FR-5 (written by a file whose
        // identity cannot be read), FR-9 (written by no file) and `bad` (no key).
        let s = shelf(vec![
            fr(1, &[]),
            reading("findings/fr-2.md", 2, "FR-2", &[]),
            fr(3, &[]),
            reading("findings/twin.md", 6, "FR-3", &[("title", text("t"))]),
            writing("findings/fr-4.md", None, "FR-4"),
            writing("findings/fr-5.md", Some("not an identity"), "FR-5"),
            fr(10, &["FR-1", "FR-2", "FR-3", "FR-4", "FR-5", "FR-9", "bad"]),
        ]);
        let doc = s.doc(docid(10)).expect("FR-10 is valid");
        let names: Vec<_> = doc.depends_names().collect();
        assert_eq!(names, [docid(1), docid(2), docid(3), docid(6)]);
        let unfollowed: Vec<_> = doc.unfollowed_names().collect();
        assert_eq!(
            unfollowed,
            [
                (docid(2), Unfollowed::TargetInvalid),
                (docid(3), Unfollowed::Ambiguous),
                (docid(6), Unfollowed::Ambiguous),
            ]
        );
        let keys: Vec<String> = doc.unnamed_keys().map(ToString::to_string).collect();
        assert_eq!(keys, ["finding:FR-4", "finding:FR-5", "finding:FR-9"]);
        // A document whose links all resolve names nothing it cannot be followed to.
        let plain = s.doc(docid(1)).expect("FR-1 is valid");
        assert_eq!(plain.unfollowed_names().count(), 0);
        assert_eq!(plain.unnamed_keys().count(), 0);
    }
}

#[cfg(test)]
mod normalize_tests {
    use super::normalize;

    #[test]
    fn line_breaks_trailing_blanks_and_the_last_lf() {
        for (text, want) in [
            ("", ""),
            ("\n\n", ""),
            ("a", "a\n"),
            ("a\r\nb\rc\n", "a\nb\nc\n"),
            ("a  \t\nb\t\n\n\n", "a\nb\n"),
            ("  lead\n", "  lead\n"),
            ("a\n\n\nb\n", "a\n\n\nb\n"),
            ("caf\u{e9} / cafe\u{301}\n", "caf\u{e9} / cafe\u{301}\n"),
        ] {
            assert_eq!(normalize(text), want, "{text:?}");
        }
    }

    #[test]
    fn normalizing_twice_changes_nothing() {
        for text in ["a \r\n\r\n b\t\r", "x\n", "", " \n \n"] {
            let once = normalize(text);
            assert_eq!(normalize(&once), once, "{text:?}");
        }
    }
}
