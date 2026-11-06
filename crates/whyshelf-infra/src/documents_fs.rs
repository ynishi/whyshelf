//! The document files of both roots: [`DocumentSource`] over the file system.
//!
//! Read only. For every Markdown file under the scanned directories of the public
//! root and of the private root, [`FsDocuments`] gives where it sits, the hash of
//! its bytes **as they are on disk** ([`FileFacts`]), and what reading it gave with
//! its normalized text ([`RawDocument`]). The bytes' hash is taken before the text is
//! decoded or normalized, so a file with CRLF line ends or trailing spaces differs
//! from its copy that has none, which the normalized text would hide.
//!
//! ```text
//!   file bytes --- BytesHash --------------------------------> FileFacts
//!        |
//!        +-- UTF-8? --- normalize (markdown::normalize) -------> RawDocument.text
//!                           |
//!                           +-- `id:` taken out, parse::read_in --> RawDoc
//!                                |
//!                                +-- references of a Spec and a Design resolved
//!                                    among the files of both roots --> Reading
//! ```
//!
//! # What a file reads as
//!
//! | the file | [`Reading`] |
//! |---|---|
//! | not UTF-8, a merge-conflict marker, front matter or keys that do not parse, a reference that names nothing or the wrong kind | `Unparsed`, with every reason and the `id:` the file shows if it shows one |
//! | reads, and has no `id:` | `Unidentified`, with the kind the file shows |
//! | reads, and has one `id:` | `Document` |
//!
//! A file that has an `id:` line that is no identifier, or two different ones, is
//! `Unparsed` for that.
//!
//! # The kind
//!
//! The `kind:` key of the front matter if there is one; else the directory the file is
//! under, if the root says a directory holds one kind ([`DocumentRoot::with_kind_dir`]);
//! else what the keys of the file name ([`DocKind::guess`](whyshelf_core::domain::event::DocKind::guess)),
//! as before. A `kind:` and a directory that disagree make the file unparsed (there
//! is no reason of its own for it in the domain yet: it is an invalid `kind`).
//!
//! # References
//!
//! `grounds` of a Spec and `spec` of a Design name documents by identifier or alias
//! (`private:<id>` for a private one). They are looked up among the files read here,
//! by the rule of [`EntityRef::resolve`](whyshelf_core::domain::reference::EntityRef::resolve) and [`bind`]:
//!
//! - an identifier or alias without a mark is seen from the file's own root: a public
//!   file sees public files, a private file sees both;
//! - `private:<id>` is looked up by identifier in either root, and where it is not
//!   found it is dangling if the private root is configured and unresolved if it is not;
//! - an identifier shared by files of two kinds is a conflict;
//! - a document is never dropped from a Spec for being dangling: the Spec is built
//!   with the identifier it wrote, and **how it wrote it** (dangling, unresolved)
//!   goes to [`Documents::written`], for the evaluation.
//!
//! The port [`DocumentSource::list`] returns the files and drops `written`, which has
//! no place in it; [`FsDocuments::read_all`] returns both.
//!
//! A Finding's `revision` is accepted when it is a full 40-digit revision (the rule of
//! the revision port), lowercased.

use std::collections::{BTreeMap, BTreeSet};
use std::io;
use std::path::PathBuf;

use whyshelf_core::app::ports::{DocumentSource, UnreadablePath};
use whyshelf_core::app::raw::{NormalizedText, RawDoc, RawDocument};
use whyshelf_core::domain::corpus::{
    Document, Entity, FileFacts, Header, Location, Reading, Visibility,
};
use whyshelf_core::domain::design::Design;
use whyshelf_core::domain::error::EntityError;
use whyshelf_core::domain::evaluate::Written;
use whyshelf_core::domain::event::{DocKind, EntityKind};
use whyshelf_core::domain::finding::Finding;
use whyshelf_core::domain::hash::BytesHash;
use whyshelf_core::domain::id::{Alias, DesignId, DocId, EntityId, FindingId, SpecId};
use whyshelf_core::domain::knowable::Knowable;
use whyshelf_core::domain::location::RelPath;
use whyshelf_core::domain::nonempty::NonEmptyVec;
use whyshelf_core::domain::policy::PinVocabulary;
use whyshelf_core::domain::reference::{
    Bound, ById, Directory, LocalRef, Located, Lookup, Reach, Ref, bind,
};
use whyshelf_core::domain::revision::RevisionId;
use whyshelf_core::domain::spec::Spec;
use whyshelf_core::domain::state::UnparsedReason;

use crate::blocking::{self, BlockingError};
use crate::git::is_full_hash;
use crate::{markdown, parse};

/// Why the documents could not be listed or read.
#[derive(Debug, thiserror::Error)]
pub enum DocumentsError {
    /// A directory tree could not be listed.
    #[error("cannot list the document files")]
    List(#[source] BlockingError),
    /// A file exists but could not be read.
    #[error("cannot read the file {path}")]
    Read {
        /// The file.
        path: RelPath,
        /// The operating system's error.
        #[source]
        source: io::Error,
    },
}

/// One root of documents: a directory, where in it documents are looked for, and
/// which directories hold which kind.
#[derive(Debug, Clone)]
pub struct DocumentRoot {
    dir: PathBuf,
    scan: Vec<RelPath>,
    kinds: Vec<(RelPath, DocKind)>,
}

impl DocumentRoot {
    /// The root at `dir`, with Markdown files looked for under `scan` (paths relative
    /// to it; a path may be a directory or one file).
    pub fn new(dir: PathBuf, scan: Vec<RelPath>) -> Self {
        Self {
            dir,
            scan,
            kinds: Vec::new(),
        }
    }

    /// Say that the files under `dir` (relative to the root) are documents of `kind`.
    pub fn with_kind_dir(mut self, dir: RelPath, kind: DocKind) -> Self {
        self.kinds.push((dir, kind));
        self
    }

    /// The kind the place of a file says, the deepest directory first.
    fn placed(&self, path: &RelPath) -> Option<DocKind> {
        self.kinds
            .iter()
            .filter(|(dir, _)| {
                path.as_str()
                    .strip_prefix(dir.as_str())
                    .is_some_and(|rest| rest.starts_with('/'))
            })
            .max_by_key(|(dir, _)| dir.as_str().len())
            .map(|(_, kind)| *kind)
    }
}

/// What reading every file gave.
#[derive(Debug, Clone, Default)]
pub struct Documents {
    /// Every file, in the order of root (public first) and path.
    pub files: Vec<(FileFacts, RawDocument)>,
    /// How each document wrote the upstream identifiers it holds that no state can
    /// answer for.
    pub written: BTreeMap<DocId, Written>,
    /// Files and directories the listing found and could not name as paths.
    pub unreadable: Vec<UnreadablePath>,
}

/// The documents of the public root and of the private one.
#[derive(Debug, Clone)]
pub struct FsDocuments {
    public: DocumentRoot,
    private: Option<DocumentRoot>,
    vocabulary: PinVocabulary,
}

impl FsDocuments {
    /// The documents of the public root, and of the private root if the person has
    /// one here.
    pub fn new(public: DocumentRoot, private: Option<DocumentRoot>) -> Self {
        Self {
            public,
            private,
            vocabulary: PinVocabulary::default(),
        }
    }

    /// Read the pins of a Spec and a Design in this vocabulary, the project's.
    pub fn with_vocabulary(mut self, vocabulary: PinVocabulary) -> Self {
        self.vocabulary = vocabulary;
        self
    }

    fn roots(&self) -> impl Iterator<Item = (Visibility, &DocumentRoot)> {
        std::iter::once((Visibility::Public, &self.public))
            .chain(self.private.iter().map(|root| (Visibility::Private, root)))
    }

    /// Every file, once, with how its references were written.
    pub async fn read_all(&self) -> Result<Documents, DocumentsError> {
        let mut unreadable: Vec<UnreadablePath> = Vec::new();
        let mut read: Vec<Read> = Vec::new();
        for (visibility, root) in self.roots() {
            let listing = blocking::list_markdown(root.dir.clone(), root.scan.clone())
                .await
                .map_err(DocumentsError::List)?;
            for problem in listing.unreadable() {
                if !unreadable.contains(problem) {
                    unreadable.push(problem.clone());
                }
            }
            for stat in listing.files() {
                let path = stat.path().clone();
                let bytes = match tokio::fs::read(root.dir.join(path.as_str())).await {
                    Ok(bytes) => bytes,
                    // Gone since it was listed.
                    Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
                    Err(source) => return Err(DocumentsError::Read { path, source }),
                };
                read.push(read_file(root, visibility, path, &bytes, &self.vocabulary));
            }
        }

        let index = Index::of(&read, self.private.is_some());
        let mut written: BTreeMap<DocId, Written> = BTreeMap::new();
        let mut files = Vec::with_capacity(read.len());
        for file in read {
            let (reading, wrote) = file.settle(&index);
            if let (Some(id), Some(wrote)) = (file.shown_id, wrote) {
                written.insert(id, wrote);
            }
            let location = Location::new(file.visibility, file.path, Knowable::Unknown);
            files.push((
                FileFacts::new(location, file.bytes),
                RawDocument::new(reading, file.text),
            ));
        }
        Ok(Documents {
            files,
            written,
            unreadable,
        })
    }
}

impl DocumentSource for FsDocuments {
    type Error = DocumentsError;

    async fn list(&self) -> Result<Vec<(FileFacts, RawDocument)>, DocumentsError> {
        Ok(self.read_all().await?.files)
    }
}

// --- one file ---------------------------------------------------------------------------

/// What reading one file's own text gave, before its references are looked up.
enum Parsed {
    /// It does not read.
    Unparsed(NonEmptyVec<UnparsedReason>),
    /// It reads and has no `id:`.
    Unidentified(DocKind),
    /// It reads and has this `id:`.
    Identified {
        id: DocId,
        raw: Box<RawDoc>,
        header: Header,
    },
}

/// One file as read, waiting for the references of the others.
struct Read {
    visibility: Visibility,
    path: RelPath,
    bytes: BytesHash,
    text: NormalizedText,
    /// The `id:` the file shows, whether or not it reads.
    shown_id: Option<DocId>,
    parsed: Parsed,
}

fn unparsed(reason: UnparsedReason) -> Parsed {
    Parsed::Unparsed(NonEmptyVec::new(reason, Vec::new()))
}

fn invalid(field: &str, detail: impl Into<String>) -> UnparsedReason {
    UnparsedReason::InvalidField {
        field: field.to_owned(),
        detail: detail.into(),
    }
}

/// The text of the file with its `id:` lines taken out of the front matter, since
/// the identity is no key of the content that is read (and hashed).
fn without_id(text: &str) -> NormalizedText {
    let mut out = String::with_capacity(text.len());
    let mut lines = text.split_inclusive('\n');
    if let Some(first) = lines.next() {
        out.push_str(first);
        if first.trim_end() == "---" {
            let mut in_front = true;
            for line in lines {
                if in_front && line.trim_end() == "---" {
                    in_front = false;
                }
                if in_front && line.starts_with("id:") {
                    continue;
                }
                out.push_str(line);
            }
            return NormalizedText::new(out);
        }
        for line in lines {
            out.push_str(line);
        }
    }
    NormalizedText::new(out)
}

/// The `kind:` the front matter says, and whether it has an `id:` line.
fn probe(text: &str) -> (Option<String>, bool) {
    let has_id = text
        .lines()
        .skip(1)
        .take_while(|line| line.trim_end() != "---")
        .any(|line| line.starts_with("id:"))
        && text.starts_with("---");
    let kind = markdown::split_front_matter(text)
        .ok()
        .and_then(|parts| serde_norway::from_str::<serde_norway::Value>(parts.yaml).ok())
        .and_then(|value| match value {
            serde_norway::Value::Mapping(map) => map
                .get("kind")
                .and_then(|kind| kind.as_str().map(str::to_owned)),
            _ => None,
        });
    (kind, has_id)
}

fn read_file(
    root: &DocumentRoot,
    visibility: Visibility,
    path: RelPath,
    bytes: &[u8],
    vocabulary: &PinVocabulary,
) -> Read {
    let hash = BytesHash::of(bytes);
    let finish = |text: NormalizedText, shown_id, parsed| Read {
        visibility,
        path: path.clone(),
        bytes: hash,
        text,
        shown_id,
        parsed,
    };
    let text = match std::str::from_utf8(bytes) {
        Ok(text) => markdown::normalize(text),
        Err(_) => {
            let lossy = markdown::normalize(&String::from_utf8_lossy(bytes));
            let reason = UnparsedReason::MalformedFrontmatter {
                detail: "the file is not valid UTF-8".to_owned(),
            };
            let shown = markdown::seen_id(lossy.as_str());
            return finish(lossy, shown, unparsed(reason));
        }
    };
    let shown_id = markdown::seen_id(text.as_str());
    let (front_kind, has_id_line) = probe(text.as_str());

    let front_kind = match front_kind.as_deref().map(|name| {
        DocKind::from_name(name).ok_or_else(|| {
            invalid(
                "kind",
                format!(
                    "`{name}` is none of {}",
                    DocKind::names().collect::<Vec<_>>().join(", ")
                ),
            )
        })
    }) {
        None => None,
        Some(Ok(kind)) => Some(kind),
        Some(Err(reason)) => return finish(text, shown_id, unparsed(reason)),
    };
    let placed = root.placed(&path);
    if let (Some(said), Some(placed)) = (front_kind, placed)
        && said != placed
    {
        let reason = invalid(
            "kind",
            format!(
                "the file says `{}` and its place says `{}`",
                said.name(),
                placed.name()
            ),
        );
        return finish(text, shown_id, unparsed(reason));
    }

    let read = parse::read_in(
        &without_id(text.as_str()),
        front_kind.or(placed),
        vocabulary,
    );
    let doc = match read.outcome {
        Ok(doc) => doc,
        Err(reasons) => return finish(text, shown_id, Parsed::Unparsed(reasons)),
    };
    let kind = doc.kind();
    let Some(id) = shown_id else {
        let parsed = if has_id_line {
            unparsed(invalid(
                "id",
                "the `id:` is no identifier, or the file has two different ones",
            ))
        } else {
            Parsed::Unidentified(kind)
        };
        return finish(text, None, parsed);
    };
    let classification = read.classification;
    let header = Header::new(
        id,
        kind,
        classification.alias,
        classification.tags.unwrap_or_default(),
        classification.owner,
    );
    finish(
        text,
        Some(id),
        Parsed::Identified {
            id,
            raw: Box::new(doc),
            header,
        },
    )
}

// --- looking references up ----------------------------------------------------------------

/// What a reference finds: which entity, of which kind.
#[derive(Debug, Clone, Copy)]
struct Found {
    id: EntityId,
    kind: EntityKind,
}

impl Located for Found {
    fn id(&self) -> EntityId {
        self.id
    }

    fn kind(&self) -> EntityKind {
        self.kind
    }
}

fn entity_kind(kind: DocKind) -> EntityKind {
    match kind {
        DocKind::Finding => EntityKind::Finding,
        DocKind::Spec => EntityKind::Spec,
        DocKind::Design => EntityKind::Design,
    }
}

/// The documents read, by identifier and by alias, per root.
struct Index {
    by_id: BTreeMap<EntityId, Vec<(Visibility, EntityKind)>>,
    by_alias: BTreeMap<Alias, BTreeSet<(Visibility, EntityId)>>,
    private_here: bool,
}

impl Index {
    fn of(files: &[Read], private_here: bool) -> Self {
        let mut by_id: BTreeMap<EntityId, Vec<(Visibility, EntityKind)>> = BTreeMap::new();
        let mut by_alias: BTreeMap<Alias, BTreeSet<(Visibility, EntityId)>> = BTreeMap::new();
        for file in files {
            if let Parsed::Identified { id, header, .. } = &file.parsed {
                by_id
                    .entry(id.entity())
                    .or_default()
                    .push((file.visibility, entity_kind(header.kind())));
                if let Some(alias) = header.alias() {
                    by_alias
                        .entry(alias.clone())
                        .or_default()
                        .insert((file.visibility, id.entity()));
                }
            }
        }
        Self {
            by_id,
            by_alias,
            private_here,
        }
    }

    /// What one identifier is, among the files `visible` allows.
    fn at(&self, id: EntityId, visible: impl Fn(Visibility) -> bool) -> Lookup<Found> {
        let kinds: BTreeSet<EntityKind> = self
            .by_id
            .get(&id)
            .into_iter()
            .flatten()
            .filter(|(root, _)| visible(*root))
            .map(|(_, kind)| *kind)
            .collect();
        let mut kinds = kinds.into_iter();
        match (kinds.next(), kinds.next()) {
            (None, _) => Lookup::Missing,
            (Some(kind), None) => Lookup::Found(Found { id, kind }),
            // Two kinds under one identifier: it is known and nothing more.
            (Some(_), Some(_)) => Lookup::Conflicted(id),
        }
    }

    /// The questions of a reference written in a file of this root.
    fn seen_from(&self, from: Visibility) -> Scoped<'_> {
        Scoped { index: self, from }
    }
}

struct Scoped<'a> {
    index: &'a Index,
    from: Visibility,
}

impl Scoped<'_> {
    /// A public file sees public files; a private file sees both.
    fn sees(&self, root: Visibility) -> bool {
        self.from == Visibility::Private || root == Visibility::Public
    }
}

impl Directory<Found> for Scoped<'_> {
    fn local(&self, reference: &LocalRef) -> Lookup<Found> {
        match reference {
            LocalRef::Id(id) => self.index.at(*id, |root| self.sees(root)),
            LocalRef::Alias(alias) => {
                let ids: BTreeSet<EntityId> = self
                    .index
                    .by_alias
                    .get(alias)
                    .into_iter()
                    .flatten()
                    .filter(|(root, _)| self.sees(*root))
                    .map(|(_, id)| *id)
                    .collect();
                let mut ids = ids.into_iter();
                match (ids.next(), ids.next()) {
                    (None, _) => Lookup::Missing,
                    (Some(id), None) => self.index.at(id, |root| self.sees(root)),
                    (Some(_), Some(_)) => Lookup::Ambiguous,
                }
            }
        }
    }

    fn by_id(&self, id: EntityId) -> ById<Found> {
        match self.index.at(id, |_| true) {
            Lookup::Found(found) => ById::Found(found),
            Lookup::Conflicted(id) => ById::Conflicted(id),
            Lookup::Missing | Lookup::Ambiguous => ById::Missing,
        }
    }

    fn private_reach(&self) -> Reach {
        if self.index.private_here {
            Reach::Available
        } else {
            Reach::Absent
        }
    }
}

/// What a reference in a key comes to, in the root of the file that wrote it.
fn bound(directory: &Scoped<'_>, reference: &Ref, field: &str, expected: EntityKind) -> Bound {
    match reference.entity_of(field) {
        Ok(entity) => bind(entity.resolve(directory), &entity, expected),
        Err(refused) => Bound::Unparsed(refused),
    }
}

impl Read {
    /// The reading of this file, now that the others are known, and how it wrote its
    /// upstream identifiers.
    fn settle(&self, index: &Index) -> (Reading, Option<Written>) {
        match &self.parsed {
            Parsed::Unparsed(reasons) => (
                Reading::Unparsed {
                    id: self.shown_id,
                    reasons: reasons.clone(),
                },
                None,
            ),
            Parsed::Unidentified(kind) => (Reading::Unidentified { kind: Some(*kind) }, None),
            Parsed::Identified { id, raw, header } => {
                let directory = index.seen_from(self.visibility);
                match build(&directory, id.entity(), (**raw).clone()) {
                    Ok((entity, written)) => match Document::new(header.clone(), entity) {
                        Ok(document) => (Reading::Document(document), Some(written)),
                        Err(error) => (
                            Reading::Unparsed {
                                id: Some(*id),
                                reasons: NonEmptyVec::new(
                                    invalid("id", error.to_string()),
                                    Vec::new(),
                                ),
                            },
                            None,
                        ),
                    },
                    Err(reasons) => (
                        Reading::Unparsed {
                            id: Some(*id),
                            reasons,
                        },
                        None,
                    ),
                }
            }
        }
    }
}

/// Build the entity a file describes, with how it wrote its upstream identifiers, or
/// the reasons it cannot be built.
fn build(
    directory: &Scoped<'_>,
    id: EntityId,
    doc: RawDoc,
) -> Result<(Entity, Written), NonEmptyVec<UnparsedReason>> {
    let one = |reason| NonEmptyVec::new(reason, Vec::new());
    match doc {
        RawDoc::Finding(raw) => {
            let revision = match raw.revision() {
                None => None,
                Some(text) => {
                    let trimmed = text.as_str().trim();
                    let checked = is_full_hash(trimmed)
                        .then(|| RevisionId::from_validated(trimmed.to_ascii_lowercase()).ok())
                        .flatten();
                    match checked {
                        Some(revision) => Some(revision),
                        None => {
                            return Err(one(UnparsedReason::RevisionRejected {
                                text: text.as_str().to_owned(),
                            }));
                        }
                    }
                }
            };
            let finding: Finding = raw.build(FindingId::new(id), revision).map_err(one)?;
            Ok((Entity::Finding(finding), Written::default()))
        }
        RawDoc::Spec(raw) => {
            let mut grounds = Vec::new();
            let mut written = Written::default();
            let mut reasons = Vec::new();
            for reference in raw.grounds().iter() {
                // An identifier is available in each case but the last: the Spec is
                // built with it, and the evaluation judges what stands behind it. No
                // ground is dropped, which would change the hash in silence.
                match bound(directory, reference, "grounds", EntityKind::Finding) {
                    Bound::Id(found) | Bound::Conflicted(found) => {
                        grounds.push(FindingId::new(found));
                    }
                    Bound::Dangling(found) => {
                        written.dangling.insert(found);
                        grounds.push(FindingId::new(found));
                    }
                    Bound::Unresolved(unreadable) => {
                        written.unresolved.insert(unreadable);
                        grounds.push(FindingId::new(unreadable));
                    }
                    Bound::Unparsed(reason) => reasons.push(reason),
                }
            }
            if let Ok(reasons) = NonEmptyVec::from_vec(reasons) {
                return Err(reasons);
            }
            let grounds =
                NonEmptyVec::from_vec(grounds).map_err(|_| one(UnparsedReason::EmptyGrounds))?;
            let spec: Spec = raw
                .build(SpecId::new(id), grounds)
                .map_err(|error| match error {
                    EntityError::DuplicateGround { finding } => {
                        one(UnparsedReason::DuplicateGround { finding })
                    }
                    other => one(invalid("grounds", other.to_string())),
                })?;
            Ok((Entity::Spec(spec), written))
        }
        RawDoc::Design(raw) => {
            let mut written = Written::default();
            let spec = match bound(directory, raw.spec(), "spec", EntityKind::Spec) {
                Bound::Id(spec) | Bound::Conflicted(spec) => spec,
                Bound::Dangling(spec) => {
                    written.dangling.insert(spec);
                    spec
                }
                Bound::Unresolved(spec) => {
                    written.unresolved.insert(spec);
                    spec
                }
                Bound::Unparsed(reason) => return Err(one(reason)),
            };
            let design: Design = raw.build(DesignId::new(id), SpecId::new(spec));
            Ok((Entity::Design(design), written))
        }
    }
}

#[cfg(test)]
mod tests;
