//! The document files of both roots: [`DocumentSource`] over the file system.
//!
//! Read only, and read and parse only. For every Markdown file under the scanned
//! directories of the public root and of the private root, [`FsDocuments`] gives
//! where it sits, the hash of its bytes **as they are on disk** ([`FileFacts`]), and
//! what reading it gave with its normalized text ([`RawDocument`]). The bytes' hash is
//! taken before the text is decoded or normalized, so a file with CRLF line ends or
//! trailing spaces differs from its copy that has none, which the normalized text
//! would hide.
//!
//! ```text
//!   file bytes --- BytesHash --------------------------------> FileFacts
//!        |
//!        +-- UTF-8? --- normalize (markdown::normalize) -------> RawDocument.text
//!                           |
//!                           +-- `id:` taken out, parse::read_in --> RawDoc
//!                                |
//!                                +-- references kept as written ---> ReadFile
//! ```
//!
//! # What a file reads as
//!
//! | the file | [`ReadFile`] |
//! |---|---|
//! | not UTF-8, a merge-conflict marker, front matter or keys that do not parse, a reference in `grounds` or `spec` that is no reference a key may hold | `Unparsed`, with every reason and the `id:` the file shows if it shows one |
//! | reads, and has no `id:` | `Unidentified`, with the kind the file shows |
//! | reads, and has one `id:` | `Document`, with its references as written |
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
//! (`private:<id>` for a private one). This adapter does not look them up: what an
//! alias names depends on the other files, and the rule is the domain's. It reads
//! each as a reference of the place a key is ([`Place::Key`]) and hands it over as
//! written; [`Corpus::build`](whyshelf_core::domain::corpus::Corpus::build) resolves it,
//! with the root of the file in mind.
//!
//! A Finding's `revision` is accepted when it is a full 40-digit revision (the rule of
//! the revision port), lowercased.
//!
//! # The private root
//!
//! Whether the person has a private root, and whether it can be read, is one decision,
//! [`PrivateRoot`], that [`DocumentSource::private_root`] returns: a root that is not
//! named is [`PrivateRoot::NotUsedHere`], a root that is named and can be listed is
//! [`PrivateRoot::Present`], and one that is named and cannot is
//! [`PrivateRoot::Absent`]. Listing a root that is absent gives no file from it, which
//! says nothing of whether it holds any; the caller that must not take its silence for
//! "none" asks `private_root` first.

use std::io;
use std::path::PathBuf;

use whyshelf_core::app::ports::{DocumentListing, DocumentSource, FileBytes, UnreadablePath};
use whyshelf_core::app::raw::{NormalizedText, RawDoc, RawDocument, ReadFile};
use whyshelf_core::domain::boundary::PrivateRoot;
use whyshelf_core::domain::corpus::{FileFacts, Header, Location, Visibility};
use whyshelf_core::domain::event::DocKind;
use whyshelf_core::domain::finding::Finding;
use whyshelf_core::domain::hash::BytesHash;
use whyshelf_core::domain::id::{DesignId, DocId, FindingId, SpecId};
use whyshelf_core::domain::knowable::Knowable;
use whyshelf_core::domain::location::RelPath;
use whyshelf_core::domain::naming::{Place, Ref};
use whyshelf_core::domain::nonempty::NonEmptyVec;
use whyshelf_core::domain::parsed::ParsedEntity;
use whyshelf_core::domain::policy::PinVocabulary;
use whyshelf_core::domain::reference::Ref as LegacyRef;
use whyshelf_core::domain::revision::RevisionId;
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
    /// A file of the private root was asked for, and this adapter has no private root.
    #[error("the file {path} is in the private root, and no private root is configured")]
    NoPrivateRoot {
        /// The file.
        path: RelPath,
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

/// What the person's settings say of the private root.
///
/// Why not an `Option<DocumentRoot>`: "no private root" and "a private root that is
/// not there" are different facts ([`PrivateRoot`]), and an `Option` would make the
/// second look like the first.
#[derive(Debug, Clone)]
pub enum PrivateDocuments {
    /// The settings name no private root.
    NotUsedHere,
    /// The settings name this root; it may or may not be readable.
    At(DocumentRoot),
}

/// The documents of the public root and of the private one.
#[derive(Debug, Clone)]
pub struct FsDocuments {
    public: DocumentRoot,
    private: PrivateDocuments,
    vocabulary: PinVocabulary,
}

impl FsDocuments {
    /// The documents of the public root, and of the private root if the person has
    /// one here.
    pub fn new(public: DocumentRoot, private: PrivateDocuments) -> Self {
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
        std::iter::once((Visibility::Public, &self.public)).chain(match &self.private {
            PrivateDocuments::NotUsedHere => None,
            PrivateDocuments::At(root) => Some((Visibility::Private, root)),
        })
    }

    /// The root a file of this visibility is under, if this adapter has one.
    fn root_of(&self, visibility: Visibility) -> Option<&DocumentRoot> {
        self.roots()
            .find(|(seen, _)| *seen == visibility)
            .map(|(_, root)| root)
    }

    /// Every file, once, as it was read.
    pub async fn read_all(&self) -> Result<DocumentListing, DocumentsError> {
        let mut unreadable: Vec<UnreadablePath> = Vec::new();
        let mut files: Vec<(FileFacts, RawDocument)> = Vec::new();
        for (visibility, root) in self.roots() {
            // A private root that `private_root` calls absent gives no file, as the
            // module says, rather than failing the whole listing.
            if visibility == Visibility::Private && !listable(root).await {
                continue;
            }
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
                files.push(read_file(root, visibility, path, &bytes, &self.vocabulary));
            }
        }
        Ok(DocumentListing::new(files, unreadable))
    }
}

impl DocumentSource for FsDocuments {
    type Error = DocumentsError;

    async fn list(&self) -> Result<DocumentListing, DocumentsError> {
        self.read_all().await
    }

    async fn bytes(&self, file: &FileFacts) -> Result<FileBytes, DocumentsError> {
        let location = file.location();
        let path = location.path();
        let Some(root) = self.root_of(location.visibility()) else {
            return Err(DocumentsError::NoPrivateRoot { path: path.clone() });
        };
        match tokio::fs::read(root.dir.join(path.as_str())).await {
            Ok(bytes) => Ok(FileBytes::Read(bytes)),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(FileBytes::Gone),
            Err(source) => Err(DocumentsError::Read {
                path: path.clone(),
                source,
            }),
        }
    }

    async fn private_root(&self) -> Result<PrivateRoot, DocumentsError> {
        match &self.private {
            PrivateDocuments::NotUsedHere => Ok(PrivateRoot::NotUsedHere),
            // Whatever stops the directory from being listed makes the root absent:
            // the question is whether its documents can be read, not why they cannot.
            PrivateDocuments::At(root) => Ok(if listable(root).await {
                PrivateRoot::Present
            } else {
                PrivateRoot::Absent
            }),
        }
    }
}

/// Whether the directory of a root can be listed: the one test of whether a private
/// root is there, for [`DocumentSource::private_root`] and for the listing alike.
async fn listable(root: &DocumentRoot) -> bool {
    tokio::fs::read_dir(&root.dir).await.is_ok()
}

// --- one file ---------------------------------------------------------------------------

fn unparsed(id: Option<DocId>, reason: UnparsedReason) -> ReadFile {
    ReadFile::Unparsed {
        id,
        reasons: NonEmptyVec::new(reason, Vec::new()),
    }
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

/// One file read: where it sits, its bytes' hash, what it reads as and its text.
fn read_file(
    root: &DocumentRoot,
    visibility: Visibility,
    path: RelPath,
    bytes: &[u8],
    vocabulary: &PinVocabulary,
) -> (FileFacts, RawDocument) {
    let hash = BytesHash::of(bytes);
    let finish = |text: NormalizedText, file: ReadFile| {
        let location = Location::new(visibility, path.clone(), Knowable::Unknown);
        (FileFacts::new(location, hash), RawDocument::new(file, text))
    };
    let text = match std::str::from_utf8(bytes) {
        Ok(text) => markdown::normalize(text),
        Err(_) => {
            let lossy = markdown::normalize(&String::from_utf8_lossy(bytes));
            let reason = UnparsedReason::MalformedFrontmatter {
                detail: "the file is not valid UTF-8".to_owned(),
            };
            let shown = markdown::seen_id(lossy.as_str());
            return finish(lossy, unparsed(shown, reason));
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
        Some(Err(reason)) => return finish(text, unparsed(shown_id, reason)),
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
        return finish(text, unparsed(shown_id, reason));
    }

    let read = parse::read_in(
        &without_id(text.as_str()),
        front_kind.or(placed),
        vocabulary,
    );
    let doc = match read.outcome {
        Ok(doc) => doc,
        Err(reasons) => {
            return finish(
                text,
                ReadFile::Unparsed {
                    id: shown_id,
                    reasons,
                },
            );
        }
    };
    let kind = doc.kind();
    let Some(id) = shown_id else {
        let file = if has_id_line {
            unparsed(
                None,
                invalid(
                    "id",
                    "the `id:` is no identifier, or the file has two different ones",
                ),
            )
        } else {
            ReadFile::Unidentified { kind: Some(kind) }
        };
        return finish(text, file);
    };
    let classification = read.classification;
    let header = Header::new(
        id,
        kind,
        classification.alias,
        classification.tags.unwrap_or_default(),
        classification.owner,
    );
    match entity_of(id, doc) {
        Ok(entity) => finish(
            text,
            ReadFile::Document {
                header,
                entity: Box::new(entity),
            },
        ),
        Err(reasons) => finish(
            text,
            ReadFile::Unparsed {
                id: Some(id),
                reasons,
            },
        ),
    }
}

/// A reference of `grounds` or `spec`, read for the place a key is.
///
/// A URL and an `ext:` reference are refused as the reason of the key, as before: an
/// entity that stands on another project's could not be approved in any environment.
fn key_ref(reference: &LegacyRef, field: &str) -> Result<Ref, UnparsedReason> {
    reference.entity_of(field)?;
    Ref::parse(&reference.to_string(), Place::Key)
        .map_err(|error| invalid(field, error.to_string()))
}

/// The entity a file describes, with its references as written, or the reasons it
/// cannot be one.
fn entity_of(id: DocId, doc: RawDoc) -> Result<ParsedEntity, NonEmptyVec<UnparsedReason>> {
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
            let finding: Finding = raw
                .build(FindingId::new(id.entity()), revision)
                .map_err(one)?;
            Ok(ParsedEntity::Finding(finding))
        }
        RawDoc::Spec(raw) => {
            let mut grounds = Vec::new();
            let mut reasons = Vec::new();
            for reference in raw.grounds().iter() {
                match key_ref(reference, "grounds") {
                    Ok(reference) => grounds.push(reference),
                    Err(reason) => reasons.push(reason),
                }
            }
            if let Ok(reasons) = NonEmptyVec::from_vec(reasons) {
                return Err(reasons);
            }
            let grounds =
                NonEmptyVec::from_vec(grounds).map_err(|_| one(UnparsedReason::EmptyGrounds))?;
            Ok(ParsedEntity::Spec(
                raw.into_parsed(SpecId::new(id.entity()), grounds),
            ))
        }
        RawDoc::Design(raw) => {
            let spec = key_ref(raw.spec(), "spec").map_err(one)?;
            Ok(ParsedEntity::Design(
                raw.into_parsed(DesignId::new(id.entity()), spec),
            ))
        }
    }
}

#[cfg(test)]
mod tests;
