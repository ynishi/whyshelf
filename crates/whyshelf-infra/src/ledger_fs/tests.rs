use super::*;

use std::collections::BTreeSet;

use whyshelf_core::app::commit::record::{append_with_object, append_without_object, scan, store};
use whyshelf_core::app::raw::NormalizedText;
use whyshelf_core::domain::actor::Timestamp;
use whyshelf_core::domain::corpus::{
    Corpus, Document, Entity, FileFacts, GitPlace, Header, Location, Reading,
};
use whyshelf_core::domain::corpus_eval::{CorpusInput, evaluate_corpus};
use whyshelf_core::domain::finding::{EvidenceLabel, Finding, FindingSource, Reproducibility};
use whyshelf_core::domain::gate::{CrInput, cr};
use whyshelf_core::domain::id::{DocId, EntityId, FindingId};
use whyshelf_core::domain::judge::{Judge, PersonName, Principal};
use whyshelf_core::domain::judgement::{Judgement, ObjectJudgement, PlainJudgement, Split};
use whyshelf_core::domain::keyring::PinKeyring;
use whyshelf_core::domain::knowable::Knowable;
use whyshelf_core::domain::ledger::{Ledger, Ledgers};
use whyshelf_core::domain::nonempty::NonBlank;
use whyshelf_core::domain::policy::GatePolicy;

use crate::secret::RegexSecretScan;

const DOC: &str = "0192f3a4-5b6c-7d8e-8f90-123456789abc";

fn eid(n: u128) -> EntityId {
    let id = format!("0192f3a4-5b6c-7d8e-8f90-{n:012x}");
    id.parse().unwrap()
}

fn judge() -> Judge {
    Judge::new(PersonName::new("ada").unwrap(), None, Principal::Human)
}

fn at() -> Timestamp {
    Timestamp::from_validated("2026-11-05T09:00:00Z".into()).unwrap()
}

fn dirs(root: &Path) -> StoreDirs {
    StoreDirs::new(root.join("public"), root.join("private"))
}

/// A corpus of one Finding, in a file under `visibility`, with its text.
struct World {
    corpus: Corpus,
    finding: Finding,
    text: NormalizedText,
}

impl World {
    fn of(visibility: Visibility) -> Self {
        let id: EntityId = DOC.parse().unwrap();
        let finding = Finding::new(
            FindingId::new(id),
            NonBlank::new("Slow login").unwrap(),
            FindingSource::UsageObservation {
                context: NonBlank::new("cold start").unwrap(),
                reproducibility: Reproducibility::Reproducible,
            },
            EvidenceLabel::Measured,
            "Login takes 8 s.\n".to_owned(),
        );
        let entity = Entity::Finding(finding.clone());
        let header = Header::new(entity.id(), entity.kind(), None, BTreeSet::new(), None);
        let text = NormalizedText::new("---\nid: x\n---\nLogin takes 8 s.\n".to_owned());
        let facts = FileFacts::new(
            Location::new(
                visibility,
                "docs/finding/slow-login.md".parse().unwrap(),
                Knowable::Known(GitPlace::Ignored),
            ),
            whyshelf_core::domain::hash::BytesHash::of(text.as_str().as_bytes()),
        );
        let reading = Reading::Document(Document::new(header, entity).unwrap());
        Self {
            corpus: Corpus::build([(facts, reading)], &BTreeSet::new()),
            finding,
            text,
        }
    }

    fn ledgers(&self) -> Ledgers {
        Ledgers::new(Ledger::new(), Ledger::new())
    }

    /// An approval of the Finding by the gate that checks it, as the judgement that
    /// keeps an object.
    fn approval(&self, n: u128, origin: Visibility) -> ObjectJudgement {
        let ledgers = self.ledgers();
        let keyring = PinKeyring::empty();
        let evaluation = evaluate_corpus(CorpusInput {
            corpus: &self.corpus,
            ledgers: &ledgers,
            keyring: &keyring,
            policy: GatePolicy::default(),
            written: &BTreeMap::new(),
        });
        let evaluated = evaluation.findings().get(&self.finding.id()).unwrap();
        let passed = cr::check(CrInput {
            finding: evaluated,
            text: self.text.text_hash(),
        })
        .expect("the finding passes CR");
        let object = whyshelf_core::domain::judgement::ObjectKey::new(
            *passed.attestation().target_hash(),
            *passed.text(),
        );
        let judgement = Judgement::approve(
            JudgementId::new(eid(n)),
            at(),
            judge(),
            origin,
            passed,
            object,
        )
        .unwrap();
        match judgement.split() {
            Split::WithObject(view) => view,
            Split::Plain(_) => panic!("an approval keeps an object"),
        }
    }

    /// A purge of the Finding's version, with `reason`, under the identifier `n`.
    fn purge(&self, n: u128, reason: &str) -> PlainJudgement {
        let judgement = Judgement::purge(
            JudgementId::new(eid(n)),
            at(),
            judge(),
            Visibility::Public,
            DocId::new(self.finding.id().entity()),
            self.text.text_hash(),
            NonBlank::new(reason).unwrap(),
        );
        match judgement.split() {
            Split::Plain(view) => view,
            Split::WithObject(_) => panic!("a purge keeps no object"),
        }
    }

    /// Put the text and write the approval, as the commit step does.
    async fn approve(&self, ledger: &FsLedger, objects: &FsObjects, n: u128) {
        let origin = self.side();
        let approval = self.approval(n, origin);
        let secret = RegexSecretScan::new().unwrap();
        let scanned = scan(
            &secret,
            &self.corpus,
            &self.ledgers(),
            &self.text,
            &approval,
        )
        .await
        .unwrap();
        let stored = store(objects, &scanned).await.unwrap();
        append_with_object(ledger, approval, &stored).await.unwrap();
    }

    fn side(&self) -> Visibility {
        self.corpus.documents()[0].location().visibility()
    }
}

async fn file_names(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = Vec::new();
    if let Ok(mut listing) = tokio::fs::read_dir(dir).await {
        while let Some(entry) = listing.next_entry().await.unwrap() {
            names.push(entry.file_name().into_string().unwrap());
        }
    }
    names.sort();
    names
}

// --- the canonical encoding read back -------------------------------------------------

#[test]
fn the_canonical_encoding_reads_back_to_the_value_that_made_it() {
    let value = Canon::map([
        ("a", Canon::Null),
        ("b", Canon::Bool(true)),
        ("c", Canon::Int(-7)),
        ("d", Canon::str("héllo")),
        (
            "e",
            Canon::list([Canon::Int(1), Canon::map([("k", Canon::Null)])]),
        ),
    ]);
    assert_eq!(decode_canon(&value.to_bytes()), Some(value));
}

#[test]
fn bytes_that_are_not_the_canonical_encoding_are_refused() {
    let good = Canon::map([("a", Canon::Int(1))]).to_bytes();
    assert!(decode_canon(&good).is_some());

    // Truncated, with a byte too many, and empty.
    assert!(decode_canon(&good[..good.len() - 1]).is_none());
    let mut long = good.clone();
    long.push(0);
    assert!(decode_canon(&long).is_none());
    assert!(decode_canon(&[]).is_none());
    // A tag that is none, a length that lies about the elements to come, text that
    // is not UTF-8, and a boolean that is neither.
    assert!(decode_canon(&[0x09, 0, 0, 0, 0, 0, 0, 0, 0]).is_none());
    assert!(decode_canon(&[0x04, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff]).is_none());
    assert!(decode_canon(&[0x03, 0, 0, 0, 0, 0, 0, 0, 1, 0xff]).is_none());
    assert!(decode_canon(&[0x01, 0, 0, 0, 0, 0, 0, 0, 1, 2]).is_none());
    // Keys out of order: a map the encoder would never write.
    let out_of_order = {
        let a = Canon::str("a").to_bytes();
        let b = Canon::str("b").to_bytes();
        let one = Canon::Int(1).to_bytes();
        let mut bytes = vec![0x05, 0, 0, 0, 0, 0, 0, 0, 2];
        for key in [&b, &a] {
            bytes.extend_from_slice(key);
            bytes.extend_from_slice(&one);
        }
        bytes
    };
    assert!(decode_canon(&out_of_order).is_none());
    // Nested far deeper than a record.
    let mut deep = Canon::Null;
    for _ in 0..(MAX_DEPTH + 2) {
        deep = Canon::list([deep]);
    }
    assert!(decode_canon(&deep.to_bytes()).is_none());
}

#[test]
fn record_names_are_the_identifier_and_perhaps_a_digest() {
    let id = JudgementId::new(eid(1));
    let digest = "a".repeat(64);
    assert_eq!(record_name(&id.to_string()), Some(id));
    assert_eq!(record_name(&format!("{id}.{digest}")), Some(id));
    for bad in [
        format!("{id}.tmp-12-0"),
        format!("{id}.{}", "A".repeat(64)),
        id.to_string().to_uppercase(),
        id.to_string().replace('-', ""),
        ".DS_Store".to_owned(),
        "layout".to_owned(),
    ] {
        assert_eq!(record_name(&bad), None, "{bad}");
    }
}

// --- the layout -------------------------------------------------------------------------

#[tokio::test]
async fn a_new_store_gets_the_mark_before_its_first_record() {
    let root = tempfile::tempdir().unwrap();
    let world = World::of(Visibility::Public);
    let (ledger, objects) = (
        FsLedger::new(dirs(root.path())),
        FsObjects::new(dirs(root.path())),
    );
    assert!(
        !root.path().join("public").exists(),
        "building the adapters writes nothing"
    );
    world.approve(&ledger, &objects, 1).await;
    let mark = tokio::fs::read_to_string(root.path().join("public/layout"))
        .await
        .unwrap();
    assert_eq!(mark, "3\n");
    assert!(
        !root.path().join("private").exists(),
        "the other side is not touched"
    );
}

#[tokio::test]
async fn a_store_in_another_layout_is_refused_and_left_alone() {
    let root = tempfile::tempdir().unwrap();
    let ledger = FsLedger::new(dirs(root.path()));
    let objects = FsObjects::new(dirs(root.path()));
    let side = root.path().join("public");
    tokio::fs::create_dir_all(side.join("events"))
        .await
        .unwrap();
    tokio::fs::write(side.join("layout"), "2\n").await.unwrap();
    tokio::fs::write(side.join("events/old.json"), "{}")
        .await
        .unwrap();

    assert!(matches!(
        ledger.read(Visibility::Public).await,
        Err(LedgerFsError::Layout {
            found: Found::Version(2),
            ..
        })
    ));
    assert!(matches!(
        objects.get(Visibility::Public, TextHash::of("x")).await,
        Err(LedgerFsError::Layout { .. })
    ));
    let world = World::of(Visibility::Public);
    let approval = world.approval(1, Visibility::Public);
    let secret = RegexSecretScan::new().unwrap();
    let scanned = scan(
        &secret,
        &world.corpus,
        &world.ledgers(),
        &world.text,
        &approval,
    )
    .await
    .unwrap();
    assert!(store(&objects, &scanned).await.is_err());
    assert_eq!(
        tokio::fs::read_to_string(side.join("layout"))
            .await
            .unwrap(),
        "2\n",
        "the mark is not rewritten"
    );
    assert_eq!(
        file_names(&side).await,
        ["events", "layout"],
        "nothing was added"
    );

    // Event files without a mark are the layout before the mark.
    tokio::fs::remove_file(side.join("layout")).await.unwrap();
    assert!(matches!(
        ledger.read(Visibility::Public).await,
        Err(LedgerFsError::Layout {
            found: Found::Version(1),
            ..
        })
    ));
}

#[tokio::test]
async fn a_side_that_is_not_there_reads_as_empty() {
    let root = tempfile::tempdir().unwrap();
    let ledger = FsLedger::new(dirs(root.path()));
    let read = ledger.read(Visibility::Private).await.unwrap();
    assert!(read.records().is_empty() && read.corrupt().is_empty());
}

// --- the ledger ---------------------------------------------------------------------------

#[tokio::test]
async fn a_record_is_one_file_named_by_its_id_holding_its_canonical_bytes() {
    let root = tempfile::tempdir().unwrap();
    let world = World::of(Visibility::Private);
    let (ledger, objects) = (
        FsLedger::new(dirs(root.path())),
        FsObjects::new(dirs(root.path())),
    );
    world.approve(&ledger, &objects, 1).await;

    let expected = world
        .approval(1, Visibility::Private)
        .judgement()
        .canonical_bytes();
    let dir = root.path().join("private/ledger");
    assert_eq!(
        file_names(&dir).await,
        [JudgementId::new(eid(1)).to_string()]
    );
    assert_eq!(
        tokio::fs::read(dir.join(JudgementId::new(eid(1)).to_string()))
            .await
            .unwrap(),
        expected
    );

    let raw = ledger.read_raw(Visibility::Private).await.unwrap();
    assert_eq!(raw.entries().len(), 1);
    let entry = &raw.entries()[0];
    assert_eq!(entry.named(), JudgementId::new(eid(1)));
    assert_eq!(entry.bytes(), expected);
    assert!(matches!(entry.shape(), Shape::Record(_)));
    assert!(raw.corrupt().is_empty());
}

#[tokio::test]
async fn appending_the_same_record_again_is_success_and_changes_nothing() {
    let root = tempfile::tempdir().unwrap();
    let world = World::of(Visibility::Public);
    let (ledger, objects) = (
        FsLedger::new(dirs(root.path())),
        FsObjects::new(dirs(root.path())),
    );
    world.approve(&ledger, &objects, 1).await;
    world.approve(&ledger, &objects, 1).await;
    world.approve(&ledger, &objects, 1).await;
    let records = file_names(&root.path().join("public/ledger")).await;
    assert_eq!(records.len(), 1);
    assert_eq!(
        file_names(&root.path().join("public/objects")).await.len(),
        1
    );
    assert!(
        !records.iter().any(|n| n.contains(".tmp-")),
        "no temporary file is left"
    );
}

#[tokio::test]
async fn a_record_of_the_same_id_with_other_bytes_is_kept_beside_the_first() {
    let root = tempfile::tempdir().unwrap();
    let world = World::of(Visibility::Public);
    let ledger = FsLedger::new(dirs(root.path()));
    let corpus = &world.corpus;
    let ledgers = world.ledgers();
    append_without_object(&ledger, world.purge(5, "first reason"), corpus, &ledgers)
        .await
        .unwrap();
    append_without_object(&ledger, world.purge(5, "other reason"), corpus, &ledgers)
        .await
        .unwrap();
    // The same again, in either order: nothing more.
    append_without_object(&ledger, world.purge(5, "other reason"), corpus, &ledgers)
        .await
        .unwrap();
    append_without_object(&ledger, world.purge(5, "first reason"), corpus, &ledgers)
        .await
        .unwrap();

    let names = file_names(&root.path().join("public/ledger")).await;
    let id = JudgementId::new(eid(5)).to_string();
    assert_eq!(names.len(), 2, "{names:?}");
    assert!(names.contains(&id));
    assert!(names.iter().any(|n| n.starts_with(&format!("{id}."))));

    // The first stays what it was; both are handed to the reader.
    let first = world.purge(5, "first reason").judgement().canonical_bytes();
    assert_eq!(
        tokio::fs::read(root.path().join("public/ledger").join(&id))
            .await
            .unwrap(),
        first
    );
    let raw = ledger.read_raw(Visibility::Public).await.unwrap();
    assert_eq!(raw.entries().len(), 2);
    assert!(
        raw.entries()
            .iter()
            .all(|e| e.named() == JudgementId::new(eid(5)))
    );
    assert_ne!(raw.entries()[0].bytes(), raw.entries()[1].bytes());
}

#[tokio::test]
async fn a_record_that_cannot_be_read_or_is_named_wrong_is_reported_not_dropped() {
    let root = tempfile::tempdir().unwrap();
    let world = World::of(Visibility::Public);
    let (ledger, objects) = (
        FsLedger::new(dirs(root.path())),
        FsObjects::new(dirs(root.path())),
    );
    world.approve(&ledger, &objects, 1).await;
    let dir = root.path().join("public/ledger");
    let one = JudgementId::new(eid(1));

    // Junk under a record's name; a record under another's name; and files that are
    // not records at all.
    tokio::fs::write(
        dir.join(JudgementId::new(eid(2)).to_string()),
        b"not a record",
    )
    .await
    .unwrap();
    tokio::fs::copy(
        dir.join(one.to_string()),
        dir.join(JudgementId::new(eid(3)).to_string()),
    )
    .await
    .unwrap();
    tokio::fs::write(dir.join(".DS_Store"), b"x").await.unwrap();
    tokio::fs::write(dir.join(format!("{one}.tmp-1-0")), b"half a rec")
        .await
        .unwrap();

    let raw = ledger.read_raw(Visibility::Public).await.unwrap();
    assert_eq!(raw.entries().len(), 3, "the stray files are not looked at");
    let corrupt = raw.corrupt();
    let problems: Vec<_> = corrupt.iter().map(|c| (c.id(), c.problem())).collect();
    assert_eq!(
        problems,
        [
            (JudgementId::new(eid(2)), RecordProblem::Undecodable),
            (JudgementId::new(eid(3)), RecordProblem::NameDisagrees),
        ]
    );
}

#[tokio::test]
async fn reading_a_record_back_as_a_judgement_is_reported_not_faked() {
    let root = tempfile::tempdir().unwrap();
    let world = World::of(Visibility::Public);
    let (ledger, objects) = (
        FsLedger::new(dirs(root.path())),
        FsObjects::new(dirs(root.path())),
    );
    world.approve(&ledger, &objects, 1).await;
    assert!(matches!(
        ledger.read(Visibility::Public).await,
        Err(LedgerFsError::DecoderMissing { records: 1, .. })
    ));
}

// --- the objects ----------------------------------------------------------------------------

#[tokio::test]
async fn an_object_is_stored_under_its_text_hash_and_read_back_checked() {
    let root = tempfile::tempdir().unwrap();
    let world = World::of(Visibility::Public);
    let (ledger, objects) = (
        FsLedger::new(dirs(root.path())),
        FsObjects::new(dirs(root.path())),
    );
    world.approve(&ledger, &objects, 1).await;

    let key = world.text.text_hash();
    assert_eq!(
        objects
            .get(Visibility::Public, key)
            .await
            .unwrap()
            .as_deref(),
        Some(world.text.as_str())
    );
    assert_eq!(
        file_names(&root.path().join("public/objects")).await,
        [key.hex()]
    );
    // The other side has no such object.
    assert_eq!(objects.get(Visibility::Private, key).await.unwrap(), None);

    // A file that does not hash to its name is corrupt, not a text.
    tokio::fs::write(root.path().join("public/objects").join(key.hex()), "other")
        .await
        .unwrap();
    assert!(matches!(
        objects.get(Visibility::Public, key).await,
        Err(LedgerFsError::CorruptObject { .. })
    ));
}

#[tokio::test]
async fn a_purge_deletes_the_object_and_the_hash_is_never_put_back() {
    let root = tempfile::tempdir().unwrap();
    let world = World::of(Visibility::Public);
    let (ledger, objects) = (
        FsLedger::new(dirs(root.path())),
        FsObjects::new(dirs(root.path())),
    );
    world.approve(&ledger, &objects, 1).await;
    let key = world.text.text_hash();
    let object_file = root.path().join("public/objects").join(key.hex());
    assert!(object_file.exists());

    append_without_object(
        &ledger,
        world.purge(2, "secret"),
        &world.corpus,
        &world.ledgers(),
    )
    .await
    .unwrap();
    assert!(
        !object_file.exists(),
        "the purge took the text off the disk"
    );
    assert_eq!(objects.get(Visibility::Public, key).await.unwrap(), None);

    // A put of the purged hash stores nothing and says so.
    let approval = world.approval(3, Visibility::Public);
    let secret = RegexSecretScan::new().unwrap();
    let scanned = scan(
        &secret,
        &world.corpus,
        &world.ledgers(),
        &world.text,
        &approval,
    )
    .await
    .unwrap();
    assert_eq!(objects.put(&scanned).await.unwrap(), Put::Purged);
    assert!(!object_file.exists());
}

#[tokio::test]
async fn a_branch_that_stored_the_object_before_the_purge_reads_it_as_gone_after_the_merge() {
    let root = tempfile::tempdir().unwrap();
    let world = World::of(Visibility::Public);
    let (ledger, objects) = (
        FsLedger::new(dirs(root.path())),
        FsObjects::new(dirs(root.path())),
    );
    world.approve(&ledger, &objects, 1).await;
    let key = world.text.text_hash();
    let object_file = root.path().join("public/objects").join(key.hex());
    let kept = tokio::fs::read(&object_file).await.unwrap();

    append_without_object(
        &ledger,
        world.purge(2, "secret"),
        &world.corpus,
        &world.ledgers(),
    )
    .await
    .unwrap();
    // The merge brings back the file the other branch had: the union of files.
    tokio::fs::write(&object_file, kept).await.unwrap();
    assert!(object_file.exists());

    assert_eq!(
        objects.get(Visibility::Public, key).await.unwrap(),
        None,
        "the purge record is the fact, whatever files are there"
    );
}

#[tokio::test]
async fn a_purge_on_one_side_purges_nothing_on_the_other() {
    let root = tempfile::tempdir().unwrap();
    let public = World::of(Visibility::Public);
    let private = World::of(Visibility::Private);
    let (ledger, objects) = (
        FsLedger::new(dirs(root.path())),
        FsObjects::new(dirs(root.path())),
    );
    public.approve(&ledger, &objects, 1).await;
    private.approve(&ledger, &objects, 2).await;
    let key = public.text.text_hash();
    assert_eq!(key, private.text.text_hash());

    append_without_object(
        &ledger,
        public.purge(3, "gone"),
        &public.corpus,
        &public.ledgers(),
    )
    .await
    .unwrap();
    assert_eq!(objects.get(Visibility::Public, key).await.unwrap(), None);
    assert!(
        objects
            .get(Visibility::Private, key)
            .await
            .unwrap()
            .is_some(),
        "the private side still holds it"
    );
}

#[tokio::test]
async fn a_purge_record_that_is_itself_corrupt_purges_nothing() {
    let root = tempfile::tempdir().unwrap();
    let world = World::of(Visibility::Public);
    let (ledger, objects) = (
        FsLedger::new(dirs(root.path())),
        FsObjects::new(dirs(root.path())),
    );
    world.approve(&ledger, &objects, 1).await;
    let corpus = &world.corpus;
    let ledgers = world.ledgers();
    // Two purges under one identifier that differ: the domain sets both aside.
    append_without_object(&ledger, world.purge(2, "one"), corpus, &ledgers)
        .await
        .unwrap();
    append_without_object(&ledger, world.purge(2, "two"), corpus, &ledgers)
        .await
        .unwrap();
    // The text was taken off the disk when the first was appended; a purge that does
    // not stand does not keep a put away.
    let approval = world.approval(3, Visibility::Public);
    let secret = RegexSecretScan::new().unwrap();
    let scanned = scan(&secret, corpus, &ledgers, &world.text, &approval)
        .await
        .unwrap();
    assert_eq!(objects.put(&scanned).await.unwrap(), Put::Stored);
}
