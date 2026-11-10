//! Tests of the JSON of `judge same` and `judge withdraw`, and of the notices an
//! edit declared to keep the meaning gives, from a shelf built by the domain.

use std::collections::BTreeMap;

use whyshelf_core::app::shelf::{Declared, DeclaredAbout, Names};
use whyshelf_core::domain::actor::Actor;
use whyshelf_core::domain::id::{DocId, EntityId};
use whyshelf_core::domain::location::RelPath;
use whyshelf_core::domain::nonempty::{NonBlank, NonEmptySet};
use whyshelf_core::domain::shelf::artifact::{ArtifactReading, ArtifactReadings};
use whyshelf_core::domain::shelf::build::{DocReading, RawValue, Shelf};
use whyshelf_core::domain::shelf::freshness::assess;
use whyshelf_core::domain::shelf::ledger::{
    Decision, JudgedAt, JudgementId, Ledger, Revocation, SameMeaning, SameMeaningTarget,
    frame_approve, frame_same_meaning,
};
use whyshelf_core::domain::shelf::schema::{
    BookSchemaDecl, Cardinality, FieldEntryDecl, FieldPresence, FieldTypeDecl, KeyPatternDecl,
    LinkEntryDecl, LinkTargetDecl, Propagation, Schema,
};

use super::*;

/// A UUID v7 identifier from a small number.
fn eid(n: u8) -> EntityId {
    format!("01900000-0000-7000-8000-0000000000{n:02x}")
        .parse()
        .unwrap()
}

fn docid(n: u8) -> DocId {
    DocId::new(eid(n))
}

fn jid(n: u8) -> JudgementId {
    JudgementId::new(eid(n))
}

fn rel(text: &str) -> RelPath {
    text.parse().unwrap()
}

fn actor() -> Actor {
    Actor::new("tester").unwrap()
}

fn because() -> NonBlank {
    NonBlank::new("a typo fixed").unwrap()
}

/// Findings (`FR`, `base` on findings) and specs (`SP`, `grounds` on findings,
/// `tests` on files).
fn schema() -> Schema {
    let link = |name: &str, target: LinkTargetDecl| LinkEntryDecl {
        name: name.to_owned(),
        target,
        cardinality: Cardinality::Many,
        propagation: Propagation::Depends,
        compare: None,
        cover: false,
    };
    let book = |name: &str, dir: &str, prefix: &str, links| BookSchemaDecl {
        name: name.to_owned(),
        dir: dir.to_owned(),
        key: KeyPatternDecl::Prefixed {
            prefix: prefix.to_owned(),
        },
        fields: vec![FieldEntryDecl {
            name: "title".to_owned(),
            ty: FieldTypeDecl::Text,
            presence: FieldPresence::Required,
        }],
        links,
    };
    Schema::validate(vec![
        book(
            "finding",
            "findings",
            "FR",
            vec![link("base", LinkTargetDecl::Book("finding".to_owned()))],
        ),
        book(
            "spec",
            "specs",
            "SP",
            vec![
                link("grounds", LinkTargetDecl::Book("finding".to_owned())),
                link("tests", LinkTargetDecl::Artifact),
            ],
        ),
    ])
    .unwrap()
}

fn reading(file: &str, n: u8, key: &str, title: &str, links: &[(&str, &str)]) -> DocReading {
    let mut entries = BTreeMap::from([
        ("id".to_owned(), RawValue::Text(docid(n).to_string())),
        ("key".to_owned(), RawValue::Text(key.to_owned())),
        ("title".to_owned(), RawValue::Text(title.to_owned())),
    ]);
    for (name, target) in links {
        entries.insert(
            (*name).to_owned(),
            RawValue::List(vec![(*target).to_owned()]),
        );
    }
    DocReading::Parsed {
        origin: rel(file),
        entries,
        body: "body\n".to_owned(),
    }
}

/// FR-1 titled `finding`, and SP-2 on it, tested by `tests/a.rs` holding `test`.
fn shelf(finding: &str, test: &str) -> Shelf {
    let artifacts: ArtifactReadings = BTreeMap::from([(
        rel("tests/a.rs"),
        ArtifactReading::of_bytes(test.as_bytes(), &Default::default(), None),
    )]);
    Shelf::build(
        &schema(),
        vec![
            reading("findings/fr-1.md", 1, "FR-1", finding, &[]),
            reading(
                "specs/sp-2.md",
                2,
                "SP-2",
                "spec",
                &[("grounds", "FR-1"), ("tests", "tests/a.rs")],
            ),
        ],
        &artifacts,
    )
}

/// The approvals of FR-1 and SP-2 on `shelf`, then a declaration about each target
/// framed on `now`, with judgement ids from 20.
fn records(shelf: &Shelf, now: &Shelf, targets: Vec<SameMeaningTarget>) -> Vec<Record> {
    let mut records = Vec::new();
    for n in [1, 2] {
        let frame = frame_approve(shelf, &Ledger::build(records.clone()), docid(n)).unwrap();
        records.push(Record::Decision(
            Decision::approve(jid(10 + n), frame, None, actor(), JudgedAt::new(10), None).unwrap(),
        ));
    }
    for (i, target) in targets.into_iter().enumerate() {
        let of = frame_same_meaning(now, &Ledger::build(records.clone()), target, None).unwrap();
        records.push(Record::SameMeaning(
            SameMeaning::new(jid(20 + i as u8), of, actor(), JudgedAt::new(50), because()).unwrap(),
        ));
    }
    records
}

#[test]
fn the_schema_of_judge_same_and_judge_withdraw_has_their_fields() {
    let outputs = schemas().outputs;
    let same = serde_json::to_string(&outputs["judge same"]).unwrap();
    for field in ["version", "declared", "doc", "artifact", "record"] {
        assert!(same.contains(&format!("\"{field}\"")), "{field}");
    }
    let withdraw = serde_json::to_string(&outputs["judge withdraw"]).unwrap();
    for field in ["version", "record"] {
        assert!(withdraw.contains(&format!("\"{field}\"")), "{field}");
    }
    // A notice names the file it is about and the declarations, beside its fields.
    for command in ["status", "get", "stale"] {
        let schema = serde_json::to_string(&outputs[command]).unwrap();
        for field in ["artifact", "declared"] {
            assert!(
                schema.contains(&format!("\"{field}\"")),
                "{command}: {field}"
            );
        }
    }
}

#[test]
fn a_declaration_is_written_with_what_it_is_about_and_the_record() {
    let then = shelf("a", "x");
    let now = shelf("b", "y");
    let all = records(
        &then,
        &now,
        vec![
            SameMeaningTarget::Doc(docid(1)),
            SameMeaningTarget::Artifact(rel("tests/a.rs")),
        ],
    );
    let declared = [
        Declared {
            record: all[2].clone(),
            about: DeclaredAbout::Doc {
                id: docid(1),
                key: now.doc(docid(1)).map(|d| d.key().clone()),
            },
        },
        Declared {
            record: all[3].clone(),
            about: DeclaredAbout::Artifact {
                path: rel("tests/a.rs"),
            },
        },
    ];
    let json = serde_json::to_value(same_meaning(&declared)).unwrap();
    let items = json["declared"].as_array().unwrap();
    assert_eq!(items.len(), 2);
    assert_eq!(items[0]["doc"]["id"], docid(1).to_string());
    assert_eq!(items[0]["doc"]["key"], "finding:FR-1");
    assert_eq!(items[0]["artifact"], Value::Null);
    assert_eq!(items[0]["record"]["kind"], "same_meaning");
    assert_eq!(items[0]["record"]["subject"]["kind"], "doc");
    assert_eq!(items[0]["record"]["reason"], "a typo fixed");
    assert_eq!(
        items[0]["record"]["to"],
        now.doc(docid(1)).unwrap().hash().to_string()
    );
    assert_eq!(items[1]["doc"], Value::Null);
    assert_eq!(items[1]["artifact"], "tests/a.rs");
    assert_eq!(items[1]["record"]["subject"]["path"], "tests/a.rs");
    assert_eq!(items[1]["record"]["from"].as_array().unwrap().len(), 1);
}

#[test]
fn a_withdrawal_is_written_as_the_revocation() {
    let record = Record::Revocation(Revocation::new(
        jid(30),
        NonEmptySet::from_set([jid(20), jid(11)].into()).unwrap(),
        actor(),
        JudgedAt::new(60),
        because(),
    ));
    let json = serde_json::to_value(withdrawn(&record)).unwrap();
    assert_eq!(json["record"]["kind"], "revocation");
    assert_eq!(json["record"]["id"], jid(30).to_string());
    let targets = json["record"]["targets"].as_array().unwrap();
    assert_eq!(targets.len(), 2);
    assert!(targets.contains(&Value::from(jid(20).to_string())));
}

#[test]
fn the_notices_of_a_declared_edit_name_the_declarations_and_the_file() {
    let then = shelf("a", "x");
    let now = shelf("b", "y");
    let all = records(
        &then,
        &now,
        vec![
            SameMeaningTarget::Doc(docid(1)),
            SameMeaningTarget::Artifact(rel("tests/a.rs")),
        ],
    );
    let assessment = assess(&now, &Ledger::build(all), JudgedAt::new(100));
    let names = Names::default();
    let namer = Namer(&names);
    let out = |n: u8| -> Vec<Value> {
        assessment
            .notices_of(docid(n))
            .iter()
            .map(|notice| serde_json::to_value(namer.notice(notice)).unwrap())
            .collect()
    };

    let finding = out(1);
    assert_eq!(finding.len(), 1, "{finding:?}");
    assert_eq!(finding[0]["kind"], "self_same_meaning");
    assert_eq!(finding[0]["link"], "");
    assert_eq!(finding[0]["artifact"], Value::Null);
    assert_eq!(
        finding[0]["declared"],
        serde_json::json!([jid(20).to_string()])
    );

    let spec = out(2);
    let of_kind = |kind: &str| {
        spec.iter()
            .find(|n| n["kind"] == kind)
            .unwrap_or_else(|| panic!("{kind} in {spec:?}"))
            .clone()
    };
    let upstream = of_kind("upstream_same_meaning");
    assert_eq!(upstream["link"], "grounds");
    assert_eq!(upstream["target"]["id"], docid(1).to_string());
    assert_eq!(upstream["artifact"], Value::Null);
    assert_eq!(
        upstream["declared"],
        serde_json::json!([jid(20).to_string()])
    );
    let artifact = of_kind("artifact_same_meaning");
    assert_eq!(artifact["link"], "tests");
    assert_eq!(artifact["target"], Value::Null);
    assert_eq!(artifact["artifact"], "tests/a.rs");
    assert_eq!(
        artifact["declared"],
        serde_json::json!([jid(21).to_string()])
    );
    assert_eq!(spec.len(), 2, "{spec:?}");
}
