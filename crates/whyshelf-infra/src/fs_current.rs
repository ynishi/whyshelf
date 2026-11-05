//! The document files: [`CurrentPort`] over the working tree.
//!
//! Read only. A listing gives each Markdown file its path, a modification stamp
//! and its size, which is all ingest needs to see what changed without reading
//! a file. A read normalizes the text ([`crate::markdown`]) and parses it
//! ([`crate::parse`]) into a [`Candidate`]: a file that does not parse is not an
//! error here but a candidate with the reasons, because the application records
//! it as a version of its own and shows the entity as invalid.

use std::io;
use std::path::PathBuf;

use whyshelf_core::app::ports::{CurrentPort, Listing};
use whyshelf_core::app::raw::Candidate;
use whyshelf_core::domain::event::DocKind;
use whyshelf_core::domain::location::RelPath;
use whyshelf_core::domain::nonempty::NonEmptyVec;
use whyshelf_core::domain::state::UnparsedReason;

use crate::blocking::{self, BlockingError};
use crate::{markdown, parse};

/// Why the files could not be listed or read.
#[derive(Debug, thiserror::Error)]
pub enum CurrentError {
    /// The directory tree could not be listed.
    #[error("cannot list the document files")]
    List(#[source] BlockingError),
    /// A file exists but could not be read.
    #[error("cannot read the file {path}")]
    Read {
        /// The file.
        path: RelPath,
        /// The operating system's error.
        #[source]
        source: io::Error,
    },
}

/// The working tree as the source of documents.
#[derive(Debug, Clone)]
pub struct FsCurrent {
    root: PathBuf,
}

impl FsCurrent {
    /// The documents under this project root.
    pub fn new(root: PathBuf) -> Self {
        Self { root }
    }
}

impl CurrentPort for FsCurrent {
    type Error = CurrentError;

    async fn list(&self, roots: &[RelPath]) -> Result<Listing, CurrentError> {
        blocking::list_markdown(self.root.clone(), roots.to_vec())
            .await
            .map_err(CurrentError::List)
    }

    async fn read(
        &self,
        path: &RelPath,
        kind: Option<DocKind>,
    ) -> Result<Option<Candidate>, CurrentError> {
        let bytes = match tokio::fs::read(self.root.join(path.as_str())).await {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(source) => {
                return Err(CurrentError::Read {
                    path: path.clone(),
                    source,
                });
            }
        };
        let (text, valid) = match String::from_utf8(bytes) {
            Ok(text) => (text, true),
            Err(error) => (
                String::from_utf8_lossy(error.as_bytes()).into_owned(),
                false,
            ),
        };
        let normalized = markdown::normalize(&text);
        if !valid {
            let reason = UnparsedReason::MalformedFrontmatter {
                detail: "the file is not valid UTF-8".to_owned(),
            };
            return Ok(Some(Candidate::unparsed(
                path.clone(),
                normalized,
                NonEmptyVec::new(reason, Vec::new()),
            )));
        }
        let read = parse::read(&normalized, kind);
        let candidate = match read.outcome {
            Ok(doc) => Candidate::parsed(path.clone(), normalized, doc),
            Err(reasons) => Candidate::unparsed(path.clone(), normalized, reasons),
        };
        Ok(Some(
            candidate
                .with_guess(read.guess)
                .with_classification(read.classification),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use whyshelf_core::app::raw::ParseOutcome;
    use whyshelf_core::domain::event::DocKind;

    fn rel(text: &str) -> RelPath {
        text.parse().unwrap()
    }

    const FINDING: &str = "---\r\nfinding_type: usage_observation\r\ntitle: \"Slow login\"  \r\n\
                           evidence: measured\r\ncontext: \"cold start\"\r\nreproducibility: reproducible\r\n---\r\n\r\n\
                           Login takes 8 s.   \r\n\r\n\r\n";

    async fn project_with(files: &[(&str, &str)]) -> (tempfile::TempDir, FsCurrent) {
        let dir = tempfile::tempdir().unwrap();
        for (name, text) in files {
            let path = dir.path().join(name);
            tokio::fs::create_dir_all(path.parent().unwrap())
                .await
                .unwrap();
            tokio::fs::write(path, text).await.unwrap();
        }
        let current = FsCurrent::new(dir.path().to_path_buf());
        (dir, current)
    }

    #[tokio::test]
    async fn a_finding_is_read_normalized_and_parsed() {
        let (_dir, current) = project_with(&[("docs/finding/slow-login.md", FINDING)]).await;
        let candidate = current
            .read(&rel("docs/finding/slow-login.md"), None)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(candidate.kind(), Some(DocKind::Finding));
        assert!(candidate.text().as_str().ends_with("Login takes 8 s.\n"));
        assert!(!candidate.text().as_str().contains('\r'));
        // The raw hash is the hash of the normalized text.
        assert_eq!(candidate.raw_hash(), candidate.text().raw_hash());
    }

    #[tokio::test]
    async fn a_file_that_does_not_parse_is_a_candidate_with_reasons() {
        let (_dir, current) = project_with(&[("docs/a.md", "just a note\n")]).await;
        let candidate = current
            .read(&rel("docs/a.md"), None)
            .await
            .unwrap()
            .unwrap();
        assert!(matches!(candidate.outcome(), ParseOutcome::Unparsed(_)));
        assert_eq!(candidate.text().as_str(), "just a note\n");
    }

    #[tokio::test]
    async fn a_conflicted_file_is_unparsed_with_the_marker_as_the_reason() {
        let text = "---\nfinding_type: usage_observation\n<<<<<<< HEAD\ntitle: a\n=======\ntitle: b\n>>>>>>> topic\n---\nx\n";
        let (_dir, current) = project_with(&[("docs/a.md", text)]).await;
        let candidate = current
            .read(&rel("docs/a.md"), None)
            .await
            .unwrap()
            .unwrap();
        let ParseOutcome::Unparsed(reasons) = candidate.outcome() else {
            panic!("parsed");
        };
        assert_eq!(reasons.first(), &UnparsedReason::MergeConflictMarker);
    }

    #[tokio::test]
    async fn bytes_that_are_not_utf8_are_unparsed() {
        let dir = tempfile::tempdir().unwrap();
        tokio::fs::write(dir.path().join("a.md"), [0x66u8, 0xff, 0x6f])
            .await
            .unwrap();
        let current = FsCurrent::new(dir.path().to_path_buf());
        let candidate = current.read(&rel("a.md"), None).await.unwrap().unwrap();
        assert!(matches!(candidate.outcome(), ParseOutcome::Unparsed(_)));
    }

    #[tokio::test]
    async fn a_missing_file_reads_as_none_and_lists_as_nothing() {
        let (_dir, current) = project_with(&[]).await;
        assert!(
            current
                .read(&rel("docs/none.md"), None)
                .await
                .unwrap()
                .is_none()
        );
        let listed = current.list(&[rel("docs")]).await.unwrap();
        assert!(listed.files().is_empty() && listed.unreadable().is_empty());
    }

    #[tokio::test]
    async fn listing_gives_a_stamp_that_changes_with_the_file() {
        let (dir, current) = project_with(&[("docs/a.md", "one\n")]).await;
        let before = current.list(&[rel("docs")]).await.unwrap();
        assert_eq!(before.files().len(), 1);
        tokio::fs::write(dir.path().join("docs/a.md"), "one and more\n")
            .await
            .unwrap();
        let after = current.list(&[rel("docs")]).await.unwrap();
        assert!(!before.files()[0].same_as(&after.files()[0]));
    }
}
