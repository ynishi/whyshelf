//! Tests of [`FileStates`]: what a name is at a side, where a link leads, the entries
//! of a directory, and what only a run of one side reads.

use std::collections::BTreeSet;
use std::path::Path;

use whyshelf_core::app::shelf::{
    AtSide, ByteBound, DirListing, EntryKind, Leads, NotADir, Side, StateList, StateRead, TreeFiles,
};
use whyshelf_core::domain::location::RelPath;
use whyshelf_core::domain::shelf::artifact::Untold;

use super::{FileStates, LEAVES};
use crate::shelf::state::StateError;
#[cfg(unix)]
use crate::shelf::tree_fixtures_tests::{ends, pipe};

fn rel(text: &str) -> RelPath {
    text.parse().unwrap()
}

async fn write(dir: &Path, at: &str, text: &str) {
    let full = dir.join(at);
    tokio::fs::create_dir_all(full.parent().unwrap())
        .await
        .unwrap();
    tokio::fs::write(full, text).await.unwrap();
}

#[cfg(unix)]
async fn link(target: impl AsRef<Path>, dir: &Path, at: &str) {
    let full = dir.join(at);
    tokio::fs::create_dir_all(full.parent().unwrap())
        .await
        .unwrap();
    tokio::fs::symlink(target, full).await.unwrap();
}

async fn one(state: &FileStates, side: Side, name: &str, most: u64) -> AtSide<RelPath> {
    state
        .read(side, &[rel(name)].into(), ByteBound::at_most(most))
        .await
        .unwrap()
        .remove(&rel(name))
        .unwrap()
        .unwrap()
}

#[tokio::test]
async fn a_file_is_its_bytes_within_one_byte_past_the_bound_a_directory_none_and_nothing_absent() {
    let root = tempfile::tempdir().unwrap();
    write(root.path(), "a/b.txt", "0123456789").await;
    let state = FileStates::new(root.path().to_path_buf());
    assert_eq!(
        one(&state, Side::After, "a/b.txt", 100).await,
        AtSide::There(b"0123456789".to_vec())
    );
    assert_eq!(
        one(&state, Side::After, "a/b.txt", 4).await,
        AtSide::There(b"01234".to_vec())
    );
    assert_eq!(one(&state, Side::After, "a", 100).await, AtSide::Dir);
    assert_eq!(
        one(&state, Side::After, "a/none", 100).await,
        AtSide::Absent
    );
    assert_eq!(
        one(&state, Side::After, "none/x", 100).await,
        AtSide::Absent
    );
    // A file on the way is the system's own error, as a reading of the path is: the
    // answer of that name, and the other names of the same reading are still read.
    let mut read = state
        .read(
            Side::After,
            &[rel("a/b.txt/c"), rel("a/b.txt")].into(),
            ByteBound::WHOLE,
        )
        .await
        .unwrap();
    let error = read.remove(&rel("a/b.txt/c")).unwrap().unwrap_err();
    assert!(
        matches!(&error, StateError::Io { source, .. } if source.kind() == std::io::ErrorKind::NotADirectory),
        "{error:?}"
    );
    assert_eq!(
        read.remove(&rel("a/b.txt")).unwrap().unwrap(),
        AtSide::There(b"0123456789".to_vec())
    );
}

#[cfg(unix)]
#[tokio::test]
async fn the_first_part_that_is_a_link_is_answered_with_where_it_leads() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("root");
    write(&root, "real/a.txt", "one").await;
    write(dir.path(), "away/a.txt", "out").await;
    link("real", &root, "near").await;
    link(root.join("real"), &root, "absolute").await;
    link(".", &root, "here").await;
    link(dir.path().join("away"), &root, "out").await;
    link("nowhere", &root, "broken").await;
    let state = FileStates::new(root.clone());
    let cases = [
        ("near/a.txt", "near", Leads::Inside(rel("real"))),
        ("absolute/a.txt", "absolute", Leads::Inside(rel("real"))),
        ("here/real/a.txt", "here", Leads::Top),
        ("out/a.txt", "out", Leads::Outside),
        ("broken", "broken", Leads::Broken),
    ];
    for (name, at, leads) in cases {
        let AtSide::Elsewhere {
            at: found,
            leads: led,
            target,
        } = one(&state, Side::After, name, 100).await
        else {
            panic!("{name}: no link");
        };
        assert_eq!((found, led), (rel(at), leads), "{name}");
        assert!(!target.is_empty(), "{name}");
    }
}

#[cfg(unix)]
#[tokio::test]
async fn a_loop_of_links_is_unresolved_and_a_look_tells_a_file_by_its_length_unopened() {
    let root = tempfile::tempdir().unwrap();
    link("two", root.path(), "one").await;
    link("one", root.path(), "two").await;
    write(root.path(), "a.txt", "0123456789").await;
    let state = FileStates::new(root.path().to_path_buf());
    assert!(matches!(
        one(&state, Side::After, "one", 100).await,
        AtSide::Elsewhere {
            leads: Leads::Unresolved,
            ..
        }
    ));
    // A pipe would wait for a writer if it were opened: the look ends.
    let piped = pipe(root.path(), "pipe").await;
    let mut names = BTreeSet::from([rel("a.txt"), rel("none"), rel("one")]);
    if piped {
        names.insert(rel("pipe"));
    }
    let looked = ends(&root.path().join("pipe"), state.look(Side::After, &names))
        .await
        .unwrap();
    let of = |name: &str| looked[&rel(name)].as_ref().unwrap();
    assert_eq!(of("a.txt"), &AtSide::There(10));
    assert_eq!(of("none"), &AtSide::Absent);
    assert!(matches!(of("one"), AtSide::Elsewhere { .. }));
    if piped {
        assert!(matches!(of("pipe"), AtSide::Untold(_)));
    }
}

#[cfg(unix)]
#[tokio::test]
async fn the_entries_of_a_directory_each_have_their_own_kind_a_link_its_own() {
    let root = tempfile::tempdir().unwrap();
    write(root.path(), "d/a.txt", "one").await;
    tokio::fs::create_dir_all(root.path().join("d/sub"))
        .await
        .unwrap();
    link("a.txt", root.path(), "d/alias").await;
    link("d", root.path(), "linked").await;
    let state = FileStates::new(root.path().to_path_buf());
    let DirListing::All(entries) = state.list(Side::After, Some(&rel("d"))).await.unwrap() else {
        panic!("a directory");
    };
    assert_eq!(
        entries.named.into_iter().collect::<Vec<_>>(),
        [
            (rel("d/a.txt"), EntryKind::File),
            (rel("d/alias"), EntryKind::Link),
            (rel("d/sub"), EntryKind::Dir),
        ]
    );
    let DirListing::All(top) = state.list(Side::After, None).await.unwrap() else {
        panic!("the top");
    };
    assert_eq!(top.named.get(&rel("linked")), Some(&EntryKind::Link));
    assert!(matches!(
        state.list(Side::After, Some(&rel("linked"))).await.unwrap(),
        DirListing::NotADir(NotADir::Elsewhere { .. })
    ));
    assert_eq!(
        state
            .list(Side::After, Some(&rel("d/a.txt")))
            .await
            .unwrap(),
        DirListing::NotADir(NotADir::File)
    );
    assert_eq!(
        state.list(Side::After, Some(&rel("none"))).await.unwrap(),
        DirListing::NotADir(NotADir::Absent)
    );
}

#[cfg(unix)]
#[tokio::test]
async fn another_kind_of_file_cannot_be_told_and_is_never_opened() {
    let root = tempfile::tempdir().unwrap();
    if !pipe(root.path(), "pipe").await {
        return;
    }
    let at = root.path().join("pipe");
    let state = FileStates::new(root.path().to_path_buf());
    // A pipe opened for reading would wait for a writer: a reading that opened it
    // fails the test (`ends`).
    let AtSide::Untold(Untold::Said(why)) = ends(&at, one(&state, Side::After, "pipe", 100)).await
    else {
        panic!("a pipe is told");
    };
    assert!(why.as_str().contains("neither a file"), "{why}");
    let read = ends(
        &at,
        state.read_outside(Side::After, &[rel("pipe")].into(), ByteBound::WHOLE),
    )
    .await
    .unwrap();
    assert!(
        matches!(read[&rel("pipe")], Ok(AtSide::Untold(_))),
        "{read:?}"
    );
    assert!(matches!(
        state.list(Side::After, Some(&rel("pipe"))).await.unwrap(),
        DirListing::NotADir(NotADir::Untold(_))
    ));
    let DirListing::All(top) = state.list(Side::After, None).await.unwrap() else {
        panic!("the top");
    };
    assert_eq!(top.named.get(&rel("pipe")), Some(&EntryKind::Other));
}

#[cfg(unix)]
#[tokio::test]
async fn a_run_of_one_side_reads_through_a_link_out_of_the_tree_and_one_of_two_cannot() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("root");
    tokio::fs::create_dir_all(&root).await.unwrap();
    write(dir.path(), "away/a.txt", "out").await;
    link(dir.path().join("away"), &root, "out").await;
    let one_side = FileStates::new(root.clone());
    let read = one_side
        .read_outside(Side::After, &[rel("out/a.txt")].into(), ByteBound::WHOLE)
        .await
        .unwrap();
    assert_eq!(
        read[&rel("out/a.txt")].as_ref().unwrap(),
        &AtSide::There(b"out".to_vec())
    );
    let DirListing::All(entries) = one_side
        .list_outside(Side::After, &rel("out"))
        .await
        .unwrap()
    else {
        panic!("a directory through the link");
    };
    assert_eq!(
        entries.named.keys().collect::<Vec<_>>(),
        [&rel("out/a.txt")]
    );

    let two_sides = FileStates::between(root.clone(), root.clone());
    for side in [Side::Before, Side::After] {
        let read = two_sides
            .read_outside(side, &[rel("out/a.txt")].into(), ByteBound::WHOLE)
            .await
            .unwrap();
        assert_eq!(
            read[&rel("out/a.txt")].as_ref().unwrap(),
            &AtSide::Untold(Untold::Said(LEAVES.into()))
        );
        assert_eq!(
            two_sides.list_outside(side, &rel("out")).await.unwrap(),
            DirListing::NotADir(NotADir::Untold(Untold::Said(LEAVES.into())))
        );
    }
}

#[tokio::test]
async fn a_run_of_one_side_reads_no_side_before_and_one_of_two_reads_each_directory() {
    let dir = tempfile::tempdir().unwrap();
    write(&dir.path().join("before"), "a.txt", "old").await;
    write(&dir.path().join("after"), "a.txt", "new").await;
    let one_side = FileStates::new(dir.path().join("after"));
    let error = one_side
        .read(Side::Before, &[rel("a.txt")].into(), ByteBound::WHOLE)
        .await
        .unwrap_err();
    assert!(matches!(error, StateError::NoBefore), "{error:?}");
    let two_sides = FileStates::between(dir.path().join("before"), dir.path().join("after"));
    assert_eq!(
        one(&two_sides, Side::Before, "a.txt", 100).await,
        AtSide::There(b"old".to_vec())
    );
    assert_eq!(
        one(&two_sides, Side::After, "a.txt", 100).await,
        AtSide::There(b"new".to_vec())
    );
    // Only a run of one side, at the side after, names addresses.
    for side in [Side::Before, Side::After] {
        assert!(
            two_sides
                .addresses(side, &[rel("a.txt")].into())
                .await
                .unwrap()
                .is_empty()
        );
    }
}

#[tokio::test]
async fn the_files_of_a_directory_outside_a_repository_cannot_be_listed() {
    let root = tempfile::tempdir().unwrap();
    write(root.path(), "a.txt", "one").await;
    let state = FileStates::new(root.path().to_path_buf());
    let files = state
        .files(Side::After, ByteBound::at_most(1 << 20))
        .await
        .unwrap();
    // A temporary directory may be inside a repository of the machine; only outside
    // one is the answer known.
    if let TreeFiles::CannotList(why) = files {
        assert!(why.as_str().contains("cannot be listed"), "{why}");
    }
}
