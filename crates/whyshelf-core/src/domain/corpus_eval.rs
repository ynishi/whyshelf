//! The state of every document and item of a corpus, from the files and the ledgers.
//!
//! [`evaluate_corpus`] reads what the files say ([`Corpus`]) and what people decided
//! ([`Ledgers`]) and gives the state of each document and each item, whenever it is
//! asked, without writing anything down. It is the entry beside the one the event log
//! feeds ([`evaluate`](super::evaluate)): the rules of the two axes and of
//! completeness are the same rules, written once there, and this module only
//! supplies what they ask for.
//!
//! ```text
//!   Corpus (files) --+                          +--> EntityState per document and item
//!                    |   evaluate::Evaluator    |    (Approval x Upstream, Completeness)
//!   Ledgers ---------+--> over ApprovalView  ---+--> Basis of each standing approval
//!   PinKeyring ------+    (LedgerView)          +--> Readiness (what the next gate lacks)
//!                                               +--> Presence of what has no file
//! ```
//!
//! # What it reuses and what it adds
//!
//! | question | answered by |
//! |---|---|
//! | Invalid, Draft, Modified, Approved; Ok, Stale, Blocked; Partial | [`Evaluator`], unchanged, asking the ledgers through [`ApprovalView`] |
//! | is `(gate, subject, hash)` approved; which hash was approved before | [`Ledgers::is_approved`] and the standing approvals |
//! | what an approval recorded of its upstreams, and whether it holds today | the same comparison as the event log's, with the keyring for keyed pins |
//! | a file the corpus cannot trust | its [`Invalidity`], an [`InvalidReason::Corpus`] |
//! | a judgement of one kind, a file of another | [`InvalidReason::KindChanged`] |
//! | what the file has no longer got | [`Presence`], by the rule of [`Ledgers`] |
//! | what the next gate lacks | [`Readiness`], by the gates' own answers ([`gate::body_lack`], [`gate::pins_lack`], [`gate::hypotheses_lack`]) |
//!
//! # Approved, and what stands behind it
//!
//! [`Approval::Approved`] says an unrevoked approval
//! exists for the current hash. What the approval stands on is beside it, in
//! [`EntityReport::bases`]: a gate's machine check (for CS, under which policy) or an
//! earlier decision taken in. A reader that needs to tell a relaxed or adopted
//! approval from a gate's reads it there.
//!
//! # Presence
//!
//! A document or item whose file is gone has no content, so it has no state; it has
//! a [`Presence`]. Something with a record that counts and no file is `Missing`,
//! unless an unrevoked retirement stands, when it is `Retired`. A record counts when
//! it is in the private ledger, or in the public one with a public `origin` or named
//! by the `Published` of its own document ([`Ledgers`] holds the rule); a record that
//! does not count is inert, and something only inert records are about has no
//! presence to tell. When the file is there, the thing is present; and if a
//! retirement still stands for it, [`CorpusEvaluation::returned`] lists it until the
//! retirement is revoked.
//!
//! A file that cannot be read is a file. When the adapter could still see its `id:`
//! ([`UnparsedFile::id`](super::corpus::UnparsedFile::id)), the document is `Present`
//! and its report is Invalid with the reasons the file gave
//! ([`InvalidReason::Unparsed`]), however many records are about it; what stands on it
//! is Blocked, as on any Invalid upstream. Only a document with no file at all, or one
//! whose file shows no `id:`, is `Missing`. A readable file with the same id remains
//! the document, and the unreadable copy adds nothing. The items of a Design that
//! cannot be read are not seen: their identifiers are in the part that did not read.
//!
//! # Pins of private upstreams
//!
//! The approvals compared here may hold a keyed pin ([`keyring`](super::keyring)) in
//! place of a hash. A pin whose key is in the keyring is recomputed from the upstream
//! today and is Same or Differs (Stale). A pin whose key is not in the keyring is
//! Unknown: the entity is not Stale for it, it is Partial, with the upstream as the
//! root of what cannot be read. The reason is
//! [`UnresolvedCause::KeyMissing`](super::state::UnresolvedCause::KeyMissing), naming
//! the key, and not that of an upstream in a store out of reach
//! ([`UnresolvedCause::Store`](super::state::UnresolvedCause::Store)): the upstream is
//! here and only the key is not, so the step is to approve again, not to open another
//! project (see [`next`](super::next)). Only the gate that pins an upstream stops on
//! it; see [`gate`] (`check_in`).
//!
//! A pin that is keyed while its upstream is public now is a [`LackField::PlainPin`]:
//! it cannot be compared in an environment without the key, and approving again pins
//! it plain.
//!
//! # What this does not read
//!
//! How a document wrote its references (`private:` marks, aliases) is naming, which
//! is not decided here: the caller hands over, per document, which upstream
//! identifiers it wrote with a mark into a store that cannot be read, in `written`.

use std::collections::{BTreeMap, BTreeSet};

use super::approval::{Gate, UpstreamRecord};
use super::corpus::{Corpus, CorpusDocument, Entity, Invalidity};
use super::design::{Design, ItemIssue, WorkItem};
use super::evaluate::{
    ApprovalView, Cause, Evaluated, Evaluator, ItemKey, States, Written, states_of,
};
use super::event::EntityKind;
use super::finding::Finding;
use super::gate::{self, UpstreamPins};
use super::hash::ContentHash;
use super::id::{DesignId, DocId, EntityId, FindingId, ItemId, ItemNumber, SpecId};
use super::judgement::{Basis, FilePresence, Target};
use super::keyring::{PinKeyring, UpstreamHash};
use super::ledger::{Ledgers, Retirement};
use super::nonempty::NonEmptyVec;
use super::policy::GatePolicy;
use super::spec::Spec;
use super::state::{
    Approval, Completeness, EntityState, InvalidReason, Lack, LackField, LackNeed, Presence,
    Readiness, Upstream,
};

/// What [`evaluate_corpus`] reads.
#[derive(Debug, Clone, Copy)]
pub struct CorpusInput<'a> {
    /// The documents as the files say they are.
    pub corpus: &'a Corpus,
    /// The judgements, public and private.
    pub ledgers: &'a Ledgers,
    /// The person's keys, to compare keyed pins under. Empty for a machine with none.
    pub keyring: &'a PinKeyring,
    /// What the project demands of a Spec at CS; it decides what a Spec lacks.
    pub policy: GatePolicy,
    /// How each document wrote the upstream identifiers it holds that no state can
    /// answer for. A document not in the map wrote none of the kind.
    pub written: &'a BTreeMap<DocId, Written>,
}

/// The ledgers as the evaluator's store of approvals.
struct LedgerView<'a> {
    ledgers: &'a Ledgers,
    keyring: &'a PinKeyring,
}

impl ApprovalView for LedgerView<'_> {
    fn is_approved_at(&self, gate: Gate, subject: EntityId, hash: &ContentHash) -> bool {
        self.ledgers.is_approved(gate, subject, hash)
    }

    /// The hash of the standing approval with the greatest record identifier. The
    /// choice is arbitrary and fixed: the time of a record orders nothing, and this
    /// value is for display.
    fn latest_approved(&self, gate: Gate, subject: EntityId) -> Option<ContentHash> {
        self.ledgers
            .approvals()
            .filter(|approval| approval.gate() == gate && approval.subject() == subject)
            .max_by_key(|approval| approval.id())
            .map(|approval| *approval.hash())
    }

    fn recorded_upstreams(
        &self,
        gate: Gate,
        subject: EntityId,
        hash: &ContentHash,
    ) -> Vec<Vec<UpstreamRecord>> {
        self.ledgers
            .approvals_at(gate, subject, hash)
            .map(|approval| approval.judgement().kind().upstream_pins())
            .collect()
    }

    /// Identity is the identifier here: a merge of two registrations, which the event
    /// log folds, does not exist when the identifier is in the file.
    fn canonical_of(&self, id: EntityId) -> EntityId {
        id
    }

    fn keyring(&self) -> &PinKeyring {
        self.keyring
    }
}

/// What is known of one document or item that has a file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EntityReport {
    state: EntityState,
    causes: Vec<Cause>,
    bases: Vec<Basis>,
    readiness: Readiness,
}

impl EntityReport {
    /// The two axes and completeness.
    pub fn state(&self) -> &EntityState {
        &self.state
    }

    /// Where a Stale or Blocked came from, root cause first.
    pub fn causes(&self) -> &[Cause] {
        &self.causes
    }

    /// What each approval that stands for the current hash stands on, in the order of
    /// the records. Empty unless the entity is Approved.
    pub fn bases(&self) -> &[Basis] {
        &self.bases
    }

    /// What the next gate needs that the entity has not.
    pub fn readiness(&self) -> &Readiness {
        &self.readiness
    }
}

/// The state of every document and item of a corpus. See the module documentation.
#[derive(Debug, Clone)]
pub struct CorpusEvaluation {
    findings: BTreeMap<FindingId, Evaluated<Finding>>,
    specs: BTreeMap<SpecId, Evaluated<Spec>>,
    designs: BTreeMap<DesignId, Evaluated<Design>>,
    documents: BTreeMap<DocId, EntityReport>,
    items: BTreeMap<ItemId, EntityReport>,
    vanished: BTreeMap<Target, Presence>,
    returned: BTreeSet<Target>,
    private: BTreeSet<EntityId>,
    unparsed: BTreeMap<DocId, EntityState>,
}

impl CorpusEvaluation {
    /// The report of a document that has a file, even one that cannot be read: its
    /// state is then Invalid, with the reasons the file gave.
    pub fn document(&self, id: &DocId) -> Option<&EntityReport> {
        self.documents.get(id)
    }

    /// The report of an item that a Design holds.
    pub fn item(&self, id: &ItemId) -> Option<&EntityReport> {
        self.items.get(id)
    }

    /// Every document that has a file, by identifier.
    pub fn documents(&self) -> &BTreeMap<DocId, EntityReport> {
        &self.documents
    }

    /// Every item that a Design holds, by identifier.
    pub fn items(&self) -> &BTreeMap<ItemId, EntityReport> {
        &self.items
    }

    /// The presence of a target: `Present` when its file is there, `Missing` or
    /// `Retired` when it is gone and a record that counts is about it, and `None`
    /// when no file and no record that counts is known for it.
    pub fn presence_of(&self, target: &Target) -> Option<Presence> {
        let present = match target {
            Target::Doc(id) => self.documents.contains_key(id),
            Target::Item(id) => self.items.contains_key(id),
        };
        if present {
            Some(Presence::Present)
        } else {
            self.vanished.get(target).copied()
        }
    }

    /// The targets whose file is gone, with `Missing` or `Retired`.
    pub fn vanished(&self) -> &BTreeMap<Target, Presence> {
        &self.vanished
    }

    /// The targets that are present again after a retirement, for as long as the
    /// retirement is not revoked. A notice, not a state.
    pub fn returned(&self) -> &BTreeSet<Target> {
        &self.returned
    }

    /// The Findings as evaluated, for the inputs of the gates.
    pub fn findings(&self) -> &BTreeMap<FindingId, Evaluated<Finding>> {
        &self.findings
    }

    /// The Specs as evaluated, for the inputs of the gates.
    pub fn specs(&self) -> &BTreeMap<SpecId, Evaluated<Spec>> {
        &self.specs
    }

    /// The Designs as evaluated, for the inputs of the gates.
    pub fn designs(&self) -> &BTreeMap<DesignId, Evaluated<Design>> {
        &self.designs
    }

    /// The state of every document by identifier, which is where a gate reads the
    /// state of an upstream.
    pub fn states(&self) -> States<'_> {
        states_with(&self.findings, &self.specs, &self.designs, &self.unparsed)
    }

    /// The upstreams that are private documents and `keyring`, for the `check_in` of
    /// the gates.
    pub fn pins<'a>(&'a self, keyring: &'a PinKeyring) -> UpstreamPins<'a> {
        UpstreamPins::new(&self.private, keyring)
    }
}

/// The state of each document whose file cannot be read but writes its `id:` where
/// the adapter could see it: Invalid, for the reasons the file gave.
///
/// It has no content, so it has no hash, no approval to be compared with and nothing
/// that stands on it but what reads its state as an upstream's (which is Blocked by
/// it). A document that has another file which can be read is not here: that file
/// is the document.
fn unparsed_states(corpus: &Corpus) -> BTreeMap<DocId, EntityState> {
    let mut reasons: BTreeMap<DocId, Vec<InvalidReason>> = BTreeMap::new();
    for file in corpus.unparsed() {
        let Some(id) = file.id() else { continue };
        if !corpus.by_id(&id).is_empty() {
            continue;
        }
        let seen = reasons.entry(id).or_default();
        for reason in file.reasons().iter() {
            let reason = InvalidReason::Unparsed(reason.clone());
            if !seen.contains(&reason) {
                seen.push(reason);
            }
        }
    }
    reasons
        .into_iter()
        .filter_map(|(id, reasons)| {
            let reasons = NonEmptyVec::from_vec(reasons).ok()?;
            let state = EntityState::new(
                Approval::Invalid(reasons),
                Upstream::Ok,
                Completeness::Complete,
            );
            Some((id, state))
        })
        .collect()
}

/// The [`States`] of what has been evaluated so far, and of the documents that cannot
/// be read.
fn states_with<'a>(
    findings: &'a BTreeMap<FindingId, Evaluated<Finding>>,
    specs: &'a BTreeMap<SpecId, Evaluated<Spec>>,
    designs: &'a BTreeMap<DesignId, Evaluated<Design>>,
    unparsed: &'a BTreeMap<DocId, EntityState>,
) -> States<'a> {
    let mut states = states_of(findings, specs, designs, std::iter::empty());
    states.extend(unparsed.iter().map(|(id, state)| (id.entity(), state)));
    states
}

/// The reasons the corpus found against a document, over all of its files.
fn corpus_reasons(files: &[&CorpusDocument]) -> Vec<InvalidReason> {
    let mut reasons: Vec<InvalidReason> = Vec::new();
    for file in files {
        for invalidity in file.standing().reasons() {
            let reason = InvalidReason::Corpus(Box::new(invalidity.clone()));
            if !reasons.contains(&reason) {
                reasons.push(reason);
            }
        }
    }
    reasons
}

/// Whether a reason the corpus gave a Design is about the item with this identity.
fn concerns_item(invalidity: &Invalidity, id: ItemId, number: ItemNumber) -> bool {
    match invalidity {
        Invalidity::Item(ItemIssue::DuplicateItemId { id: shared, .. }) => *shared == id,
        Invalidity::Item(ItemIssue::ItemIdIsDesignId { number: at }) => *at == number,
        Invalidity::Item(ItemIssue::DuplicateItemShortId { short, .. }) => {
            short.matches(id.entity())
        }
        Invalidity::DuplicateItemIdAcross { id: shared, .. } => *shared == id.entity(),
        Invalidity::DuplicateId { .. }
        | Invalidity::DuplicateAlias { .. }
        | Invalidity::PrivateExposed { .. }
        | Invalidity::Item(ItemIssue::UnidentifiedItem { .. }) => false,
    }
}

/// The kind of every identifier the files give: documents, and the items of Designs
/// (a document wins an identifier that both claim).
fn kinds_of(corpus: &Corpus) -> BTreeMap<EntityId, EntityKind> {
    let mut kinds = BTreeMap::new();
    for file in corpus.documents() {
        if let Entity::Design(design) = file.document().entity() {
            for item in design.items().values() {
                if let Some(id) = item.id() {
                    kinds.insert(id.entity(), EntityKind::Item);
                }
            }
        }
    }
    for file in corpus.documents() {
        kinds.insert(file.id().entity(), file.document().kind().entity_kind());
    }
    kinds
}

/// For each identifier whose file is of another kind than a standing judgement was
/// about: the kind it was judged as, and the kind it is.
fn kind_changes(
    kinds: &BTreeMap<EntityId, EntityKind>,
    ledgers: &Ledgers,
) -> BTreeMap<EntityId, BTreeSet<(EntityKind, EntityKind)>> {
    let mut changes: BTreeMap<EntityId, BTreeSet<(EntityKind, EntityKind)>> = BTreeMap::new();
    for approval in ledgers.approvals() {
        let was = approval.gate().target_kind();
        if let Some(now) = kinds.get(&approval.subject())
            && *now != was
        {
            changes
                .entry(approval.subject())
                .or_default()
                .insert((was, *now));
        }
    }
    changes
}

fn kind_reasons(
    changes: &BTreeMap<EntityId, BTreeSet<(EntityKind, EntityKind)>>,
    id: EntityId,
) -> Vec<InvalidReason> {
    changes
        .get(&id)
        .into_iter()
        .flatten()
        .map(|(was, now)| InvalidReason::KindChanged {
            was: *was,
            now: *now,
        })
        .collect()
}

/// The upstreams that every standing approval of the entity at its current hash
/// pinned with a key although they are public now.
///
/// One approval that pins everything plain (or by a key that is still needed) makes
/// the entity verifiable anywhere, so only when none does is there a lack.
fn now_public_keyed(
    view: &LedgerView<'_>,
    corpus: &Corpus,
    gate: Gate,
    subject: EntityId,
    hash: &ContentHash,
) -> BTreeSet<EntityId> {
    let public = |id: &EntityId| {
        let doc = DocId::new(*id);
        !corpus.by_id(&doc).is_empty() && !corpus.is_private(&doc)
    };
    let per_approval: Vec<BTreeSet<EntityId>> = view
        .recorded_upstreams(gate, subject, hash)
        .into_iter()
        .map(|records| {
            records
                .into_iter()
                .filter_map(|record| match record {
                    UpstreamRecord::Entity {
                        id,
                        hash: UpstreamHash::Keyed(_),
                    } if public(&id) => Some(id),
                    UpstreamRecord::Entity { .. } | UpstreamRecord::Event(_) => None,
                })
                .collect()
        })
        .collect();
    if per_approval.is_empty() || per_approval.iter().any(BTreeSet::is_empty) {
        return BTreeSet::new();
    }
    per_approval.into_iter().flatten().collect()
}

/// What the next gate lacks of a document.
///
/// What is lacking, and whether the gate refuses on it, is the gate's own answer
/// ([`gate::body_lack`] and its kin); this only lowers a refusal to a note once the
/// gate has let the entity through.
fn readiness_of(
    entity: &Entity,
    approved: bool,
    policy: GatePolicy,
    plain_pins: &BTreeSet<EntityId>,
) -> Readiness {
    // Required only while the gate that asks for it has not let the entity through;
    // afterwards it is something to know.
    let lack = |field: LackField, asked: Option<LackNeed>| {
        asked.map(|need| {
            let need = if approved {
                LackNeed::Informational
            } else {
                need
            };
            Lack::new(field, need)
        })
    };
    let mut lacks = Vec::new();
    match entity {
        Entity::Finding(finding) => {
            lacks.extend(lack(LackField::Body, gate::body_lack(finding)));
        }
        Entity::Spec(spec) => {
            lacks.extend(lack(LackField::Pins, gate::pins_lack(spec, policy)));
            lacks.extend(lack(
                LackField::Hypotheses,
                gate::hypotheses_lack(spec, policy),
            ));
        }
        Entity::Design(design) => {
            if design.items().is_empty() {
                lacks.push(Lack::new(LackField::Items, LackNeed::Informational));
            }
        }
    }
    lacks.extend(plain_pin_lacks(plain_pins));
    Readiness::new(lacks)
}

/// A keyed pin of an upstream that is public now needs approving again.
fn plain_pin_lacks(upstreams: &BTreeSet<EntityId>) -> impl Iterator<Item = Lack> + '_ {
    upstreams.iter().map(|upstream| {
        Lack::new(
            LackField::PlainPin {
                upstream: *upstream,
            },
            LackNeed::Required,
        )
    })
}

/// What an item lacks: only a keyed pin of a Design that is public now.
fn readiness_of_plain_pins(upstreams: &BTreeSet<EntityId>) -> Readiness {
    Readiness::new(plain_pin_lacks(upstreams).collect())
}

/// Every item identifier some Design file holds.
fn held_items(corpus: &Corpus) -> BTreeSet<ItemId> {
    corpus
        .documents()
        .iter()
        .filter_map(|file| match file.document().entity() {
            Entity::Design(design) => Some(design),
            Entity::Finding(_) | Entity::Spec(_) => None,
        })
        .flat_map(|design| design.items().values().filter_map(WorkItem::id))
        .collect()
}

/// The presence of what has judgements that count and no file.
///
/// A document whose file cannot be read has one: it is here, and Invalid, not gone.
fn vanished_targets(
    corpus: &Corpus,
    ledgers: &Ledgers,
    unparsed: &BTreeMap<DocId, EntityState>,
) -> BTreeMap<Target, Presence> {
    let docs: BTreeSet<DocId> = corpus
        .documents()
        .iter()
        .map(CorpusDocument::id)
        .chain(unparsed.keys().copied())
        .collect();
    let items = held_items(corpus);
    ledgers
        .targets()
        .into_iter()
        .filter(|target| match target {
            Target::Doc(id) => !docs.contains(id),
            Target::Item(id) => !items.contains(id),
        })
        .map(|target| {
            let presence = match ledgers.retirement(&target, FilePresence::Absent) {
                Retirement::Retired => Presence::Retired,
                Retirement::NotRetired | Retirement::Returned => Presence::Missing,
            };
            (target, presence)
        })
        .collect()
}

/// Evaluate every document and item of the corpus against the ledgers.
///
/// Findings first, then Specs, Designs and items, each against the stage above, as
/// [`Evaluator`] does for the event log.
pub fn evaluate_corpus(input: CorpusInput<'_>) -> CorpusEvaluation {
    let CorpusInput {
        corpus,
        ledgers,
        keyring,
        policy,
        written,
    } = input;
    let view = LedgerView { ledgers, keyring };
    let no_written = Written::default();
    let written_of = |id: DocId| written.get(&id).unwrap_or(&no_written);

    let mut files: BTreeMap<DocId, Vec<&CorpusDocument>> = BTreeMap::new();
    for file in corpus.documents() {
        files.entry(file.id()).or_default().push(file);
    }
    let unparsed = unparsed_states(corpus);
    let kinds = kinds_of(corpus);
    let changes = kind_changes(&kinds, ledgers);
    let external = |id: DocId, group: &[&CorpusDocument]| {
        let mut reasons = corpus_reasons(group);
        reasons.extend(kind_reasons(&changes, id.entity()));
        reasons
    };

    let mut findings: BTreeMap<FindingId, Evaluated<Finding>> = BTreeMap::new();
    for (id, group) in &files {
        if let Entity::Finding(finding) = group[0].document().entity() {
            findings.insert(
                finding.id(),
                Evaluator::finding(finding, &view, &external(*id, group)),
            );
        }
    }

    let mut specs: BTreeMap<SpecId, Evaluated<Spec>> = BTreeMap::new();
    {
        let no_specs = BTreeMap::new();
        let no_designs = BTreeMap::new();
        let states = states_with(&findings, &no_specs, &no_designs, &unparsed);
        for (id, group) in &files {
            if let Entity::Spec(spec) = group[0].document().entity() {
                specs.insert(
                    spec.id(),
                    Evaluator::spec(
                        spec,
                        &findings,
                        &states,
                        &view,
                        written_of(*id),
                        &external(*id, group),
                    ),
                );
            }
        }
    }

    let mut designs: BTreeMap<DesignId, Evaluated<Design>> = BTreeMap::new();
    {
        let no_designs = BTreeMap::new();
        let states = states_with(&findings, &specs, &no_designs, &unparsed);
        for (id, group) in &files {
            if let Entity::Design(design) = group[0].document().entity() {
                designs.insert(
                    design.id(),
                    Evaluator::design(
                        design,
                        &specs,
                        &states,
                        &view,
                        written_of(*id),
                        &external(*id, group),
                    ),
                );
            }
        }
    }

    let states = states_with(&findings, &specs, &designs, &unparsed);
    let mut items: BTreeMap<ItemId, EntityReport> = BTreeMap::new();
    for group in files.values() {
        let Entity::Design(design) = group[0].document().entity() else {
            continue;
        };
        for (number, work) in design.items() {
            let Some(id) = work.id() else { continue };
            if items.contains_key(&id) {
                continue;
            }
            let key = ItemKey {
                id,
                design: design.id(),
                number: *number,
            };
            let mut reasons: Vec<InvalidReason> = corpus_reasons(group)
                .into_iter()
                .filter(|reason| {
                    matches!(reason, InvalidReason::Corpus(invalidity)
                        if concerns_item(invalidity.as_ref(), id, *number))
                })
                .collect();
            reasons.extend(kind_reasons(&changes, id.entity()));
            let (state, causes) =
                Evaluator::item_state(&key, work, &designs, &states, &view, &reasons);
            let hash = work.hash(design.id());
            let plain_pins = if matches!(state.approval(), Approval::Approved) {
                now_public_keyed(&view, corpus, Gate::Ca, id.entity(), &hash)
            } else {
                BTreeSet::new()
            };
            let readiness = readiness_of_plain_pins(&plain_pins);
            items.insert(
                id,
                report(
                    &view,
                    Gate::Ca,
                    id.entity(),
                    &hash,
                    state,
                    causes,
                    readiness,
                ),
            );
        }
    }

    let mut documents: BTreeMap<DocId, EntityReport> = BTreeMap::new();
    for (id, group) in &files {
        let entity = group[0].document().entity();
        let (hash, evaluated_state, causes) = match entity {
            Entity::Finding(f) => {
                let ev = &findings[&f.id()];
                (*f.hash(), ev.state().clone(), ev.causes().to_vec())
            }
            Entity::Spec(s) => {
                let ev = &specs[&s.id()];
                (*s.contract_hash(), ev.state().clone(), ev.causes().to_vec())
            }
            Entity::Design(d) => {
                let ev = &designs[&d.id()];
                (*d.design_hash(), ev.state().clone(), ev.causes().to_vec())
            }
        };
        let gate = Gate::of_doc(entity.kind());
        let approved = matches!(evaluated_state.approval(), Approval::Approved);
        let plain_pins = if approved {
            now_public_keyed(&view, corpus, gate, id.entity(), &hash)
        } else {
            BTreeSet::new()
        };
        let readiness = readiness_of(entity, approved, policy, &plain_pins);
        documents.insert(
            *id,
            report(
                &view,
                gate,
                id.entity(),
                &hash,
                evaluated_state,
                causes,
                readiness,
            ),
        );
    }

    // A document whose file cannot be read is here, and Invalid; it is no content to
    // approve, so it lacks nothing and rests on nothing.
    for (id, state) in &unparsed {
        documents.insert(
            *id,
            EntityReport {
                state: state.clone(),
                causes: Vec::new(),
                bases: Vec::new(),
                readiness: Readiness::default(),
            },
        );
    }

    let returned = documents
        .keys()
        .map(|id| Target::Doc(*id))
        .chain(items.keys().map(|id| Target::Item(*id)))
        .filter(|target| ledgers.retirement(target, FilePresence::Present) == Retirement::Returned)
        .collect();
    // A publication not recorded yet leaves its document private: a gate pins it by
    // key, although the file read is the public one.
    let private = corpus
        .documents()
        .iter()
        .map(CorpusDocument::id)
        .filter(|id| corpus.is_private(id))
        .map(|id| id.entity())
        .collect();
    let vanished = vanished_targets(corpus, ledgers, &unparsed);

    CorpusEvaluation {
        findings,
        specs,
        designs,
        documents,
        items,
        vanished,
        returned,
        private,
        unparsed,
    }
}

fn report(
    view: &LedgerView<'_>,
    gate: Gate,
    subject: EntityId,
    hash: &ContentHash,
    state: EntityState,
    causes: Vec<Cause>,
    readiness: Readiness,
) -> EntityReport {
    let bases = if matches!(state.approval(), Approval::Approved) {
        view.ledgers
            .approvals_at(gate, subject, hash)
            .map(|approval| approval.basis().clone())
            .collect()
    } else {
        Vec::new()
    };
    EntityReport {
        state,
        causes,
        bases,
        readiness,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::approval::test_support::approve as logged;
    use crate::domain::approval::{ApprovalSet, Attestation, Pinned, TestOutcome};
    use crate::domain::classification::fold;
    use crate::domain::corpus::{FileFacts, GitPlace, Location, Reading, Visibility};
    use crate::domain::design::test_support::{design, identified, num};
    use crate::domain::evaluate::ImplView;
    use crate::domain::event::Event;
    use crate::domain::finding::test_support::finding;
    use crate::domain::gate::{CaInput, Checked, CiInput, CsInput, Passed, ca, ci, cs};
    use crate::domain::hash::BytesHash;
    use crate::domain::hash::PinKey;
    use crate::domain::id::test_support::{did, docid, eid, fid, iid, sid};
    use crate::domain::judge::test_support::judge;
    use crate::domain::judgement::test_support::{Filed, approve, corpus_of, jid, ts, why};
    use crate::domain::judgement::{Judgement, JudgementKind};
    use crate::domain::knowable::Knowable;
    use crate::domain::ledger::{Ledger, RevokeRequest, RevokeWhat};
    use crate::domain::lint::{CheckViolation, UpstreamProblem, upstream_problems};
    use crate::domain::next::{CmDue, NextInput, NextStep, next_step};
    use crate::domain::nonempty::NonEmptyVec;
    use crate::domain::revision::{Revision, RevisionId};
    use crate::domain::spec::{DeclaredPins, test_support::spec};
    use crate::domain::state::{
        BlockReason, Completeness, StaleReason, UnparsedReason, UnresolvedCause, UnresolvedReason,
        Upstream,
    };

    // --- the chain: Finding 1, Spec 11 on it, Design 21 on that, item 31 --------

    #[derive(Clone)]
    struct Chain {
        f: Finding,
        s: Spec,
        d: Design,
    }

    fn chain() -> Chain {
        Chain {
            f: finding(1, "Login takes 8 s."),
            s: spec(11, &[1], &["PIN-A"]),
            d: design(21, 11, vec![(1, identified(31, "cache", &["PIN-A"]))]),
        }
    }

    impl Chain {
        fn files(&self) -> Vec<Filed> {
            vec![
                Filed::public(Entity::Finding(self.f.clone())),
                Filed::public(Entity::Spec(self.s.clone())),
                Filed::public(Entity::Design(self.d.clone())),
            ]
        }

        fn cr(&self) -> Attestation {
            Attestation::Cr {
                finding: Pinned::new(self.f.id(), *self.f.hash()),
            }
        }

        fn cs_with(&self, ground: UpstreamHash, policy: GatePolicy) -> Attestation {
            Attestation::Cs {
                spec: Pinned::new(self.s.id(), *self.s.contract_hash()),
                hyp: *self.s.hyp_hash(),
                grounds: NonEmptyVec::new(Pinned::new(self.f.id(), ground), Vec::new()),
                policy,
            }
        }

        fn cs(&self) -> Attestation {
            self.cs_with((*self.f.hash()).into(), GatePolicy::default())
        }

        fn ci_with(&self, spec: UpstreamHash) -> Attestation {
            Attestation::Ci {
                design: Pinned::new(self.d.id(), *self.d.design_hash()),
                spec: Pinned::new(self.s.id(), spec),
            }
        }

        fn ci(&self) -> Attestation {
            self.ci_with((*self.s.contract_hash()).into())
        }

        fn ca(&self) -> Attestation {
            Attestation::Ca {
                item: Pinned::new(iid(31), self.d.item_hash(num(1)).unwrap()),
                design: Pinned::new(self.d.id(), (*self.d.design_hash()).into()),
                revision: Revision::Commit(RevisionId::from_validated("a1".into()).unwrap()),
                tests: TestOutcome::Passed,
            }
        }

        fn all(&self) -> Vec<Attestation> {
            vec![self.cr(), self.cs(), self.ci(), self.ca()]
        }
    }

    fn record(n: u128, origin: Visibility, attestation: Attestation) -> Judgement {
        approve(n, &judge("ada"), origin, Passed::for_test(attestation))
    }

    fn ledgers(public: Vec<Judgement>, private: Vec<Judgement>) -> Ledgers {
        Ledgers::new(
            Ledger::from_judgements(public),
            Ledger::from_judgements(private),
        )
    }

    fn evaluate(files: Vec<Filed>, ledgers: &Ledgers) -> CorpusEvaluation {
        evaluate_with(
            files,
            ledgers,
            &PinKeyring::empty(),
            GatePolicy::default(),
            &BTreeMap::new(),
        )
    }

    fn evaluate_with(
        files: Vec<Filed>,
        ledgers: &Ledgers,
        keyring: &PinKeyring,
        policy: GatePolicy,
        written: &BTreeMap<DocId, Written>,
    ) -> CorpusEvaluation {
        let corpus = corpus_of(files);
        evaluate_corpus(CorpusInput {
            corpus: &corpus,
            ledgers,
            keyring,
            policy,
            written,
        })
    }

    // --- the same two axes as the event log --------------------------------------

    /// The chain evaluated by the event log, as the application does it today.
    struct Logged {
        findings: BTreeMap<FindingId, Evaluated<Finding>>,
        specs: BTreeMap<SpecId, Evaluated<Spec>>,
        designs: BTreeMap<DesignId, Evaluated<Design>>,
        item: (EntityState, Vec<Cause>),
    }

    fn by_event_log(now: &Chain, approvals: &[Attestation]) -> Logged {
        let events: Vec<Event> = approvals.iter().map(|a| logged(a.clone(), &[])).collect();
        let l = ApprovalSet::from_events(&events, &fold(&events));
        let none = Written::default();
        let no_specs = BTreeMap::new();
        let no_designs = BTreeMap::new();
        let findings: BTreeMap<_, _> = [(now.f.id(), Evaluator::finding(&now.f, &l, &[]))].into();
        let specs: BTreeMap<_, _> = {
            let states = states_of(&findings, &no_specs, &no_designs, []);
            [(
                now.s.id(),
                Evaluator::spec(&now.s, &findings, &states, &l, &none, &[]),
            )]
            .into()
        };
        let designs: BTreeMap<_, _> = {
            let states = states_of(&findings, &specs, &no_designs, []);
            [(
                now.d.id(),
                Evaluator::design(&now.d, &specs, &states, &l, &none, &[]),
            )]
            .into()
        };
        let item = {
            let states = states_of(&findings, &specs, &designs, []);
            let key = ItemKey {
                id: iid(31),
                design: did(21),
                number: num(1),
            };
            Evaluator::item_state(&key, &now.d.items()[&num(1)], &designs, &states, &l, &[])
        };
        Logged {
            findings,
            specs,
            designs,
            item,
        }
    }

    /// The same chain, the same approvals, read from the files and the ledgers: the
    /// states and the causes are those of the event log.
    fn assert_same_as_event_log(now: &Chain, approvals: &[Attestation]) -> CorpusEvaluation {
        let old = by_event_log(now, approvals);
        let records = approvals
            .iter()
            .enumerate()
            .map(|(i, a)| record(i as u128 + 1, Visibility::Public, a.clone()))
            .collect();
        let new = evaluate(now.files(), &ledgers(records, Vec::new()));
        let doc = |n: u128| new.document(&docid(n)).unwrap();
        let finding = &old.findings[&fid(1)];
        assert_eq!(doc(1).state(), finding.state());
        assert_eq!(doc(1).causes(), finding.causes());
        let spec = &old.specs[&sid(11)];
        assert_eq!(doc(11).state(), spec.state());
        assert_eq!(doc(11).causes(), spec.causes());
        let design = &old.designs[&did(21)];
        assert_eq!(doc(21).state(), design.state());
        assert_eq!(doc(21).causes(), design.causes());
        let item = new.item(&iid(31)).unwrap();
        assert_eq!(item.state(), &old.item.0);
        assert_eq!(item.causes(), old.item.1.as_slice());
        new
    }

    #[test]
    fn an_approved_chain_reads_as_the_event_log_reads_it() {
        let c = chain();
        let new = assert_same_as_event_log(&c, &c.all());
        for id in [1, 11, 21] {
            let report = new.document(&docid(id)).unwrap();
            assert_eq!(report.state().approval(), &Approval::Approved);
            assert_eq!(report.state().upstream(), &Upstream::Ok);
            assert_eq!(report.state().completeness(), &Completeness::Complete);
        }
        assert_eq!(
            new.item(&iid(31)).unwrap().state().approval(),
            &Approval::Approved
        );
    }

    #[test]
    fn a_corrected_finding_makes_the_chain_modified_then_stale_as_the_event_log_does() {
        let approved = chain();
        let mut now = approved.clone();
        now.f = finding(1, "Login takes 7 s.");
        let new = assert_same_as_event_log(&now, &approved.all());
        let doc = |n: u128| new.document(&docid(n)).unwrap().state();
        assert!(matches!(doc(1).approval(), Approval::Modified { .. }));
        assert_eq!(doc(11).approval(), &Approval::Approved);
        assert!(matches!(doc(11).upstream(), Upstream::Stale(_)));
        assert!(matches!(doc(21).upstream(), Upstream::Stale(_)));
        assert!(matches!(
            new.item(&iid(31)).unwrap().state().upstream(),
            Upstream::Stale(_)
        ));
        // F -> S -> D -> I: the root of the chain is the Finding.
        let causes = new.item(&iid(31)).unwrap().causes();
        assert_eq!(causes.first().map(Cause::via), Some(eid(1)));
    }

    #[test]
    fn approvals_above_an_unapproved_finding_are_blocked_as_in_the_event_log() {
        let c = chain();
        let new = assert_same_as_event_log(&c, &[c.cs(), c.ci(), c.ca()]);
        assert_eq!(
            new.document(&docid(1)).unwrap().state().approval(),
            &Approval::Draft
        );
        assert!(matches!(
            new.document(&docid(11)).unwrap().state().upstream(),
            Upstream::Blocked(_)
        ));
    }

    #[test]
    fn nothing_approved_is_all_draft_and_blocked_as_in_the_event_log() {
        let new = assert_same_as_event_log(&chain(), &[]);
        assert_eq!(
            new.document(&docid(11)).unwrap().state().approval(),
            &Approval::Draft
        );
        assert!(
            new.document(&docid(11))
                .unwrap()
                .state()
                .upstream()
                .ne(&Upstream::Ok)
        );
    }

    #[test]
    fn the_basis_of_an_approval_is_the_gate_it_passed() {
        let c = chain();
        let records = vec![record(1, Visibility::Public, c.cr())];
        let new = evaluate(c.files(), &ledgers(records, Vec::new()));
        assert_eq!(
            new.document(&docid(1)).unwrap().bases(),
            [Basis::Gate(crate::domain::approval::GateBasis::Fixed {
                gate: Gate::Cr
            })]
        );
        assert!(new.document(&docid(11)).unwrap().bases().is_empty());
    }

    // --- what the corpus does not trust ------------------------------------------

    #[test]
    fn a_fault_the_corpus_found_makes_the_document_invalid_with_its_reason() {
        let c = chain();
        let mut files = c.files();
        files.push(Filed::public_at("copy.md", Entity::Finding(c.f.clone())));
        let new = evaluate(files, &Ledgers::default());
        let Approval::Invalid(reasons) = new.document(&docid(1)).unwrap().state().approval() else {
            panic!("a document with two files is invalid");
        };
        assert!(reasons.iter().any(|r| matches!(
            r,
            InvalidReason::Corpus(i) if matches!(**i, Invalidity::DuplicateId { .. })
        )));
        // And what stands on it is blocked by it.
        assert!(matches!(
            new.document(&docid(11)).unwrap().state().upstream(),
            Upstream::Blocked(_)
        ));
    }

    #[test]
    fn an_item_whose_identity_is_shared_is_invalid() {
        let c = chain();
        let twice = design(22, 11, vec![(1, identified(31, "again", &["PIN-A"]))]);
        let mut files = c.files();
        files.push(Filed::public(Entity::Design(twice)));
        let new = evaluate(files, &Ledgers::default());
        let Approval::Invalid(reasons) = new.item(&iid(31)).unwrap().state().approval() else {
            panic!("an item claimed by two Designs is invalid");
        };
        assert!(reasons.iter().any(|r| matches!(
            r,
            InvalidReason::Corpus(i) if matches!(**i, Invalidity::DuplicateItemIdAcross { .. })
        )));
    }

    // --- KindChanged ---------------------------------------------------------------

    #[test]
    fn a_judgement_of_a_spec_under_the_id_of_a_finding_makes_it_invalid() {
        let c = chain();
        // Spec 11 was approved; the file with id 11 is a Finding now.
        let files = vec![Filed::public(Entity::Finding(finding(11, "now a finding")))];
        let l = ledgers(vec![record(1, Visibility::Public, c.cs())], Vec::new());
        let new = evaluate(files, &l);
        let Approval::Invalid(reasons) = new.document(&docid(11)).unwrap().state().approval()
        else {
            panic!("a changed kind is invalid");
        };
        assert_eq!(
            reasons.iter().collect::<Vec<_>>(),
            [&InvalidReason::KindChanged {
                was: EntityKind::Spec,
                now: EntityKind::Finding
            }]
        );
    }

    #[test]
    fn a_judgement_of_an_item_under_the_id_of_a_document_makes_it_invalid() {
        let c = chain();
        // Item 31 was approved; 31 is the id of a Finding now.
        let files = vec![Filed::public(Entity::Finding(finding(31, "now a finding")))];
        let l = ledgers(vec![record(1, Visibility::Public, c.ca())], Vec::new());
        let new = evaluate(files, &l);
        let Approval::Invalid(reasons) = new.document(&docid(31)).unwrap().state().approval()
        else {
            panic!("a changed kind is invalid");
        };
        assert_eq!(
            reasons.first(),
            &InvalidReason::KindChanged {
                was: EntityKind::Item,
                now: EntityKind::Finding
            }
        );
    }

    #[test]
    fn a_revoked_judgement_does_not_hold_a_kind_against_the_file() {
        let c = chain();
        let files = vec![Filed::public(Entity::Finding(finding(11, "now a finding")))];
        let approved = record(1, Visibility::Public, c.cs());
        let l = ledgers(vec![approved.clone()], Vec::new());
        let revoke = l
            .revoke(RevokeRequest {
                id: jid(2),
                at: ts(),
                by: judge("ada"),
                origin: Visibility::Public,
                force: false,
                reason: why(),
                what: RevokeWhat::Attestation {
                    gate: Gate::Cs,
                    subject: eid(11),
                    hash: *c.s.contract_hash(),
                    of: None,
                },
            })
            .unwrap();
        let new = evaluate(files, &ledgers(vec![approved, revoke], Vec::new()));
        assert_eq!(
            new.document(&docid(11)).unwrap().state().approval(),
            &Approval::Draft
        );
    }

    // --- presence ------------------------------------------------------------------

    fn retire(n: u128, target: Target, origin: Visibility) -> Judgement {
        Judgement::retire(
            jid(n),
            ts(),
            judge("ada"),
            origin,
            target,
            FilePresence::Absent,
            why(),
        )
        .unwrap()
    }

    #[test]
    fn a_document_and_an_item_with_a_public_record_and_no_file_are_missing() {
        let c = chain();
        let l = ledgers(
            vec![
                record(1, Visibility::Public, c.cr()),
                record(2, Visibility::Public, c.ca()),
            ],
            Vec::new(),
        );
        let new = evaluate(Vec::new(), &l);
        assert_eq!(
            new.presence_of(&Target::Doc(docid(1))),
            Some(Presence::Missing)
        );
        assert_eq!(
            new.presence_of(&Target::Item(iid(31))),
            Some(Presence::Missing)
        );
        assert!(new.document(&docid(1)).is_none());
        // Nothing is said of what nobody judged.
        assert_eq!(new.presence_of(&Target::Doc(docid(2))), None);
    }

    #[test]
    fn a_retired_document_is_retired_and_a_document_with_a_file_is_present() {
        let c = chain();
        let target = Target::Doc(docid(1));
        let l = ledgers(
            vec![
                record(1, Visibility::Public, c.cr()),
                retire(2, target, Visibility::Public),
            ],
            Vec::new(),
        );
        let gone = evaluate(Vec::new(), &l);
        assert_eq!(gone.presence_of(&target), Some(Presence::Retired));
        assert!(gone.returned().is_empty());

        let back = evaluate(c.files(), &l);
        assert_eq!(back.presence_of(&target), Some(Presence::Present));
    }

    #[test]
    fn a_retired_document_that_comes_back_is_noticed_until_the_retirement_is_revoked() {
        let c = chain();
        let target = Target::Doc(docid(1));
        let base = vec![
            record(1, Visibility::Public, c.cr()),
            retire(2, target, Visibility::Public),
        ];
        let l = ledgers(base.clone(), Vec::new());
        let back = evaluate(c.files(), &l);
        assert_eq!(back.returned(), &BTreeSet::from([target]));
        // The notice is not a state: the document is evaluated as any other.
        assert_eq!(
            back.document(&docid(1)).unwrap().state().approval(),
            &Approval::Approved
        );

        let revoke = l
            .revoke(RevokeRequest {
                id: jid(3),
                at: ts(),
                by: judge("ada"),
                origin: Visibility::Public,
                force: false,
                reason: why(),
                what: RevokeWhat::Retire { target, of: None },
            })
            .unwrap();
        let mut revoked = base;
        revoked.push(revoke);
        let quiet = evaluate(c.files(), &ledgers(revoked, Vec::new()));
        assert!(quiet.returned().is_empty());
    }

    #[test]
    fn a_private_record_copied_to_the_public_ledger_gives_the_public_side_no_presence() {
        let c = chain();
        let copied = record(1, Visibility::Private, c.cr());
        // A colleague reads the public ledger: the copy is inert, nothing is known.
        let colleague = evaluate(Vec::new(), &ledgers(vec![copied.clone()], Vec::new()));
        assert_eq!(colleague.presence_of(&Target::Doc(docid(1))), None);
        assert!(colleague.vanished().is_empty());
        // The owner reads their private ledger too: the document is missing for them.
        let owner = evaluate(
            Vec::new(),
            &ledgers(vec![copied.clone()], vec![copied.clone()]),
        );
        assert_eq!(
            owner.presence_of(&Target::Doc(docid(1))),
            Some(Presence::Missing)
        );
    }

    #[test]
    fn a_publication_makes_the_copied_record_count_and_the_document_missing() {
        let c = chain();
        let copied = record(1, Visibility::Private, c.cr());
        let before = ledgers(vec![copied.clone()], Vec::new());
        let publish = Judgement::published(
            jid(2),
            ts(),
            judge("ada"),
            docid(1),
            BTreeSet::from([jid(1)]),
            &before,
        )
        .unwrap();
        assert!(matches!(publish.kind(), JudgementKind::Published { .. }));
        let after = ledgers(vec![copied, publish], Vec::new());
        let new = evaluate(Vec::new(), &after);
        assert_eq!(
            new.presence_of(&Target::Doc(docid(1))),
            Some(Presence::Missing)
        );
    }

    // --- a file that cannot be read is not a file that is gone ----------------------

    /// A file under the public root that cannot be read, which still shows `id`.
    fn unreadable(path: &str, id: Option<DocId>) -> (FileFacts, Reading) {
        let location = Location::new(
            Visibility::Public,
            path.parse().unwrap(),
            Knowable::Known(GitPlace::Tracked),
        );
        (
            FileFacts::new(location, BytesHash::of(b"<<<<<<< ours")),
            Reading::Unparsed {
                id,
                reasons: NonEmptyVec::new(UnparsedReason::MergeConflictMarker, Vec::new()),
            },
        )
    }

    fn evaluate_readings(
        mut readings: Vec<(FileFacts, Reading)>,
        files: Vec<Filed>,
        ledgers: &Ledgers,
    ) -> CorpusEvaluation {
        readings.extend(files.into_iter().map(Filed::reading));
        let corpus = Corpus::build(readings, &BTreeSet::new());
        evaluate_corpus(CorpusInput {
            corpus: &corpus,
            ledgers,
            keyring: &PinKeyring::empty(),
            policy: GatePolicy::default(),
            written: &BTreeMap::new(),
        })
    }

    #[test]
    fn an_approved_finding_with_a_conflict_marker_is_present_and_invalid_not_missing() {
        let c = chain();
        let l = ledgers(vec![record(1, Visibility::Public, c.cr())], Vec::new());
        let others = || {
            vec![
                Filed::public(Entity::Spec(c.s.clone())),
                Filed::public(Entity::Design(c.d.clone())),
            ]
        };
        let target = Target::Doc(docid(1));

        // The file of Finding 1 holds a conflict marker, and its `id:` line still reads.
        let seen = evaluate_readings(vec![unreadable("finding.md", Some(docid(1)))], others(), &l);
        assert_eq!(seen.presence_of(&target), Some(Presence::Present));
        assert!(seen.vanished().is_empty());
        let report = seen.document(&docid(1)).expect("the file is here");
        assert_eq!(
            report.state().approval(),
            &Approval::Invalid(NonEmptyVec::new(
                InvalidReason::Unparsed(UnparsedReason::MergeConflictMarker),
                Vec::new()
            ))
        );
        assert!(report.bases().is_empty());
        assert!(report.readiness().is_ready());
        // What stands on it is blocked by it, and does not take it for a dangling
        // reference.
        let spec = seen.document(&docid(11)).unwrap().state();
        assert!(!matches!(spec.approval(), Approval::Invalid(_)), "{spec:?}");
        let Upstream::Blocked(blocks) = spec.upstream() else {
            panic!("a Spec on an unreadable Finding is blocked: {spec:?}");
        };
        assert!(
            blocks
                .iter()
                .any(|b| b == &BlockReason::UpstreamInvalid { upstream: eid(1) })
        );
        // A gate that reads the states stops on it too.
        assert!(matches!(
            seen.states().get(&eid(1)).map(|s| s.approval()),
            Some(Approval::Invalid(_))
        ));

        // When the id cannot be seen, nothing says whose file it is: the document is
        // missing, as before.
        let blind = evaluate_readings(vec![unreadable("finding.md", None)], others(), &l);
        assert_eq!(blind.presence_of(&target), Some(Presence::Missing));
        assert!(blind.document(&docid(1)).is_none());
    }

    #[test]
    fn a_file_that_cannot_be_read_does_not_replace_a_document_that_can() {
        let c = chain();
        let new = evaluate_readings(
            vec![unreadable("copy.md", Some(docid(1)))],
            c.files(),
            &Ledgers::default(),
        );
        // The Finding that reads is the document; the copy that does not adds nothing.
        assert_eq!(
            new.document(&docid(1)).unwrap().state().approval(),
            &Approval::Draft
        );
    }

    // --- readiness -----------------------------------------------------------------

    fn spec_without_pins() -> Chain {
        let mut c = chain();
        c.s = spec(11, &[1], &[]);
        c.d = design(21, 11, vec![]);
        c
    }

    fn lacks_of(new: &CorpusEvaluation, id: u128) -> Vec<(LackField, LackNeed)> {
        new.document(&docid(id))
            .unwrap()
            .readiness()
            .lacks()
            .iter()
            .map(|lack| (lack.field(), lack.need()))
            .collect()
    }

    #[test]
    fn a_spec_without_pins_lacks_them_under_the_default_policy() {
        let c = spec_without_pins();
        let new = evaluate(c.files(), &Ledgers::default());
        assert_eq!(lacks_of(&new, 11), [(LackField::Pins, LackNeed::Required)]);
        assert!(!new.document(&docid(11)).unwrap().readiness().is_ready());
        // Completeness is another fact: the Spec still stands on what it should.
        assert_eq!(
            new.document(&docid(11)).unwrap().state().completeness(),
            &Completeness::Complete
        );
    }

    #[test]
    fn a_spec_without_pins_under_a_relaxed_policy_lacks_them_as_a_note() {
        let c = spec_without_pins();
        let relaxed = GatePolicy::new(false, true);
        let new = evaluate_with(
            c.files(),
            &Ledgers::default(),
            &PinKeyring::empty(),
            relaxed,
            &BTreeMap::new(),
        );
        assert_eq!(
            lacks_of(&new, 11),
            [(LackField::Pins, LackNeed::Informational)]
        );
        assert!(new.document(&docid(11)).unwrap().readiness().is_ready());
    }

    #[test]
    fn a_spec_that_passed_cs_under_a_relaxed_policy_without_pins_says_so() {
        let c = spec_without_pins();
        let relaxed = GatePolicy::new(false, true);
        let l = ledgers(
            vec![
                record(1, Visibility::Public, c.cr()),
                record(
                    2,
                    Visibility::Public,
                    c.cs_with((*c.f.hash()).into(), relaxed),
                ),
            ],
            Vec::new(),
        );
        // Even if the settings are back to the default, the approval stands, shows
        // the policy it passed under, and the Spec still lacks its pins as a note.
        let new = evaluate(c.files(), &l);
        let report = new.document(&docid(11)).unwrap();
        assert_eq!(report.state().approval(), &Approval::Approved);
        assert_eq!(
            report.bases(),
            [Basis::Gate(crate::domain::approval::GateBasis::Cs {
                policy: relaxed
            })]
        );
        assert_eq!(
            lacks_of(&new, 11),
            [(LackField::Pins, LackNeed::Informational)]
        );
    }

    #[test]
    fn a_design_without_items_lacks_them_as_a_note() {
        let c = spec_without_pins();
        let new = evaluate(c.files(), &Ledgers::default());
        assert_eq!(
            lacks_of(&new, 21),
            [(LackField::Items, LackNeed::Informational)]
        );
    }

    // --- keyed pins ----------------------------------------------------------------

    fn ring(kid: &str, byte: u8) -> PinKeyring {
        PinKeyring::new(kid.parse().unwrap(), PinKey::new(&[byte; 32]).unwrap())
    }

    /// Finding 1 is private, and approved in the private ledger; the Spec is public
    /// and its CS approval, in the public ledger, pins the ground by `ring`.
    fn private_ground(now: &Chain, approved: &Chain, ring: &PinKeyring) -> (Vec<Filed>, Ledgers) {
        let pin = ring.pin(eid(1), approved.f.hash()).unwrap();
        let cs = approved.cs_with(UpstreamHash::Keyed(pin), GatePolicy::default());
        let l = ledgers(
            vec![record(2, Visibility::Public, cs)],
            vec![record(1, Visibility::Private, approved.cr())],
        );
        let files = vec![
            Filed::private(Entity::Finding(now.f.clone())),
            Filed::public(Entity::Spec(now.s.clone())),
            Filed::public(Entity::Design(now.d.clone())),
        ];
        (files, l)
    }

    fn eval_keyed(files: Vec<Filed>, l: &Ledgers, keys: &PinKeyring) -> CorpusEvaluation {
        evaluate_with(files, l, keys, GatePolicy::default(), &BTreeMap::new())
    }

    #[test]
    fn a_keyed_pin_is_current_while_the_private_upstream_is_unchanged() {
        let c = chain();
        let keys = ring("k1", 1);
        let (files, l) = private_ground(&c, &c, &keys);
        let spec = eval_keyed(files, &l, &keys);
        let state = spec.document(&docid(11)).unwrap().state();
        assert_eq!(state.approval(), &Approval::Approved);
        assert_eq!(state.upstream(), &Upstream::Ok);
        assert_eq!(state.completeness(), &Completeness::Complete);
    }

    #[test]
    fn a_private_upstream_edited_makes_the_spec_stale_when_the_key_is_there() {
        let approved = chain();
        let mut now = approved.clone();
        now.f = finding(1, "Login takes 7 s.");
        let keys = ring("k1", 1);
        let (files, l) = private_ground(&now, &approved, &keys);
        let new = eval_keyed(files, &l, &keys);
        let state = new.document(&docid(11)).unwrap().state();
        let Upstream::Stale(reasons) = state.upstream() else {
            panic!("the ground changed: {state:?}");
        };
        assert!(matches!(
            reasons.first(),
            StaleReason::UpstreamChanged { upstream, recorded: UpstreamHash::Keyed(_), .. }
                if *upstream == eid(1)
        ));
    }

    #[test]
    fn a_keyed_pin_with_a_kid_the_keyring_lacks_is_unknown_not_stale() {
        let approved = chain();
        let mut now = approved.clone();
        now.f = finding(1, "Login takes 7 s.");
        let theirs = ring("k1", 1);
        // Whoever reads this has another key, or none: they cannot tell whether the
        // ground changed, although it did. (The edited Finding is approved again, so
        // the only question left is the pin.)
        for keys in [ring("k2", 2), PinKeyring::empty()] {
            let (files, l) = private_ground(&now, &approved, &theirs);
            let again = record(4, Visibility::Private, now.cr());
            let l = ledgers(
                l.public().judgements().cloned().collect(),
                l.private()
                    .judgements()
                    .cloned()
                    .chain(std::iter::once(again))
                    .collect(),
            );
            let new = eval_keyed(files, &l, &keys);
            let state = new.document(&docid(11)).unwrap().state();
            assert_eq!(state.approval(), &Approval::Approved);
            assert_eq!(state.upstream(), &Upstream::Ok, "not Stale");
            assert_eq!(
                state.completeness().reasons().collect::<Vec<_>>(),
                [&UnresolvedReason::key_missing(
                    eid(1),
                    "k1".parse().unwrap()
                )],
                "Partial, with the upstream as the root, and the key that is lacking"
            );
        }
    }

    /// The step of Spec 11 as the application would ask for it: its own state, the
    /// problems of its upstreams (`finding` is the Finding's state, `None` when the
    /// file is not here).
    fn next_of_spec(new: &CorpusEvaluation, finding: Option<&EntityState>) -> Option<NextStep> {
        let report = new.document(&docid(11)).unwrap();
        let problems = upstream_problems(report.state().completeness(), &[(eid(1), finding)]);
        next_step(
            report.state(),
            report.causes(),
            &problems,
            NextInput::Spec {
                cm_due: CmDue::NotDue,
            },
        )
    }

    #[test]
    fn a_missing_key_asks_to_approve_again_and_a_missing_store_to_open_it() {
        let approved = chain();
        let mut now = approved.clone();
        now.f = finding(1, "Login takes 7 s.");
        let (files, l) = private_ground(&now, &approved, &ring("k1", 1));
        let again = record(4, Visibility::Private, now.cr());
        let l = ledgers(
            l.public().judgements().cloned().collect(),
            l.private()
                .judgements()
                .cloned()
                .chain(std::iter::once(again))
                .collect(),
        );
        // The Finding is here and read; the key its pin was made under is not.
        let keyless = eval_keyed(files, &l, &ring("k2", 2));
        let finding_state = keyless.document(&docid(1)).unwrap().state();
        assert_eq!(
            next_of_spec(&keyless, Some(finding_state)),
            Some(NextStep::Reapprove(Gate::Cs))
        );

        // The Finding is in a store this clone does not have.
        let written = BTreeMap::from([(
            docid(11),
            Written {
                unresolved: BTreeSet::from([eid(1)]),
                ..Written::default()
            },
        )]);
        let storeless = evaluate_with(
            vec![
                Filed::public(Entity::Spec(approved.s.clone())),
                Filed::public(Entity::Design(approved.d.clone())),
            ],
            &Ledgers::default(),
            &PinKeyring::empty(),
            GatePolicy::default(),
            &written,
        );
        let cause = |new: &CorpusEvaluation| {
            new.document(&docid(11))
                .unwrap()
                .state()
                .completeness()
                .reasons()
                .map(|reason| reason.cause().clone())
                .collect::<Vec<_>>()
        };
        assert_eq!(cause(&storeless), [UnresolvedCause::Store]);
        assert_eq!(
            cause(&keyless),
            [UnresolvedCause::KeyMissing {
                kid: "k1".parse().unwrap()
            }]
        );
        assert_eq!(
            next_of_spec(&storeless, None),
            Some(NextStep::OpenWithPrivateStore)
        );
    }

    fn rebuild(now: &Chain) -> Vec<Filed> {
        vec![
            Filed::private(Entity::Finding(now.f.clone())),
            Filed::public(Entity::Spec(now.s.clone())),
            Filed::public(Entity::Design(now.d.clone())),
        ]
    }

    #[test]
    fn a_pin_made_before_a_rotation_still_verifies_and_new_pins_use_the_new_key() {
        let approved = chain();
        let mut now = approved.clone();
        now.f = finding(1, "Login takes 7 s.");
        let old = ring("k1", 1);
        let (_, l) = private_ground(&now, &approved, &old);
        let rotated = old
            .clone()
            .rotated("k2".parse().unwrap(), PinKey::new(&[2; 32]).unwrap())
            .unwrap();

        // Unchanged: still Same under the rotated keyring.
        let (files, l_same) = private_ground(&approved, &approved, &old);
        let same = eval_keyed(files, &l_same, &rotated);
        assert_eq!(
            same.document(&docid(11)).unwrap().state().upstream(),
            &Upstream::Ok
        );
        // Changed: still Stale (the old key is kept to say so).
        let changed = eval_keyed(rebuild(&now), &l, &rotated);
        assert!(matches!(
            changed.document(&docid(11)).unwrap().state().upstream(),
            Upstream::Stale(_)
        ));
        // A new pin is made under the new key.
        assert_eq!(
            rotated.pin(eid(1), now.f.hash()).unwrap().kid().as_str(),
            "k2"
        );
    }

    #[test]
    fn a_keyed_pin_of_an_upstream_that_is_public_now_lacks_a_plain_pin() {
        let c = chain();
        let keys = ring("k1", 1);
        let (_, l) = private_ground(&c, &c, &keys);
        // The Finding moved to the public root; the approval still holds a keyed pin.
        let new = eval_keyed(c.files(), &l, &keys);
        let report = new.document(&docid(11)).unwrap();
        assert_eq!(report.state().approval(), &Approval::Approved);
        assert_eq!(
            lacks_of(&new, 11),
            [(LackField::PlainPin { upstream: eid(1) }, LackNeed::Required)]
        );
        assert!(!report.readiness().is_ready());
        // While the upstream is private, there is nothing to say.
        let (files, l) = private_ground(&c, &c, &keys);
        let still = eval_keyed(files, &l, &keys);
        assert!(lacks_of(&still, 11).is_empty());
    }

    #[test]
    fn an_approval_with_a_plain_pin_beside_the_keyed_one_lacks_nothing() {
        let c = chain();
        let keys = ring("k1", 1);
        let (_, l) = private_ground(&c, &c, &keys);
        let plain = record(3, Visibility::Public, c.cs());
        let both = ledgers(
            l.public()
                .judgements()
                .cloned()
                .chain(std::iter::once(plain))
                .collect(),
            l.private().judgements().cloned().collect(),
        );
        let new = eval_keyed(c.files(), &both, &keys);
        assert!(lacks_of(&new, 11).is_empty());
    }

    // --- UpstreamUnverifiable and Partial --------------------------------------------

    fn run_cs(new: &CorpusEvaluation, c: &Chain, keys: &PinKeyring) -> Checked {
        let states = new.states();
        cs::check_in(
            CsInput {
                spec: &new.specs()[&c.s.id()],
                grounds: new.findings().iter().map(|(id, e)| (*id, e)).collect(),
                states: &states,
                policy: GatePolicy::default(),
            },
            new.pins(keys),
        )
    }

    fn violations(checked: Checked) -> Vec<CheckViolation> {
        match checked {
            Ok(passed) => panic!("expected violations, passed {:?}", passed.gate()),
            Err(violations) => violations.into_vec(),
        }
    }

    fn passed(checked: Checked) -> Passed {
        match checked {
            Ok(passed) => passed,
            Err(violations) => panic!("expected a pass, found {:?}", violations.into_vec()),
        }
    }

    /// Finding 1 private and approved, nothing else approved.
    fn private_finding_only(c: &Chain) -> (Vec<Filed>, Ledgers) {
        (
            vec![
                Filed::private(Entity::Finding(c.f.clone())),
                Filed::public(Entity::Spec(c.s.clone())),
                Filed::public(Entity::Design(c.d.clone())),
            ],
            ledgers(Vec::new(), vec![record(1, Visibility::Private, c.cr())]),
        )
    }

    #[test]
    fn cs_pins_a_private_ground_by_key_and_stops_without_one() {
        let c = chain();
        let (files, l) = private_finding_only(&c);
        let new = evaluate(files, &l);

        let none = PinKeyring::empty();
        assert_eq!(
            violations(run_cs(&new, &c, &none)),
            [CheckViolation::UpstreamUnverifiable { upstream: eid(1) }]
        );

        let keys = ring("k1", 1);
        let proof = passed(run_cs(&new, &c, &keys));
        let Attestation::Cs { grounds, .. } = proof.attestation() else {
            panic!("a CS attestation");
        };
        let pin = grounds.first().pin();
        assert_eq!(
            pin.plain(),
            None,
            "the hash of a private text is not written"
        );
        assert_eq!(pin.keyed(), keys.pin(eid(1), c.f.hash()).as_ref());
    }

    #[test]
    fn cs_pins_a_public_ground_plain_even_with_a_key() {
        let c = chain();
        let l = ledgers(vec![record(1, Visibility::Public, c.cr())], Vec::new());
        let new = evaluate(c.files(), &l);
        let proof = passed(run_cs(&new, &c, &ring("k1", 1)));
        let Attestation::Cs { grounds, .. } = proof.attestation() else {
            panic!("a CS attestation");
        };
        assert_eq!(grounds.first().pin().plain(), Some(c.f.hash()));
    }

    #[test]
    fn cs_stops_on_a_ground_that_cannot_be_read_as_unverifiable() {
        let c = chain();
        // Finding 1 is not in this clone; the Spec wrote it with a mark into the
        // private store.
        let written = BTreeMap::from([(
            docid(11),
            Written {
                unresolved: BTreeSet::from([eid(1)]),
                ..Written::default()
            },
        )]);
        let files = vec![
            Filed::public(Entity::Spec(c.s.clone())),
            Filed::public(Entity::Design(c.d.clone())),
        ];
        let new = evaluate_with(
            files,
            &Ledgers::default(),
            &PinKeyring::empty(),
            GatePolicy::default(),
            &written,
        );
        assert_eq!(
            violations(run_cs(&new, &c, &PinKeyring::empty())),
            [CheckViolation::UpstreamUnverifiable { upstream: eid(1) }]
        );
        // The same Spec is Partial: nothing is Invalid for it.
        let state = new.document(&docid(11)).unwrap().state();
        assert!(state.completeness().is_partial());
        assert_eq!(state.approval(), &Approval::Draft);
    }

    /// A clone with no private store: Spec 11 is approved (its ground, Finding 1, is
    /// private and not here), Design 21 and item 31 are the ones to approve.
    fn clone_without_the_ground(c: &Chain, keys: &PinKeyring) -> CorpusEvaluation {
        let pin = ring("theirs", 7).pin(eid(1), c.f.hash()).unwrap();
        let cs = c.cs_with(UpstreamHash::Keyed(pin), GatePolicy::default());
        let l = ledgers(vec![record(1, Visibility::Public, cs)], Vec::new());
        let written = BTreeMap::from([(
            docid(11),
            Written {
                unresolved: BTreeSet::from([eid(1)]),
                ..Written::default()
            },
        )]);
        evaluate_with(
            vec![
                Filed::public(Entity::Spec(c.s.clone())),
                Filed::public(Entity::Design(c.d.clone())),
            ],
            &l,
            keys,
            GatePolicy::default(),
            &written,
        )
    }

    fn run_ci(new: &CorpusEvaluation, c: &Chain, keys: &PinKeyring, strict: bool) -> Checked {
        let states = new.states();
        let input = CiInput {
            design: &new.designs()[&c.d.id()],
            specs: new.specs().iter().map(|(id, e)| (*id, e)).collect(),
            states: &states,
        };
        if strict {
            ci::check(input)
        } else {
            ci::check_in(input, new.pins(keys))
        }
    }

    #[test]
    fn what_is_unreadable_further_up_leaves_the_entity_partial_and_the_gate_passes() {
        let c = chain();
        let keys = PinKeyring::empty();
        let new = clone_without_the_ground(&c, &keys);

        // The Spec is approved and Partial; the Design under it is Partial too.
        let spec = new.document(&docid(11)).unwrap().state();
        assert_eq!(spec.approval(), &Approval::Approved);
        assert_eq!(spec.upstream(), &Upstream::Ok);
        assert!(spec.completeness().is_partial());
        let design = new.document(&docid(21)).unwrap().state();
        assert!(design.completeness().is_partial());
        assert_eq!(
            design.completeness().reasons().collect::<Vec<_>>(),
            [&UnresolvedReason::new(eid(1), eid(11))],
            "the root is the ground, reached through the Spec"
        );

        // CI looks at the pin it checks itself, the Spec's, which is fine.
        let proof = passed(run_ci(&new, &c, &keys, false));
        let Attestation::Ci { spec, .. } = proof.attestation() else {
            panic!("a CI attestation");
        };
        assert_eq!(spec.pin().plain(), Some(c.s.contract_hash()));
        // The check of the event log stops on anything that cannot be read above.
        assert_eq!(
            violations(run_ci(&new, &c, &keys, true)),
            [CheckViolation::Upstream {
                upstream: eid(11),
                problem: UpstreamProblem::Unresolved
            }]
        );
    }

    /// Spec 11 is private and approved in the private ledger; the Design is public.
    fn private_spec(c: &Chain) -> CorpusEvaluation {
        let l = ledgers(
            Vec::new(),
            vec![
                record(1, Visibility::Private, c.cr()),
                record(2, Visibility::Private, c.cs()),
            ],
        );
        evaluate(
            vec![
                Filed::private(Entity::Spec(c.s.clone())),
                Filed::public(Entity::Finding(c.f.clone())),
                Filed::public(Entity::Design(c.d.clone())),
            ],
            &l,
        )
    }

    #[test]
    fn ci_pins_a_private_spec_by_key_and_stops_without_one() {
        let c = chain();
        let new = private_spec(&c);
        assert_eq!(
            violations(run_ci(&new, &c, &PinKeyring::empty(), false)),
            [CheckViolation::UpstreamUnverifiable { upstream: eid(11) }]
        );
        let keys = ring("k1", 1);
        let proof = passed(run_ci(&new, &c, &keys, false));
        let Attestation::Ci { spec, .. } = proof.attestation() else {
            panic!("a CI attestation");
        };
        assert_eq!(
            spec.pin().keyed(),
            keys.pin(eid(11), c.s.contract_hash()).as_ref()
        );
    }

    #[test]
    fn ca_pins_a_private_design_by_key_and_stops_without_one() {
        let c = chain();
        let l = ledgers(
            vec![
                record(1, Visibility::Public, c.cr()),
                record(2, Visibility::Public, c.cs()),
            ],
            vec![record(3, Visibility::Private, c.ci())],
        );
        let new = evaluate(
            vec![
                Filed::public(Entity::Finding(c.f.clone())),
                Filed::public(Entity::Spec(c.s.clone())),
                Filed::private(Entity::Design(c.d.clone())),
            ],
            &l,
        );
        let run = |keys: &PinKeyring| {
            let states = new.states();
            let key = ItemKey {
                id: iid(31),
                design: did(21),
                number: num(1),
            };
            let work = &c.d.items()[&num(1)];
            let view = ImplView::new(
                RevisionId::from_validated("head".into()).unwrap(),
                None,
                BTreeMap::from([(
                    "PIN-A".parse().unwrap(),
                    vec!["tests/a.rs:3".parse().unwrap()],
                )]),
                Default::default(),
            );
            let evaluation = Evaluator::item(
                &key,
                work,
                new.designs(),
                &states,
                &ApprovalSet::default(),
                &view,
                &[],
            );
            ca::check_in(
                CaInput {
                    key: &key,
                    item: work,
                    evaluation: &evaluation,
                    designs: new.designs().iter().map(|(id, e)| (*id, e)).collect(),
                    declared: &DeclaredPins::of(new.specs().values().filter_map(Evaluated::entity)),
                    states: &states,
                    impl_view: &view,
                    revision: RevisionId::from_validated("head".into()).unwrap(),
                    tests: crate::domain::approval::TestsRun::Finished(TestOutcome::Passed),
                },
                new.pins(keys),
            )
        };
        assert_eq!(
            violations(run(&PinKeyring::empty())),
            [CheckViolation::UpstreamUnverifiable { upstream: eid(21) }]
        );
        let keys = ring("k1", 1);
        let proof = passed(run(&keys));
        let Attestation::Ca { design, .. } = proof.attestation() else {
            panic!("a CA attestation");
        };
        assert_eq!(
            design.pin().keyed(),
            keys.pin(eid(21), c.d.design_hash()).as_ref()
        );
    }
}
