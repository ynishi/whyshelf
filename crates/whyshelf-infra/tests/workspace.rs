//! The whole stack: a `Workspace` over the real adapters, in a real Git repository
//! and a temporary private store.

mod common;

use std::collections::BTreeSet;

use common::{Project, commit_all, finding_text, json_files, project, spec_text, write};
use serde_json::{Value, json};
use uuid::Uuid;
use whyshelf_core::app::Workspace;
use whyshelf_core::app::ports::{AppendOutcome, ExportKind, IndexCachePort, Ports, StorePort};
use whyshelf_core::app::register::AddOptions;
use whyshelf_core::app::view::ExportProblem;
use whyshelf_core::domain::actor::Timestamp;
use whyshelf_core::domain::event::{
    DocKind, DocRegistration, Event, EventBody, Publication, Registration, StoreKind, Version,
};
use whyshelf_core::domain::finding::FindingType;
use whyshelf_core::domain::hash::ContentHash;
use whyshelf_core::domain::id::EntityId;
use whyshelf_core::domain::location::RelPath;
use whyshelf_core::domain::nonempty::NonBlank;
use whyshelf_core::domain::reference::Ref;
use whyshelf_infra::export::{render_json, split_marker, with_marker};
use whyshelf_infra::store_fs::FsStore;
use whyshelf_infra::{InfraPorts, Shutdown, open, open_with};

fn rel(text: &str) -> RelPath {
    text.parse().unwrap()
}

fn refer(text: &str) -> Ref {
    text.parse().unwrap()
}

async fn read(project: &Project, rel: &str) -> String {
    tokio::fs::read_to_string(project.root().join(rel))
        .await
        .unwrap()
}

async fn exists(path: &std::path::Path) -> bool {
    tokio::fs::try_exists(path).await.unwrap()
}

async fn opened(project: &Project) -> (Workspace<InfraPorts>, Shutdown) {
    open_with(project.root(), &project.dirs()).await.unwrap()
}

const LOGIN: &str = "docs/finding/slow-login.md";

async fn with_one_finding(extra: &str) -> Project {
    let p = project(extra).await;
    write(
        p.root(),
        LOGIN,
        &finding_text("Login feels slow", "It takes eight seconds."),
    )
    .await;
    commit_all(p.root(), "Add the finding").await;
    p
}

fn alias(text: &str) -> AddOptions {
    AddOptions::new().with_alias(text.parse().unwrap())
}

#[tokio::test]
async fn registering_and_editing_records_history_and_keeps_cache_and_exports_current() {
    let p = with_one_finding("").await;
    let (mut ws, shutdown) = opened(&p).await;

    ws.add(&[rel("docs")], &alias("finding/slow-login"))
        .await
        .unwrap();
    let events = p.root().join(".whyshelf/events");
    let objects = p.root().join(".whyshelf/objects");
    assert_eq!(json_files(&events).await.len(), 1);
    assert_eq!(common::all_files(&objects).await.len(), 1);
    let map_after_add = read(&p, ".whyshelf/map.json").await;
    assert!(map_after_add.contains("\"alias\": \"finding/slow-login\""));
    assert!(exists(&p.root().join(".whyshelf/map.schema.json")).await);
    assert!(exists(&p.root().join(".whyshelf/cache/index.db")).await);

    // The cache was told, and searches the text.
    let found = ws
        .ports()
        .cache()
        .search_fts(&NonBlank::new("eight").unwrap(), 5)
        .await
        .unwrap();
    assert_eq!(found, vec![rel(LOGIN)]);

    // Editing the file is history after the next ingest: a Revised event and a new object.
    write(
        p.root(),
        LOGIN,
        &finding_text("Login feels slow", "It takes nine seconds."),
    )
    .await;
    let report = ws.ingest().await.unwrap();
    assert_eq!(report.revised().len(), 1);
    assert_eq!(json_files(&events).await.len(), 2);
    assert_eq!(common::all_files(&objects).await.len(), 2);
    assert_eq!(
        read(&p, ".whyshelf/map.json").await,
        map_after_add,
        "the map has no content in it, so editing the text does not change it"
    );
    let now = ws
        .ports()
        .cache()
        .search_fts(&NonBlank::new("nine").unwrap(), 5)
        .await
        .unwrap();
    assert_eq!(now, vec![rel(LOGIN)]);

    // Nothing changed: nothing is written.
    let report = ws.ingest().await.unwrap();
    assert!(report.revised().is_empty());
    assert_eq!(json_files(&events).await.len(), 2);

    shutdown.close().await.unwrap();
}

#[tokio::test]
async fn a_file_that_does_not_parse_is_recorded_as_a_version_of_its_own() {
    let p = with_one_finding("").await;
    let (mut ws, shutdown) = opened(&p).await;
    ws.add(&[rel("docs")], &alias("finding/slow-login"))
        .await
        .unwrap();

    let conflicted = "---\nfinding_type: usage_observation\n<<<<<<< HEAD\ntitle: a\n=======\ntitle: b\n>>>>>>> topic\n---\nx\n";
    write(p.root(), LOGIN, conflicted).await;
    let report = ws.ingest().await.unwrap();
    assert_eq!(report.revised().len(), 1);
    // The text, conflict markers and all, is an object like any other.
    let objects = common::all_files(&p.root().join(".whyshelf/objects")).await;
    assert_eq!(objects.len(), 2);
    shutdown.close().await.unwrap();
}

#[tokio::test]
async fn the_generated_files_carry_a_marker_and_drift_is_reported() {
    let p = with_one_finding("[export]\nstatus_md = true\nchangelog_md = true\n").await;
    let (mut ws, shutdown) = opened(&p).await;
    ws.add(&[rel("docs")], &alias("finding/slow-login"))
        .await
        .unwrap();

    for name in ["map.json", "status.md", "changelog.md"] {
        let text = read(&p, &format!(".whyshelf/{name}")).await;
        assert!(
            text.contains("generated by whyshelf; do not edit"),
            "{name} has a marker"
        );
    }
    let status = read(&p, ".whyshelf/status.md").await;
    assert!(status.starts_with("<!-- generated by whyshelf; do not edit; content: sha256:"));
    assert!(status.contains("finding/slow-login"));
    assert!(
        !status.contains(&p.root().display().to_string()),
        "the machine's directory is not committed"
    );
    assert!(
        read(&p, ".whyshelf/changelog.md")
            .await
            .contains("Registered")
    );
    assert!(ws.status().await.unwrap().exports().is_empty());

    // Edited by hand.
    let map = p.root().join(".whyshelf/map.json");
    let edited = tokio::fs::read_to_string(&map)
        .await
        .unwrap()
        .replace("slow-login", "fast-login");
    tokio::fs::write(&map, edited).await.unwrap();
    let notes = ws.status().await.unwrap().exports().to_vec();
    assert_eq!(notes.len(), 1);
    assert_eq!(
        (notes[0].scope, notes[0].kind, notes[0].problem),
        (
            StoreKind::Repo,
            ExportKind::Map,
            ExportProblem::EditedByHand
        )
    );

    // Not marked at all counts as edited.
    tokio::fs::write(&map, "{}\n").await.unwrap();
    assert_eq!(
        ws.status().await.unwrap().exports()[0].problem,
        ExportProblem::EditedByHand
    );

    // Consistent with its marker, but behind the sources (as after a merge).
    let stale = with_marker(
        ExportKind::Map,
        &render_json(&json!({"entities": {}, "version": 1})),
    );
    tokio::fs::write(&map, stale).await.unwrap();
    assert_eq!(
        ws.status().await.unwrap().exports()[0].problem,
        ExportProblem::Outdated
    );

    // Missing.
    tokio::fs::remove_file(&map).await.unwrap();
    assert_eq!(
        ws.status().await.unwrap().exports()[0].problem,
        ExportProblem::Missing
    );

    // A rebuild writes it again.
    ws.rebuild().await.unwrap();
    assert!(ws.status().await.unwrap().exports().is_empty());
    shutdown.close().await.unwrap();
}

#[tokio::test]
async fn private_entities_stay_out_of_the_committed_files() {
    let p = with_one_finding("[export]\nstatus_md = true\nchangelog_md = true\n").await;
    write(
        p.root(),
        "docs/finding/secret-plan.md",
        &finding_text("A plan not for publication", "Only for us."),
    )
    .await;
    let (mut ws, shutdown) = opened(&p).await;
    ws.add(&[rel(LOGIN)], &alias("finding/slow-login"))
        .await
        .unwrap();
    ws.add(
        &[rel("docs/finding/secret-plan.md")],
        &alias("finding/secret-plan").with_store(StoreKind::Private),
    )
    .await
    .unwrap();

    for name in ["map.json", "status.md", "changelog.md"] {
        let public = read(&p, &format!(".whyshelf/{name}")).await;
        assert!(!public.contains("secret-plan"), "{name}");
        assert!(!public.contains("not for publication"), "{name}");
    }
    let private_map = tokio::fs::read_to_string(p.private().join("map.json"))
        .await
        .unwrap();
    assert!(private_map.contains("finding/secret-plan"));
    assert_eq!(json_files(&p.private().join("events")).await.len(), 1);
    assert_eq!(
        json_files(&p.root().join(".whyshelf/events")).await.len(),
        1
    );
    shutdown.close().await.unwrap();
}

#[tokio::test]
async fn publishing_writes_an_event_in_each_store() {
    let p = with_one_finding("").await;
    let (mut ws, shutdown) = opened(&p).await;
    ws.add(
        &[rel(LOGIN)],
        &alias("finding/slow-login").with_store(StoreKind::Private),
    )
    .await
    .unwrap();
    assert!(
        json_files(&p.root().join(".whyshelf/events"))
            .await
            .is_empty()
    );

    ws.publish(&refer("finding/slow-login")).await.unwrap();
    let store = FsStore::new(p.root().join(".whyshelf"), p.private().to_path_buf());
    let repo = store.events(StoreKind::Repo).await.unwrap();
    assert!(matches!(
        repo.as_slice(),
        [e] if matches!(e.body(), EventBody::Published(Publication::Incoming { .. })) && e.heads().is_empty()
    ));
    let private = store.events(StoreKind::Private).await.unwrap();
    assert!(
        private
            .iter()
            .any(|e| matches!(e.body(), EventBody::Published(Publication::Outgoing { .. })))
    );
    assert!(
        read(&p, ".whyshelf/map.json")
            .await
            .contains("finding/slow-login")
    );
    shutdown.close().await.unwrap();
}

#[tokio::test]
async fn a_scaffold_makes_a_file_that_asks_to_be_filled_in_and_is_never_overwritten() {
    let p = project("").await;
    let (mut ws, shutdown) = opened(&p).await;
    let title = NonBlank::new("Login feels slow").unwrap();
    let made = ws
        .finding_new(FindingType::UsageObservation, title.clone(), None)
        .await
        .unwrap();
    assert_eq!(made.path().as_str(), "docs/finding/login-feels-slow.md");
    let text = read(&p, "docs/finding/login-feels-slow.md").await;
    assert!(text.contains("finding_type: usage_observation"));
    assert!(text.contains("title: \"Login feels slow\""));

    // The person's edit survives a second scaffold, which is refused.
    write(p.root(), "docs/finding/login-feels-slow.md", "mine\n").await;
    assert!(
        ws.finding_new(FindingType::UsageObservation, title, None)
            .await
            .is_err()
    );
    assert_eq!(read(&p, "docs/finding/login-feels-slow.md").await, "mine\n");
    shutdown.close().await.unwrap();
}

#[tokio::test]
async fn two_workspaces_ingesting_the_same_change_write_one_event() {
    let p = with_one_finding("").await;
    let (mut first, first_down) = opened(&p).await;
    let (mut second, second_down) = opened(&p).await;
    first
        .add(&[rel("docs")], &alias("finding/slow-login"))
        .await
        .unwrap();
    let events = p.root().join(".whyshelf/events");
    assert_eq!(json_files(&events).await.len(), 1);

    write(
        p.root(),
        LOGIN,
        &finding_text("Login feels slow", "It now takes nine whole seconds."),
    )
    .await;
    let (a, b) = tokio::join!(first.ingest(), second.ingest());
    a.unwrap();
    b.unwrap();

    // One Registered and one Revised; the second process wrote nothing new.
    let files = json_files(&events).await;
    assert_eq!(files.len(), 2, "{files:?}");
    assert_eq!(
        common::all_files(&events).await,
        files,
        "no temporary files are left"
    );
    assert_eq!(
        common::all_files(&p.root().join(".whyshelf/objects"))
            .await
            .len(),
        2
    );

    // And both see the same history.
    let again = tokio::join!(first.ingest(), second.ingest());
    assert!(again.0.unwrap().revised().is_empty());
    assert!(again.1.unwrap().revised().is_empty());
    first_down.close().await.unwrap();
    second_down.close().await.unwrap();
}

#[tokio::test]
async fn opening_clears_the_record_of_a_checkout_that_is_gone() {
    let p = project("").await;
    let stale = p.root().join(".whyshelf/cache/rev-stale");
    let head = common::git(p.root(), &["rev-parse", "HEAD"]).await;
    common::git(
        p.root(),
        &[
            "worktree",
            "add",
            "-q",
            "--detach",
            stale.to_str().unwrap(),
            &head,
        ],
    )
    .await;
    tokio::fs::remove_dir_all(&stale).await.unwrap();
    assert_eq!(
        common::git(p.root(), &["worktree", "list"])
            .await
            .lines()
            .count(),
        2
    );

    let (_ws, shutdown) = opened(&p).await;
    assert_eq!(
        common::git(p.root(), &["worktree", "list"])
            .await
            .lines()
            .count(),
        1
    );
    shutdown.close().await.unwrap();
}

#[tokio::test]
async fn opening_without_settings_or_outside_a_repository_is_an_error() {
    let dir = tempfile::tempdir().unwrap();
    assert!(open(dir.path()).await.is_err(), "no settings file");
    write(
        dir.path(),
        ".whyshelf.toml",
        "version = 1\nproject = \"demo\"\n[test]\ncommand = \"true\"\n",
    )
    .await;
    assert!(open(dir.path()).await.is_err(), "not a Git repository");
    assert!(
        open(&dir.path().join("missing")).await.is_err(),
        "no such directory"
    );
}

// ---- the map ---------------------------------------------------------------

fn entity(n: u128) -> EntityId {
    let bits = (n & 0x0fff_ffff_ffff_ffff) | (0x7u128 << 76) | (0b10u128 << 62);
    EntityId::new(Uuid::from_u128(bits)).unwrap()
}

fn digest(text: &str) -> ContentHash {
    ContentHash::raw(text.as_bytes())
}

fn registered(id: u128, kind: DocKind, path: &str, alias: &str, tags: &[&str]) -> Event {
    Event::new(
        entity(id),
        BTreeSet::new(),
        Timestamp::from_validated("2026-11-04T21:40:00Z".to_owned()).unwrap(),
        EventBody::Registered(Registration::Doc(DocRegistration::new(
            kind,
            path.parse().unwrap(),
            Some(alias.parse().unwrap()),
            None,
            tags.iter().map(|t| t.parse().unwrap()).collect(),
            Version::Parsed {
                content: digest(&format!("{alias}/content")),
                raw: digest(&format!("{alias}/raw")),
            },
        ))),
    )
    .unwrap()
}

/// A project whose repo store was written by hand with fixed identifiers, so that
/// its map is the same bytes every time.
async fn fixed_project() -> (Project, Workspace<InfraPorts>, Shutdown) {
    let p = project("").await;
    write(
        p.root(),
        LOGIN,
        &finding_text("Login feels slow", "It takes eight seconds."),
    )
    .await;
    let hidden = entity(0x99);
    write(
        p.root(),
        "docs/spec/session-reuse.md",
        &spec_text(
            &["finding/slow-login", &format!("private:{hidden}")],
            "The session is reused.",
        ),
    )
    .await;
    let store = FsStore::new(p.root().join(".whyshelf"), p.private().to_path_buf());
    for event in [
        registered(
            0x11,
            DocKind::Finding,
            LOGIN,
            "finding/slow-login",
            &["area:auth", "wip"],
        ),
        registered(
            0x22,
            DocKind::Spec,
            "docs/spec/session-reuse.md",
            "spec/session-reuse",
            &[],
        ),
    ] {
        assert_eq!(
            store.append(StoreKind::Repo, &event).await.unwrap(),
            AppendOutcome::Written
        );
    }
    let (mut ws, shutdown) = open_with(p.root(), &p.dirs()).await.unwrap();
    ws.rebuild().await.unwrap();
    (p, ws, shutdown)
}

/// The map this fixture must render, byte for byte.
const GOLDEN_BODY: &str = "{\n  \"entities\": {\n    \"00000000-0000-7000-8000-000000000011\": {\n      \"alias\": \"finding/slow-login\",\n      \"kind\": \"finding\",\n      \"path\": \"docs/finding/slow-login.md\",\n      \"refs\": {\n        \"external\": [],\n        \"grounds\": [],\n        \"upstream\": []\n      },\n      \"tags\": {\n        \"extended\": [\n          \"wip\"\n        ],\n        \"official\": [\n          \"area:auth\"\n        ]\n      },\n      \"title\": \"Login feels slow\"\n    },\n    \"00000000-0000-7000-8000-000000000022\": {\n      \"alias\": \"spec/session-reuse\",\n      \"kind\": \"spec\",\n      \"path\": \"docs/spec/session-reuse.md\",\n      \"refs\": {\n        \"external\": [\n          \"private:00000000-0000-7000-8000-000000000099\"\n        ],\n        \"grounds\": [\n          \"00000000-0000-7000-8000-000000000011\"\n        ],\n        \"upstream\": []\n      },\n      \"tags\": {\n        \"extended\": [],\n        \"official\": []\n      }\n    }\n  },\n  \"version\": 1\n}\n";

/// The `export` hash of [`GOLDEN_BODY`], the content of the marker.
const GOLDEN_HASH: &str = "sha256:47769199bb327369e48d2fa3791f7da9b1b0026f91d46a3cc1b51d9584380e56";

#[tokio::test]
async fn the_map_is_the_same_bytes_for_the_same_model() {
    let (p, ws, shutdown) = fixed_project().await;
    let file = read(&p, ".whyshelf/map.json").await;
    let (marker, body) = split_marker(ExportKind::Map, file.as_bytes()).unwrap();
    assert_eq!(body, GOLDEN_BODY);
    assert_eq!(marker.to_string(), GOLDEN_HASH);
    assert_eq!(marker, ContentHash::export(GOLDEN_BODY.as_bytes()));
    assert!(file.starts_with(
        "{\n  \"generated\": {\n    \"note\": \"generated by whyshelf; do not edit\","
    ));

    // Rendering again from scratch, and in a second workspace, gives the same file.
    drop(ws);
    shutdown.close().await.unwrap();
    tokio::fs::remove_file(p.root().join(".whyshelf/map.json"))
        .await
        .unwrap();
    let (mut again, shutdown) = open_with(p.root(), &p.dirs()).await.unwrap();
    again.rebuild().await.unwrap();
    assert_eq!(read(&p, ".whyshelf/map.json").await, file);
    shutdown.close().await.unwrap();
}

/// A small JSON Schema checker: what the map schema uses (`$ref`, `type`,
/// `properties`, `required`, `additionalProperties` as a schema, `items`).
fn check(root: &Value, schema: &Value, value: &Value, at: &str, errors: &mut Vec<String>) {
    if let Some(target) = schema.get("$ref").and_then(Value::as_str) {
        let pointer = target.strip_prefix('#').unwrap();
        return check(root, root.pointer(pointer).unwrap(), value, at, errors);
    }
    if let Some(types) = schema.get("type") {
        let names: Vec<&str> = match types {
            Value::String(name) => vec![name.as_str()],
            Value::Array(items) => items.iter().filter_map(Value::as_str).collect(),
            _ => Vec::new(),
        };
        let fits = names.iter().any(|name| match *name {
            "object" => value.is_object(),
            "array" => value.is_array(),
            "string" => value.is_string(),
            "integer" => value.is_u64() || value.is_i64(),
            "null" => value.is_null(),
            "boolean" => value.is_boolean(),
            _ => false,
        });
        if !fits {
            errors.push(format!("{at}: expected {names:?}"));
            return;
        }
    }
    if let Value::Object(map) = value {
        let properties = schema.get("properties").and_then(Value::as_object);
        for key in schema
            .get("required")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            let key = key.as_str().unwrap();
            if !map.contains_key(key) {
                errors.push(format!("{at}: the required key `{key}` is missing"));
            }
        }
        for (key, item) in map {
            let path = format!("{at}/{key}");
            match (
                properties.and_then(|p| p.get(key)),
                schema.get("additionalProperties"),
            ) {
                (Some(sub), _) => check(root, sub, item, &path, errors),
                (None, Some(extra)) if extra.is_object() => check(root, extra, item, &path, errors),
                (None, _) if properties.is_some() => {
                    errors.push(format!("{path}: the key is not in the schema"));
                }
                (None, _) => {}
            }
        }
    }
    if let (Value::Array(items), Some(sub)) = (value, schema.get("items")) {
        for (index, item) in items.iter().enumerate() {
            check(root, sub, item, &format!("{at}[{index}]"), errors);
        }
    }
}

#[tokio::test]
async fn the_map_conforms_to_the_schema_that_is_written_next_to_it() {
    let (p, _ws, shutdown) = fixed_project().await;
    let map: Value = serde_json::from_str(&read(&p, ".whyshelf/map.json").await).unwrap();
    let schema_text = read(&p, ".whyshelf/map.schema.json").await;
    let schema: Value = serde_json::from_str(&schema_text).unwrap();

    let mut errors = Vec::new();
    check(&schema, &schema, &map, "", &mut errors);
    assert!(errors.is_empty(), "{errors:?}");

    // The schema asks for these keys, and the output has them.
    let required = |name: &str| -> Vec<String> {
        schema["$defs"][name]["required"]
            .as_array()
            .unwrap()
            .iter()
            .map(|k| k.as_str().unwrap().to_owned())
            .collect()
    };
    assert_eq!(required("MapEntityV1"), ["kind", "tags", "refs"]);
    assert_eq!(required("MapTagsV1"), ["official", "extended"]);
    assert_eq!(required("MapRefsV1"), ["grounds", "upstream", "external"]);
    let root_required: Vec<&str> = schema["required"]
        .as_array()
        .unwrap()
        .iter()
        .map(|k| k.as_str().unwrap())
        .collect();
    assert_eq!(root_required, ["version", "entities"]);
    for key in &root_required {
        assert!(map.get(key).is_some());
    }

    // A map that lacks a required key does not conform.
    let mut broken = map.clone();
    broken["entities"]["00000000-0000-7000-8000-000000000011"]
        .as_object_mut()
        .unwrap()
        .remove("refs");
    let mut errors = Vec::new();
    check(&schema, &schema, &broken, "", &mut errors);
    assert_eq!(errors.len(), 1, "{errors:?}");

    // The schema file is what `schema_json` renders, deterministically.
    assert_eq!(schema_text, whyshelf_infra::map_dto::schema_json().unwrap());
    shutdown.close().await.unwrap();
}
