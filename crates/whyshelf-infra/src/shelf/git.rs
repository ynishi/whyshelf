//! The files of a project that are no document, and where their content can be read
//! back: the working tree, and Git.
//!
//! | port | what it does | with |
//! |---|---|---|
//! | [`ArtifactSource`] | reads each file's bytes from the working tree and hashes them; names the blob its content is (or would be) in Git | `tokio::fs`, `git hash-object -- <paths>` (no `-w`: only the name, nothing is written) |
//! | [`RevisionSource`] | the commit `HEAD` names | `git rev-parse --verify HEAD` |
//! | [`ContentStore`] | the content of a blob, by its name | `git cat-file blob <name>` |
//!
//! ```text
//!   path ──tokio::fs::read──► bytes, kept when no part of the path names an entry in another case only
//!                               ──ArtifactReading::of_bytes──► a hash per comparison asked (always)
//!                               └──git hash-object──► blob name ──► ContentAddress (in a repository)
//!   HEAD ──git rev-parse──► commit ──► Revision                                   (in a repository)
//!   ContentAddress ──git cat-file blob──► bytes, or not readable
//! ```
//!
//! # Outside Git
//!
//! The hash of a file never needs Git, so freshness works the same in any directory.
//! Outside a repository, or with no `git` to run, there is no address and no
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
//! [`ArtifactSource`]: whyshelf_core::app::shelf::ArtifactSource
//! [`RevisionSource`]: whyshelf_core::app::shelf::RevisionSource
//! [`ContentStore`]: whyshelf_core::app::shelf::ContentStore

use std::io;
use std::path::PathBuf;
use std::process::Stdio;

use tokio::process::Command;
use whyshelf_core::app::shelf::{ArtifactSource, ContentStore, RevisionSource};
use whyshelf_core::domain::location::RelPath;
use whyshelf_core::domain::shelf::artifact::{
    ArtifactAsks, ArtifactReading, ArtifactReadings, ContentAddress, OpaqueProblem, Revision,
};

/// Why the files or Git could not be read.
#[derive(Debug, thiserror::Error)]
pub enum ArtifactError {
    /// A file is there and could not be read.
    #[error("cannot read {}", .path.display())]
    Io {
        /// The file.
        path: PathBuf,
        /// The operating system's error.
        #[source]
        source: io::Error,
    },
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

/// The working tree of a project, and its Git repository when it is in one.
#[derive(Debug, Clone)]
pub struct GitArtifacts {
    root: PathBuf,
}

impl GitArtifacts {
    /// The files under `root`.
    pub fn new(root: PathBuf) -> Self {
        Self { root }
    }

    /// A Git command run at the root, without a prompt and without the optional
    /// locks a read does not need. `None` when `git` cannot be started.
    async fn git(&self, args: &[&str]) -> Option<std::process::Output> {
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
        match self.git(&["rev-parse", "--is-inside-work-tree"]).await {
            Some(out) => out.status.success() && out.stdout.trim_ascii() == b"true",
            None => false,
        }
    }

    /// The bytes of a file, or none when no file is there (nothing, or a directory).
    async fn bytes(&self, path: &RelPath) -> Result<Option<Vec<u8>>, ArtifactError> {
        let full = self.root.join(path.as_str());
        let io_error = |source| ArtifactError::Io {
            path: full.clone(),
            source,
        };
        let bytes = match tokio::fs::read(&full).await {
            Ok(bytes) => bytes,
            Err(e) if is_absent(&e) => return Ok(None),
            Err(e) => return Err(io_error(e)),
        };
        if self.named_as_written(path).await {
            Ok(Some(bytes))
        } else {
            Ok(None)
        }
    }

    /// Whether a path that opened names the file as written (the rule of
    /// [`ArtifactSource::read_artifacts`]): no part of it is the name of an entry of
    /// its directory in another ASCII case only. A directory that cannot be listed
    /// says nothing against the path, which opened.
    async fn named_as_written(&self, path: &RelPath) -> bool {
        let mut dir = self.root.clone();
        for part in path.as_str().split('/') {
            let Ok(mut entries) = tokio::fs::read_dir(&dir).await else {
                return true;
            };
            let mut names = Vec::new();
            while let Ok(Some(entry)) = entries.next_entry().await {
                names.push(entry.file_name());
            }
            if part_names_entry(part, names.iter().map(|n| n.as_os_str())) == PartName::OtherCase {
                return false;
            }
            dir.push(part);
        }
        true
    }

    /// The blob names of these files, in order, or none outside a repository.
    async fn blob_names(
        &self,
        paths: &[&RelPath],
    ) -> Result<Option<Vec<ContentAddress>>, ArtifactError> {
        if paths.is_empty() || !self.in_repository().await {
            return Ok(None);
        }
        let mut args = vec!["hash-object", "--"];
        args.extend(paths.iter().map(|p| p.as_str()));
        let Some(out) = self.git(&args).await else {
            return Ok(None);
        };
        if !out.status.success() {
            return Err(ArtifactError::HashObject {
                stderr: String::from_utf8_lossy(&out.stderr).trim().to_owned(),
            });
        }
        let text = String::from_utf8_lossy(&out.stdout);
        let names = text
            .lines()
            .map(|line| {
                let name = object_name(line)?;
                ContentAddress::from_validated(name.clone())
                    .map_err(|source| ArtifactError::Refused { text: name, source })
            })
            .collect::<Result<Vec<_>, _>>()?;
        if names.len() != paths.len() {
            return Err(ArtifactError::BadName {
                text: text.into_owned(),
            });
        }
        Ok(Some(names))
    }
}

/// What the entries of a directory say about one part of a path that opened.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PartName {
    /// An entry has the name as written.
    Written,
    /// No entry has it, and one has it in another ASCII case only: a file system that
    /// does not tell case apart opened it, and Git and one that does would not.
    OtherCase,
    /// Neither: a name the system stores in another form (macOS keeps some
    /// non-ASCII names decomposed), which it opened and Git names as written.
    Unknown,
}

/// The one place a part of a path is held against the names of its directory.
fn part_names_entry<'a>(part: &str, names: impl Iterator<Item = &'a std::ffi::OsStr>) -> PartName {
    let mut other_case = false;
    for name in names {
        if name == part {
            return PartName::Written;
        }
        if name
            .to_str()
            .is_some_and(|name| name.eq_ignore_ascii_case(part))
        {
            other_case = true;
        }
    }
    if other_case {
        PartName::OtherCase
    } else {
        PartName::Unknown
    }
}

/// Whether a read failed because no file is at the path: nothing there, a directory
/// on the way that is a file, or a directory where the file would be.
fn is_absent(error: &io::Error) -> bool {
    matches!(
        error.kind(),
        io::ErrorKind::NotFound | io::ErrorKind::NotADirectory | io::ErrorKind::IsADirectory
    )
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
fn object_name(line: &str) -> Result<String, ArtifactError> {
    let text = line.trim();
    if is_object_name(text) {
        Ok(text.to_owned())
    } else {
        Err(ArtifactError::BadName {
            text: text.to_owned(),
        })
    }
}

impl ArtifactSource for GitArtifacts {
    type Error = ArtifactError;

    async fn read_artifacts(&self, asks: &ArtifactAsks) -> Result<ArtifactReadings, ArtifactError> {
        let mut readings = ArtifactReadings::new();
        let mut present = Vec::new();
        for (path, comparisons) in asks {
            match self.bytes(path).await? {
                Some(bytes) => {
                    present.push(path);
                    readings.insert(
                        path.clone(),
                        ArtifactReading::of_bytes(&bytes, comparisons, None),
                    );
                }
                None => {
                    readings.insert(path.clone(), ArtifactReading::absent());
                }
            }
        }
        if let Some(names) = self.blob_names(&present).await? {
            for (path, name) in present.into_iter().zip(names) {
                if let Some(reading) = readings.get_mut(path) {
                    reading.set_address(name);
                }
            }
        }
        Ok(readings)
    }

    async fn current(&self, path: &RelPath) -> Result<Option<Vec<u8>>, ArtifactError> {
        self.bytes(path).await
    }
}

impl ContentStore for GitArtifacts {
    type Error = ArtifactError;

    async fn read_back(&self, address: &ContentAddress) -> Result<Option<Vec<u8>>, ArtifactError> {
        // A name that is not Git's is nothing Git can read, and is never passed on.
        if !is_object_name(address.as_str()) {
            return Ok(None);
        }
        match self.git(&["cat-file", "blob", address.as_str()]).await {
            Some(out) if out.status.success() => Ok(Some(out.stdout)),
            Some(_) | None => Ok(None),
        }
    }
}

impl RevisionSource for GitArtifacts {
    type Error = ArtifactError;

    async fn revision(&self) -> Result<Option<Revision>, ArtifactError> {
        let Some(out) = self
            .git(&["rev-parse", "--verify", "--quiet", "HEAD"])
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
            .map_err(|source| ArtifactError::Refused { text, source })
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use whyshelf_core::domain::shelf::artifact::{ArtifactState, Comparison, LinePrefix};

    use super::*;

    /// Whether `git` can be run here; the tests that need it pass without it.
    async fn git_installed() -> bool {
        Command::new("git")
            .arg("--version")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .await
            .is_ok_and(|s| s.success())
    }

    async fn run(dir: &std::path::Path, args: &[&str]) -> String {
        let out = Command::new("git")
            .arg("-C")
            .arg(dir)
            .args(["-c", "user.name=t", "-c", "user.email=t@example.com"])
            .args(args)
            .output()
            .await
            .unwrap();
        assert!(out.status.success(), "git {args:?}: {out:?}");
        String::from_utf8(out.stdout).unwrap().trim().to_owned()
    }

    fn p(text: &str) -> RelPath {
        text.parse().unwrap()
    }

    /// Each path, asked for its bytes only.
    fn bytes_of(paths: &[&str]) -> ArtifactAsks {
        paths
            .iter()
            .map(|t| (p(t), BTreeSet::from([Comparison::Bytes])))
            .collect()
    }

    fn lines(ignore: &[&str]) -> Comparison {
        Comparison::Lines {
            ignore: ignore.iter().map(|i| LinePrefix::new(i).unwrap()).collect(),
        }
    }

    async fn write(dir: &std::path::Path, at: &str, text: &str) {
        let full = dir.join(at);
        if let Some(parent) = full.parent() {
            tokio::fs::create_dir_all(parent).await.unwrap();
        }
        tokio::fs::write(full, text).await.unwrap();
    }

    #[tokio::test]
    async fn outside_a_repository_a_file_has_a_hash_and_no_address() {
        let dir = tempfile::tempdir().unwrap();
        write(dir.path(), "tests/a.rs", "one\n").await;
        let source = GitArtifacts::new(dir.path().to_path_buf());
        let found = source
            .read_artifacts(&bytes_of(&["tests/a.rs", "tests/none.rs"]))
            .await
            .unwrap();
        assert_eq!(
            found.get(&p("tests/a.rs")),
            Some(&ArtifactReading::of_bytes(b"one\n", &BTreeSet::new(), None))
        );
        assert_eq!(found[&p("tests/a.rs")].address(), None);
        assert_eq!(
            found.get(&p("tests/none.rs")),
            Some(&ArtifactReading::absent())
        );
        assert_eq!(source.revision().await.unwrap(), None);
        assert_eq!(
            source.current(&p("tests/a.rs")).await.unwrap().as_deref(),
            Some(&b"one\n"[..])
        );
        assert_eq!(source.current(&p("tests/none.rs")).await.unwrap(), None);
    }

    #[test]
    fn a_part_is_held_against_the_names_of_its_directory() {
        use std::ffi::OsStr;
        let names = |all: &'static [&'static str]| all.iter().map(OsStr::new);
        assert_eq!(
            part_names_entry("Justfile", names(&["a", "Justfile"])),
            PartName::Written
        );
        assert_eq!(
            part_names_entry("justfile", names(&["a", "Justfile"])),
            PartName::OtherCase
        );
        // Both spellings there: the one written is a file of its own.
        assert_eq!(
            part_names_entry("justfile", names(&["Justfile", "justfile"])),
            PartName::Written
        );
        // A name stored decomposed (e + U+0301) against the composed one written.
        assert_eq!(
            part_names_entry("caf\u{e9}", names(&["cafe\u{301}"])),
            PartName::Unknown
        );
        assert_eq!(part_names_entry("x", names(&[])), PartName::Unknown);
    }

    #[tokio::test]
    async fn a_path_that_differs_from_the_name_only_in_case_is_no_file() {
        let dir = tempfile::tempdir().unwrap();
        write(dir.path(), "Justfile", "check:\n").await;
        write(dir.path(), "Tests/a.rs", "one\n").await;
        let source = GitArtifacts::new(dir.path().to_path_buf());
        let found = source
            .read_artifacts(&bytes_of(&[
                "justfile",
                "tests/a.rs",
                "Tests/A.rs",
                "Justfile",
            ]))
            .await
            .unwrap();
        for wrong in ["justfile", "tests/a.rs", "Tests/A.rs"] {
            assert_eq!(
                found.get(&p(wrong)),
                Some(&ArtifactReading::absent()),
                "{wrong}"
            );
        }
        assert!(
            found
                .get(&p("Justfile"))
                .is_some_and(ArtifactReading::is_present)
        );
        assert_eq!(source.current(&p("justfile")).await.unwrap(), None);
    }

    #[tokio::test]
    async fn a_directory_is_no_file() {
        let dir = tempfile::tempdir().unwrap();
        write(dir.path(), "tests/a.rs", "one\n").await;
        let source = GitArtifacts::new(dir.path().to_path_buf());
        let found = source.read_artifacts(&bytes_of(&["tests"])).await.unwrap();
        assert_eq!(found.get(&p("tests")), Some(&ArtifactReading::absent()));
    }

    #[tokio::test]
    async fn a_file_is_hashed_under_each_comparison_asked_and_its_bytes_always() {
        let dir = tempfile::tempdir().unwrap();
        let text = "// a comment\nfn  main() {}\n";
        write(dir.path(), "src/a.rs", text).await;
        write(dir.path(), "src/b.rs", text).await;
        let source = GitArtifacts::new(dir.path().to_path_buf());
        let slashes = lines(&["//"]);
        let asks = ArtifactAsks::from([
            (p("src/a.rs"), BTreeSet::from([slashes.clone(), lines(&[])])),
            (p("src/b.rs"), BTreeSet::new()),
        ]);
        let found = source.read_artifacts(&asks).await.unwrap();

        let a = &found[&p("src/a.rs")];
        assert!(a.is_present());
        for comparison in [Comparison::Bytes, slashes.clone(), lines(&[])] {
            assert_eq!(
                a.state(&comparison),
                ArtifactState::Present(comparison.fingerprint(text.as_bytes())),
                "{comparison:?}"
            );
        }
        // The whole reading is the one the domain makes of those bytes.
        assert_eq!(
            a,
            &ArtifactReading::of_bytes(text.as_bytes(), &asks[&p("src/a.rs")], None)
        );
        assert_eq!(a.state(&lines(&["#"])), ArtifactState::Absent);
        // Asked for nothing, a file still has the hash of its bytes, and no other.
        let b = &found[&p("src/b.rs")];
        assert_eq!(
            b.state(&Comparison::Bytes),
            ArtifactState::Present(Comparison::Bytes.fingerprint(text.as_bytes()))
        );
        assert_eq!(b.state(&slashes), ArtifactState::Absent);
    }

    #[tokio::test]
    async fn in_a_repository_a_file_has_its_blob_name_and_reads_back_once_committed() {
        if !git_installed().await {
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        run(dir.path(), &["init", "-q"]).await;
        write(dir.path(), "tests/a.rs", "one\n").await;
        let source = GitArtifacts::new(dir.path().to_path_buf());

        // Before the first commit: an address, no revision, and nothing to read back.
        let found = source
            .read_artifacts(&bytes_of(&["tests/a.rs"]))
            .await
            .unwrap();
        let Some(address) = found
            .get(&p("tests/a.rs"))
            .and_then(ArtifactReading::address)
        else {
            panic!("a file in a repository has an address: {found:?}");
        };
        let expected = run(dir.path(), &["hash-object", "tests/a.rs"]).await;
        assert_eq!(address.as_str(), expected);
        assert_eq!(source.revision().await.unwrap(), None);
        assert_eq!(source.read_back(address).await.unwrap(), None);

        run(dir.path(), &["add", "."]).await;
        run(dir.path(), &["commit", "-q", "-m", "start"]).await;
        let head = run(dir.path(), &["rev-parse", "HEAD"]).await;
        assert_eq!(
            source
                .revision()
                .await
                .unwrap()
                .map(|r| r.as_str().to_owned()),
            Some(head)
        );

        // Edited after: the old content still reads back by its name.
        write(dir.path(), "tests/a.rs", "two\n").await;
        assert_eq!(
            source.read_back(address).await.unwrap().as_deref(),
            Some(&b"one\n"[..])
        );
    }

    #[tokio::test]
    async fn a_name_that_is_not_gits_is_not_readable_and_never_reaches_git() {
        let dir = tempfile::tempdir().unwrap();
        let source = GitArtifacts::new(dir.path().to_path_buf());
        for text in ["--output=/tmp/x", "abc", &"g".repeat(40)] {
            let address = ContentAddress::from_validated(text.to_owned()).unwrap();
            assert_eq!(source.read_back(&address).await.unwrap(), None);
        }
    }

    #[test]
    fn an_object_name_is_forty_or_sixty_four_lowercase_hex_digits() {
        assert!(is_object_name(&"a".repeat(40)));
        assert!(is_object_name(&"0".repeat(64)));
        assert!(!is_object_name(&"A".repeat(40)));
        assert!(!is_object_name(&"a".repeat(39)));
        assert!(!is_object_name("-p"));
    }
}
