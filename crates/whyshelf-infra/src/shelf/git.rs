//! The project's version control, Git: the revision the work is at, the names and
//! contents of blobs, for the adapters that keep content by address, the files of
//! the work tree and the renames since a commit, for `moves`, and the `user.name` of
//! the project, for who judges.
//!
//! | what | with |
//! |---|---|
//! | [`RevisionSource`]: the commit `HEAD` names | `git rev-parse --verify HEAD` |
//! | [`Git::blob_names`]: the blob each file's content is (or would be) in, converted as Git converts it on the way in | `git config -z --name-only --list` (the keys `filter.<driver>.clean`, `process`, `smudge`, `required` picked out as bytes), then `git -c filter.<driver>.clean= … hash-object -- <paths>` (no `-w`: only the name, nothing is written) |
//! | [`Git::read_blob`], [`Git::read_blob_within`]: the content of a blob, by its name, the second within a bound of bytes | `git cat-file blob <name>` |
//! | [`Git::work_tree_files`]: the files of the work tree Git lists | `git ls-files -z --cached --others --exclude-standard` |
//! | [`Git::renames_since`]: the renames between a commit and the index | `git rev-parse --verify --quiet <commit>^{commit}`, then `git diff-index --cached --no-ext-diff --no-textconv --ignore-submodules=all --relative --find-renames --name-status -z <commit> --` |
//! | [`Git::user_name`]: who the project's settings say the user is | `git config --get user.name` |
//! | the tree of a commit, for [`GitStates`] | `git rev-parse --verify --quiet <rev>^{commit}`, `git rev-parse --show-prefix`, then `git ls-tree -r -t -z --full-tree <commit>` |
//! | the index, for [`GitStates`] | `git ls-files -s -z`, and `git diff-index --cached --quiet --no-ext-diff --no-textconv HEAD -- ':(top,glob)**/.gitattributes'` (whether its attributes are `HEAD`'s) |
//! | the blobs of either, for [`GitStates`] | `git --attr-source=<commit> cat-file --filters --path=<path> <oid>` for each file (converted by the attributes of that commit: a batch with `--filters` writes the size of the blob as stored before the content converted, so its answers cannot be read apart), and one `git cat-file --batch -Z` for the texts of the links |
//! | whether this Git knows each of the forms above, tried once when such a tree is opened | the form itself ([`TreeCall`]): a usage it does not know exits 129 |
//!
//! [`GitStates`]: super::state::git::GitStates
//!
//! # Running Git: what a call may and may not do
//!
//! Every call goes through one function, `Git::run`, and the promise is kept there,
//! for every call, not by each command:
//!
//! - **What runs.** The `git` on the path, with an argument list and no shell, at the
//!   root (`-C`), with stdin closed and no prompt (`GIT_TERMINAL_PROMPT=0`). One call
//!   is given a stdin, `cat-file --batch -Z` (`Git::blobs`, blobs as stored): what
//!   flows into it is only the lines this module writes, an oid ended by NUL, each
//!   checked to be an object name a listing gave. A call that converts a blob of a
//!   commit (`cat-file --filters`) shuts out the filter drivers as
//!   `Git::run_converting` does, and reads the attributes of a commit
//!   (`--attr-source`), not of the work tree. Only a test runs another program than
//!   `git` (`Git::with_program`).
//! - **Nothing of the repository chooses a program.** The settings a repository can
//!   write that name a program a reading could reach are shut out on the command
//!   line: no file system monitor (`-c core.fsmonitor=`), no attributes file of the
//!   settings (`-c core.attributesFile=`) and none of the system
//!   (`GIT_ATTR_NOSYSTEM=1`). The one call that converts the work tree on the way
//!   in, `hash-object`, goes through `Git::run_converting`, which first reads the
//!   filter drivers the settings define (`filter.<driver>.clean`, `process`,
//!   `smudge`, `required`) and shuts each out on the command line
//!   (`-c filter.<driver>.clean= -c filter.<driver>.process=
//!   -c filter.<driver>.smudge= -c filter.<driver>.required=false`): Git runs no
//!   filter whose command is empty, and a driver the attributes name and no setting
//!   defines runs nothing. Git's own conversions (line endings under `core.autocrlf`
//!   or `text`, `ident`, `working-tree-encoding`) run no program and are kept, so an
//!   address is the blob Git stores. A driver whose name cannot be written in a `-c`
//!   exactly (one that is not UTF-8, or holds `=`) cannot be shut out, and then no
//!   converting call runs and no blob is named; a name is never read lossily. The
//!   other calls
//!   read no work tree through attributes: the renames compare the commit with the
//!   index, which holds blobs (`diff-index --cached`, with `--no-ext-diff` and
//!   `--no-textconv`), and `ls-files` and `cat-file blob` read no content through
//!   them. Submodules are not entered (`--ignore-submodules=all`).
//! - **A reading writes nothing.** No call writes an object (`hash-object` without
//!   `-w`), the index is never refreshed (`GIT_OPTIONAL_LOCKS=0`, and the plumbing
//!   `diff-index`, which writes no index), and a partial clone never fetches a
//!   missing object from its remote (`GIT_NO_LAZY_FETCH=1`): such an object is
//!   missing, and the call says so.
//! - **No call hangs the command.** Each has a bound ([`TIMEOUT`]); past it the
//!   process is killed and that call fails with [`GitError::TimedOut`].
//! - **What it writes is read as data.** Paths are written as they are
//!   (`-c core.quotepath=off`) and read from `-z` output as [`RelPath`]s, or left out;
//!   a commit or a blob name is checked to be an object name before it is passed,
//!   so a record cannot pass an option.
//!
//! What the promise does not cover: Git reads the repository's settings and its
//! ignore rules (`--exclude-standard`), and `config --get user.name` reads the
//! settings it is asked about. The caller's own environment (`GIT_*` variables, the
//! person's global settings) is the caller's.
//!
//! The revision is the project's, not a space's or the documents': it is the one
//! [`RevisionSource`] of a project. The blob names and contents are used by the
//! adapter of the documents ([`docs`](super::docs)) and by the adapter of the space
//! `file` ([`artifact::file`](super::artifact::file)), each for its own content.
//!
//! # A blob name is of the content as Git stores it
//!
//! `hash-object` names the blob Git would store for a file, line endings converted,
//! so a text file of a repository that converts them (`core.autocrlf`,
//! `* text=auto`, the usual case on Windows) reads back. The hash whyshelf compares
//! is of the bytes in the work tree, and an address is never compared (rule A4):
//! only reading back reads it. For a file a filter driver would change, the blob
//! named is of the content unfiltered, which Git does not hold: it reads back as not
//! readable.
//!
//! # Outside Git
//!
//! Outside a repository, or with no `git` to run, there is no blob name and no
//! revision (`None`), and that is no error: a record made there keeps none, and
//! `get --approved` says the content cannot be read back because there is no
//! address. Content that was approved and never committed has a name Git does not
//! hold: it reads back as not readable, which is the truth.
//!
//! # What crosses into the domain
//!
//! A blob name and a commit are checked here to be what Git writes (40 or 64
//! lowercase hexadecimal digits) before [`ContentAddress::from_validated`] or
//! [`Revision::from_validated`] makes a value of them (rule A7), and a name read back
//! from a record is checked again before it is handed to Git, so a record cannot pass
//! an option to the command.
//!
//! [`RevisionSource`]: whyshelf_core::app::shelf::RevisionSource

use std::collections::BTreeSet;
use std::io;
use std::path::PathBuf;
use std::process::{ExitStatus, Stdio};
use std::time::Duration;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::process::Command;
use whyshelf_core::app::shelf::RevisionSource;
use whyshelf_core::domain::location::RelPath;
use whyshelf_core::domain::shelf::artifact::{ContentAddress, OpaqueProblem, Revision};

/// How long one call of Git may take before it is killed and fails: long enough for
/// a listing or a rename detection of a large repository, short enough that a call
/// that hangs (a lock, a network file system) is told rather than waited on.
pub const TIMEOUT: Duration = Duration::from_secs(30);

/// The settings every call runs with, before its command (see the module page).
const SETTINGS: [&str; 6] = [
    "-c",
    "core.quotepath=off",
    "-c",
    "core.fsmonitor=",
    "-c",
    "core.attributesFile=",
];

/// The environment every call runs with, beside the caller's (see the module page).
const ENVIRONMENT: [(&str, &str); 4] = [
    ("GIT_OPTIONAL_LOCKS", "0"),
    ("GIT_TERMINAL_PROMPT", "0"),
    ("GIT_NO_LAZY_FETCH", "1"),
    ("GIT_ATTR_NOSYSTEM", "1"),
];

/// Why Git gave no usable answer.
#[derive(Debug, thiserror::Error)]
pub enum GitError {
    /// Git, in a repository, named an object in a form it does not write.
    #[error("git gave `{text}`, which is no object name")]
    BadName {
        /// What it gave.
        text: String,
    },
    /// The domain refused a name Git gave (a bug if it ever happens: the form is
    /// checked first).
    #[error("the domain refused the object name `{text}`")]
    Refused {
        /// The name.
        text: String,
        /// Why.
        #[source]
        source: OpaqueProblem,
    },
    /// `git hash-object` failed in a repository, for files that are there.
    #[error("git hash-object failed: {stderr}")]
    HashObject {
        /// What Git said.
        stderr: String,
    },
    /// `git ls-files` or `git diff-index`, run in a repository, exited with a
    /// failure (for `diff-index`, an object missing from a partial clone is one).
    #[error("git {command} failed: {stderr}")]
    Failed {
        /// The command.
        command: &'static str,
        /// What Git said.
        stderr: String,
    },
    /// A call took longer than its bound ([`TIMEOUT`]) and was killed.
    #[error("git {command} took longer than {} s and was stopped", .after.as_secs())]
    TimedOut {
        /// The command.
        command: String,
        /// The bound.
        after: Duration,
    },
    /// A call wrote more than the bytes it may (its `cap`), and was stopped: what it
    /// gave is a part, and is not read.
    #[error("git {command} wrote more than {cap} bytes, and was stopped")]
    Cut {
        /// The command.
        command: &'static str,
        /// The bound, in bytes.
        cap: usize,
    },
    /// What Git gave could not be read, as the blocking thread it was read on
    /// stopped (a bug if it ever happens, or a runtime shutting down).
    #[error("the reading of what git {command} gave stopped: {why}")]
    Stopped {
        /// The command.
        command: &'static str,
        /// Why the thread stopped.
        why: String,
    },
    /// `git` was started and could not be talked to (its pipes, its exit), or could
    /// not be started for another reason than its absence.
    #[error("cannot run git {command}")]
    Run {
        /// The command.
        command: String,
        /// The operating system's error.
        #[source]
        source: io::Error,
    },
}

/// What one call gave.
#[derive(Debug)]
struct Ran {
    status: ExitStatus,
    stdout: Vec<u8>,
    stderr: Vec<u8>,
    /// Whether stdout was cut at the bound it was read with.
    cut: bool,
}

impl Ran {
    /// Whether it succeeded.
    fn ok(&self) -> bool {
        self.status.success()
    }

    /// What it said on stderr, trimmed.
    fn said(&self) -> String {
        first_line(&String::from_utf8_lossy(&self.stderr))
    }
}

/// The files of the work tree [`Git::work_tree_files`] lists.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Listing {
    /// The files, sorted, each once.
    pub files: Vec<RelPath>,
    /// Whether the listing was cut at the bound it was read with, so files are
    /// missing from it.
    pub cut: bool,
    /// How many entries Git listed that are no path of the project (not UTF-8, or
    /// refused by [`RelPath`]): files that are there and are left out of `files`.
    pub unnamed: usize,
}

/// The Git repository a project root is in, if it is in one.
#[derive(Debug, Clone)]
pub struct Git {
    root: PathBuf,
    timeout: Duration,
    /// The program run: `git` on the path, and nothing else outside a test.
    program: PathBuf,
}

impl Git {
    /// The repository of the project at `root`, each call bounded by [`TIMEOUT`].
    pub fn new(root: PathBuf) -> Self {
        Self {
            root,
            timeout: TIMEOUT,
            program: PathBuf::from("git"),
        }
    }

    /// The same, each call bounded by `timeout` (for a test of the bound).
    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    /// The same, running `program` in place of the `git` on the path: what a test
    /// stands a Git in with, that knows less than the one installed.
    #[cfg(test)]
    pub(crate) fn with_program(mut self, program: PathBuf) -> Self {
        self.program = program;
        self
    }

    /// The one place Git is run: `args` (a command and its arguments) at the root,
    /// keeping the promise of the module page for every call. `None` when no `git`
    /// can be found to start.
    async fn run(&self, args: &[&str]) -> Result<Option<Ran>, GitError> {
        self.run_bounded(args, None, &[]).await
    }

    /// As [`Git::run`], for a call that converts the work tree on the way in (the one
    /// such call is `hash-object`; any other must come here too): every filter
    /// driver the settings define is shut out first, on the command line, and Git's
    /// own conversions are kept (the module page). `None` too when a driver's name
    /// cannot be written in a `-c`, so it could not be shut out.
    async fn run_converting(&self, args: &[&str]) -> Result<Option<Ran>, GitError> {
        let Some(shut) = self.drivers_shut_out().await? else {
            return Ok(None);
        };
        self.run_bounded(args, None, &shut).await
    }

    /// The settings that shut out every filter driver the settings define, or none
    /// when one cannot be named in a `-c` (or no `git` can run): what a converting
    /// call runs with.
    async fn drivers_shut_out(&self) -> Result<Option<Vec<String>>, GitError> {
        let Some(drivers) = self.filter_drivers().await? else {
            return Ok(None);
        };
        Ok(Some(
            drivers
                .iter()
                .flat_map(|driver| {
                    ["clean=", "process=", "smudge=", "required=false"]
                        .map(|setting| format!("filter.{driver}.{setting}"))
                })
                .flat_map(|setting| ["-c".to_owned(), setting])
                .collect(),
        ))
    }

    /// The command of one call, as the promise of the module page has it, but for its
    /// stdin: the program, at the root, with the settings and the environment every
    /// call runs with, `extra` before the command `args`, stdout and stderr piped, and
    /// killed when dropped.
    fn command(&self, extra: &[String], args: &[&str]) -> Command {
        let mut command = Command::new(&self.program);
        command
            .arg("-C")
            .arg(&self.root)
            .args(SETTINGS)
            .args(extra)
            .args(args)
            .envs(ENVIRONMENT)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        command
    }

    /// The names of the filter drivers the settings define, or none when one of them
    /// cannot be named exactly in a `-c` (not UTF-8, or holding a `=`, which a `-c`
    /// would read as the end of the key), or no `git` can run.
    async fn filter_drivers(&self) -> Result<Option<BTreeSet<String>>, GitError> {
        // Every key, matched here as bytes: Git's own regular expression does not
        // match a key that is not valid text in its locale, and would let its driver
        // through unseen.
        let Some(out) = self.run(&["config", "-z", "--name-only", "--list"]).await? else {
            return Ok(None);
        };
        if !out.ok() {
            return Err(GitError::Failed {
                command: "config",
                stderr: out.said(),
            });
        }
        let mut drivers = BTreeSet::new();
        // A key of a driver's program that is not UTF-8, or whose driver holds a `=`,
        // cannot be named exactly in a `-c`, so no converting call runs at all: never
        // a lossy reading, which would shut out another driver than the one the
        // settings name. (Git writes the section and the variable in lower case.)
        for key in out.stdout.split(|b| *b == 0).filter(|k| !k.is_empty()) {
            let Some(rest) = key.strip_prefix(b"filter.") else {
                continue;
            };
            let Some(dot) = rest.iter().rposition(|b| *b == b'.') else {
                continue;
            };
            let (driver, variable) = (&rest[..dot], &rest[dot + 1..]);
            if !matches!(variable, b"clean" | b"process" | b"smudge" | b"required") {
                continue;
            }
            let Ok(driver) = std::str::from_utf8(driver) else {
                return Ok(None);
            };
            if driver.is_empty() || driver.contains('=') {
                return Ok(None);
            }
            drivers.insert(driver.to_owned());
        }
        Ok(Some(drivers))
    }

    /// As [`Git::run`], with `extra` settings before the command, reading at most
    /// `cap` bytes of stdout when given: past them the pipe is closed, and Git stops
    /// on its next write.
    async fn run_bounded(
        &self,
        args: &[&str],
        cap: Option<usize>,
        extra: &[String],
    ) -> Result<Option<Ran>, GitError> {
        let command_word = args.first().copied().unwrap_or("").to_owned();
        let failed = |source: io::Error| GitError::Run {
            command: command_word.clone(),
            source,
        };
        let mut command = self.command(extra, args);
        command.stdin(Stdio::null());
        let mut child = match command.spawn() {
            Ok(child) => child,
            // No `git` to run: the same as outside a repository.
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(source) => return Err(failed(source)),
        };
        let (Some(mut stdout), Some(mut stderr)) = (child.stdout.take(), child.stderr.take())
        else {
            return Err(failed(io::Error::other("git was started with no pipes")));
        };
        let work = async {
            let read_out = async move {
                let mut out = Vec::new();
                match cap {
                    Some(cap) => {
                        let limit = u64::try_from(cap).unwrap_or(u64::MAX).saturating_add(1);
                        (&mut stdout).take(limit).read_to_end(&mut out).await?;
                    }
                    None => {
                        stdout.read_to_end(&mut out).await?;
                    }
                }
                // `stdout` is dropped here: a command cut short ends on its next write.
                Ok::<_, io::Error>(out)
            };
            let read_err = async move {
                let mut err = Vec::new();
                stderr.read_to_end(&mut err).await?;
                Ok::<_, io::Error>(err)
            };
            let (out, err) = tokio::try_join!(read_out, read_err)?;
            let status = child.wait().await?;
            Ok::<_, io::Error>((status, out, err))
        };
        match bounded(self.timeout, command_word.clone(), work).await {
            // The child is dropped with this function, and killed (`kill_on_drop`).
            Err(timed_out) => Err(timed_out),
            Ok(Err(source)) => Err(failed(source)),
            Ok(Ok((status, mut stdout, stderr))) => {
                let cut = cap.is_some_and(|cap| stdout.len() > cap);
                if let Some(cap) = cap.filter(|_| cut) {
                    stdout.truncate(cap);
                }
                Ok(Some(Ran {
                    status,
                    stdout,
                    stderr,
                    cut,
                }))
            }
        }
    }

    /// Whether the root is in a Git work tree that `git` can be run in.
    async fn in_repository(&self) -> Result<bool, GitError> {
        Ok(
            match self.run(&["rev-parse", "--is-inside-work-tree"]).await? {
                Some(out) => out.ok() && out.stdout.trim_ascii() == b"true",
                None => false,
            },
        )
    }

    /// The blob names of these files (relative to the root, each there), in order:
    /// the blob Git would store for each, its line endings converted as Git converts
    /// them and no filter driver run; none outside a repository, or when a driver
    /// could not be shut out (the module page, "Running Git").
    pub async fn blob_names(
        &self,
        paths: &[&RelPath],
    ) -> Result<Option<Vec<ContentAddress>>, GitError> {
        if paths.is_empty() || !self.in_repository().await? {
            return Ok(None);
        }
        let mut args = vec!["hash-object", "--"];
        args.extend(paths.iter().map(|p| p.as_str()));
        let Some(out) = self.run_converting(&args).await? else {
            return Ok(None);
        };
        if !out.ok() {
            return Err(GitError::HashObject { stderr: out.said() });
        }
        let text = String::from_utf8_lossy(&out.stdout);
        let names = text
            .lines()
            .map(|line| {
                let name = object_name(line)?;
                ContentAddress::from_validated(name.clone())
                    .map_err(|source| GitError::Refused { text: name, source })
            })
            .collect::<Result<Vec<_>, _>>()?;
        if names.len() != paths.len() {
            return Err(GitError::BadName {
                text: text.into_owned(),
            });
        }
        Ok(Some(names))
    }

    /// The files of the work tree Git lists, relative to the root: those it tracks
    /// and those it does not that no ignore rule leaves out (`git ls-files --cached
    /// --others --exclude-standard`), each once, or none outside a repository. A file
    /// tracked and deleted is among them; a name that is no path of the root
    /// ([`RelPath`]) or not UTF-8 is not. At most `cap` bytes of the listing are
    /// read: past them it is cut, and says so.
    pub async fn work_tree_files(&self, cap: usize) -> Result<Option<Listing>, GitError> {
        if !self.in_repository().await? {
            return Ok(None);
        }
        let args = [
            "ls-files",
            "-z",
            "--cached",
            "--others",
            "--exclude-standard",
            "--",
        ];
        let Some(out) = self.run_bounded(&args, Some(cap), &[]).await? else {
            return Ok(None);
        };
        // A listing cut short was stopped on purpose; its status says nothing.
        if !out.cut && !out.ok() {
            return Err(GitError::Failed {
                command: "ls-files",
                stderr: out.said(),
            });
        }
        let cut = out.cut;
        // The listing may be as large as `cap`: it is cut into paths on a blocking
        // thread, not on the async ones.
        let (files, unnamed) = tokio::task::spawn_blocking(move || listed(&out.stdout, cut))
            .await
            .map_err(|e| GitError::Stopped {
                command: "ls-files",
                why: e.to_string(),
            })?;
        Ok(Some(Listing {
            files,
            cut,
            unnamed,
        }))
    }

    /// What Git's rename detection pairs between the commit `revision` and the index
    /// (`git diff-index --cached --find-renames --name-status -z <revision>`, paths
    /// relative to the root): each old path with its new one and how alike they are.
    /// It compares blobs and reads nothing of the work tree, so a file moved is seen
    /// once Git has it in the index (`git mv`, or moved and `git add`ed), with its
    /// content as added. Outside a repository, and for a revision that is no commit
    /// of it, nothing is looked at, and that is the answer. At most `cap` bytes of
    /// Git's answer are read: past them it is [`GitError::Cut`], never the renames of a
    /// part of it.
    pub async fn renames_since(
        &self,
        revision: &Revision,
        cap: usize,
    ) -> Result<RenamesSince, GitError> {
        if !self.in_repository().await? {
            return Ok(RenamesSince::NotARepository);
        }
        // A revision read from a record is checked again before it reaches Git, so it
        // cannot pass an option.
        let rev = revision.as_str();
        if !is_object_name(rev) {
            return Ok(RenamesSince::UnknownRevision);
        }
        let commit = format!("{rev}^{{commit}}");
        match self
            .run(&["rev-parse", "--verify", "--quiet", &commit])
            .await?
        {
            Some(out) if out.ok() => {}
            Some(_) | None => return Ok(RenamesSince::UnknownRevision),
        }
        let args = [
            "diff-index",
            "--cached",
            "--no-ext-diff",
            "--no-textconv",
            "--ignore-submodules=all",
            "--relative",
            "--find-renames",
            "--name-status",
            "-z",
            rev,
            "--",
        ];
        let Some(out) = self.run_bounded(&args, Some(cap), &[]).await? else {
            return Ok(RenamesSince::NotARepository);
        };
        if out.cut {
            return Err(GitError::Cut {
                command: "diff-index",
                cap,
            });
        }
        if !out.ok() {
            return Err(GitError::Failed {
                command: "diff-index",
                stderr: out.said(),
            });
        }
        // The answer may be as large as `cap`: it is read into renames on a blocking
        // thread, not on the async ones.
        let renames = tokio::task::spawn_blocking(move || renames_of(&out.stdout))
            .await
            .map_err(|e| GitError::Stopped {
                command: "diff-index",
                why: e.to_string(),
            })?;
        Ok(RenamesSince::Found(renames))
    }

    /// The content of the blob `address` names, or none when it is no name of Git's
    /// or Git holds no such blob (in a partial clone, one not fetched yet).
    pub async fn read_blob(&self, address: &ContentAddress) -> Result<Option<Vec<u8>>, GitError> {
        self.blob(address, None).await
    }

    /// As [`Git::read_blob`], reading at most `cap` bytes: a blob larger than that is
    /// none, as it cannot be read back whole.
    pub async fn read_blob_within(
        &self,
        address: &ContentAddress,
        cap: usize,
    ) -> Result<Option<Vec<u8>>, GitError> {
        self.blob(address, Some(cap)).await
    }

    /// The content of the blob `address` names, within `cap` bytes when given.
    async fn blob(
        &self,
        address: &ContentAddress,
        cap: Option<usize>,
    ) -> Result<Option<Vec<u8>>, GitError> {
        // A name that is not Git's is nothing Git can read, and is never passed on.
        if !is_object_name(address.as_str()) {
            return Ok(None);
        }
        let args = ["cat-file", "blob", address.as_str()];
        Ok(match self.run_bounded(&args, cap, &[]).await? {
            Some(out) if out.ok() && !out.cut => Some(out.stdout),
            Some(_) | None => None,
        })
    }

    /// The `user.name` Git's settings give at the root (read even outside a
    /// repository), or none when they give none or there is no `git`.
    pub async fn user_name(&self) -> Result<Option<String>, GitError> {
        let Some(out) = self.run(&["config", "--get", "user.name"]).await? else {
            return Ok(None);
        };
        let name = String::from_utf8_lossy(&out.stdout).trim().to_owned();
        Ok((out.ok() && !name.is_empty()).then_some(name))
    }

    /// The commit `rev` names, as its object name, or none when it names no commit
    /// (or the root is in no repository, or no `git` runs). A revision that starts
    /// with `-` is none: it is never passed, so it cannot pass an option.
    pub(crate) async fn commit(&self, rev: &str) -> Result<Option<String>, GitError> {
        if rev.is_empty() || rev.starts_with('-') {
            return Ok(None);
        }
        let commit = format!("{rev}^{{commit}}");
        let Some(out) = self
            .run(&["rev-parse", "--verify", "--quiet", &commit])
            .await?
        else {
            return Ok(None);
        };
        if !out.ok() {
            return Ok(None);
        }
        object_name(&String::from_utf8_lossy(&out.stdout)).map(Some)
    }

    /// Whether a `git` can be started at all (`git --version`).
    pub(crate) async fn runs(&self) -> Result<bool, GitError> {
        Ok(self.run(&["--version"]).await?.is_some_and(|out| out.ok()))
    }

    /// Where the root is in its repository, from the top of the work tree (`""` at
    /// the top, `dir/` below it), as the bytes Git wrote (the paths of a listing are
    /// bytes too, and are compared with it), or none outside a repository.
    pub(crate) async fn prefix(&self) -> Result<Option<Vec<u8>>, GitError> {
        let Some(out) = self.run(&["rev-parse", "--show-prefix"]).await? else {
            return Ok(None);
        };
        if !out.ok() {
            return Ok(None);
        }
        let mut prefix = out.stdout;
        while matches!(prefix.last(), Some(b'\n' | b'\r')) {
            prefix.pop();
        }
        Ok(Some(prefix))
    }

    /// Whether this Git knows the form `call` reads with, tried once on `commit`: a
    /// Git that does not exits as it does for a usage it does not know (129). None
    /// when no `git` runs.
    pub(crate) async fn knows(
        &self,
        call: TreeCall,
        commit: &str,
    ) -> Result<Option<bool>, GitError> {
        let source = format!("{ATTR_SOURCE}={commit}");
        let ran = match call {
            TreeCall::AttrSource => {
                self.run_bounded(
                    &["rev-parse", "--verify", "--quiet", commit],
                    None,
                    &[source],
                )
                .await?
            }
            TreeCall::Converted => {
                let mut args = CONVERTED.to_vec();
                args.extend(["--path=.gitattributes", commit]);
                self.run_bounded(&args, Some(0), &[source]).await?
            }
            TreeCall::Stored => self.run_bounded(&STORED, None, &[]).await?,
            TreeCall::TreeListing => {
                let mut args = TREE_LISTING.to_vec();
                args.push(commit);
                self.run_bounded(&args, Some(0), &[]).await?
            }
            TreeCall::IndexListing => self.run_bounded(&INDEX_LISTING, Some(0), &[]).await?,
        };
        let Some(out) = ran else {
            return Ok(None);
        };
        // Only a usage Git does not know says it does not know the form: a trial that
        // fails otherwise (an object of another kind than the form reads) knew it.
        Ok(Some(out.cut || out.status.code() != Some(USAGE)))
    }

    /// Every entry of the tree of `commit` (`ls-tree -r -t -z --full-tree`, trees
    /// too), within `cap` bytes of the listing: [`TreeListing::Past`] beyond them.
    pub(crate) async fn tree_entries(
        &self,
        commit: &str,
        cap: usize,
    ) -> Result<TreeListing, GitError> {
        let mut args = TREE_LISTING.to_vec();
        args.push(commit);
        self.entries(&args, cap, TreeCall::TreeListing).await
    }

    /// Every entry of the index under the root (`ls-files -s -z`), each with its
    /// stage, within `cap` bytes: [`TreeListing::Past`] beyond them.
    pub(crate) async fn index_entries(&self, cap: usize) -> Result<TreeListing, GitError> {
        self.entries(&INDEX_LISTING, cap, TreeCall::IndexListing)
            .await
    }

    async fn entries(
        &self,
        args: &[&str],
        cap: usize,
        call: TreeCall,
    ) -> Result<TreeListing, GitError> {
        let Some(out) = self.run_bounded(args, Some(cap), &[]).await? else {
            return Err(GitError::Failed {
                command: call.command(),
                stderr: "no git can be run".to_owned(),
            });
        };
        if out.cut {
            return Ok(TreeListing::Past);
        }
        if !out.ok() {
            return Err(GitError::Failed {
                command: call.command(),
                stderr: out.said(),
            });
        }
        let index = call == TreeCall::IndexListing;
        let stdout = out.stdout;
        tokio::task::spawn_blocking(move || tree_entries_of(&stdout, index))
            .await
            .map_err(|e| GitError::Stopped {
                command: call.command(),
                why: e.to_string(),
            })?
            .map(TreeListing::Entries)
    }

    /// Whether the `.gitattributes` files of the index are those of `HEAD`, blob for
    /// blob (no filter run): `diff-index --cached --quiet HEAD` over them.
    pub(crate) async fn index_attributes_as_head(&self) -> Result<bool, GitError> {
        let args = [
            "diff-index",
            "--cached",
            "--quiet",
            "--no-ext-diff",
            "--no-textconv",
            "HEAD",
            "--",
            ":(top,glob)**/.gitattributes",
        ];
        let Some(out) = self.run(&args).await? else {
            return Ok(false);
        };
        match out.status.code() {
            Some(0) => Ok(true),
            Some(1) => Ok(false),
            _ => Err(GitError::Failed {
                command: "diff-index",
                stderr: out.said(),
            }),
        }
    }

    /// The blobs `asks` name, in order, each of at most `keep` bytes held (the rest
    /// read and counted): converted as a checkout writes them, by the attributes of
    /// the tree `convert` names with every filter driver shut out, each by its own
    /// `cat-file --filters` (a batch writes the size of the blob as stored before the
    /// content converted, so a batch of converted blobs cannot be read apart); or as
    /// stored when `convert` is none, all by one `cat-file --batch -Z`, its stdin the
    /// lines written here (the module page). None when a driver could not be shut
    /// out, or no `git` runs.
    pub(crate) async fn blobs(
        &self,
        convert: Option<&str>,
        asks: &[BlobAsk],
        keep: u64,
    ) -> Result<Option<Vec<Blob>>, GitError> {
        if asks.is_empty() {
            return Ok(Some(Vec::new()));
        }
        for ask in asks {
            if !is_object_name(&ask.oid) || ask.path.contains(&0) {
                return Err(GitError::BadName {
                    text: ask.oid.clone(),
                });
            }
        }
        let Some(source) = convert else {
            return self.stored(asks, keep).await;
        };
        if !is_object_name(source) {
            return Err(GitError::BadName {
                text: source.to_owned(),
            });
        }
        let Some(mut extra) = self.drivers_shut_out().await? else {
            return Ok(None);
        };
        extra.push(format!("{ATTR_SOURCE}={source}"));
        let mut blobs = Vec::with_capacity(asks.len());
        for ask in asks {
            let path = format!("--path={}", String::from_utf8_lossy(&ask.path));
            let mut args = CONVERTED.to_vec();
            args.extend([path.as_str(), ask.oid.as_str()]);
            match self.counted(&extra, &args, keep).await? {
                Some(blob) => blobs.push(blob),
                None => return Ok(None),
            }
        }
        Ok(Some(blobs))
    }

    /// One call whose stdout is a content: at most `keep` bytes of it held, all of it
    /// counted; a call that fails is a blob Git cannot give.
    async fn counted(
        &self,
        extra: &[String],
        args: &[&str],
        keep: u64,
    ) -> Result<Option<Blob>, GitError> {
        let command_word = args.first().copied().unwrap_or("").to_owned();
        let failed = |source: io::Error| GitError::Run {
            command: command_word.clone(),
            source,
        };
        let mut command = self.command(extra, args);
        command.stdin(Stdio::null());
        let mut child = match command.spawn() {
            Ok(child) => child,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(source) => return Err(failed(source)),
        };
        let (Some(mut stdout), Some(mut stderr)) = (child.stdout.take(), child.stderr.take())
        else {
            return Err(failed(io::Error::other("git was started with no pipes")));
        };
        let work = async {
            let read_out = async move {
                let mut bytes = Vec::new();
                (&mut stdout).take(keep).read_to_end(&mut bytes).await?;
                let rest = tokio::io::copy(&mut stdout, &mut tokio::io::sink()).await?;
                let len = u64::try_from(bytes.len())
                    .unwrap_or(u64::MAX)
                    .saturating_add(rest);
                Ok::<_, io::Error>((len, bytes))
            };
            let read_err = async move {
                let mut err = Vec::new();
                stderr.read_to_end(&mut err).await?;
                Ok::<_, io::Error>(err)
            };
            let ((len, bytes), _) = tokio::try_join!(read_out, read_err)?;
            let status = child.wait().await?;
            Ok::<_, io::Error>((status, len, bytes))
        };
        match bounded(self.timeout, command_word.clone(), work).await? {
            Err(source) => Err(failed(source)),
            Ok((status, _, _)) if !status.success() => Ok(Some(Blob::Missing)),
            Ok((_, len, bytes)) => Ok(Some(Blob::There { len, bytes })),
        }
    }

    /// The blobs `asks` name, as stored, by one `cat-file --batch -Z`.
    async fn stored(&self, asks: &[BlobAsk], keep: u64) -> Result<Option<Vec<Blob>>, GitError> {
        let (extra, args): (Vec<String>, &[&str]) = (Vec::new(), &STORED);
        let mut input = Vec::new();
        for ask in asks {
            input.extend_from_slice(ask.oid.as_bytes());
            input.push(0);
        }
        let failed = |source: io::Error| GitError::Run {
            command: "cat-file".to_owned(),
            source,
        };
        let mut command = self.command(&extra, args);
        command.stdin(Stdio::piped());
        let mut child = match command.spawn() {
            Ok(child) => child,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(source) => return Err(failed(source)),
        };
        let (Some(mut stdin), Some(stdout), Some(mut stderr)) =
            (child.stdin.take(), child.stdout.take(), child.stderr.take())
        else {
            return Err(failed(io::Error::other("git was started with no pipes")));
        };
        let count = asks.len();
        let work = async {
            let write = async move {
                stdin.write_all(&input).await?;
                // stdin is dropped here: Git reads to its end and stops.
                Ok::<_, io::Error>(())
            };
            let read = async move {
                let mut out = tokio::io::BufReader::new(stdout);
                let mut blobs = Vec::with_capacity(count);
                for _ in 0..count {
                    blobs.push(read_blob_answer(&mut out, keep).await?);
                }
                Ok::<_, io::Error>(blobs)
            };
            let read_err = async move {
                let mut err = Vec::new();
                stderr.read_to_end(&mut err).await?;
                Ok::<_, io::Error>(err)
            };
            let ((), blobs, err) = tokio::try_join!(write, read, read_err)?;
            let status = child.wait().await?;
            Ok::<_, io::Error>((status, blobs, err))
        };
        match bounded(self.timeout, "cat-file".to_owned(), work).await? {
            Err(source) => Err(GitError::Failed {
                command: "cat-file",
                stderr: source.to_string(),
            }),
            Ok((status, _, err)) if !status.success() => Err(GitError::Failed {
                command: "cat-file",
                stderr: first_line(&String::from_utf8_lossy(&err)),
            }),
            Ok((_, blobs, _)) => Ok(Some(blobs)),
        }
    }
}

/// The exit status of Git for a usage it does not know (an option, a command).
const USAGE: i32 = 129;

/// The global option a call converts by the attributes of a tree with.
const ATTR_SOURCE: &str = "--attr-source";

/// The reading of a blob converted by the attributes of a path (`--path=<path>
/// <oid>` after it).
const CONVERTED: [&str; 2] = ["cat-file", "--filters"];

/// The reading of blobs as stored, their names on stdin.
const STORED: [&str; 3] = ["cat-file", "--batch", "-Z"];

/// The listing of the tree of a commit, before the commit.
const TREE_LISTING: [&str; 5] = ["ls-tree", "-r", "-t", "-z", "--full-tree"];

/// The listing of the index, each entry with its stage.
const INDEX_LISTING: [&str; 3] = ["ls-files", "-s", "-z"];

/// A form of a call the tree of a commit, or of the index, is read with: the one
/// definition of what is run, which both the reading and its trial when the tree is
/// opened (`Git::knows`) read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TreeCall {
    /// The attributes of a tree, not of the work tree (`--attr-source=<commit>`).
    AttrSource,
    /// A blob, converted by those attributes (`cat-file --filters`).
    Converted,
    /// Blobs as stored, the texts of links (`cat-file --batch -Z`).
    Stored,
    /// The tree of a commit (`ls-tree -r -t -z --full-tree`).
    TreeListing,
    /// The index (`ls-files -s -z`).
    IndexListing,
}

impl TreeCall {
    /// The calls a tree of commits is read with, in the order they are tried.
    pub const OF_COMMITS: [Self; 4] = [
        Self::AttrSource,
        Self::Converted,
        Self::Stored,
        Self::TreeListing,
    ];

    /// The form, as it is written.
    pub fn written(self) -> String {
        match self {
            Self::AttrSource => ATTR_SOURCE.to_owned(),
            Self::Converted => CONVERTED.join(" "),
            Self::Stored => STORED.join(" "),
            Self::TreeListing => TREE_LISTING.join(" "),
            Self::IndexListing => INDEX_LISTING.join(" "),
        }
    }

    /// The command it runs.
    fn command(self) -> &'static str {
        match self {
            Self::AttrSource => "rev-parse",
            Self::Converted => CONVERTED[0],
            Self::Stored => STORED[0],
            Self::TreeListing => TREE_LISTING[0],
            Self::IndexListing => INDEX_LISTING[0],
        }
    }
}

/// What a listing of a tree gave.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum TreeListing {
    /// Every entry.
    Entries(Vec<TreeEntry>),
    /// The listing went past the bytes it may hold.
    Past,
}

/// One entry of a listing of a tree: its mode, object, stage (0 but for a conflict
/// of the index) and path as Git wrote it (from the top of the work tree for a
/// commit, from the root for the index).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TreeEntry {
    pub(crate) mode: u32,
    pub(crate) oid: String,
    pub(crate) stage: u8,
    pub(crate) path: Vec<u8>,
}

/// The entries of `ls-tree -z` (`<mode> <type> <oid>\t<path>`) or `ls-files -s -z`
/// (`<mode> <oid> <stage>\t<path>`), each object name checked.
fn tree_entries_of(out: &[u8], index: bool) -> Result<Vec<TreeEntry>, GitError> {
    let bad = |record: &[u8]| GitError::BadName {
        text: String::from_utf8_lossy(record).into_owned(),
    };
    let mut entries = Vec::new();
    for record in out.split(|b| *b == 0).filter(|r| !r.is_empty()) {
        let tab = record
            .iter()
            .position(|b| *b == b'\t')
            .ok_or_else(|| bad(record))?;
        let (head, path) = (&record[..tab], &record[tab + 1..]);
        let head = std::str::from_utf8(head).map_err(|_| bad(record))?;
        let words: Vec<&str> = head.split(' ').collect();
        let (mode, oid, stage) = match (index, words.as_slice()) {
            (false, [mode, _, oid]) => (*mode, *oid, "0"),
            (true, [mode, oid, stage]) => (*mode, *oid, *stage),
            _ => return Err(bad(record)),
        };
        if !is_object_name(oid) {
            return Err(bad(record));
        }
        entries.push(TreeEntry {
            mode: u32::from_str_radix(mode, 8).map_err(|_| bad(record))?,
            oid: oid.to_owned(),
            stage: stage.parse().map_err(|_| bad(record))?,
            path: path.to_vec(),
        });
    }
    Ok(entries)
}

/// A blob to read: its object, and the path its attributes are read for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct BlobAsk {
    pub(crate) oid: String,
    pub(crate) path: Vec<u8>,
}

/// What a blob read was.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Blob {
    /// Its length, and its first bytes, at most as many as were to be held.
    There { len: u64, bytes: Vec<u8> },
    /// Git holds no such object.
    Missing,
}

/// One answer of `cat-file --batch -Z`: a header ended by NUL (`<oid> <type> <size>`,
/// or `<oid> missing`), then that many bytes and a NUL.
async fn read_blob_answer<R: tokio::io::AsyncBufRead + Unpin>(
    out: &mut R,
    keep: u64,
) -> io::Result<Blob> {
    use tokio::io::AsyncBufReadExt;
    let unreadable = |what: &str| io::Error::other(format!("cat-file wrote {what}"));
    let mut header = Vec::new();
    out.read_until(0, &mut header).await?;
    if header.pop() != Some(0) {
        return Err(unreadable("less than an answer"));
    }
    let header = String::from_utf8_lossy(&header);
    let words: Vec<&str> = header.split(' ').collect();
    let size = match words.as_slice() {
        [_, "missing"] => return Ok(Blob::Missing),
        [_, _, size] => size
            .parse::<u64>()
            .map_err(|_| unreadable("a header with no size"))?,
        _ => return Err(unreadable("a header it does not write")),
    };
    let mut bytes = Vec::new();
    (&mut *out)
        .take(size.min(keep))
        .read_to_end(&mut bytes)
        .await?;
    let rest = size - u64::try_from(bytes.len()).unwrap_or(u64::MAX).min(size);
    tokio::io::copy(&mut (&mut *out).take(rest), &mut tokio::io::sink()).await?;
    let mut end = [0u8; 1];
    out.read_exact(&mut end).await?;
    if end[0] != 0 {
        return Err(unreadable("an answer longer than its header says"));
    }
    Ok(Blob::There { len: size, bytes })
}

/// What [`Git::renames_since`] found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RenamesSince {
    /// The root is in no Git work tree (or `git` cannot be run).
    NotARepository,
    /// The revision is no commit of the repository.
    UnknownRevision,
    /// The pairs Git's rename detection gave, in its order.
    Found(Vec<Rename>),
}

/// One rename Git detected: an old path, the new one, and how alike they are.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rename {
    /// The path at the revision.
    pub from: RelPath,
    /// The path in the index.
    pub to: RelPath,
    /// Git's score, a percent, when it gave one.
    pub similarity: Option<u8>,
}

/// The renames of `git diff-index --name-status -z`: a status (`R` and a score for a
/// rename), then its paths, each ended by a NUL. Every other status (`M`, `A`, `D`,
/// `C` and the rest) is read past; a path that is no [`RelPath`] leaves its pair out.
/// `work` within `bound`, or [`GitError::TimedOut`] naming `command`: the one bound
/// every call of [`Git`] runs under.
async fn bounded<T>(
    bound: Duration,
    command: String,
    work: impl std::future::Future<Output = T>,
) -> Result<T, GitError> {
    tokio::time::timeout(bound, work)
        .await
        .map_err(|_| GitError::TimedOut {
            command,
            after: bound,
        })
}

fn renames_of(output: &[u8]) -> Vec<Rename> {
    let mut fields = output.split(|b| *b == 0).filter(|f| !f.is_empty());
    let mut renames = Vec::new();
    while let Some(status) = fields.next() {
        let paths = match status.first() {
            Some(b'R' | b'C') => 2,
            _ => 1,
        };
        let taken: Vec<&[u8]> = fields.by_ref().take(paths).collect();
        if status.first() != Some(&b'R') {
            continue;
        }
        let (Some(from), Some(to)) = (
            taken.first().and_then(|p| path_of_bytes(p)),
            taken.get(1).and_then(|p| path_of_bytes(p)),
        ) else {
            continue;
        };
        let similarity = std::str::from_utf8(&status[1..])
            .ok()
            .and_then(|digits| digits.parse::<u8>().ok())
            .filter(|percent| *percent <= 100);
        renames.push(Rename {
            from,
            to,
            similarity,
        });
    }
    renames
}

/// The first line of a text that is not blank, trimmed: what Git said, in one line.
pub fn first_line(text: &str) -> String {
    text.lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .unwrap_or("")
        .to_owned()
}

/// A path Git wrote, when it is UTF-8 and a path of the root.
fn path_of_bytes(bytes: &[u8]) -> Option<RelPath> {
    std::str::from_utf8(bytes).ok()?.parse().ok()
}

/// The paths of a listing of `git ls-files -z`, sorted, each once, and how many
/// entries are no path of the project; the last entry of a listing that was `cut` is
/// left out, as it may be a part of a path, and so is the empty one after the last
/// separator.
fn listed(stdout: &[u8], cut: bool) -> (Vec<RelPath>, usize) {
    let mut entries: Vec<&[u8]> = stdout.split(|b| *b == 0).collect();
    if cut {
        entries.pop();
    }
    let mut files = Vec::new();
    let mut unnamed = 0;
    for entry in entries.into_iter().filter(|entry| !entry.is_empty()) {
        match path_of_bytes(entry) {
            Some(path) => files.push(path),
            None => unnamed += 1,
        }
    }
    files.sort();
    files.dedup();
    (files, unnamed)
}

/// Whether a text is an object name as Git writes it: 40 (SHA-1) or 64 (SHA-256)
/// lowercase hexadecimal digits.
fn is_object_name(text: &str) -> bool {
    matches!(text.len(), 40 | 64)
        && text
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

/// A line of Git's output, checked to be an object name.
fn object_name(line: &str) -> Result<String, GitError> {
    let text = line.trim();
    if is_object_name(text) {
        Ok(text.to_owned())
    } else {
        Err(GitError::BadName {
            text: text.to_owned(),
        })
    }
}

impl RevisionSource for Git {
    type Error = GitError;

    async fn revision(&self) -> Result<Option<Revision>, GitError> {
        let Some(out) = self
            .run(&["rev-parse", "--verify", "--quiet", "HEAD"])
            .await?
        else {
            return Ok(None);
        };
        if !out.ok() {
            // Outside a repository, or before the first commit.
            return Ok(None);
        }
        let text = object_name(&String::from_utf8_lossy(&out.stdout))?;
        Revision::from_validated(text.clone())
            .map(Some)
            .map_err(|source| GitError::Refused { text, source })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_listed_entry_that_is_no_path_is_counted_not_dropped_in_silence() {
        // Not UTF-8, and not a relative path: both counted; the empty entry after
        // the last separator is no entry.
        let (files, unnamed) = listed(b"b.rs\0\xff.rs\0/abs\0a.rs\0", false);
        assert_eq!(
            files.iter().map(RelPath::as_str).collect::<Vec<_>>(),
            ["a.rs", "b.rs"]
        );
        assert_eq!(unnamed, 2);
        // A cut listing leaves out its last entry, which may be a part of a path,
        // and does not count it.
        let (files, unnamed) = listed(b"a.rs\0\xff", true);
        assert_eq!(files.len(), 1);
        assert_eq!(unnamed, 0);
    }

    #[test]
    fn an_object_name_is_forty_or_sixty_four_lowercase_hex_digits() {
        assert!(is_object_name(&"a".repeat(40)));
        assert!(is_object_name(&"0".repeat(64)));
        assert!(!is_object_name(&"A".repeat(40)));
        assert!(!is_object_name(&"a".repeat(39)));
        assert!(!is_object_name("-p"));
    }

    #[test]
    fn the_renames_are_read_from_gits_nul_separated_output() {
        let out = b"M\0notes/N-1.md\0R087\0src/a b.rs\0src/b.rs\0D\0gone.rs\0C075\0x\0y\0R100\0../out\0in\0R\0p\0q\0";
        let renames = renames_of(out);
        assert_eq!(renames.len(), 2, "{renames:?}");
        assert_eq!(renames[0].from.as_str(), "src/a b.rs");
        assert_eq!(renames[0].to.as_str(), "src/b.rs");
        assert_eq!(renames[0].similarity, Some(87));
        // A copy is no rename, a path out of the root leaves its pair out, and a
        // rename with no score has none.
        assert_eq!(renames[1].from.as_str(), "p");
        assert_eq!(renames[1].similarity, None);
    }

    /// The prefix is the bytes Git wrote: a directory whose name is not UTF-8 is
    /// kept as it is, not as its lossy text. Passes without a check where the file
    /// system refuses such a name (macOS), or no `git` runs.
    #[cfg(unix)]
    #[tokio::test]
    async fn the_prefix_keeps_the_bytes_of_a_directory_that_is_not_utf8() {
        use std::os::unix::ffi::OsStrExt;
        let dir = tempfile::tempdir().unwrap();
        let init = Command::new("git")
            .arg("-C")
            .arg(dir.path())
            .args(["init", "-q"])
            .output()
            .await;
        if !init.is_ok_and(|out| out.status.success()) {
            eprintln!("note: no git runs here, the test is skipped");
            return;
        }
        let odd = dir.path().join(std::ffi::OsStr::from_bytes(b"d\xff"));
        if tokio::fs::create_dir(&odd).await.is_err() {
            eprintln!("note: this file system refuses a name that is not UTF-8, skipped");
            return;
        }
        let git = Git::new(odd);
        assert_eq!(git.prefix().await.unwrap(), Some(b"d\xff/".to_vec()));
    }

    #[tokio::test]
    async fn outside_a_repository_nothing_is_listed_and_no_rename_is_looked_for() {
        let dir = tempfile::tempdir().unwrap();
        let git = Git::new(dir.path().to_path_buf());
        assert_eq!(git.work_tree_files(1 << 20).await.unwrap(), None);
        let revision = Revision::from_validated("a".repeat(40)).unwrap();
        assert_eq!(
            git.renames_since(&revision, 1 << 20).await.unwrap(),
            RenamesSince::NotARepository
        );
    }

    #[tokio::test]
    async fn outside_a_repository_there_is_no_revision_and_no_blob() {
        let dir = tempfile::tempdir().unwrap();
        let git = Git::new(dir.path().to_path_buf());
        assert_eq!(git.revision().await.unwrap(), None);
        let file: RelPath = "a".parse().unwrap();
        assert_eq!(git.blob_names(&[&file]).await.unwrap(), None);
        for text in ["--output=/tmp/x", "abc", &"g".repeat(40)] {
            let address = ContentAddress::from_validated(text.to_owned()).unwrap();
            assert_eq!(git.read_blob(&address).await.unwrap(), None);
        }
    }

    /// A call past its bound fails and says which. The bound is tested on work that
    /// never ends, so the test does not depend on how fast Git answers: under a bound
    /// of zero, a Git that has already exited when first polled would still answer.
    #[tokio::test]
    async fn a_call_past_its_bound_fails_and_says_which() {
        let bound = Duration::from_millis(10);
        match bounded(bound, "rev-parse".to_owned(), std::future::pending::<()>()).await {
            Err(GitError::TimedOut { command, after }) => {
                assert_eq!(command, "rev-parse");
                assert_eq!(after, bound);
            }
            other => panic!("a call past its bound fails: {other:?}"),
        }
        // Work that ends within its bound gives its answer.
        assert_eq!(
            bounded(bound, "rev-parse".to_owned(), async { 7 })
                .await
                .unwrap(),
            7
        );
    }
}
