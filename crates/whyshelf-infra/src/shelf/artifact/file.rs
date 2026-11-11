//! The adapter of the space `file`: the files of a project, by their path under the
//! root, and where their content can be read back.
//!
//! | what | what it does | with |
//! |---|---|---|
//! | [`FileSpace::resolve`] | reads each named file's bytes from the working tree and hashes them; names the blob its content is (or would be) in Git | `tokio::fs`, [`Git::blob_names`] |
//! | [`FileSpace::current`] | the bytes of a named file now | `tokio::fs` |
//! | [`FileSpace::read_back`] | the content of a blob, by its name | [`Git::read_blob`] |
//! | [`presets`] | the preset `file`: the space, `compare = "bytes"`, `means = "depends"` | values |
//!
//! ```text
//!   name ──a path under the root?── no ──► not a name (why)
//!     │ yes
//!   path ──tokio::fs::read──► bytes, kept when no part of the path names an entry in another case only
//!                               ──ArtifactReading::of_bytes──► a hash per comparison asked (always)
//!                               └──git hash-object──► blob name ──► ContentAddress (in a repository)
//!        └─ no file there, or one named in another case only ──► absent
//!   ContentAddress ──git cat-file blob──► bytes, or not readable
//! ```
//!
//! # The names of the space
//!
//! A name of the space `file` is a path relative to the project root, as
//! [`RelPath`] reads it: not empty, not absolute (no leading `/`), no backslash and
//! no control character, and no part that is empty, `.` or `..` (so `a//b`, `./a`
//! and `a/../b` are none). Any other text is not a name of the space, and the adapter
//! says so with why ([`ArtifactReading::not_a_name`]); the document that writes it is
//! invalid. A file that is there is the only thing a name has there: a directory is
//! nothing there. The adapter always answers, and never "cannot tell".
//!
//! # The case of a path
//!
//! A path names a file only as written: one that differs from the name of an entry
//! of its directory in ASCII case only names no file, on every system, as for Git (a
//! file system that does not tell case apart would open it). A name that differs in
//! a non-ASCII letter's case, or that the system stores in another form (macOS keeps
//! some non-ASCII names decomposed), is left to the system.
//!
//! # Outside Git
//!
//! The hash of a file never needs Git, so freshness works the same in any directory.
//! Outside a repository there is no address (see [`git`](super::super::git)).

use std::io;
use std::path::PathBuf;

use whyshelf_core::domain::error::ValueError;
use whyshelf_core::domain::location::RelPath;
use whyshelf_core::domain::nonempty::NonBlank;
use whyshelf_core::domain::shelf::artifact::{
    ArtifactName, ArtifactReading, ContentAddress, Space, SpaceAsks, SpaceReadings,
};
use whyshelf_core::domain::shelf::ledger::PATH_ONLY_SPACE;
use whyshelf_core::domain::shelf::schema::{BuiltinPresets, PresetName, PresetValues, Propagation};

use super::super::git::{Git, GitError};

/// The name of the space this adapter resolves names in: the space a record written
/// before spaces existed is read as naming its files in
/// ([`PATH_ONLY_SPACE`]), so an old pin always names a space this adapter answers
/// for.
pub const FILE_SPACE: &str = PATH_ONLY_SPACE;

/// The name of the preset this adapter offers.
pub const FILE_PRESET: &str = "file";

/// The presets this adapter offers when it resolves the names of `space`: one,
/// [`FILE_PRESET`], for a link to files that stands on each byte of them
/// (`space` that space, `compare = "bytes"`, `means = "depends"`).
pub fn presets(space: &Space) -> BuiltinPresets {
    let values = PresetValues {
        space: Some(space.as_str().to_owned()),
        compare: Some("bytes".to_owned()),
        ignore: None,
        means: Some(Propagation::Depends),
    };
    match PresetName::new(FILE_PRESET) {
        Ok(name) => BuiltinPresets::from([(name, values)]),
        // Not a name: no preset. The test of this module holds it to be one.
        Err(_) => BuiltinPresets::new(),
    }
}

/// Why the files could not be read.
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
    /// Git gave no usable name for a file that is there.
    #[error(transparent)]
    Git(#[from] GitError),
    /// The domain refused the reason this adapter gave (a bug if it ever happens:
    /// the reason is never blank).
    #[error("the domain refused a reason of the adapter")]
    Reason(#[source] ValueError),
}

/// The files of a project, and its Git repository when it is in one.
#[derive(Debug, Clone)]
pub struct FileSpace {
    root: PathBuf,
    git: Git,
}

/// The path a name of the space is: the one reader of the grammar of the names.
fn path_of(name: &ArtifactName) -> Result<RelPath, ValueError> {
    name.as_str().parse::<RelPath>()
}

impl FileSpace {
    /// The files under `root`.
    pub fn new(root: PathBuf) -> Self {
        Self {
            git: Git::new(root.clone()),
            root,
        }
    }

    /// For each name: not a name (no path under the root), the hash of the file
    /// under each comparison asked and where its content can be read back, or no
    /// file. Every name asked about has an answer.
    pub async fn resolve(&self, asks: &SpaceAsks) -> Result<SpaceReadings, ArtifactError> {
        let mut readings = SpaceReadings::new();
        let mut present = Vec::new();
        for (name, comparisons) in asks {
            let path = match path_of(name) {
                Ok(path) => path,
                Err(why) => {
                    let why =
                        NonBlank::new(format!("no path under the root of the project: {why}"))
                            .map_err(ArtifactError::Reason)?;
                    readings.insert(name.clone(), ArtifactReading::not_a_name(why));
                    continue;
                }
            };
            match bytes(&self.root, &path).await? {
                Some(bytes) => {
                    readings.insert(
                        name.clone(),
                        ArtifactReading::of_bytes(&bytes, comparisons, None),
                    );
                    present.push((name, path));
                }
                None => {
                    readings.insert(name.clone(), ArtifactReading::absent());
                }
            }
        }
        let paths: Vec<&RelPath> = present.iter().map(|(_, path)| path).collect();
        if let Some(addresses) = self.git.blob_names(&paths).await? {
            for ((name, _), address) in present.iter().zip(addresses) {
                if let Some(reading) = readings.get_mut(*name) {
                    reading.set_address(address);
                }
            }
        }
        Ok(readings)
    }

    /// The bytes of the file a name is, as they are now, or none when it is no path
    /// or no file is there.
    pub async fn current(&self, name: &ArtifactName) -> Result<Option<Vec<u8>>, ArtifactError> {
        match path_of(name) {
            Ok(path) => bytes(&self.root, &path).await,
            Err(_) => Ok(None),
        }
    }

    /// The content a file had at `address`, or none when it cannot be read back.
    pub async fn read_back(&self, address: &ContentAddress) -> Option<Vec<u8>> {
        self.git.read_blob(address).await
    }
}

/// The bytes of a file under `root`, or none when no file is there (nothing, a
/// directory, or a path that names an entry in another case only).
async fn bytes(root: &std::path::Path, path: &RelPath) -> Result<Option<Vec<u8>>, ArtifactError> {
    let full = root.join(path.as_str());
    let bytes = match tokio::fs::read(&full).await {
        Ok(bytes) => bytes,
        Err(e) if is_absent(&e) => return Ok(None),
        Err(source) => return Err(ArtifactError::Io { path: full, source }),
    };
    if named_as_written(root, path).await {
        Ok(Some(bytes))
    } else {
        Ok(None)
    }
}

/// Whether a path that opened names the file as written (the rule of the case of a
/// path, on the module page): no part of it is the name of an entry of its directory
/// in another ASCII case only. A directory that cannot be listed says nothing against
/// the path, which opened.
async fn named_as_written(root: &std::path::Path, path: &RelPath) -> bool {
    let mut dir = root.to_path_buf();
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

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;
    use std::process::Stdio;

    use tokio::process::Command;
    use whyshelf_core::app::shelf::RevisionSource;
    use whyshelf_core::domain::shelf::artifact::{ArtifactState, Comparison, LinePrefix, Untold};

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

    fn p(text: &str) -> ArtifactName {
        ArtifactName::new(text).unwrap()
    }

    /// Each name, asked for its bytes only.
    fn bytes_of(paths: &[&str]) -> SpaceAsks {
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

    #[test]
    fn the_space_of_the_files_is_the_one_an_old_record_names() {
        assert_eq!(FILE_SPACE, PATH_ONLY_SPACE);
    }

    #[test]
    fn the_adapter_offers_the_preset_file_for_its_space() {
        let space = Space::new(FILE_SPACE).unwrap();
        let offered = presets(&space);
        let name = PresetName::new(FILE_PRESET).unwrap();
        assert_eq!(
            offered,
            BuiltinPresets::from([(
                name,
                PresetValues {
                    space: Some("file".to_owned()),
                    compare: Some("bytes".to_owned()),
                    ignore: None,
                    means: Some(Propagation::Depends),
                }
            )])
        );
    }

    #[tokio::test]
    async fn outside_a_repository_a_file_has_a_hash_and_no_address() {
        let dir = tempfile::tempdir().unwrap();
        write(dir.path(), "tests/a.rs", "one\n").await;
        let source = FileSpace::new(dir.path().to_path_buf());
        let found = source
            .resolve(&bytes_of(&["tests/a.rs", "tests/none.rs"]))
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
        assert_eq!(
            source.current(&p("tests/a.rs")).await.unwrap().as_deref(),
            Some(&b"one\n"[..])
        );
        assert_eq!(source.current(&p("tests/none.rs")).await.unwrap(), None);
    }

    #[tokio::test]
    async fn a_text_that_is_no_path_under_the_root_is_not_a_name() {
        let dir = tempfile::tempdir().unwrap();
        write(dir.path(), "a.rs", "one\n").await;
        let source = FileSpace::new(dir.path().to_path_buf());
        let wrongs = ["../a.rs", "/a.rs", "a//b", "./a.rs", "a/../a.rs", "a\\b"];
        let mut all = wrongs.to_vec();
        all.push("a.rs");
        let found = source.resolve(&bytes_of(&all)).await.unwrap();
        for wrong in wrongs {
            let why = found[&p(wrong)].not_a_name_why();
            assert!(
                why.is_some_and(|w| w.as_str().starts_with("no path under the root")),
                "{wrong}: {why:?}"
            );
            assert_eq!(source.current(&p(wrong)).await.unwrap(), None);
        }
        assert!(found[&p("a.rs")].is_present());
        assert_eq!(found[&p("a.rs")].not_a_name_why(), None);
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
        let source = FileSpace::new(dir.path().to_path_buf());
        let found = source
            .resolve(&bytes_of(&[
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
        let source = FileSpace::new(dir.path().to_path_buf());
        let found = source.resolve(&bytes_of(&["tests"])).await.unwrap();
        assert_eq!(found.get(&p("tests")), Some(&ArtifactReading::absent()));
    }

    #[tokio::test]
    async fn a_file_is_hashed_under_each_comparison_asked_and_its_bytes_always() {
        let dir = tempfile::tempdir().unwrap();
        let text = "// a comment\nfn  main() {}\n";
        write(dir.path(), "src/a.rs", text).await;
        write(dir.path(), "src/b.rs", text).await;
        let source = FileSpace::new(dir.path().to_path_buf());
        let slashes = lines(&["//"]);
        let asks = SpaceAsks::from([
            (p("src/a.rs"), BTreeSet::from([slashes.clone(), lines(&[])])),
            (p("src/b.rs"), BTreeSet::new()),
        ]);
        let found = source.resolve(&asks).await.unwrap();

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
        // A comparison not asked cannot be told.
        assert_eq!(
            a.state(&lines(&["#"])),
            ArtifactState::Unknown(Untold::NotAsked)
        );
        // Asked for nothing, a file still has the hash of its bytes, and no other.
        let b = &found[&p("src/b.rs")];
        assert_eq!(
            b.state(&Comparison::Bytes),
            ArtifactState::Present(Comparison::Bytes.fingerprint(text.as_bytes()))
        );
        assert_eq!(b.state(&slashes), ArtifactState::Unknown(Untold::NotAsked));
    }

    #[tokio::test]
    async fn in_a_repository_a_file_has_its_blob_name_and_reads_back_once_committed() {
        if !git_installed().await {
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        run(dir.path(), &["init", "-q"]).await;
        write(dir.path(), "tests/a.rs", "one\n").await;
        let source = FileSpace::new(dir.path().to_path_buf());
        let git = Git::new(dir.path().to_path_buf());

        // Before the first commit: an address, no revision, and nothing to read back.
        let found = source.resolve(&bytes_of(&["tests/a.rs"])).await.unwrap();
        let Some(address) = found
            .get(&p("tests/a.rs"))
            .and_then(ArtifactReading::address)
        else {
            panic!("a file in a repository has an address: {found:?}");
        };
        let expected = run(dir.path(), &["hash-object", "tests/a.rs"]).await;
        assert_eq!(address.as_str(), expected);
        assert_eq!(git.revision().await.unwrap(), None);
        assert_eq!(source.read_back(address).await, None);

        run(dir.path(), &["add", "."]).await;
        run(dir.path(), &["commit", "-q", "-m", "start"]).await;
        let head = run(dir.path(), &["rev-parse", "HEAD"]).await;
        assert_eq!(
            git.revision().await.unwrap().map(|r| r.as_str().to_owned()),
            Some(head)
        );

        // Edited after: the old content still reads back by its name.
        write(dir.path(), "tests/a.rs", "two\n").await;
        assert_eq!(
            source.read_back(address).await.as_deref(),
            Some(&b"one\n"[..])
        );
    }
}
