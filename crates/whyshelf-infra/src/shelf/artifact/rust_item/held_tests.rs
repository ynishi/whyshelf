//! The tests of the holding of the docs against the sources: the text of a page of the
//! source and of a file as compiled, and, through the space as a project asks it, what
//! the docs of a tree answer when they are of its sources and when they are not.
//!
//! The pages are written by hand in the layout rustdoc 1.95 writes, with words of this
//! repository only; the end-to-end tests of the command line run `cargo doc`.

use std::collections::BTreeSet;
use std::path::Path;

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
    let tree = DocsTree::of(&state, Side::After, at.clone())
        .await
        .unwrap()
        .unwrap();
    let sited = Sited::of(&state, Side::After, &at, &tree).await.unwrap();
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
        "{}/debug/build/demo-1/out/made.rs",
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
    let built = format!("{}/debug/build/demo-1/out/made.rs", whole_path(&resolved));
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
