//! Tests of [`GitStates`]: what it answers alike with a directory read by
//! [`FileStates`](super::super::files::FileStates) (the same commit checked out),
//! what may differ, the attributes of each side, the index, the bound of a listing,
//! what stops it from opening, and the spaces read on it.
//!
//! Each test makes its own repository of a few files in a temporary directory.

use std::path::{Path, PathBuf};
use std::process::Stdio;

use tokio::process::Command;
use whyshelf_core::app::shelf::{SpaceAdapter, StateList, StateRead};
use whyshelf_core::domain::shelf::artifact::{
    ArtifactName, ArtifactReading, ArtifactState, Comparison, NotOfSourcesWhy, SpaceAsks,
};

use super::super::super::OpenError;
use super::super::super::artifact::file::FileSpace;
use super::super::super::artifact::js_test::JsTestSpace;
use super::super::super::artifact::rust_item::RustItemSpace;
use super::super::super::artifact::rust_test::RustTestSpace;
use super::super::files::{FileStates, LEAVES};
use super::super::{GitAfter, ProjectTree};
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

/// Git run on `dir` with no `GIT_*` variable of the caller, and whether it succeeded.
async fn git_may(dir: &Path, args: &[&str]) -> (bool, String) {
    let mut command = Command::new("git");
    for (name, _) in std::env::vars_os() {
        if name.to_string_lossy().starts_with("GIT_") {
            command.env_remove(&name);
        }
    }
    let out = command
        .arg("-C")
        .arg(dir)
        .args(["-c", "user.name=t", "-c", "user.email=t@example.com"])
        .args(["-c", "init.defaultBranch=main", "-c", "core.autocrlf=false"])
        .args(args)
        .output()
        .await
        .unwrap();
    (
        out.status.success(),
        String::from_utf8_lossy(&out.stdout).trim().to_owned(),
    )
}

async fn git(dir: &Path, args: &[&str]) -> String {
    let (ok, out) = git_may(dir, args).await;
    assert!(ok, "git {args:?}");
    out
}

async fn write(dir: &Path, at: &str, text: &str) {
    let full = dir.join(at);
    tokio::fs::create_dir_all(full.parent().unwrap())
        .await
        .unwrap();
    tokio::fs::write(full, text).await.unwrap();
}

fn rel(text: &str) -> RelPath {
    text.parse().unwrap()
}

/// A repository `repo`, and a directory `outside` beside it, with one commit: two
/// files, a link inside, a link out through `..`, a link that holds an absolute path.
struct Repo {
    _dir: tempfile::TempDir,
    repo: PathBuf,
}

async fn repo() -> Repo {
    let dir = tempfile::tempdir().unwrap();
    let repo = dir.path().join("repo");
    tokio::fs::create_dir_all(dir.path().join("outside"))
        .await
        .unwrap();
    write(&repo, "a.txt", "one\n").await;
    write(&repo, "d/b.txt", "two\n").await;
    git(&repo, &["init", "-q"]).await;
    #[cfg(unix)]
    {
        tokio::fs::symlink("d/b.txt", repo.join("l")).await.unwrap();
        tokio::fs::symlink("../outside", repo.join("up"))
            .await
            .unwrap();
        tokio::fs::symlink(repo.join("a.txt"), repo.join("abs"))
            .await
            .unwrap();
    }
    git(&repo, &["add", "-A"]).await;
    git(&repo, &["commit", "-q", "-m", "one"]).await;
    Repo { _dir: dir, repo }
}

async fn open(repo: &Path, since: &str, until: GitAfter) -> GitStates {
    GitStates::open(repo.to_path_buf(), since, &until)
        .await
        .unwrap()
}

fn head() -> GitAfter {
    GitAfter::Commit("HEAD".to_owned())
}

/// An answer, with an error told by its kind alone: what two adapters are compared by.
fn shown<C: std::fmt::Debug>(answer: Result<AtSide<RelPath, C>, StateError>) -> String {
    match answer {
        Ok(at) => format!("{at:?}"),
        Err(StateError::Io { source, .. }) => format!("io {:?}", source.kind()),
        Err(other) => format!("error {other}"),
    }
}

/// The names both adapters answer alike: a file, a file in a directory, a directory,
/// a link inside, a link out of the tree, nothing there, and a path through a file.
const ALIKE: [&str; 8] = [
    "a.txt", "d/b.txt", "d", "l", "l/x", "up/x", "missing", "a.txt/x",
];

#[tokio::test]
async fn a_commit_and_its_checkout_answer_alike() {
    if !git_installed().await || cfg!(not(unix)) {
        return;
    }
    let r = repo().await;
    let git = open(&r.repo, "HEAD", head()).await;
    let files = FileStates::between(r.repo.clone(), r.repo.clone());
    let names: BTreeSet<RelPath> = ALIKE.iter().map(|n| rel(n)).collect();
    for side in [Side::Before, Side::After] {
        let mut read_git = git.read(side, &names, ByteBound::WHOLE).await.unwrap();
        let mut read_files = files.read(side, &names, ByteBound::WHOLE).await.unwrap();
        let mut look_git = git.look(side, &names).await.unwrap();
        let mut look_files = files.look(side, &names).await.unwrap();
        for name in &names {
            assert_eq!(
                shown(read_git.remove(name).unwrap()),
                shown(read_files.remove(name).unwrap()),
                "read {name}"
            );
            assert_eq!(
                shown(look_git.remove(name).unwrap()),
                shown(look_files.remove(name).unwrap()),
                "look {name}"
            );
        }
        for dir in [None, Some(rel("d")), Some(rel("l")), Some(rel("a.txt"))] {
            // The directory of a checkout holds the repository, which is no part of
            // the tree of a commit.
            let mut checkout = files.list(side, dir.as_ref()).await.unwrap();
            if let DirListing::All(entries) = &mut checkout {
                entries.named.remove(&rel(".git"));
            }
            assert_eq!(
                git.list(side, dir.as_ref()).await.unwrap(),
                checkout,
                "list {dir:?}"
            );
        }
        assert_eq!(
            git.files(side, ByteBound::WHOLE).await.unwrap(),
            files.files(side, ByteBound::WHOLE).await.unwrap()
        );
    }
}

#[tokio::test]
async fn what_a_commit_and_a_directory_may_answer_otherwise() {
    if !git_installed().await || cfg!(not(unix)) {
        return;
    }
    let r = repo().await;
    // A submodule, a file a filter driver names, then a file not tracked.
    let info = format!("160000,{},sub", git(&r.repo, &["rev-parse", "HEAD"]).await);
    git(&r.repo, &["update-index", "--add", "--cacheinfo", &info]).await;
    write(&r.repo, ".gitattributes", "f.txt filter=upper\n").await;
    write(&r.repo, "f.txt", "kept\n").await;
    git(&r.repo, &["add", ".gitattributes", "f.txt"]).await;
    git(&r.repo, &["commit", "-q", "-m", "two"]).await;
    git(&r.repo, &["config", "filter.upper.smudge", "tr a-z A-Z"]).await;
    write(&r.repo, "u.txt", "new\n").await;
    let git_states = open(&r.repo, "HEAD", head()).await;
    let files = FileStates::between(r.repo.clone(), r.repo.clone());
    let one = |names: &[&str]| names.iter().map(|n| rel(n)).collect::<BTreeSet<_>>();

    let read = git_states
        .read(
            Side::After,
            &one(&["u.txt", "sub", "f.txt", "abs"]),
            ByteBound::WHOLE,
        )
        .await
        .unwrap();
    // Not tracked: not in the commit.
    assert_eq!(read[&rel("u.txt")].as_ref().unwrap(), &AtSide::Absent);
    assert_eq!(
        read[&rel("sub")].as_ref().unwrap(),
        &AtSide::Untold(said(SUBMODULE))
    );
    // The driver is not run: the content as a checkout that runs none writes it.
    assert_eq!(
        read[&rel("f.txt")].as_ref().unwrap(),
        &AtSide::There(b"kept\n".to_vec())
    );
    // A link that holds an absolute path leads out of a commit, wherever it points.
    assert!(matches!(
        read[&rel("abs")].as_ref().unwrap(),
        AtSide::Elsewhere {
            leads: Leads::Outside,
            ..
        }
    ));
    let read = files
        .read(Side::After, &one(&["u.txt", "abs"]), ByteBound::WHOLE)
        .await
        .unwrap();
    assert_eq!(
        read[&rel("u.txt")].as_ref().unwrap(),
        &AtSide::There(b"new\n".to_vec())
    );
    assert!(matches!(
        read[&rel("abs")].as_ref().unwrap(),
        AtSide::Elsewhere {
            leads: Leads::Inside(_),
            ..
        }
    ));
}

#[tokio::test]
async fn each_commit_converts_by_its_own_attributes() {
    if !git_installed().await {
        return;
    }
    let r = repo().await;
    let before = git(&r.repo, &["rev-parse", "HEAD"]).await;
    write(&r.repo, ".gitattributes", "a.txt text eol=crlf\n").await;
    git(&r.repo, &["add", ".gitattributes"]).await;
    git(&r.repo, &["commit", "-q", "-m", "crlf"]).await;
    let states = open(&r.repo, &before, head()).await;
    let names = BTreeSet::from([rel("a.txt"), rel("d/b.txt")]);
    let at = |side| {
        let states = states.clone();
        let names = names.clone();
        async move {
            let mut read = states.read(side, &names, ByteBound::WHOLE).await.unwrap();
            let mut look = states.look(side, &names).await.unwrap();
            (
                read.remove(&rel("a.txt")).unwrap().unwrap(),
                look.remove(&rel("a.txt")).unwrap().unwrap(),
                read.remove(&rel("d/b.txt")).unwrap().unwrap(),
            )
        }
    };
    assert_eq!(
        at(Side::Before).await,
        (
            AtSide::There(b"one\n".to_vec()),
            AtSide::There(4),
            AtSide::There(b"two\n".to_vec())
        )
    );
    // The side after converts by the attributes it holds; the reading of the next
    // blob is not thrown by a content longer than the blob.
    assert_eq!(
        at(Side::After).await,
        (
            AtSide::There(b"one\r\n".to_vec()),
            AtSide::There(5),
            AtSide::There(b"two\n".to_vec())
        )
    );
}

#[tokio::test]
async fn a_bound_of_bytes_is_held_to_and_one_byte_past_it_is_read() {
    if !git_installed().await {
        return;
    }
    let r = repo().await;
    let states = open(&r.repo, "HEAD", head()).await;
    let read = states
        .read(
            Side::After,
            &BTreeSet::from([rel("a.txt")]),
            ByteBound::at_most(2),
        )
        .await
        .unwrap();
    assert_eq!(
        read[&rel("a.txt")].as_ref().unwrap(),
        &AtSide::There(b"one".to_vec())
    );
}

#[tokio::test]
async fn the_index_is_read_when_its_attributes_are_heads_and_not_opened_otherwise() {
    if !git_installed().await {
        return;
    }
    let r = repo().await;
    write(&r.repo, "a.txt", "staged\n").await;
    git(&r.repo, &["add", "a.txt"]).await;
    write(&r.repo, "a.txt", "not staged\n").await;
    let states = open(&r.repo, "HEAD", GitAfter::Index).await;
    let read = states
        .read(
            Side::After,
            &BTreeSet::from([rel("a.txt")]),
            ByteBound::WHOLE,
        )
        .await
        .unwrap();
    assert_eq!(
        read[&rel("a.txt")].as_ref().unwrap(),
        &AtSide::There(b"staged\n".to_vec())
    );
    assert_eq!(
        states.path_of(Side::After, &rel("a.txt")),
        PathBuf::from(":a.txt")
    );
    assert_eq!(
        states.path_of(Side::Before, &rel("a.txt")),
        PathBuf::from("HEAD:a.txt")
    );
    // The index changes the attributes: it cannot be read as a checkout of it.
    write(&r.repo, "d/.gitattributes", "*.txt text\n").await;
    git(&r.repo, &["add", "d/.gitattributes"]).await;
    let opened = GitStates::open(r.repo.clone(), "HEAD", &GitAfter::Index).await;
    assert!(
        matches!(opened, Err(OpenError::IndexAttributes)),
        "{opened:?}"
    );
}

#[tokio::test]
async fn a_name_below_a_root_in_a_subdirectory_is_written_from_the_top_of_the_work_tree() {
    if !git_installed().await {
        return;
    }
    let r = repo().await;
    let sub = r.repo.join("d");
    let states = open(&sub, "HEAD", head()).await;
    assert_eq!(
        states.path_of(Side::Before, &rel("b.txt")),
        PathBuf::from("HEAD:d/b.txt")
    );
    let read = states
        .read(
            Side::After,
            &BTreeSet::from([rel("b.txt")]),
            ByteBound::WHOLE,
        )
        .await
        .unwrap();
    assert_eq!(
        read[&rel("b.txt")].as_ref().unwrap(),
        &AtSide::There(b"two\n".to_vec())
    );
    let states = open(&sub, "HEAD", GitAfter::Index).await;
    assert_eq!(
        states.path_of(Side::After, &rel("b.txt")),
        PathBuf::from(":d/b.txt")
    );
}

/// A link whose text is past `LINK_BYTES` (made in the index from a blob, as no file
/// system holds a link so long), and one that leads through it.
#[cfg(unix)]
#[tokio::test]
async fn a_link_whose_text_is_not_read_whole_leads_nowhere_that_can_be_told() {
    if !git_installed().await {
        return;
    }
    let r = repo().await;
    let most = usize::try_from(LINK_BYTES).unwrap();
    let text = format!("{}../a.txt", "./".repeat(most / 2 + 1));
    assert!(text.len() > most);
    let outside = r.repo.parent().unwrap();
    write(outside, "long-text", &text).await;
    let blob = git(
        &r.repo,
        &[
            "hash-object",
            "-w",
            outside.join("long-text").to_str().unwrap(),
        ],
    )
    .await;
    git(
        &r.repo,
        &[
            "update-index",
            "--add",
            "--cacheinfo",
            &format!("120000,{blob},d/long"),
        ],
    )
    .await;
    tokio::fs::symlink("d/long", r.repo.join("via"))
        .await
        .unwrap();
    git(&r.repo, &["add", "via"]).await;
    git(&r.repo, &["commit", "-q", "-m", "long link"]).await;
    let states = open(&r.repo, "HEAD", head()).await;
    let names = BTreeSet::from([rel("d/long"), rel("via")]);
    let read = states
        .read(Side::After, &names, ByteBound::WHOLE)
        .await
        .unwrap();
    // At the start, and for a link met along the way, not the directory of the link.
    for name in ["d/long", "via"] {
        assert!(
            matches!(
                read[&rel(name)].as_ref().unwrap(),
                AtSide::Elsewhere {
                    leads: Leads::Unresolved,
                    ..
                }
            ),
            "{name}: {:?}",
            read[&rel(name)]
        );
    }
}

#[tokio::test]
async fn a_path_of_a_conflict_in_the_index_cannot_be_told() {
    if !git_installed().await {
        return;
    }
    let r = repo().await;
    git(&r.repo, &["checkout", "-q", "-b", "other"]).await;
    write(&r.repo, "a.txt", "theirs\n").await;
    git(&r.repo, &["commit", "-q", "-am", "theirs"]).await;
    git(&r.repo, &["checkout", "-q", "main"]).await;
    write(&r.repo, "a.txt", "ours\n").await;
    git(&r.repo, &["commit", "-q", "-am", "ours"]).await;
    let (merged, _) = git_may(&r.repo, &["merge", "-q", "other"]).await;
    assert!(!merged);
    let states = open(&r.repo, "HEAD", GitAfter::Index).await;
    let read = states
        .read(
            Side::After,
            &BTreeSet::from([rel("a.txt")]),
            ByteBound::WHOLE,
        )
        .await
        .unwrap();
    assert_eq!(
        read[&rel("a.txt")].as_ref().unwrap(),
        &AtSide::Untold(said(CONFLICT))
    );
}

#[tokio::test]
async fn a_side_whose_listing_is_past_its_bound_cannot_tell_any_name() {
    if !git_installed().await {
        return;
    }
    let r = repo().await;
    write(&r.repo, "c.txt", "three\n").await;
    git(&r.repo, &["add", "c.txt"]).await;
    git(&r.repo, &["commit", "-q", "-m", "two"]).await;
    let states = open(&r.repo, "HEAD~1", head()).await.with_list_bound(8);
    let names = BTreeSet::from([rel("a.txt")]);
    let read = states
        .read(Side::Before, &names, ByteBound::WHOLE)
        .await
        .unwrap();
    let Ok(AtSide::Untold(Untold::Said(why))) = &read[&rel("a.txt")] else {
        panic!("{read:?}");
    };
    assert_eq!(
        why.as_str(),
        "the listing of the tree of this side is past 8 bytes"
    );
    // Both sides past the bound say the same of a name, whatever revision each is,
    // so the two are alike: no change.
    let after = states
        .read(Side::After, &names, ByteBound::WHOLE)
        .await
        .unwrap();
    assert_eq!(
        read[&rel("a.txt")].as_ref().unwrap(),
        after[&rel("a.txt")].as_ref().unwrap()
    );
    assert!(matches!(
        states.list(Side::After, None).await.unwrap(),
        DirListing::NotADir(NotADir::Untold(_))
    ));
    assert!(matches!(
        states.files(Side::After, ByteBound::WHOLE).await.unwrap(),
        TreeFiles::CannotList(_)
    ));
}

#[tokio::test]
async fn neither_side_reads_what_the_system_resolves() {
    if !git_installed().await {
        return;
    }
    let r = repo().await;
    let states = open(&r.repo, "HEAD", head()).await;
    let names = BTreeSet::from([rel("up/x")]);
    let leaves = AtSide::Untold(Untold::Said(LEAVES.into()));
    for side in [Side::Before, Side::After] {
        let read = states
            .read_outside(side, &names, ByteBound::WHOLE)
            .await
            .unwrap();
        assert_eq!(read[&rel("up/x")].as_ref().unwrap(), &leaves);
        assert_eq!(
            states.list_outside(side, &rel("up")).await.unwrap(),
            DirListing::NotADir(NotADir::Untold(Untold::Said(LEAVES.into())))
        );
        assert!(states.addresses(side, &names).await.unwrap().is_empty());
    }
    assert_eq!(states.system_read(), SystemRead::Untold(leaves_untold()));
}

#[tokio::test]
async fn a_side_of_git_has_no_resolved_path_and_a_directory_has_its_canonical_one() {
    if !git_installed().await {
        return;
    }
    let r = repo().await;
    let top = Beyond {
        dir: None,
        parts: Vec::new(),
    };
    let tree = ProjectTree::git(open(&r.repo, "HEAD", head()).await);
    for side in [Side::Before, Side::After] {
        assert_eq!(tree.real_path(side, &top).await, None);
    }
    let files = ProjectTree::files(FileStates::new(r.repo.clone()));
    let canonical = tokio::fs::canonicalize(&r.repo).await.unwrap();
    assert_eq!(
        files.real_path(Side::After, &top).await,
        Some(canonical.clone())
    );
    let d = Beyond {
        dir: Some(rel("d")),
        parts: Vec::new(),
    };
    assert_eq!(
        files.real_path(Side::After, &d).await,
        Some(canonical.join("d"))
    );
    // A build directory that is the top, as `target-dir = "."` names it.
    let built = Beyond {
        dir: None,
        parts: Vec::new(),
    };
    assert_eq!(files.real_path(Side::After, &built).await, Some(canonical));
    assert_eq!(files.real_path(Side::Before, &top).await, None);
}

#[tokio::test]
async fn the_docs_a_commit_holds_are_held_against_the_sources_of_that_commit() {
    if !git_installed().await {
        return;
    }
    const MARK: &str = "<meta name=\"generator\" content=\"rustdoc\">";
    const LIB: &str = "pub struct Shelf;\n";
    let r = repo().await;
    write(&r.repo, "Cargo.toml", "[package]\nname = \"demo\"\n").await;
    write(&r.repo, "src/lib.rs", LIB).await;
    let page = |body: &str| format!("<html><head>{MARK}</head><body>{body}</body></html>");
    write(&r.repo, "target/doc/demo/index.html", &page("")).await;
    write(&r.repo, "target/doc/demo/struct.Shelf.html", &page("")).await;
    write(
        &r.repo,
        "target/doc/src/demo/lib.rs.html",
        &page(&format!("<pre class=\"rust\"><code>{LIB}</code></pre>")),
    )
    .await;
    git(&r.repo, &["add", "-A"]).await;
    git(&r.repo, &["commit", "-q", "-m", "docs"]).await;
    let item = ArtifactName::new("demo::Shelf").unwrap();
    let read = |tree: ProjectTree| async move {
        let space = RustItemSpace::on(tree, None);
        SpaceAdapter::resolve(
            &space,
            Side::After,
            &asks(&["demo::Shelf"], Comparison::Presence),
        )
        .await
        .unwrap()
    };
    let found = read(ProjectTree::git(open(&r.repo, "HEAD", head()).await)).await;
    assert!(found[&item].is_present(), "{:?}", found[&item]);
    // The source committed again, and not the docs.
    write(
        &r.repo,
        "src/lib.rs",
        "pub struct Shelf;\npub struct Added;\n",
    )
    .await;
    git(&r.repo, &["commit", "-q", "-am", "source"]).await;
    let found = read(ProjectTree::git(open(&r.repo, "HEAD", head()).await)).await;
    assert!(
        matches!(
            found[&item].state(&Comparison::Presence),
            ArtifactState::Unknown(Untold::NotOfSources(not))
                if not.why() == &NotOfSourcesWhy::Differs { file: rel("src/lib.rs") }
        ),
        "{:?}",
        found[&item]
    );
}

fn leaves_untold() -> Untold {
    Untold::Said(LEAVES.into())
}

#[tokio::test]
async fn a_tree_that_cannot_be_read_as_asked_is_not_opened() {
    if !git_installed().await {
        return;
    }
    let r = repo().await;
    for rev in ["no-such-rev", "-p"] {
        let opened = GitStates::open(r.repo.clone(), rev, &head()).await;
        assert!(
            matches!(&opened, Err(OpenError::NotACommit { rev: asked }) if asked == rev),
            "{opened:?}"
        );
    }
    let outside = tempfile::tempdir().unwrap();
    let opened = GitStates::open(outside.path().to_path_buf(), "HEAD", &head()).await;
    assert!(
        matches!(opened, Err(OpenError::NotARepository { .. })),
        "{opened:?}"
    );
    // No git to run at all is not the repository's: it is its own answer.
    let nowhere = outside.path().join("no-such-git");
    let opened = GitStates::open_with_program(r.repo.clone(), "HEAD", &head(), nowhere).await;
    assert!(matches!(opened, Err(OpenError::NoGit)), "{opened:?}");
}

/// The `git` on the path, by its absolute path, or none.
async fn real_git() -> Option<PathBuf> {
    for dir in std::env::split_paths(&std::env::var_os("PATH")?) {
        let path = dir.join("git");
        if let Ok(meta) = tokio::fs::metadata(&path).await {
            // arch-lint: allow(no-sync-io) reason="`meta` is Metadata fetched with `.await`: a pure accessor, no I/O"
            if meta.is_file() {
                return Some(path);
            }
        }
    }
    None
}

/// A Git that does not know a form `GitStates` reads with: each call holding `words`
/// exits as Git does for a usage it does not know, and every other is the real one's.
/// Opening must name that form: if it stopped trying the form, it would open, and this
/// test would fail.
#[cfg(unix)]
#[tokio::test]
async fn a_git_that_does_not_know_a_form_a_tree_is_read_with_is_not_opened() {
    use std::os::unix::fs::PermissionsExt;
    if !git_installed().await {
        return;
    }
    let Some(real) = real_git().await else {
        eprintln!("not checked: no git found on the path");
        return;
    };
    let r = repo().await;
    let bin = tempfile::tempdir().unwrap();
    for (words, until, call) in [
        ("--attr-source", head(), TreeCall::AttrSource),
        ("cat-file --filters", head(), TreeCall::Converted),
        ("cat-file --batch", head(), TreeCall::Stored),
        ("ls-tree", head(), TreeCall::TreeListing),
        ("ls-files -s", GitAfter::Index, TreeCall::IndexListing),
    ] {
        let script = bin.path().join(format!("git-{}", words.len()));
        let text = format!(
            "#!/bin/sh\ncase \"$*\" in\n  *'{words}'*) echo 'unknown option' >&2; exit 129 ;;\nesac\nexec '{}' \"$@\"\n",
            real.display()
        );
        tokio::fs::write(&script, text).await.unwrap();
        tokio::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755))
            .await
            .unwrap();
        let opened = GitStates::open_with_program(r.repo.clone(), "HEAD", &until, script).await;
        match opened {
            Err(OpenError::UnknownOption { option }) => assert_eq!(option, call.written()),
            other => panic!("{words}: {other:?}"),
        }
    }
}

#[cfg(not(unix))]
#[test]
fn a_git_that_does_not_know_a_form_a_tree_is_read_with_is_not_opened() {
    eprintln!("not checked: no script can stand in for git here");
}

fn asks(names: &[&str], comparison: Comparison) -> SpaceAsks {
    names
        .iter()
        .map(|name| {
            (
                ArtifactName::new(name).unwrap(),
                BTreeSet::from([comparison.clone()]),
            )
        })
        .collect()
}

fn untold_why(reading: &ArtifactReading) -> String {
    match reading.state(&Comparison::Presence) {
        ArtifactState::Unknown(Untold::Said(why)) => why.as_str().to_owned(),
        other => panic!("{other:?}"),
    }
}

#[tokio::test]
async fn the_spaces_read_a_run_through_git() {
    if !git_installed().await {
        return;
    }
    let r = repo().await;
    write(&r.repo, "src/t.rs", "#[test]\nfn top() {}\n").await;
    write(&r.repo, "web/a.test.ts", "test('one', () => {});\n").await;
    write(&r.repo, "Cargo.toml", "[workspace]\n").await;
    git(&r.repo, &["add", "-A"]).await;
    git(&r.repo, &["commit", "-q", "-m", "code"]).await;
    // What is not committed is not read: the file edited, a test renamed, and the
    // other's file removed, in the work tree only.
    write(&r.repo, "a.txt", "edited\n").await;
    write(&r.repo, "src/t.rs", "#[test]\nfn renamed() {}\n").await;
    tokio::fs::remove_file(r.repo.join("web/a.test.ts"))
        .await
        .unwrap();
    let tree = ProjectTree::git(open(&r.repo, "HEAD", head()).await);

    let file = FileSpace::on(tree.clone(), r.repo.clone());
    let found = SpaceAdapter::resolve(&file, Side::Before, &asks(&["a.txt"], Comparison::Bytes))
        .await
        .unwrap();
    assert_eq!(
        found[&ArtifactName::new("a.txt").unwrap()].state(&Comparison::Bytes),
        ArtifactState::Present(Comparison::Bytes.fingerprint(b"one\n"))
    );
    let rust = RustTestSpace::on(tree.clone());
    let found = SpaceAdapter::resolve(
        &rust,
        Side::After,
        &asks(&["src/t.rs::top"], Comparison::Bytes),
    )
    .await
    .unwrap();
    assert!(found[&ArtifactName::new("src/t.rs::top").unwrap()].is_present());
    let js = JsTestSpace::on(tree.clone());
    let found = SpaceAdapter::resolve(
        &js,
        Side::After,
        &asks(&["web/a.test.ts::one"], Comparison::Bytes),
    )
    .await
    .unwrap();
    assert!(found[&ArtifactName::new("web/a.test.ts::one").unwrap()].is_present());

    // The built docs: none in a commit (nothing is built there), and none outside it
    // at either side.
    let item = ArtifactName::new("demo::Shelf").unwrap();
    let inside = RustItemSpace::on(tree.clone(), None);
    let found = SpaceAdapter::resolve(
        &inside,
        Side::After,
        &asks(&["demo::Shelf"], Comparison::Presence),
    )
    .await
    .unwrap();
    let why = untold_why(&found[&item]);
    assert_ne!(why, NonBlank::from(LEAVES).as_str());
    let outside = RustItemSpace::on(tree, Some(r.repo.join("target")));
    let found = SpaceAdapter::resolve(
        &outside,
        Side::Before,
        &asks(&["demo::Shelf"], Comparison::Presence),
    )
    .await
    .unwrap();
    assert_eq!(untold_why(&found[&item]), NonBlank::from(LEAVES).as_str());
}
