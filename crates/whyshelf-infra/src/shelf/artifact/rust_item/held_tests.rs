//! The tests of the holding of the docs against the sources: the text of a page of the
//! source and of a file as compiled, and, through the space as a project asks it, what
//! the docs of a tree answer when they are of its sources and when they are not.
//!
//! The pages are written by hand in the layout rustdoc 1.95 writes, with words of this
//! repository only; the end-to-end tests of the command line run `cargo doc`.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use whyshelf_core::app::shelf::{DirListing, EntryKind, NotADir};
use whyshelf_core::domain::nonempty::{NonBlank, TextLiteral};

use whyshelf_core::domain::location::RelPath;
use whyshelf_core::domain::shelf::artifact::{
    ArtifactName, ArtifactState, Comparison, NotOfSourcesWhy, SpaceAsks, SpaceReadings, Untold,
};

use super::super::fixture::{index, source_page, whole, write};
use super::super::{RustItemSpace, TARGET_DIR_NAME};
use super::*;

#[test]
fn the_text_of_a_page_of_the_source_is_its_code_without_the_numbers_of_its_lines_and_tags() {
    let text = "//! A crate.\npub fn a() -> Vec<u8> { vec![1 & 2] }\n\"quoted\" 'c'\n";
    assert_eq!(page_text(&source_page(text)).as_deref(), Some(text));
    // No last line break is kept so.
    let bare = "pub fn a() {}";
    assert_eq!(page_text(&source_page(bare)).as_deref(), Some(bare));
    // Entities read back, a numeric one too; one that is none is kept as written.
    let page = source_page("x").replace(
        "<span class=\"kw\">x</span>",
        "&#39;&#x27;&quot;&lt;&amp;bogus;",
    );
    assert_eq!(page_text(&page).as_deref(), Some("''\"<&bogus;"));
    // A page with no code block holds no copy of a file.
    assert_eq!(
        page_text("<html><body><p>Redirecting</p></body></html>"),
        None
    );
}

#[test]
fn a_file_as_compiled_has_no_byte_order_mark_and_lf_for_crlf_and_keeps_its_last_line_break() {
    assert_eq!(as_compiled(b"\xEF\xBB\xBFfn a() {}\r\n"), b"fn a() {}\n");
    assert_eq!(as_compiled(b"a\r\nb\rc\n"), b"a\nb\rc\n");
    assert_eq!(as_compiled(b"no break"), b"no break");
    // A mark that is not at the start is a character of the file.
    assert_eq!(as_compiled(b"a\xEF\xBB\xBF"), b"a\xEF\xBB\xBF");
}

/// The text of `src/lib.rs` of the crate `demo` of [`project`].
const LIB: &str = "//! The demo crate.\npub struct Shelf;\n";

/// The names asked of the docs of [`project`].
const NAMES: [&str; 3] = ["demo", "demo::Shelf", "demo::Missing"];

fn n(text: &str) -> ArtifactName {
    ArtifactName::new(text).unwrap()
}

fn file(text: &str) -> RelPath {
    text.parse().unwrap()
}

/// The docs of the crate `name` under `docs`: its index and a struct `Shelf`.
async fn crate_docs(docs: &Path, name: &str) {
    write(
        docs,
        &format!("{name}/index.html"),
        &whole("<h2>Structs</h2>"),
    )
    .await;
    write(
        docs,
        &format!("{name}/struct.Shelf.html"),
        &whole("<pre class=\"rust item-decl\"><code>pub struct Shelf;</code></pre>"),
    )
    .await;
}

/// A package `demo` at `root` with its library [`LIB`], and its docs beside its
/// `Cargo.toml`, of these sources.
async fn project(root: &Path) {
    write(root, "Cargo.toml", "[package]\nname = \"demo\"\n").await;
    write(root, "src/lib.rs", LIB).await;
    let docs = docs_of(root);
    crate_docs(&docs, "demo").await;
    write(&docs, "src/demo/lib.rs.html", &source_page(LIB)).await;
}

/// The docs of the project at `root`, beside its `Cargo.toml`.
fn docs_of(root: &Path) -> std::path::PathBuf {
    root.join(TARGET_DIR_NAME).join("doc")
}

/// The readings of `names` under presence, from a space made now.
async fn read(root: &Path, names: &[&str]) -> SpaceReadings {
    let asks: SpaceAsks = names
        .iter()
        .map(|name| (n(name), BTreeSet::from([Comparison::Presence])))
        .collect();
    RustItemSpace::new(root.to_path_buf(), None)
        .resolve(&asks)
        .await
        .unwrap()
}

/// Why a reading is not of its sources, when it is that.
fn not_of(found: &SpaceReadings, name: &str) -> Option<NotOfSourcesWhy> {
    match found[&n(name)].state(&Comparison::Presence) {
        ArtifactState::Unknown(Untold::NotOfSources(not)) => Some(not.why().clone()),
        _ => None,
    }
}

/// Each name of `names` is not of its sources, for `why`.
fn all_not_of(found: &SpaceReadings, names: &[&str], why: &NotOfSourcesWhy) {
    for name in names {
        assert_eq!(
            not_of(found, name).as_ref(),
            Some(why),
            "{name}: {:?}",
            found[&n(name)]
        );
    }
}

fn is_there(found: &SpaceReadings, name: &str) -> bool {
    matches!(
        found[&n(name)].state(&Comparison::Presence),
        ArtifactState::Present(_)
    )
}

#[tokio::test]
async fn docs_of_their_sources_answer_as_before_and_a_file_edited_since_makes_every_name_not_of_them()
 {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    project(root).await;
    let found = read(root, &NAMES).await;
    assert!(is_there(&found, "demo") && is_there(&found, "demo::Shelf"));
    assert_eq!(
        found[&n("demo::Missing")].state(&Comparison::Presence),
        ArtifactState::Absent
    );
    // An item added to the source: the docs say nothing of it, there or absent.
    write(root, "src/lib.rs", &format!("{LIB}pub struct Added;\n")).await;
    let found = read(
        root,
        &["demo", "demo::Shelf", "demo::Missing", "demo::Added"],
    )
    .await;
    all_not_of(
        &found,
        &["demo", "demo::Shelf", "demo::Missing", "demo::Added"],
        &NotOfSourcesWhy::Differs {
            file: file("src/lib.rs"),
        },
    );
    // What it says: what was read, the file, and what to do.
    let ArtifactState::Unknown(untold) = found[&n("demo::Shelf")].state(&Comparison::Presence)
    else {
        panic!("not told")
    };
    let said = untold.to_string();
    assert!(
        said.contains(&format!("the built docs at {}", docs_of(root).display())),
        "{said}"
    );
    assert!(said.contains("src/lib.rs differs"), "{said}");
    assert!(said.contains("build the docs again"), "{said}");
}

#[tokio::test]
async fn a_page_of_a_file_that_is_gone_a_crate_with_no_page_and_one_two_packages_name_are_not_of_the_sources()
 {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    project(root).await;
    write(
        &docs_of(root),
        "src/demo/old.rs.html",
        &source_page("pub fn old() {}\n"),
    )
    .await;
    let found = read(root, &NAMES).await;
    all_not_of(
        &found,
        &NAMES,
        &NotOfSourcesWhy::Gone {
            file: file("src/old.rs"),
        },
    );

    // No page of the source at all.
    let bare = tempfile::tempdir().unwrap();
    project(bare.path()).await;
    tokio::fs::remove_dir_all(docs_of(bare.path()).join("src"))
        .await
        .unwrap();
    all_not_of(
        &read(bare.path(), &NAMES).await,
        &NAMES,
        &NotOfSourcesWhy::NoTrace,
    );

    // Two packages of one crate name.
    let two = tempfile::tempdir().unwrap();
    let r = two.path();
    write(r, "Cargo.toml", "[workspace]\nmembers = [\"a\", \"b\"]\n").await;
    for package in ["a", "b"] {
        write(
            r,
            &format!("{package}/Cargo.toml"),
            "[package]\nname = \"same\"\n",
        )
        .await;
        write(r, &format!("{package}/src/lib.rs"), LIB).await;
    }
    crate_docs(&docs_of(r), "same").await;
    write(&docs_of(r), "src/same/lib.rs.html", &source_page(LIB)).await;
    all_not_of(
        &read(r, &["same::Shelf"]).await,
        &["same::Shelf"],
        &NotOfSourcesWhy::NoTrace,
    );
}

#[tokio::test]
async fn the_docs_of_a_dependency_are_not_of_the_sources_and_the_crates_of_the_project_still_answer()
 {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    project(root).await;
    crate_docs(&docs_of(root), "serde").await;
    let found = read(root, &["serde", "serde::Shelf", "demo::Shelf"]).await;
    all_not_of(
        &found,
        &["serde", "serde::Shelf"],
        &NotOfSourcesWhy::NoTrace,
    );
    assert!(is_there(&found, "demo::Shelf"));
}

#[tokio::test]
async fn a_manifest_that_cannot_be_read_and_a_bound_past_make_the_docs_unread() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    project(root).await;
    write(root, "Cargo.toml", "[package\n").await;
    let found = read(root, &NAMES).await;
    for name in NAMES {
        assert!(
            matches!(not_of(&found, name), Some(NotOfSourcesWhy::Unread(_))),
            "{name}"
        );
    }

    // Past the bound of pages of the holding.
    let bound = tempfile::tempdir().unwrap();
    let r = bound.path();
    project(r).await;
    write(r, "src/more.rs", "pub fn more() {}\n").await;
    write(
        &docs_of(r),
        "src/demo/more.rs.html",
        &source_page("pub fn more() {}\n"),
    )
    .await;
    let limits = HoldLimits {
        pages: 1,
        ..HoldLimits::default()
    };
    let not = held_with(r, &limits).await.unwrap_err();
    assert!(
        matches!(not.why(), NotOfSourcesWhy::Unread(why) if why.as_str().contains("more than 1 pages")),
        "{not}"
    );
    assert!(held_with(r, &HoldLimits::default()).await.is_ok());
}

/// The holding of the docs of the project at `root`, beside its `Cargo.toml`, within
/// `limits`, as the space makes it.
async fn held_with(root: &Path, limits: &HoldLimits) -> Result<HeldDocs, NotOfSources> {
    let state = ProjectTree::files(super::super::super::super::state::files::FileStates::new(
        root.to_path_buf(),
    ));
    let docs = docs_of(root);
    let at = super::super::docs_tree::placed(&state, Side::After, docs)
        .await
        .unwrap();
    let tree = DocsTree::of(&state, Side::After, at)
        .await
        .unwrap()
        .unwrap();
    let sited = Sited::of(&state, Side::After, &tree).await.unwrap();
    let crates = Crates::of(&state, Side::After).await;
    hold(&state, Side::After, tree, crates, sited, limits).await
}

#[tokio::test]
async fn a_name_through_a_reexport_is_not_of_the_sources_when_only_the_crate_it_leads_to_differs() {
    let dir = tempfile::tempdir().unwrap();
    let r = dir.path();
    write(r, "Cargo.toml", "[workspace]\nmembers = [\"a\", \"b\"]\n").await;
    for package in ["a", "b"] {
        write(
            r,
            &format!("{package}/Cargo.toml"),
            &format!("[package]\nname = \"{package}\"\n"),
        )
        .await;
        write(r, &format!("{package}/src/lib.rs"), LIB).await;
        write(
            &docs_of(r),
            &format!("src/{package}/lib.rs.html"),
            &source_page(LIB),
        )
        .await;
    }
    crate_docs(&docs_of(r), "b").await;
    write(
        &docs_of(r),
        "a/index.html",
        &index(&[("Shelf", Some("../b/struct.Shelf.html"))]),
    )
    .await;
    let found = read(r, &["a::Shelf"]).await;
    assert!(is_there(&found, "a::Shelf"));
    // Only `b` is edited, and `a` is told through it as much as `b` is.
    write(r, "b/src/lib.rs", &format!("{LIB}pub struct More;\n")).await;
    let found = read(r, &["a::Shelf", "b::Shelf"]).await;
    all_not_of(
        &found,
        &["a::Shelf", "b::Shelf"],
        &NotOfSourcesWhy::Differs {
            file: file("b/src/lib.rs"),
        },
    );
}

#[tokio::test]
async fn a_file_with_a_byte_order_mark_and_crlf_is_of_its_page_and_one_that_lost_its_last_line_break_is_not()
 {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    project(root).await;
    write(
        root,
        "src/lib.rs",
        "\u{feff}//! The demo crate.\r\npub struct Shelf;\r\n",
    )
    .await;
    assert!(is_there(&read(root, &["demo::Shelf"]).await, "demo::Shelf"));
    write(root, "src/lib.rs", LIB.trim_end_matches('\n')).await;
    all_not_of(
        &read(root, &["demo::Shelf"]).await,
        &["demo::Shelf"],
        &NotOfSourcesWhy::Differs {
            file: file("src/lib.rs"),
        },
    );
}

/// The parts of `path`, an absolute path, joined as a page of the source names them.
fn whole_path(path: &Path) -> String {
    path.display()
        .to_string()
        .trim_start_matches('/')
        .to_owned()
}

#[tokio::test]
async fn a_page_by_the_whole_path_of_the_build_directory_is_not_compared_and_one_of_the_root_is() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    project(root).await;
    // The build directory named by the cargo settings: the docs move with it.
    write(
        root,
        ".cargo/config.toml",
        "[build]\ntarget-dir = \"out\"\n",
    )
    .await;
    let docs = root.join("out/doc");
    crate_docs(&docs, "demo").await;
    write(&docs, "src/demo/lib.rs.html", &source_page(LIB)).await;
    let built = format!(
        "{}/debug/build/demo-0123456789abcdef/out/made.rs",
        whole_path(&root.join("out"))
    );
    write(
        &docs,
        &format!("src/demo/{built}.html"),
        &source_page("no file's text\n"),
    )
    .await;
    assert!(is_there(&read(root, &["demo::Shelf"]).await, "demo::Shelf"));
    // A page by the whole path of the root is of the file under it.
    let of_root = format!("{}/src/lib.rs", whole_path(root));
    write(
        &docs,
        &format!("src/demo/{of_root}.html"),
        &source_page("another text\n"),
    )
    .await;
    all_not_of(
        &read(root, &["demo::Shelf"]).await,
        &["demo::Shelf"],
        &NotOfSourcesWhy::Differs {
            file: file("src/lib.rs"),
        },
    );
}

#[tokio::test]
async fn a_page_that_climbs_past_the_root_has_no_trace_and_one_of_a_tree_moved_elsewhere_is_gone() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    project(root).await;
    write(
        &docs_of(root),
        "src/demo/up/up/x.rs.html",
        &source_page("x\n"),
    )
    .await;
    all_not_of(
        &read(root, &["demo::Shelf"]).await,
        &["demo::Shelf"],
        &NotOfSourcesWhy::NoTrace,
    );

    // The whole path of a build directory elsewhere: the tree was moved with its build.
    let moved = tempfile::tempdir().unwrap();
    let r = moved.path();
    project(r).await;
    write(
        &docs_of(r),
        "src/demo/elsewhere/target/debug/out/made.rs.html",
        &source_page("x\n"),
    )
    .await;
    all_not_of(
        &read(r, &["demo::Shelf"]).await,
        &["demo::Shelf"],
        &NotOfSourcesWhy::Gone {
            file: file("src/elsewhere/target/debug/out/made.rs"),
        },
    );
}

#[tokio::test]
async fn a_file_outside_the_directory_of_the_root_file_of_a_member_is_found_in_the_package_or_the_workspace()
 {
    let dir = tempfile::tempdir().unwrap();
    let r = dir.path();
    write(r, "Cargo.toml", "[workspace]\nmembers = [\"crates/m\"]\n").await;
    write(r, "crates/m/Cargo.toml", "[package]\nname = \"m\"\n").await;
    let files = [
        ("crates/m/src/lib.rs", "lib.rs", "//! m\n"),
        ("crates/m/src/inner/deep.rs", "inner/deep.rs", "//! deep\n"),
        ("crates/m/outside/near.rs", "outside/near.rs", "//! near\n"),
        ("shared/wide.rs", "shared/wide.rs", "//! wide\n"),
    ];
    let docs = docs_of(r);
    crate_docs(&docs, "m").await;
    for (at, page, text) in files {
        write(r, at, text).await;
        write(&docs, &format!("src/m/{page}.html"), &source_page(text)).await;
    }
    assert!(is_there(&read(r, &["m::Shelf"]).await, "m::Shelf"));
    // Each is held against the file it is of.
    write(r, "shared/wide.rs", "//! wider\n").await;
    all_not_of(
        &read(r, &["m::Shelf"]).await,
        &["m::Shelf"],
        &NotOfSourcesWhy::Differs {
            file: file("shared/wide.rs"),
        },
    );
    write(r, "shared/wide.rs", "//! wide\n").await;
    write(r, "crates/m/outside/near.rs", "//! nearer\n").await;
    all_not_of(
        &read(r, &["m::Shelf"]).await,
        &["m::Shelf"],
        &NotOfSourcesWhy::Differs {
            file: file("crates/m/outside/near.rs"),
        },
    );
}

#[cfg(unix)]
#[tokio::test]
async fn a_page_by_the_resolved_path_of_a_build_directory_reached_through_a_link_is_not_compared() {
    let real = tempfile::tempdir().unwrap();
    let links = tempfile::tempdir().unwrap();
    let root = links.path().join("linked");
    tokio::fs::symlink(real.path(), &root).await.unwrap();
    project(&root).await;
    let resolved = tokio::fs::canonicalize(root.join(TARGET_DIR_NAME))
        .await
        .unwrap();
    assert_ne!(resolved, root.join(TARGET_DIR_NAME));
    let built = format!(
        "{}/debug/build/demo-0123456789abcdef/out/made.rs",
        whole_path(&resolved)
    );
    write(
        &docs_of(&root),
        &format!("src/demo/{built}.html"),
        &source_page("no file's\n"),
    )
    .await;
    assert!(is_there(
        &read(&root, &["demo::Shelf"]).await,
        "demo::Shelf"
    ));
}

#[tokio::test]
async fn the_docs_of_a_side_are_held_once_for_a_space() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    project(root).await;
    let space = RustItemSpace::new(root.to_path_buf(), None);
    let shelf = n("demo::Shelf");
    assert!(space.current(&shelf).await.unwrap().is_some());
    // Edited after the first holding: the same space holds what it held (a run writes
    // nothing), and one made again holds the docs anew.
    write(root, "src/lib.rs", "//! Edited.\n").await;
    assert!(space.current(&n("demo")).await.unwrap().is_some());
    let again = RustItemSpace::new(root.to_path_buf(), None);
    assert_eq!(again.current(&shelf).await.unwrap(), None);
}

/// The reason of the reading of `name`, as a person reads it.
fn said(found: &SpaceReadings, name: &str) -> String {
    match found[&n(name)].state(&Comparison::Presence) {
        ArtifactState::Unknown(untold) => untold.to_string(),
        other => panic!("{name} is told: {other:?}"),
    }
}

#[test]
fn a_listing_of_the_docs_is_not_there_only_when_nothing_is_and_one_cut_or_failing_is_unread() {
    use whyshelf_core::app::shelf::Entries;
    let shown = Path::new("/t/doc/src");
    let entries = || {
        let mut entries = Entries::default();
        entries.named.insert(
            DocsPath::top().join("lib.rs.html").unwrap(),
            EntryKind::File,
        );
        entries
    };
    assert_eq!(
        of_listing(shown, Ok(DirListing::All(entries()))),
        Listed::Entries(BTreeMap::from([(
            "lib.rs.html".to_owned(),
            EntryKind::File
        )]))
    );
    assert_eq!(
        of_listing(shown, Ok(DirListing::NotADir(NotADir::Absent))),
        Listed::Absent
    );
    // Cut at a bound: not read as the entries it has.
    let cut = of_listing(
        shown,
        Ok(DirListing::Cut {
            entries: entries(),
            why: NonBlank::from(TextLiteral::new("past the bound")),
        }),
    );
    assert!(
        matches!(&cut, Listed::Unread(why) if why.contains("was cut: past the bound")),
        "{cut:?}"
    );
    assert!(matches!(
        of_listing(shown, Ok(DirListing::NotADir(NotADir::File))),
        Listed::NotADir(_)
    ));
    let failed = of_listing(
        shown,
        Err(StateError::Io {
            path: shown.to_path_buf(),
            source: std::io::Error::from(std::io::ErrorKind::PermissionDenied),
        }),
    );
    assert!(
        matches!(&failed, Listed::Unread(why) if why.contains("cannot be listed")),
        "{failed:?}"
    );
}

#[cfg(unix)]
#[tokio::test]
async fn docs_that_cannot_be_listed_are_unread_and_no_crate_of_the_project_is_told_as_another() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    project(root).await;
    let locked = |at: std::path::PathBuf, mode: u32| async move {
        tokio::fs::set_permissions(&at, std::fs::Permissions::from_mode(mode))
            .await
            .unwrap();
    };
    // The pages of the source cannot be listed: unread, not "no trace".
    let src = docs_of(root).join("src");
    locked(src.clone(), 0o000).await;
    // Run as a user every directory opens to (root), it is no case to test.
    let refused = tokio::fs::read_dir(&src).await.is_err();
    let found = read(root, &NAMES).await;
    locked(src, 0o755).await;
    if !refused {
        return;
    }
    for name in NAMES {
        assert!(
            matches!(not_of(&found, name), Some(NotOfSourcesWhy::Unread(_))),
            "{name}: {:?}",
            found[&n(name)]
        );
        assert!(
            !said(&found, name).contains("no crate of the manifests"),
            "{name}"
        );
    }
    // The docs themselves cannot be listed: unread, not docs that are not built.
    let docs = docs_of(root);
    locked(docs.clone(), 0o000).await;
    let found = read(root, &NAMES).await;
    locked(docs, 0o755).await;
    for name in NAMES {
        assert!(
            matches!(not_of(&found, name), Some(NotOfSourcesWhy::Unread(_))),
            "{name}: {:?}",
            found[&n(name)]
        );
    }
}

#[cfg(unix)]
#[tokio::test]
async fn a_symbolic_link_at_the_top_of_the_docs_is_no_crate_and_is_passed_over() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    project(root).await;
    let outside = tempfile::tempdir().unwrap();
    write(outside.path(), "far/index.html", &whole("")).await;
    let docs = docs_of(root);
    // A link to a file of the root, and one to a directory out of the root.
    tokio::fs::symlink(root.join("src/lib.rs"), docs.join("linked.html"))
        .await
        .unwrap();
    tokio::fs::symlink(outside.path().join("far"), docs.join("far"))
        .await
        .unwrap();
    let found = read(root, &["demo::Shelf"]).await;
    assert!(is_there(&found, "demo::Shelf"), "{found:?}");
}

#[tokio::test]
async fn with_the_build_directory_at_the_root_a_page_of_a_file_of_the_root_is_compared_and_one_a_build_script_made_is_not()
 {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    write(root, "Cargo.toml", "[package]\nname = \"demo\"\n").await;
    write(root, "src/lib.rs", LIB).await;
    write(root, ".cargo/config.toml", "[build]\ntarget-dir = \".\"\n").await;
    let docs = root.join("doc");
    crate_docs(&docs, "demo").await;
    write(&docs, "src/demo/lib.rs.html", &source_page(LIB)).await;
    let built = format!(
        "{}/debug/build/demo-0123456789abcdef/out/made.rs",
        whole_path(root)
    );
    write(
        &docs,
        &format!("src/demo/{built}.html"),
        &source_page("no file's text\n"),
    )
    .await;
    assert!(is_there(&read(root, &["demo::Shelf"]).await, "demo::Shelf"));
    // A file of the project on a path of the same words, with no package directory of
    // cargo's form, is compared: it is no output of a build script.
    write(root, "x/build/y/out/z.rs", "pub fn z() {}\n").await;
    let alike = format!("{}/x/build/y/out/z.rs", whole_path(root));
    write(
        &docs,
        &format!("src/demo/{alike}.html"),
        &source_page("pub fn old() {}\n"),
    )
    .await;
    all_not_of(
        &read(root, &["demo::Shelf"]).await,
        &["demo::Shelf"],
        &NotOfSourcesWhy::Differs {
            file: file("x/build/y/out/z.rs"),
        },
    );
    tokio::fs::remove_file(docs.join(format!("src/demo/{alike}.html")))
        .await
        .unwrap();
    // A file of the root a crate reads by its whole path is compared, the build
    // directory being the root.
    write(root, "src/abs.rs", "pub fn abs() {}\n").await;
    let abs = format!("{}/src/abs.rs", whole_path(root));
    write(
        &docs,
        &format!("src/demo/{abs}.html"),
        &source_page("pub fn old() {}\n"),
    )
    .await;
    all_not_of(
        &read(root, &["demo::Shelf"]).await,
        &["demo::Shelf"],
        &NotOfSourcesWhy::Differs {
            file: file("src/abs.rs"),
        },
    );
}

#[tokio::test]
async fn a_crate_in_a_directory_at_the_top_of_a_project_with_no_manifest_at_its_root_is_held_and_one_deeper_is_none_of_its_crates()
 {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    write(root, "crate/Cargo.toml", "[package]\nname = \"demo\"\n").await;
    write(root, "crate/src/lib.rs", LIB).await;
    let docs = root.join("build/doc");
    crate_docs(&docs, "demo").await;
    write(&docs, "src/demo/lib.rs.html", &source_page(LIB)).await;
    let space = || RustItemSpace::new(root.to_path_buf(), Some(root.join("build")));
    let asks: SpaceAsks = ["demo::Shelf"]
        .iter()
        .map(|name| (n(name), BTreeSet::from([Comparison::Presence])))
        .collect();
    let found = space().resolve(&asks).await.unwrap();
    assert!(is_there(&found, "demo::Shelf"), "{found:?}");
    write(root, "crate/src/lib.rs", "//! Edited.\n").await;
    let found = space().resolve(&asks).await.unwrap();
    all_not_of(
        &found,
        &["demo::Shelf"],
        &NotOfSourcesWhy::Differs {
            file: file("crate/src/lib.rs"),
        },
    );

    // A crate two directories down is none of the manifests read, and is not told as
    // a dependency.
    let deeper = tempfile::tempdir().unwrap();
    let r = deeper.path();
    write(r, "rust/demo/Cargo.toml", "[package]\nname = \"demo\"\n").await;
    write(r, "rust/demo/src/lib.rs", LIB).await;
    let docs = r.join("build/doc");
    crate_docs(&docs, "demo").await;
    write(&docs, "src/demo/lib.rs.html", &source_page(LIB)).await;
    let found = RustItemSpace::new(r.to_path_buf(), Some(r.join("build")))
        .resolve(&asks)
        .await
        .unwrap();
    let why = said(&found, "demo::Shelf");
    assert!(
        why.contains("no Cargo.toml at its root, nor in a directory at its top"),
        "{why}"
    );
    assert!(!why.contains("dependency"), "{why}");
}

#[tokio::test]
async fn the_index_of_a_crate_that_was_not_held_is_not_read() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    project(root).await;
    // An index that is no page at all (a directory): reading it would say so.
    tokio::fs::create_dir_all(docs_of(root).join("other/index.html"))
        .await
        .unwrap();
    let found = read(root, &["other::Shelf"]).await;
    assert_eq!(
        not_of(&found, "other::Shelf"),
        Some(NotOfSourcesWhy::NoTrace)
    );
    let why = said(&found, "other::Shelf");
    assert!(!why.contains("cannot be read"), "{why}");
    assert!(!why.contains("dependency"), "{why}");
}

#[tokio::test]
async fn docs_of_a_crate_that_depends_by_a_path_on_one_of_the_project_whose_source_they_do_not_hold_have_no_trace()
 {
    let dir = tempfile::tempdir().unwrap();
    let r = dir.path();
    write(
        r,
        "Cargo.toml",
        "[workspace]\nmembers = [\"a\", \"b\", \"c\"]\n[workspace.dependencies]\nc = { path = \"c\" }\n",
    )
    .await;
    write(
        r,
        "a/Cargo.toml",
        "[package]\nname = \"a\"\n[dependencies]\nb = { path = \"../b\" }\n",
    )
    .await;
    write(
        r,
        "b/Cargo.toml",
        "[package]\nname = \"b\"\n[dependencies]\nc.workspace = true\n",
    )
    .await;
    write(r, "c/Cargo.toml", "[package]\nname = \"c\"\n").await;
    for package in ["a", "b", "c"] {
        write(r, &format!("{package}/src/lib.rs"), LIB).await;
    }
    // Docs of `a` alone (`cargo doc -p a`): what it re-exports of `b` is in its docs,
    // and no page of `b`'s source.
    let docs = docs_of(r);
    crate_docs(&docs, "a").await;
    write(&docs, "src/a/lib.rs.html", &source_page(LIB)).await;
    let found = read(r, &["a::Shelf"]).await;
    all_not_of(&found, &["a::Shelf"], &NotOfSourcesWhy::NoTrace);
    let why = said(&found, "a::Shelf");
    assert!(
        why.contains(
            "the docs of `a` hold no page of the source of `b`, which it depends on by a path: build the docs of both"
        ),
        "{why}"
    );
    // With `b`'s pages, `b` depends on `c` (through the dependencies of the root) and
    // the docs hold no page of `c`: still no trace, now of `c`.
    write(&docs, "src/b/lib.rs.html", &source_page(LIB)).await;
    let found = read(r, &["a::Shelf"]).await;
    all_not_of(&found, &["a::Shelf"], &NotOfSourcesWhy::NoTrace);
    let why = said(&found, "a::Shelf");
    assert!(
        why.contains("the docs of `b` hold no page of the source of `c`"),
        "{why}"
    );
    // With `c`'s pages too, all are held.
    write(&docs, "src/c/lib.rs.html", &source_page(LIB)).await;
    assert!(is_there(&read(r, &["a::Shelf"]).await, "a::Shelf"));
}
