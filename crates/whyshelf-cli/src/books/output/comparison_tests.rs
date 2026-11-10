//! Tests of the JSON of a change of comparison, and of the diff of a file whose
//! `lines` hash is the same, from a shelf built by the domain.

use std::collections::{BTreeMap, BTreeSet};

use whyshelf_core::app::shelf::{DiffItem, Names};
use whyshelf_core::domain::actor::Actor;
use whyshelf_core::domain::id::{DocId, EntityId};
use whyshelf_core::domain::location::RelPath;
use whyshelf_core::domain::shelf::artifact::{
    ArtifactAsks, ArtifactReading, ArtifactReadings, Space,
};
use whyshelf_core::domain::shelf::build::{DocReading, RawValue, Shelf};
use whyshelf_core::domain::shelf::freshness::assess;
use whyshelf_core::domain::shelf::ledger::{
    Decision, JudgedAt, JudgementId, Ledger, frame_approve,
};
use whyshelf_core::domain::shelf::schema::{
    BookSchemaDecl, Cardinality, CompareDecl, FieldEntryDecl, FieldPresence, FieldTypeDecl,
    KeyPatternDecl, LinkEntryDecl, LinkTargetDecl, Propagation, Schema,
};
use whyshelf_core::domain::shelf::snapshot::Snapshot;

use super::*;

fn eid(n: u8) -> EntityId {
    format!("01900000-0000-7000-8000-0000000000{n:02x}")
        .parse()
        .unwrap()
}

fn rel(text: &str) -> RelPath {
    text.parse().unwrap()
}

/// One book of specs (`SP`), whose `tests` link to artifacts compares as `compare`.
fn schema(compare: Option<CompareDecl>) -> Schema {
    Schema::validate(
        vec![BookSchemaDecl {
            name: "spec".to_owned(),
            dir: "specs".to_owned(),
            key: KeyPatternDecl::Prefixed {
                prefix: "SP".to_owned(),
            },
            description: None,
            fields: vec![FieldEntryDecl {
                name: "title".to_owned(),
                ty: FieldTypeDecl::Text,
                presence: FieldPresence::Required,
            }],
            links: vec![LinkEntryDecl {
                name: "tests".to_owned(),
                target: LinkTargetDecl::Artifact,
                cardinality: Cardinality::Many,
                propagation: Propagation::Depends,
                space: Some("file".to_owned()),
                compare,
                cover: None,
            }],
        }],
        &BTreeSet::from([Space::new("file").unwrap()]),
    )
    .unwrap()
}

fn lines_without_slashes() -> Option<CompareDecl> {
    Some(CompareDecl {
        kind: "lines".to_owned(),
        ignore: vec!["//".to_owned()],
    })
}

/// SP-1, tested by `tests/a.rs` holding `bytes`, under `schema`.
fn shelf(schema: &Schema, bytes: &[u8]) -> Shelf {
    let readings = vec![DocReading::Parsed {
        origin: rel("specs/sp-1.md"),
        entries: BTreeMap::from([
            (
                "id".to_owned(),
                RawValue::Text(DocId::new(eid(1)).to_string()),
            ),
            ("key".to_owned(), RawValue::Text("SP-1".to_owned())),
            ("title".to_owned(), RawValue::Text("spec".to_owned())),
            (
                "tests".to_owned(),
                RawValue::List(vec!["tests/a.rs".to_owned()]),
            ),
        ]),
        body: "body\n".to_owned(),
    }];
    let asks: ArtifactAsks = Shelf::artifact_asks(schema, &readings);
    let found: ArtifactReadings = asks
        .iter()
        .map(|(s, n, c)| {
            (
                s.clone(),
                n.clone(),
                ArtifactReading::of_bytes(bytes, c, None),
            )
        })
        .collect();
    Shelf::build(schema, readings, &found)
}

/// The ledger that approves SP-1 on `shelf`.
fn approved(shelf: &Shelf) -> Ledger {
    let id = DocId::new(eid(1));
    let frame = frame_approve(shelf, &Ledger::build(Vec::new()), id).unwrap();
    Ledger::build(vec![Record::Decision(
        Decision::approve(
            JudgementId::new(eid(10)),
            frame,
            None,
            Actor::new("tester").unwrap(),
            JudgedAt::new(10),
            None,
        )
        .unwrap(),
    )])
}

const THEN: &[u8] = b"assert!(fast);\n";

#[test]
fn a_change_of_comparison_is_written_with_the_file_the_link_and_both_comparisons() {
    let ledger = approved(&shelf(&schema(None), THEN));
    let now = shelf(&schema(lines_without_slashes()), THEN);
    let assessment = assess(&now, &ledger, JudgedAt::new(100));
    let Standing::Approved(Freshness::Degraded(d)) = assessment.standing_of(DocId::new(eid(1)))
    else {
        panic!("the comparison changed");
    };
    let names = Names::default();
    let out = Namer(&names).cause(d.causes().first());
    assert_eq!(out.kind, "artifact_comparison_changed");
    assert_eq!(out.severity, "stale");
    assert_eq!(out.artifact.as_deref(), Some("tests/a.rs"));
    assert_eq!(out.link.as_deref(), Some("tests"));
    assert!(out.detail.contains("lines (ignoring //)"), "{}", out.detail);
    assert!(out.detail.contains("bytes"), "{}", out.detail);
    let json = serde_json::to_value(&out).unwrap();
    assert_eq!(json["kind"], "artifact_comparison_changed");
    assert_eq!(json["artifact"], "tests/a.rs");
    assert_eq!(json["space"], "file");
}

/// The diff item of `tests/a.rs` for an approval of `then` and the file `now`, both
/// compared by the lines without `//`.
fn diff_of(then: &[u8], now: &[u8]) -> DiffItemOut {
    let lines = schema(lines_without_slashes());
    let ledger = approved(&shelf(&lines, then));
    let snapshot = Snapshot::of(&ledger, DocId::new(eid(1))).unwrap();
    let item = snapshot
        .items()
        .iter()
        .find(|i| matches!(i.role(), ItemRole::Artifact { .. }))
        .unwrap()
        .clone();
    let state = shelf(&lines, now)
        .doc(DocId::new(eid(1)))
        .unwrap()
        .artifact_links()["tests"]
        .artifacts()[0]
        .state()
        .clone();
    let now_item = ItemNow::Artifact(state);
    let changed = item.changed_from(&now_item);
    let diff = DiffItem {
        item,
        approved: Retrieval::Read(then.to_vec()),
        now: now_item,
        file: Some(rel("tests/a.rs")),
        current: Some(now.to_vec()),
        changed,
    };
    let names = Names::default();
    diff_item(&Namer(&names), &diff)
}

#[test]
fn the_diff_of_a_file_whose_lines_are_the_same_is_empty() {
    let same = diff_of(THEN, b"// why\n    assert!(fast);   \n\n");
    assert!(!same.changed);
    assert_eq!(same.diff.as_deref(), Some(""));

    let other = diff_of(THEN, b"assert!(slow);\n");
    assert!(other.changed);
    assert_eq!(other.now, Some("present"));
    let text = other.diff.unwrap();
    assert!(text.contains("-assert!(fast);"), "{text}");
    assert!(text.contains("+assert!(slow);"), "{text}");
}

#[test]
fn an_artifact_that_cannot_be_told_is_unknown_and_not_absent_in_diff_and_trace() {
    use whyshelf_core::domain::shelf::artifact::{ArtifactState, Untold};
    let ledger = approved(&shelf(&schema(None), THEN));
    let snapshot = Snapshot::of(&ledger, DocId::new(eid(1))).unwrap();
    let item = snapshot
        .items()
        .iter()
        .find(|i| matches!(i.role(), ItemRole::Artifact { .. }))
        .unwrap()
        .clone();
    let now = ItemNow::Artifact(ArtifactState::Unknown(Untold::NoAnswer));
    let diff = DiffItem {
        changed: item.changed_from(&now),
        item,
        approved: Retrieval::Read(THEN.to_vec()),
        now,
        file: None,
        current: None,
    };
    let names = Names::default();
    let out = diff_item(&Namer(&names), &diff);
    assert!(out.changed);
    assert_eq!(out.now, Some("unknown"));
    assert_eq!(out.current_hash, None);
    let json = serde_json::to_value(&out).unwrap();
    assert_eq!(json["now"], "unknown");
    assert_eq!(json["item"]["name"], "tests/a.rs");
    assert_eq!(json["item"]["path"], "tests/a.rs");

    // The target of a trace says the same.
    let s = Shelf::build(
        &schema(None),
        vec![DocReading::Parsed {
            origin: rel("specs/sp-1.md"),
            entries: BTreeMap::from([
                (
                    "id".to_owned(),
                    RawValue::Text(DocId::new(eid(1)).to_string()),
                ),
                ("key".to_owned(), RawValue::Text("SP-1".to_owned())),
                ("title".to_owned(), RawValue::Text("spec".to_owned())),
                (
                    "tests".to_owned(),
                    RawValue::List(vec!["tests/a.rs".to_owned()]),
                ),
            ]),
            body: "body\n".to_owned(),
        }],
        &ArtifactReadings::new(),
    );
    let artifact = &s.doc(DocId::new(eid(1))).unwrap().artifact_links()["tests"].artifacts()[0];
    let target = serde_json::to_value(artifact_target(artifact)).unwrap();
    assert_eq!(target["now"], "unknown");
    assert_eq!(target["hash"], Value::Null);
    assert_eq!(target["name"], "tests/a.rs");
}

#[test]
fn a_change_of_the_links_to_artifacts_gives_the_space_only_when_every_name_agrees() {
    use whyshelf_core::domain::shelf::artifact::{ArtifactKey, ArtifactName};
    use whyshelf_core::domain::shelf::schema::EntryName;
    let key = |space: &str, name: &str| {
        ArtifactKey::new(Space::new(space).unwrap(), ArtifactName::new(name).unwrap())
    };
    let names = Names::default();
    let namer = Namer(&names);
    let cause = |added: Vec<ArtifactKey>, removed: Vec<ArtifactKey>| {
        namer.cause(&Cause::ArtifactLinksChanged {
            link: EntryName::new("tests").unwrap(),
            added,
            removed,
        })
    };

    let same = cause(vec![key("file", "b.rs")], vec![key("file", "a.rs")]);
    assert_eq!(same.space.as_deref(), Some("file"));

    // The link moved from `file` to `other`: each name in its own space, no one space.
    let moved = cause(vec![key("other", "a.rs")], vec![key("file", "a.rs")]);
    assert_eq!(moved.kind, "links_changed");
    assert_eq!(moved.space, None);
    assert!(
        moved.detail.contains("`a.rs` in `other`") && moved.detail.contains("`a.rs` in `file`"),
        "{}",
        moved.detail
    );
}
