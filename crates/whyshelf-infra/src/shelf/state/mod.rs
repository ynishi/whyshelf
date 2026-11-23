//! The adapters of the state of a tree of names
//! ([`StateRead`](whyshelf_core::app::shelf::StateRead),
//! [`StateList`](whyshelf_core::app::shelf::StateList)), and the walk along the links of
//! a name that every reader of the project's tree shares.
//!
//! | module | what it holds |
//! |---|---|
//! | [`files`] | [`FileStates`](files::FileStates): the tree of the project in a directory of the file system, at one side or at two |
//! | `paths` | the walk along the links of a name (`follow`, by the rule a reader gives, `Leave`: whether it reads through a link out of the tree), and whether a name is written as the state writes it (`named_as_written`) |
//!
//! A reader of the project (the declaration, the documents, the ledger, a space) reads
//! the tree through these and nothing else: it knows a [`Side`] and the names of the
//! tree, and the adapter alone knows what a side is (a directory, for
//! [`FileStates`](files::FileStates)).
//!
//! [`Side`]: whyshelf_core::app::shelf::Side

pub mod files;
pub(crate) mod paths;

use std::io;
use std::path::PathBuf;

use super::git::GitError;

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
    /// Git, which names the addresses of the contents and lists the files, could not
    /// be asked.
    #[error(transparent)]
    Git(#[from] GitError),
}
