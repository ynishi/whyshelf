//! Publishing a private document: the plan that is shown before anything is done.
//!
//! Publishing moves a document from the private root to the public one, and carries
//! with it what people decided about it. [`plan_publication`] reads the corpus and
//! the ledgers and says what would be done, in the order it would be done, or refuses.
//! The same plan is shown by a dry run and by the real run, and its
//! [`PlanDigest`] is what a person confirms.
//!
//! # The steps, and the commit point
//!
//! ```text
//!   1 CopyRecordsAndObjects   the records and the texts they point at, into the public ledger
//!   2 CreatePublicFile        the file, byte for byte, under the public root (never over another)
//!   3 WritePublished          one record that names the copied records: the commit point
//!   4 RemovePrivateFile       the private file, last
//! ```
//!
//! Until step 3 the public side sees nothing: a record copied by step 1 is inert (it
//! does not count until a `Published` of its own document names it), so stopping
//! before 3 leaves nothing to clean up. Every step is safe to run again, and a plan
//! is made again each time from what is there.
//!
//! | what the corpus shows | the plan |
//! |---|---|
//! | the document in the private root only | all four steps |
//! | [`PublishForm::FileCopied`]: step 2 is done, 3 is not | steps 1, 3 and 4 (the records are looked for again; those already copied are skipped) |
//! | [`PublishForm::PrivateCopyLeft`]: step 3 is done | step 4 only |
//!
//! # What is copied
//!
//! - **Records**: the approvals, adoptions and retirements of the document and of its
//!   items that no revocation takes away. Not copied: the ones that are revoked, the
//!   revocations that name them, and the purges. They are counted
//!   ([`NotCopied`]). A purge or a revoked approval is about a version that is not
//!   published, and its key is a fingerprint of that version.
//! - **Objects**: the texts of the versions the copied records point at, and no others.
//!   A text that a purge names is not copied.
//! - Records are copied as they are, byte for byte.
//!
//! # What cannot be undone
//!
//! Step 1 puts the document's id and the hashes of the versions it approved into a
//! file that Git keeps. A record that stays inert is still in the history.
//! [`PublicationPlan::irreversible`] says what, and a real run goes on only after a
//! person confirms the plan ([`PlanDigest`]; the confirmation itself is the
//! application's). So does removing a private copy whose bytes differ from the public
//! file: the private root may be outside Git, and the difference is lost
//! ([`PublicationPlan::private_copy_differs`]).
//!
//! # The leak check
//!
//! Everything that would cross is checked ([`boundary`](super::boundary)): the records,
//! the object texts, the text of the file, its destination, and the judge who writes
//! `Published` (a name, a mail or a tool name is prose too). All leaks are listed
//! and then no plan is made; a plan exists only when it has none. When the person's
//! private root cannot be read the check cannot run and the plan is refused. The
//! document's own alias and title belong in its text and are not leaks; those of
//! other private documents are.
//!
//! The text of the file is not given beside its bytes: the evidence carries the
//! **bytes**, and the check reads the text decoded from those same bytes
//! ([`DocumentText::from_bytes`]; a byte that is not UTF-8 is read as a replacement
//! character, which is enough for a check). The plan first compares
//! [`BytesHash::of`] the bytes with the [`BytesHash`] the corpus has for the file, and
//! refuses a mismatch ([`PublicationError::EvidenceStale`]): what is checked is what
//! is copied. The digest pins that same [`BytesHash`].
//!
//! When the leak is in the file already made public ([`PublishForm::FileCopied`]) it
//! cannot be edited away, because the changed bytes would no longer be a copy of the
//! private file; the way out is [`LeakRemedy::AbortPublish`].

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use super::boundary::{
    CrossingText, DocumentText, ObjectText, PrivateNames, PrivateRoot, check_crossing,
};
use super::canon::{Canon, ToCanon};
use super::corpus::{Corpus, Invalidity, PublishForm, Visibility};
use super::error::{LeakRefusal, PublicationError};
use super::hash::{BytesHash, ContentHash, HashKind, TextHash};
use super::id::DocId;
use super::judge::Judge;
use super::judgement::{Judgement, JudgementId, JudgementKind, ObjectKey};
use super::ledger::Ledgers;
use super::location::RelPath;
use super::nonempty::NonEmptyVec;

/// The texts of the objects that exist, as the application read them from the
/// stores.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ObjectTexts {
    by_key: BTreeMap<TextHash, ObjectText>,
}

impl ObjectTexts {
    /// No texts.
    pub fn new() -> Self {
        Self::default()
    }

    /// The texts given.
    pub fn of(objects: impl IntoIterator<Item = ObjectText>) -> Self {
        Self {
            by_key: objects
                .into_iter()
                .map(|object| (object.key(), object))
                .collect(),
        }
    }

    /// The text kept under a hash.
    pub fn get(&self, key: &TextHash) -> Option<&ObjectText> {
        self.by_key.get(key)
    }
}

/// What the application read, beside the corpus and the ledgers, for a plan.
#[derive(Debug, Clone, Copy)]
pub struct PublishEvidence<'a> {
    /// The texts of the objects.
    pub objects: &'a ObjectTexts,
    /// The bytes of the file that is or will be public: the private file for a new
    /// publication, the public file of one that stopped after step 2. Their
    /// [`BytesHash`] must be the one the corpus has for that file
    /// ([`PublicationError::EvidenceStale`]). Not read for a publication that
    /// stopped after step 3.
    pub document: &'a [u8],
    /// Whether this person has a private root here, and whether it can be read.
    pub private_root: PrivateRoot,
}

/// One step of a publication, in the order they are done.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum PublicationStep {
    /// Copy the records and the objects into the public ledger (those already there
    /// are skipped).
    CopyRecordsAndObjects,
    /// Create the file under the public root; it must not exist.
    CreatePublicFile,
    /// Write `Published`, the commit point.
    WritePublished,
    /// Remove the private file.
    RemovePrivateFile,
}

impl PublicationStep {
    /// The word for the step.
    pub fn name(self) -> &'static str {
        match self {
            Self::CopyRecordsAndObjects => "copy-records-and-objects",
            Self::CreatePublicFile => "create-public-file",
            Self::WritePublished => "write-published",
            Self::RemovePrivateFile => "remove-private-file",
        }
    }
}

/// What to do about a leak.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum LeakRemedy {
    /// Nothing is public yet: take the name out (revoke the approval, purge the
    /// version, edit the private file), and plan again.
    FixAndPlanAgain,
    /// The file is public already and cannot be edited into a copy again: abort the
    /// publication, which removes the public file.
    AbortPublish,
}

/// A record the plan carries.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlannedRecord {
    id: JudgementId,
    kind: &'static str,
    in_public_ledger: bool,
}

impl PlannedRecord {
    /// The record.
    pub fn id(&self) -> JudgementId {
        self.id
    }

    /// The word for its kind (`approve`, `adopt`, `retire`).
    pub fn kind(&self) -> &'static str {
        self.kind
    }

    /// Whether the public ledger holds it already, so that step 1 skips it.
    pub fn in_public_ledger(&self) -> bool {
        self.in_public_ledger
    }
}

/// How many records about the document are not copied, and why.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct NotCopied {
    /// Approvals, adoptions and retirements that a revocation takes away.
    pub revoked: usize,
    /// Revocations of those.
    pub revocations: usize,
    /// Purges of versions of the document.
    pub purges: usize,
}

impl NotCopied {
    /// All of them.
    pub fn total(&self) -> usize {
        self.revoked + self.revocations + self.purges
    }
}

/// What a real run of step 1 puts where it cannot be taken back.
#[derive(Debug, Clone, Copy)]
pub struct Irreversible<'a> {
    doc: DocId,
    versions: &'a [ObjectKey],
}

impl<'a> Irreversible<'a> {
    /// The document whose id is written into the public ledger.
    pub fn doc(&self) -> DocId {
        self.doc
    }

    /// The versions whose two hashes (what was judged, and the text kept) are
    /// written there.
    pub fn versions(&self) -> &'a [ObjectKey] {
        self.versions
    }
}

/// The hash of what a plan does: what a person confirms, and what a run checks it
/// against just before it starts.
///
/// It covers the document, the form, the destination, the steps, the records, the
/// versions, and the bytes of the files the plan names (the private file, and the
/// public one when it exists). A change to any of them changes the digest, so a
/// confirmation does not carry over to a plan that has changed under it.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PlanDigest(ContentHash);

impl PlanDigest {
    /// The first 8 characters of the digest: what a person types back.
    pub fn short(&self) -> String {
        self.0.hex().chars().take(8).collect()
    }

    /// The whole digest.
    pub fn hash(&self) -> &ContentHash {
        &self.0
    }
}

impl fmt::Debug for PlanDigest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "PlanDigest({})", self.short())
    }
}

impl fmt::Display for PlanDigest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(&self.0, f)
    }
}

/// What publishing a document would do. It has no leak: it can be made no other way
/// than by [`plan_publication`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PublicationPlan {
    doc: DocId,
    form: PlanForm,
    destination: RelPath,
    steps: Vec<PublicationStep>,
    records: Vec<PlannedRecord>,
    not_copied: NotCopied,
    objects: Vec<ObjectKey>,
    private_file: BytesHash,
    digest: PlanDigest,
}

/// The point a plan starts from, with what only that point knows. A new publication
/// has no public file; one that stopped has it, and knows whether the private copy
/// to be removed differs from it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PlanForm {
    /// A new publication.
    Fresh,
    /// A publication that stopped after step 2 ([`PublishForm::FileCopied`]).
    FileCopied {
        public_file: BytesHash,
        private_copy_differs: bool,
    },
    /// A publication that stopped after step 3 ([`PublishForm::PrivateCopyLeft`]).
    PrivateCopyLeft {
        public_file: BytesHash,
        private_copy_differs: bool,
    },
}

impl PlanForm {
    fn publish_form(self) -> Option<PublishForm> {
        match self {
            Self::Fresh => None,
            Self::FileCopied { .. } => Some(PublishForm::FileCopied),
            Self::PrivateCopyLeft { .. } => Some(PublishForm::PrivateCopyLeft),
        }
    }

    fn public_file(self) -> Option<BytesHash> {
        match self {
            Self::Fresh => None,
            Self::FileCopied { public_file, .. } | Self::PrivateCopyLeft { public_file, .. } => {
                Some(public_file)
            }
        }
    }

    fn private_copy_differs(self) -> bool {
        match self {
            Self::Fresh => false,
            Self::FileCopied {
                private_copy_differs,
                ..
            }
            | Self::PrivateCopyLeft {
                private_copy_differs,
                ..
            } => private_copy_differs,
        }
    }

    fn name(self) -> &'static str {
        match self {
            Self::Fresh => "new",
            Self::FileCopied { .. } => "file-copied",
            Self::PrivateCopyLeft { .. } => "private-copy-left",
        }
    }
}

impl PublicationPlan {
    /// The document.
    pub fn doc(&self) -> DocId {
        self.doc
    }

    /// The point a publication that stopped is at; `None` for a new one.
    pub fn form(&self) -> Option<PublishForm> {
        self.form.publish_form()
    }

    /// The path of the public file, under the public root.
    pub fn destination(&self) -> &RelPath {
        &self.destination
    }

    /// The steps to do, in order.
    pub fn steps(&self) -> &[PublicationStep] {
        &self.steps
    }

    /// The records to carry, by identifier. Empty when only step 4 is left.
    pub fn records(&self) -> &[PlannedRecord] {
        &self.records
    }

    /// The ids `Published` names: every record carried, whether or not step 1 has
    /// to write it.
    pub fn published_records(&self) -> BTreeSet<JudgementId> {
        self.records.iter().map(PlannedRecord::id).collect()
    }

    /// The records step 1 has to write: those the public ledger lacks.
    pub fn records_to_write(&self) -> impl Iterator<Item = &PlannedRecord> {
        self.records
            .iter()
            .filter(|record| !record.in_public_ledger)
    }

    /// How many records are carried.
    pub fn copied_count(&self) -> usize {
        self.records.len()
    }

    /// The records about the document that are not carried, counted.
    pub fn not_copied(&self) -> NotCopied {
        self.not_copied
    }

    /// The versions whose texts are copied, in order.
    pub fn objects(&self) -> &[ObjectKey] {
        &self.objects
    }

    /// The bytes of the private file this plan removes (the private copy, for a
    /// publication that stopped after step 3).
    pub fn private_file(&self) -> BytesHash {
        self.private_file
    }

    /// Whether the private copy to be removed differs from the public file: the
    /// difference is lost with it. Always `false` for a new publication.
    pub fn private_copy_differs(&self) -> bool {
        self.form.private_copy_differs()
    }

    /// What step 1 would put where it cannot be taken back; `None` when the plan has
    /// no step 1.
    pub fn irreversible(&self) -> Option<Irreversible<'_>> {
        self.steps
            .contains(&PublicationStep::CopyRecordsAndObjects)
            .then_some(Irreversible {
                doc: self.doc,
                versions: &self.objects,
            })
    }

    /// Whether a real run needs a person's confirmation: it does when it has step 1,
    /// or removes a private copy that differs.
    pub fn needs_human_confirmation(&self) -> bool {
        self.irreversible().is_some() || self.private_copy_differs()
    }

    /// The digest of the plan.
    pub fn digest(&self) -> PlanDigest {
        self.digest
    }
}

/// Where the document is, and what that makes of the plan.
struct Located {
    form: PlanForm,
    destination: RelPath,
    private_file: BytesHash,
}

fn locate(doc: DocId, corpus: &Corpus, to: &RelPath) -> Result<Located, PublicationError> {
    let files = corpus.by_id(&doc);
    if files.is_empty() {
        return Err(PublicationError::NotFound { doc });
    }
    // A private file that Git would add is what a publication ends; every other
    // reason stops it.
    let reasons: Vec<Invalidity> = files
        .iter()
        .flat_map(|file| file.standing().reasons())
        .filter(|reason| !matches!(reason, Invalidity::PrivateExposed { .. }))
        .cloned()
        .collect();
    if let Ok(reasons) = NonEmptyVec::from_vec(reasons) {
        return Err(PublicationError::DocInvalid { doc, reasons });
    }
    if let Some(open) = corpus.publishing_of(&doc) {
        let destination = open.public().location().path().clone();
        if destination != *to {
            return Err(PublicationError::DestinationDiffers {
                file: destination,
                to: to.clone(),
            });
        }
        let private = open.private_copy().facts().bytes();
        let public_file = open.public().bytes();
        let private_copy_differs = private != public_file;
        let form = match open.form() {
            PublishForm::FileCopied => PlanForm::FileCopied {
                public_file,
                private_copy_differs,
            },
            PublishForm::PrivateCopyLeft => PlanForm::PrivateCopyLeft {
                public_file,
                private_copy_differs,
            },
        };
        return Ok(Located {
            form,
            destination,
            private_file: private,
        });
    }
    let [file] = files else {
        return Err(PublicationError::AlreadyPublic { doc });
    };
    if file.location().visibility() == Visibility::Public {
        return Err(PublicationError::AlreadyPublic { doc });
    }
    let taken = corpus
        .documents()
        .iter()
        .map(|document| document.location())
        .chain(corpus.candidates().iter().map(|c| c.facts().location()))
        .chain(corpus.unparsed().iter().map(|u| u.facts().location()))
        .any(|location| location.visibility() == Visibility::Public && location.path() == to);
    if taken {
        return Err(PublicationError::DestinationTaken { to: to.clone() });
    }
    Ok(Located {
        form: PlanForm::Fresh,
        destination: to.clone(),
        private_file: file.facts().bytes(),
    })
}

/// The records about `doc` that count, and what is left out.
fn records_of(doc: DocId, ledgers: &Ledgers) -> (Vec<&Judgement>, NotCopied) {
    let about: Vec<&Judgement> = ledgers
        .judgements()
        .filter(|record| record.kind().is_revocable() && ledgers.is_of_doc(record, doc))
        .collect();
    let (revoked, carried): (Vec<&Judgement>, Vec<&Judgement>) = about
        .iter()
        .partition(|record| ledgers.is_revoked(&record.id()));
    let revoked_ids: BTreeSet<JudgementId> = revoked.iter().map(|record| record.id()).collect();
    let mut not_copied = NotCopied {
        revoked: revoked.len(),
        ..NotCopied::default()
    };
    for record in ledgers.judgements() {
        match record.kind() {
            JudgementKind::Revoke { targets, .. }
                if targets.iter().any(|target| revoked_ids.contains(target)) =>
            {
                not_copied.revocations += 1;
            }
            JudgementKind::Purge { doc: purged, .. } if *purged == doc => not_copied.purges += 1,
            JudgementKind::Approve { .. }
            | JudgementKind::Adopt { .. }
            | JudgementKind::Revoke { .. }
            | JudgementKind::Retire { .. }
            | JudgementKind::Published { .. }
            | JudgementKind::Purge { .. } => {}
        }
    }
    (carried, not_copied)
}

/// Plan the publication of `doc` to `to` under the public root.
///
/// `to` is the path of the public file; for a publication that stopped after step 2
/// or 3 it is where that file is. `judge` is who will write the `Published` record:
/// it is public text, and is checked with the rest. The plan is made again each
/// time, from what the corpus and the ledgers show now. Nothing is planned when any
/// text that would cross names a private document, when the bytes given are not
/// those of the file, or when the check cannot run: see the module documentation.
pub fn plan_publication(
    doc: DocId,
    corpus: &Corpus,
    ledgers: &Ledgers,
    to: RelPath,
    judge: &Judge,
    evidence: &PublishEvidence<'_>,
) -> Result<PublicationPlan, PublicationError> {
    let at = locate(doc, corpus, &to)?;

    if matches!(at.form, PlanForm::PrivateCopyLeft { .. }) {
        return Ok(seal(
            doc,
            at,
            vec![PublicationStep::RemovePrivateFile],
            Carried::default(),
        ));
    }

    // The file that is or will be public: the public one when it exists already.
    let expected = at.form.public_file().unwrap_or(at.private_file);
    let found = BytesHash::of(evidence.document);
    if found != expected {
        return Err(PublicationError::EvidenceStale { expected, found });
    }

    let (carried, not_copied) = records_of(doc, ledgers);
    let mut keys: BTreeSet<ObjectKey> = BTreeSet::new();
    for record in &carried {
        if let Some(key) = record.object()
            && !ledgers.is_purged(key.text())
        {
            keys.insert(*key);
        }
    }

    let mut crossing = CrossingText::default();
    crossing.include(&at.destination);
    for record in &carried {
        crossing.include(*record);
    }
    for key in &keys {
        let object = evidence
            .objects
            .get(key.text())
            .ok_or(PublicationError::ObjectUnavailable { text: *key.text() })?;
        crossing.include(object);
    }
    crossing.include(&DocumentText::from_bytes(evidence.document));
    crossing.include(judge);

    let names = PrivateNames::of(corpus, evidence.private_root, &BTreeSet::from([doc]));
    check_crossing(&crossing, &names).map_err(|refusal| match refusal {
        LeakRefusal::Uncheckable => PublicationError::LeakUncheckable,
        LeakRefusal::Leaks(leaks) => {
            let remedy = match at.form {
                PlanForm::Fresh => LeakRemedy::FixAndPlanAgain,
                PlanForm::FileCopied { .. } | PlanForm::PrivateCopyLeft { .. } => {
                    LeakRemedy::AbortPublish
                }
            };
            PublicationError::Leaks { leaks, remedy }
        }
    })?;

    let mut records: Vec<PlannedRecord> = carried
        .iter()
        .map(|record| PlannedRecord {
            id: record.id(),
            kind: record.kind().name(),
            in_public_ledger: ledgers.public().contains(&record.id()),
        })
        .collect();
    records.sort_by_key(PlannedRecord::id);

    let mut steps = vec![PublicationStep::CopyRecordsAndObjects];
    if at.form == PlanForm::Fresh {
        steps.push(PublicationStep::CreatePublicFile);
    }
    steps.push(PublicationStep::WritePublished);
    steps.push(PublicationStep::RemovePrivateFile);

    Ok(seal(
        doc,
        at,
        steps,
        Carried {
            records,
            not_copied,
            objects: keys.into_iter().collect(),
        },
    ))
}

/// The records and versions a plan carries.
#[derive(Default)]
struct Carried {
    records: Vec<PlannedRecord>,
    not_copied: NotCopied,
    objects: Vec<ObjectKey>,
}

/// Make the plan and its digest. The one constructor of [`PublicationPlan`].
fn seal(doc: DocId, at: Located, steps: Vec<PublicationStep>, carried: Carried) -> PublicationPlan {
    let Located {
        form,
        destination,
        private_file,
    } = at;
    let Carried {
        records,
        not_copied,
        objects,
    } = carried;
    let canon = Canon::map([
        ("doc", doc.to_canon()),
        ("form", Canon::str(form.name())),
        ("destination", destination.to_canon()),
        (
            "steps",
            Canon::list(steps.iter().map(|step| Canon::str(step.name()))),
        ),
        (
            "records",
            Canon::list(records.iter().map(|record| record.id.to_canon())),
        ),
        (
            "objects",
            Canon::list(objects.iter().map(ToCanon::to_canon)),
        ),
        ("private_file", Canon::str(private_file.to_string())),
        (
            "public_file",
            form.public_file()
                .map_or(Canon::Null, |bytes| Canon::str(bytes.to_string())),
        ),
        (
            "private_copy_differs",
            Canon::Bool(form.private_copy_differs()),
        ),
    ]);
    PublicationPlan {
        doc,
        form,
        destination,
        steps,
        records,
        not_copied,
        objects,
        private_file,
        digest: PlanDigest(ContentHash::of(HashKind::Plan, &canon)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::approval::{Attestation, Pinned};
    use crate::domain::boundary::CrossingField;
    use crate::domain::corpus::{Document, Entity, FileFacts, GitPlace, Header, Location, Reading};
    use crate::domain::design::test_support::{design, identified};
    use crate::domain::finding::test_support::finding;
    use crate::domain::gate::Passed;
    use crate::domain::id::test_support::{did, docid, iid, sid};
    use crate::domain::judge::test_support::judge;
    use crate::domain::judge::{Email, PersonName, Principal, ToolName};
    use crate::domain::judgement::test_support::{
        approve, ca_passed, hash_of, jid, object_of, ts, why,
    };
    use crate::domain::judgement::{FilePresence, Target};
    use crate::domain::knowable::Knowable;
    use crate::domain::ledger::Ledger;
    use crate::domain::nonempty::NonBlank;

    /// The document published: a private Design, id 21, with an item, id 31.
    const DOC: u128 = 21;

    fn ada() -> Judge {
        judge("ada")
    }

    fn rel(path: &str) -> RelPath {
        path.parse().unwrap()
    }

    fn file(
        entity: Entity,
        alias: Option<&str>,
        visibility: Visibility,
        path: &str,
        bytes: &[u8],
    ) -> (FileFacts, Reading) {
        let header = Header::new(
            entity.id(),
            entity.kind(),
            alias.map(|a| a.parse().unwrap()),
            BTreeSet::new(),
            None,
        );
        let git = match visibility {
            Visibility::Public => GitPlace::Tracked,
            Visibility::Private => GitPlace::Ignored,
        };
        (
            FileFacts::new(
                Location::new(visibility, rel(path), Knowable::Known(git)),
                BytesHash::of(bytes),
            ),
            Reading::Document(Document::new(header, entity).unwrap()),
        )
    }

    fn design_entity() -> Entity {
        Entity::Design(design(DOC, 11, vec![(1, identified(31, "the item", &[]))]))
    }

    /// Another private document, whose alias must not cross.
    fn secret() -> (FileFacts, Reading) {
        file(
            Entity::Finding(finding(1, "body")),
            Some("finding/secret-plan"),
            Visibility::Private,
            "secret.md",
            b"secret",
        )
    }

    fn private_design(bytes: &[u8]) -> (FileFacts, Reading) {
        file(design_entity(), None, Visibility::Private, "d21.md", bytes)
    }

    fn corpus_of(files: Vec<(FileFacts, Reading)>) -> Corpus {
        Corpus::build(files, &BTreeSet::new())
    }

    fn ci_passed(tag: &str) -> Passed {
        Passed::for_test(Attestation::Ci {
            design: Pinned::new(did(DOC), hash_of(tag)),
            spec: Pinned::new(sid(11), hash_of("spec").into()),
        })
    }

    /// The records about the document: two that stand (r1 the Design, r2 an item), one
    /// that is revoked (r3) with its revocation (r4), and a purge (r5); and one about
    /// another document, which is nobody's business here.
    fn records() -> Vec<Judgement> {
        vec![
            approve(1, &ada(), Visibility::Private, ci_passed("d1")),
            approve(2, &ada(), Visibility::Private, ca_passed(31, "i1")),
            approve(3, &ada(), Visibility::Private, ca_passed(31, "i0")),
            Judgement::assemble(
                jid(4),
                ts(),
                ada(),
                Visibility::Private,
                JudgementKind::Revoke {
                    targets: NonEmptyVec::new(jid(3), Vec::new()),
                    reason: why(),
                },
            ),
            Judgement::purge(
                jid(5),
                ts(),
                ada(),
                Visibility::Private,
                docid(DOC),
                TextHash::of("an old version"),
                why(),
            ),
            approve(
                6,
                &ada(),
                Visibility::Private,
                crate::domain::judgement::test_support::cr_passed(1, "other"),
            ),
        ]
    }

    fn ledgers_of(private: Vec<Judgement>) -> Ledgers {
        Ledgers::new(Ledger::new(), Ledger::from_judgements(private))
    }

    /// The text of every object the records point at.
    fn texts_of(ledgers: &Ledgers) -> ObjectTexts {
        ObjectTexts::of(
            ledgers
                .judgements()
                .filter_map(Judgement::object)
                .map(|key| ObjectText::new(format!("text of {}", key.content()))),
        )
    }

    /// The bytes of a clean file: what the fixtures that say `b"bytes"` hold.
    fn body() -> &'static [u8] {
        b"bytes"
    }

    fn plan_with(
        corpus: &Corpus,
        ledgers: &Ledgers,
        to: &str,
        root: PrivateRoot,
        document: &[u8],
    ) -> Result<PublicationPlan, PublicationError> {
        let objects = texts_of(ledgers);
        plan_publication(
            docid(DOC),
            corpus,
            ledgers,
            rel(to),
            &ada(),
            &PublishEvidence {
                objects: &objects,
                document,
                private_root: root,
            },
        )
    }

    fn plan(corpus: &Corpus, ledgers: &Ledgers) -> Result<PublicationPlan, PublicationError> {
        plan_with(corpus, ledgers, "docs/d21.md", PrivateRoot::Present, body())
    }

    fn fresh() -> (Corpus, Ledgers) {
        (
            corpus_of(vec![private_design(b"bytes"), secret()]),
            ledgers_of(records()),
        )
    }

    // --- what the plan holds --------------------------------------------------

    #[test]
    fn a_plan_has_the_four_steps_in_order() {
        let (corpus, ledgers) = fresh();
        let plan = plan(&corpus, &ledgers).unwrap();
        assert_eq!(
            plan.steps(),
            [
                PublicationStep::CopyRecordsAndObjects,
                PublicationStep::CreatePublicFile,
                PublicationStep::WritePublished,
                PublicationStep::RemovePrivateFile,
            ]
        );
        assert_eq!(plan.form(), None);
        assert_eq!(plan.doc(), docid(DOC));
        assert_eq!(plan.destination(), &rel("docs/d21.md"));
        assert_eq!(plan.private_file(), BytesHash::of(b"bytes"));
        assert!(!plan.private_copy_differs());
    }

    #[test]
    fn the_standing_records_are_carried_and_the_others_are_counted() {
        let (corpus, ledgers) = fresh();
        let plan = plan(&corpus, &ledgers).unwrap();
        // The approval of the Design and of the item; not the revoked one, not its
        // revocation, not the purge, and not the record about another document.
        assert_eq!(plan.published_records(), BTreeSet::from([jid(1), jid(2)]));
        assert_eq!(plan.copied_count(), 2);
        assert!(plan.records().iter().all(|r| r.kind() == "approve"));
        assert_eq!(
            plan.not_copied(),
            NotCopied {
                revoked: 1,
                revocations: 1,
                purges: 1
            }
        );
        assert_eq!(plan.not_copied().total(), 3);
        // Both are written, none is in the public ledger yet.
        assert_eq!(plan.records_to_write().count(), 2);
    }

    #[test]
    fn only_the_versions_the_carried_records_point_at_are_copied() {
        let (corpus, ledgers) = fresh();
        let plan = plan(&corpus, &ledgers).unwrap();
        let expected: BTreeSet<ObjectKey> = [jid(1), jid(2)]
            .iter()
            .map(|id| *ledgers.get(id).unwrap().object().unwrap())
            .collect();
        assert_eq!(
            plan.objects().iter().copied().collect::<BTreeSet<_>>(),
            expected
        );
        // The revoked approval's version is not among them.
        let revoked = *ledgers.get(&jid(3)).unwrap().object().unwrap();
        assert!(!plan.objects().contains(&revoked));
    }

    #[test]
    fn a_retirement_of_an_item_of_the_document_is_carried() {
        let mut all = records();
        all.push(Judgement::assemble(
            jid(7),
            ts(),
            ada(),
            Visibility::Private,
            JudgementKind::Retire {
                target: Target::Item(iid(31)),
                reason: why(),
            },
        ));
        let corpus = corpus_of(vec![private_design(b"bytes")]);
        let plan = plan(&corpus, &ledgers_of(all)).unwrap();
        let carried = plan.published_records();
        assert!(carried.contains(&jid(7)));
        assert!(
            plan.records()
                .iter()
                .any(|r| r.id() == jid(7) && r.kind() == "retire")
        );
    }

    #[test]
    fn what_was_already_copied_is_marked_and_not_written_again() {
        let all = records();
        let copied = all[0].clone();
        let ledgers = Ledgers::new(
            Ledger::from_judgements([copied]),
            Ledger::from_judgements(all),
        );
        let corpus = corpus_of(vec![private_design(b"bytes")]);
        let plan = plan(&corpus, &ledgers).unwrap();
        // It still goes into `Published`; step 1 does not write it again.
        assert_eq!(plan.published_records(), BTreeSet::from([jid(1), jid(2)]));
        let to_write: Vec<JudgementId> = plan.records_to_write().map(PlannedRecord::id).collect();
        assert_eq!(to_write, vec![jid(2)]);
        assert!(
            plan.records()
                .iter()
                .find(|r| r.id() == jid(1))
                .unwrap()
                .in_public_ledger()
        );
    }

    #[test]
    fn a_purged_version_is_not_copied_and_needs_no_text() {
        let all = records();
        let key = *all[0].object().unwrap();
        let mut with_purge = all;
        with_purge.push(Judgement::purge(
            jid(8),
            ts(),
            ada(),
            Visibility::Private,
            docid(DOC),
            *key.text(),
            why(),
        ));
        let ledgers = ledgers_of(with_purge);
        let corpus = corpus_of(vec![private_design(b"bytes")]);
        // No text of that version is given.
        let objects = ObjectTexts::of(
            ledgers
                .judgements()
                .filter_map(Judgement::object)
                .filter(|k| k.text() != key.text())
                .map(|k| ObjectText::new(format!("text of {}", k.content()))),
        );
        let plan = plan_publication(
            docid(DOC),
            &corpus,
            &ledgers,
            rel("docs/d21.md"),
            &ada(),
            &PublishEvidence {
                objects: &objects,
                document: body(),
                private_root: PrivateRoot::Present,
            },
        )
        .unwrap();
        assert!(!plan.objects().contains(&key));
        assert_eq!(plan.copied_count(), 2);
        assert_eq!(plan.not_copied().purges, 2);
    }

    #[test]
    fn a_text_that_is_neither_given_nor_purged_stops_the_plan() {
        let (corpus, ledgers) = fresh();
        let nothing = ObjectTexts::new();
        let got = plan_publication(
            docid(DOC),
            &corpus,
            &ledgers,
            rel("docs/d21.md"),
            &ada(),
            &PublishEvidence {
                objects: &nothing,
                document: body(),
                private_root: PrivateRoot::Present,
            },
        );
        assert!(matches!(
            got,
            Err(PublicationError::ObjectUnavailable { .. })
        ));
    }

    #[test]
    fn a_step_that_cannot_be_undone_is_said_and_needs_a_person() {
        let (corpus, ledgers) = fresh();
        let plan = plan(&corpus, &ledgers).unwrap();
        let irreversible = plan.irreversible().unwrap();
        assert_eq!(irreversible.doc(), docid(DOC));
        assert_eq!(irreversible.versions(), plan.objects());
        assert!(plan.needs_human_confirmation());
    }

    // --- the leak check -----------------------------------------------------

    fn leaky_records(reason: &str) -> Vec<Judgement> {
        let mut all = records();
        // An item that is gone, retired with a reason that names a private document;
        // its approval pins the Design, which makes the retirement the document's.
        all.push(approve(
            9,
            &ada(),
            Visibility::Private,
            ca_passed(32, "gone"),
        ));
        all.push(
            Judgement::retire(
                jid(10),
                ts(),
                ada(),
                Visibility::Private,
                Target::Item(iid(32)),
                FilePresence::Absent,
                NonBlank::new(reason).unwrap(),
            )
            .unwrap(),
        );
        all
    }

    #[test]
    fn a_leak_in_a_record_refuses_the_plan() {
        let corpus = corpus_of(vec![private_design(b"bytes"), secret()]);
        let ledgers = ledgers_of(leaky_records("replaced by finding/secret-plan"));
        let Err(PublicationError::Leaks { leaks, remedy }) = plan(&corpus, &ledgers) else {
            panic!("expected leaks");
        };
        assert_eq!(remedy, LeakRemedy::FixAndPlanAgain);
        assert_eq!(leaks.len(), 1);
        assert_eq!(leaks.first().field(), CrossingField::Reason);
    }

    #[test]
    fn a_leak_in_a_copied_object_refuses_the_plan() {
        let mut all = records();
        let leaky = "an old text that cites finding/secret-plan";
        // The gate read the leaky text, so the approval keeps it.
        let passed = Passed::for_test_read(
            ci_passed("leaky").attestation().clone(),
            TextHash::of(leaky),
        );
        let content = *passed.attestation().target_hash();
        all.push(
            Judgement::approve(
                jid(11),
                ts(),
                ada(),
                Visibility::Private,
                passed,
                ObjectKey::new(content, TextHash::of(leaky)),
            )
            .unwrap(),
        );
        let ledgers = ledgers_of(all);
        let corpus = corpus_of(vec![private_design(b"bytes"), secret()]);
        let mut objects = texts_of(&ledgers);
        objects = ObjectTexts::of(
            objects
                .by_key
                .values()
                .cloned()
                .chain([ObjectText::new(leaky.to_owned())]),
        );
        let got = plan_publication(
            docid(DOC),
            &corpus,
            &ledgers,
            rel("docs/d21.md"),
            &ada(),
            &PublishEvidence {
                objects: &objects,
                document: body(),
                private_root: PrivateRoot::Present,
            },
        );
        let Err(PublicationError::Leaks { leaks, .. }) = got else {
            panic!("expected leaks, got {got:?}");
        };
        assert_eq!(leaks.first().field(), CrossingField::Object);
    }

    #[test]
    fn all_the_leaks_are_listed_and_the_documents_own_names_are_not_among_them() {
        let id = docid(1).to_string();
        let leaky_document =
            format!("---\nspec: {id}\n---\nmentions finding/secret-plan and slow login\n");
        let corpus = corpus_of(vec![private_design(leaky_document.as_bytes()), secret()]);
        let ledgers = ledgers_of(leaky_records("see finding/secret-plan"));
        let got = plan_with(
            &corpus,
            &ledgers,
            &format!("docs/{id}.md"),
            PrivateRoot::Present,
            leaky_document.as_bytes(),
        );
        let Err(PublicationError::Leaks { leaks, .. }) = got else {
            panic!("expected leaks, got {got:?}");
        };
        let fields: BTreeSet<CrossingField> = leaks.iter().map(|leak| leak.field()).collect();
        assert_eq!(
            fields,
            BTreeSet::from([
                CrossingField::Reason,
                CrossingField::Path,
                CrossingField::FrontMatter,
                CrossingField::Body,
            ])
        );
        // Its own id, alias and item ids are the document's to write.
        let own = format!(
            "---\nid: {}\nalias: design/own\n---\nthe item {}\n",
            docid(DOC),
            iid(31)
        );
        let own_corpus = corpus_of(vec![
            file(
                design_entity(),
                Some("design/own"),
                Visibility::Private,
                "d21.md",
                own.as_bytes(),
            ),
            secret(),
        ]);
        assert!(
            plan_with(
                &own_corpus,
                &ledgers_of(records()),
                "docs/d21.md",
                PrivateRoot::Present,
                own.as_bytes()
            )
            .is_ok()
        );
    }

    #[test]
    fn a_name_found_only_in_the_bytes_is_caught() {
        // The text checked is decoded from the bytes themselves: no text is given
        // beside them to be clean while the bytes are not.
        let bytes = b"---\nid: x\n---\nsee finding/secret-plan\n";
        let corpus = corpus_of(vec![private_design(bytes), secret()]);
        let ledgers = ledgers_of(records());
        let got = plan_with(
            &corpus,
            &ledgers,
            "docs/d21.md",
            PrivateRoot::Present,
            bytes,
        );
        let Err(PublicationError::Leaks { leaks, remedy }) = got else {
            panic!("expected leaks, got {got:?}");
        };
        assert_eq!(remedy, LeakRemedy::FixAndPlanAgain);
        assert_eq!(leaks.len(), 1);
        assert_eq!(leaks.first().field(), CrossingField::Body);

        // Bytes that are not UTF-8 still have the rest read.
        let lossy = b"\xff\xfe finding/secret-plan";
        let corpus = corpus_of(vec![private_design(lossy), secret()]);
        let got = plan_with(
            &corpus,
            &ledgers,
            "docs/d21.md",
            PrivateRoot::Present,
            lossy,
        );
        assert!(matches!(got, Err(PublicationError::Leaks { .. })));
    }

    #[test]
    fn bytes_that_are_not_those_of_the_file_are_stale() {
        let (corpus, ledgers) = fresh();
        // The file is `bytes`; a clean text of another content was read.
        let got = plan_with(
            &corpus,
            &ledgers,
            "docs/d21.md",
            PrivateRoot::Present,
            b"a clean body, but not the file",
        );
        assert_eq!(
            got,
            Err(PublicationError::EvidenceStale {
                expected: BytesHash::of(b"bytes"),
                found: BytesHash::of(b"a clean body, but not the file"),
            })
        );
        // Even one byte more.
        let got = plan_with(
            &corpus,
            &ledgers,
            "docs/d21.md",
            PrivateRoot::Present,
            b"bytes\n",
        );
        assert!(matches!(got, Err(PublicationError::EvidenceStale { .. })));

        // A publication that stopped after step 2 compares with the public file.
        let ledgers = ledgers_of(records());
        let corpus = cut_short(false, b"bytes");
        let same = plan_with(
            &corpus,
            &ledgers,
            "docs/d21.md",
            PrivateRoot::Present,
            b"bytes",
        );
        assert_eq!(same.unwrap().form(), Some(PublishForm::FileCopied));
        let other = plan_with(
            &corpus,
            &ledgers,
            "docs/d21.md",
            PrivateRoot::Present,
            b"other",
        );
        assert_eq!(
            other,
            Err(PublicationError::EvidenceStale {
                expected: BytesHash::of(b"bytes"),
                found: BytesHash::of(b"other"),
            })
        );
    }

    #[test]
    fn the_judge_who_writes_published_crosses_and_is_checked() {
        let (corpus, ledgers) = fresh();
        let objects = texts_of(&ledgers);
        let plan_as = |judge: &Judge| {
            plan_publication(
                docid(DOC),
                &corpus,
                &ledgers,
                rel("docs/d21.md"),
                judge,
                &PublishEvidence {
                    objects: &objects,
                    document: body(),
                    private_root: PrivateRoot::Present,
                },
            )
        };
        assert!(plan_as(&ada()).is_ok());

        let named = Judge::new(
            PersonName::new("ada of finding/secret-plan").unwrap(),
            None,
            Principal::Human,
        );
        let Err(PublicationError::Leaks { leaks, remedy }) = plan_as(&named) else {
            panic!("expected a leak in the name");
        };
        assert_eq!(remedy, LeakRemedy::FixAndPlanAgain);
        assert_eq!(leaks.len(), 1);
        assert_eq!(leaks.first().field(), CrossingField::JudgeName);

        let tool = Judge::new(
            PersonName::new("ada").unwrap(),
            None,
            Principal::Agent {
                tool: ToolName::new("finding/secret-plan").unwrap(),
            },
        );
        let Err(PublicationError::Leaks { leaks, .. }) = plan_as(&tool) else {
            panic!("expected a leak in the tool");
        };
        assert_eq!(leaks.first().field(), CrossingField::JudgeTool);

        let mail = Judge::new(
            PersonName::new("ada").unwrap(),
            Some(Email::new("finding/secret-plan@example.org").unwrap()),
            Principal::Human,
        );
        let Err(PublicationError::Leaks { leaks, .. }) = plan_as(&mail) else {
            panic!("expected a leak in the mail");
        };
        assert_eq!(leaks.first().field(), CrossingField::JudgeEmail);
    }

    #[test]
    fn a_destination_built_from_a_private_alias_refuses_the_plan() {
        let (corpus, ledgers) = fresh();
        for to in ["finding/secret-plan.md", "docs/finding/secret-plan.md"] {
            let got = plan_with(&corpus, &ledgers, to, PrivateRoot::Present, body());
            let Err(PublicationError::Leaks { leaks, remedy }) = got else {
                panic!("expected a leak in {to}, got {got:?}");
            };
            assert_eq!(remedy, LeakRemedy::FixAndPlanAgain);
            assert_eq!(leaks.len(), 1, "{to}: {leaks:?}");
            assert_eq!(leaks.first().field(), CrossingField::Path);
        }
        // Beside it, a name of its own is fine.
        assert!(
            plan_with(
                &corpus,
                &ledgers,
                "finding/secret-plan-review.md",
                PrivateRoot::Present,
                body()
            )
            .is_ok()
        );
    }

    #[test]
    fn the_private_root_decides_whether_the_check_runs() {
        let corpus = corpus_of(vec![private_design(b"bytes"), secret()]);
        let ledgers = ledgers_of(leaky_records("see finding/secret-plan"));
        let unreadable = plan_with(
            &corpus,
            &ledgers,
            "docs/d21.md",
            PrivateRoot::Absent,
            body(),
        );
        assert_eq!(unreadable, Err(PublicationError::LeakUncheckable));
        // Where this person has no private documents there is nothing to name.
        let none = plan_with(
            &corpus,
            &ledgers,
            "docs/d21.md",
            PrivateRoot::NotUsedHere,
            body(),
        );
        assert!(none.is_ok());
    }

    // --- where the document is ----------------------------------------------

    #[test]
    fn a_document_that_is_not_there_or_is_public_already_is_not_planned() {
        let (_, ledgers) = fresh();
        let empty = corpus_of(vec![secret()]);
        assert_eq!(
            plan(&empty, &ledgers),
            Err(PublicationError::NotFound { doc: docid(DOC) })
        );
        let public = corpus_of(vec![file(
            design_entity(),
            None,
            Visibility::Public,
            "d21.md",
            b"bytes",
        )]);
        assert_eq!(
            plan(&public, &ledgers),
            Err(PublicationError::AlreadyPublic { doc: docid(DOC) })
        );
    }

    #[test]
    fn a_document_that_cannot_be_trusted_is_not_planned_but_one_git_would_add_is() {
        let (_, ledgers) = fresh();
        let twin = file(
            Entity::Finding(finding(2, "x")),
            Some("design/shared"),
            Visibility::Private,
            "twin.md",
            b"t",
        );
        let named = file(
            design_entity(),
            Some("design/shared"),
            Visibility::Private,
            "d21.md",
            b"d",
        );
        let corpus = corpus_of(vec![named, twin]);
        assert!(matches!(
            plan(&corpus, &ledgers),
            Err(PublicationError::DocInvalid { .. })
        ));
        // A private file in a place Git would add is what a publication ends.
        let exposed = {
            let mut facts = private_design(b"bytes");
            facts.0 = FileFacts::new(
                Location::new(
                    Visibility::Private,
                    rel("d21.md"),
                    Knowable::Known(GitPlace::Tracked),
                ),
                BytesHash::of(b"bytes"),
            );
            corpus_of(vec![facts])
        };
        assert!(plan(&exposed, &ledgers).is_ok());
    }

    #[test]
    fn a_destination_with_a_file_is_not_planned() {
        let (_, ledgers) = fresh();
        let taken = corpus_of(vec![
            private_design(b"bytes"),
            file(
                Entity::Finding(finding(2, "x")),
                None,
                Visibility::Public,
                "docs/d21.md",
                b"x",
            ),
        ]);
        assert_eq!(
            plan(&taken, &ledgers),
            Err(PublicationError::DestinationTaken {
                to: rel("docs/d21.md")
            })
        );
    }

    fn cut_short(published: bool, private_bytes: &[u8]) -> Corpus {
        let public = file(
            design_entity(),
            None,
            Visibility::Public,
            "docs/d21.md",
            b"bytes",
        );
        let private = private_design(private_bytes);
        let marked: BTreeSet<DocId> = if published {
            BTreeSet::from([docid(DOC)])
        } else {
            BTreeSet::new()
        };
        Corpus::build([public, private], &marked)
    }

    #[test]
    fn a_publication_that_stopped_after_the_file_was_made_runs_the_rest() {
        let ledgers = ledgers_of(records());
        let corpus = cut_short(false, b"bytes");
        let plan = plan(&corpus, &ledgers).unwrap();
        assert_eq!(plan.form(), Some(PublishForm::FileCopied));
        assert_eq!(
            plan.steps(),
            [
                PublicationStep::CopyRecordsAndObjects,
                PublicationStep::WritePublished,
                PublicationStep::RemovePrivateFile,
            ]
        );
        assert_eq!(plan.published_records(), BTreeSet::from([jid(1), jid(2)]));
        assert!(!plan.private_copy_differs());
        // The public file is where it is, whatever else was asked.
        assert_eq!(
            plan_with(
                &corpus,
                &ledgers,
                "docs/elsewhere.md",
                PrivateRoot::Present,
                body()
            ),
            Err(PublicationError::DestinationDiffers {
                file: rel("docs/d21.md"),
                to: rel("docs/elsewhere.md")
            })
        );
    }

    #[test]
    fn a_public_file_that_leaks_is_refused_and_the_way_out_is_to_abort() {
        let ledgers = ledgers_of(records());
        let with_public = |bytes: &[u8]| {
            corpus_of(vec![
                file(
                    design_entity(),
                    None,
                    Visibility::Public,
                    "docs/d21.md",
                    bytes,
                ),
                // Until `Published` is recorded the private file is a byte-for-byte
                // copy of the public one.
                private_design(bytes),
                secret(),
            ])
        };
        let leaky = b"---\nid: x\n---\ncites finding/secret-plan\n";
        let got = plan_with(
            &with_public(leaky),
            &ledgers,
            "docs/d21.md",
            PrivateRoot::Present,
            leaky,
        );
        let Err(PublicationError::Leaks { leaks, remedy }) = got else {
            panic!("expected leaks, got {got:?}");
        };
        assert_eq!(remedy, LeakRemedy::AbortPublish);
        assert_eq!(leaks.first().field(), CrossingField::Body);
        // A clean one goes on.
        let clean = plan_with(
            &with_public(body()),
            &ledgers,
            "docs/d21.md",
            PrivateRoot::Present,
            body(),
        );
        assert_eq!(clean.unwrap().form(), Some(PublishForm::FileCopied));
    }

    #[test]
    fn a_publication_recorded_but_not_ended_removes_the_private_copy_only() {
        let ledgers = ledgers_of(records());
        let same = plan(&cut_short(true, b"bytes"), &ledgers).unwrap();
        assert_eq!(same.form(), Some(PublishForm::PrivateCopyLeft));
        assert_eq!(same.steps(), [PublicationStep::RemovePrivateFile]);
        assert!(same.records().is_empty());
        assert!(same.objects().is_empty());
        assert!(!same.private_copy_differs());
        assert!(!same.needs_human_confirmation());
        assert!(same.irreversible().is_none());

        // A copy that has an edit nobody published is lost with it: flagged.
        let edited = plan(&cut_short(true, b"edited since"), &ledgers).unwrap();
        assert!(edited.private_copy_differs());
        assert!(edited.needs_human_confirmation());
        assert_eq!(edited.private_file(), BytesHash::of(b"edited since"));
        // The private copy's bytes are in what is confirmed.
        assert_ne!(same.digest(), edited.digest());
    }

    #[test]
    fn a_publication_recorded_is_not_checked_for_leaks_again() {
        // Nothing is written to the public ledger any more; the private root need not
        // even be readable to take the private copy away.
        let ledgers = ledgers_of(records());
        let got = plan_with(
            &cut_short(true, b"bytes"),
            &ledgers,
            "docs/d21.md",
            PrivateRoot::Absent,
            body(),
        );
        assert_eq!(got.unwrap().steps(), [PublicationStep::RemovePrivateFile]);
    }

    // --- the digest ---------------------------------------------------------

    #[test]
    fn the_digest_is_stable_and_changes_with_the_plan() {
        let (corpus, ledgers) = fresh();
        let base = plan(&corpus, &ledgers).unwrap();
        assert_eq!(plan(&corpus, &ledgers).unwrap().digest(), base.digest());
        assert_eq!(base.digest().short().len(), 8);
        assert!(
            base.digest()
                .to_string()
                .starts_with(&format!("sha256:{}", base.digest().short()))
        );

        // Another destination.
        let moved = plan_with(
            &corpus,
            &ledgers,
            "docs/other.md",
            PrivateRoot::Present,
            body(),
        );
        assert_ne!(moved.unwrap().digest(), base.digest());

        // Another record to carry.
        let mut more = records();
        more.push(approve(
            12,
            &ada(),
            Visibility::Private,
            ca_passed(31, "i2"),
        ));
        let grown = plan(&corpus, &ledgers_of(more)).unwrap();
        assert_ne!(grown.digest(), base.digest());

        // A record taken away.
        let mut fewer = records();
        fewer.remove(1);
        let shrunk = plan(&corpus, &ledgers_of(fewer)).unwrap();
        assert_ne!(shrunk.digest(), base.digest());

        // The private file edited since it was looked at.
        let edited = corpus_of(vec![private_design(b"edited"), secret()]);
        let edited = plan_with(
            &edited,
            &ledgers,
            "docs/d21.md",
            PrivateRoot::Present,
            b"edited",
        );
        assert_ne!(edited.unwrap().digest(), base.digest());

        // A change that crosses nothing, such as what is already copied, does not
        // move what a person confirmed.
        let all = records();
        let copied = Ledgers::new(
            Ledger::from_judgements([all[0].clone()]),
            Ledger::from_judgements(all),
        );
        assert_eq!(plan(&corpus, &copied).unwrap().digest(), base.digest());
    }

    #[test]
    fn each_approved_version_adds_its_object() {
        let (corpus, _) = fresh();
        let mut all = records();
        all.push(approve(13, &ada(), Visibility::Private, ci_passed("d2")));
        let plan = plan(&corpus, &ledgers_of(all)).unwrap();
        assert_eq!(plan.objects().len(), 3);
        assert!(plan.objects().contains(&object_of(&hash_of("d2"))));
    }
}
