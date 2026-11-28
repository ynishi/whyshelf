//! The crates of the project at a side, as the targets of its manifests give them
//! ([`Targets`](super::super::targets)): the directory of the project the source of
//! each crate is under, so that a page of the source in the docs
//! (`src/<crate>/<path>.html`, rustdoc's name of a file by its crate and its path under
//! the directory of the crate's root file) names a file of the project.
//!
//! | what | what it does |
//! |---|---|
//! | `Crates::of` | the targets of the project at a side, from the manifests the space reads (at the root, else in each directory at the top of the tree): its libraries and binaries are its crates |
//! | `Crates::crate_dir` | the directory of the root file of a crate of the project, or what the crate is instead ([`SourceOf`]) |
//! | `Crates::path_dependencies` | the crates of the project the package of a crate depends on by a path |
//! | `Crates::file_of` | the file of the project a page of the source in the docs is of, or what it is instead ([`SourceOf`]) |
//!
//! The rule of which manifests are read, which crates a package has and where the
//! source of each is, is on the page of the space, "Where a name lives"
//! ([`rust_item`](super)); the targets carry it out, and this module reads no manifest.
//! What could not be read of the project is kept with why, and a crate not found then
//! cannot be told, rather than not one of the project.
//!
//! The file of a page is told by the part `P` of its place after `src/<crate>/` (its
//! last part without `.html`), by the first of these that applies:
//!
//! | rule | when | the page is of |
//! |---|---|---|
//! | 1 | `P` starts with the parts of the build directory, as an absolute path ([`Sited`], either form), and goes on as the output of a build script does (`<profile>/build/<package>-<hash>/out/…`, or with a target triple before the profile) | a source a build made ([`SourceOf::MadeByBuild`]) |
//! | 2 | `P` starts with the parts of the root of the project, as an absolute path | the file of the rest of `P` ([`SourceOf::File`]) |
//! | 3 | `P` has a part `up` (rustdoc's `..`, as older versions write it) | the file `P` names from the directory of the crate's root file, `up` read as `..`; one that climbs past the root leaves the project ([`SourceOf::LeavesProject`]) |
//! | 4 | any other | the first file of the side of `P` under the directory of the crate's root file, then under each directory above it up to the root: rustdoc drops a `..` of a path and does not say how many there were; with none there, the one under the directory of the crate's root file |
//!
//! This module reads the project through its state alone, as every reader does.

use whyshelf_core::app::shelf::Side;
use whyshelf_core::domain::location::RelPath;
use whyshelf_core::domain::nonempty::NonBlank;

use super::super::super::state::ProjectTree;
use super::super::file;
use super::super::targets::{CrateOf, Dir, MANIFEST, Targets, Top, shown};
use super::docs_path::DocsPath;
use super::sited::Sited;

/// The name rustdoc gives the directory of the pages of the source, at the top of the
/// docs.
const SOURCE_DIR: &str = "src";

/// The part rustdoc writes in the path of a page of the source for `..`.
const UP: &str = "up";

/// What a page of the source in the docs is of ([`Crates::file_of`]), or, for a crate,
/// what it is instead of one of the project ([`Crates::crate_dir`]): the one value
/// that the holding of the docs against the sources and the place of a name read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum SourceOf {
    /// A file of the project, by the rules of the module page.
    File(RelPath),
    /// A source a build made, in the build directory: no file of the project.
    MadeByBuild,
    /// A path that climbs out of the project, or is no path of it.
    LeavesProject,
    /// The crate is no crate of the manifests the space reads: a dependency's, or one
    /// of a package those manifests do not reach.
    NoCrate,
    /// Two packages of the project have a crate of the name, and why it is said so.
    TwoCrates(NonBlank),
    /// No crate of the manifests read has the name, and some could not be read: why.
    ManifestsUnread(NonBlank),
    /// The place is no page of the source (`src/<crate>/<path>.html`).
    NotASourcePage,
}

impl SourceOf {
    /// What an item placed by it is told when it names no file, as a reason writes it,
    /// for the page `page`; none for a file.
    pub(super) fn unplaced(&self, page: &DocsPath) -> Option<String> {
        let parts: Vec<&str> = page.parts().collect();
        let krate = parts.get(1).copied().unwrap_or_default();
        match self {
            Self::File(_) => None,
            Self::MadeByBuild => Some(NOT_A_FILE.to_owned()),
            Self::LeavesProject => Some(format!(
                "the source of the crate `{krate}` leads out of the project: {}",
                parts.join("/")
            )),
            Self::NoCrate => Some(format!(
                "the crate `{krate}` is no crate of the manifests read (those at the root, else in each directory at its top)"
            )),
            Self::TwoCrates(why) | Self::ManifestsUnread(why) => Some(why.to_string()),
            Self::NotASourcePage => Some(format!(
                "the docs link to no page of the source: {}",
                parts.join("/")
            )),
        }
    }
}

/// What an item whose page names a source a build made is placed as. The path is not
/// said: rustdoc names such a source by its whole path.
pub(super) const NOT_A_FILE: &str = "the source its page names is no file of the project (rustdoc names a source outside the directory of the crate's root file, such as one a build writes, by its whole path)";

/// The crates of the project at one side, by the name rustdoc gives each: the targets
/// of the project.
#[derive(Debug, Clone)]
pub(super) struct Crates {
    /// The tree of the project, and the side read: where a file of rule 4 is looked
    /// for.
    state: ProjectTree,
    side: Side,
    /// The targets of the project.
    targets: Targets,
    /// What could not be read, each with why: a manifest, a member, a directory of a
    /// package, or the manifest at the root that is not there.
    unread: Vec<String>,
}

impl Crates {
    /// The crates of the project at `side`: the targets read from the manifest at its
    /// root and those of the members of its workspace, or, with none at the root, from
    /// the manifest of each directory at its top ([`Targets::of_project`]).
    pub(super) async fn of(state: &ProjectTree, side: Side) -> Self {
        let targets = Targets::of_project(state, side).await;
        let mut unread: Vec<String> = targets.unread().iter().map(ToString::to_string).collect();
        if targets.top() == Top::Absent {
            unread.push(format!(
                "the project has no {MANIFEST} at its root, nor in a directory at its top"
            ));
        }
        Self {
            state: state.clone(),
            side,
            targets,
            unread,
        }
    }

    /// The directory of the root file of the crate `krate` of the project, or what the
    /// crate is instead: none of the project ([`SourceOf::NoCrate`]), two
    /// ([`SourceOf::TwoCrates`]), or not found while some manifest could not be read
    /// ([`SourceOf::ManifestsUnread`]).
    pub(super) fn crate_dir(&self, krate: &str) -> Result<Dir, SourceOf> {
        // The texts below are never blank: a blank one would be no page of the source.
        let said = |text: String, of: fn(NonBlank) -> SourceOf| {
            NonBlank::new(text).map_or(SourceOf::NotASourcePage, of)
        };
        match self.targets.crate_named(krate) {
            CrateOf::One(target) => Ok(target.root_dir()),
            CrateOf::Two(one, two) => Err(said(
                format!(
                    "two packages of the project have a crate `{krate}`, under {} and {}",
                    shown(&one.root_dir()),
                    shown(&two.root_dir())
                ),
                SourceOf::TwoCrates,
            )),
            CrateOf::None if self.unread.is_empty() => Err(SourceOf::NoCrate),
            CrateOf::None => Err(said(
                format!(
                    "the crate `{krate}` is no crate of the manifests read, and some could not be: {}",
                    self.unread.join("; ")
                ),
                SourceOf::ManifestsUnread,
            )),
        }
    }

    /// The crates of the project (their names) that the package of the crate `krate`
    /// depends on by a path: none for a crate that is no one crate of the project.
    pub(super) fn path_dependencies(&self, krate: &str) -> Vec<String> {
        match self.targets.crate_named(krate) {
            CrateOf::One(target) => self
                .targets
                .path_dependencies(target)
                .into_iter()
                .map(|lib| lib.name.clone())
                .collect(),
            CrateOf::Two(..) | CrateOf::None => Vec::new(),
        }
    }

    /// What the page of the source `page` is of, by the rules of the module page, with
    /// `sited` where the docs were built from and into: a file of the project, or what
    /// it is instead.
    pub(super) async fn file_of(&self, page: &DocsPath, sited: &Sited) -> SourceOf {
        let parts: Vec<&str> = page.parts().collect();
        let (krate, below) = match parts.as_slice() {
            [top, krate, below @ ..] if *top == SOURCE_DIR && !below.is_empty() => (*krate, below),
            _ => return SourceOf::NotASourcePage,
        };
        let mut rest: Vec<String> = below.iter().map(|part| (*part).to_owned()).collect();
        match rest
            .last_mut()
            .and_then(|last| last.strip_suffix(".html").map(str::to_owned))
        {
            Some(name) if !name.is_empty() => {
                if let Some(last) = rest.last_mut() {
                    *last = name;
                }
            }
            _ => return SourceOf::NotASourcePage,
        }
        let dir = match self.crate_dir(krate) {
            Ok(dir) => dir,
            Err(instead) => return instead,
        };
        // 1: a source the build made, by its whole path: the output of a build script
        // under the build directory, and nothing else there (the build directory may be
        // the root, or above it).
        if sited.build.rest(&rest).is_some_and(made_by_a_build_script) {
            return SourceOf::MadeByBuild;
        }
        // 2: a file of the project, by its whole path.
        if let Some(under) = sited.root.rest(&rest) {
            return file_at(under);
        }
        // 3: rustdoc's `up` for `..`.
        if rest.iter().any(|part| part == UP) {
            let mut file = dir;
            for part in rest {
                if part == UP {
                    if file.pop().is_none() {
                        return SourceOf::LeavesProject;
                    }
                } else {
                    file.push(part);
                }
            }
            return file_at(&file);
        }
        // 4: the first file of the side from the directory of the crate's root file up.
        let first = joined(&dir, &rest);
        for above in (0..=dir.len()).rev() {
            let Ok(path) = joined(&dir[..above], &rest).parse::<RelPath>() else {
                continue;
            };
            let there = match file::placed_within(&self.state, self.side, &path).await {
                file::Placed::Files { there, .. } => there,
                file::Placed::Unplaced(_) => true,
            };
            if there {
                return SourceOf::File(path);
            }
        }
        file_at_text(&first)
    }
}

/// Whether `rest`, the parts of a place under the build directory, is in the output of
/// a build script, as cargo lays it out: `<profile>/build/<package>-<hash>/out/…`, or
/// `<triple>/<profile>/build/<package>-<hash>/out/…`.
fn made_by_a_build_script(rest: &[String]) -> bool {
    [1, 2].into_iter().any(|at| {
        rest.get(at).is_some_and(|part| part == "build")
            && rest.get(at + 2).is_some_and(|part| part == "out")
            && rest.len() > at + 3
    })
}

/// The file at the parts `parts`, or a path that leaves the project when they are no
/// path of it.
fn file_at(parts: &[String]) -> SourceOf {
    file_at_text(&parts.join("/"))
}

fn file_at_text(text: &str) -> SourceOf {
    text.parse().map_or(SourceOf::LeavesProject, SourceOf::File)
}

/// The path of `rest` under the directory `dir`, as text.
fn joined(dir: &[String], rest: &[String]) -> String {
    dir.iter()
        .chain(rest)
        .map(String::as_str)
        .collect::<Vec<_>>()
        .join("/")
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use super::super::super::targets::matches;
    use super::super::fixture::write;
    use super::super::sited::Forms;
    use super::*;

    fn page(text: &str) -> DocsPath {
        DocsPath::of_relative(Path::new(text)).unwrap()
    }

    fn file(text: &str) -> RelPath {
        text.parse().unwrap()
    }

    /// What the page `text` is of, as a file or the reason an item placed by it is
    /// told, where neither the root nor the build directory is a whole path of it.
    async fn of(crates: &Crates, text: &str) -> Result<RelPath, String> {
        let sited = Sited {
            root: Forms::new(PathBuf::from("/nowhere/root"), None),
            build: Forms::new(PathBuf::from("/nowhere/build"), None),
        };
        let page = page(text);
        match crates.file_of(&page, &sited).await {
            SourceOf::File(file) => Ok(file),
            other => Err(other.unplaced(&page).unwrap_or_default()),
        }
    }

    async fn crates_at(root: &Path) -> Crates {
        let tree = ProjectTree::files(crate::shelf::state::files::FileStates::new(
            root.to_path_buf(),
        ));
        Crates::of(&tree, Side::After).await
    }

    #[test]
    fn a_part_of_a_member_matches_by_its_stars_and_marks() {
        assert!(matches("*", "whyshelf-core"));
        assert!(matches("whyshelf-*", "whyshelf-core"));
        assert!(matches("?ore", "core"));
        assert!(matches("a*b*c", "aXbYc"));
        assert!(!matches("a*b", "aXc"));
        assert!(!matches("core?", "core"));
    }

    #[tokio::test]
    async fn a_package_at_the_root_has_its_library_and_its_binary() {
        let dir = tempfile::tempdir().unwrap();
        write(dir.path(), MANIFEST, "[package]\nname = \"my-demo\"\n").await;
        write(dir.path(), "src/lib.rs", "pub mod a;\n").await;
        let crates = crates_at(dir.path()).await;
        assert_eq!(
            of(&crates, "src/my_demo/lib.rs.html").await,
            Ok(file("src/lib.rs"))
        );
        assert_eq!(
            of(&crates, "src/my_demo/a/b.rs.html").await,
            Ok(file("src/a/b.rs"))
        );
        // A part `up` is rustdoc's `..`.
        assert_eq!(
            of(&crates, "src/my_demo/up/build.rs.html").await,
            Ok(file("build.rs"))
        );
        assert!(
            of(&crates, "src/my_demo/up/up/x.rs.html")
                .await
                .unwrap_err()
                .contains("out of the project")
        );
        assert!(
            of(&crates, "src/serde/lib.rs.html")
                .await
                .unwrap_err()
                .contains("no crate of the project")
        );
        assert!(of(&crates, "my_demo/struct.A.html").await.is_err());
    }

    #[tokio::test]
    async fn a_package_with_no_library_is_its_binaries_alone() {
        let dir = tempfile::tempdir().unwrap();
        write(
            dir.path(),
            MANIFEST,
            "[package]\nname = \"tool\"\n[[bin]]\nname = \"tool\"\npath = \"cli/main.rs\"\n",
        )
        .await;
        write(dir.path(), "src/util.rs", "pub fn x() {}\n").await;
        write(dir.path(), "cli/main.rs", "fn main() {}\n").await;
        let crates = crates_at(dir.path()).await;
        // No `[lib]` and no `src/lib.rs`: the crate `tool` is the binary, under `cli`.
        assert_eq!(
            of(&crates, "src/tool/main.rs.html").await,
            Ok(file("cli/main.rs"))
        );
        // With a `src/lib.rs`, the library of the name is the crate.
        write(dir.path(), "src/lib.rs", "pub mod util;\n").await;
        let crates = crates_at(dir.path()).await;
        assert_eq!(
            of(&crates, "src/tool/util.rs.html").await,
            Ok(file("src/util.rs"))
        );
    }

    #[tokio::test]
    async fn the_binaries_the_layout_holds_are_found_beside_those_declared_unless_autobins_is_off()
    {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        write(
            root,
            MANIFEST,
            "[package]\nname = \"kit\"\nedition = \"2021\"\n[[bin]]\nname = \"extra\"\npath = \"tools/extra.rs\"\n",
        )
        .await;
        write(root, "src/main.rs", "fn main() {}\n").await;
        write(root, "src/bin/one.rs", "fn main() {}\n").await;
        write(root, "src/bin/two/main.rs", "fn main() {}\n").await;
        write(root, "src/bin/two/part.rs", "pub fn x() {}\n").await;
        write(root, "tools/extra.rs", "fn main() {}\n").await;
        let crates = crates_at(root).await;
        // A `[[bin]]` declared, and still `src/main.rs` and `src/bin/*` by the layout.
        assert_eq!(
            of(&crates, "src/extra/extra.rs.html").await,
            Ok(file("tools/extra.rs"))
        );
        assert_eq!(
            of(&crates, "src/kit/main.rs.html").await,
            Ok(file("src/main.rs"))
        );
        assert_eq!(
            of(&crates, "src/one/one.rs.html").await,
            Ok(file("src/bin/one.rs"))
        );
        assert_eq!(
            of(&crates, "src/two/part.rs.html").await,
            Ok(file("src/bin/two/part.rs"))
        );
        // A declared binary of a found one's name takes its place.
        write(
            root,
            MANIFEST,
            "[package]\nname = \"kit\"\nedition = \"2021\"\n[[bin]]\nname = \"one\"\npath = \"tools/extra.rs\"\n",
        )
        .await;
        let crates = crates_at(root).await;
        assert_eq!(
            of(&crates, "src/one/extra.rs.html").await,
            Ok(file("tools/extra.rs"))
        );
        // `autobins = false`: only what is declared.
        write(
            root,
            MANIFEST,
            "[package]\nname = \"kit\"\nautobins = false\n[[bin]]\nname = \"extra\"\npath = \"tools/extra.rs\"\n",
        )
        .await;
        let crates = crates_at(root).await;
        assert_eq!(
            of(&crates, "src/extra/extra.rs.html").await,
            Ok(file("tools/extra.rs"))
        );
        for gone in ["src/kit/main.rs.html", "src/one/one.rs.html"] {
            assert!(of(&crates, gone).await.is_err(), "{gone}");
        }
        // A declared binary with no path, `autobins = false`: its path is still the
        // layout's, `src/bin/<name>/main.rs` among them.
        write(
            root,
            MANIFEST,
            "[package]\nname = \"kit\"\nedition = \"2021\"\nautobins = false\n[[bin]]\nname = \"two\"\n",
        )
        .await;
        let crates = crates_at(root).await;
        assert_eq!(
            of(&crates, "src/two/part.rs.html").await,
            Ok(file("src/bin/two/part.rs"))
        );
        assert!(of(&crates, "src/one/one.rs.html").await.is_err());
    }

    #[tokio::test]
    async fn of_the_edition_2015_a_declared_binary_turns_the_layout_off_unless_autobins_says() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        write(root, "src/main.rs", "fn main() {}\n").await;
        write(root, "src/bin/one.rs", "fn main() {}\n").await;
        write(root, "tools/extra.rs", "fn main() {}\n").await;
        let declared = "[[bin]]\nname = \"extra\"\npath = \"tools/extra.rs\"\n";
        // No edition written is 2015: only what is declared.
        write(
            root,
            MANIFEST,
            &format!("[package]\nname = \"kit\"\n{declared}"),
        )
        .await;
        let crates = crates_at(root).await;
        assert_eq!(
            of(&crates, "src/extra/extra.rs.html").await,
            Ok(file("tools/extra.rs"))
        );
        assert!(of(&crates, "src/kit/main.rs.html").await.is_err());
        assert!(of(&crates, "src/one/one.rs.html").await.is_err());
        // `autobins = true` finds them again; so does the edition 2018 and later.
        for package in [
            "[package]\nname = \"kit\"\nautobins = true\n",
            "[package]\nname = \"kit\"\nedition = \"2018\"\n",
        ] {
            write(root, MANIFEST, &format!("{package}{declared}")).await;
            let crates = crates_at(root).await;
            assert_eq!(
                of(&crates, "src/one/one.rs.html").await,
                Ok(file("src/bin/one.rs")),
                "{package}"
            );
        }
        // An edition inherited from the workspace is the workspace's.
        write(
            root,
            MANIFEST,
            &format!(
                "[workspace]\n[workspace.package]\nedition = \"2021\"\n[package]\nname = \"kit\"\nedition.workspace = true\n{declared}"
            ),
        )
        .await;
        let crates = crates_at(root).await;
        assert_eq!(
            of(&crates, "src/kit/main.rs.html").await,
            Ok(file("src/main.rs"))
        );
        // With no binary declared, the layout is read whatever the edition.
        write(root, MANIFEST, "[package]\nname = \"kit\"\n").await;
        let crates = crates_at(root).await;
        assert_eq!(
            of(&crates, "src/one/one.rs.html").await,
            Ok(file("src/bin/one.rs"))
        );
    }

    #[tokio::test]
    async fn autolib_off_leaves_a_src_lib_rs_no_library() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        write(root, "src/lib.rs", "pub fn x() {}\n").await;
        write(
            root,
            MANIFEST,
            "[package]\nname = \"kit\"\nautolib = false\n",
        )
        .await;
        let crates = crates_at(root).await;
        assert!(of(&crates, "src/kit/lib.rs.html").await.is_err());
    }

    #[tokio::test]
    async fn exclude_takes_out_what_is_under_its_paths_and_a_member_written_as_a_path_stays() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        write(
            root,
            MANIFEST,
            "[workspace]\nmembers = [\"crates/*\", \"crates/old/kept\", \"vendor/*/*\"]\nexclude = [\"crates/old\", \"vendor/x\"]\n",
        )
        .await;
        for (at, name) in [
            ("crates/new", "new"),
            ("crates/old", "old"),
            ("crates/old/kept", "kept"),
            ("vendor/x/a", "xa"),
            ("vendor/y/b", "yb"),
        ] {
            write(
                root,
                &format!("{at}/Cargo.toml"),
                &format!("[package]\nname = \"{name}\"\n"),
            )
            .await;
            write(root, &format!("{at}/src/lib.rs"), "pub fn x() {}\n").await;
        }
        let crates = crates_at(root).await;
        assert_eq!(
            of(&crates, "src/new/lib.rs.html").await,
            Ok(file("crates/new/src/lib.rs"))
        );
        assert_eq!(
            of(&crates, "src/kept/lib.rs.html").await,
            Ok(file("crates/old/kept/src/lib.rs"))
        );
        assert_eq!(
            of(&crates, "src/yb/lib.rs.html").await,
            Ok(file("vendor/y/b/src/lib.rs"))
        );
        assert!(of(&crates, "src/old/lib.rs.html").await.is_err());
        assert!(of(&crates, "src/xa/lib.rs.html").await.is_err());
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn a_member_through_a_link_and_a_source_that_cannot_be_listed_are_not_read_and_said() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        write(root, MANIFEST, "[workspace]\nmembers = [\"crates/*\"]\n").await;
        write(root, "elsewhere/Cargo.toml", "[package]\nname = \"far\"\n").await;
        write(root, "elsewhere/src/lib.rs", "pub fn x() {}\n").await;
        write(
            root,
            "crates/near/Cargo.toml",
            "[package]\nname = \"near\"\n",
        )
        .await;
        write(root, "crates/near/src/lib.rs", "pub fn x() {}\n").await;
        // arch-lint: allow(readers-read-through-the-state) reason="a test sets up its tree on the file system"
        tokio::fs::symlink(root.join("elsewhere"), root.join("crates/far"))
            .await
            .unwrap();
        let crates = crates_at(root).await;
        let why = of(&crates, "src/far/lib.rs.html").await.unwrap_err();
        assert!(why.contains("symbolic link"), "{why}");
        // A `src` that cannot be listed is not read, and said: not a library missing.
        let src = root.join("crates/near/src");
        // arch-lint: allow(readers-read-through-the-state) reason="a test locks a directory of its tree"
        tokio::fs::set_permissions(&src, std::fs::Permissions::from_mode(0o000))
            .await
            .unwrap();
        // arch-lint: allow(readers-read-through-the-state) reason="a test asks whether its tree locks a directory"
        let locked = tokio::fs::read_dir(&src).await.is_err();
        let crates = crates_at(root).await;
        // arch-lint: allow(readers-read-through-the-state) reason="a test unlocks a directory of its tree"
        tokio::fs::set_permissions(&src, std::fs::Permissions::from_mode(0o755))
            .await
            .unwrap();
        // Run as a user every directory opens to (root), it is no case to test.
        if locked {
            let why = of(&crates, "src/near/lib.rs.html").await.unwrap_err();
            assert!(why.contains("could not be"), "{why}");
        }
    }

    #[tokio::test]
    async fn the_members_of_a_workspace_are_read_by_their_manifests_and_patterns() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        write(
            root,
            MANIFEST,
            "[workspace]\nmembers = [\"crates/*\", \"tool\"]\nexclude = [\"crates/skip\"]\n",
        )
        .await;
        write(
            root,
            "crates/core/Cargo.toml",
            "[package]\nname = \"my-core\"\n",
        )
        .await;
        write(
            root,
            "crates/cli/Cargo.toml",
            "[package]\nname = \"my-cli\"\n[[bin]]\nname = \"my\"\npath = \"src/main.rs\"\n",
        )
        .await;
        write(
            root,
            "crates/lib/Cargo.toml",
            "[package]\nname = \"x\"\n[lib]\nname = \"other\"\npath = \"code/top.rs\"\n",
        )
        .await;
        write(
            root,
            "crates/skip/Cargo.toml",
            "[package]\nname = \"skip\"\n",
        )
        .await;
        write(root, "crates/empty/README", "no manifest\n").await;
        write(root, "tool/Cargo.toml", "[package]\nname = \"tool\"\n").await;
        for lib in [
            "crates/core/src/lib.rs",
            "crates/skip/src/lib.rs",
            "tool/src/lib.rs",
        ] {
            write(root, lib, "pub fn x() {}\n").await;
        }
        let crates = crates_at(root).await;
        assert_eq!(
            of(&crates, "src/my_core/app/x.rs.html").await,
            Ok(file("crates/core/src/app/x.rs"))
        );
        assert_eq!(
            of(&crates, "src/my/main.rs.html").await,
            Ok(file("crates/cli/src/main.rs"))
        );
        assert_eq!(
            of(&crates, "src/other/top.rs.html").await,
            Ok(file("crates/lib/code/top.rs"))
        );
        assert_eq!(
            of(&crates, "src/tool/lib.rs.html").await,
            Ok(file("tool/src/lib.rs"))
        );
        assert!(of(&crates, "src/skip/lib.rs.html").await.is_err());
        assert_eq!(crates.unread, Vec::<String>::new());
    }

    #[tokio::test]
    async fn two_libraries_of_one_name_and_a_manifest_not_read_cannot_be_told() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        write(
            root,
            MANIFEST,
            "[workspace]\nmembers = [\"a\", \"b\", \"c/**\", \"d/[xy]\"]\n",
        )
        .await;
        write(root, "a/Cargo.toml", "[package]\nname = \"same\"\n").await;
        write(root, "b/Cargo.toml", "[package]\nname = \"same\"\n").await;
        write(root, "a/src/lib.rs", "pub fn x() {}\n").await;
        write(root, "b/src/lib.rs", "pub fn x() {}\n").await;
        let crates = crates_at(root).await;
        assert!(
            of(&crates, "src/same/lib.rs.html")
                .await
                .unwrap_err()
                .contains("two packages")
        );
        assert!(
            of(&crates, "src/elsewhere/lib.rs.html")
                .await
                .unwrap_err()
                .contains("`**`")
        );
        // A member with `[` is not read either, and says so.
        assert!(
            of(&crates, "src/elsewhere/lib.rs.html")
                .await
                .unwrap_err()
                .contains("`[`")
        );
        // No manifest at the root: nothing is a crate of the project, and it says so.
        let bare = tempfile::tempdir().unwrap();
        let crates = crates_at(bare.path()).await;
        assert!(
            of(&crates, "src/demo/lib.rs.html")
                .await
                .unwrap_err()
                .contains("has no Cargo.toml")
        );
    }
}
