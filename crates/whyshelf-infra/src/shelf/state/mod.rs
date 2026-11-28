//! The adapters of the state of a tree of names
//! ([`StateRead`], [`StateList`]), the one tree every reader of a run holds
//! ([`ProjectTree`]), the choice of it ([`ChangeAsk`]), and the walk along the links of a
//! name that every reader of the project's tree shares.
//!
//! | module | what it holds |
//! |---|---|
//! | [`files`] | [`FileStates`]: the tree of the project in a directory of the file system, at one side or at two |
//! | [`git`] | [`GitStates`]: the tree of the project in a commit of Git, or in its index, at two sides |
//! | `paths` | the walk along the links of a name (`follow`, by the rule a reader gives, `Leave`: whether it reads through a link out of the tree), and whether a name is written as the state writes it (`named_as_written`) |
//!
//! A reader of the project (the declaration, the documents, the ledger, a space) reads
//! the tree through these and nothing else: it knows a [`Side`] and the names of the
//! tree, and the adapter alone knows what a side is (a directory, for
//! [`FileStates`]; a commit or the index, for [`GitStates`]).
//!
//! [`Side`]: whyshelf_core::app::shelf::Side

pub mod files;
pub mod git;
pub(crate) mod paths;

use std::collections::{BTreeMap, BTreeSet};
use std::io;
use std::path::{Path, PathBuf};

use whyshelf_core::app::shelf::{
    AtSide, Beyond, ByteBound, DirListing, NameAnswers, Side, StateList, StateRead, TreeFiles,
};
use whyshelf_core::domain::location::RelPath;
use whyshelf_core::domain::shelf::artifact::{ContentAddress, Untold};

use super::git::GitError;
use files::FileStates;
use git::GitStates;

/// Why the state of a tree could not be read: what stops a reading, never the answer
/// for one name.
#[derive(Debug, thiserror::Error)]
pub enum StateError {
    /// The run reads one side of a change, and the side before was asked.
    #[error("this run reads no state before")]
    NoBefore,
    /// A name, or a directory on its way, could not be read.
    #[error("cannot read {}", .path.display())]
    Io {
        /// The path.
        path: PathBuf,
        /// The operating system's error.
        #[source]
        source: io::Error,
    },
    /// Git, which names the addresses of the contents and lists the files, or holds
    /// the tree a side is read from, could not be asked.
    #[error(transparent)]
    Git(#[from] GitError),
}

/// How a run that compares two ends of a change reads them, as the command line
/// asks: the argument of the composition root
/// ([`Project::for_change`](super::Project::for_change)), which turns it into the one
/// [`ProjectTree`] of the run. The core never sees it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChangeAsk {
    /// Through Git: the commit `since` names before, and `until` after.
    Git {
        /// The revision of the side before, as written.
        since: String,
        /// The side after.
        until: GitAfter,
    },
    /// Through two directories: `before`, and `after` (none: the project root).
    Files {
        /// The directory of the side before, as written.
        before: PathBuf,
        /// The directory of the side after, as written.
        after: Option<PathBuf>,
    },
}

/// The side after of a change read through Git.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GitAfter {
    /// The commit this revision names, as written.
    Commit(String),
    /// The index: what is staged.
    Index,
}

/// Whether a run reads what the system that holds the tree resolves (a link out of
/// the tree, a place that is no name of it, built docs outside the project) at one
/// side: the one answer of it, which every reading of such a place reads.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum SystemRead<'a> {
    /// It does, under this directory of the side.
    At(&'a Path),
    /// It cannot, and why ([`LEAVES`](files::LEAVES)): a run of two sides cannot read
    /// such a place alike at both.
    Untold(Untold),
}

/// The tree of the project a run reads, one of the adapters of a tree. Every reader of
/// one run (the declaration, the documents, the ledger, every space, the built docs of
/// `rust-item`) holds a clone of the one value the composition root made, so the two
/// sides of a change are read by the one adapter it chose. Which adapter it is, is the
/// composition root's alone: a reader never looks.
#[derive(Debug, Clone)]
pub struct ProjectTree(Arm);

/// The adapters a [`ProjectTree`] can be.
#[derive(Debug, Clone)]
enum Arm {
    Files(FileStates),
    Git(GitStates),
}

impl ProjectTree {
    /// The tree in the directories of the file system `state` reads.
    pub fn files(state: FileStates) -> Self {
        Self(Arm::Files(state))
    }

    /// The tree in the commits, or the index, of Git `state` reads.
    pub fn git(state: GitStates) -> Self {
        Self(Arm::Git(state))
    }

    /// Whether this run reads `side`: an error for one it does not.
    pub(crate) fn reads(&self, side: Side) -> Result<(), StateError> {
        match &self.0 {
            Arm::Files(state) => state.dir(side).map(|_| ()),
            Arm::Git(_) => Ok(()),
        }
    }

    /// How `name` at `side` is written in what a reader says (a path of the file
    /// system; `<rev>:<path>` of a commit, `:<path>` of the index): never read.
    pub(crate) fn path_of(&self, side: Side, name: &RelPath) -> Result<PathBuf, StateError> {
        match &self.0 {
            Arm::Files(state) => state.path_of(side, name),
            Arm::Git(state) => Ok(state.path_of(side, name)),
        }
    }

    /// How the place `at`, which the tree cannot name, is written in what a reader
    /// says, as [`path_of`](Self::path_of) writes a name (`at` with no part is the top).
    pub(crate) fn beyond_path(
        &self,
        side: Side,
        at: &Beyond<RelPath>,
    ) -> Result<PathBuf, StateError> {
        match &self.0 {
            Arm::Files(state) => state.beyond_path(side, at),
            Arm::Git(state) => Ok(state.beyond_path(side, at)),
        }
    }

    /// The place `at` of `side` (`at` with no part is the top) with every symbolic link
    /// resolved, as a path of the file system, when the side is a directory of one and
    /// the place can be resolved; none for a side of Git, whose places are no paths of
    /// the file system. The one place a reader asks for the resolved form of a place
    /// of the tree, beside the form [`beyond_path`](Self::beyond_path) writes.
    pub(crate) async fn real_path(&self, side: Side, at: &Beyond<RelPath>) -> Option<PathBuf> {
        match &self.0 {
            Arm::Files(state) => {
                let path = state.beyond_path(side, at).ok()?;
                tokio::fs::canonicalize(path).await.ok()
            }
            Arm::Git(_) => None,
        }
    }

    /// The directory of the file system `side` is read from, when it is one.
    pub(crate) fn dir(&self, side: Side) -> Option<&Path> {
        match &self.0 {
            Arm::Files(state) => state.dir(side).ok(),
            Arm::Git(_) => None,
        }
    }

    /// Whether this run reads what the system resolves at `side` ([`SystemRead`]).
    pub(crate) fn system_read(&self, side: Side) -> Result<SystemRead<'_>, StateError> {
        match &self.0 {
            Arm::Files(state) => state.system_read(side),
            Arm::Git(state) => Ok(state.system_read()),
        }
    }
}

impl StateRead<RelPath> for ProjectTree {
    type Error = StateError;

    async fn read(
        &self,
        side: Side,
        names: &BTreeSet<RelPath>,
        bound: ByteBound,
    ) -> Result<NameAnswers<RelPath, Vec<u8>, StateError>, StateError> {
        match &self.0 {
            Arm::Files(state) => state.read(side, names, bound).await,
            Arm::Git(state) => state.read(side, names, bound).await,
        }
    }

    async fn look(
        &self,
        side: Side,
        names: &BTreeSet<RelPath>,
    ) -> Result<NameAnswers<RelPath, u64, StateError>, StateError> {
        match &self.0 {
            Arm::Files(state) => state.look(side, names).await,
            Arm::Git(state) => state.look(side, names).await,
        }
    }

    async fn read_outside(
        &self,
        side: Side,
        names: &BTreeSet<RelPath>,
        bound: ByteBound,
    ) -> Result<NameAnswers<RelPath, Vec<u8>, StateError>, StateError> {
        match &self.0 {
            Arm::Files(state) => state.read_outside(side, names, bound).await,
            Arm::Git(state) => state.read_outside(side, names, bound).await,
        }
    }

    async fn read_beyond(
        &self,
        side: Side,
        at: &Beyond<RelPath>,
        bound: ByteBound,
    ) -> Result<AtSide<RelPath>, StateError> {
        match &self.0 {
            Arm::Files(state) => state.read_beyond(side, at, bound).await,
            Arm::Git(state) => state.read_beyond(side, at, bound).await,
        }
    }

    async fn addresses(
        &self,
        side: Side,
        names: &BTreeSet<RelPath>,
    ) -> Result<BTreeMap<RelPath, ContentAddress>, StateError> {
        match &self.0 {
            Arm::Files(state) => state.addresses(side, names).await,
            Arm::Git(state) => state.addresses(side, names).await,
        }
    }
}

impl StateList<RelPath> for ProjectTree {
    async fn list(
        &self,
        side: Side,
        dir: Option<&RelPath>,
    ) -> Result<DirListing<RelPath>, StateError> {
        match &self.0 {
            Arm::Files(state) => state.list(side, dir).await,
            Arm::Git(state) => state.list(side, dir).await,
        }
    }

    async fn list_outside(
        &self,
        side: Side,
        dir: &RelPath,
    ) -> Result<DirListing<RelPath>, StateError> {
        match &self.0 {
            Arm::Files(state) => state.list_outside(side, dir).await,
            Arm::Git(state) => state.list_outside(side, dir).await,
        }
    }

    async fn list_beyond(
        &self,
        side: Side,
        at: &Beyond<RelPath>,
    ) -> Result<DirListing<RelPath>, StateError> {
        match &self.0 {
            Arm::Files(state) => state.list_beyond(side, at).await,
            Arm::Git(state) => state.list_beyond(side, at).await,
        }
    }

    async fn files(&self, side: Side, bound: ByteBound) -> Result<TreeFiles<RelPath>, StateError> {
        match &self.0 {
            Arm::Files(state) => state.files(side, bound).await,
            Arm::Git(state) => state.files(side, bound).await,
        }
    }
}
