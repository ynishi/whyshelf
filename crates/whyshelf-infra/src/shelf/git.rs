//! The project's version control, Git: the revision the work is at, and the names
//! and contents of blobs, for the adapters that keep content by address.
//!
//! | what | with |
//! |---|---|
//! | [`RevisionSource`]: the commit `HEAD` names | `git rev-parse --verify HEAD` |
//! | [`Git::blob_names`]: the blob each file's content is (or would be) in | `git hash-object -- <paths>` (no `-w`: only the name, nothing is written) |
//! | [`Git::read_blob`]: the content of a blob, by its name | `git cat-file blob <name>` |
//!
//! The revision is the project's, not a space's or the documents': it is the one
//! [`RevisionSource`] of a project. The blob names and contents are used by the
//! adapter of the documents ([`docs`](super::docs)) and by the adapter of the space
//! `file` ([`artifact::file`](super::artifact::file)), each for its own content.
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

use std::path::PathBuf;
use std::process::Stdio;

use tokio::process::Command;
use whyshelf_core::app::shelf::RevisionSource;
use whyshelf_core::domain::location::RelPath;
use whyshelf_core::domain::shelf::artifact::{ContentAddress, OpaqueProblem, Revision};

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
}

/// The Git repository a project root is in, if it is in one.
#[derive(Debug, Clone)]
pub struct Git {
    root: PathBuf,
}

impl Git {
    /// The repository of the project at `root`.
    pub fn new(root: PathBuf) -> Self {
        Self { root }
    }

    /// A Git command run at the root, without a prompt and without the optional
    /// locks a read does not need. `None` when `git` cannot be started.
    async fn run(&self, args: &[&str]) -> Option<std::process::Output> {
        Command::new("git")
            .arg("-C")
            .arg(&self.root)
            .args(args)
            .env("GIT_OPTIONAL_LOCKS", "0")
            .env("GIT_TERMINAL_PROMPT", "0")
            .stdin(Stdio::null())
            .output()
            .await
            .ok()
    }

    /// Whether the root is in a Git work tree that `git` can be run in.
    async fn in_repository(&self) -> bool {
        match self.run(&["rev-parse", "--is-inside-work-tree"]).await {
            Some(out) => out.status.success() && out.stdout.trim_ascii() == b"true",
            None => false,
        }
    }

    /// The blob names of these files (relative to the root, each there), in order,
    /// or none outside a repository.
    pub async fn blob_names(
        &self,
        paths: &[&RelPath],
    ) -> Result<Option<Vec<ContentAddress>>, GitError> {
        if paths.is_empty() || !self.in_repository().await {
            return Ok(None);
        }
        let mut args = vec!["hash-object", "--"];
        args.extend(paths.iter().map(|p| p.as_str()));
        let Some(out) = self.run(&args).await else {
            return Ok(None);
        };
        if !out.status.success() {
            return Err(GitError::HashObject {
                stderr: String::from_utf8_lossy(&out.stderr).trim().to_owned(),
            });
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

    /// The content of the blob `address` names, or none when it is no name of Git's
    /// or Git holds no such blob.
    pub async fn read_blob(&self, address: &ContentAddress) -> Option<Vec<u8>> {
        // A name that is not Git's is nothing Git can read, and is never passed on.
        if !is_object_name(address.as_str()) {
            return None;
        }
        match self.run(&["cat-file", "blob", address.as_str()]).await {
            Some(out) if out.status.success() => Some(out.stdout),
            Some(_) | None => None,
        }
    }
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
            .await
        else {
            return Ok(None);
        };
        if !out.status.success() {
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

    #[tokio::test]
    async fn outside_a_repository_there_is_no_revision_and_no_blob() {
        let dir = tempfile::tempdir().unwrap();
        let git = Git::new(dir.path().to_path_buf());
        assert_eq!(git.revision().await.unwrap(), None);
        let file: RelPath = "a".parse().unwrap();
        assert_eq!(git.blob_names(&[&file]).await.unwrap(), None);
        for text in ["--output=/tmp/x", "abc", &"g".repeat(40)] {
            let address = ContentAddress::from_validated(text.to_owned()).unwrap();
            assert_eq!(git.read_blob(&address).await, None);
        }
    }
}
