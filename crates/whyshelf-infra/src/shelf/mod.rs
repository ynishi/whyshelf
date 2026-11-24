//! The adapters of a shelf: the files of a project, read and written for the use
//! cases of [`app::shelf`](whyshelf_core::app::shelf).
//!
//! Every wire format is here and nowhere else: TOML for the declaration of the
//! books, Markdown with a YAML front matter for the documents, JSON for the records
//! of the ledger. Each is translated into the values of the domain at this boundary
//! (an anti-corruption layer), so the domain never sees a table, a mapping or a
//! JSON text.
//!
//! ```text
//!   <root>/.whyshelf/books.toml        --books-->  Vec<BookSchemaDecl>
//!   <root>/<book dir>/**/*.md          <-docs--->  Vec<DocReading>; a new <key>.md
//!   <root>/.whyshelf/ledger/<id>.json  <-ledger->  Record (through Canon)
//!   a name a link to artifacts writes --artifact-->  ArtifactReadings, by the adapter
//!                                                    of its space (for `file`: the
//!                                                    file at the path, blob name?;
//!                                                    for `rust-item`: the item in
//!                                                    the docs `cargo doc` wrote;
//!                                                    for `rust-test`: the test
//!                                                    function's tokens in its file;
//!                                                    for `js-test`: the test call's
//!                                                    tokens in its file)
//!   an address of an approval          --docs / artifact-->  content read back, by the
//!                                                    adapter that named it
//!   Git: HEAD                          --git---->  Revision
//!   system clock, UUID v7, actor       --system->  JudgedAt, JudgementId, Actor
//! ```
//!
//! | file | port | module |
//! |---|---|---|
//! | `.whyshelf/books.toml` | [`BookSchemaSource`], [`DeclarationWrite`] | [`books`] |
//! | the templates `init` writes (a declaration, and documents beside it) | none: data for [`InitRequest`] | [`init`] |
//! | a file `init` writes beside the documents (a table a template's document names) | [`FileWrite`] | [`artifact::file`] |
//! | the Markdown files of the books | [`DocSource`], [`DocWrite`], [`DocReplace`], [`DocRemove`] | [`docs`] |
//! | the files `new`, `init` and `rekey` wrote, put on disk together at the end of the run, and the removals of `rekey` | [`FileSync`] | [`Project`], which holds the root |
//! | `.whyshelf/ledger/<id>.json` | [`LedgerRead`], [`LedgerAppend`] | [`ledger`] |
//! | what links to artifacts name, by space | [`ArtifactSource`] | [`artifact`] |
//! | where a pinned artifact that is gone may have moved, by space | [`MoveSource`] | [`artifact`] (for `file`: the same content, Git's renames; for `rust-item`: the names in the docs that end as the gone one does) |
//! | content kept by address, read back by the adapter of the documents or of the space | [`ContentStore`] | [`Project`], to [`docs`] or [`artifact`] |
//! | the project's version control | [`RevisionSource`] | [`git`] |
//! | the tree of the project at one side of a change, which every reader above reads its files through | [`StateRead`], [`StateList`] | [`state`] |
//! | none | [`Clock`], [`IdSource`], [`ActorSource`] | [`system`] |
//!
//! Git's words (blob, commit, `HEAD`) are this crate's alone: the domain holds an
//! opaque [`ContentAddress`] and [`Revision`], and nothing but [`git`] and the
//! adapters that use it know they are Git's.
//!
//! [`ArtifactSource`]: whyshelf_core::app::shelf::ArtifactSource
//! [`MoveSource`]: whyshelf_core::app::shelf::MoveSource
//! [`ContentStore`]: whyshelf_core::app::shelf::ContentStore
//! [`RevisionSource`]: whyshelf_core::app::shelf::RevisionSource
//! [`StateRead`]: whyshelf_core::app::shelf::StateRead
//! [`StateList`]: whyshelf_core::app::shelf::StateList
//! [`ContentAddress`]: whyshelf_core::domain::shelf::artifact::ContentAddress
//! [`Revision`]: whyshelf_core::domain::shelf::artifact::Revision
//!
//! [`Project`] bundles them for a project root, as one value the command line hands
//! to [`ShelfApp`](whyshelf_core::app::shelf::ShelfApp).
//!
//! Only `judge`, `new`, `init`, `fmt` and `rekey` write. `judge` writes one new file, a record;
//! `new` writes a document or, for a run of rows, many (create-only, never in place of
//! a file that is there). `init` writes the declaration, the other files of its
//! template and documents beside it, each create-only. The files of `new` and `init`
//! are written whole and are put on disk together when the run ends ([`FileSync`]),
//! not one at a time. `fmt` writes over documents' files that are there, each in the one form,
//! after comparing it with what was read, and whole ([`DocReplace`]); an edit
//! saved between the comparison and the write is overwritten. `rekey` creates a
//! document's file, writes over others as `fmt` does, and removes the file the document
//! was in ([`DocRemove`], after the same comparison; the removal is put on disk by
//! [`FileSync::persist_gone`]); when it stops it puts the files back as they were
//! (the `restore` of [`DocReplace`]). Reading never creates a directory or a file.
//!
//! [`BookSchemaSource`]: whyshelf_core::app::shelf::BookSchemaSource
//! [`DeclarationWrite`]: whyshelf_core::app::shelf::DeclarationWrite
//! [`InitRequest`]: whyshelf_core::app::shelf::InitRequest
//! [`FileWrite`]: whyshelf_core::app::shelf::FileWrite
//! [`FileSync`]: whyshelf_core::app::shelf::FileSync
//! [`DocSource`]: whyshelf_core::app::shelf::DocSource
//! [`DocWrite`]: whyshelf_core::app::shelf::DocWrite
//! [`DocReplace`]: whyshelf_core::app::shelf::DocReplace
//! [`DocRemove`]: whyshelf_core::app::shelf::DocRemove
//! [`LedgerRead`]: whyshelf_core::app::shelf::LedgerRead
//! [`LedgerAppend`]: whyshelf_core::app::shelf::LedgerAppend
//! [`Clock`]: whyshelf_core::app::shelf::Clock
//! [`IdSource`]: whyshelf_core::app::shelf::IdSource
//! [`ActorSource`]: whyshelf_core::app::shelf::ActorSource

pub mod artifact;
pub mod books;
mod create;
pub mod docs;
pub mod git;
pub mod init;
pub mod ledger;
pub mod state;
pub mod system;

#[cfg(test)]
mod project_tests;
#[cfg(test)]
mod readers_tests;
#[cfg(test)]
mod tree_fixtures_tests;

use std::path::{Path, PathBuf};

pub use create::PersistFailed;
use whyshelf_core::app::shelf::{
    ContentOf, ContentStore, DispatchError, FileSync, ReadWay, ShelfRead, ShelfWrite, Spaces,
};
use whyshelf_core::domain::location::{PathLiteral, RelPath};
use whyshelf_core::domain::nonempty::{NonBlank, NonEmptyVec, TextLiteral};
use whyshelf_core::domain::shelf::artifact::ContentAddress;

use artifact::file::FileSpace;
use artifact::js_test::JsTestSpace;
use artifact::rust_item::RustItemSpace;
use artifact::rust_test::RustTestSpace;
use artifact::{ArtifactSpaces, Registry};
use books::BooksToml;
use docs::MarkdownDocs;
use git::Git;
use ledger::JsonLedger;
use state::files::FileStates;
use state::git::GitStates;
use state::{ChangeAsk, GitAfter, ProjectTree};
use system::{ProjectActor, SystemClock, UuidV7Ids};

/// The directory of whyshelf's own files in a project.
pub const WHYSHELF_DIR: &str = ".whyshelf";

/// The file of the declaration of the books, relative to the project root: a
/// literal held to the rules of a relative path when this crate builds.
pub const BOOKS_FILE: PathLiteral = PathLiteral::new(".whyshelf/books.toml");

/// The directory of the ledger, relative to the project root: the one the paths of
/// its records are from.
const LEDGER_DIR: PathLiteral = PathLiteral::new(".whyshelf/ledger");

/// Every adapter of a shelf, for one project root.
#[derive(Debug, Clone)]
pub struct Project {
    /// The project root: the one the writers write under, and the one the paths of
    /// [`FileSync::persist`] are from.
    root: PathBuf,
    books: BooksToml,
    docs: MarkdownDocs,
    ledger: JsonLedger,
    clock: SystemClock,
    ids: UuidV7Ids,
    actor: ProjectActor,
    artifacts: ArtifactSpaces,
    files: FileSpace,
    git: Git,
    /// How the run reads, as named here.
    way: ReadWay,
}

/// The way of a run that reads the files of the root of the project as they are. The
/// root is not written in it, so an answer that names the way names no local path.
const FILES_WAY: TextLiteral = TextLiteral::new("files: the project root");

/// The way of a run that compares two ends, when what names it cannot be made (it
/// never cannot: it starts with the kind of the way).
const CHANGE_WAY: TextLiteral = TextLiteral::new("two ends of a change");

impl Project {
    /// The adapters of the project at `root`. `actor` is who judges when the
    /// environment says so (`WHYSHELF_ACTOR`, read by the caller); without it the
    /// Git setting `user.name` of the project is asked. `ran_by` is who ran the
    /// command when the environment says so (`WHYSHELF_RAN_BY`, read by the caller);
    /// without it no one is named. `cargo_target_dir` is the
    /// build directory the environment names (`CARGO_TARGET_DIR`, read by the
    /// caller), where the space `rust-item` looks for the docs first.
    ///
    /// An error when the adapters of the spaces cannot be dispatched to
    /// ([`Spaces::new`]): one names as its space a text that is no space, two name one
    /// space, two offer a built-in preset of one name, or one offers a built-in preset
    /// for another space than its own.
    ///
    /// Every reader reads the project through one state of its tree: the root as it
    /// is, at one side ([`FileStates::new`]), and the run's way names it so
    /// (`files: the project root`).
    pub fn new(
        root: impl Into<PathBuf>,
        actor: Option<String>,
        ran_by: Option<String>,
        cargo_target_dir: Option<PathBuf>,
    ) -> Result<Self, DispatchError> {
        let root: PathBuf = root.into();
        let tree = ProjectTree::files(FileStates::new(root.clone()));
        let way = ReadWay::new(NonBlank::from(FILES_WAY));
        Self::on(tree, way, root, actor, ran_by, cargo_target_dir)
    }

    /// The adapters of the project at `root` for a run that compares two ends of a
    /// change, both read the one way `change` asks ([`ChangeAsk`]): every reader, the
    /// spaces and the built docs of `rust-item` on the one tree it opens
    /// ([`GitStates`] or [`FileStates::between`]), and the
    /// run's way named from what was asked, as written (`git: <since> -> <until>`,
    /// `git: <since> -> the index`, `files: <before> -> <after>`, with
    /// `the project root` for an after not asked). `actor`, `ran_by` and
    /// `cargo_target_dir` are as for [`Project::new`].
    ///
    /// An error when the ends cannot be opened as asked ([`OpenError`]), or the
    /// adapters of the spaces cannot be dispatched to.
    pub async fn for_change(
        root: impl Into<PathBuf>,
        change: ChangeAsk,
        actor: Option<String>,
        ran_by: Option<String>,
        cargo_target_dir: Option<PathBuf>,
    ) -> Result<Self, OpenError> {
        let root: PathBuf = root.into();
        let (tree, way) = match change {
            ChangeAsk::Git { since, until } => {
                let after = match &until {
                    GitAfter::Commit(rev) => rev.clone(),
                    GitAfter::Index => "the index".to_owned(),
                };
                let states = GitStates::open(root.clone(), &since, &until).await?;
                (ProjectTree::git(states), format!("git: {since} -> {after}"))
            }
            ChangeAsk::Files { before, after } => {
                for dir in std::iter::once(&before).chain(&after) {
                    match tokio::fs::metadata(dir).await {
                        // arch-lint: allow(no-sync-io) reason="`meta` is Metadata fetched with `.await`: a pure accessor, no I/O"
                        Ok(meta) if meta.is_dir() => {}
                        Ok(_) => return Err(OpenError::NoDirectory { path: dir.clone() }),
                        Err(e)
                            if matches!(
                                e.kind(),
                                std::io::ErrorKind::NotFound | std::io::ErrorKind::NotADirectory
                            ) =>
                        {
                            return Err(OpenError::NoDirectory { path: dir.clone() });
                        }
                        Err(source) => {
                            return Err(OpenError::Unreadable {
                                path: dir.clone(),
                                source,
                            });
                        }
                    }
                }
                let shown = after.as_ref().map_or_else(
                    || "the project root".to_owned(),
                    |dir| dir.display().to_string(),
                );
                let way = format!("files: {} -> {shown}", before.display());
                let after = after.unwrap_or_else(|| root.clone());
                (ProjectTree::files(FileStates::between(before, after)), way)
            }
        };
        let way = ReadWay::new(NonBlank::new(way).unwrap_or_else(|_| NonBlank::from(CHANGE_WAY)));
        Ok(Self::on(tree, way, root, actor, ran_by, cargo_target_dir)?)
    }

    /// The adapters of the project at `root`, every reader on the one `tree` (a clone
    /// of it each), the run's way named `way`: the one assembly both entries share.
    /// The writers write under `root`, whatever the tree reads.
    fn on(
        tree: ProjectTree,
        way: ReadWay,
        root: PathBuf,
        actor: Option<String>,
        ran_by: Option<String>,
        cargo_target_dir: Option<PathBuf>,
    ) -> Result<Self, DispatchError> {
        Ok(Self {
            books: BooksToml::on(tree.clone(), root.clone(), BOOKS_FILE.into()),
            docs: MarkdownDocs::on(tree.clone(), root.clone()),
            ledger: JsonLedger::on(tree.clone(), root.clone(), LEDGER_DIR.into()),
            clock: SystemClock,
            ids: UuidV7Ids,
            artifacts: Spaces::new(Registry::of_project(
                FileSpace::on(tree.clone(), root.clone()),
                RustItemSpace::on(tree.clone(), cargo_target_dir),
                RustTestSpace::on(tree.clone()),
                JsTestSpace::on(tree.clone()),
            ))?,
            files: FileSpace::on(tree, root.clone()),
            git: Git::new(root.clone()),
            actor: ProjectActor::new(root.clone(), actor, ran_by),
            way,
            root,
        })
    }

    /// The file of the declaration of the books.
    pub fn books_file(&self) -> &Path {
        self.books.path()
    }
}

/// Why the two ends of a change cannot be opened for a run that compares them. What
/// the person asked that cannot be read as asked (every variant but four) the command
/// line tells as an error of the request (exit 2); a dispatch that cannot be made, a
/// Git that cannot be found or run or that fails, and a directory that cannot be looked
/// at ([`OpenError::Dispatch`], [`OpenError::NoGit`], [`OpenError::Git`],
/// [`OpenError::Unreadable`]), are of the run and not of the request, told as a run
/// that failed (exit 3), as an error met while reading is.
#[derive(Debug, thiserror::Error)]
pub enum OpenError {
    /// The adapters of the spaces cannot be dispatched to, as for [`Project::new`].
    #[error(transparent)]
    Dispatch(#[from] DispatchError),
    /// No `git` can be found or started here.
    #[error("no git can be run here")]
    NoGit,
    /// The project is in no Git repository.
    #[error("{} is not in a Git repository", .root.display())]
    NotARepository {
        /// The project root.
        root: PathBuf,
    },
    /// A revision names no commit.
    #[error("`{rev}` names no commit of the repository")]
    NotACommit {
        /// The revision, as written.
        rev: String,
    },
    /// The Git that runs does not know a form a commit is read with.
    #[error("the git that runs does not know `{option}`, which reading a commit needs")]
    UnknownOption {
        /// The form, as written.
        option: String,
    },
    /// The index holds other `.gitattributes` than `HEAD`, so its content cannot be
    /// converted as a checkout of it would be.
    #[error(
        "the index changes .gitattributes, so the index cannot be read as a checkout; ask with --before/--after"
    )]
    IndexAttributes,
    /// A directory of a side is not there, or is no directory.
    #[error("{} is no directory", .path.display())]
    NoDirectory {
        /// The directory, as written.
        path: PathBuf,
    },
    /// What is at a directory of a side could not be looked at (for another reason
    /// than nothing there): no error of the request, a run that failed.
    #[error("cannot look at {}", .path.display())]
    Unreadable {
        /// The directory, as written.
        path: PathBuf,
        /// The operating system's error.
        #[source]
        source: std::io::Error,
    },
    /// Git could not be asked: no error of the request, a run that failed.
    #[error(transparent)]
    Git(#[from] git::GitError),
}

/// Content read back by the adapter that named its address: the documents' for a
/// document's file, the space's for an artifact.
impl ContentStore for Project {
    type Error = StoreError;

    async fn read_back(
        &self,
        of: &ContentOf,
        address: &ContentAddress,
    ) -> Result<Option<Vec<u8>>, StoreError> {
        Ok(match of {
            ContentOf::Doc => self.docs.read_back(address).await?,
            ContentOf::Artifact(space) => self.artifacts.read_back(space, address).await?,
        })
    }
}

/// Why content could not be read back: the adapter that named its address could not
/// ask Git (it could not be run, or took too long). Content that is not there is no
/// error: it reads back as none.
#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    /// The adapter of the documents.
    #[error(transparent)]
    Docs(#[from] docs::DocsError),
    /// The adapter of a space.
    #[error(transparent)]
    Artifacts(#[from] artifact::SpacesError),
}

impl ShelfRead for Project {
    type Schemas = BooksToml;
    type Docs = MarkdownDocs;
    type Ledger = JsonLedger;
    type Clock = SystemClock;
    type Artifacts = ArtifactSpaces;
    type Store = Self;
    type Moves = ArtifactSpaces;

    fn artifacts(&self) -> &ArtifactSpaces {
        &self.artifacts
    }

    fn moves(&self) -> &ArtifactSpaces {
        &self.artifacts
    }

    fn store(&self) -> &Self {
        self
    }

    fn schemas(&self) -> &BooksToml {
        &self.books
    }

    fn docs(&self) -> &MarkdownDocs {
        &self.docs
    }

    fn ledger(&self) -> &JsonLedger {
        &self.ledger
    }

    fn clock(&self) -> &SystemClock {
        &self.clock
    }

    fn way(&self) -> ReadWay {
        self.way.clone()
    }
}

/// The files a run wrote, put on disk: each, and the directories above it up to the
/// root, by the rules of the platform (on Apple systems one full flush of the drive
/// for the whole run, not one for each file). The paths are from the root of the
/// project, the one the three writers ([`BooksToml`], [`MarkdownDocs`],
/// [`FileSpace`]) write under, so the root is held here once.
impl FileSync for Project {
    type Error = PersistFailed;

    async fn persist(&self, files: &NonEmptyVec<RelPath>) -> Result<(), PersistFailed> {
        let files: Vec<RelPath> = files.iter().cloned().collect();
        create::persist(&self.root, &files).await
    }

    /// The directory each file was in, put on disk (`create::persist_gone`).
    async fn persist_gone(&self, files: &NonEmptyVec<RelPath>) -> Result<(), PersistFailed> {
        let files: Vec<RelPath> = files.iter().cloned().collect();
        create::persist_gone(&self.root, &files).await
    }
}

impl ShelfWrite for Project {
    type Append = JsonLedger;
    type Ids = UuidV7Ids;
    type Actor = ProjectActor;
    type Revisions = Git;
    type DocWriter = MarkdownDocs;
    type DocReplacer = MarkdownDocs;
    type DocRemover = MarkdownDocs;
    type DeclarationWriter = BooksToml;
    type FileWriter = FileSpace;
    type FileSyncer = Self;

    fn file_syncer(&self) -> &Self {
        self
    }

    fn declaration_writer(&self) -> &BooksToml {
        &self.books
    }

    fn file_writer(&self) -> &FileSpace {
        &self.files
    }

    fn revisions(&self) -> &Git {
        &self.git
    }

    fn doc_writer(&self) -> &MarkdownDocs {
        &self.docs
    }

    fn doc_replacer(&self) -> &MarkdownDocs {
        &self.docs
    }

    fn doc_remover(&self) -> &MarkdownDocs {
        &self.docs
    }

    fn append(&self) -> &JsonLedger {
        &self.ledger
    }

    fn ids(&self) -> &UuidV7Ids {
        &self.ids
    }

    fn actor_source(&self) -> &ProjectActor {
        &self.actor
    }
}
