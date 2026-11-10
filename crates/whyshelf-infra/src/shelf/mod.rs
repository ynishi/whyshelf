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
//!   <root>/<book dir>/**/*.md          --docs--->  Vec<DocReading>
//!   <root>/.whyshelf/ledger/<id>.json  <-ledger->  Record (through Canon)
//!   <root>/<path an artifact link names> --git-->  ArtifactReadings (hash, blob name?)
//!   Git: HEAD, blobs                   --git--->  Revision, content read back
//!   system clock, UUID v7, actor       --system->  JudgedAt, JudgementId, Actor
//! ```
//!
//! | file | port | module |
//! |---|---|---|
//! | `.whyshelf/books.toml` | [`BookSchemaSource`] | [`books`] |
//! | the Markdown files of the books | [`DocSource`] | [`docs`] |
//! | `.whyshelf/ledger/<id>.json` | [`LedgerRead`], [`LedgerAppend`] | [`ledger`] |
//! | the files links to artifacts name, and Git | [`ArtifactSource`], [`ContentStore`], [`RevisionSource`] | [`git`] |
//! | none | [`Clock`], [`IdSource`], [`ActorSource`] | [`system`] |
//!
//! Git's words (blob, commit, `HEAD`) are this crate's alone: the domain holds an
//! opaque [`ContentAddress`] and [`Revision`], and nothing but [`git`] knows they are
//! Git's.
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
//! Only `judge` writes, and only one file: a new record. Reading never creates a
//! directory or a file.
//!
//! [`BookSchemaSource`]: whyshelf_core::app::shelf::BookSchemaSource
//! [`DocSource`]: whyshelf_core::app::shelf::DocSource
//! [`LedgerRead`]: whyshelf_core::app::shelf::LedgerRead
//! [`LedgerAppend`]: whyshelf_core::app::shelf::LedgerAppend
//! [`Clock`]: whyshelf_core::app::shelf::Clock
//! [`IdSource`]: whyshelf_core::app::shelf::IdSource
//! [`ActorSource`]: whyshelf_core::app::shelf::ActorSource

pub mod books;
pub mod docs;
pub mod git;
pub mod ledger;
pub mod system;

use std::path::{Path, PathBuf};

use whyshelf_core::app::shelf::{ShelfRead, ShelfWrite};

use books::BooksToml;
use docs::MarkdownDocs;
use git::GitArtifacts;
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
    git: GitArtifacts,
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
            git: GitArtifacts::new(root.clone()),
            actor: ProjectActor::new(root, actor),
        }
    }

    /// The file of the declaration of the books.
    pub fn books_file(&self) -> &Path {
        self.books.path()
    }
}

impl ShelfRead for Project {
    type Schemas = BooksToml;
    type Docs = MarkdownDocs;
    type Ledger = JsonLedger;
    type Clock = SystemClock;
    type Artifacts = GitArtifacts;
    type Store = GitArtifacts;

    fn artifacts(&self) -> &GitArtifacts {
        &self.git
    }

    fn store(&self) -> &GitArtifacts {
        &self.git
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
    type Revisions = GitArtifacts;

    fn revisions(&self) -> &GitArtifacts {
        &self.git
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
