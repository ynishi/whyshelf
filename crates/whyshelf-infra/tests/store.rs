//! The stores on disk: records written once and idempotently, objects addressed by
//! the hash of their text, the mark of the layout, and what happens to damaged files.

mod common;

use std::collections::BTreeSet;
use std::sync::Arc;

use common::{all_files, commit_all, git, init_repo, json_files, write};
use uuid::Uuid;
use whyshelf_core::app::ports::{AppendOutcome, StorePort};
use whyshelf_core::app::raw::NormalizedText;
use whyshelf_core::app::store_contract::check_append_and_events;
use whyshelf_core::domain::actor::Timestamp;
use whyshelf_core::domain::event::{
    DocKind, DocRegistration, Event, EventBody, RecordDigest, Registration, StoreKind, Version,
};
use whyshelf_core::domain::hash::ContentHash;
use whyshelf_core::domain::id::EntityId;
use whyshelf_core::domain::reference::Reach;
use whyshelf_infra::format::Found;
use whyshelf_infra::markdown::normalize;
use whyshelf_infra::store_fs::{FsStore, LAYOUT, StoreError};

fn entity(n: u128) -> EntityId {
    let bits = (n & 0x0fff_ffff_ffff_ffff) | (0x7u128 << 76) | (0b10u128 << 62);
    EntityId::new(Uuid::from_u128(bits)).unwrap()
}

fn at(text: &str) -> Timestamp {
    Timestamp::from_validated(text.to_owned()).unwrap()
}

fn moved(target: u128, to: &str) -> Event {
    Event::new(
        entity(target),
        BTreeSet::new(),
        EventBody::Moved {
            old: "docs/a.md".parse().unwrap(),
            new: to.parse().unwrap(),
        },
    )
    .unwrap()
}

/// The name of the file of the record of `event` at `time`.
fn record_name(event: &Event, time: &Timestamp) -> String {
    format!(
        "{}.{}.json",
        event.id().hash().hex(),
        RecordDigest::of(&event.id(), time)
    )
}

/// A registration; its identifier leaves out the tags, so two of them with
/// different tags are two events that disagree and carry one identifier.
fn registered_with(tags: &[&str]) -> Event {
    Event::new(
        entity(5),
        BTreeSet::new(),
        EventBody::Registered(Registration::Doc(DocRegistration::new(
            DocKind::Spec,
            "docs/spec/a.md".parse().unwrap(),
            None,
            None,
            tags.iter().map(|t| t.parse().unwrap()).collect(),
            Version::Unparsed {
                raw: ContentHash::raw(b"a"),
            },
        ))),
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
    let event = moved(1, "docs/b.md");
    let time = at("2026-11-04T21:40:00+09:00");
    assert_eq!(
        s.store
            .append(StoreKind::Repo, &event, &time)
            .await
            .unwrap(),
        AppendOutcome::Written
    );
    assert_eq!(
        s.store
            .append(StoreKind::Repo, &event, &time)
            .await
            .unwrap(),
        AppendOutcome::AlreadyPresent
    );

    let files = json_files(&s.repo.join("events")).await;
    assert_eq!(files, [record_name(&event, &time)]);
    let back = s.store.events(StoreKind::Repo).await.unwrap();
    assert_eq!(back.len(), 1);
    assert_eq!(back[0].event(), &event);
    assert_eq!(back[0].shown_at().as_str(), "2026-11-04T21:40:00+09:00");
    assert_eq!(back[0].at().len(), 1);
}

/// Two writers who saw the same state find the same change at different times: two
/// files, beside each other, never one over the other; read as one event with both
/// times. At the same time, it is one file and `AlreadyPresent`.
#[tokio::test]
async fn the_same_change_at_two_times_is_two_files_and_one_event_with_two_times() {
    let s = stores();
    let event = moved(1, "docs/b.md");
    let (first, second) = (at("2026-11-04T21:40:00+09:00"), at("2027-01-01T00:00:00Z"));
    assert_eq!(
        s.store
            .append(StoreKind::Repo, &event, &first)
            .await
            .unwrap(),
        AppendOutcome::Written
    );
    let before = tokio::fs::read(s.repo.join("events").join(record_name(&event, &first)))
        .await
        .unwrap();
    assert_eq!(
        s.store
            .append(StoreKind::Repo, &event, &second)
            .await
            .unwrap(),
        AppendOutcome::Written
    );
    // The same time again is the same record.
    assert_eq!(
        s.store
            .append(StoreKind::Repo, &event, &second)
            .await
            .unwrap(),
        AppendOutcome::AlreadyPresent
    );

    let mut names = vec![record_name(&event, &first), record_name(&event, &second)];
    names.sort();
    assert_eq!(json_files(&s.repo.join("events")).await, names);
    // The first writer's file stays as it was.
    let after = tokio::fs::read(s.repo.join("events").join(record_name(&event, &first)))
        .await
        .unwrap();
    assert_eq!(before, after);

    let back = s.store.events(StoreKind::Repo).await.unwrap();
    assert_eq!(back.len(), 1, "one event");
    assert_eq!(back[0].event(), &event);
    assert_eq!(back[0].at().len(), 2, "with two times");
    let times: BTreeSet<&str> = back[0].at().iter().map(Timestamp::as_str).collect();
    assert_eq!(times, BTreeSet::from([first.as_str(), second.as_str()]));
}

/// The contract the in-memory store keeps too: one function, run on each.
#[tokio::test]
async fn the_store_on_disk_keeps_the_store_contract() {
    let s = stores();
    check_append_and_events(&s.store).await.unwrap();
}

#[tokio::test]
async fn the_two_stores_are_separate() {
    let s = stores();
    let event = moved(1, "docs/b.md");
    s.store
        .append(StoreKind::Private, &event, &at("2026-11-04T21:40:00Z"))
        .await
        .unwrap();
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
    let event = moved(1, "docs/b.md");
    s.store
        .append(StoreKind::Private, &event, &at("2026-11-04T21:40:00Z"))
        .await
        .unwrap();
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
    let event = Arc::new(moved(7, "docs/c.md"));
    let time = at("2026-11-04T21:40:00Z");
    let mut tasks = Vec::new();
    for _ in 0..16 {
        let (store, event, time) = (Arc::clone(&store), Arc::clone(&event), time.clone());
        tasks.push(tokio::spawn(async move {
            store.append(StoreKind::Repo, &event, &time).await
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
    assert_eq!(names, [record_name(&event, &time)]);
    // The mark was written by whichever writer was first, and is one file.
    assert_eq!(
        tokio::fs::read_to_string(s.repo.join("layout"))
            .await
            .unwrap(),
        format!("{LAYOUT}\n")
    );
}

#[tokio::test]
async fn a_leftover_temporary_file_is_not_an_event() {
    let s = stores();
    let event = moved(1, "docs/b.md");
    let time = at("2026-11-04T21:40:00Z");
    s.store
        .append(StoreKind::Repo, &event, &time)
        .await
        .unwrap();
    let name = record_name(&event, &time);
    let events = s.repo.join("events");
    tokio::fs::write(events.join(format!("{name}.tmp-123-0")), "half a fi")
        .await
        .unwrap();
    assert_eq!(s.store.events(StoreKind::Repo).await.unwrap().len(), 1);
}

#[tokio::test]
async fn a_renamed_or_edited_event_file_is_refused() {
    let s = stores();
    let event = moved(1, "docs/b.md");
    let time = at("2026-11-04T21:40:00Z");
    s.store
        .append(StoreKind::Repo, &event, &time)
        .await
        .unwrap();
    let events = s.repo.join("events");
    let path = events.join(record_name(&event, &time));

    // Edited content: the identifier inside no longer matches.
    let text = tokio::fs::read_to_string(&path).await.unwrap();
    tokio::fs::write(&path, text.replace("docs/b.md", "docs/z.md"))
        .await
        .unwrap();
    assert!(matches!(
        s.store.events(StoreKind::Repo).await,
        Err(StoreError::Read { .. })
    ));

    // Right content, a name of another identifier.
    tokio::fs::write(&path, &text).await.unwrap();
    let other_id = events.join(format!(
        "{}.{}.json",
        "ab".repeat(32),
        RecordDigest::of(&event.id(), &time)
    ));
    tokio::fs::rename(&path, &other_id).await.unwrap();
    assert!(matches!(
        s.store.events(StoreKind::Repo).await,
        Err(StoreError::NameMismatch { .. })
    ));
}

/// The digest in the name is computed again from the identifier and the time inside:
/// a time edited by hand, with the name left alone, does not pass for the record.
#[tokio::test]
async fn a_digest_that_does_not_match_the_content_is_a_name_mismatch() {
    let s = stores();
    let event = moved(1, "docs/b.md");
    let time = at("2026-11-04T21:40:00Z");
    s.store
        .append(StoreKind::Repo, &event, &time)
        .await
        .unwrap();
    let path = s.repo.join("events").join(record_name(&event, &time));
    let text = tokio::fs::read_to_string(&path).await.unwrap();
    tokio::fs::write(
        &path,
        text.replace("2026-11-04T21:40:00Z", "2031-01-01T00:00:00Z"),
    )
    .await
    .unwrap();
    let Err(StoreError::NameMismatch {
        path: found,
        id,
        digest,
    }) = s.store.events(StoreKind::Repo).await
    else {
        panic!("a name that does not match its content is refused");
    };
    assert_eq!(found, path);
    assert_eq!(id, event.id());
    assert_eq!(digest, RecordDigest::of(&id, &at("2031-01-01T00:00:00Z")));
}

#[tokio::test]
async fn a_file_not_named_id_digest_is_refused() {
    let s = stores();
    let event = moved(1, "docs/b.md");
    let time = at("2026-11-04T21:40:00Z");
    s.store
        .append(StoreKind::Repo, &event, &time)
        .await
        .unwrap();
    let events = s.repo.join("events");
    let path = events.join(record_name(&event, &time));
    for bad in [
        // Only the identifier, as a store before the digest named it.
        format!("{}.json", event.id().hash().hex()),
        // A digest of the wrong length, upper case, an extra part.
        format!("{}.{}.json", event.id().hash().hex(), "0".repeat(15)),
        format!("{}.{}.json", event.id().hash().hex(), "A".repeat(16)),
        format!("{}.{}.x.json", event.id().hash().hex(), "0".repeat(16)),
        "notes.json".to_owned(),
    ] {
        let stray = events.join(&bad);
        tokio::fs::copy(&path, &stray).await.unwrap();
        let Err(StoreError::BadName { path: found }) = s.store.events(StoreKind::Repo).await else {
            panic!("{bad} is refused");
        };
        assert_eq!(found, stray);
        tokio::fs::remove_file(&stray).await.unwrap();
    }
    assert_eq!(s.store.events(StoreKind::Repo).await.unwrap().len(), 1);
}

/// Two files of one identifier that disagree on the event as a whole are damage, and
/// both paths are named. (The identifier of a registration leaves out the starting
/// tags, so two registrations with different tags are such a pair; real data does
/// not make one, because every registration gets a new entity identifier.)
#[tokio::test]
async fn two_files_of_one_identifier_that_differ_in_the_event_are_corrupt_and_name_both_paths() {
    let s = stores();
    let plain = registered_with(&[]);
    let tagged = registered_with(&["wip"]);
    assert_eq!(plain.id(), tagged.id());
    let (first, second) = (at("2026-11-04T21:40:00Z"), at("2026-11-04T21:41:00Z"));
    s.store
        .append(StoreKind::Repo, &plain, &first)
        .await
        .unwrap();
    s.store
        .append(StoreKind::Repo, &tagged, &second)
        .await
        .unwrap();
    let events = s.repo.join("events");
    let Err(StoreError::Corrupt { paths }) = s.store.events(StoreKind::Repo).await else {
        panic!("the pair is refused");
    };
    let mut expected = vec![
        events.join(record_name(&plain, &first)),
        events.join(record_name(&tagged, &second)),
    ];
    expected.sort();
    let mut paths = paths;
    paths.sort();
    assert_eq!(paths, expected);
}

// --- the mark of the layout ----------------------------------------------------

#[tokio::test]
async fn the_first_append_writes_the_mark_before_the_first_event() {
    let s = stores();
    // Nothing written: no directory, no mark, and an empty store.
    assert!(s.store.events(StoreKind::Repo).await.unwrap().is_empty());
    assert!(!tokio::fs::try_exists(s.repo.join("layout")).await.unwrap());

    let event = moved(1, "docs/b.md");
    s.store
        .append(StoreKind::Repo, &event, &at("2026-11-04T21:40:00Z"))
        .await
        .unwrap();
    assert_eq!(
        tokio::fs::read_to_string(s.repo.join("layout"))
            .await
            .unwrap(),
        "2\n"
    );
    assert_eq!(LAYOUT, 2);
}

#[tokio::test]
async fn a_store_with_no_mark_and_no_event_is_empty_whether_or_not_the_directory_is_there() {
    let s = stores();
    tokio::fs::create_dir_all(s.repo.join("events"))
        .await
        .unwrap();
    assert!(s.store.events(StoreKind::Repo).await.unwrap().is_empty());
    // And it can be written to: the first append marks it.
    s.store
        .append(
            StoreKind::Repo,
            &moved(1, "docs/b.md"),
            &at("2026-11-04T21:40:00Z"),
        )
        .await
        .unwrap();
    assert_eq!(s.store.events(StoreKind::Repo).await.unwrap().len(), 1);
}

/// A store of an earlier layout (events, and no mark) or of a layout this build does
/// not know is refused whole, in the store's own words, for the reads and for the
/// appends; the message points at `whyshelf migrate`.
#[tokio::test]
async fn a_store_of_another_layout_is_refused_and_the_message_points_at_migrate() {
    let s = stores();
    let event = moved(1, "docs/b.md");
    let time = at("2026-11-04T21:40:00Z");
    let events = s.repo.join("events");
    tokio::fs::create_dir_all(&events).await.unwrap();

    // Events and no mark: the layout before the mark existed.
    tokio::fs::write(
        events.join(format!("{}.json", event.id().hash().hex())),
        "{}",
    )
    .await
    .unwrap();
    let Err(error) = s.store.events(StoreKind::Repo).await else {
        panic!("an unmarked store with events is refused");
    };
    assert!(matches!(
        error,
        StoreError::Layout {
            found: Found::Version(1),
            reads: LAYOUT
        }
    ));
    assert!(error.to_string().contains("whyshelf migrate"), "{error}");
    assert!(matches!(
        s.store.append(StoreKind::Repo, &event, &time).await,
        Err(StoreError::Layout { .. })
    ));

    // A mark of a number this build does not know, and one that is no number.
    for (mark, found) in [("3\n", Found::Version(3)), ("two\n", Found::Unknown)] {
        tokio::fs::write(s.repo.join("layout"), mark).await.unwrap();
        let Err(error) = s.store.events(StoreKind::Repo).await else {
            panic!("a mark of {mark:?} is refused");
        };
        assert!(
            matches!(&error, StoreError::Layout { found: f, reads } if *f == found && *reads == LAYOUT),
            "{error}"
        );
        assert!(error.to_string().contains("whyshelf migrate"), "{error}");
        assert!(matches!(
            s.store.append(StoreKind::Repo, &event, &time).await,
            Err(StoreError::Layout { .. })
        ));
    }
    // Nothing was written to the refused store.
    assert!(json_files(&events).await.len() == 1);
}

// --- two branches of Git ------------------------------------------------------

/// A Git repository whose `.whyshelf/` is a store.
async fn git_store() -> (tempfile::TempDir, FsStore) {
    let dir = tempfile::tempdir().unwrap();
    init_repo(dir.path()).await;
    write(dir.path(), "README", "a project\n").await;
    commit_all(dir.path(), "Start").await;
    let store = FsStore::new(dir.path().join(".whyshelf"), dir.path().join("private"));
    (dir, store)
}

/// Two branches each record the same change at their own time and are merged: Git
/// has nothing to resolve, because the two files have different names (and the mark
/// both wrote first is the same bytes). Read after the merge, it is one event with
/// both times.
#[tokio::test]
async fn two_branches_recording_the_same_change_merge_without_a_conflict() {
    let (dir, store) = git_store().await;
    let root = dir.path();
    let event = moved(1, "docs/b.md");
    let (on_a, on_b) = (
        at("2026-11-04T21:40:00+09:00"),
        at("2026-11-04T22:10:00+09:00"),
    );

    git(root, &["checkout", "-q", "-b", "a"]).await;
    store.append(StoreKind::Repo, &event, &on_a).await.unwrap();
    commit_all(root, "Record on a").await;
    git(root, &["checkout", "-q", "main"]).await;
    git(root, &["checkout", "-q", "-b", "b"]).await;
    store.append(StoreKind::Repo, &event, &on_b).await.unwrap();
    commit_all(root, "Record on b").await;

    git(root, &["checkout", "-q", "a"]).await;
    git(root, &["merge", "-q", "--no-edit", "b"]).await;

    let merged = store.events(StoreKind::Repo).await.unwrap();
    assert_eq!(merged.len(), 1, "one event");
    assert_eq!(merged[0].event(), &event);
    let times: BTreeSet<&str> = merged[0].at().iter().map(Timestamp::as_str).collect();
    assert_eq!(times, BTreeSet::from([on_a.as_str(), on_b.as_str()]));
    let files = json_files(&root.join(".whyshelf/events")).await;
    assert_eq!(files.len(), 2);
}

/// The mark is written by the first append of each branch, and both write the same
/// bytes: Git joins two additions of one content without a conflict.
#[tokio::test]
async fn the_mark_two_branches_made_alike_merges_without_a_conflict() {
    let (dir, store) = git_store().await;
    let root = dir.path();
    let time = at("2026-11-04T21:40:00Z");

    git(root, &["checkout", "-q", "-b", "a"]).await;
    store
        .append(StoreKind::Repo, &moved(1, "docs/b.md"), &time)
        .await
        .unwrap();
    commit_all(root, "First event on a").await;
    git(root, &["checkout", "-q", "main"]).await;
    git(root, &["checkout", "-q", "-b", "b"]).await;
    store
        .append(StoreKind::Repo, &moved(2, "docs/c.md"), &time)
        .await
        .unwrap();
    commit_all(root, "First event on b").await;

    git(root, &["checkout", "-q", "a"]).await;
    git(root, &["merge", "-q", "--no-edit", "b"]).await;
    assert_eq!(
        tokio::fs::read_to_string(root.join(".whyshelf/layout"))
            .await
            .unwrap(),
        "2\n"
    );
    assert_eq!(store.events(StoreKind::Repo).await.unwrap().len(), 2);
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
