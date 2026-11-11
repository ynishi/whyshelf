//! The project's version control, Git: the revision the work is at, the names and
//! contents of blobs, for the adapters that keep content by address, the files of
//! the work tree and the renames since a commit, for `moves`, and the `user.name` of
//! the project, for who judges.
//!
//! | what | with |
//! |---|---|
//! | [`RevisionSource`]: the commit `HEAD` names | `git rev-parse --verify HEAD` |
//! | [`Git::blob_names`]: the blob each file's content is (or would be) in, converted as Git converts it on the way in | `git config -z --name-only --list` (the keys `filter.<driver>.clean`, `process`, `smudge`, `required` picked out as bytes), then `git -c filter.<driver>.clean= … hash-object -- <paths>` (no `-w`: only the name, nothing is written) |
//! | [`Git::read_blob`]: the content of a blob, by its name | `git cat-file blob <name>` |
//! | [`Git::work_tree_files`]: the files of the work tree Git lists | `git ls-files -z --cached --others --exclude-standard` |
//! | [`Git::renames_since`]: the renames between a commit and the index | `git rev-parse --verify --quiet <commit>^{commit}`, then `git diff-index --cached --no-ext-diff --no-textconv --ignore-submodules=all --relative --find-renames --name-status -z <commit> --` |
//! | [`Git::user_name`]: who the project's settings say the user is | `git config --get user.name` |
//!
//! # Running Git: what a call may and may not do
//!
//! Every call goes through one function, `Git::run`, and the promise is kept there,
//! for every call, not by each command:
//!
//! - **What runs.** The `git` on the path, with an argument list and no shell, at the
//!   root (`-C`), with stdin closed and no prompt (`GIT_TERMINAL_PROMPT=0`).
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

use tokio::io::AsyncReadExt;
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
}

/// The Git repository a project root is in, if it is in one.
#[derive(Debug, Clone)]
pub struct Git {
    root: PathBuf,
    timeout: Duration,
}

impl Git {
    /// The repository of the project at `root`, each call bounded by [`TIMEOUT`].
    pub fn new(root: PathBuf) -> Self {
        Self {
            root,
            timeout: TIMEOUT,
        }
    }

    /// The same, each call bounded by `timeout` (for a test of the bound).
    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
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
        let Some(drivers) = self.filter_drivers().await? else {
            return Ok(None);
        };
        let shut: Vec<String> = drivers
            .iter()
            .flat_map(|driver| {
                ["clean=", "process=", "smudge=", "required=false"]
                    .map(|setting| format!("filter.{driver}.{setting}"))
            })
            .flat_map(|setting| ["-c".to_owned(), setting])
            .collect();
        self.run_bounded(args, None, &shut).await
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
        let mut command = Command::new("git");
        command
            .arg("-C")
            .arg(&self.root)
            .args(SETTINGS)
            .args(extra)
            .args(args)
            .envs(ENVIRONMENT)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
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
        let mut entries: Vec<&[u8]> = out.stdout.split(|b| *b == 0).collect();
        if out.cut {
            // The last entry of a cut listing may be a part of a path.
            entries.pop();
        }
        let mut files: Vec<RelPath> = entries.into_iter().filter_map(path_of_bytes).collect();
        files.sort();
        files.dedup();
        Ok(Some(Listing {
            files,
            cut: out.cut,
        }))
    }

    /// What Git's rename detection pairs between the commit `revision` and the index
    /// (`git diff-index --cached --find-renames --name-status -z <revision>`, paths
    /// relative to the root): each old path with its new one and how alike they are.
    /// It compares blobs and reads nothing of the work tree, so a file moved is seen
    /// once Git has it in the index (`git mv`, or moved and `git add`ed), with its
    /// content as added. Outside a repository, and for a revision that is no commit
    /// of it, nothing is looked at, and that is the answer.
    pub async fn renames_since(&self, revision: &Revision) -> Result<RenamesSince, GitError> {
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
        let Some(out) = self.run(&args).await? else {
            return Ok(RenamesSince::NotARepository);
        };
        if !out.ok() {
            return Err(GitError::Failed {
                command: "diff-index",
                stderr: out.said(),
            });
        }
        Ok(RenamesSince::Found(renames_of(&out.stdout)))
    }

    /// The content of the blob `address` names, or none when it is no name of Git's
    /// or Git holds no such blob (in a partial clone, one not fetched yet).
    pub async fn read_blob(&self, address: &ContentAddress) -> Result<Option<Vec<u8>>, GitError> {
        // A name that is not Git's is nothing Git can read, and is never passed on.
        if !is_object_name(address.as_str()) {
            return Ok(None);
        }
        Ok(
            match self.run(&["cat-file", "blob", address.as_str()]).await? {
                Some(out) if out.ok() => Some(out.stdout),
                Some(_) | None => None,
            },
        )
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

    #[tokio::test]
    async fn outside_a_repository_nothing_is_listed_and_no_rename_is_looked_for() {
        let dir = tempfile::tempdir().unwrap();
        let git = Git::new(dir.path().to_path_buf());
        assert_eq!(git.work_tree_files(1 << 20).await.unwrap(), None);
        let revision = Revision::from_validated("a".repeat(40)).unwrap();
        assert_eq!(
            git.renames_since(&revision).await.unwrap(),
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
