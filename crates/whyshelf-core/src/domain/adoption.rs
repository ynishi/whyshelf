//! Taking an earlier decision in as an approval: the check, and the proof it passed.
//!
//! A project may have decided things before whyshelf was used: in a frozen ledger,
//! an issue, a pull request. [`check`] lets such a decision stand as an approval
//! without running a gate, and returns an [`Adoptable`], which is the only thing a
//! [`Judgement::adopt`](super::judgement::Judgement::adopt) can be made from, as
//! [`Passed`](super::gate::Passed) is the only thing an approval can be made from.
//!
//! ```compile_fail,E0451
//! use whyshelf_core::domain::adoption::{Adoptable, AdoptedAttestation};
//! use whyshelf_core::domain::location::SourceUri;
//!
//! // `Adoptable` has private fields: only `adoption::check` can make one.
//! fn forged(attestation: AdoptedAttestation, source: SourceUri) -> Adoptable {
//!     Adoptable { attestation, source }
//! }
//! ```
//!
//! # What the check looks at
//!
//! | rule | refusal |
//! |---|---|
//! | the gate is CR, CS, CI or CA; CM records a measurement, and a measurement is not adopted | [`GateNotAdoptable`](AdoptionRefusal::GateNotAdoptable) |
//! | the source is not empty | [`SourceEmpty`](AdoptionRefusal::SourceEmpty) |
//! | the gate approves the kind of the target (a document for CR, CS, CI; an item for CA) | [`GateTargetMismatch`](AdoptionRefusal::GateTargetMismatch) |
//! | the target is in the corpus, in one file | [`TargetUnknown`](AdoptionRefusal::TargetUnknown) |
//! | the target is valid as it stands | [`TargetInvalid`](AdoptionRefusal::TargetInvalid) |
//! | what the approval would pin upstream is in the corpus | [`UpstreamUnavailable`](AdoptionRefusal::UpstreamUnavailable) |
//!
//! The hash the adoption pins is the target's own, taken from the corpus, so it
//! cannot disagree with the file.
//!
//! # What an adoption records
//!
//! An [`AdoptedAttestation`] is the target at its hash and the upstream facts the
//! gate would have pinned, and nothing else: it has no revision and no test outcome.
//! A test that did not run cannot be recorded as having passed.
//!
//! | gate | target `(id, hash)` | upstream pinned |
//! |---|---|---|
//! | CR | Finding, its hash | none |
//! | CS | Spec, `contract_hash` | every ground, `(id, hash)` |
//! | CI | Design, `design_hash` | the Spec, `(id, contract_hash)` |
//! | CA | item, its item hash | the Design, `(id, design_hash)` |
//!
//! An adoption is stronger than a gate, since no test and no clean tree stood
//! behind it. What it stands on is shown as
//! [`Basis::Adopted`](super::judgement::Basis::Adopted), with its source, and kept
//! apart from a gate's. Who may adopt, and the confirmation it takes, are for the
//! layer that knows the principal and the terminal; this module does not decide them.

use super::approval::{Gate, Pinned};
use super::canon::{Canon, ToCanon};
use super::corpus::{Corpus, CorpusDocument, Entity};
use super::error::AdoptionRefusal;
use super::event::EntityKind;
use super::hash::ContentHash;
use super::id::{DocId, EntityId, ItemId};
use super::judgement::Target;
use super::location::SourceUri;

/// What an adopted approval says: the target at its hash, and the upstream pinned.
///
/// Unlike an [`Attestation`](super::approval::Attestation) it has no revision and no
/// test outcome.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AdoptedAttestation {
    gate: Gate,
    subject: EntityId,
    hash: ContentHash,
    upstream: Vec<Pinned<EntityId>>,
}

impl AdoptedAttestation {
    /// The gate whose approval is taken in.
    pub fn gate(&self) -> Gate {
        self.gate
    }

    /// The document or item approved.
    pub fn subject(&self) -> EntityId {
        self.subject
    }

    /// The hash pinned for it.
    pub fn hash(&self) -> &ContentHash {
        &self.hash
    }

    /// The upstream facts pinned, in the order the gate pins them.
    pub fn upstream(&self) -> &[Pinned<EntityId>] {
        &self.upstream
    }
}

impl ToCanon for AdoptedAttestation {
    fn to_canon(&self) -> Canon {
        Canon::map([
            ("gate", Canon::str(self.gate.name())),
            ("target", Pinned::new(self.subject, self.hash).to_canon()),
            (
                "upstream",
                Canon::list(self.upstream.iter().map(ToCanon::to_canon)),
            ),
        ])
    }
}

/// Proof that an adoption was checked: the only way to record one.
///
/// See the module documentation for why it cannot be made anywhere else.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Adoptable {
    attestation: AdoptedAttestation,
    source: SourceUri,
}

impl Adoptable {
    /// What the adopted approval says.
    pub fn attestation(&self) -> &AdoptedAttestation {
        &self.attestation
    }

    /// Where the decision came from.
    pub fn source(&self) -> &SourceUri {
        &self.source
    }
}

impl ToCanon for Adoptable {
    fn to_canon(&self) -> Canon {
        Canon::map([
            ("attestation", self.attestation.to_canon()),
            ("source", self.source.to_canon()),
        ])
    }
}

/// What [`check`] reads.
#[derive(Debug, Clone, Copy)]
pub struct AdoptionInput<'a> {
    /// The documents as the files say they are.
    pub corpus: &'a Corpus,
    /// The gate whose approval is taken in.
    pub gate: Gate,
    /// The document or item approved.
    pub target: Target,
    /// Where the decision came from: an address, or a citation in free text.
    pub source: &'a str,
}

/// The one document that carries `id`, if exactly one file does.
fn sole_file(corpus: &Corpus, id: DocId) -> Result<&CorpusDocument, AdoptionRefusal> {
    match corpus.by_id(&id) {
        [] => Err(AdoptionRefusal::TargetUnknown {
            target: Target::Doc(id),
        }),
        [file] => Ok(file),
        _ => Err(AdoptionRefusal::TargetInvalid {
            target: Target::Doc(id),
        }),
    }
}

/// The entity of the one file that carries `id`, for an upstream to pin.
fn upstream_entity(corpus: &Corpus, id: EntityId) -> Result<&Entity, AdoptionRefusal> {
    match corpus.by_id(&DocId::new(id)) {
        [file] => Ok(file.document().entity()),
        _ => Err(AdoptionRefusal::UpstreamUnavailable { upstream: id }),
    }
}

fn valid(file: &CorpusDocument, target: Target) -> Result<(), AdoptionRefusal> {
    if file.standing().is_valid() {
        Ok(())
    } else {
        Err(AdoptionRefusal::TargetInvalid { target })
    }
}

/// Check an adoption and give the proof, or the first rule it breaks.
pub fn check(input: AdoptionInput<'_>) -> Result<Adoptable, AdoptionRefusal> {
    let AdoptionInput {
        corpus,
        gate,
        target,
        source,
    } = input;
    if gate == Gate::Cm {
        return Err(AdoptionRefusal::GateNotAdoptable { gate });
    }
    let source = source
        .parse::<SourceUri>()
        .map_err(AdoptionRefusal::SourceEmpty)?;
    let mismatch = || AdoptionRefusal::GateTargetMismatch { gate, target };
    let (hash, upstream) = match target {
        Target::Doc(doc) => {
            if gate.target_kind() == EntityKind::Item {
                return Err(mismatch());
            }
            let file = sole_file(corpus, doc)?;
            valid(file, target)?;
            let entity = file.document().entity();
            if Gate::of_doc(entity.kind()) != gate {
                return Err(mismatch());
            }
            match entity {
                Entity::Finding(finding) => (*finding.hash(), Vec::new()),
                Entity::Spec(spec) => {
                    let grounds = spec
                        .grounds()
                        .iter()
                        .map(|ground| match upstream_entity(corpus, ground.entity())? {
                            Entity::Finding(found) => {
                                Ok(Pinned::new(ground.entity(), *found.hash()))
                            }
                            _ => Err(AdoptionRefusal::UpstreamUnavailable {
                                upstream: ground.entity(),
                            }),
                        })
                        .collect::<Result<Vec<_>, _>>()?;
                    (*spec.contract_hash(), grounds)
                }
                Entity::Design(design) => {
                    let spec = design.spec().entity();
                    let Entity::Spec(found) = upstream_entity(corpus, spec)? else {
                        return Err(AdoptionRefusal::UpstreamUnavailable { upstream: spec });
                    };
                    (
                        *design.design_hash(),
                        vec![Pinned::new(spec, *found.contract_hash())],
                    )
                }
            }
        }
        Target::Item(item) => {
            if gate.target_kind() != EntityKind::Item {
                return Err(mismatch());
            }
            let holders = designs_holding(corpus, item);
            let [(file, design, number)] = holders.as_slice() else {
                return Err(if holders.is_empty() {
                    AdoptionRefusal::TargetUnknown { target }
                } else {
                    AdoptionRefusal::TargetInvalid { target }
                });
            };
            valid(file, target)?;
            let hash = design
                .item_hash(*number)
                .ok_or(AdoptionRefusal::TargetUnknown { target })?;
            let design_id = design.id().entity();
            (hash, vec![Pinned::new(design_id, *design.design_hash())])
        }
    };
    Ok(Adoptable {
        attestation: AdoptedAttestation {
            gate,
            subject: target.entity(),
            hash,
            upstream,
        },
        source,
    })
}

/// The Design files that hold an item with this identity, with the item's number.
pub(super) fn designs_holding(
    corpus: &Corpus,
    item: ItemId,
) -> Vec<(
    &CorpusDocument,
    &super::design::Design,
    super::id::ItemNumber,
)> {
    corpus
        .documents()
        .iter()
        .filter_map(|file| match file.document().entity() {
            Entity::Design(design) => Some((file, design)),
            _ => None,
        })
        .flat_map(|(file, design)| {
            design
                .items()
                .iter()
                .filter(move |(_, work)| work.id() == Some(item))
                .map(move |(number, _)| (file, design, *number))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::design::Design;
    use crate::domain::design::test_support::{design, identified, num};
    use crate::domain::finding::test_support::finding;
    use crate::domain::id::test_support::{docid, eid, iid};
    use crate::domain::judgement::test_support::{Filed, corpus_of};
    use crate::domain::spec::test_support::spec;

    /// Finding 1, Spec 11 (ground 1), Design 21 on Spec 11 with item 31 (number 1).
    fn chain() -> Vec<Filed> {
        let f = finding(1, "Login takes 8 s.");
        let s = spec(11, &[1], &["PIN-A"]);
        let d: Design = design(21, 11, vec![(1, identified(31, "cache", &["PIN-A"]))]);
        vec![
            Filed::public(Entity::Finding(f)),
            Filed::public(Entity::Spec(s)),
            Filed::public(Entity::Design(d)),
        ]
    }

    fn run(
        files: Vec<Filed>,
        gate: Gate,
        target: Target,
        source: &str,
    ) -> Result<Adoptable, AdoptionRefusal> {
        let corpus = corpus_of(files);
        check(AdoptionInput {
            corpus: &corpus,
            gate,
            target,
            source,
        })
    }

    #[test]
    fn a_finding_is_adopted_at_its_own_hash_with_nothing_upstream() {
        let f = finding(1, "Login takes 8 s.");
        let got = run(
            chain(),
            Gate::Cr,
            Target::Doc(docid(1)),
            "https://tracker.example/issues/7",
        )
        .unwrap();
        assert_eq!(got.attestation().gate(), Gate::Cr);
        assert_eq!(got.attestation().subject(), eid(1));
        assert_eq!(got.attestation().hash(), f.hash());
        assert!(got.attestation().upstream().is_empty());
        assert_eq!(got.source().to_string(), "https://tracker.example/issues/7");
    }

    #[test]
    fn each_gate_pins_what_the_gate_would_pin_upstream() {
        let f = finding(1, "Login takes 8 s.");
        let s = spec(11, &[1], &["PIN-A"]);
        let d = design(21, 11, vec![(1, identified(31, "cache", &["PIN-A"]))]);

        let cs = run(chain(), Gate::Cs, Target::Doc(docid(11)), "frozen ledger").unwrap();
        assert_eq!(cs.attestation().hash(), s.contract_hash());
        assert_eq!(
            cs.attestation().upstream(),
            [Pinned::new(eid(1), *f.hash())]
        );

        let ci = run(chain(), Gate::Ci, Target::Doc(docid(21)), "frozen ledger").unwrap();
        assert_eq!(ci.attestation().hash(), d.design_hash());
        assert_eq!(
            ci.attestation().upstream(),
            [Pinned::new(eid(11), *s.contract_hash())]
        );

        let ca = run(chain(), Gate::Ca, Target::Item(iid(31)), "pr 12").unwrap();
        assert_eq!(ca.attestation().subject(), eid(31));
        assert_eq!(ca.attestation().hash(), &d.item_hash(num(1)).unwrap());
        assert_eq!(
            ca.attestation().upstream(),
            [Pinned::new(eid(21), *d.design_hash())]
        );
    }

    #[test]
    fn a_measurement_is_not_adopted() {
        assert_eq!(
            run(chain(), Gate::Cm, Target::Doc(docid(11)), "x"),
            Err(AdoptionRefusal::GateNotAdoptable { gate: Gate::Cm })
        );
    }

    #[test]
    fn an_empty_source_is_refused() {
        for blank in ["", "   "] {
            assert!(matches!(
                run(chain(), Gate::Cr, Target::Doc(docid(1)), blank),
                Err(AdoptionRefusal::SourceEmpty(_))
            ));
        }
    }

    #[test]
    fn a_gate_that_does_not_approve_the_kind_of_the_target_is_refused() {
        let mismatch = |gate, target| AdoptionRefusal::GateTargetMismatch { gate, target };
        let spec_doc = Target::Doc(docid(11));
        assert_eq!(
            run(chain(), Gate::Cr, spec_doc, "x"),
            Err(mismatch(Gate::Cr, spec_doc))
        );
        assert_eq!(
            run(chain(), Gate::Ca, spec_doc, "x"),
            Err(mismatch(Gate::Ca, spec_doc))
        );
        let item = Target::Item(iid(31));
        assert_eq!(
            run(chain(), Gate::Ci, item, "x"),
            Err(mismatch(Gate::Ci, item))
        );
    }

    #[test]
    fn a_target_that_is_not_in_the_corpus_is_unknown() {
        let doc = Target::Doc(docid(99));
        assert_eq!(
            run(chain(), Gate::Cr, doc, "x"),
            Err(AdoptionRefusal::TargetUnknown { target: doc })
        );
        let item = Target::Item(iid(99));
        assert_eq!(
            run(chain(), Gate::Ca, item, "x"),
            Err(AdoptionRefusal::TargetUnknown { target: item })
        );
    }

    #[test]
    fn an_invalid_target_is_refused() {
        // Two files with one id are both invalid.
        let mut files = chain();
        files.push(Filed::public_at(
            "copy/finding.md",
            Entity::Finding(finding(1, "A different body.")),
        ));
        let doc = Target::Doc(docid(1));
        assert_eq!(
            run(files, Gate::Cr, doc, "x"),
            Err(AdoptionRefusal::TargetInvalid { target: doc })
        );

        // A private document that git would publish is invalid.
        let exposed = vec![Filed::private_exposed(Entity::Finding(finding(
            1,
            "Login takes 8 s.",
        )))];
        assert_eq!(
            run(exposed, Gate::Cr, doc, "x"),
            Err(AdoptionRefusal::TargetInvalid { target: doc })
        );
    }

    #[test]
    fn an_upstream_that_is_not_there_cannot_be_pinned() {
        let without_finding: Vec<Filed> = chain().into_iter().skip(1).collect();
        assert_eq!(
            run(without_finding, Gate::Cs, Target::Doc(docid(11)), "x"),
            Err(AdoptionRefusal::UpstreamUnavailable { upstream: eid(1) })
        );
        let only_design = vec![Filed::public(Entity::Design(design(
            21,
            11,
            vec![(1, identified(31, "cache", &["PIN-A"]))],
        )))];
        assert_eq!(
            run(only_design, Gate::Ci, Target::Doc(docid(21)), "x"),
            Err(AdoptionRefusal::UpstreamUnavailable { upstream: eid(11) })
        );
    }

    #[test]
    fn a_ground_that_is_not_a_finding_cannot_be_pinned() {
        // Spec 12 names document 11 (a Spec) as its ground.
        let odd = Entity::Spec(spec(12, &[11], &["PIN-A"]));
        let mut files = chain();
        files.push(Filed::public_at("odd.md", odd));
        assert_eq!(
            run(files, Gate::Cs, Target::Doc(docid(12)), "x"),
            Err(AdoptionRefusal::UpstreamUnavailable { upstream: eid(11) })
        );
    }

    #[test]
    fn an_item_of_a_private_design_is_adopted_like_any_other() {
        let d = design(21, 11, vec![(1, identified(31, "cache", &["PIN-A"]))]);
        let files = vec![
            Filed::public(Entity::Spec(spec(11, &[1], &["PIN-A"]))),
            Filed::private(Entity::Design(d)),
        ];
        assert!(run(files, Gate::Ca, Target::Item(iid(31)), "x").is_ok());
    }
}
