//! Running the tests at a revision: [`TestRunPort`].
//!
//! The command of the project (`test.command`) runs through the shell, in a tree
//! that holds exactly the revision asked for:
//!
//! - **the working tree**, when the revision is `HEAD` and the tree is clean
//!   (see [`crate::git`] for what clean means), so that the usual build cache is used;
//! - **a temporary checkout** otherwise: `git worktree add --detach` into
//!   `<project>/.whyshelf/cache/rev-<short>-<pid>-<counter>`, a name that no
//!   other run uses, even for the same revision at the same time. The checkout is
//!   removed with `git worktree remove --force` when the command ends, and a failure
//!   to remove it is an error that is returned, not swallowed. `open` runs
//!   `git worktree prune` to clear what a killed run left behind.
//!
//! The command's own output is collected and dropped: a tool server speaks on its
//! standard output, so a test run must never write there. The outcome is the exit
//! status: zero passes, anything else fails.

use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::atomic::{AtomicU64, Ordering};

use tokio::process::Command;
use whyshelf_core::app::config::TestCommand;
use whyshelf_core::app::ports::TestRunPort;
use whyshelf_core::domain::approval::TestOutcome;
use whyshelf_core::domain::revision::RevisionId;

use crate::git::{Git, GitError, full_hash};

static CHECKOUT_COUNTER: AtomicU64 = AtomicU64::new(0);

/// Why the tests could not be run (a failing test is an outcome, not this).
#[derive(Debug, thiserror::Error)]
pub enum TestRunError {
    /// Git failed to answer, or to make the checkout.
    #[error("cannot prepare the tree to test")]
    Git(#[source] GitError),
    /// The directory for temporary checkouts could not be made.
    #[error("cannot make the directory {path}")]
    Io {
        /// The directory.
        path: PathBuf,
        /// The operating system's error.
        #[source]
        source: std::io::Error,
    },
    /// The test command could not be started.
    #[error("cannot start the test command `{command}`")]
    Spawn {
        /// The command line.
        command: String,
        /// The operating system's error.
        #[source]
        source: std::io::Error,
    },
    /// The temporary checkout could not be removed.
    #[error("cannot remove the temporary checkout {path}")]
    Cleanup {
        /// The checkout.
        path: PathBuf,
        /// What Git said.
        #[source]
        source: GitError,
    },
}

/// Runs the project's test command.
#[derive(Debug, Clone)]
pub struct GitTestRun {
    git: Git,
}

impl GitTestRun {
    /// Run tests for the repository of `root`.
    pub fn new(root: PathBuf) -> Self {
        Self {
            git: Git::new(root),
        }
    }
}

async fn run_command(dir: &Path, command: &TestCommand) -> Result<TestOutcome, TestRunError> {
    let mut shell = if cfg!(windows) {
        let mut shell = Command::new("cmd");
        shell.arg("/C");
        shell
    } else {
        let mut shell = Command::new("sh");
        shell.arg("-c");
        shell
    };
    let output = shell
        .arg(command.as_str())
        .current_dir(dir)
        .stdin(Stdio::null())
        .output()
        .await
        .map_err(|source| TestRunError::Spawn {
            command: command.as_str().to_owned(),
            source,
        })?;
    Ok(if output.status.success() {
        TestOutcome::Passed
    } else {
        TestOutcome::Failed
    })
}

impl TestRunPort for GitTestRun {
    type Error = TestRunError;

    async fn run(
        &self,
        revision: &RevisionId,
        command: &TestCommand,
    ) -> Result<TestOutcome, TestRunError> {
        let rev = full_hash(revision).map_err(TestRunError::Git)?;
        let head = self.git.head().await.map_err(TestRunError::Git)?;
        if head.eq_ignore_ascii_case(rev) && self.git.is_clean().await.map_err(TestRunError::Git)? {
            return run_command(self.git.root(), command).await;
        }

        let cache = self.git.root().join(".whyshelf").join("cache");
        tokio::fs::create_dir_all(&cache)
            .await
            .map_err(|source| TestRunError::Io {
                path: cache.clone(),
                source,
            })?;
        let counter = CHECKOUT_COUNTER.fetch_add(1, Ordering::Relaxed);
        let short: String = rev.chars().take(12).collect();
        let checkout = cache.join(format!("rev-{short}-{}-{counter}", std::process::id()));
        self.git
            .run([
                OsStr::new("worktree"),
                OsStr::new("add"),
                OsStr::new("--detach"),
                checkout.as_os_str(),
                OsStr::new(rev),
            ])
            .await
            .map_err(TestRunError::Git)?;

        let ran = run_command(&checkout, command).await;
        let removed = self
            .git
            .run([
                OsStr::new("worktree"),
                OsStr::new("remove"),
                OsStr::new("--force"),
                checkout.as_os_str(),
            ])
            .await;
        match (ran, removed) {
            (Err(error), _) => Err(error),
            (Ok(_), Err(source)) => Err(TestRunError::Cleanup {
                path: checkout,
                source,
            }),
            (Ok(outcome), Ok(_)) => Ok(outcome),
        }
    }
}
