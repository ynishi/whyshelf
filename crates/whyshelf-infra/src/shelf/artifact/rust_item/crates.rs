//! The crates of the project at a side, as the targets of its manifests give them
//! ([`Targets`](super::super::targets)): the directory of the project the source of
//! each crate is under, so that a page of the source in the docs
//! (`src/<crate>/<path>.html`, rustdoc's name of a file by its crate and its path under
//! the directory of the crate's root file) names a file of the project.
//!
//! | what | what it does |
//! |---|---|
//! | `Crates::of` | the targets of the project at a side: its libraries and binaries are its crates |
//! | `Crates::file_of` | the file of the project a page of the source in the docs is of, or why it cannot be told |
//!
//! The rule of which manifests are read, which crates a package has and where the
//! source of each is, is on the page of the space, "Where a name lives"
//! ([`rust_item`](super)); the targets carry it out, and this module reads no manifest.
//! What could not be read of the project is kept with why, and a crate not found then
//! cannot be told, rather than not one of the project.
//!
//! This module reads the project through its state alone, as every reader does.

use whyshelf_core::app::shelf::Side;
use whyshelf_core::domain::location::RelPath;

use super::super::super::state::ProjectTree;
use super::super::targets::{CrateOf, MANIFEST, Targets, Top, shown};
use super::docs_path::DocsPath;

/// The name rustdoc gives the directory of the pages of the source, at the top of the
/// docs.
const SOURCE_DIR: &str = "src";

/// The part rustdoc writes in the path of a page of the source for `..`.
const UP: &str = "up";

/// The crates of the project at one side, by the name rustdoc gives each: the targets
/// of the project.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(super) struct Crates {
    /// The targets of the project.
    targets: Targets,
    /// What could not be read, each with why: a manifest, a member, a directory of a
    /// package, or the manifest at the root that is not there.
    unread: Vec<String>,
}

impl Crates {
    /// The crates of the project at `side`: the targets read from the manifest at its
    /// root and those of the members of its workspace.
    pub(super) async fn of(state: &ProjectTree, side: Side) -> Self {
        let targets = Targets::of(state, side).await;
        let mut unread: Vec<String> = targets.unread().iter().map(ToString::to_string).collect();
        if targets.top() == Top::Absent {
            unread.push(format!("the project has no {MANIFEST}"));
        }
        Self { targets, unread }
    }

    /// The file of the project the page of the source `page` is of: the part after
    /// `src/<crate>/` and before `.html`, under the directory of the crate's source,
    /// or why it cannot be told.
    pub(super) fn file_of(&self, page: &DocsPath) -> Result<RelPath, String> {
        let parts: Vec<&str> = page.parts().collect();
        let (krate, below) = match parts.as_slice() {
            [top, krate, below @ ..] if *top == SOURCE_DIR && !below.is_empty() => (*krate, below),
            _ => {
                return Err(format!(
                    "the docs link to no page of the source: {}",
                    joined(&parts)
                ));
            }
        };
        let dir = match self.targets.crate_named(krate) {
            CrateOf::One(target) => target.root_dir(),
            CrateOf::Two(one, two) => {
                return Err(format!(
                    "two packages of the project have a crate `{krate}`, under {} and {}",
                    shown(&one.root_dir()),
                    shown(&two.root_dir())
                ));
            }
            CrateOf::None if self.unread.is_empty() => {
                return Err(format!(
                    "the crate `{krate}` is no crate of the project's manifests (a dependency, or docs left by an earlier build)"
                ));
            }
            CrateOf::None => {
                return Err(format!(
                    "the crate `{krate}` is no crate of the manifests read, and some could not be: {}",
                    self.unread.join("; ")
                ));
            }
        };
        let mut file = dir;
        for (at, part) in below.iter().enumerate() {
            let part = match (at + 1 == below.len(), part.strip_suffix(".html")) {
                (true, Some(name)) => name,
                (true, None) => {
                    return Err(format!(
                        "the docs link to no page of the source: {}",
                        joined(&parts)
                    ));
                }
                (false, _) => part,
            };
            if part == UP {
                if file.pop().is_none() {
                    return Err(format!(
                        "the source of the crate `{krate}` leads out of the project: {}",
                        joined(&parts)
                    ));
                }
            } else {
                file.push(part.to_owned());
            }
        }
        file.join("/").parse().map_err(|e| {
            format!("the source of the crate `{krate}` is no path of the project: {e}")
        })
    }
}

/// The parts of a place in the docs, as a reason writes them.
fn joined(parts: &[&str]) -> String {
    parts.join("/")
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::super::super::targets::matches;
    use super::super::fixture::write;
    use super::*;

    fn page(text: &str) -> DocsPath {
        DocsPath::of_relative(Path::new(text)).unwrap()
    }

    fn file(text: &str) -> RelPath {
        text.parse().unwrap()
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
            crates.file_of(&page("src/my_demo/lib.rs.html")),
            Ok(file("src/lib.rs"))
        );
        assert_eq!(
            crates.file_of(&page("src/my_demo/a/b.rs.html")),
            Ok(file("src/a/b.rs"))
        );
        // A part `up` is rustdoc's `..`.
        assert_eq!(
            crates.file_of(&page("src/my_demo/up/build.rs.html")),
            Ok(file("build.rs"))
        );
        assert!(
            crates
                .file_of(&page("src/my_demo/up/up/x.rs.html"))
                .unwrap_err()
                .contains("out of the project")
        );
        assert!(
            crates
                .file_of(&page("src/serde/lib.rs.html"))
                .unwrap_err()
                .contains("no crate of the project")
        );
        assert!(crates.file_of(&page("my_demo/struct.A.html")).is_err());
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
            crates.file_of(&page("src/tool/main.rs.html")),
            Ok(file("cli/main.rs"))
        );
        // With a `src/lib.rs`, the library of the name is the crate.
        write(dir.path(), "src/lib.rs", "pub mod util;\n").await;
        let crates = crates_at(dir.path()).await;
        assert_eq!(
            crates.file_of(&page("src/tool/util.rs.html")),
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
        let of = |crates: &Crates, text: &str| crates.file_of(&page(text));
        // A `[[bin]]` declared, and still `src/main.rs` and `src/bin/*` by the layout.
        assert_eq!(
            of(&crates, "src/extra/extra.rs.html"),
            Ok(file("tools/extra.rs"))
        );
        assert_eq!(of(&crates, "src/kit/main.rs.html"), Ok(file("src/main.rs")));
        assert_eq!(
            of(&crates, "src/one/one.rs.html"),
            Ok(file("src/bin/one.rs"))
        );
        assert_eq!(
            of(&crates, "src/two/part.rs.html"),
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
            of(&crates, "src/one/extra.rs.html"),
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
            of(&crates, "src/extra/extra.rs.html"),
            Ok(file("tools/extra.rs"))
        );
        for gone in ["src/kit/main.rs.html", "src/one/one.rs.html"] {
            assert!(of(&crates, gone).is_err(), "{gone}");
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
            of(&crates, "src/two/part.rs.html"),
            Ok(file("src/bin/two/part.rs"))
        );
        assert!(of(&crates, "src/one/one.rs.html").is_err());
    }

    #[tokio::test]
    async fn of_the_edition_2015_a_declared_binary_turns_the_layout_off_unless_autobins_says() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        write(root, "src/main.rs", "fn main() {}\n").await;
        write(root, "src/bin/one.rs", "fn main() {}\n").await;
        write(root, "tools/extra.rs", "fn main() {}\n").await;
        let declared = "[[bin]]\nname = \"extra\"\npath = \"tools/extra.rs\"\n";
        let of = |crates: &Crates, text: &str| crates.file_of(&page(text));
        // No edition written is 2015: only what is declared.
        write(
            root,
            MANIFEST,
            &format!("[package]\nname = \"kit\"\n{declared}"),
        )
        .await;
        let crates = crates_at(root).await;
        assert_eq!(
            of(&crates, "src/extra/extra.rs.html"),
            Ok(file("tools/extra.rs"))
        );
        assert!(of(&crates, "src/kit/main.rs.html").is_err());
        assert!(of(&crates, "src/one/one.rs.html").is_err());
        // `autobins = true` finds them again; so does the edition 2018 and later.
        for package in [
            "[package]\nname = \"kit\"\nautobins = true\n",
            "[package]\nname = \"kit\"\nedition = \"2018\"\n",
        ] {
            write(root, MANIFEST, &format!("{package}{declared}")).await;
            let crates = crates_at(root).await;
            assert_eq!(
                of(&crates, "src/one/one.rs.html"),
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
        assert_eq!(of(&crates, "src/kit/main.rs.html"), Ok(file("src/main.rs")));
        // With no binary declared, the layout is read whatever the edition.
        write(root, MANIFEST, "[package]\nname = \"kit\"\n").await;
        let crates = crates_at(root).await;
        assert_eq!(
            of(&crates, "src/one/one.rs.html"),
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
        assert!(crates.file_of(&page("src/kit/lib.rs.html")).is_err());
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
        let of = |text: &str| crates.file_of(&page(text));
        assert_eq!(of("src/new/lib.rs.html"), Ok(file("crates/new/src/lib.rs")));
        assert_eq!(
            of("src/kept/lib.rs.html"),
            Ok(file("crates/old/kept/src/lib.rs"))
        );
        assert_eq!(of("src/yb/lib.rs.html"), Ok(file("vendor/y/b/src/lib.rs")));
        assert!(of("src/old/lib.rs.html").is_err());
        assert!(of("src/xa/lib.rs.html").is_err());
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
        let why = crates.file_of(&page("src/far/lib.rs.html")).unwrap_err();
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
            let why = crates.file_of(&page("src/near/lib.rs.html")).unwrap_err();
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
        let of = |text: &str| crates.file_of(&page(text));
        assert_eq!(
            of("src/my_core/app/x.rs.html"),
            Ok(file("crates/core/src/app/x.rs"))
        );
        assert_eq!(
            of("src/my/main.rs.html"),
            Ok(file("crates/cli/src/main.rs"))
        );
        assert_eq!(
            of("src/other/top.rs.html"),
            Ok(file("crates/lib/code/top.rs"))
        );
        assert_eq!(of("src/tool/lib.rs.html"), Ok(file("tool/src/lib.rs")));
        assert!(of("src/skip/lib.rs.html").is_err());
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
            crates
                .file_of(&page("src/same/lib.rs.html"))
                .unwrap_err()
                .contains("two packages")
        );
        assert!(
            crates
                .file_of(&page("src/elsewhere/lib.rs.html"))
                .unwrap_err()
                .contains("`**`")
        );
        // A member with `[` is not read either, and says so.
        assert!(
            crates
                .file_of(&page("src/elsewhere/lib.rs.html"))
                .unwrap_err()
                .contains("`[`")
        );
        // No manifest at the root: nothing is a crate of the project, and it says so.
        let bare = tempfile::tempdir().unwrap();
        let crates = crates_at(bare.path()).await;
        assert!(
            crates
                .file_of(&page("src/demo/lib.rs.html"))
                .unwrap_err()
                .contains("has no Cargo.toml")
        );
    }
}
