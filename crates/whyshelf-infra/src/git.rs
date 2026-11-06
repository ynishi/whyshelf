//! Git: [`RevisionPort`], [`CodeScanPort`] and [`RevisionFactsPort`] (see
//! [`GitFacts`]) over the `git` command.
//!
//! The model speaks only of a revision, of being integrated and of a clean tree;
//! Git's words stay in this module. Every call starts `git` with `tokio::process`
//! in the project root and reads its output; none of them changes the repository.
//!
//! | question | git |
//! |---|---|
//! | is this text a revision (`validate`) | 40 hexadecimal digits, checked without starting a process |
//! | the revision of the working tree | `rev-parse --verify --quiet HEAD`; before the first commit it answers none, which is no failure |
//! | is the tree clean | `status --porcelain`, **leaving out `.whyshelf/`** |
//! | has a revision reached the baseline | `rev-parse --verify <baseline>^{commit}`, then `merge-base --is-ancestor <rev> <baseline commit>`, and when that fails `rev-parse --verify <rev>^{commit}` to tell a missing commit from a failure |
//! | the name of the line of work | `rev-parse --abbrev-ref HEAD` (none when detached) |
//! | the pins in the tests at a revision | one `grep -I -n -z -e '' <rev> -- <pathspecs>` (see [`GitCodeScan`]) |
//!
//! **A clone that cannot say is not a failure.** A baseline that names no commit
//! here, or an approved commit that is not in the clone (a shallow or single-branch
//! checkout of a CI machine), makes the answer "unknown", with the reason
//! ([`IntegrationFact::Unknown`]). The baseline text comes from the committed
//! settings, so it is handed to Git only as a revision (after `--end-of-options`) and
//! only as a commit (`^{commit}`). Any other failure of `merge-base` is an error.
//!
//! **Why `.whyshelf/` does not make the tree dirty.** The tool itself writes
//! events, objects and generated files there; if those counted, recording one
//! approval would make the next one fail with "commit first". What matters to an
//! approval at a revision is that the code is that revision, so only the rest of
//! the tree is looked at.
//!
//! **Why the pin scan reads the revision, not the working tree.** A CA approval
//! is made at one revision; the pins it checks must be those of that revision,
//! however the tree has changed since, and reading Git's object store needs no
//! checkout.

use std::collections::{BTreeSet, HashMap};
use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::{Arc, Mutex};

use regex::Regex;
use tokio::process::Command;
use whyshelf_core::app::config::{PinPattern, PinSearch};
use whyshelf_core::app::ports::{
    Author, BaselineAnswer, BaselineQuery, CodeScanPort, PinScan, RevisionFactsPort, RevisionPort,
    WorkTree, WorktreeRoot,
};
use whyshelf_core::app::raw::RawRevision;
use whyshelf_core::domain::corpus::{GitPlace, Visibility};
use whyshelf_core::domain::error::{FingerprintError, ValueError};
use whyshelf_core::domain::hash::HunkId;
use whyshelf_core::domain::integration::{
    BaselineFacts, ChangeFingerprint, FingerprintUnavailable, Fingerprinting, Hunk, HunkScheme,
    UnknownWhy,
};
use whyshelf_core::domain::judge::{Email, PersonName};
use whyshelf_core::domain::knowable::Knowable;
use whyshelf_core::domain::location::{CodeLocation, RelPath, TestFileSet};
use whyshelf_core::domain::nonempty::NonBlank;
use whyshelf_core::domain::pin::{MalformedPin, PinId};
use whyshelf_core::domain::revision::{IntegrationFact, Revision, RevisionId, UnknownIntegration};

use crate::config::DEFAULT_PIN_PATTERN;

/// Why a Git question could not be answered.
#[derive(Debug, thiserror::Error)]
pub enum GitError {
    /// `git` could not be started (not installed, or the directory is gone).
    #[error("cannot start `git {command}`")]
    Spawn {
        /// The arguments, for the message.
        command: String,
        /// The operating system's error.
        #[source]
        source: std::io::Error,
    },
    /// `git` ran and failed.
    #[error("`git {command}` failed ({status}): {stderr}")]
    Failed {
        /// The arguments, for the message.
        command: String,
        /// The exit status, or `signal` when it was killed.
        status: String,
        /// What Git printed on its error stream, trimmed.
        stderr: String,
    },
    /// `git` printed something that is not what the question expects.
    #[error("`git {command}` printed an unexpected answer")]
    Unexpected {
        /// The arguments, for the message.
        command: String,
    },
    /// A revision is not 40 hexadecimal digits, so it cannot be handed to Git as it is.
    #[error("`{text}` is not a full 40-digit revision")]
    NotFullHash {
        /// The text.
        text: String,
    },
    /// A value Git gave was refused by the rule of its type.
    #[error("Git gave a value the domain refuses")]
    Value(#[source] ValueError),
    /// The fingerprint of a change could not be built for a reason that is not an
    /// absence of one.
    #[error("the fingerprint of the change cannot be built")]
    Fingerprint(#[source] FingerprintError),
    /// The pin pattern is not a regular expression.
    #[error("the pin pattern does not compile")]
    Pattern(#[source] regex::Error),
    /// The pin pattern has no group that captures the pin.
    #[error("the pin pattern needs a group that captures the pin name")]
    PatternGroup,
}

/// A place to run `git` in: the project root.
#[derive(Debug, Clone)]
pub(crate) struct Git {
    root: PathBuf,
}

/// What a finished `git` printed.
#[derive(Debug)]
pub(crate) struct Finished {
    pub(crate) code: Option<i32>,
    pub(crate) stdout: Vec<u8>,
    pub(crate) stderr: Vec<u8>,
}

impl Git {
    pub(crate) fn new(root: PathBuf) -> Self {
        Self { root }
    }

    pub(crate) fn root(&self) -> &Path {
        &self.root
    }

    /// Run `git` and return its output whatever the exit status.
    pub(crate) async fn finish<I, S>(&self, args: I) -> Result<Finished, GitError>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        let args: Vec<S> = args.into_iter().collect();
        let command = describe(&args);
        let output = Command::new("git")
            .args(&args)
            .current_dir(&self.root)
            .env("GIT_OPTIONAL_LOCKS", "0")
            .env("GIT_TERMINAL_PROMPT", "0")
            .stdin(Stdio::null())
            .output()
            .await
            .map_err(|source| GitError::Spawn { command, source })?;
        Ok(Finished {
            code: output.status.code(),
            stdout: output.stdout,
            stderr: output.stderr,
        })
    }

    /// Run `git` and require exit status 0.
    pub(crate) async fn run<I, S>(&self, args: I) -> Result<Vec<u8>, GitError>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        let args: Vec<S> = args.into_iter().collect();
        let finished = self.finish(&args).await?;
        if finished.code == Some(0) {
            Ok(finished.stdout)
        } else {
            Err(failed(&args, &finished))
        }
    }

    /// Run `git` and read its output as one trimmed line of text.
    pub(crate) async fn line<I, S>(&self, args: I) -> Result<String, GitError>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        let args: Vec<S> = args.into_iter().collect();
        let stdout = self.run(&args).await?;
        String::from_utf8(stdout)
            .map(|text| text.trim().to_owned())
            .map_err(|_| GitError::Unexpected {
                command: describe(&args),
            })
    }

    /// The revision `HEAD` names.
    pub(crate) async fn head(&self) -> Result<String, GitError> {
        let args = ["rev-parse", "HEAD"];
        let head = self.line(args).await?;
        if is_full_hash(&head) {
            Ok(head)
        } else {
            Err(GitError::Unexpected {
                command: describe(&args),
            })
        }
    }

    /// The revision `HEAD` names, or `None` before the first commit (an unborn
    /// `HEAD`). `rev-parse --verify --quiet` answers that case with exit status 1 and
    /// nothing printed; any other failure (not a repository, say) is an error.
    pub(crate) async fn head_if_born(&self) -> Result<Option<String>, GitError> {
        let args = ["rev-parse", "--verify", "--quiet", "HEAD"];
        let finished = self.finish(args).await?;
        match finished.code {
            Some(0) => {
                let head = String::from_utf8_lossy(&finished.stdout).trim().to_owned();
                if is_full_hash(&head) {
                    Ok(Some(head))
                } else {
                    Err(GitError::Unexpected {
                        command: describe(&args),
                    })
                }
            }
            Some(1) if finished.stderr.is_empty() => Ok(None),
            _ => Err(failed(&args, &finished)),
        }
    }

    /// The commit a revision text names, or `None` when it names no commit here.
    ///
    /// The text may come from a committed file (the baseline), so it is handed to Git
    /// as a revision and nothing else (`--end-of-options`: it cannot be read as an
    /// option) and only as a commit (`^{commit}`: a tree is not a baseline).
    /// `rev-parse --verify` answers a text that is no commit of this repository with
    /// a failure of its own, which is the answer and not an error; only a `git` that
    /// cannot run is one.
    pub(crate) async fn commit_of(&self, text: &str) -> Result<Option<String>, GitError> {
        let spec = format!("{text}^{{commit}}");
        let args = [
            "rev-parse",
            "--verify",
            "--quiet",
            "--end-of-options",
            spec.as_str(),
        ];
        let finished = self.finish(args).await?;
        if finished.code != Some(0) {
            return Ok(None);
        }
        let printed = String::from_utf8_lossy(&finished.stdout).trim().to_owned();
        Ok(is_full_hash(&printed).then_some(printed))
    }

    /// Whether the directory is inside a work tree. `false` for a directory outside a
    /// repository and for a bare one, which is an answer; only a `git` that cannot run
    /// is an error.
    pub(crate) async fn inside_work_tree(&self) -> Result<bool, GitError> {
        let finished = self.finish(["rev-parse", "--is-inside-work-tree"]).await?;
        Ok(finished.code == Some(0) && finished.stdout.trim_ascii() == b"true")
    }

    /// Whether the working tree has no change outside `.whyshelf/`.
    pub(crate) async fn is_clean(&self) -> Result<bool, GitError> {
        let stdout = self
            .run(["status", "--porcelain", "--", ".", ":(exclude).whyshelf"])
            .await?;
        Ok(stdout.is_empty())
    }
}

fn describe<S: AsRef<OsStr>>(args: &[S]) -> String {
    args.iter()
        .map(|arg| arg.as_ref().to_string_lossy().into_owned())
        .collect::<Vec<_>>()
        .join(" ")
}

fn failed<S: AsRef<OsStr>>(args: &[S], finished: &Finished) -> GitError {
    GitError::Failed {
        command: describe(args),
        status: finished
            .code
            .map_or_else(|| "signal".to_owned(), |code| format!("exit {code}")),
        stderr: String::from_utf8_lossy(&finished.stderr).trim().to_owned(),
    }
}

/// Whether a text is 40 hexadecimal digits.
pub(crate) fn is_full_hash(text: &str) -> bool {
    text.len() == 40 && text.bytes().all(|b| b.is_ascii_hexdigit())
}

/// A revision as an argument for Git: only a full hash is accepted, so a
/// revision can never be taken for an option or for another kind of name.
pub(crate) fn full_hash(revision: &RevisionId) -> Result<&str, GitError> {
    if is_full_hash(revision.as_str()) {
        Ok(revision.as_str())
    } else {
        Err(GitError::NotFullHash {
            text: revision.as_str().to_owned(),
        })
    }
}

/// The version-control side of a workspace.
#[derive(Debug, Clone)]
pub struct GitRevision {
    git: Git,
    baseline: String,
}

impl GitRevision {
    /// Questions about the repository of `root`; a revision is integrated when it
    /// is an ancestor of `baseline` (a ref such as `main`).
    pub fn new(root: PathBuf, baseline: String) -> Self {
        Self {
            git: Git::new(root),
            baseline,
        }
    }

    /// Remove the records of worktrees whose directories are gone, so that the
    /// leftovers of a test run that was cut short do not pile up.
    pub async fn prune_worktrees(&self) -> Result<(), GitError> {
        self.git.run(["worktree", "prune"]).await.map(|_| ())
    }
}

impl RevisionPort for GitRevision {
    type Error = GitError;

    async fn validate(&self, text: &RawRevision) -> Result<Option<RevisionId>, GitError> {
        let text = text.as_str().trim();
        if !is_full_hash(text) {
            return Ok(None);
        }
        RevisionId::from_validated(text.to_ascii_lowercase())
            .map(Some)
            .map_err(GitError::Value)
    }

    async fn current(&self) -> Result<Option<RevisionId>, GitError> {
        match self.git.head_if_born().await? {
            Some(head) => RevisionId::from_validated(head)
                .map(Some)
                .map_err(GitError::Value),
            None => Ok(None),
        }
    }

    async fn is_clean(&self) -> Result<bool, GitError> {
        self.git.is_clean().await
    }

    async fn integration(&self, revision: &RevisionId) -> Result<IntegrationFact, GitError> {
        let rev = full_hash(revision)?;
        // A clone without the baseline or without the commit cannot say "no". The
        // baseline is asked for as a commit first; the revision only when `merge-base`
        // fails, so that what is left of its failures is a failure of the tool.
        let Some(baseline) = self.git.commit_of(self.baseline.as_str()).await? else {
            return Ok(IntegrationFact::Unknown {
                reason: UnknownIntegration::BaselineUnresolved,
            });
        };
        let args = ["merge-base", "--is-ancestor", rev, baseline.as_str()];
        let finished = self.git.finish(args).await?;
        match finished.code {
            Some(0) => Ok(IntegrationFact::Integrated),
            Some(1) => Ok(IntegrationFact::NotIntegrated),
            _ if self.git.commit_of(rev).await?.is_none() => Ok(IntegrationFact::Unknown {
                reason: UnknownIntegration::RevisionAbsent,
            }),
            _ => Err(failed(&args, &finished)),
        }
    }

    async fn label(&self) -> Result<Option<String>, GitError> {
        let name = self.git.line(["rev-parse", "--abbrev-ref", "HEAD"]).await?;
        Ok((name != "HEAD").then_some(name))
    }

    async fn worktree(&self) -> Result<WorktreeRoot, GitError> {
        WorktreeRoot::new(self.git.root().display().to_string()).map_err(GitError::Value)
    }
}

/// Finds pins in the test files of a revision.
///
/// **One `git grep` for the whole scan.** `git grep -I -n -z -e '' <revision> --
/// <pathspecs>` prints every line of every text file the pathspecs name, in the
/// revision and not in the working tree, and skips binary files itself (`-I`). The
/// pathspecs are the globs of `test.files`, each as `:(glob)<glob>` and as
/// `:(glob)<glob>/**` so that a directory name means everything under it. Each line is
/// then matched against the pin pattern **here**, by the regular-expression engine
/// that checked the pattern when the settings were read, and the first group is the
/// pin.
///
/// Why the pattern is not given to `git grep`: `-E` is the C library's POSIX dialect,
/// which differs from place to place (the same `\s` is whitespace on one machine and a
/// letter on another), so the same settings would find different pins on two
/// machines. Matching the pattern here means one dialect everywhere, and Git still
/// does what it is good at: reading the revision's files in one process, with the
/// pathspec.
///
/// A file counts when it is under one of the scan directories **and** a glob names
/// it. A tag whose name is no pin's is returned as malformed, not dropped.
#[derive(Debug, Clone)]
pub struct GitCodeScan {
    git: Git,
}

impl GitCodeScan {
    /// Scan the repository of `root`.
    pub fn new(root: PathBuf) -> Self {
        Self {
            git: Git::new(root),
        }
    }
}

/// One line of `git grep -n -z`: `<revision>:<path>`, NUL, the line number, NUL, the
/// line. `None` for anything that is not that shape.
fn grep_record<'a>(
    record: &'a [u8],
    prefix: &str,
) -> Option<(RelPath, u32, std::borrow::Cow<'a, str>)> {
    let mut parts = record.splitn(3, |byte| *byte == 0);
    let (path, number, line) = (parts.next()?, parts.next()?, parts.next()?);
    let path = std::str::from_utf8(path).ok()?.strip_prefix(prefix)?;
    let number: u32 = std::str::from_utf8(number).ok()?.parse().ok()?;
    Some((path.parse().ok()?, number, String::from_utf8_lossy(line)))
}

/// Whether a path is a scan directory or lies under one.
fn under(path: &RelPath, dirs: &[RelPath]) -> bool {
    dirs.iter().any(|dir| {
        path.as_str() == dir.as_str()
            || path
                .as_str()
                .strip_prefix(dir.as_str())
                .is_some_and(|rest| rest.starts_with('/'))
    })
}

/// The pathspecs that make a glob mean the files it names, and a directory
/// everything under it.
fn pathspecs(files: &TestFileSet) -> Vec<String> {
    files
        .globs()
        .iter()
        .flat_map(|glob| {
            let glob = glob.trim_end_matches('/');
            [format!(":(glob){glob}"), format!(":(glob){glob}/**")]
        })
        .collect()
}

/// The regular expression that cuts a mark out of a line, built from the settings of
/// the scan.
///
/// The project's own pattern if it gave one, else [`DEFAULT_PIN_PATTERN`]. The
/// expression only delimits a mark; it does not say what a pin is. The vocabulary of
/// the search decides that, mark by mark ([`PinId::parse_in`]), so a mark of a prefix
/// the vocabulary does not list is found by the same expression and reported as
/// malformed, and a custom prefix needs no change of the expression.
fn mark_regex(search: &PinSearch) -> Result<Regex, GitError> {
    let text = search
        .pattern()
        .map_or(DEFAULT_PIN_PATTERN, PinPattern::as_str);
    let re = Regex::new(text).map_err(GitError::Pattern)?;
    if re.captures_len() < 2 {
        return Err(GitError::PatternGroup);
    }
    Ok(re)
}

impl CodeScanPort for GitCodeScan {
    type Error = GitError;

    async fn pins_at(
        &self,
        revision: &RevisionId,
        scan_dirs: &[RelPath],
        files: &TestFileSet,
        search: &PinSearch,
    ) -> Result<PinScan, GitError> {
        let rev = full_hash(revision)?;
        let re = mark_regex(search)?;
        let mut scan = PinScan::default();
        if scan_dirs.is_empty() {
            return Ok(scan);
        }
        let mut args: Vec<String> = ["grep", "-I", "-n", "-z", "--no-color", "-e", "", rev, "--"]
            .iter()
            .map(|arg| (*arg).to_owned())
            .collect();
        args.extend(pathspecs(files));
        let finished = self.git.finish(&args).await?;
        match finished.code {
            Some(0) => {}
            // Nothing in the revision matches the pathspecs (or the files are empty).
            Some(1) => return Ok(scan),
            _ => return Err(failed(&args, &finished)),
        }
        let prefix = format!("{rev}:");
        for record in finished.stdout.split(|byte| *byte == b'\n') {
            let Some((path, number, line)) = grep_record(record, &prefix) else {
                continue;
            };
            if !under(&path, scan_dirs) {
                continue;
            }
            for captures in re.captures_iter(&line) {
                let Some(named) = captures.get(1) else {
                    continue;
                };
                let location = CodeLocation::new(path.clone(), number).map_err(GitError::Value)?;
                match PinId::parse_in(search.vocabulary(), named.as_str()) {
                    Ok(pin) => scan.pins.entry(pin).or_default().push(location),
                    Err(_) => scan.malformed.push(MalformedPin::new(
                        location,
                        NonBlank::new(named.as_str()).map_err(GitError::Value)?,
                    )),
                }
            }
        }
        Ok(scan)
    }
}

// --- the facts the corpus model takes as values --------------------------------------

/// How many commits after the approval's tip of the baseline are looked through, by
/// default (`integration.window`).
pub const DEFAULT_WINDOW: usize = 1000;

/// How many commits between where the change branched off and the approval's tip of
/// the baseline are looked through, by default (`integration.backlog`).
pub const DEFAULT_BACKLOG: usize = 5000;

/// How many commits one `git log` reads.
const LOG_CHUNK: usize = 64;

/// The hunks of a commit, per commit and per scheme.
type HunkCache = HashMap<(String, String), Arc<BTreeSet<HunkId>>>;

/// More entries than this and the cache starts again: it is made of things that can be
/// made again, and the point is to bound it, not to keep it.
const CACHE_LIMIT: usize = 200_000;

/// The flags every diff the hunks are cut from is made with, on the approval's side
/// and on the baseline's alike. Whatever the person's configuration says (renames,
/// prefixes, the algorithm, quoting) is overridden, since two sides cut with different
/// flags would cut different hunks from the same change.
const DIFF_FLAGS: [&str; 8] = [
    "-U0",
    "--no-renames",
    "--no-color",
    "--no-ext-diff",
    "--no-textconv",
    "--diff-algorithm=myers",
    "--src-prefix=a/",
    "--dst-prefix=b/",
];

/// What version control says about the code, in the terms of the corpus model:
/// [`RevisionFactsPort`] over the `git` command.
///
/// | question | git |
/// |---|---|
/// | what Git makes of a path ([`git_place`](RevisionFactsPort::git_place)) | `ls-files --error-unmatch` (tracked), else `check-ignore` (ignored), else untracked; a path outside the work tree is outside |
/// | who wrote a revision ([`author`](RevisionFactsPort::author)) | `log -1 --format=%an%x00%ae` |
/// | the fingerprint of a change ([`change_fingerprint`](RevisionFactsPort::change_fingerprint)) | the baseline's `rev-list --first-parent`, a search along it for the base, and `diff -U0 <base> <revision>` cut into hunks |
/// | whether a change landed ([`baseline_facts`](RevisionFactsPort::baseline_facts)) | `merge-base --is-ancestor`, and `log --first-parent -p -U0` over the window |
///
/// # The base
///
/// The *base* of an approved revision is its nearest ancestor on the baseline's
/// first-parent line: the commit of the baseline the change branched off. It is found
/// by a binary search along that line (a commit on the line is an ancestor of the
/// revision exactly when every older one is, so the ancestors form the old end of the
/// line), and it is on the line by its definition, so a change that branched off
/// another feature branch (a stacked pull request) still has one. A revision that is
/// itself on the line has no change of its own, and no fingerprint. The *baseline at
/// approval* is the line's tip then.
///
/// # The hunks
///
/// Every diff is cut with `-U0` and the flags of the `DIFF_FLAGS` constant, and a
/// file's header is read only before its first hunk, so a removed line that starts
/// with `-- ` is not taken for a header. A merge commit on the line is read as its
/// diff against its first parent: the whole of what the merge brought in, which is
/// what makes a merge-commit flow land by its hunks as well as by ancestry. Binary
/// files have no hunks. A path that is no [`RelPath`] is left out, which can only make
/// a change look less landed, never more.
///
/// # What is looked through
///
/// Along the line, the commits strictly after the base up to the baseline's tip at
/// approval (at most `backlog` of them, from the tip at approval back), then the
/// commits after the tip at approval, oldest first (at most `window`). The search
/// stops at the first commit that completes the set. A commit the search had to cut
/// off, when the set is not complete, makes the answer
/// [`UnknownWhy::OutsideWindow`]; nothing cut off makes it a "no".
///
/// # Not knowing
///
/// | what is missing | the answer |
/// |---|---|
/// | the baseline is no commit here | both facts unknown, [`UnknownWhy::BaselineUnresolved`] |
/// | the approved commit is not here | the ancestry is unknown: [`UnknownWhy::Shallow`] in a shallow clone, else [`UnknownWhy::BaseMissing`] |
/// | the base or the tip at approval is not on the baseline's line here | the hunks are unknown, for the same reasons |
/// | the approval recorded no fingerprint | the hunks are unknown, [`UnknownWhy::NoFingerprint`] |
/// | the fingerprint's scheme is not one this build has | the hunks are unknown, [`UnknownWhy::FingerprintVersion`] |
///
/// A revision that is a patch is never an ancestor, has no fingerprint, and has no
/// known author.
///
/// # The cache
///
/// The hunks of a commit are kept per commit and scheme, in memory, for as long as
/// this value (and its clones) live: it is made from the commit and can be made again,
/// so dropping it changes nothing but the time. The cache port of the application
/// ([`IndexCachePort`](whyshelf_core::app::ports::IndexCachePort)) holds file stamps
/// and searchable text and has no place for it.
#[derive(Debug, Clone)]
pub struct GitFacts {
    git: Git,
    baseline: String,
    private_root: Option<PathBuf>,
    window: usize,
    backlog: usize,
    cache: Arc<Mutex<HunkCache>>,
}

/// One part of a diff or a log: the hunks of one commit (or of the whole diff).
struct Section {
    commit: Option<String>,
    hunks: Vec<Hunk>,
}

impl GitFacts {
    /// The facts of the repository of `root`, against `baseline` (a ref such as
    /// `main`), with the default limits.
    pub fn new(root: PathBuf, baseline: String) -> Self {
        Self {
            git: Git::new(root),
            baseline,
            private_root: None,
            window: DEFAULT_WINDOW,
            backlog: DEFAULT_BACKLOG,
            cache: Arc::default(),
        }
    }

    /// The directory of the private root, for [`git_place`](RevisionFactsPort::git_place)
    /// of a private file. Without it, what Git makes of a private path is unknown.
    pub fn with_private_root(mut self, root: PathBuf) -> Self {
        self.private_root = Some(root);
        self
    }

    /// The limits of what is looked through: `window` commits after the approval's tip
    /// of the baseline, `backlog` before it.
    pub fn with_limits(mut self, window: usize, backlog: usize) -> Self {
        self.window = window;
        self.backlog = backlog;
        self
    }

    async fn is_shallow(&self) -> Result<bool, GitError> {
        Ok(self
            .git
            .line(["rev-parse", "--is-shallow-repository"])
            .await?
            == "true")
    }

    /// Why a commit the model needs is not on the line here.
    async fn missing(&self) -> Result<UnknownWhy, GitError> {
        Ok(if self.is_shallow().await? {
            UnknownWhy::Shallow
        } else {
            UnknownWhy::BaseMissing
        })
    }

    /// The same reason, as it is recorded beside an approval made in this clone.
    async fn unavailable(&self) -> Result<FingerprintUnavailable, GitError> {
        Ok(if self.is_shallow().await? {
            FingerprintUnavailable::Shallow
        } else {
            FingerprintUnavailable::BaseMissing
        })
    }

    /// The commits of the baseline's first-parent line, newest first.
    async fn first_parent_line(&self, tip: &str) -> Result<Vec<String>, GitError> {
        let stdout = self.git.run(["rev-list", "--first-parent", tip]).await?;
        Ok(String::from_utf8_lossy(&stdout)
            .lines()
            .map(str::to_owned)
            .collect())
    }

    /// The index in `line` of the newest commit that is an ancestor of `approved` (or
    /// is it), or `None` when none is.
    async fn nearest_on_line(
        &self,
        line: &[String],
        approved: &str,
    ) -> Result<Option<usize>, GitError> {
        let (mut low, mut high) = (0, line.len());
        while low < high {
            let mid = low + (high - low) / 2;
            let args = ["merge-base", "--is-ancestor", line[mid].as_str(), approved];
            let finished = self.git.finish(args).await?;
            match finished.code {
                Some(0) => high = mid,
                Some(1) => low = mid + 1,
                _ => return Err(failed(&args, &finished)),
            }
        }
        Ok((low < line.len()).then_some(low))
    }

    /// The hunk identities of these commits, each cut from its diff against its first
    /// parent, from the cache where it is there.
    async fn commit_hunks(
        &self,
        commits: &[&String],
        scheme: &HunkScheme,
    ) -> Result<Vec<Arc<BTreeSet<HunkId>>>, GitError> {
        let key = |commit: &str| (commit.to_owned(), scheme.name().to_owned());
        let missing: Vec<&String> = {
            let cache = self.cache.lock().unwrap_or_else(|e| e.into_inner());
            commits
                .iter()
                .copied()
                .filter(|c| !cache.contains_key(&key(c)))
                .collect()
        };
        if !missing.is_empty() {
            let mut args: Vec<String> = ["-c", "core.quotepath=false", "log"]
                .into_iter()
                .chain(["--no-walk=unsorted", "--first-parent", "-p"])
                .chain(DIFF_FLAGS)
                .chain(["--format=%x1e%H"])
                .map(str::to_owned)
                .collect();
            args.extend(missing.iter().map(|c| (*c).clone()));
            args.push("--".to_owned());
            let stdout = self.git.run(&args).await?;
            let text = String::from_utf8_lossy(&stdout);
            let mut cache = self.cache.lock().unwrap_or_else(|e| e.into_inner());
            if cache.len() > CACHE_LIMIT {
                cache.clear();
            }
            for commit in &missing {
                cache.insert(key(commit), Arc::default());
            }
            for section in parse_diff(&text) {
                let Some(commit) = section.commit else {
                    continue;
                };
                let ids: BTreeSet<HunkId> = section
                    .hunks
                    .iter()
                    .filter_map(|hunk| hunk.id(scheme))
                    .collect();
                cache.insert(key(&commit), Arc::new(ids));
            }
        }
        let cache = self.cache.lock().unwrap_or_else(|e| e.into_inner());
        Ok(commits
            .iter()
            .map(|c| cache.get(&key(c)).cloned().unwrap_or_default())
            .collect())
    }

    /// Whether every hunk of the fingerprint is in the diff of some commit looked
    /// through, and if not, whether that is for want of looking.
    async fn hunks_contained(
        &self,
        fingerprint: &ChangeFingerprint,
        tip: &str,
    ) -> Result<(Knowable<bool>, Option<UnknownWhy>), GitError> {
        let line = self.first_parent_line(tip).await?;
        let position = |revision: &Revision| match revision {
            Revision::Commit(id) => line.iter().position(|commit| commit == id.as_str()),
            Revision::Patch { .. } => None,
        };
        let (Some(base), Some(at)) = (
            position(fingerprint.base()),
            position(fingerprint.baseline_at_approval()),
        ) else {
            return Ok((Knowable::Unknown, Some(self.missing().await?)));
        };
        // The line is newest first: the tip at approval is not older than the base.
        if at > base {
            return Ok((Knowable::Unknown, Some(UnknownWhy::BaseMissing)));
        }

        // After the tip at approval, oldest first; then back from it to the base.
        let after = (0..at.min(self.window)).map(|n| &line[at - 1 - n]);
        let before = (0..(base - at).min(self.backlog)).map(|n| &line[at + n]);
        let cut_off = at > self.window || base - at > self.backlog;
        let order: Vec<&String> = after.chain(before).collect();

        let mut needed: BTreeSet<HunkId> = fingerprint.hunks().iter().copied().collect();
        for chunk in order.chunks(LOG_CHUNK) {
            for found in self.commit_hunks(chunk, fingerprint.scheme()).await? {
                needed.retain(|hunk| !found.contains(hunk));
            }
            if needed.is_empty() {
                return Ok((Knowable::Known(true), None));
            }
        }
        if cut_off {
            Ok((Knowable::Unknown, Some(UnknownWhy::OutsideWindow)))
        } else {
            Ok((Knowable::Known(false), None))
        }
    }
}

/// The hunks of a diff or of a log of diffs, by the commit they belong to (a log marks
/// each commit with a line that starts with the record separator).
fn parse_diff(text: &str) -> Vec<Section> {
    enum State {
        Outside,
        Header,
        Body,
    }

    fn flush(
        section: &mut Section,
        path: &Option<RelPath>,
        removed: &mut Vec<String>,
        added: &mut Vec<String>,
    ) {
        let (removed, added) = (std::mem::take(removed), std::mem::take(added));
        if let Some(path) = path
            && !(removed.is_empty() && added.is_empty())
        {
            section.hunks.push(Hunk::new(path.clone(), removed, added));
        }
    }

    let mut sections = vec![Section {
        commit: None,
        hunks: Vec::new(),
    }];
    let mut state = State::Outside;
    let (mut old, mut new): (Option<String>, Option<String>) = (None, None);
    let mut path: Option<RelPath> = None;
    let (mut removed, mut added): (Vec<String>, Vec<String>) = (Vec::new(), Vec::new());
    for line in text.lines() {
        if let Some(commit) = line.strip_prefix('\u{1e}') {
            if let Some(section) = sections.last_mut() {
                flush(section, &path, &mut removed, &mut added);
            }
            path = None;
            state = State::Outside;
            sections.push(Section {
                commit: Some(commit.trim().to_owned()),
                hunks: Vec::new(),
            });
            continue;
        }
        if line.starts_with("diff --git ") {
            if let Some(section) = sections.last_mut() {
                flush(section, &path, &mut removed, &mut added);
            }
            (old, new, path) = (None, None, None);
            state = State::Header;
            continue;
        }
        match state {
            State::Outside => {}
            State::Header => {
                if let Some(name) = line.strip_prefix("--- ") {
                    old = diff_path(name, "a/");
                } else if let Some(name) = line.strip_prefix("+++ ") {
                    new = diff_path(name, "b/");
                } else if line.starts_with("@@") {
                    path = new
                        .take()
                        .or_else(|| old.take())
                        .and_then(|p| p.parse().ok());
                    state = State::Body;
                }
            }
            State::Body => {
                if line.starts_with("@@") {
                    if let Some(section) = sections.last_mut() {
                        flush(section, &path, &mut removed, &mut added);
                    }
                } else if let Some(text) = line.strip_prefix('+') {
                    added.push(text.to_owned());
                } else if let Some(text) = line.strip_prefix('-') {
                    removed.push(text.to_owned());
                }
            }
        }
    }
    if let Some(section) = sections.last_mut() {
        flush(section, &path, &mut removed, &mut added);
    }
    sections
}

/// The path a `---` or `+++` line names, without its `a/` or `b/`; `None` for
/// `/dev/null`, and for a quoted name this cannot read.
fn diff_path(name: &str, prefix: &str) -> Option<String> {
    let name = name.strip_suffix('\t').unwrap_or(name);
    if name == "/dev/null" {
        return None;
    }
    let name = if name.starts_with('"') {
        unquote(name)?
    } else {
        name.to_owned()
    };
    name.strip_prefix(prefix).map(str::to_owned)
}

/// A name Git quoted in the C way: `"..."` with `\\`, `\"`, `\t`, `\n`, `\r` and
/// three-digit octal escapes.
fn unquote(quoted: &str) -> Option<String> {
    let inner = quoted.strip_prefix('"')?.strip_suffix('"')?;
    let mut bytes: Vec<u8> = Vec::new();
    let mut chars = inner.bytes();
    while let Some(byte) = chars.next() {
        if byte != b'\\' {
            bytes.push(byte);
            continue;
        }
        match chars.next()? {
            b'\\' => bytes.push(b'\\'),
            b'"' => bytes.push(b'"'),
            b't' => bytes.push(b'\t'),
            b'n' => bytes.push(b'\n'),
            b'r' => bytes.push(b'\r'),
            first @ b'0'..=b'3' => {
                let second = chars.next().filter(|b| (b'0'..=b'7').contains(b))?;
                let third = chars.next().filter(|b| (b'0'..=b'7').contains(b))?;
                bytes.push(((first - b'0') << 6) | ((second - b'0') << 3) | (third - b'0'));
            }
            _ => return None,
        }
    }
    String::from_utf8(bytes).ok()
}

impl RevisionFactsPort for GitFacts {
    type Error = GitError;

    async fn baseline_facts(&self, query: &BaselineQuery<'_>) -> Result<BaselineAnswer, GitError> {
        let Some(tip) = self.git.commit_of(self.baseline.as_str()).await? else {
            return Ok(BaselineAnswer::new(
                BaselineFacts::new(Knowable::Unknown, Knowable::Unknown),
                UnknownWhy::BaselineUnresolved,
            ));
        };

        let (ancestor, ancestor_why) = match query.approved() {
            // A patch itself is never integrated, only its base can be; the model asks
            // of the commit, and a patch is none.
            Revision::Patch { .. } => (Knowable::Known(false), None),
            Revision::Commit(id) => {
                let rev = full_hash(id)?;
                let args = ["merge-base", "--is-ancestor", rev, tip.as_str()];
                let finished = self.git.finish(args).await?;
                match finished.code {
                    Some(0) => (Knowable::Known(true), None),
                    Some(1) => (Knowable::Known(false), None),
                    _ if self.git.commit_of(rev).await?.is_none() => {
                        (Knowable::Unknown, Some(self.missing().await?))
                    }
                    _ => return Err(failed(&args, &finished)),
                }
            }
        };

        let (hunks, hunks_why) = match query.fingerprint() {
            None => (Knowable::Unknown, Some(UnknownWhy::NoFingerprint)),
            Some(fingerprint) if !fingerprint.scheme().is_known() => {
                (Knowable::Unknown, Some(UnknownWhy::FingerprintVersion))
            }
            Some(fingerprint) => self.hunks_contained(fingerprint, tip.as_str()).await?,
        };

        Ok(BaselineAnswer::new(
            BaselineFacts::new(ancestor, hunks),
            ancestor_why
                .or(hunks_why)
                .unwrap_or(UnknownWhy::NoFingerprint),
        ))
    }

    async fn change_fingerprint(&self, approved: &Revision) -> Result<Fingerprinting, GitError> {
        let Revision::Commit(id) = approved else {
            // A patch is a change that is not committed: there is no commit of it to
            // cut against the baseline.
            return Ok(Fingerprinting::Unavailable(
                FingerprintUnavailable::NotCommitted,
            ));
        };
        let rev = full_hash(id)?;
        let Some(tip) = self.git.commit_of(self.baseline.as_str()).await? else {
            return Ok(Fingerprinting::Unavailable(
                FingerprintUnavailable::BaselineUnresolved,
            ));
        };
        let Some(approved) = self.git.commit_of(rev).await? else {
            return Ok(Fingerprinting::Unavailable(self.unavailable().await?));
        };
        let line = self.first_parent_line(&tip).await?;
        let Some(nearest) = self.nearest_on_line(&line, &approved).await? else {
            return Ok(Fingerprinting::Unavailable(self.unavailable().await?));
        };
        let base = &line[nearest];
        if *base == approved {
            // The revision is on the baseline: its change has landed by being there.
            return Ok(Fingerprinting::NothingToFingerprint);
        }

        let mut args: Vec<&str> = vec!["-c", "core.quotepath=false", "diff"];
        args.extend(DIFF_FLAGS);
        args.extend([base.as_str(), approved.as_str(), "--"]);
        let stdout = self.git.run(&args).await?;
        let text = String::from_utf8_lossy(&stdout);
        let hunks: Vec<Hunk> = parse_diff(&text)
            .into_iter()
            .flat_map(|section| section.hunks)
            .collect();

        let revision = |sha: &str| {
            RevisionId::from_validated(sha.to_owned())
                .map(Revision::Commit)
                .map_err(GitError::Value)
        };
        match ChangeFingerprint::of_hunks(
            revision(base)?,
            revision(&tip)?,
            &hunks,
            HunkScheme::CURRENT,
        ) {
            Ok(fingerprint) => Ok(Fingerprinting::Of(fingerprint)),
            // A change of no hunk (a merge that brought nothing, a mode change) has
            // nothing to find in the baseline.
            Err(FingerprintError::NoHunks) => Ok(Fingerprinting::NothingToFingerprint),
            // The current scheme is one this build has, so this cannot arise; it is
            // reported as it is and not read as an absence of a fingerprint.
            Err(error @ FingerprintError::UnknownScheme { .. }) => {
                Err(GitError::Fingerprint(error))
            }
        }
    }

    async fn work_tree(&self) -> Result<Knowable<WorkTree>, GitError> {
        if !self.git.inside_work_tree().await? {
            return Ok(Knowable::Unknown);
        }
        Ok(Knowable::Known(if self.git.is_clean().await? {
            WorkTree::Clean
        } else {
            WorkTree::Dirty
        }))
    }

    async fn author(&self, at: &Revision) -> Result<Knowable<Author>, GitError> {
        let Revision::Commit(id) = at else {
            return Ok(Knowable::Unknown);
        };
        let rev = full_hash(id)?;
        let finished = self
            .git
            .finish(["log", "-1", "--no-walk", "--format=%an%x00%ae", rev])
            .await?;
        if finished.code != Some(0) {
            // The commit is not in this clone.
            return Ok(Knowable::Unknown);
        }
        let printed = String::from_utf8_lossy(&finished.stdout);
        let (name, mail) = printed
            .trim_end()
            .split_once('\0')
            .unwrap_or((&printed, ""));
        Ok(match PersonName::new(name) {
            Ok(name) => Knowable::Known(Author::new(name, Email::new(mail).ok())),
            Err(_) => Knowable::Unknown,
        })
    }

    async fn git_place(
        &self,
        root: Visibility,
        path: &RelPath,
    ) -> Result<Knowable<GitPlace>, GitError> {
        let base = match root {
            Visibility::Public => self.git.root().to_path_buf(),
            Visibility::Private => match &self.private_root {
                Some(root) => root.clone(),
                None => return Ok(Knowable::Unknown),
            },
        };
        let (Ok(work), Ok(base)) = (
            tokio::fs::canonicalize(self.git.root()).await,
            tokio::fs::canonicalize(&base).await,
        ) else {
            return Ok(Knowable::Unknown);
        };
        let full = base.join(path.as_str());
        let Ok(inside) = full.strip_prefix(&work) else {
            return Ok(Knowable::Known(GitPlace::Outside));
        };
        let Some(inside) = inside.to_str().map(str::to_owned) else {
            return Ok(Knowable::Unknown);
        };

        let literal = format!(":(literal){inside}");
        let tracked = self
            .git
            .finish(["ls-files", "--error-unmatch", "--", literal.as_str()])
            .await?;
        match tracked.code {
            Some(0) => return Ok(Knowable::Known(GitPlace::Tracked)),
            Some(1) => {}
            // Not a repository, or Git cannot say.
            _ => return Ok(Knowable::Unknown),
        }
        let ignored = self
            .git
            .finish(["check-ignore", "-q", "--", inside.as_str()])
            .await?;
        Ok(match ignored.code {
            Some(0) => Knowable::Known(GitPlace::Ignored),
            Some(1) => Knowable::Known(GitPlace::Untracked),
            _ => Knowable::Unknown,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_log_is_cut_into_the_hunks_of_each_commit() {
        let log = "\u{1e}aaa\n\
            \n\
            diff --git a/src/lib.rs b/src/lib.rs\n\
            index 1..2 100644\n\
            --- a/src/lib.rs\n\
            +++ b/src/lib.rs\n\
            @@ -3 +3,2 @@ fn x()\n\
            -let a = 1;\n\
            +let a = 2;\n\
            +let b = 3;\n\
            @@ -9,0 +10 @@\n\
            +let c = 4;\n\
            \\ No newline at end of file\n\
            diff --git a/new.txt b/new.txt\n\
            new file mode 100644\n\
            --- /dev/null\n\
            +++ b/new.txt\n\
            @@ -0,0 +1 @@\n\
            +hello\n\
            diff --git a/gone.txt b/gone.txt\n\
            deleted file mode 100644\n\
            --- a/gone.txt\n\
            +++ /dev/null\n\
            @@ -1 +0,0 @@\n\
            -bye\n\
            \u{1e}bbb\n\
            \n\
            diff --git a/img.png b/img.png\n\
            Binary files a/img.png and b/img.png differ\n\
            \u{1e}ccc\n";
        let sections = parse_diff(log);
        let commits: Vec<_> = sections.iter().map(|s| s.commit.as_deref()).collect();
        assert_eq!(commits, [None, Some("aaa"), Some("bbb"), Some("ccc")]);
        assert_eq!(sections[1].hunks.len(), 4, "{:?}", sections[1].hunks);
        assert!(sections[2].hunks.is_empty(), "a binary file has no hunk");
        assert!(
            sections[3].hunks.is_empty(),
            "a commit with no diff has none"
        );

        let lib = |removed: &[&str], added: &[&str]| {
            Hunk::new(
                "src/lib.rs".parse().unwrap(),
                removed.iter().map(|s| (*s).to_owned()).collect(),
                added.iter().map(|s| (*s).to_owned()).collect(),
            )
        };
        assert_eq!(
            sections[1].hunks[0],
            lib(&["let a = 1;"], &["let a = 2;", "let b = 3;"])
        );
        assert_eq!(sections[1].hunks[1], lib(&[], &["let c = 4;"]));
        assert_eq!(
            sections[1].hunks[2],
            Hunk::new("new.txt".parse().unwrap(), vec![], vec!["hello".to_owned()])
        );
        assert_eq!(
            sections[1].hunks[3],
            Hunk::new("gone.txt".parse().unwrap(), vec!["bye".to_owned()], vec![])
        );
    }

    #[test]
    fn a_removed_line_that_looks_like_a_header_is_content() {
        let diff = "diff --git a/a.txt b/a.txt\n\
            --- a/a.txt\n\
            +++ b/a.txt\n\
            @@ -1,2 +1,2 @@\n\
            --- a/a.txt\n\
            -- not a header\n\
            +++ b/a.txt\n\
            ++ not a header\n";
        let sections = parse_diff(diff);
        let [hunk] = sections[0].hunks.as_slice() else {
            panic!("one hunk, got {:?}", sections[0].hunks);
        };
        let expected = Hunk::new(
            "a.txt".parse().unwrap(),
            vec!["-- a/a.txt".to_owned(), "- not a header".to_owned()],
            vec!["++ b/a.txt".to_owned(), "+ not a header".to_owned()],
        );
        assert_eq!(hunk, &expected);
    }

    #[test]
    fn a_quoted_name_is_read_and_an_odd_one_is_left_out() {
        assert_eq!(unquote(r#""a\tb""#).as_deref(), Some("a\tb"));
        assert_eq!(unquote(r#""caf\303\251""#).as_deref(), Some("café"));
        assert_eq!(unquote(r#""q\"q\\""#).as_deref(), Some("q\"q\\"));
        assert_eq!(unquote(r#""bad\x""#), None);
        assert_eq!(unquote(r#""bad\30""#), None);
        assert_eq!(
            diff_path("a/my file.txt\t", "a/").as_deref(),
            Some("my file.txt")
        );
        assert_eq!(diff_path("/dev/null", "a/"), None);
        assert_eq!(diff_path("b/x", "a/"), None, "the prefix is checked");
    }

    #[test]
    fn only_a_full_hash_is_a_revision() {
        assert!(is_full_hash(&"a1".repeat(20)));
        assert!(is_full_hash(&"A1".repeat(20)));
        assert!(!is_full_hash(&"a1".repeat(19)));
        assert!(!is_full_hash(&"g1".repeat(20)));
        assert!(!is_full_hash("main"));
        assert!(!is_full_hash("--upload-pack=x"));
    }

    #[test]
    fn a_grep_record_is_a_path_a_line_number_and_a_line() {
        let record = b"abc:tests/a.rs\x0012\x00    // @pin: PIN-A";
        let (path, number, line) = grep_record(record, "abc:").unwrap();
        assert_eq!(
            (path.as_str(), number, &*line),
            ("tests/a.rs", 12, "    // @pin: PIN-A")
        );
        // A line may hold anything after the second separator, a NUL included.
        let (_, _, line) = grep_record(b"abc:a.rs\x001\x00x\x00y", "abc:").unwrap();
        assert_eq!(&*line, "x\0y");
        for bad in [
            &b"abc:a.rs"[..],
            b"abc:a.rs\x00x\x00l",
            b"other:a.rs\x001\x00l",
            b"",
        ] {
            assert!(grep_record(bad, "abc:").is_none());
        }
    }

    #[test]
    fn a_file_counts_only_under_a_scan_directory() {
        let dirs: Vec<RelPath> = ["src", "tests/unit"]
            .iter()
            .map(|d| d.parse().unwrap())
            .collect();
        let path = |text: &str| text.parse::<RelPath>().unwrap();
        assert!(under(&path("src/a.rs"), &dirs));
        assert!(under(&path("tests/unit/deep/b.rs"), &dirs));
        assert!(
            !under(&path("srcs/a.rs"), &dirs),
            "a prefix is not a directory"
        );
        assert!(!under(&path("tests/other.rs"), &dirs));
    }

    #[test]
    fn a_glob_names_files_and_a_directory_name_names_what_is_under_it() {
        let files =
            TestFileSet::new(vec!["tests/".to_owned(), "src/**/*_test.go".to_owned()]).unwrap();
        assert_eq!(
            pathspecs(&files),
            [
                ":(glob)tests",
                ":(glob)tests/**",
                ":(glob)src/**/*_test.go",
                ":(glob)src/**/*_test.go/**"
            ]
        );
    }

    #[tokio::test]
    async fn validate_accepts_a_full_hash_and_lowercases_it() {
        let git = GitRevision::new(PathBuf::from("."), "main".to_owned());
        let upper = "A1B2C3D4E5".repeat(4);
        let got = git
            .validate(&RawRevision::new(format!(" {upper} ")).unwrap())
            .await
            .unwrap();
        assert_eq!(got.unwrap().as_str(), upper.to_ascii_lowercase());
        assert!(
            git.validate(&RawRevision::new("v1.2").unwrap())
                .await
                .unwrap()
                .is_none()
        );
    }
}
