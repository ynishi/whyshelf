//! The tests of the reading of the files that ask its own parts: a reading within a
//! bound, the choice of the files a selection takes (`Lister`), and the one writing
//! of the reason of a listing that is not all (`listing`).

use std::path::Path;

use super::*;
use crate::shelf::artifact::conformance::{git_runs, isolated_git};
use crate::shelf::state::files::FileStates;

fn tree(root: &Path) -> ProjectTree {
    ProjectTree::files(FileStates::new(root.to_path_buf()))
}

fn count(n: usize) -> NonZeroUsize {
    NonZeroUsize::new(n).unwrap()
}

fn why(text: &str) -> NonBlank {
    NonBlank::new(text).unwrap()
}

/// What a selection of the `.rs` files makes of a path: each taken as its path, but
/// those in `unnameable`, taken with a path that is no name.
fn rs_files(unnameable: &[&str]) -> impl Fn(&RelPath) -> Take<RelPath> {
    let unnameable: Vec<String> = unnameable.iter().map(|p| (*p).to_owned()).collect();
    move |path: &RelPath| {
        if !path.as_str().ends_with(".rs") {
            Take::Skip
        } else if unnameable.iter().any(|p| p == path.as_str()) {
            Take::Unnameable
        } else {
            Take::Named(path.clone())
        }
    }
}

/// `files` written under `root`, in a repository of Git (untracked, so that Git
/// lists them as files not ignored).
async fn listed(root: &Path, files: &[(&str, &str)]) {
    for (at, text) in files {
        write(root, at, text).await;
    }
    isolated_git(root, &["init", "-q"]).await;
}

const THREE: [(&str, &str); 4] = [
    ("a.rs", "fn a() {}\n"),
    ("b.rs", "fn b() {}\n"),
    ("c.rs", "fn c() {}\n"),
    ("notes.txt", "not taken\n"),
];

#[tokio::test]
async fn a_lister_takes_the_files_the_state_lists_that_the_selection_takes_in_their_order() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    for (at, text) in THREE {
        write(root, at, text).await;
    }
    let lister = Lister::on(tree(root), MAX_TEST_FILE);
    // Outside a repository the state cannot list: no listing, and no error.
    assert!(matches!(
        lister
            .taken(Side::After, rs_files(&[]), None)
            .await
            .unwrap(),
        Taken::CannotList(_)
    ));
    if !git_runs().await {
        return;
    }
    isolated_git(root, &["init", "-q"]).await;
    let all = vec![rel("a.rs"), rel("b.rs"), rel("c.rs")];
    assert_eq!(
        lister
            .taken(Side::After, rs_files(&[]), None)
            .await
            .unwrap(),
        Taken::All(all.clone())
    );
    // As many as `most` is all of them; one fewer stops at the next.
    assert_eq!(
        lister
            .taken(Side::After, rs_files(&[]), Some(3))
            .await
            .unwrap(),
        Taken::All(all)
    );
    assert_eq!(
        lister
            .taken(Side::After, rs_files(&[]), Some(2))
            .await
            .unwrap(),
        Taken::Some {
            found: vec![rel("a.rs"), rel("b.rs")],
            short: NonEmptyVec::new(Short::Past { limit: 2 }, vec![]),
        }
    );
}

#[tokio::test]
async fn the_bound_of_names_counts_only_the_files_taken_as_a_name() {
    if !git_runs().await {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    listed(root, &THREE).await;
    let lister = Lister::on(tree(root), MAX_TEST_FILE);
    // Two names and a path that is no name, held to two names: the path is counted
    // apart, and the listing does not say it takes more than two names.
    let taken = lister
        .taken(Side::After, rs_files(&["b.rs"]), Some(2))
        .await
        .unwrap();
    let Taken::Some { found, short } = taken else {
        panic!("a path that is no name keeps the listing from being all: {taken:?}");
    };
    assert_eq!(found, vec![rel("a.rs"), rel("c.rs")]);
    assert_eq!(short.clone().into_vec(), vec![Short::Unnameable(count(1))]);
    let names: BTreeSet<ArtifactName> = found
        .iter()
        .map(|p| ArtifactName::new(p.as_str()).unwrap())
        .collect();
    let causes = short.into_vec().into_iter().map(Cause::from).collect();
    assert_eq!(
        listing(names.clone(), causes, Vec::new()).unwrap(),
        NameListing::Some {
            found: names,
            why: why("1 file(s) the selection takes with a path too long for a name"),
        }
    );
    // One name past the bound: that alone is said.
    let taken = lister
        .taken(Side::After, rs_files(&["b.rs"]), Some(1))
        .await
        .unwrap();
    let Taken::Some { found, short } = taken else {
        panic!("past the bound of names: {taken:?}");
    };
    assert_eq!(found, vec![rel("a.rs")]);
    let causes: Vec<Cause> = short.into_vec().into_iter().map(Cause::from).collect();
    let NameListing::Some { why, .. } = listing(BTreeSet::new(), causes, Vec::new()).unwrap()
    else {
        panic!("some of them");
    };
    assert_eq!(
        why.as_str(),
        "the selection takes more than 1 names, and the listing stops there"
    );
}

#[tokio::test]
async fn a_file_a_reading_cannot_tell_and_a_listing_the_state_cut_keep_it_from_being_all() {
    if !git_runs().await {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    listed(root, &[("a.rs", "fn a() {}\n"), ("b.rs", "x")]).await;
    // a.rs is past the bound of bytes of a file: there, but a reading cannot tell it.
    let small = Lister::on(tree(root), 4);
    assert_eq!(
        small.taken(Side::After, rs_files(&[]), None).await.unwrap(),
        Taken::Some {
            found: vec![rel("b.rs")],
            short: NonEmptyVec::new(Short::Untold(count(1)), vec![]),
        }
    );
    // The state's listing cut at its bound: what it said is kept as it said it.
    let cut = Lister::on(tree(root), MAX_TEST_FILE).with_limits(ScanLimits {
        listing_bytes: 8,
        ..ScanLimits::default()
    });
    let taken = cut.taken(Side::After, rs_files(&[]), None).await.unwrap();
    assert!(
        matches!(&taken, Taken::Some { short, .. } if short.iter().any(|s| matches!(s, Short::Left(_)))),
        "{taken:?}"
    );
}

#[tokio::test]
async fn the_bounds_a_space_file_is_given_are_the_ones_its_listing_and_its_reading_read() {
    if !git_runs().await {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    listed(root, &THREE).await;
    let selection = Selection::new(vec!["*.rs".to_owned()], vec![]).unwrap();
    // The bound of names given is the listing's.
    let two = FileSpace::new(root.to_path_buf()).with_limits(ScanLimits {
        names: 2,
        ..ScanLimits::default()
    });
    assert!(matches!(
        two.list(&selection).await.unwrap(),
        NameListing::Some { found, .. } if found.len() == 2
    ));
    // The bound of bytes of a file given is the one the listing looks with and the
    // one a reading reads with: a file past it is one neither can tell.
    let small = FileSpace::new(root.to_path_buf()).with_max_file(4);
    let listing = small.list(&selection).await.unwrap();
    assert!(
        matches!(&listing, NameListing::Some { found, .. } if found.is_empty()),
        "{listing:?}"
    );
    let name = ArtifactName::new("a.rs").unwrap();
    assert_eq!(small.current(&name).await.unwrap(), None);
    let asks: SpaceAsks = [(
        name.clone(),
        [whyshelf_core::domain::shelf::artifact::Comparison::Bytes].into(),
    )]
    .into();
    assert!(matches!(
        small.resolve(&asks).await.unwrap()[&name]
            .state(&whyshelf_core::domain::shelf::artifact::Comparison::Bytes),
        whyshelf_core::domain::shelf::artifact::ArtifactState::Unknown(_)
    ));
}

#[test]
fn a_listing_is_all_when_nothing_kept_it_from_being_so_and_says_every_cause_in_one_order() {
    let names: BTreeSet<ArtifactName> = [ArtifactName::new("a.rs").unwrap()].into();
    assert_eq!(
        listing(names.clone(), Vec::new(), Vec::new()).unwrap(),
        NameListing::All(names.clone())
    );
    let said = |causes: Vec<Cause>| match listing(names.clone(), causes, Vec::new()).unwrap() {
        NameListing::Some { found, why } => {
            assert_eq!(found, names, "the names found are kept");
            why.into_string()
        }
        other => panic!("{other:?}"),
    };
    // The bound of names, alone, whatever else there is.
    assert_eq!(
        said(vec![
            Cause::Left(why("cut")),
            Cause::PastNames { limit: 9 },
            Cause::Untold(count(2)),
        ]),
        "the selection takes more than 9 names, and the listing stops there"
    );
    // Otherwise every cause, in one order whatever the order they came in, in the
    // words the listing of the space file has said.
    assert_eq!(
        said(vec![
            Cause::Own(why("the adapter's own")),
            Cause::Untold(count(2)),
            Cause::Unnameable {
                count: count(1),
                of: Unit::Files,
            },
            Cause::Left(why("the state cut its listing")),
        ]),
        "the state cut its listing; 1 file(s) the selection takes with a path too long for a name; 2 file(s) the selection takes that a reading cannot tell (past the bound of bytes, leading out of the project, or not readable); the adapter's own"
    );
    // Tests whose name cannot be written are tests, not files or paths.
    let tests = said(vec![Cause::Unnameable {
        count: count(3),
        of: Unit::Tests,
    }]);
    assert_eq!(
        tests,
        "3 test(s) whose name cannot be written as a name of the space"
    );
    assert!(
        !tests.contains("file") && !tests.contains("path"),
        "{tests}"
    );
}

#[test]
fn a_listing_with_parts_alone_is_in_parts_and_one_with_a_cause_of_the_whole_is_some() {
    let name = |text: &str| ArtifactName::new(text).unwrap();
    let names: BTreeSet<ArtifactName> = [name("a.rs::t")].into();
    let key = |path: &str| super::super::name_of(&rel(path)).unwrap();
    let parts = vec![
        ListingPart::new(
            key("b.rs"),
            why("a derive"),
            PartRead::Found([name("b.rs::u")].into()),
        ),
        ListingPart::new(key("c.rs"), why("no file"), PartRead::Unread),
    ];
    // Only parts kept it from being all: each part is told apart, with its why.
    assert_eq!(
        listing(names.clone(), Vec::new(), parts.clone()).unwrap(),
        NameListing::Parted {
            found: names.clone(),
            parts: NonEmptyVec::from_vec(parts.clone()).unwrap(),
        }
    );
    // A cause of the whole selection says nothing of which part is missing: some of
    // the names, the names found in the parts among them, and the cause alone.
    assert_eq!(
        listing(names, vec![Cause::Untold(count(1))], parts).unwrap(),
        NameListing::Some {
            found: [name("a.rs::t"), name("b.rs::u")].into(),
            why: why(
                "1 file(s) the selection takes that a reading cannot tell (past the bound of bytes, leading out of the project, or not readable)"
            ),
        }
    );
}

/// The bound of bytes of a file in the tests of the lister.
const MAX_TEST_FILE: u64 = 1024;

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

#[tokio::test]
async fn a_file_is_read_within_its_bound_and_a_path_through_a_file_is_no_file() {
    let root = tempfile::tempdir().unwrap();
    write(root.path(), "a.txt", "0123456789").await;
    let state = ProjectTree::files(crate::shelf::state::files::FileStates::new(
        root.path().to_path_buf(),
    ));
    let read = |path: &'static str, max: u64| {
        let state = state.clone();
        async move {
            read_within(&state, Side::After, &rel(path), max)
                .await
                .unwrap()
        }
    };
    assert_eq!(
        read("a.txt", 10).await,
        Bounded::Bytes(b"0123456789".to_vec())
    );
    assert_eq!(read("a.txt", 9).await, Bounded::TooLarge);
    assert_eq!(read("a.txt/b", 10).await, Bounded::NoFile);
    assert_eq!(read("none.txt", 10).await, Bounded::NoFile);
}
