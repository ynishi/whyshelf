//! Builders for the tests of the shelf: a small schema, documents, and records.
//!
//! The schema has three books.
//!
//! ```text
//!   finding  findings/   FR-<n>      base       -> finding  Many  Depends
//!                                    related    -> finding  Many  Informs
//!                                    supersedes -> finding  Many  Replaces
//!   spec     specs/      SP-<n>      grounds    -> finding  Many  Depends
//!   design   designs/    <slug>      specs      -> spec     Many  Depends
//! ```

use std::collections::BTreeMap;

use uuid::Uuid;

use crate::domain::actor::Actor;
use crate::domain::id::{DocId, EntityId};
use crate::domain::location::RelPath;

use super::build::{DocReading, RawValue, Shelf};
use super::ledger::{
    Decision, JudgedAt, JudgementId, Ledger, Record, frame_approve, frame_present, frame_retire,
};
use super::schema::{
    BookSchemaDecl, Cardinality, FieldEntryDecl, FieldPresence, FieldTypeDecl, KeyPatternDecl,
    LinkEntryDecl, Propagation, Schema,
};

/// A UUID v7 identifier from a small number.
pub(super) fn eid(n: u128) -> EntityId {
    let bits = (n & 0x0fff_ffff_ffff_ffff) | (0x7u128 << 76) | (0b10u128 << 62);
    EntityId::new(Uuid::from_u128(bits)).unwrap()
}

pub(super) fn docid(n: u128) -> DocId {
    DocId::new(eid(n))
}

pub(super) fn jid(n: u128) -> JudgementId {
    JudgementId::new(eid(n))
}

pub(super) fn path(text: &str) -> RelPath {
    text.parse().unwrap()
}

pub(super) fn actor() -> Actor {
    Actor::new("tester").unwrap()
}

pub(super) fn text(s: &str) -> RawValue {
    RawValue::Text(s.to_owned())
}

pub(super) fn list(items: &[&str]) -> RawValue {
    RawValue::List(items.iter().map(|s| (*s).to_owned()).collect())
}

fn field(name: &str, ty: FieldTypeDecl, presence: FieldPresence) -> FieldEntryDecl {
    FieldEntryDecl {
        name: name.to_owned(),
        ty,
        presence,
    }
}

fn link(name: &str, target: &str, propagation: Propagation) -> LinkEntryDecl {
    LinkEntryDecl {
        name: name.to_owned(),
        target: target.to_owned(),
        cardinality: Cardinality::Many,
        propagation,
    }
}

/// The declarations of the three books.
pub(super) fn decls() -> Vec<BookSchemaDecl> {
    vec![
        BookSchemaDecl {
            name: "finding".to_owned(),
            dir: "findings".to_owned(),
            key: KeyPatternDecl::Prefixed {
                prefix: "FR".to_owned(),
            },
            fields: vec![
                field("title", FieldTypeDecl::Text, FieldPresence::Required),
                field("note", FieldTypeDecl::Text, FieldPresence::Optional),
                field("count", FieldTypeDecl::Integer, FieldPresence::Optional),
                field("flag", FieldTypeDecl::Bool, FieldPresence::Optional),
                field("tags", FieldTypeDecl::TextList, FieldPresence::Optional),
                field(
                    "level",
                    FieldTypeDecl::Enum(vec!["low".to_owned(), "high".to_owned()]),
                    FieldPresence::Optional,
                ),
            ],
            links: vec![
                link("base", "finding", Propagation::Depends),
                link("related", "finding", Propagation::Informs),
                link("supersedes", "finding", Propagation::Replaces),
            ],
        },
        BookSchemaDecl {
            name: "spec".to_owned(),
            dir: "specs".to_owned(),
            key: KeyPatternDecl::Prefixed {
                prefix: "SP".to_owned(),
            },
            fields: vec![field("title", FieldTypeDecl::Text, FieldPresence::Required)],
            links: vec![link("grounds", "finding", Propagation::Depends)],
        },
        BookSchemaDecl {
            name: "design".to_owned(),
            dir: "designs".to_owned(),
            key: KeyPatternDecl::Slug,
            fields: vec![field("title", FieldTypeDecl::Text, FieldPresence::Required)],
            links: vec![link("specs", "spec", Propagation::Depends)],
        },
    ]
}

pub(super) fn schema() -> Schema {
    Schema::validate(decls()).unwrap()
}

pub(super) fn entries(items: &[(&str, RawValue)]) -> BTreeMap<String, RawValue> {
    items
        .iter()
        .map(|(k, v)| ((*k).to_owned(), v.clone()))
        .collect()
}

/// A parsed file with `id`, `key` and the given entries, body `body\n`.
pub(super) fn reading(file: &str, id: u128, key: &str, extra: &[(&str, RawValue)]) -> DocReading {
    let mut all = entries(extra);
    all.insert("id".to_owned(), text(&docid(id).to_string()));
    all.insert("key".to_owned(), text(key));
    DocReading::Parsed {
        origin: path(file),
        entries: all,
        body: "body\n".to_owned(),
    }
}

/// A finding `FR-<n>` with identity `n`, a title and `base` links.
pub(super) fn fr(n: u128, base: &[&str]) -> DocReading {
    fr_titled(n, "title", base)
}

pub(super) fn fr_titled(n: u128, title: &str, base: &[&str]) -> DocReading {
    let mut extra = vec![("title", text(title))];
    if !base.is_empty() {
        extra.push(("base", list(base)));
    }
    reading(
        &format!("findings/fr-{n}.md"),
        n,
        &format!("FR-{n}"),
        &extra,
    )
}

pub(super) fn shelf(readings: Vec<DocReading>) -> Shelf {
    Shelf::build(&schema(), readings)
}

pub(super) fn ledger(records: Vec<Record>) -> Ledger {
    Ledger::build(records)
}

/// An approval of `subject` (a small number) that the shelf's frame allows.
pub(super) fn approve(
    shelf: &Shelf,
    ledger: &Ledger,
    subject: u128,
    judgement: u128,
    at: i64,
    expires: Option<i64>,
) -> Record {
    let frame = frame_approve(shelf, ledger, docid(subject)).unwrap();
    Record::Decision(
        Decision::approve(
            jid(judgement),
            frame,
            expires.map(JudgedAt::new),
            actor(),
            JudgedAt::new(at),
            None,
        )
        .unwrap(),
    )
}

/// The ledger that approves each of `subjects` in order (upstream first), at time 10,
/// with judgement ids `1000 + subject`.
pub(super) fn approved(shelf: &Shelf, subjects: &[u128]) -> Ledger {
    let mut records = Vec::new();
    for subject in subjects {
        let so_far = Ledger::build(records.clone());
        records.push(approve(shelf, &so_far, *subject, 1000 + subject, 10, None));
    }
    Ledger::build(records)
}

pub(super) fn deny(shelf: &Shelf, subject: u128, judgement: u128, at: i64) -> Record {
    let frame = frame_present(shelf, docid(subject)).unwrap();
    Record::Decision(
        Decision::deny(jid(judgement), frame, actor(), JudgedAt::new(at), None).unwrap(),
    )
}

pub(super) fn deprecate(shelf: &Shelf, subject: u128, judgement: u128, at: i64) -> Record {
    let frame = frame_present(shelf, docid(subject)).unwrap();
    Record::Decision(
        Decision::deprecate(jid(judgement), frame, actor(), JudgedAt::new(at), None).unwrap(),
    )
}

pub(super) fn retire(shelf: &Shelf, subject: u128, judgement: u128, at: i64) -> Record {
    let frame = frame_retire(shelf, docid(subject));
    Record::Decision(
        Decision::retire(jid(judgement), frame, actor(), JudgedAt::new(at), None).unwrap(),
    )
}
