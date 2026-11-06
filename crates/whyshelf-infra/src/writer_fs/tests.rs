use super::*;

use std::fs;
use std::os::unix::fs::{MetadataExt, PermissionsExt, symlink};
use std::path::Path;

use whyshelf_core::domain::knowable::Knowable;

fn at(visibility: Visibility, path: &str) -> Location {
    Location::new(visibility, path.parse().unwrap(), Knowable::Unknown)
}

fn public(path: &str) -> Location {
    at(Visibility::Public, path)
}

fn writer(dir: &Path) -> FsWriter {
    FsWriter::new(dir.to_path_buf(), None)
}

fn mode(path: &Path) -> u32 {
    fs::metadata(path).unwrap().permissions().mode() & 0o7777
}

/// Every name in the directory, to see that no temporary file stays.
fn names(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
}

#[tokio::test]
async fn create_new_writes_the_file_and_then_says_it_exists() {
    let dir = tempfile::tempdir().unwrap();
    let w = writer(dir.path());
    let at = public("a.md");
    assert_eq!(w.create_new(&at, b"first").await.unwrap(), Write::Done);
    assert_eq!(w.create_new(&at, b"second").await.unwrap(), Write::Exists);
    assert_eq!(fs::read(dir.path().join("a.md")).unwrap(), b"first");
    assert_eq!(names(dir.path()), ["a.md"]);
}

#[tokio::test]
async fn create_new_makes_the_directories_on_the_way() {
    let dir = tempfile::tempdir().unwrap();
    let w = writer(dir.path());
    assert_eq!(
        w.create_new(&public("x/y/a.md"), b"t").await.unwrap(),
        Write::Done
    );
    assert_eq!(fs::read(dir.path().join("x/y/a.md")).unwrap(), b"t");
}

#[tokio::test]
async fn a_dangling_symlink_is_not_a_regular_file_and_is_not_followed() {
    let dir = tempfile::tempdir().unwrap();
    symlink("nowhere.md", dir.path().join("a.md")).unwrap();
    let w = writer(dir.path());
    assert_eq!(
        w.create_new(&public("a.md"), b"t").await.unwrap(),
        Write::NotRegularFile
    );
    assert!(!dir.path().join("nowhere.md").exists());
    assert_eq!(names(dir.path()), ["a.md"]);
}

#[tokio::test]
async fn a_symlink_to_a_file_is_refused_by_rewrite_and_remove() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("real.md"), b"old").unwrap();
    symlink("real.md", dir.path().join("a.md")).unwrap();
    let w = writer(dir.path());
    let hash = BytesHash::of(b"old");
    assert_eq!(
        w.rewrite(&public("a.md"), hash, b"new").await.unwrap(),
        Write::NotRegularFile
    );
    assert_eq!(
        w.remove(&public("a.md"), hash).await.unwrap(),
        Write::NotRegularFile
    );
    assert_eq!(fs::read(dir.path().join("real.md")).unwrap(), b"old");
    assert_eq!(names(dir.path()), ["a.md", "real.md"]);
}

#[tokio::test]
async fn a_symlink_in_a_directory_on_the_way_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    symlink(outside.path(), dir.path().join("link")).unwrap();
    let w = writer(dir.path());
    assert_eq!(
        w.create_new(&public("link/a.md"), b"t").await.unwrap(),
        Write::NotRegularFile
    );
    assert!(names(outside.path()).is_empty());
}

#[tokio::test]
async fn a_directory_is_not_a_regular_file() {
    let dir = tempfile::tempdir().unwrap();
    fs::create_dir(dir.path().join("a.md")).unwrap();
    let w = writer(dir.path());
    let hash = BytesHash::of(b"");
    assert_eq!(
        w.create_new(&public("a.md"), b"t").await.unwrap(),
        Write::NotRegularFile
    );
    assert_eq!(
        w.rewrite(&public("a.md"), hash, b"t").await.unwrap(),
        Write::NotRegularFile
    );
    assert_eq!(
        w.remove(&public("a.md"), hash).await.unwrap(),
        Write::NotRegularFile
    );
    assert!(dir.path().join("a.md").is_dir());
}

#[tokio::test]
async fn rewrite_with_the_bytes_replaces_the_content_and_keeps_the_mode() {
    for want in [0o640, 0o600, 0o644, 0o664] {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.md");
        fs::write(&path, b"old\r\n").unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(want)).unwrap();
        let before = fs::metadata(&path).unwrap();
        let w = writer(dir.path());
        let done = w
            .rewrite(&public("a.md"), BytesHash::of(b"old\r\n"), b"new")
            .await
            .unwrap();
        assert_eq!(done, Write::Done);
        assert_eq!(fs::read(&path).unwrap(), b"new");
        assert_eq!(mode(&path), want);
        let after = fs::metadata(&path).unwrap();
        assert_eq!((after.uid(), after.gid()), (before.uid(), before.gid()));
        assert_eq!(names(dir.path()), ["a.md"]);
    }
}

#[tokio::test]
async fn rewrite_with_other_bytes_leaves_the_file_alone() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("a.md");
    fs::write(&path, b"old").unwrap();
    let w = writer(dir.path());
    let done = w
        .rewrite(&public("a.md"), BytesHash::of(b"older"), b"new")
        .await
        .unwrap();
    assert_eq!(done, Write::ChangedUnderfoot);
    assert_eq!(fs::read(&path).unwrap(), b"old");
    assert_eq!(names(dir.path()), ["a.md"]);
}

#[tokio::test]
async fn rewrite_and_remove_of_a_missing_file_say_it_changed() {
    let dir = tempfile::tempdir().unwrap();
    let w = writer(dir.path());
    let hash = BytesHash::of(b"x");
    for path in ["a.md", "no/a.md"] {
        assert_eq!(
            w.rewrite(&public(path), hash, b"n").await.unwrap(),
            Write::ChangedUnderfoot
        );
        assert_eq!(
            w.remove(&public(path), hash).await.unwrap(),
            Write::ChangedUnderfoot
        );
    }
    assert!(names(dir.path()).is_empty());
}

#[tokio::test]
async fn a_file_with_two_hard_links_is_not_replaced() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("a.md");
    fs::write(&path, b"old").unwrap();
    fs::hard_link(&path, dir.path().join("b.md")).unwrap();
    let w = writer(dir.path());
    let done = w
        .rewrite(&public("a.md"), BytesHash::of(b"old"), b"new")
        .await
        .unwrap();
    assert_eq!(done, Write::MultipleLinks);
    assert_eq!(fs::read(&path).unwrap(), b"old");
    assert_eq!(fs::read(dir.path().join("b.md")).unwrap(), b"old");
    assert_eq!(names(dir.path()), ["a.md", "b.md"]);
}

#[tokio::test]
async fn remove_with_the_bytes_removes_and_with_other_bytes_does_not() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("a.md");
    fs::write(&path, b"old").unwrap();
    let w = writer(dir.path());
    assert_eq!(
        w.remove(&public("a.md"), BytesHash::of(b"other"))
            .await
            .unwrap(),
        Write::ChangedUnderfoot
    );
    assert_eq!(fs::read(&path).unwrap(), b"old");
    assert_eq!(
        w.remove(&public("a.md"), BytesHash::of(b"old"))
            .await
            .unwrap(),
        Write::Done
    );
    assert!(names(dir.path()).is_empty());
}

#[tokio::test]
async fn a_private_location_goes_to_the_private_root_only() {
    let public_dir = tempfile::tempdir().unwrap();
    let private_dir = tempfile::tempdir().unwrap();
    let w = FsWriter::new(
        public_dir.path().to_path_buf(),
        Some(private_dir.path().to_path_buf()),
    );
    let at = at(Visibility::Private, "p.md");
    assert_eq!(w.create_new(&at, b"t").await.unwrap(), Write::Done);
    assert_eq!(names(private_dir.path()), ["p.md"]);
    assert!(names(public_dir.path()).is_empty());

    let alone = writer(public_dir.path());
    let error = alone.create_new(&at, b"t").await.unwrap_err();
    assert!(matches!(error, WriterError::NoPrivateRoot { .. }));
    assert!(names(public_dir.path()).is_empty());
}

#[tokio::test]
async fn what_create_new_makes_in_the_private_root_is_the_persons_alone() {
    let public_dir = tempfile::tempdir().unwrap();
    let private_dir = tempfile::tempdir().unwrap();
    let w = FsWriter::new(
        public_dir.path().to_path_buf(),
        Some(private_dir.path().to_path_buf()),
    );
    let made = w
        .create_new(&at(Visibility::Private, "d/p.md"), b"t")
        .await
        .unwrap();
    assert_eq!(made, Write::Done);
    // Whatever the umask, nothing is left for the group or others.
    assert_eq!(mode(&private_dir.path().join("d/p.md")) & 0o077, 0);
    assert_eq!(mode(&private_dir.path().join("d")) & 0o077, 0);
}

#[tokio::test]
async fn an_unwritable_directory_is_an_error_and_leaves_no_temporary_file() {
    // Root ignores directory permissions, so there is nothing to see.
    let dir = tempfile::tempdir().unwrap();
    if fs::metadata(dir.path()).unwrap().uid() == 0 {
        return;
    }
    let path = dir.path().join("a.md");
    fs::write(&path, b"old").unwrap();
    fs::set_permissions(dir.path(), fs::Permissions::from_mode(0o500)).unwrap();
    let w = writer(dir.path());
    let result = w
        .rewrite(&public("a.md"), BytesHash::of(b"old"), b"new")
        .await;
    fs::set_permissions(dir.path(), fs::Permissions::from_mode(0o700)).unwrap();
    assert!(matches!(result, Err(WriterError::Io { .. })));
    assert_eq!(fs::read(&path).unwrap(), b"old");
    assert_eq!(names(dir.path()), ["a.md"]);
}

// Not covered here, because it needs privileges: that `fchown` to another owner is
// refused (`CannotKeepOwner`), and that a group the process is not in is refused
// (`CannotKeepGroup`) or let go when the group has no access. They are the two
// `Errno::PERM` branches of `replace`.
