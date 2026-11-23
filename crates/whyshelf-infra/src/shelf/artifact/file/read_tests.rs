//! The tests of the reading of the files that ask its own parts.

use std::path::Path;

use super::*;

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
    let state = FileStates::new(root.path().to_path_buf());
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
