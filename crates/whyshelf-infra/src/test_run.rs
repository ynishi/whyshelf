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
//!   `git worktree prune` to clear what a killed run left behind. When the project
//!   root is a directory inside the repository (`git rev-parse --show-prefix`), the
//!   command runs in that same directory of the checkout, not at its top.
//!
//! The command's own output never reaches ours: a tool server speaks on its standard
//! output, so a test run must not write there. Both of its streams are read as they
//! come, and only the last 64 KiB of each is held, so a command that prints for an
//! hour costs no more memory than one that prints a line.
//!
//! **Time limit.** The command is given `test.timeout` (thirty minutes unless the
//! settings say otherwise). When that is up it is stopped, with the processes it
//! started (on Unix it leads a process group of its own, which is killed), and the
//! answer is [`TestsRun::TimedOut`]. A stopped command has no result, so the answer is
//! no [`TestOutcome`] and nothing can be recorded from it; the CA gate reports it as a
//! violation. Otherwise the outcome is the exit status: zero passes, anything else
//! fails.
//!
//! ```text
//!   run(revision, command)
//!     |
//!     |  revision == HEAD  and  tree clean (outside .whyshelf/)?
//!     |
//!     +-- yes --> run `sh -c <command>` in the project root ------------+
//!     |                                                                  |
//!     +-- no  --> git worktree add --detach                              |
//!                   <root>/.whyshelf/cache/rev-<12 hex>-<pid>-<n>  <rev> |
//!                        |                                               |
//!                        v                                               |
//!                 run `sh -c <command>` in that checkout                 |
//!                        |                                               |
//!                        v                                               |
//!                 git worktree remove --force <checkout>                 |
//!                        |   a failed removal is an error, returned       |
//!                        v                                               v
//!                 TestOutcome::Passed (exit 0) | Failed (anything else)
//!
//!   open():  git worktree prune   clears a checkout left by a killed run
//! ```
//!
//! Two things to know about a run at another revision. The command is the one in the
//! settings **as they are**, not the one the revision had. And the checkout holds the
//! tracked files of that revision only: what a build leaves untracked in the working
//! tree (build output, installed dependencies) is not there, so the
//! command starts as it would in a fresh clone.

use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use tokio::io::{AsyncRead, AsyncReadExt};
use tokio::process::{Child, Command};
use whyshelf_core::app::config::{TestCommand, TestTimeout};
use whyshelf_core::app::ports::TestRunPort;
use whyshelf_core::domain::approval::{TestOutcome, TestsRun};
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

/// How much of a command's output is held: the last of it. The pipes have to be read
/// or a command that prints enough would wait for ever on a full pipe, and what is
/// read must not grow with the command.
const TAIL_LIMIT: usize = 64 * 1024;

/// The last [`TAIL_LIMIT`] bytes that went through.
#[derive(Debug, Default)]
struct Tail(Vec<u8>);

impl Tail {
    fn push(&mut self, chunk: &[u8]) {
        self.0.extend_from_slice(chunk);
        if self.0.len() > TAIL_LIMIT {
            let excess = self.0.len() - TAIL_LIMIT;
            self.0.drain(..excess);
        }
    }
}

/// Read a pipe to its end, holding its tail.
async fn drain<R: AsyncRead + Unpin>(mut pipe: R) -> Tail {
    let mut tail = Tail::default();
    let mut chunk = vec![0u8; 8192];
    // A read error ends the stream like its end does: the command's status, not its
    // output, is the result.
    while let Ok(read) = pipe.read(&mut chunk).await {
        if read == 0 {
            break;
        }
        tail.push(&chunk[..read]);
    }
    tail
}

/// Stop a command and what it started. On Unix the command leads a process group of
/// its own, so a test runner that started children does not leave them running; the
/// group is killed with the `kill` utility, which needs no C library of ours.
async fn stop(child: &mut Child) {
    #[cfg(unix)]
    if let Some(pid) = child.id() {
        let group = format!("-{pid}");
        let _ = Command::new("kill")
            .args(["-KILL", "--", group.as_str()])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .await;
    }
    let _ = child.start_kill();
    let _ = child.wait().await;
}

async fn run_command(
    dir: &Path,
    command: &TestCommand,
    timeout: Duration,
) -> Result<TestsRun, TestRunError> {
    let mut shell = if cfg!(windows) {
        let mut shell = Command::new("cmd");
        shell.arg("/C");
        shell
    } else {
        let mut shell = Command::new("sh");
        shell.arg("-c");
        shell
    };
    shell
        .arg(command.as_str())
        .current_dir(dir)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    #[cfg(unix)]
    shell.process_group(0);
    let spawn_error = |source| TestRunError::Spawn {
        command: command.as_str().to_owned(),
        source,
    };
    let mut child = shell.spawn().map_err(spawn_error)?;
    let readers: Vec<_> = [
        child.stdout.take().map(|pipe| tokio::spawn(drain(pipe))),
        child.stderr.take().map(|pipe| tokio::spawn(drain(pipe))),
    ]
    .into_iter()
    .flatten()
    .collect();

    let waited = tokio::time::timeout(timeout, child.wait()).await;
    let ran = match waited {
        Ok(status) => {
            let status = status.map_err(spawn_error)?;
            Ok(TestsRun::Finished(if status.success() {
                TestOutcome::Passed
            } else {
                TestOutcome::Failed
            }))
        }
        Err(_) => {
            stop(&mut child).await;
            Ok(TestsRun::TimedOut { after: timeout })
        }
    };
    // The command is over; a child it left behind may still hold the pipes, so the
    // readers get a moment and are then cut off.
    for mut reader in readers {
        if tokio::time::timeout(Duration::from_secs(1), &mut reader)
            .await
            .is_err()
        {
            reader.abort();
        }
    }
    ran
}

impl TestRunPort for GitTestRun {
    type Error = TestRunError;

    async fn run(
        &self,
        revision: &RevisionId,
        command: &TestCommand,
        timeout: TestTimeout,
    ) -> Result<TestsRun, TestRunError> {
        let rev = full_hash(revision).map_err(TestRunError::Git)?;
        let head = self.git.head().await.map_err(TestRunError::Git)?;
        let timeout = timeout.get();
        if head.eq_ignore_ascii_case(rev) && self.git.is_clean().await.map_err(TestRunError::Git)? {
            return run_command(self.git.root(), command, timeout).await;
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

        // The checkout is the whole repository; the project may be a directory in it,
        // and its tests run in that same directory, as they do in the working tree.
        let ran = match self.git.line(["rev-parse", "--show-prefix"]).await {
            Ok(prefix) => run_command(&checkout.join(prefix), command, timeout).await,
            Err(error) => Err(TestRunError::Git(error)),
        };
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_last_of_the_output_is_held() {
        let mut tail = Tail::default();
        tail.push(&vec![b'a'; TAIL_LIMIT]);
        tail.push(b"end");
        assert_eq!(tail.0.len(), TAIL_LIMIT);
        assert!(tail.0.ends_with(b"aend"));
        // A single chunk longer than the limit keeps its last bytes.
        tail.push(&vec![b'b'; TAIL_LIMIT * 3]);
        assert_eq!(tail.0, vec![b'b'; TAIL_LIMIT]);
    }

    #[tokio::test]
    async fn a_pipe_is_read_to_its_end_holding_its_tail() {
        let data = vec![b'x'; TAIL_LIMIT * 4 + 5];
        let tail = drain(data.as_slice()).await;
        assert_eq!(tail.0.len(), TAIL_LIMIT);
    }
}
