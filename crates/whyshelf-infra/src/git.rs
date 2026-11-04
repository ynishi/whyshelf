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
//! | has a revision reached the baseline | `merge-base --is-ancestor <rev> <baseline>` |
//! | the name of the line of work | `rev-parse --abbrev-ref HEAD` (none when detached) |
//! | the pins in the tests at a revision | `ls-tree -r` and `show <rev>:<path>` |
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

use std::collections::BTreeMap;
use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::process::Stdio;

use regex::Regex;
use tokio::process::Command;
use whyshelf_core::app::config::PinPattern;
use whyshelf_core::app::ports::{CodeScanPort, RevisionPort, WorktreeRoot};
use whyshelf_core::app::raw::RawRevision;
use whyshelf_core::domain::error::ValueError;
use whyshelf_core::domain::location::{CodeLocation, RelPath};
use whyshelf_core::domain::pin::PinId;
use whyshelf_core::domain::revision::RevisionId;

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

    async fn is_integrated(&self, revision: &RevisionId) -> Result<bool, GitError> {
        let args = [
            "merge-base",
            "--is-ancestor",
            full_hash(revision)?,
            self.baseline.as_str(),
        ];
        let finished = self.git.finish(args).await?;
        match finished.code {
            Some(0) => Ok(true),
            Some(1) => Ok(false),
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

/// The paths of the files (not submodules or links) that `ls-tree -z -r` listed.
fn blob_paths(listing: &[u8]) -> Vec<String> {
    listing
        .split(|byte| *byte == 0)
        .filter_map(|entry| {
            let entry = std::str::from_utf8(entry).ok()?;
            let (meta, path) = entry.split_once('\t')?;
            let mut fields = meta.split(' ');
            let (_mode, kind) = (fields.next()?, fields.next()?);
            (kind == "blob").then(|| path.to_owned())
        })
        .collect()
}

impl CodeScanPort for GitCodeScan {
    type Error = GitError;

    async fn pins_at(
        &self,
        revision: &RevisionId,
        scan_dirs: &[RelPath],
        pattern: &PinPattern,
    ) -> Result<BTreeMap<PinId, Vec<CodeLocation>>, GitError> {
        let rev = full_hash(revision)?;
        let re = Regex::new(pattern.as_str()).map_err(GitError::Pattern)?;
        if re.captures_len() < 2 {
            return Err(GitError::PatternGroup);
        }
        let mut found: BTreeMap<PinId, Vec<CodeLocation>> = BTreeMap::new();
        if scan_dirs.is_empty() {
            return Ok(found);
        }
        let mut args: Vec<&str> = vec!["ls-tree", "-r", "-z", rev, "--"];
        args.extend(scan_dirs.iter().map(RelPath::as_str));
        let listing = self.git.run(args).await?;
        for path in blob_paths(&listing) {
            let Ok(rel) = path.parse::<RelPath>() else {
                continue;
            };
            let object = format!("{rev}:./{path}");
            let content = self.git.run(["show", object.as_str()]).await?;
            let Ok(text) = String::from_utf8(content) else {
                continue;
            };
            if text.contains('\0') {
                continue;
            }
            for (index, line) in text.lines().enumerate() {
                for captures in re.captures_iter(line) {
                    let Some(pin) = captures
                        .get(1)
                        .and_then(|m| m.as_str().parse::<PinId>().ok())
                    else {
                        continue;
                    };
                    let line_number = u32::try_from(index + 1).unwrap_or(u32::MAX);
                    let location =
                        CodeLocation::new(rel.clone(), line_number).map_err(GitError::Value)?;
                    found.entry(pin).or_default().push(location);
                }
            }
        }
        Ok(found)
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
    fn the_listing_keeps_files_and_drops_submodules() {
        let listing = b"100644 blob aaaa\tsrc/a.rs\x00160000 commit bbbb\tvendor/x\x00100755 blob cccc\ttests/b.sh\x00";
        assert_eq!(blob_paths(listing), ["src/a.rs", "tests/b.sh"]);
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
