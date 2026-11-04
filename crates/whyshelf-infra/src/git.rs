//! Git: [`RevisionPort`] and [`CodeScanPort`] over the `git` command.
//!
//! The model speaks only of a revision, of being integrated and of a clean tree;
//! Git's words stay in this module. Every call starts `git` with `tokio::process`
//! in the project root and reads its output; none of them changes the repository.
//!
//! | question | git |
//! |---|---|
//! | is this text a revision (`validate`) | 40 hexadecimal digits, checked without starting a process |
//! | the revision of the working tree | `rev-parse HEAD` |
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

use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::process::Stdio;

use regex::Regex;
use tokio::process::Command;
use whyshelf_core::app::config::PinPattern;
use whyshelf_core::app::ports::{CodeScanPort, PinScan, RevisionPort, WorktreeRoot};
use whyshelf_core::app::raw::RawRevision;
use whyshelf_core::domain::error::ValueError;
use whyshelf_core::domain::location::{CodeLocation, RelPath, TestFileSet};
use whyshelf_core::domain::nonempty::NonBlank;
use whyshelf_core::domain::pin::{MalformedPin, PinId};
use whyshelf_core::domain::revision::{IntegrationFact, RevisionId, UnknownIntegration};

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

    async fn current(&self) -> Result<RevisionId, GitError> {
        RevisionId::from_validated(self.git.head().await?).map_err(GitError::Value)
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

impl CodeScanPort for GitCodeScan {
    type Error = GitError;

    async fn pins_at(
        &self,
        revision: &RevisionId,
        scan_dirs: &[RelPath],
        files: &TestFileSet,
        pattern: &PinPattern,
    ) -> Result<PinScan, GitError> {
        let rev = full_hash(revision)?;
        let re = Regex::new(pattern.as_str()).map_err(GitError::Pattern)?;
        if re.captures_len() < 2 {
            return Err(GitError::PatternGroup);
        }
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
                match named.as_str().parse::<PinId>() {
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

#[cfg(test)]
mod tests {
    use super::*;

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
