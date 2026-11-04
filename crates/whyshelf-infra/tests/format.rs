//! The census of versions against real directories: what `whyshelf migrate` looks at.

mod common;

use std::path::Path;

use common::{PROJECT_ID, project, write};
use whyshelf_core::domain::event::StoreKind;
use whyshelf_infra::config::UserDirs;
use whyshelf_infra::format::{
    Census, FormatError, Found, Projection, Source, UnplacedReason, Verdict, census,
};

const REPO: Source = Source::Event(StoreKind::Repo);
const PRIVATE: Source = Source::Event(StoreKind::Private);

fn v(n: u32) -> Found {
    Found::Version(n)
}

fn counted(census: &Census, source: Source) -> Vec<(Found, usize)> {
    census.sources()[&source]
        .iter()
        .map(|(found, count)| (*found, *count))
        .collect()
}

#[tokio::test]
async fn a_project_of_version_one_files_is_readable() {
    let p = project("").await;
    write(p.root(), ".whyshelf/events/aa.json", "{\"v\": 1}\n").await;
    write(
        p.root(),
        ".whyshelf/events/bb.json",
        "{\"v\": 1, \"more\": []}\n",
    )
    .await;
    write(&p.private(), "events/cc.json", "{\"v\": 1}\n").await;
    write(
        p.root(),
        ".whyshelf/map.json",
        "{\"version\": 1, \"entities\": {}}\n",
    )
    .await;

    let census = census(p.root(), &p.dirs()).await.unwrap();
    assert_eq!(counted(&census, Source::Settings), [(v(1), 1)]);
    assert_eq!(counted(&census, REPO), [(v(1), 2)]);
    assert_eq!(counted(&census, PRIVATE), [(v(1), 1)]);
    assert_eq!(census.projections()[&Projection::Map].get(&v(1)), Some(&1));
    assert_eq!(census.verdict(), Verdict::Readable);
}

#[tokio::test]
async fn an_event_file_of_version_two_makes_it_unreadable_and_is_counted_as_it_is() {
    let p = project("").await;
    write(p.root(), ".whyshelf/events/aa.json", "{\"v\": 1}\n").await;
    write(p.root(), ".whyshelf/events/bb.json", "{\"v\": 2}\n").await;
    write(p.root(), ".whyshelf/events/cc.json", "{\"v\": 2}\n").await;

    let census = census(p.root(), &p.dirs()).await.unwrap();
    assert_eq!(counted(&census, REPO), [(v(1), 1), (v(2), 2)]);
    let Verdict::Unreadable(groups) = census.verdict() else {
        panic!("unreadable");
    };
    assert_eq!(groups.len(), 1);
    assert_eq!(
        (
            groups.first().source,
            groups.first().found,
            groups.first().count.get()
        ),
        (REPO, v(2), 2)
    );
}

#[tokio::test]
async fn a_file_with_no_version_and_a_file_that_is_no_json_are_counted_as_unknown() {
    let p = project("").await;
    write(
        p.root(),
        ".whyshelf/events/aa.json",
        "{\"no\": \"version\"}\n",
    )
    .await;
    write(p.root(), ".whyshelf/events/bb.json", "this is not json\n").await;
    write(p.root(), ".whyshelf/events/cc.json", "{\"v\": \"1\"}\n").await;
    write(
        p.root(),
        ".whyshelf/events/notes.txt",
        "not an event file\n",
    )
    .await;

    let census = census(p.root(), &p.dirs()).await.unwrap();
    assert_eq!(counted(&census, REPO), [(Found::Unknown, 3)]);
    assert!(matches!(census.verdict(), Verdict::Unreadable(_)));
}

/// A cache database of a version, made by hand so that the census is not the one that
/// writes it.
async fn write_cache(path: &Path, version: &str) {
    tokio::fs::create_dir_all(path.parent().unwrap())
        .await
        .unwrap();
    let conn = rusqlite::Connection::open(path).unwrap();
    conn.execute_batch("CREATE TABLE meta (key TEXT PRIMARY KEY, value TEXT NOT NULL)")
        .unwrap();
    conn.execute(
        "INSERT INTO meta (key, value) VALUES ('schema_version', ?1)",
        [version],
    )
    .unwrap();
}

#[tokio::test]
async fn an_old_map_and_an_old_cache_are_shown_and_do_not_make_the_data_unreadable() {
    let p = project("").await;
    write(p.root(), ".whyshelf/events/aa.json", "{\"v\": 1}\n").await;
    write(
        p.root(),
        ".whyshelf/map.json",
        "{\"version\": 0, \"entities\": {}}\n",
    )
    .await;
    write(&p.private(), "map.json", "{\"version\": 3}\n").await;
    let cache = p.root().join(".whyshelf/cache/index.db");
    write_cache(&cache, "0").await;

    let census = census(p.root(), &p.dirs()).await.unwrap();
    assert_eq!(census.verdict(), Verdict::Readable);
    let map = &census.projections()[&Projection::Map];
    assert_eq!((map.get(&v(0)), map.get(&v(3))), (Some(&1), Some(&1)));
    assert_eq!(
        census.projections()[&Projection::Cache].get(&v(0)),
        Some(&1)
    );
}

#[tokio::test]
async fn a_cache_that_is_no_database_is_counted_as_unknown_and_judges_nothing() {
    let p = project("").await;
    write(
        p.root(),
        ".whyshelf/cache/index.db",
        "not a database at all\n",
    )
    .await;
    let census = census(p.root(), &p.dirs()).await.unwrap();
    assert_eq!(
        census.projections()[&Projection::Cache].get(&Found::Unknown),
        Some(&1)
    );
    assert_eq!(census.verdict(), Verdict::Readable);
}

#[tokio::test]
async fn the_census_does_not_change_the_cache() {
    let p = project("").await;
    let cache = p.root().join(".whyshelf/cache/index.db");
    write_cache(&cache, "0").await;
    let before = tokio::fs::read(&cache).await.unwrap();
    let siblings_before = common::all_files(cache.parent().unwrap()).await;

    census(p.root(), &p.dirs()).await.unwrap();
    census(p.root(), &p.dirs()).await.unwrap();

    // Not a byte, and no `-wal` or `-shm` file beside it: the normal way of opening
    // would have migrated the version 0 away.
    assert_eq!(tokio::fs::read(&cache).await.unwrap(), before);
    assert_eq!(
        common::all_files(cache.parent().unwrap()).await,
        siblings_before
    );
}

async fn look(p: &common::Project) -> (Census, Vec<UnplacedReason>) {
    let census = census(p.root(), &p.dirs()).await.unwrap();
    let reasons = census.unplaced().iter().map(|u| u.reason).collect();
    (census, reasons)
}

#[tokio::test]
async fn settings_of_another_version_or_without_an_id_are_counted_and_the_private_store_is_not_looked_at()
 {
    // Version 2 of the settings: counted as it is, and what it says of the project is
    // not relied on.
    let p = project("").await;
    let settings = tokio::fs::read_to_string(p.root().join(".whyshelf.toml"))
        .await
        .unwrap();
    write(
        p.root(),
        ".whyshelf.toml",
        &settings.replace("version = 1", "version = 2"),
    )
    .await;
    write(p.root(), ".whyshelf/events/aa.json", "{\"v\": 1}\n").await;
    let (census, reasons) = look(&p).await;
    assert_eq!(counted(&census, Source::Settings), [(v(2), 1)]);
    assert_eq!(reasons, [UnplacedReason::SettingsUnreadable]);
    assert!(!census.sources().contains_key(&PRIVATE));
    let Verdict::Incomplete {
        unplaced,
        unreadable,
    } = census.verdict()
    else {
        panic!("incomplete");
    };
    assert_eq!(unplaced.first().source, PRIVATE);
    assert_eq!(
        unreadable.len(),
        1,
        "the settings of version 2 are listed too"
    );

    // No `project_id`.
    let p = project("").await;
    let settings = tokio::fs::read_to_string(p.root().join(".whyshelf.toml"))
        .await
        .unwrap();
    let without = settings.replace(&format!("project_id = \"{PROJECT_ID}\"\n"), "");
    assert_ne!(without, settings);
    write(p.root(), ".whyshelf.toml", &without).await;
    let (census, reasons) = look(&p).await;
    assert_eq!(reasons, [UnplacedReason::NoProjectId]);
    assert!(matches!(census.verdict(), Verdict::Incomplete { .. }));
    assert_eq!(
        counted(&census, REPO),
        [],
        "the repo store is still looked at"
    );

    // A settings file that is not TOML at all.
    let p = project("").await;
    write(p.root(), ".whyshelf.toml", "version = [[[\n").await;
    let (census, reasons) = look(&p).await;
    assert_eq!(reasons, [UnplacedReason::SettingsUnreadable]);
    assert_eq!(counted(&census, Source::Settings), [(Found::Unknown, 1)]);
}

#[tokio::test]
async fn a_clone_without_a_private_store_is_readable_the_store_is_there_to_be_empty() {
    let p = project("").await;
    write(p.root(), ".whyshelf/events/aa.json", "{\"v\": 1}\n").await;
    assert!(!tokio::fs::try_exists(p.private()).await.unwrap());
    let census = census(p.root(), &p.dirs()).await.unwrap();
    assert!(census.unplaced().is_empty());
    assert_eq!(counted(&census, PRIVATE), [], "looked at, and empty");
    assert_eq!(census.verdict(), Verdict::Readable);
}

#[tokio::test]
async fn a_directory_without_settings_is_no_project_and_a_data_dir_that_cannot_be_used_is_incomplete()
 {
    let empty = tempfile::tempdir().unwrap();
    assert!(matches!(
        census(empty.path(), &UserDirs::default().with_data_dir("/data")).await,
        Err(FormatError::NotAProject { .. })
    ));

    let p = project("").await;
    // A relative data dir is an error of the environment, not of the data.
    let relative = UserDirs::default().with_data_dir("relative");
    let census = census(p.root(), &relative).await.unwrap();
    assert_eq!(
        census
            .unplaced()
            .iter()
            .map(|u| u.reason)
            .collect::<Vec<_>>(),
        [UnplacedReason::DataDirUnusable]
    );
    assert!(matches!(census.verdict(), Verdict::Incomplete { .. }));
    let nothing = UserDirs::default().with_home("relative");
    let census = self::census(p.root(), &nothing).await.unwrap();
    assert!(matches!(census.verdict(), Verdict::Incomplete { .. }));
}
