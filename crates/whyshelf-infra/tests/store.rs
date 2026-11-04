//! The stores on disk: events written once and idempotently, objects addressed by
//! the hash of their text, and what happens to damaged files.

mod common;

use std::collections::BTreeSet;
use std::sync::Arc;

use common::{all_files, json_files};
use uuid::Uuid;
use whyshelf_core::app::ports::{AppendOutcome, StorePort};
use whyshelf_core::app::raw::NormalizedText;
use whyshelf_core::domain::actor::Timestamp;
use whyshelf_core::domain::event::{Event, EventBody, StoreKind};
use whyshelf_core::domain::hash::ContentHash;
use whyshelf_core::domain::id::EntityId;
use whyshelf_core::domain::reference::Reach;
use whyshelf_infra::markdown::normalize;
use whyshelf_infra::store_fs::{FsStore, StoreError};

fn entity(n: u128) -> EntityId {
    let bits = (n & 0x0fff_ffff_ffff_ffff) | (0x7u128 << 76) | (0b10u128 << 62);
    EntityId::new(Uuid::from_u128(bits)).unwrap()
}

fn moved(target: u128, to: &str, at: &str) -> Event {
    Event::new(
        entity(target),
        BTreeSet::new(),
        Timestamp::from_validated(at.to_owned()).unwrap(),
        EventBody::Moved {
            old: "docs/a.md".parse().unwrap(),
            new: to.parse().unwrap(),
        },
    )
    .unwrap()
}

struct Stores {
    _dir: tempfile::TempDir,
    repo: std::path::PathBuf,
    private: std::path::PathBuf,
    store: FsStore,
}

fn stores() -> Stores {
    let dir = tempfile::tempdir().unwrap();
    let repo = dir.path().join("repo-store");
    let private = dir.path().join("private-store");
    Stores {
        store: FsStore::new(repo.clone(), private.clone()),
        _dir: dir,
        repo,
        private,
    }
}

#[tokio::test]
async fn an_event_is_written_once_and_read_back() {
    let s = stores();
    let event = moved(1, "docs/b.md", "2026-11-04T21:40:00+09:00");
    assert_eq!(
        s.store.append(StoreKind::Repo, &event).await.unwrap(),
        AppendOutcome::Written
    );
    assert_eq!(
        s.store.append(StoreKind::Repo, &event).await.unwrap(),
        AppendOutcome::AlreadyPresent
    );
    // The same change found by another writer at another time is the same file, and
    // the first writer's file stays as it was.
    let later = moved(1, "docs/b.md", "2027-01-01T00:00:00Z");
    assert_eq!(later.id(), event.id());
    assert_eq!(
        s.store.append(StoreKind::Repo, &later).await.unwrap(),
        AppendOutcome::AlreadyPresent
    );

    let files = json_files(&s.repo.join("events")).await;
    assert_eq!(files, [format!("{}.json", event.id().hash().hex())]);
    let back = s.store.events(StoreKind::Repo).await.unwrap();
    assert_eq!(back, vec![event]);
    assert_eq!(back[0].at().as_str(), "2026-11-04T21:40:00+09:00");
}

#[tokio::test]
async fn the_two_stores_are_separate() {
    let s = stores();
    let event = moved(1, "docs/b.md", "2026-11-04T21:40:00Z");
    s.store.append(StoreKind::Private, &event).await.unwrap();
    assert!(s.store.events(StoreKind::Repo).await.unwrap().is_empty());
    assert_eq!(s.store.events(StoreKind::Private).await.unwrap().len(), 1);
    assert!(json_files(&s.repo.join("events")).await.is_empty());
    assert_eq!(json_files(&s.private.join("events")).await.len(), 1);
}

#[tokio::test]
async fn an_empty_store_has_no_events() {
    let s = stores();
    assert!(s.store.events(StoreKind::Repo).await.unwrap().is_empty());
}

#[tokio::test]
async fn a_private_store_that_is_not_there_is_absent_and_an_empty_one_is_available() {
    let s = stores();
    // No directory: the events are empty, and the store cannot be read.
    assert!(s.store.events(StoreKind::Private).await.unwrap().is_empty());
    assert_eq!(s.store.private_reach().await.unwrap(), Reach::Absent);

    // A directory with nothing in it is a store: the same empty events, readable.
    tokio::fs::create_dir_all(&s.private).await.unwrap();
    assert!(s.store.events(StoreKind::Private).await.unwrap().is_empty());
    assert_eq!(s.store.private_reach().await.unwrap(), Reach::Available);
}

#[tokio::test]
async fn the_first_write_to_the_private_store_makes_it_available() {
    let s = stores();
    assert_eq!(s.store.private_reach().await.unwrap(), Reach::Absent);
    let event = moved(1, "docs/b.md", "2026-11-04T21:40:00Z");
    s.store.append(StoreKind::Private, &event).await.unwrap();
    assert_eq!(s.store.private_reach().await.unwrap(), Reach::Available);
}

#[tokio::test]
async fn a_file_where_the_private_store_should_be_is_not_a_store() {
    let s = stores();
    tokio::fs::write(&s.private, b"not a directory")
        .await
        .unwrap();
    assert_eq!(s.store.private_reach().await.unwrap(), Reach::Absent);
}

#[tokio::test]
async fn many_writers_of_one_event_leave_one_file_and_no_temporary_file() {
    let s = stores();
    let store = Arc::new(s.store.clone());
    let event = Arc::new(moved(7, "docs/c.md", "2026-11-04T21:40:00Z"));
    let mut tasks = Vec::new();
    for _ in 0..16 {
        let (store, event) = (Arc::clone(&store), Arc::clone(&event));
        tasks.push(tokio::spawn(async move {
            store.append(StoreKind::Repo, &event).await
        }));
    }
    let mut written = 0;
    for task in tasks {
        if task.await.unwrap().unwrap() == AppendOutcome::Written {
            written += 1;
        }
    }
    assert!(written >= 1);
    let names = all_files(&s.repo.join("events")).await;
    assert_eq!(names, [format!("{}.json", event.id().hash().hex())]);
}

#[tokio::test]
async fn a_leftover_temporary_file_is_not_an_event() {
    let s = stores();
    let event = moved(1, "docs/b.md", "2026-11-04T21:40:00Z");
    s.store.append(StoreKind::Repo, &event).await.unwrap();
    let name = format!("{}.json", event.id().hash().hex());
    let events = s.repo.join("events");
    tokio::fs::write(events.join(format!("{name}.tmp-123-0")), "half a fi")
        .await
        .unwrap();
    assert_eq!(s.store.events(StoreKind::Repo).await.unwrap().len(), 1);
}

#[tokio::test]
async fn a_renamed_or_edited_event_file_is_refused() {
    let s = stores();
    let event = moved(1, "docs/b.md", "2026-11-04T21:40:00Z");
    s.store.append(StoreKind::Repo, &event).await.unwrap();
    let events = s.repo.join("events");
    let path = events.join(format!("{}.json", event.id().hash().hex()));

    // Edited content: the identifier inside no longer matches.
    let text = tokio::fs::read_to_string(&path).await.unwrap();
    tokio::fs::write(&path, text.replace("docs/b.md", "docs/z.md"))
        .await
        .unwrap();
    assert!(matches!(
        s.store.events(StoreKind::Repo).await,
        Err(StoreError::Read { .. })
    ));

    // Right content, wrong name.
    tokio::fs::write(&path, text).await.unwrap();
    tokio::fs::rename(&path, events.join(format!("{}.json", "ab".repeat(32))))
        .await
        .unwrap();
    assert!(matches!(
        s.store.events(StoreKind::Repo).await,
        Err(StoreError::NameMismatch { .. })
    ));
}

fn text(body: &str) -> NormalizedText {
    normalize(body)
}

#[tokio::test]
async fn an_object_is_named_by_the_hash_of_its_text() {
    let s = stores();
    let version = text("---\nfinding_type: x\n---\nbody   \r\n");
    let raw = version.raw_hash();
    assert_eq!(
        s.store
            .put_object(StoreKind::Repo, &raw, &version)
            .await
            .unwrap(),
        AppendOutcome::Written
    );
    assert_eq!(
        s.store
            .put_object(StoreKind::Repo, &raw, &version)
            .await
            .unwrap(),
        AppendOutcome::AlreadyPresent
    );
    // The same text normalizes to the same object.
    let again = text("---\nfinding_type: x\n---\nbody\n");
    assert_eq!(again.raw_hash(), raw);

    assert_eq!(all_files(&s.repo.join("objects")).await, [raw.hex()]);
    let stored = tokio::fs::read(s.repo.join("objects").join(raw.hex()))
        .await
        .unwrap();
    assert_eq!(
        stored,
        version.as_str().as_bytes(),
        "the object is the normalized text"
    );

    assert_eq!(
        s.store.get_object(StoreKind::Repo, &raw).await.unwrap(),
        Some(version)
    );
    assert_eq!(
        s.store.get_object(StoreKind::Private, &raw).await.unwrap(),
        None
    );
}

#[tokio::test]
async fn a_text_under_another_hash_is_refused() {
    let s = stores();
    let version = text("one\n");
    let other = text("two\n").raw_hash();
    assert!(matches!(
        s.store.put_object(StoreKind::Repo, &other, &version).await,
        Err(StoreError::KeyMismatch { .. })
    ));
    assert!(!tokio::fs::try_exists(s.repo.join("objects")).await.unwrap());
}

#[tokio::test]
async fn a_damaged_object_is_detected_when_it_is_read() {
    let s = stores();
    let version = text("one\n");
    let raw = version.raw_hash();
    s.store
        .put_object(StoreKind::Repo, &raw, &version)
        .await
        .unwrap();
    tokio::fs::write(s.repo.join("objects").join(raw.hex()), "tampered\n")
        .await
        .unwrap();
    assert!(matches!(
        s.store.get_object(StoreKind::Repo, &raw).await,
        Err(StoreError::Corrupt { .. })
    ));
}

#[tokio::test]
async fn purging_removes_the_object_and_says_whether_there_was_one() {
    let s = stores();
    let version = text("a secret slipped through\n");
    let raw = version.raw_hash();
    s.store
        .put_object(StoreKind::Repo, &raw, &version)
        .await
        .unwrap();
    assert!(s.store.purge_object(StoreKind::Repo, &raw).await.unwrap());
    assert!(!s.store.purge_object(StoreKind::Repo, &raw).await.unwrap());
    assert_eq!(
        s.store.get_object(StoreKind::Repo, &raw).await.unwrap(),
        None
    );
    // An unrelated hash was never there.
    assert!(
        !s.store
            .purge_object(StoreKind::Repo, &ContentHash::raw(b"nothing"))
            .await
            .unwrap()
    );
}
