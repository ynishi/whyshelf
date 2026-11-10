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
//!                                                    file at the path, blob name?)
//!   an address of an approval          --docs / artifact-->  content read back, by the
//!                                                    adapter that named it
//!   Git: HEAD                          --git---->  Revision
//!   system clock, UUID v7, actor       --system->  JudgedAt, JudgementId, Actor
//! ```
//!
//! | file | port | module |
//! |---|---|---|
//! | `.whyshelf/books.toml` | [`BookSchemaSource`] | [`books`] |
//! | the Markdown files of the books | [`DocSource`], [`DocWrite`] | [`docs`] |
//! | `.whyshelf/ledger/<id>.json` | [`LedgerRead`], [`LedgerAppend`] | [`ledger`] |
//! | what links to artifacts name, by space | [`ArtifactSource`] | [`artifact`] |
//! | content kept by address, read back by the adapter of the documents or of the space | [`ContentStore`] | [`Project`], to [`docs`] or [`artifact`] |
//! | the project's version control | [`RevisionSource`] | [`git`] |
//! | none | [`Clock`], [`IdSource`], [`ActorSource`] | [`system`] |
//!
//! Git's words (blob, commit, `HEAD`) are this crate's alone: the domain holds an
//! opaque [`ContentAddress`] and [`Revision`], and nothing but [`git`] and the
//! adapters that use it know they are Git's.
//!
//! [`ArtifactSource`]: whyshelf_core::app::shelf::ArtifactSource
//! [`ContentStore`]: whyshelf_core::app::shelf::ContentStore
//! [`RevisionSource`]: whyshelf_core::app::shelf::RevisionSource
//! [`ContentAddress`]: whyshelf_core::domain::shelf::artifact::ContentAddress
//! [`Revision`]: whyshelf_core::domain::shelf::artifact::Revision
//!
//! [`Project`] bundles them for a project root, as one value the command line hands
//! to [`ShelfApp`](whyshelf_core::app::shelf::ShelfApp).
//!
//! Only `judge` and `new` write, each one new file: `judge` a record, `new` a
//! document (create-only, never in place of a file that is there). Reading never
//! creates a directory or a file.
//!
//! [`BookSchemaSource`]: whyshelf_core::app::shelf::BookSchemaSource
//! [`DocSource`]: whyshelf_core::app::shelf::DocSource
//! [`DocWrite`]: whyshelf_core::app::shelf::DocWrite
//! [`LedgerRead`]: whyshelf_core::app::shelf::LedgerRead
//! [`LedgerAppend`]: whyshelf_core::app::shelf::LedgerAppend
//! [`Clock`]: whyshelf_core::app::shelf::Clock
//! [`IdSource`]: whyshelf_core::app::shelf::IdSource
//! [`ActorSource`]: whyshelf_core::app::shelf::ActorSource

pub mod artifact;
pub mod books;
pub mod docs;
pub mod git;
pub mod ledger;
pub mod system;

use std::convert::Infallible;
use std::path::{Path, PathBuf};

use whyshelf_core::app::shelf::{ContentOf, ContentStore, ShelfRead, ShelfWrite};
use whyshelf_core::domain::shelf::artifact::ContentAddress;

use artifact::ArtifactSpaces;
use artifact::file::FileSpace;
use books::BooksToml;
use docs::MarkdownDocs;
use git::Git;
use ledger::JsonLedger;
use system::{ProjectActor, SystemClock, UuidV7Ids};

/// The directory of whyshelf's own files in a project.
pub const WHYSHELF_DIR: &str = ".whyshelf";

/// Every adapter of a shelf, for one project root.
#[derive(Debug, Clone)]
pub struct Project {
    books: BooksToml,
    docs: MarkdownDocs,
    ledger: JsonLedger,
    clock: SystemClock,
    ids: UuidV7Ids,
    actor: ProjectActor,
    artifacts: ArtifactSpaces,
    git: Git,
}

impl Project {
    /// The adapters of the project at `root`. `actor` is who judges when the
    /// environment says so (`WHYSHELF_ACTOR`, read by the caller); without it the
    /// Git setting `user.name` of the project is asked.
    pub fn new(root: impl Into<PathBuf>, actor: Option<String>) -> Self {
        let root: PathBuf = root.into();
        Self {
            books: BooksToml::new(root.join(WHYSHELF_DIR).join("books.toml")),
            docs: MarkdownDocs::new(root.clone()),
            ledger: JsonLedger::new(root.join(WHYSHELF_DIR).join("ledger"), WHYSHELF_DIR),
            clock: SystemClock,
            ids: UuidV7Ids,
            artifacts: ArtifactSpaces::of_project(FileSpace::new(root.clone())),
            git: Git::new(root.clone()),
            actor: ProjectActor::new(root, actor),
        }
    }

    /// The file of the declaration of the books.
    pub fn books_file(&self) -> &Path {
        self.books.path()
    }
}

/// Content read back by the adapter that named its address: the documents' for a
/// document's file, the space's for an artifact.
impl ContentStore for Project {
    type Error = Infallible;

    async fn read_back(
        &self,
        of: &ContentOf,
        address: &ContentAddress,
    ) -> Result<Option<Vec<u8>>, Infallible> {
        Ok(match of {
            ContentOf::Doc => self.docs.read_back(address).await,
            ContentOf::Artifact(space) => self.artifacts.read_back(space, address).await,
        })
    }
}

impl ShelfRead for Project {
    type Schemas = BooksToml;
    type Docs = MarkdownDocs;
    type Ledger = JsonLedger;
    type Clock = SystemClock;
    type Artifacts = ArtifactSpaces;
    type Store = Self;

    fn artifacts(&self) -> &ArtifactSpaces {
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
}

impl ShelfWrite for Project {
    type Append = JsonLedger;
    type Ids = UuidV7Ids;
    type Actor = ProjectActor;
    type Revisions = Git;
    type DocWriter = MarkdownDocs;

    fn revisions(&self) -> &Git {
        &self.git
    }

    fn doc_writer(&self) -> &MarkdownDocs {
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
