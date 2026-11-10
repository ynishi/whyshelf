//! In-memory ports for the tests of the use cases of the shelf.
//!
//! One struct, [`FakeShelf`], implements every port and both bundles, so a test holds
//! the world in one place: the declarations, the readings of the files, the records
//! of the ledger. It counts what was written, so a test of a read can say that
//! nothing was.

use std::future::Future;
use std::pin::pin;
use std::sync::{Arc, Mutex, MutexGuard};
use std::task::{Context, Poll, Wake, Waker};

use uuid::Uuid;

use crate::domain::actor::Actor;
use crate::domain::id::{DocId, EntityId};
use crate::domain::location::RelPath;
use crate::domain::shelf::build::{DocReading, RawValue};
use crate::domain::shelf::ledger::{JudgedAt, JudgementId, Record};
use crate::domain::shelf::schema::{
    BookDir, BookSchemaDecl, Cardinality, FieldEntryDecl, FieldPresence, FieldTypeDecl,
    KeyPatternDecl, LinkEntryDecl, Propagation,
};

use super::ports::{
    ActorSource, BookSchemaSource, Clock, DocSource, IdSource, LedgerAppend, LedgerContents,
    LedgerRead, ShelfRead, ShelfWrite, UnreadableRecord,
};

/// Drive a future to completion on the current thread. The fakes never wait.
pub(super) fn block_on<F: Future>(future: F) -> F::Output {
    struct Noop;
    impl Wake for Noop {
        fn wake(self: Arc<Self>) {}
    }
    let waker = Waker::from(Arc::new(Noop));
    let mut cx = Context::from_waker(&waker);
    let mut future = pin!(future);
    loop {
        if let Poll::Ready(value) = future.as_mut().poll(&mut cx) {
            return value;
        }
    }
}

/// The only error the fakes produce, on request.
#[derive(Debug, thiserror::Error)]
#[error("fake failure: {0}")]
pub(super) struct FakeError(pub(super) &'static str);

/// A UUID v7 from a small number.
pub(super) fn eid(n: u128) -> EntityId {
    let bits = (n & 0x0fff_ffff_ffff_ffff) | (0x7u128 << 76) | (0b10u128 << 62);
    EntityId::new(Uuid::from_u128(bits)).expect("a v7 identifier")
}

/// The identity of a document from a small number.
pub(super) fn docid(n: u128) -> DocId {
    DocId::new(eid(n))
}

pub(super) fn path(text: &str) -> RelPath {
    text.parse().expect("a relative path")
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

/// Two books: findings (`FR-<n>`, `base` depends on findings) and specs (`SP-<n>`,
/// `grounds` depends on findings, `related` informs of findings, `supersedes`
/// replaces specs).
pub(super) fn decls() -> Vec<BookSchemaDecl> {
    vec![
        BookSchemaDecl {
            name: "finding".to_owned(),
            dir: "findings".to_owned(),
            key: KeyPatternDecl::Prefixed {
                prefix: "FR".to_owned(),
            },
            fields: vec![field("title", FieldTypeDecl::Text, FieldPresence::Required)],
            links: vec![link("base", "finding", Propagation::Depends)],
        },
        BookSchemaDecl {
            name: "spec".to_owned(),
            dir: "specs".to_owned(),
            key: KeyPatternDecl::Prefixed {
                prefix: "SP".to_owned(),
            },
            fields: vec![field("title", FieldTypeDecl::Text, FieldPresence::Required)],
            links: vec![
                link("grounds", "finding", Propagation::Depends),
                link("related", "finding", Propagation::Informs),
                link("supersedes", "spec", Propagation::Replaces),
            ],
        },
    ]
}

/// A parsed file with an identity, a key, a title and these links.
pub(super) fn file(
    origin: &str,
    n: u128,
    key: &str,
    title: &str,
    links: &[(&str, &[&str])],
) -> DocReading {
    let mut entries = std::collections::BTreeMap::new();
    entries.insert("id".to_owned(), RawValue::Text(docid(n).to_string()));
    entries.insert("key".to_owned(), RawValue::Text(key.to_owned()));
    entries.insert("title".to_owned(), RawValue::Text(title.to_owned()));
    for (name, keys) in links {
        entries.insert(
            (*name).to_owned(),
            RawValue::List(keys.iter().map(|k| (*k).to_owned()).collect()),
        );
    }
    DocReading::Parsed {
        origin: path(origin),
        entries,
        body: format!("{title}\n"),
    }
}

/// A finding `FR-<n>` with identity `n`.
pub(super) fn fr(n: u128, title: &str, base: &[&str]) -> DocReading {
    file(
        &format!("findings/fr-{n}.md"),
        n,
        &format!("FR-{n}"),
        title,
        &[("base", base)],
    )
}

/// A spec `SP-<n>` with identity `n`, grounded in these findings.
pub(super) fn sp(n: u128, title: &str, grounds: &[&str]) -> DocReading {
    file(
        &format!("specs/sp-{n}.md"),
        n,
        &format!("SP-{n}"),
        title,
        &[("grounds", grounds)],
    )
}

/// The world the ports show.
#[derive(Debug)]
pub(super) struct FakeShelf {
    pub(super) decls: Mutex<Vec<BookSchemaDecl>>,
    pub(super) readings: Mutex<Vec<DocReading>>,
    pub(super) records: Mutex<Vec<Record>>,
    pub(super) unreadable: Mutex<Vec<UnreadableRecord>>,
    pub(super) now: Mutex<i64>,
    pub(super) next_id: Mutex<u128>,
    pub(super) actor: Option<Actor>,
    pub(super) appends: Mutex<usize>,
    pub(super) failing: Option<&'static str>,
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().expect("a test does not poison its own lock")
}

impl FakeShelf {
    /// The two books, no document, no record, time 1000, the actor `alice`.
    pub(super) fn new() -> Self {
        Self {
            decls: Mutex::new(decls()),
            readings: Mutex::new(Vec::new()),
            records: Mutex::new(Vec::new()),
            unreadable: Mutex::new(Vec::new()),
            now: Mutex::new(1000),
            next_id: Mutex::new(9000),
            actor: Some(Actor::new("alice").expect("a name")),
            appends: Mutex::new(0),
            failing: None,
        }
    }

    /// The files of the project, replacing what was there.
    pub(super) fn with_files(self, files: Vec<DocReading>) -> Self {
        *lock(&self.readings) = files;
        self
    }

    /// Replace the file of one document (an edit), by the identity it carries.
    pub(super) fn edit(&self, replacement: DocReading, n: u128) {
        let mut readings = lock(&self.readings);
        let wanted = docid(n).to_string();
        readings.retain(|r| match r {
            DocReading::Parsed { entries, .. } => {
                entries.get("id") != Some(&RawValue::Text(wanted.clone()))
            }
            DocReading::Unparsed { .. } => true,
        });
        readings.push(replacement);
    }

    pub(super) fn set_now(&self, millis: i64) {
        *lock(&self.now) = millis;
    }

    pub(super) fn record_count(&self) -> usize {
        lock(&self.records).len()
    }

    pub(super) fn append_count(&self) -> usize {
        *lock(&self.appends)
    }

    pub(super) fn records(&self) -> Vec<Record> {
        lock(&self.records).clone()
    }

    fn check(&self) -> Result<(), FakeError> {
        match self.failing {
            Some(why) => Err(FakeError(why)),
            None => Ok(()),
        }
    }
}

impl BookSchemaSource for FakeShelf {
    type Error = FakeError;

    async fn declarations(&self) -> Result<Vec<BookSchemaDecl>, FakeError> {
        self.check()?;
        Ok(lock(&self.decls).clone())
    }
}

impl DocSource for FakeShelf {
    type Error = FakeError;

    async fn read_docs(&self, _dirs: &[BookDir]) -> Result<Vec<DocReading>, FakeError> {
        self.check()?;
        Ok(lock(&self.readings).clone())
    }
}

impl LedgerRead for FakeShelf {
    type Error = FakeError;

    async fn read(&self) -> Result<LedgerContents, FakeError> {
        self.check()?;
        Ok(LedgerContents {
            records: lock(&self.records).clone(),
            unreadable: lock(&self.unreadable).clone(),
        })
    }
}

impl LedgerAppend for FakeShelf {
    type Error = FakeError;

    async fn append(&self, record: &Record) -> Result<(), FakeError> {
        self.check()?;
        *lock(&self.appends) += 1;
        lock(&self.records).push(record.clone());
        Ok(())
    }
}

impl Clock for FakeShelf {
    type Error = FakeError;

    async fn now(&self) -> Result<JudgedAt, FakeError> {
        Ok(JudgedAt::new(*lock(&self.now)))
    }
}

impl IdSource for FakeShelf {
    type Error = FakeError;

    async fn new_judgement_id(&self) -> Result<JudgementId, FakeError> {
        let mut next = lock(&self.next_id);
        *next += 1;
        Ok(JudgementId::new(eid(*next)))
    }
}

impl ActorSource for FakeShelf {
    type Error = FakeError;

    async fn actor(&self) -> Result<Actor, FakeError> {
        self.actor.clone().ok_or(FakeError("no actor"))
    }
}

impl ShelfRead for FakeShelf {
    type Schemas = Self;
    type Docs = Self;
    type Ledger = Self;
    type Clock = Self;

    fn schemas(&self) -> &Self {
        self
    }
    fn docs(&self) -> &Self {
        self
    }
    fn ledger(&self) -> &Self {
        self
    }
    fn clock(&self) -> &Self {
        self
    }
}

impl ShelfWrite for FakeShelf {
    type Append = Self;
    type Ids = Self;
    type Actor = Self;

    fn append(&self) -> &Self {
        self
    }
    fn ids(&self) -> &Self {
        self
    }
    fn actor_source(&self) -> &Self {
        self
    }
}
