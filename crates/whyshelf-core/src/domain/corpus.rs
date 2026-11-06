//! The documents of a project as the files say they are: one set, with the
//! reasons it cannot be trusted where it cannot.
//!
//! A *document* is a file of one of the three kinds that have one (Finding, Spec,
//! Design). Its identity is the `id:` its front matter carries, a UUID v7 ([`DocId`]),
//! which a rename, an edit or a move to another directory does not change. The
//! [`Corpus`] is built from what the adapters read: for each file, the facts about
//! it ([`FileFacts`]: where it sits, and the hash of its bytes) and what reading it
//! gave ([`Reading`]). Building is pure. Which files exist, what they say and what
//! Git says about their place are all inputs.
//!
//! # Reading the layers
//!
//! ```text
//!   adapter (infra)   Markdown + YAML --> a typed value, the bytes' hash
//!         |
//!         v
//!   app               + where the file sits (Location) --> (FileFacts, Reading)
//!         |
//!         v
//!   domain            front: which key is what         Corpus::build: which files,
//!                                                      taken together, can be trusted
//! ```
//!
//! Parsing Markdown or YAML is not done here. A [`Reading`] holds the value the
//! adapter produced. Looking the references of a Spec and a Design up is done here, in
//! [`Corpus::build`], and not by the adapter: what an alias names depends on the other
//! files, and a rule that depends on them is the domain's ([`naming`](super::naming),
//! [`parsed`](super::parsed)).
//!
//! # What a file can be
//!
//! | reading | the file is | kept as |
//! |---|---|---|
//! | [`Reading::Document`] | a document with an `id:` | a [`CorpusDocument`], valid or not; or an [`UnparsedFile`] when a reference it holds cannot be held (see [`parsed`](super::parsed)) |
//! | [`Reading::Unidentified`] | a file of a known (or guessable) kind with no `id:` | an [`ImportCandidate`]; it is not a document |
//! | [`Reading::Unparsed`] | a file that cannot be read | an [`UnparsedFile`] with its reasons, and the `id:` it writes if that could still be seen |
//!
//! A file with no `id:` is never made a document by guessing one for it; an
//! identity written in the file is the only one that survives a rename together with
//! an edit.
//!
//! A file that cannot be read may still show its `id:` (a merge-conflict marker in
//! the body, or in a value of the front matter, leaves the line `id: ...` as it was).
//! The adapter then hands it over beside the reasons, so that a document judged under
//! that id is told apart from one whose file is gone: the first is here and cannot be
//! trusted, the second is missing. This module only carries the id; it is not a
//! document, and it is not made one.
//!
//! # Merging is Git's job
//!
//! A document's content, its classification and its id are merged by Git. This
//! module has no rule for combining two versions of a file. The corpus is read
//! *after* a merge, and it reports what is there: when two branches each added a
//! file with the same id, or gave two documents the same alias, the corpus does not
//! pick one. It says so, for every file concerned.
//!
//! # When a document is invalid
//!
//! Each rule gives a reason ([`Invalidity`]) to every file it concerns, and a file
//! can have several. No rule chooses one file over another.
//!
//! | situation | every file concerned gets |
//! |---|---|
//! | two or more files with one id | [`Invalidity::DuplicateId`], naming the others |
//! | two documents (different ids) with one alias | [`Invalidity::DuplicateAlias`], naming the others |
//! | a private document in a place Git tracks or does not ignore (`git add -A` would publish it) | [`Invalidity::PrivateExposed`] |
//! | a Design with an item that has no `id:`, or two items that share one | [`Invalidity::Item`], one for each [`ItemIssue`] ([`Design::item_issues`]) |
//! | a Design with an item whose `id:` is the Design's own | [`Invalidity::Item`] with [`ItemIssue::ItemIdIsDesignId`] |
//! | an item `id:` that is the item `id:` of an item of another Design, or the `id:` of another document | [`Invalidity::DuplicateItemIdAcross`], naming the other files |
//!
//! Identities are one namespace across the corpus: a document id and an item id are
//! both entity ids, and none may be claimed twice by different documents. Two items
//! of one Design are [`ItemIssue::DuplicateItemId`]'s business, and files that share
//! a document id are `DuplicateId`'s, so neither is reported again. The private copy
//! of a publication cut short is not a second document and never counts.
//!
//! Two items of one Design with one number are not among these: the Design is not
//! built from such a file, so the file is [`Reading::Unparsed`].
//!
//! Where Git cannot say what a place is ([`Knowable::Unknown`], as outside a work
//! tree), a private document is not made invalid; the corpus lists a [`Notice`]
//! instead, so that the fact that it could not be checked is not lost.
//!
//! Exposure is a fact about a file, not about a document. A private file that is
//! not a document of the corpus (one with no `id:`, one that cannot be read, or
//! the private copy of a publication cut short) is published by `git add -A` all
//! the same, so every private file is checked: [`Corpus::exposed`] lists each one
//! Git would publish, documents included, and a place Git cannot tell gives a
//! [`Notice`] whatever the file is.
//!
//! # A document in two roots: a publication cut short
//!
//! Publishing a document moves it from the private root to the public one. The same
//! id in exactly one public and one private file is therefore not always a
//! duplicate; it is a publication that stopped part way, and what it is depends on
//! whether the document has been recorded as published (`published`, the set of
//! ids passed to [`Corpus::build`]):
//!
//! | published | bytes of the two files | form | the document is |
//! |---|---|---|---|
//! | yes | any | [`PublishForm::PrivateCopyLeft`] | the public file; the private file is kept as a [`PrivateCopy`], a value for showing a difference and never used for state |
//! | no | equal | [`PublishForm::FileCopied`] | the public file; the private file is a copy of it |
//! | no | different | none: [`Invalidity::DuplicateId`] on both | neither (a file copied by hand and then edited must not be removed by a tool) |
//!
//! Only the public file is read as the document in the first two forms, so one id
//! has one content. Neither form makes the document invalid; the forms are listed
//! by [`Corpus::publishing`], for the layers above to decide what may be done while
//! one is open.
//!
//! # Reading the corpus
//!
//! Lookups never choose silently. [`Corpus::by_id`] and [`Corpus::by_alias`] return
//! every file that matches; more than one is itself the finding. The documents are
//! ordered by id and then by location, whatever the order the files arrived in.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use super::actor::Actor;
use super::design::{Design, ItemIssue};
use super::error::DocumentError;
use super::event::DocKind;
use super::finding::Finding;
use super::hash::{BytesHash, TextHash};
use super::id::{Alias, DocId, EntityId};
use super::knowable::Knowable;
use super::location::RelPath;
use super::naming::{Index, Resolver, Written};
use super::nonempty::NonEmptyVec;
use super::parsed::ParsedDocument;
use super::spec::Spec;
use super::state::UnparsedReason;
use super::tag::Tag;

/// Whether a document is shared with the project's readers or kept to one person.
///
/// Decided by the root the file sits under, never by a flag in the file: a flag
/// that is forgotten would leave a document public.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Visibility {
    /// Under the shared root: committed with the project.
    Public,
    /// Under the private root: for one person, never to be committed.
    Private,
}

/// What Git makes of a path in the work tree.
///
/// Why it is here: a private file that Git tracks, or does not ignore, is one
/// `git add -A` publishes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum GitPlace {
    /// Outside the work tree: Git does not see it.
    Outside,
    /// Inside, and ignored.
    Ignored,
    /// Inside, not ignored, and not yet tracked.
    Untracked,
    /// Inside, and tracked.
    Tracked,
}

impl fmt::Display for GitPlace {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Outside => "outside the work tree",
            Self::Ignored => "ignored",
            Self::Untracked => "untracked and not ignored",
            Self::Tracked => "tracked",
        })
    }
}

/// Where a file is: under which root, at which path, and what Git makes of it.
///
/// The path is relative to the root; the root says [`Visibility`]. The Git fact is
/// supplied by the layer that can ask Git, and is [`Knowable::Unknown`] when it
/// cannot be told.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Location {
    visibility: Visibility,
    path: RelPath,
    git: Knowable<GitPlace>,
}

impl Location {
    /// A location.
    pub fn new(visibility: Visibility, path: RelPath, git: Knowable<GitPlace>) -> Self {
        Self {
            visibility,
            path,
            git,
        }
    }

    /// Whether the file sits under the shared root or the private one.
    pub fn visibility(&self) -> Visibility {
        self.visibility
    }

    /// The path under the root.
    pub fn path(&self) -> &RelPath {
        &self.path
    }

    /// What Git makes of the path, if it can be told.
    pub fn git(&self) -> Knowable<GitPlace> {
        self.git
    }
}

/// What is known about a file before it is read: where it is, and the hash of its
/// bytes.
///
/// The hash is of the bytes as they are on disk, before the text is normalized
/// (see [`BytesHash`]), so it tells whether two files are byte-for-byte copies.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileFacts {
    location: Location,
    bytes: BytesHash,
}

impl FileFacts {
    /// The facts of one file.
    pub fn new(location: Location, bytes: BytesHash) -> Self {
        Self { location, bytes }
    }

    /// Where the file is.
    pub fn location(&self) -> &Location {
        &self.location
    }

    /// The hash of the file's bytes before normalization.
    pub fn bytes(&self) -> BytesHash {
        self.bytes
    }
}

/// What a document says about itself that is not its content: the keys of the front
/// matter that classify it.
///
/// A header is the file's own values. The corpus compares them across files (the
/// alias) and keys on them (the id); it does not correct them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Header {
    id: DocId,
    kind: DocKind,
    alias: Option<Alias>,
    tags: BTreeSet<Tag>,
    owner: Option<Actor>,
}

impl Header {
    /// A header from the classification keys of a file.
    pub fn new(
        id: DocId,
        kind: DocKind,
        alias: Option<Alias>,
        tags: BTreeSet<Tag>,
        owner: Option<Actor>,
    ) -> Self {
        Self {
            id,
            kind,
            alias,
            tags,
            owner,
        }
    }

    /// The identity, the `id:` key.
    pub fn id(&self) -> DocId {
        self.id
    }

    /// The kind of document.
    pub fn kind(&self) -> DocKind {
        self.kind
    }

    /// The human-readable name, if the file has one.
    pub fn alias(&self) -> Option<&Alias> {
        self.alias.as_ref()
    }

    /// The tags the file carries.
    pub fn tags(&self) -> &BTreeSet<Tag> {
        &self.tags
    }

    /// The owner the file names, if any.
    pub fn owner(&self) -> Option<&Actor> {
        self.owner.as_ref()
    }
}

/// The entity a document file holds: one of the three stages that have a file.
///
/// Why this and not the parsed value of the adapter: by the time a document reaches
/// the corpus, the references it holds are resolved and its hash is computed. That
/// is what the entities already are.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Entity {
    /// A Finding.
    Finding(Finding),
    /// A Spec.
    Spec(Spec),
    /// A Design.
    Design(Design),
}

impl Entity {
    /// The kind of document this entity is.
    pub fn kind(&self) -> DocKind {
        match self {
            Self::Finding(_) => DocKind::Finding,
            Self::Spec(_) => DocKind::Spec,
            Self::Design(_) => DocKind::Design,
        }
    }

    /// The identity the entity carries, as a document identity.
    pub fn id(&self) -> DocId {
        let entity: EntityId = match self {
            Self::Finding(finding) => finding.id().into(),
            Self::Spec(spec) => spec.id().into(),
            Self::Design(design) => design.id().into(),
        };
        DocId::new(entity)
    }
}

/// A file read as a document: its header and the entity it holds.
///
/// The two agree on the id and on the kind. That is checked once, here, so the
/// corpus can key on the header's id and the layers above can take the entity at its
/// word.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Document {
    header: Header,
    // Boxed so that a reading that is not a document stays small.
    entity: Box<Entity>,
}

impl Document {
    /// A document; refused when the header and the entity disagree on the id or the
    /// kind.
    pub fn new(header: Header, entity: Entity) -> Result<Self, DocumentError> {
        if header.kind != entity.kind() {
            return Err(DocumentError::KindMismatch {
                header: header.kind,
                entity: entity.kind(),
            });
        }
        if header.id != entity.id() {
            return Err(DocumentError::IdMismatch {
                header: header.id,
                entity: entity.id(),
            });
        }
        Ok(Self {
            header,
            entity: Box::new(entity),
        })
    }

    /// The classification keys of the file.
    pub fn header(&self) -> &Header {
        &self.header
    }

    /// The entity the file holds.
    pub fn entity(&self) -> &Entity {
        &self.entity
    }

    /// The identity.
    pub fn id(&self) -> DocId {
        self.header.id
    }

    /// The kind.
    pub fn kind(&self) -> DocKind {
        self.header.kind
    }
}

/// What reading one file gave.
///
/// A document is read as it is written, with its references still to be looked up
/// ([`ParsedDocument`]); [`Corpus::build`] looks them up once it has every file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Reading {
    /// The file is a document.
    Document(ParsedDocument),
    /// The file has no `id:`. It is not a document; it is a file that could be
    /// imported once an `id:` is written into it. `kind` is what the file shows of
    /// its kind, if that can be told.
    Unidentified {
        /// The kind the file shows, if any.
        kind: Option<DocKind>,
    },
    /// The file cannot be read as a document, for these reasons (a merge-conflict
    /// marker is one).
    Unparsed {
        /// The `id:` of the file, when the adapter could still see it though the
        /// file as a whole could not be read. `None` when it could not.
        id: Option<DocId>,
        /// Why the file cannot be read.
        reasons: NonEmptyVec<UnparsedReason>,
    },
}

#[cfg(test)]
impl Reading {
    /// A document whose references are identifiers already, written as such; the hash
    /// of its text follows from what it says, so two documents that differ have texts
    /// that differ.
    pub(crate) fn resolved(document: Document) -> Self {
        let text = TextHash::of(&format!("{document:?}"));
        Self::Document(ParsedDocument::from_document(document, text))
    }

    /// The same, with the references to the `private` documents written behind a
    /// `private:` mark, as a file does that stands on a document of the private root;
    /// `None` marks every reference.
    pub(crate) fn resolved_marking(document: Document, private: Option<&BTreeSet<DocId>>) -> Self {
        let text = TextHash::of(&format!("{document:?}"));
        let parsed = ParsedDocument::from_document(document, text);
        Self::Document(match private {
            Some(private) => parsed.with_private_marks_of(private),
            None => parsed.with_private_marks(),
        })
    }
}

/// Why a document cannot be trusted as it stands.
///
/// Each reason is a fact about the files together; none says which file is right.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum Invalidity {
    /// Other files carry the same id.
    ///
    /// Fix: give all but one a new id, or remove the copies.
    #[error("{} other files carry the same id", .others.len())]
    DuplicateId {
        /// The other files with this id.
        others: NonEmptyVec<Location>,
    },
    /// Another document (another id) has the same alias.
    ///
    /// Fix: give one of them another alias.
    #[error("{} files of other documents have the alias {alias}", .others.len())]
    DuplicateAlias {
        /// The shared alias.
        alias: Alias,
        /// The files of the other documents with this alias.
        others: NonEmptyVec<Location>,
    },
    /// A private document sits where Git tracks it or would add it, so `git add -A`
    /// would publish it.
    ///
    /// Fix: ignore the place (or move the file to one that is), and take it out of
    /// the index if it is tracked.
    #[error("a private document is in a place Git would publish ({place})")]
    PrivateExposed {
        /// What Git makes of the place.
        place: GitPlace,
    },
    /// The identity of the items of a Design cannot be trusted: an item has no
    /// `id:`, or two items share one.
    ///
    /// The rule is [`Design::item_issues`]; the Design has one reason per issue.
    /// Fix: as the issue says.
    #[error("{0}")]
    Item(ItemIssue),
    /// An item `id:` is also the item `id:` of an item of another Design, or the
    /// `id:` of another document.
    ///
    /// Fix: give one of the claimants a new id.
    #[error("the id {id} is also claimed by {} files of other documents", .others.len())]
    DuplicateItemIdAcross {
        /// The id claimed more than once.
        id: EntityId,
        /// The files of the other documents that claim it (as an item id or as
        /// their own id).
        others: NonEmptyVec<Location>,
    },
}

/// Whether a document can be trusted as it stands.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Standing {
    /// No rule found a fault with it.
    Valid,
    /// At least one rule did; every reason is kept.
    Invalid(NonEmptyVec<Invalidity>),
}

impl Standing {
    /// Whether no rule found a fault.
    pub fn is_valid(&self) -> bool {
        matches!(self, Self::Valid)
    }

    /// Every reason the document is invalid; empty when it is valid.
    pub fn reasons(&self) -> Vec<&Invalidity> {
        match self {
            Self::Valid => Vec::new(),
            Self::Invalid(reasons) => reasons.iter().collect(),
        }
    }
}

/// A document file of the corpus: the file, what it says, and whether it can be
/// trusted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CorpusDocument {
    facts: FileFacts,
    document: Document,
    text: TextHash,
    standing: Standing,
}

impl CorpusDocument {
    /// The facts about the file.
    pub fn facts(&self) -> &FileFacts {
        &self.facts
    }

    /// The hash of the normalized text the document was read from: the version a gate
    /// is told it read and the key of the stored object of that version.
    ///
    /// Why not part of [`FileFacts`]: the facts are what is known before the file is
    /// read, and the hash of the bytes there is another hash of another thing.
    pub fn text(&self) -> TextHash {
        self.text
    }

    /// Where the file is.
    pub fn location(&self) -> &Location {
        &self.facts.location
    }

    /// The document.
    pub fn document(&self) -> &Document {
        &self.document
    }

    /// The identity.
    pub fn id(&self) -> DocId {
        self.document.id()
    }

    /// Whether the document can be trusted, and if not, why.
    pub fn standing(&self) -> &Standing {
        &self.standing
    }
}

/// A file with no `id:`: a candidate to be given one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportCandidate {
    facts: FileFacts,
    kind: Option<DocKind>,
}

impl ImportCandidate {
    /// The facts about the file.
    pub fn facts(&self) -> &FileFacts {
        &self.facts
    }

    /// The kind the file shows, if it shows one.
    pub fn kind(&self) -> Option<DocKind> {
        self.kind
    }
}

/// A file that could not be read, with the reasons.
///
/// The `id:` it carries may still have been seen. A document judged under that id is
/// then not gone: its file is here and cannot be trusted, which is not what a missing
/// file is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnparsedFile {
    facts: FileFacts,
    id: Option<DocId>,
    reasons: NonEmptyVec<UnparsedReason>,
}

impl UnparsedFile {
    /// The facts about the file.
    pub fn facts(&self) -> &FileFacts {
        &self.facts
    }

    /// The identity the file writes as `id:`, if it could still be seen.
    pub fn id(&self) -> Option<DocId> {
        self.id
    }

    /// Why it could not be read.
    pub fn reasons(&self) -> &NonEmptyVec<UnparsedReason> {
        &self.reasons
    }
}

/// The shape of a publication that stopped with the same id in a public and a
/// private file (see the module documentation).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum PublishForm {
    /// The document has been recorded as published, and a private file with its id
    /// is left. The public file is the document.
    PrivateCopyLeft,
    /// The document has not been recorded as published, and the private file is a
    /// byte-for-byte copy of the public one.
    FileCopied,
}

/// The private file left behind when a publication stopped.
///
/// It is a value: what the file said, kept so that a difference from the public
/// file can be shown. It is never used to derive a state, a hash or a judgement.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrivateCopy {
    facts: FileFacts,
    document: Document,
}

impl PrivateCopy {
    /// The facts about the private file.
    pub fn facts(&self) -> &FileFacts {
        &self.facts
    }

    /// What the private file says.
    pub fn document(&self) -> &Document {
        &self.document
    }
}

/// A document in a public and a private file at once: a publication cut short.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PublishInProgress {
    id: DocId,
    form: PublishForm,
    public: FileFacts,
    private_copy: PrivateCopy,
}

impl PublishInProgress {
    /// The id both files carry.
    pub fn id(&self) -> DocId {
        self.id
    }

    /// Which form it is.
    pub fn form(&self) -> PublishForm {
        self.form
    }

    /// The public file, which is the document.
    pub fn public(&self) -> &FileFacts {
        &self.public
    }

    /// The private file, kept as a value.
    pub fn private_copy(&self) -> &PrivateCopy {
        &self.private_copy
    }
}

/// Something the corpus could not check, which is not a fault.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Notice {
    /// A private file whose place Git could not be asked about, so whether it
    /// would be published is not known.
    GitPlaceUnknown {
        /// The file.
        location: Location,
    },
}

impl Notice {
    /// The file the notice is about.
    pub fn location(&self) -> &Location {
        match self {
            Self::GitPlaceUnknown { location } => location,
        }
    }
}

/// A private file that sits where Git tracks it or would add it.
///
/// Why a list of its own: exposure is a fact about a file, not about a document.
/// A private file with no `id:`, one that cannot be read, and the private copy a
/// cut-short publication leaves are not documents of the corpus, and `git add -A`
/// publishes them all the same.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExposedFile {
    location: Location,
    place: GitPlace,
}

impl ExposedFile {
    /// The file.
    pub fn location(&self) -> &Location {
        &self.location
    }

    /// What Git makes of its place: [`GitPlace::Untracked`] or [`GitPlace::Tracked`].
    pub fn place(&self) -> GitPlace {
        self.place
    }
}

/// The exposure of every private file, collected as the files are checked.
#[derive(Default)]
struct Exposures {
    exposed: Vec<ExposedFile>,
    notices: Vec<Notice>,
}

impl Exposures {
    /// Check one file. Returns the place when the file is private and Git would
    /// publish it; records a notice when Git cannot say.
    fn check(&mut self, location: &Location) -> Option<GitPlace> {
        if location.visibility != Visibility::Private {
            return None;
        }
        match location.git {
            Knowable::Known(place @ (GitPlace::Untracked | GitPlace::Tracked)) => {
                self.exposed.push(ExposedFile {
                    location: location.clone(),
                    place,
                });
                Some(place)
            }
            Knowable::Known(GitPlace::Outside | GitPlace::Ignored) => None,
            Knowable::Unknown => {
                self.notices.push(Notice::GitPlaceUnknown {
                    location: location.clone(),
                });
                None
            }
        }
    }
}

/// The documents of a project, with the reasons they can or cannot be trusted.
///
/// Built once from what was read ([`Corpus::build`]); never changed afterwards. See
/// the module documentation for the rules.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Corpus {
    documents: Vec<CorpusDocument>,
    candidates: Vec<ImportCandidate>,
    unparsed: Vec<UnparsedFile>,
    publishing: Vec<PublishInProgress>,
    exposed: Vec<ExposedFile>,
    notices: Vec<Notice>,
    index: Index,
    written: BTreeMap<DocId, Written>,
}

/// A document file on its way to the corpus: not yet judged.
struct Pending {
    facts: FileFacts,
    document: Document,
    text: TextHash,
    reasons: Vec<Invalidity>,
}

/// A document file whose references were looked up: where it is, what it says, and the
/// hash of its text.
type File = (FileFacts, Document, TextHash);

/// The files that share one id, told by their number and their roots.
enum Group {
    Sole(File),
    Pair { public: File, private: File },
    Many(Vec<File>),
}

fn group(files: Vec<File>) -> Group {
    let files = match <[File; 1]>::try_from(files) {
        Ok([file]) => return Group::Sole(file),
        Err(files) => files,
    };
    match <[File; 2]>::try_from(files) {
        Ok([a, b]) => match (a.0.location.visibility, b.0.location.visibility) {
            (Visibility::Public, Visibility::Private) => Group::Pair {
                public: a,
                private: b,
            },
            (Visibility::Private, Visibility::Public) => Group::Pair {
                public: b,
                private: a,
            },
            _ => Group::Many(vec![a, b]),
        },
        Err(files) => Group::Many(files),
    }
}

fn non_empty<T>(items: Vec<T>) -> Option<NonEmptyVec<T>> {
    NonEmptyVec::from_vec(items).ok()
}

impl Corpus {
    /// Collect what was read into a corpus and apply the rules of the module
    /// documentation.
    ///
    /// `published` is the set of document ids that have been recorded as published;
    /// it decides what a document in both a public and a private file is. The order
    /// of `readings` does not matter.
    ///
    /// The references of the documents are looked up here, and nowhere else:
    ///
    /// ```text
    ///   readings --> Index (every header) --> Resolver --> entities, Written
    /// ```
    ///
    /// A file whose reference cannot be held (an alias that names nothing, several, a
    /// document of the wrong kind) is listed with the files that cannot be read
    /// ([`Corpus::unparsed`]), under the `id:` it shows. The histories that explain a
    /// rename are not available here, so a miss carries no hint.
    pub fn build(
        readings: impl IntoIterator<Item = (FileFacts, Reading)>,
        published: &BTreeSet<DocId>,
    ) -> Self {
        let mut parsed: Vec<(FileFacts, ParsedDocument)> = Vec::new();
        let mut candidates = Vec::new();
        let mut unparsed = Vec::new();
        for (facts, reading) in readings {
            match reading {
                Reading::Document(document) => parsed.push((facts, document)),
                Reading::Unidentified { kind } => candidates.push(ImportCandidate { facts, kind }),
                Reading::Unparsed { id, reasons } => {
                    unparsed.push(UnparsedFile { facts, id, reasons });
                }
            }
        }

        // Every header is indexed before any entity is built: a Spec holds the ids its
        // grounds come to, so the grounds are looked up first.
        let heard = Index::of_parsed(
            parsed
                .iter()
                .map(|(facts, document)| (&facts.location, document)),
            &unparsed,
        );
        let resolver = Resolver::new(&heard, Knowable::Unknown, Knowable::Unknown);
        let mut by_id: BTreeMap<DocId, Vec<File>> = BTreeMap::new();
        let mut written: BTreeMap<DocId, Written> = BTreeMap::new();
        for (facts, document) in parsed {
            let id = document.id();
            let seen_from = resolver.seen_from(facts.location.visibility());
            match document.settle(&seen_from, &heard) {
                Ok(settled) => {
                    if settled.written != Written::default() {
                        let mine = written.entry(id).or_default();
                        mine.unresolved.extend(settled.written.unresolved);
                        mine.dangling.extend(settled.written.dangling);
                    }
                    by_id
                        .entry(id)
                        .or_default()
                        .push((facts, settled.document, settled.text));
                }
                Err(reasons) => unparsed.push(UnparsedFile {
                    facts,
                    id: Some(id),
                    reasons,
                }),
            }
        }
        candidates.sort_by(|a, b| a.facts.location.cmp(&b.facts.location));
        unparsed.sort_by(|a, b| a.facts.location.cmp(&b.facts.location));

        let mut pending = Vec::new();
        let mut publishing = Vec::new();
        for (id, mut files) in by_id {
            files.sort_by(|a, b| a.0.location.cmp(&b.0.location));
            let many = match group(files) {
                Group::Sole(file) => {
                    pending.push(Self::pending(file, Vec::new()));
                    continue;
                }
                Group::Pair { public, private } => {
                    let form = if published.contains(&id) {
                        Some(PublishForm::PrivateCopyLeft)
                    } else if public.0.bytes == private.0.bytes {
                        Some(PublishForm::FileCopied)
                    } else {
                        None
                    };
                    match form {
                        Some(form) => {
                            publishing.push(PublishInProgress {
                                id,
                                form,
                                public: public.0.clone(),
                                private_copy: PrivateCopy {
                                    facts: private.0,
                                    document: private.1,
                                },
                            });
                            pending.push(Self::pending(public, Vec::new()));
                            continue;
                        }
                        None => vec![public, private],
                    }
                }
                Group::Many(files) => files,
            };
            let locations: Vec<Location> =
                many.iter().map(|file| file.0.location.clone()).collect();
            for (index, file) in many.into_iter().enumerate() {
                let others = locations
                    .iter()
                    .enumerate()
                    .filter(|(other, _)| *other != index)
                    .map(|(_, location)| location.clone())
                    .collect();
                let reasons = non_empty(others)
                    .map(|others| Invalidity::DuplicateId { others })
                    .into_iter()
                    .collect();
                pending.push(Self::pending(file, reasons));
            }
        }
        pending.sort_by(|a, b| {
            a.document
                .id()
                .cmp(&b.document.id())
                .then_with(|| a.facts.location.cmp(&b.facts.location))
        });

        Self::mark_duplicate_aliases(&mut pending);
        Self::mark_duplicate_item_ids_across(&mut pending);
        publishing.sort_by_key(|form| form.id);

        // Exposure is per file: the files that are not documents are checked too.
        let mut exposures = Exposures::default();
        for file in &mut pending {
            if let Some(place) = exposures.check(&file.facts.location) {
                file.reasons.push(Invalidity::PrivateExposed { place });
            }
        }
        let not_documents = candidates
            .iter()
            .map(|file| &file.facts)
            .chain(unparsed.iter().map(|file| &file.facts))
            .chain(publishing.iter().map(|form| &form.private_copy.facts));
        for facts in not_documents {
            exposures.check(&facts.location);
        }
        let Exposures {
            mut exposed,
            mut notices,
        } = exposures;
        exposed.sort_by(|a, b| a.location.cmp(&b.location));
        notices.sort_by(|a, b| a.location().cmp(b.location()));

        let documents: Vec<CorpusDocument> = pending
            .into_iter()
            .map(|file| CorpusDocument {
                facts: file.facts,
                document: file.document,
                text: file.text,
                standing: match non_empty(file.reasons) {
                    Some(reasons) => Standing::Invalid(reasons),
                    None => Standing::Valid,
                },
            })
            .collect();
        // The index of the documents the corpus holds and of the files it could not
        // read, which the lookups after the build stand on. It leaves out what the
        // lookups of the build saw and the corpus does not keep: the private copy of a
        // publication cut short.
        let index = Index::of_documents(&documents, &unparsed);
        Self {
            documents,
            candidates,
            unparsed,
            publishing,
            exposed,
            notices,
            index,
            written,
        }
    }

    /// A file on its way to the corpus, with the reasons it has so far and those of
    /// the document's own content: the faults of the identity of a Design's items.
    fn pending((facts, document, text): File, mut reasons: Vec<Invalidity>) -> Pending {
        if let Entity::Design(design) = document.entity() {
            reasons.extend(design.item_issues().into_iter().map(Invalidity::Item));
        }
        Pending {
            facts,
            document,
            text,
            reasons,
        }
    }

    /// Two documents with different ids and one alias are both at fault. Files that
    /// share an id are one document claimed twice: that is `DuplicateId`'s business.
    fn mark_duplicate_aliases(pending: &mut [Pending]) {
        let mut by_alias: BTreeMap<Alias, Vec<usize>> = BTreeMap::new();
        for (index, file) in pending.iter().enumerate() {
            if let Some(alias) = file.document.header.alias() {
                by_alias.entry(alias.clone()).or_default().push(index);
            }
        }
        for (alias, members) in by_alias {
            for &index in &members {
                let id = pending[index].document.id();
                let others: Vec<Location> = members
                    .iter()
                    .filter(|&&other| pending[other].document.id() != id)
                    .map(|&other| pending[other].facts.location.clone())
                    .collect();
                if let Some(others) = non_empty(others) {
                    pending[index].reasons.push(Invalidity::DuplicateAlias {
                        alias: alias.clone(),
                        others,
                    });
                }
            }
        }
    }

    /// An id claimed by different documents, at least one of them as an item id, is a
    /// fault of every claimant. `pending` holds one file per publication, so the
    /// private copy of a cut-short publication never claims anything.
    fn mark_duplicate_item_ids_across(pending: &mut [Pending]) {
        // Each claim is (file index, the claimant's document id); `pending` is sorted,
        // so walking it in order keeps every list ordered.
        let mut claims: BTreeMap<EntityId, Vec<(usize, DocId)>> = BTreeMap::new();
        for (index, file) in pending.iter().enumerate() {
            let doc = file.document.id();
            claims.entry(doc.entity()).or_default().push((index, doc));
            if let Entity::Design(design) = file.document.entity() {
                let ids: BTreeSet<EntityId> = design
                    .items()
                    .values()
                    .filter_map(|item| item.id())
                    .map(|id| id.entity())
                    .collect();
                for id in ids {
                    claims.entry(id).or_default().push((index, doc));
                }
            }
        }
        for (id, claimants) in claims {
            for &(index, doc) in &claimants {
                let others: BTreeSet<usize> = claimants
                    .iter()
                    .filter(|(_, other)| *other != doc)
                    .map(|&(other, _)| other)
                    .collect();
                let locations = others
                    .into_iter()
                    .map(|other| pending[other].facts.location.clone())
                    .collect();
                if let Some(others) = non_empty(locations) {
                    pending[index]
                        .reasons
                        .push(Invalidity::DuplicateItemIdAcross { id, others });
                }
            }
        }
    }

    /// Every document file, ordered by id and then by location.
    ///
    /// A document in a publication cut short appears once, as its public file.
    pub fn documents(&self) -> &[CorpusDocument] {
        &self.documents
    }

    /// The files that carry this id: none if the document is not there, one if it
    /// is, and several if the id is duplicated (every one of them is invalid).
    pub fn by_id(&self, id: &DocId) -> &[CorpusDocument] {
        let start = self.documents.partition_point(|file| file.id() < *id);
        let end = self.documents.partition_point(|file| file.id() <= *id);
        &self.documents[start..end]
    }

    /// The document files that have this alias: several means the alias is shared
    /// (every one of them is invalid).
    pub fn by_alias(&self, alias: &Alias) -> Vec<&CorpusDocument> {
        self.documents
            .iter()
            .filter(|file| file.document.header.alias() == Some(alias))
            .collect()
    }

    /// The files with no `id:`, which could be imported.
    pub fn candidates(&self) -> &[ImportCandidate] {
        &self.candidates
    }

    /// The files that could not be read.
    pub fn unparsed(&self) -> &[UnparsedFile] {
        &self.unparsed
    }

    /// The publications cut short, ordered by id.
    pub fn publishing(&self) -> &[PublishInProgress] {
        &self.publishing
    }

    /// The cut-short publication of this document, if there is one.
    pub fn publishing_of(&self, id: &DocId) -> Option<&PublishInProgress> {
        self.publishing.iter().find(|form| form.id == *id)
    }

    /// Whether the document is private as an upstream, so that a pin of it is keyed:
    /// a file of it is in the private root, or it is in a publication that is not
    /// recorded yet ([`PublishForm::FileCopied`]). Until `Published` is written the
    /// document is still private, although the file this corpus reads is the public
    /// one.
    pub fn is_private(&self, id: &DocId) -> bool {
        self.by_id(id)
            .iter()
            .any(|file| file.location().visibility() == Visibility::Private)
            || self
                .publishing_of(id)
                .is_some_and(|form| form.form() == PublishForm::FileCopied)
    }

    /// Every private file Git would publish, ordered by location: the documents
    /// among them (which are also [`Invalidity::PrivateExposed`]) and the files that
    /// are not documents (no `id:`, unreadable, or the private copy of a
    /// publication cut short).
    pub fn exposed(&self) -> &[ExposedFile] {
        &self.exposed
    }

    /// What could not be checked, which is not a fault, ordered by location.
    pub fn notices(&self) -> &[Notice] {
        &self.notices
    }

    /// The index the references are looked up in: the ids, aliases, places and item
    /// numbers of the documents this corpus holds, to build a
    /// [`Resolver`] on.
    pub fn index(&self) -> &Index {
        &self.index
    }

    /// How each document wrote the upstream identifiers it holds that no state can
    /// answer for, made while the references were looked up. A document not in the map
    /// wrote none of the kind.
    pub fn written(&self) -> &BTreeMap<DocId, Written> {
        &self.written
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::design::WorkItem;
    use crate::domain::design::test_support::{design as design_of, identified, item, num};
    use crate::domain::id::test_support::{docid, fid, iid, sid};
    use crate::domain::nonempty::NonBlank;

    fn rel(path: &str) -> RelPath {
        path.parse().unwrap()
    }

    fn at(visibility: Visibility, path: &str, git: Knowable<GitPlace>) -> Location {
        Location::new(visibility, rel(path), git)
    }

    fn facts(location: Location, bytes: &[u8]) -> FileFacts {
        FileFacts::new(location, BytesHash::of(bytes))
    }

    fn public(path: &str, bytes: &[u8]) -> FileFacts {
        facts(
            at(Visibility::Public, path, Knowable::Known(GitPlace::Tracked)),
            bytes,
        )
    }

    fn private(path: &str, bytes: &[u8], git: Knowable<GitPlace>) -> FileFacts {
        facts(at(Visibility::Private, path, git), bytes)
    }

    fn safe() -> Knowable<GitPlace> {
        Knowable::Known(GitPlace::Ignored)
    }

    fn spec(n: u128) -> Entity {
        Entity::Spec(
            Spec::new(
                sid(n),
                NonEmptyVec::new(fid(900), Vec::new()),
                NonBlank::new("the contract").unwrap(),
                BTreeMap::new(),
                BTreeMap::new(),
            )
            .unwrap(),
        )
    }

    fn doc_with(n: u128, alias: Option<&str>) -> Document {
        let header = Header::new(
            docid(n),
            DocKind::Spec,
            alias.map(|a| a.parse().unwrap()),
            BTreeSet::new(),
            None,
        );
        Document::new(header, spec(n)).unwrap()
    }

    fn doc(n: u128) -> Reading {
        Reading::resolved(doc_with(n, None))
    }

    fn aliased(n: u128, alias: &str) -> Reading {
        Reading::resolved(doc_with(n, Some(alias)))
    }

    fn none() -> BTreeSet<DocId> {
        BTreeSet::new()
    }

    fn reasons_of(file: &CorpusDocument) -> Vec<Invalidity> {
        file.standing().reasons().into_iter().cloned().collect()
    }

    fn paths(locations: &NonEmptyVec<Location>) -> Vec<&str> {
        locations.iter().map(|l| l.path().as_str()).collect()
    }

    #[test]
    fn a_lone_document_is_valid() {
        let corpus = Corpus::build([(public("a.md", b"a"), doc(1))], &none());
        let files = corpus.by_id(&docid(1));
        assert_eq!(files.len(), 1);
        assert!(files[0].standing().is_valid());
        assert!(corpus.publishing().is_empty());
        assert!(corpus.notices().is_empty());
    }

    #[test]
    fn two_files_with_one_id_are_both_duplicates() {
        let corpus = Corpus::build(
            [
                (public("a.md", b"one"), doc(1)),
                (public("b.md", b"two"), doc(1)),
            ],
            &none(),
        );
        let files = corpus.by_id(&docid(1));
        assert_eq!(files.len(), 2);
        for file in files {
            let reasons = reasons_of(file);
            assert_eq!(reasons.len(), 1);
            let Invalidity::DuplicateId { others } = &reasons[0] else {
                panic!("expected DuplicateId, got {reasons:?}");
            };
            let mine = file.location().path().as_str();
            let expected = if mine == "a.md" { "b.md" } else { "a.md" };
            assert_eq!(paths(others), [expected]);
        }
    }

    #[test]
    fn three_files_with_one_id_name_the_two_others() {
        let corpus = Corpus::build(
            [
                (public("a.md", b"1"), doc(1)),
                (private("b.md", b"2", safe()), doc(1)),
                (public("c.md", b"3"), doc(1)),
            ],
            &none(),
        );
        let files = corpus.by_id(&docid(1));
        assert_eq!(files.len(), 3);
        for file in files {
            let reasons = reasons_of(file);
            let [Invalidity::DuplicateId { others }] = reasons.as_slice() else {
                panic!("expected one DuplicateId");
            };
            assert_eq!(others.len(), 2);
        }
    }

    #[test]
    fn a_public_and_a_private_file_with_equal_bytes_are_a_copied_file() {
        let corpus = Corpus::build(
            [
                (public("a.md", b"same"), doc(1)),
                (private("a.md", b"same", safe()), doc(1)),
            ],
            &none(),
        );
        let files = corpus.by_id(&docid(1));
        assert_eq!(files.len(), 1, "the private file is not a document");
        assert!(files[0].standing().is_valid());
        assert_eq!(files[0].location().visibility(), Visibility::Public);
        let form = corpus.publishing_of(&docid(1)).unwrap();
        assert_eq!(form.form(), PublishForm::FileCopied);
        assert_eq!(form.public().location().visibility(), Visibility::Public);
        assert_eq!(
            form.private_copy().facts().location().visibility(),
            Visibility::Private
        );
    }

    #[test]
    fn a_public_and_a_private_file_with_different_bytes_are_duplicates() {
        let corpus = Corpus::build(
            [
                (public("a.md", b"one"), doc(1)),
                (private("a.md", b"edited", safe()), doc(1)),
            ],
            &none(),
        );
        let files = corpus.by_id(&docid(1));
        assert_eq!(files.len(), 2);
        for file in files {
            assert!(matches!(
                reasons_of(file).as_slice(),
                [Invalidity::DuplicateId { .. }]
            ));
        }
        assert!(corpus.publishing().is_empty());
    }

    #[test]
    fn a_published_document_with_a_private_file_left_is_read_from_the_public_one() {
        for private_bytes in [&b"same"[..], &b"edited"[..]] {
            let published = BTreeSet::from([docid(1)]);
            let corpus = Corpus::build(
                [
                    // The private copy says another alias; it must not be read as state.
                    (
                        private("a.md", private_bytes, safe()),
                        aliased(1, "left-behind"),
                    ),
                    (public("a.md", b"same"), aliased(1, "kept")),
                ],
                &published,
            );
            let files = corpus.by_id(&docid(1));
            assert_eq!(files.len(), 1);
            assert!(files[0].standing().is_valid());
            assert_eq!(files[0].location().visibility(), Visibility::Public);
            assert_eq!(corpus.by_alias(&"kept".parse().unwrap()).len(), 1);
            assert!(corpus.by_alias(&"left-behind".parse().unwrap()).is_empty());

            let form = corpus.publishing_of(&docid(1)).unwrap();
            assert_eq!(form.form(), PublishForm::PrivateCopyLeft);
            assert_eq!(
                form.private_copy()
                    .document()
                    .header()
                    .alias()
                    .map(Alias::as_str),
                Some("left-behind")
            );
        }
    }

    #[test]
    fn publication_is_only_recognised_for_one_public_and_one_private_file() {
        let published = BTreeSet::from([docid(1)]);
        let corpus = Corpus::build(
            [
                (public("a.md", b"x"), doc(1)),
                (public("b.md", b"x"), doc(1)),
                (private("c.md", b"x", safe()), doc(1)),
            ],
            &published,
        );
        assert_eq!(corpus.by_id(&docid(1)).len(), 3);
        assert!(corpus.publishing().is_empty());
        assert!(
            corpus
                .by_id(&docid(1))
                .iter()
                .all(|file| !file.standing().is_valid())
        );
    }

    #[test]
    fn two_documents_with_one_alias_are_both_at_fault() {
        let corpus = Corpus::build(
            [
                (public("a.md", b"a"), aliased(1, "spec/cache")),
                (public("b.md", b"b"), aliased(2, "spec/cache")),
                (public("c.md", b"c"), aliased(3, "spec/other")),
            ],
            &none(),
        );
        let alias: Alias = "spec/cache".parse().unwrap();
        let shared = corpus.by_alias(&alias);
        assert_eq!(shared.len(), 2);
        for file in shared {
            let reasons = reasons_of(file);
            let [
                Invalidity::DuplicateAlias {
                    alias: found,
                    others,
                },
            ] = reasons.as_slice()
            else {
                panic!("expected one DuplicateAlias");
            };
            assert_eq!(found, &alias);
            assert_eq!(others.len(), 1);
        }
        assert!(corpus.by_id(&docid(3))[0].standing().is_valid());
    }

    #[test]
    fn one_id_claimed_twice_with_one_alias_is_not_also_a_shared_alias() {
        let corpus = Corpus::build(
            [
                (public("a.md", b"1"), aliased(1, "x")),
                (public("b.md", b"2"), aliased(1, "x")),
            ],
            &none(),
        );
        for file in corpus.documents() {
            assert!(matches!(
                reasons_of(file).as_slice(),
                [Invalidity::DuplicateId { .. }]
            ));
        }
    }

    #[test]
    fn a_private_document_that_git_tracks_or_would_add_is_exposed() {
        for place in [GitPlace::Untracked, GitPlace::Tracked] {
            let corpus = Corpus::build(
                [(private("p.md", b"p", Knowable::Known(place)), doc(1))],
                &none(),
            );
            let file = &corpus.documents()[0];
            assert_eq!(
                reasons_of(file),
                [Invalidity::PrivateExposed { place }],
                "{place:?}"
            );
        }
    }

    #[test]
    fn a_private_document_outside_git_or_ignored_is_valid() {
        for place in [GitPlace::Outside, GitPlace::Ignored] {
            let corpus = Corpus::build(
                [(private("p.md", b"p", Knowable::Known(place)), doc(1))],
                &none(),
            );
            assert!(corpus.documents()[0].standing().is_valid(), "{place:?}");
            assert!(corpus.notices().is_empty());
        }
    }

    #[test]
    fn a_private_document_whose_place_is_unknown_is_valid_with_a_notice() {
        let location = at(Visibility::Private, "p.md", Knowable::Unknown);
        let corpus = Corpus::build([(facts(location.clone(), b"p"), doc(1))], &none());
        assert!(corpus.documents()[0].standing().is_valid());
        assert_eq!(corpus.notices(), [Notice::GitPlaceUnknown { location }]);
    }

    #[test]
    fn an_exposed_private_document_is_also_listed_as_an_exposed_file() {
        let corpus = Corpus::build(
            [(
                private("p.md", b"p", Knowable::Known(GitPlace::Tracked)),
                doc(1),
            )],
            &none(),
        );
        let [exposed] = corpus.exposed() else {
            panic!("expected one exposed file");
        };
        assert_eq!(exposed.location().path().as_str(), "p.md");
        assert_eq!(exposed.place(), GitPlace::Tracked);
    }

    #[test]
    fn a_private_file_that_is_not_a_document_is_still_exposed() {
        let unparsed = || Reading::Unparsed {
            id: None,
            reasons: NonEmptyVec::new(UnparsedReason::MergeConflictMarker, Vec::new()),
        };
        for place in [GitPlace::Untracked, GitPlace::Tracked] {
            let git = Knowable::Known(place);
            let corpus = Corpus::build(
                [
                    (
                        private("n.md", b"n", git),
                        Reading::Unidentified { kind: None },
                    ),
                    (private("x.md", b"x", git), unparsed()),
                    // Not private: never exposed, whatever Git makes of it.
                    (
                        facts(at(Visibility::Public, "y.md", git), b"y"),
                        Reading::Unidentified { kind: None },
                    ),
                ],
                &none(),
            );
            assert!(corpus.documents().is_empty());
            let found: Vec<(&str, GitPlace)> = corpus
                .exposed()
                .iter()
                .map(|file| (file.location().path().as_str(), file.place()))
                .collect();
            assert_eq!(found, [("n.md", place), ("x.md", place)], "{place:?}");
        }
    }

    #[test]
    fn a_private_file_that_is_not_a_document_in_an_unknown_place_gives_a_notice() {
        let location = at(Visibility::Private, "n.md", Knowable::Unknown);
        let corpus = Corpus::build(
            [(
                facts(location.clone(), b"n"),
                Reading::Unidentified { kind: None },
            )],
            &none(),
        );
        assert!(corpus.exposed().is_empty());
        assert_eq!(corpus.notices(), [Notice::GitPlaceUnknown { location }]);
    }

    #[test]
    fn the_private_copy_of_a_publication_cut_short_is_still_exposed() {
        for published in [none(), BTreeSet::from([docid(1)])] {
            let corpus = Corpus::build(
                [
                    (public("a.md", b"same"), doc(1)),
                    (
                        private("a.md", b"same", Knowable::Known(GitPlace::Untracked)),
                        doc(1),
                    ),
                ],
                &published,
            );
            assert!(corpus.publishing_of(&docid(1)).is_some());
            assert!(corpus.by_id(&docid(1))[0].standing().is_valid());
            let [exposed] = corpus.exposed() else {
                panic!("expected the private copy to be exposed");
            };
            assert_eq!(exposed.location().visibility(), Visibility::Private);
            assert_eq!(exposed.place(), GitPlace::Untracked);
        }
    }

    #[test]
    fn a_public_document_is_not_exposed_by_being_tracked() {
        let corpus = Corpus::build(
            [(
                facts(
                    at(
                        Visibility::Public,
                        "a.md",
                        Knowable::Known(GitPlace::Untracked),
                    ),
                    b"a",
                ),
                doc(1),
            )],
            &none(),
        );
        assert!(corpus.documents()[0].standing().is_valid());
    }

    #[test]
    fn every_reason_is_kept_when_several_apply() {
        let corpus = Corpus::build(
            [
                (
                    private("a.md", b"1", Knowable::Known(GitPlace::Tracked)),
                    aliased(1, "x"),
                ),
                (public("b.md", b"2"), aliased(2, "x")),
            ],
            &none(),
        );
        let private_file = &corpus.by_id(&docid(1))[0];
        assert!(matches!(
            reasons_of(private_file).as_slice(),
            [
                Invalidity::DuplicateAlias { .. },
                Invalidity::PrivateExposed { .. }
            ]
        ));
    }

    #[test]
    fn a_file_without_an_id_is_a_candidate_and_not_a_document() {
        let corpus = Corpus::build(
            [(
                public("n.md", b"n"),
                Reading::Unidentified {
                    kind: Some(DocKind::Finding),
                },
            )],
            &none(),
        );
        assert!(corpus.documents().is_empty());
        assert_eq!(corpus.candidates().len(), 1);
        assert_eq!(corpus.candidates()[0].kind(), Some(DocKind::Finding));
        assert_eq!(
            corpus.candidates()[0].facts().location().path().as_str(),
            "n.md"
        );
    }

    #[test]
    fn a_file_that_cannot_be_read_is_listed_with_its_reasons() {
        let reasons = NonEmptyVec::new(UnparsedReason::MergeConflictMarker, Vec::new());
        let corpus = Corpus::build(
            [(
                public("x.md", b"<<<<<<<"),
                Reading::Unparsed {
                    id: None,
                    reasons: reasons.clone(),
                },
            )],
            &none(),
        );
        assert!(corpus.documents().is_empty());
        assert_eq!(corpus.unparsed().len(), 1);
        assert_eq!(corpus.unparsed()[0].reasons(), &reasons);
        assert_eq!(corpus.unparsed()[0].id(), None);
    }

    #[test]
    fn a_file_that_cannot_be_read_keeps_the_id_it_still_shows_and_is_no_document() {
        let reasons = NonEmptyVec::new(UnparsedReason::MergeConflictMarker, Vec::new());
        let corpus = Corpus::build(
            [(
                public("x.md", b"<<<<<<<"),
                Reading::Unparsed {
                    id: Some(docid(1)),
                    reasons,
                },
            )],
            &none(),
        );
        assert_eq!(corpus.unparsed()[0].id(), Some(docid(1)));
        assert!(corpus.by_id(&docid(1)).is_empty(), "not a document");
    }

    #[test]
    fn the_order_the_files_arrive_in_changes_nothing() {
        let files = || {
            vec![
                (public("a.md", b"1"), aliased(1, "x")),
                (public("b.md", b"2"), aliased(2, "x")),
                (public("c.md", b"3"), doc(3)),
                (public("c.md", b"3"), doc(3)),
                (public("n.md", b"n"), Reading::Unidentified { kind: None }),
            ]
        };
        let forward = Corpus::build(files(), &none());
        let backward = Corpus::build(files().into_iter().rev(), &none());
        assert_eq!(forward, backward);
    }

    #[test]
    fn lookups_that_find_nothing_say_so() {
        let corpus = Corpus::build([(public("a.md", b"a"), doc(1))], &none());
        assert!(corpus.by_id(&docid(2)).is_empty());
        assert!(corpus.by_alias(&"nothing".parse().unwrap()).is_empty());
        assert!(corpus.publishing_of(&docid(1)).is_none());
    }

    fn design_reading(n: u128, items: Vec<(u32, WorkItem)>) -> Reading {
        let design = design_of(10 + n, 1, items);
        let header = Header::new(docid(10 + n), DocKind::Design, None, BTreeSet::new(), None);
        Reading::resolved(Document::new(header, Entity::Design(design)).unwrap())
    }

    #[test]
    fn a_design_whose_items_each_carry_an_id_is_valid() {
        let corpus = Corpus::build(
            [(
                public("d.md", b"d"),
                design_reading(
                    1,
                    vec![(1, identified(1, "a", &[])), (2, identified(2, "b", &[]))],
                ),
            )],
            &none(),
        );
        assert!(corpus.by_id(&docid(11))[0].standing().is_valid());
    }

    #[test]
    fn a_design_with_an_unidentified_item_is_invalid() {
        let corpus = Corpus::build(
            [(
                public("d.md", b"d"),
                design_reading(1, vec![(1, identified(1, "a", &[])), (3, item("b", &[]))]),
            )],
            &none(),
        );
        let files = corpus.by_id(&docid(11));
        assert_eq!(
            reasons_of(&files[0]),
            vec![Invalidity::Item(ItemIssue::UnidentifiedItem {
                number: num(3)
            })]
        );
    }

    #[test]
    fn a_design_with_one_item_id_twice_is_invalid() {
        let corpus = Corpus::build(
            [(
                public("d.md", b"d"),
                design_reading(
                    1,
                    vec![(1, identified(4, "a", &[])), (2, identified(4, "b", &[]))],
                ),
            )],
            &none(),
        );
        let files = corpus.by_id(&docid(11));
        assert_eq!(
            reasons_of(&files[0]),
            vec![Invalidity::Item(ItemIssue::DuplicateItemId {
                id: iid(4),
                numbers: NonEmptyVec::new(num(1), vec![num(2)]),
            })]
        );
    }

    #[test]
    fn the_faults_of_the_items_come_with_the_other_reasons() {
        let corpus = Corpus::build(
            [
                (
                    public("a.md", b"one"),
                    design_reading(1, vec![(1, item("a", &[]))]),
                ),
                (
                    public("b.md", b"two"),
                    design_reading(1, vec![(1, item("a", &[]))]),
                ),
            ],
            &none(),
        );
        for file in corpus.by_id(&docid(11)) {
            let reasons = reasons_of(file);
            assert_eq!(reasons.len(), 2, "{reasons:?}");
            assert!(matches!(reasons[0], Invalidity::DuplicateId { .. }));
            assert!(matches!(reasons[1], Invalidity::Item(_)));
        }
    }

    #[test]
    fn an_item_id_shared_by_two_designs_makes_both_invalid() {
        let corpus = Corpus::build(
            [
                (
                    public("a.md", b"a"),
                    design_reading(1, vec![(1, identified(5, "a", &[]))]),
                ),
                (
                    public("b.md", b"b"),
                    design_reading(2, vec![(1, identified(5, "b", &[]))]),
                ),
            ],
            &none(),
        );
        for (n, other) in [(11, "b.md"), (12, "a.md")] {
            let reasons = reasons_of(&corpus.by_id(&docid(n))[0]);
            let [Invalidity::DuplicateItemIdAcross { id, others }] = reasons.as_slice() else {
                panic!("expected one across-duplicate, got {reasons:?}");
            };
            assert_eq!(*id, iid(5).entity());
            assert_eq!(paths(others), vec![other]);
        }
    }

    #[test]
    fn an_item_id_equal_to_a_document_id_makes_both_invalid() {
        let corpus = Corpus::build(
            [
                (
                    public("d.md", b"d"),
                    design_reading(1, vec![(1, identified(7, "a", &[]))]),
                ),
                (public("s.md", b"s"), doc(7)),
            ],
            &none(),
        );
        for (n, other) in [(11, "s.md"), (7, "d.md")] {
            let reasons = reasons_of(&corpus.by_id(&docid(n))[0]);
            let [Invalidity::DuplicateItemIdAcross { others, .. }] = reasons.as_slice() else {
                panic!("expected one across-duplicate, got {reasons:?}");
            };
            assert_eq!(paths(others), vec![other]);
        }
    }

    #[test]
    fn an_item_id_equal_to_its_own_designs_id_is_only_the_item_issue() {
        let corpus = Corpus::build(
            [(
                public("d.md", b"d"),
                design_reading(1, vec![(1, identified(11, "a", &[]))]),
            )],
            &none(),
        );
        let reasons = reasons_of(&corpus.by_id(&docid(11))[0]);
        assert!(matches!(
            reasons.as_slice(),
            [Invalidity::Item(ItemIssue::ItemIdIsDesignId { .. })]
        ));
    }

    #[test]
    fn an_item_id_twice_in_one_design_is_only_the_item_issue() {
        let corpus = Corpus::build(
            [(
                public("d.md", b"d"),
                design_reading(
                    1,
                    vec![(1, identified(4, "a", &[])), (2, identified(4, "b", &[]))],
                ),
            )],
            &none(),
        );
        let reasons = reasons_of(&corpus.by_id(&docid(11))[0]);
        assert!(matches!(
            reasons.as_slice(),
            [Invalidity::Item(ItemIssue::DuplicateItemId { .. })]
        ));
    }

    #[test]
    fn the_private_copy_of_a_publication_is_not_a_second_design() {
        let reading = || design_reading(1, vec![(1, identified(5, "a", &[]))]);
        for published in [none(), BTreeSet::from([docid(11)])] {
            let corpus = Corpus::build(
                [
                    (public("d.md", b"d"), reading()),
                    (private("p.md", b"d", safe()), reading()),
                ],
                &published,
            );
            assert_eq!(corpus.publishing().len(), 1);
            assert!(corpus.by_id(&docid(11))[0].standing().is_valid());
        }
    }

    #[test]
    fn a_spec_has_no_item_faults() {
        let corpus = Corpus::build([(public("a.md", b"a"), doc(1))], &none());
        assert!(corpus.by_id(&docid(1))[0].standing().is_valid());
    }

    #[test]
    fn a_document_needs_a_header_that_agrees_with_its_entity() {
        let wrong_kind = Header::new(docid(1), DocKind::Design, None, BTreeSet::new(), None);
        assert_eq!(
            Document::new(wrong_kind, spec(1)),
            Err(DocumentError::KindMismatch {
                header: DocKind::Design,
                entity: DocKind::Spec,
            })
        );
        let wrong_id = Header::new(docid(2), DocKind::Spec, None, BTreeSet::new(), None);
        assert_eq!(
            Document::new(wrong_id, spec(1)),
            Err(DocumentError::IdMismatch {
                header: docid(2),
                entity: docid(1),
            })
        );
    }
}
