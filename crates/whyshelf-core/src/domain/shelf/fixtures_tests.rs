//! Builders for the tests of the shelf: a small schema, documents, and records.
//!
//! The schema has three books.
//!
//! ```text
//!   finding  findings/   FR-<n>      base       -> finding  Many  Depends
//!                                    related    -> finding  Many  Informs
//!                                    supersedes -> finding  Many  Replaces
//!   spec     specs/      SP-<n>      grounds    -> finding  Many  Depends
//!                                    tests      -> artifact Many  Depends
//!                                    code       -> artifact Many  Informs
//!   design   designs/    <slug>      specs      -> spec     Many  Depends
//! ```

use std::collections::{BTreeMap, BTreeSet};

use uuid::Uuid;

use crate::domain::actor::{Actor, Attribution};
use crate::domain::id::{DocId, EntityId};
use crate::domain::location::RelPath;

use super::artifact::{
    ArtifactHash, ArtifactKey, ArtifactName, ArtifactReading, ArtifactReadings, Comparison,
    LinePrefix, Space,
};
use super::build::{DocReading, RawValue, Shelf};
use super::ledger::{
    Admit, Decision, DeclaredNow, JudgedAt, JudgementId, Ledger, ReadPin, Record, StandingApproval,
    frame_approve, frame_present, frame_retire,
};
use super::schema::{
    BookRuleDecl, BookSchemaDecl, BuiltinPresets, Cardinality, CompareDecl, DecideDecl,
    FieldEntryDecl, FieldPresence, FieldTypeDecl, GroupDecl, KeyPatternDecl, LinkEntryDecl,
    LinkTargetDecl, MemberDecl, Propagation, RuleDecl, RuleTime, Schema, ShelfDecl,
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

/// The space the links to artifacts of [`decls`] are in.
pub(super) fn file_space() -> Space {
    Space::new("file").unwrap()
}

/// The spaces the adapters of these tests offer: `file` and `other`.
pub(super) fn spaces() -> BTreeSet<Space> {
    BTreeSet::from([file_space(), Space::new("other").unwrap()])
}

/// A name of an artifact.
pub(super) fn name(text: &str) -> ArtifactName {
    ArtifactName::new(text).unwrap()
}

/// The artifact `text` in the space `file`.
pub(super) fn file_artifact(text: &str) -> ArtifactKey {
    ArtifactKey::new(file_space(), name(text))
}

pub(super) fn actor() -> Actor {
    Actor::new("tester").unwrap()
}

/// The plain one-name attribution of [`actor`]: a record whose actor ran the command too.
pub(super) fn by() -> Attribution {
    Attribution::one(actor())
}

pub(super) fn text(s: &str) -> RawValue {
    RawValue::Text(s.to_owned())
}

pub(super) fn list(items: &[&str]) -> RawValue {
    RawValue::List(items.iter().map(|s| (*s).to_owned()).collect())
}

fn scalar(word: &str) -> FieldTypeDecl {
    FieldTypeDecl::Word(word.to_owned())
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
        target: LinkTargetDecl::Book(target.to_owned()),
        cardinality: Cardinality::Many,
        propagation,
        space: None,
        compare: None,
        cover: None,
    }
}

/// A link to artifacts in the space `file`.
pub(super) fn artifact_link(name: &str, propagation: Propagation) -> LinkEntryDecl {
    LinkEntryDecl {
        name: name.to_owned(),
        target: LinkTargetDecl::Artifact,
        cardinality: Cardinality::Many,
        propagation,
        space: Some("file".to_owned()),
        compare: None,
        cover: None,
    }
}

/// The declarations of the three books.
pub(super) fn decls() -> Vec<BookSchemaDecl> {
    vec![
        BookSchemaDecl {
            name: "finding".to_owned(),
            dir: "findings".to_owned(),
            scope: Default::default(),
            key: KeyPatternDecl::Prefixed {
                prefix: "FR".to_owned(),
            },
            description: None,
            fields: vec![
                field("title", scalar("text"), FieldPresence::Required),
                field("note", scalar("text"), FieldPresence::Optional),
                field("count", scalar("integer"), FieldPresence::Optional),
                field("flag", scalar("bool"), FieldPresence::Optional),
                field("tags", scalar("list"), FieldPresence::Optional),
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
            scope: Default::default(),
            key: KeyPatternDecl::Prefixed {
                prefix: "SP".to_owned(),
            },
            description: None,
            fields: vec![field("title", scalar("text"), FieldPresence::Required)],
            links: vec![
                link("grounds", "finding", Propagation::Depends),
                artifact_link("tests", Propagation::Depends),
                artifact_link("code", Propagation::Informs),
            ],
        },
        BookSchemaDecl {
            name: "design".to_owned(),
            dir: "designs".to_owned(),
            scope: Default::default(),
            key: KeyPatternDecl::slug(),
            description: None,
            fields: vec![field("title", scalar("text"), FieldPresence::Required)],
            links: vec![link("specs", "spec", Propagation::Depends)],
        },
    ]
}

pub(super) fn schema() -> Schema {
    Schema::validate(decls(), &spaces()).unwrap()
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

/// A shelf with nothing at any name its links to artifacts write: the adapter
/// answers "absent" for each.
pub(super) fn shelf(readings: Vec<DocReading>) -> Shelf {
    shelf_answered(readings, &[])
}

/// A shelf whose artifacts are these names in the space `file`: a thing with this
/// hash (as its bytes compare), or none. Every other name asked is absent.
pub(super) fn shelf_with(
    readings: Vec<DocReading>,
    artifacts: &[(&str, Option<ArtifactHash>)],
) -> Shelf {
    let answers: Vec<(&str, ArtifactReading)> = artifacts
        .iter()
        .map(|(p, hash)| {
            let reading = match hash {
                Some(hash) => {
                    ArtifactReading::of_hashes(BTreeMap::from([(Comparison::Bytes, *hash)]), None)
                }
                None => ArtifactReading::absent(),
            };
            (*p, reading)
        })
        .collect();
    shelf_answered(readings, &answers)
}

/// A shelf whose artifacts in the space `file` are these answers, as an adapter
/// answers: every other name asked, in any space, is absent.
pub(super) fn shelf_answered(
    readings: Vec<DocReading>,
    artifacts: &[(&str, ArtifactReading)],
) -> Shelf {
    shelf_answered_by(&schema(), readings, artifacts)
}

/// The schema of [`decls`] with a second `Depends` link of the findings, `also`, into
/// the findings: a finding then has two links it stands on, `base` and `also`.
pub(super) fn schema_of_two_links() -> Schema {
    let mut decls = decls();
    decls[0]
        .links
        .push(link("also", "finding", Propagation::Depends));
    Schema::validate(decls, &spaces()).unwrap()
}

/// As [`shelf`], with the books of `schema`.
pub(super) fn shelf_of(schema: &Schema, readings: Vec<DocReading>) -> Shelf {
    shelf_answered_by(schema, readings, &[])
}

/// As [`shelf_answered`], with the books of `schema`.
fn shelf_answered_by(
    schema: &Schema,
    readings: Vec<DocReading>,
    artifacts: &[(&str, ArtifactReading)],
) -> Shelf {
    let mut found: ArtifactReadings = Shelf::artifact_asks(schema, &readings)
        .iter()
        .map(|(space, at, _)| (space.clone(), at.clone(), ArtifactReading::absent()))
        .collect();
    for (p, reading) in artifacts {
        found.insert(file_space(), name(p), reading.clone());
    }
    Shelf::build(schema, readings, &found)
}

/// A spec `SP-<n>` with identity `n`, grounded in these findings and tested by these
/// paths (its `tests` link to artifacts).
pub(super) fn sp_testing(n: u128, grounds: &[&str], tests: &[&str]) -> DocReading {
    let mut extra = vec![("title", text("spec"))];
    if !grounds.is_empty() {
        extra.push(("grounds", list(grounds)));
    }
    if !tests.is_empty() {
        extra.push(("tests", list(tests)));
    }
    reading(&format!("specs/sp-{n}.md"), n, &format!("SP-{n}"), &extra)
}

/// The schema of [`decls`] with the spec's `tests` link comparing `lines`, leaving out
/// the lines that start with one of `ignore`.
pub(super) fn schema_of_lines(ignore: &[&str]) -> Schema {
    Schema::validate(decls_of_lines(ignore), &spaces()).unwrap()
}

/// [`decls`] with the spec's `tests` link comparing `lines`, leaving out the lines that
/// start with one of `ignore`.
fn decls_of_lines(ignore: &[&str]) -> Vec<BookSchemaDecl> {
    let mut decls = decls();
    let tests = decls[1]
        .links
        .iter_mut()
        .find(|l| l.name == "tests")
        .expect("the spec has a tests link");
    tests.compare = Some(CompareDecl {
        kind: "lines".to_owned(),
        ignore: ignore.iter().map(|i| (*i).to_owned()).collect(),
    });
    decls
}

/// The comparison `lines` with these prefixes left out.
pub(super) fn lines(ignore: &[&str]) -> Comparison {
    Comparison::Lines {
        ignore: ignore.iter().map(|i| LinePrefix::new(i).unwrap()).collect(),
    }
}

/// A shelf of `schema` whose artifacts hold these bytes (a name with none has
/// nothing there), read as an adapter does: each name under the comparisons
/// [`Shelf::artifact_asks`] asks, whatever its space.
pub(super) fn shelf_reading(
    schema: &Schema,
    readings: Vec<DocReading>,
    files: &[(&str, &[u8])],
) -> Shelf {
    let files: BTreeMap<ArtifactName, &[u8]> = files.iter().map(|(p, b)| (name(p), *b)).collect();
    let found: ArtifactReadings = Shelf::artifact_asks(schema, &readings)
        .iter()
        .map(|(space, at, comparisons)| {
            let reading = match files.get(at) {
                Some(bytes) => ArtifactReading::of_bytes(bytes, comparisons, None),
                None => ArtifactReading::absent(),
            };
            (space.clone(), at.clone(), reading)
        })
        .collect();
    Shelf::build(schema, readings, &found)
}

/// The ledger of `records`, read as a project that declares no rule of who may
/// decide reads it.
pub(super) fn ledger(records: Vec<Record>) -> Ledger {
    Ledger::build(records, &Admit::unruled())
}

/// The approval that stands about `id` in `ledger`, which the test made stand.
pub(super) fn standing(ledger: &Ledger, id: DocId) -> StandingApproval<'_> {
    match ledger.declared(id) {
        DeclaredNow::Approved(standing) => standing,
        other => panic!("an approval stands about {id}, got {other:?}"),
    }
}

/// The artifact pins of the approval that stands about `id`, each as it is read now.
pub(super) fn read_pins(ledger: &Ledger, id: DocId) -> Vec<ReadPin> {
    standing(ledger, id).view().read_pins()
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
            by(),
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
        let so_far = ledger(records.clone());
        records.push(approve(shelf, &so_far, *subject, 1000 + subject, 10, None));
    }
    ledger(records)
}

/// Who may decide, asking for `approvals` different names: `maintainers` (`ana`, `bo`,
/// `cy`) may write every kind of judgement about the findings (from `since`, when
/// given) and the records of no book.
pub(super) fn asking(approvals: i64, since: Option<i64>) -> DecideDecl {
    let maintainers = vec!["maintainers".to_owned()];
    let member = |name: &str| MemberDecl {
        name: name.to_owned(),
        from: None,
        until: None,
    };
    DecideDecl {
        groups: vec![GroupDecl {
            name: "maintainers".to_owned(),
            members: vec![member("ana"), member("bo"), member("cy")],
        }],
        shelf: Some(RuleDecl {
            default: Some(maintainers.clone()),
            approvals: Some(approvals),
            ..RuleDecl::default()
        }),
        books: vec![BookRuleDecl {
            book: "finding".to_owned(),
            rule: RuleDecl {
                default: Some(maintainers),
                since: since.map(RuleTime::from_millis),
                approvals: Some(approvals),
                ..RuleDecl::default()
            },
        }],
    }
}

/// The schema of the three books under who may decide as `decide` says.
fn ruled_schema(decide: DecideDecl) -> Schema {
    ruled_schema_of(decls(), decide)
}

/// The schema of `books` under who may decide as `decide` says.
fn ruled_schema_of(books: Vec<BookSchemaDecl>, decide: DecideDecl) -> Schema {
    let mut decl = ShelfDecl::of_books(books);
    decl.decide = decide;
    Schema::declared(decl, &BuiltinPresets::new(), &spaces(), &Default::default()).unwrap()
}

/// A shelf of the three books and `readings`, under who may decide as `decide` says.
pub(super) fn ruled_shelf(decide: DecideDecl, readings: Vec<DocReading>) -> Shelf {
    Shelf::build(&ruled_schema(decide), readings, &ArtifactReadings::new())
}

/// As [`ruled_shelf`], whose artifacts in the space `file` are these names, each with
/// this hash (as its bytes compare); every other name asked is absent.
pub(super) fn ruled_shelf_with(
    decide: DecideDecl,
    readings: Vec<DocReading>,
    artifacts: &[(&str, ArtifactHash)],
) -> Shelf {
    let answers: Vec<(&str, ArtifactReading)> = artifacts
        .iter()
        .map(|(p, hash)| {
            let hashes = BTreeMap::from([(Comparison::Bytes, *hash)]);
            (*p, ArtifactReading::of_hashes(hashes, None))
        })
        .collect();
    ruled_shelf_answered(decide, readings, &answers)
}

/// As [`ruled_shelf`], whose artifacts in the space `file` are these answers, as an
/// adapter answers: every other name asked is absent.
pub(super) fn ruled_shelf_answered(
    decide: DecideDecl,
    readings: Vec<DocReading>,
    artifacts: &[(&str, ArtifactReading)],
) -> Shelf {
    shelf_answered_by(&ruled_schema(decide), readings, artifacts)
}

/// The schema of the three books under who may decide as `decide` says, the spec's
/// `tests` link comparing `lines` without the lines that start with one of `ignore`.
pub(super) fn ruled_schema_of_lines(decide: DecideDecl, ignore: &[&str]) -> Schema {
    ruled_schema_of(decls_of_lines(ignore), decide)
}

/// The ledger of `records`, read with who may decide as `shelf` declares it.
pub(super) fn admitted(shelf: &Shelf, records: Vec<Record>) -> Ledger {
    Ledger::build(records, &Admit::of(shelf))
}

/// An approval of `subject` by `by`, made at `at` and lapsing at `until`, on the frame
/// `ledger` gives.
pub(super) fn approve_as(
    shelf: &Shelf,
    ledger: &Ledger,
    subject: u128,
    judgement: u128,
    (by, at, until): (&str, i64, Option<i64>),
) -> Record {
    let frame = frame_approve(shelf, ledger, docid(subject)).unwrap();
    Record::Decision(
        Decision::approve(
            jid(judgement),
            frame,
            until.map(JudgedAt::new),
            Attribution::one(Actor::new(by).unwrap()),
            JudgedAt::new(at),
            None,
        )
        .unwrap(),
    )
}

pub(super) fn deny(shelf: &Shelf, subject: u128, judgement: u128, at: i64) -> Record {
    let frame = frame_present(shelf, docid(subject)).unwrap();
    Record::Decision(Decision::deny(jid(judgement), frame, by(), JudgedAt::new(at), None).unwrap())
}

pub(super) fn deprecate(shelf: &Shelf, subject: u128, judgement: u128, at: i64) -> Record {
    let frame = frame_present(shelf, docid(subject)).unwrap();
    Record::Decision(
        Decision::deprecate(jid(judgement), frame, by(), JudgedAt::new(at), None).unwrap(),
    )
}

pub(super) fn retire(shelf: &Shelf, subject: u128, judgement: u128, at: i64) -> Record {
    let frame = frame_retire(shelf, docid(subject));
    Record::Decision(
        Decision::retire(jid(judgement), frame, by(), JudgedAt::new(at), None).unwrap(),
    )
}
