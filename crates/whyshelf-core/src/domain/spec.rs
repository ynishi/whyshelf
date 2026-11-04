//! Stage 2: the Spec, a Contract and the way it will be bound.
//!
//! A Spec has two parts:
//!
//! - the **Contract**: the boundary conditions, invariants and input/output
//!   promises that must hold;
//! - the **acceptance declaration**: which PINs and unit-test tags will bind the
//!   Contract, and which hypotheses (HYP) say how a metric is expected to move.
//!   No test code is written at this stage; the tests come with the code in the
//!   implementation stage and are checked by gate CA.
//!
//! A Spec names the Findings it stands on (`grounds`, at least one, kept as a
//! [`NonEmptyVec`]). It never contains a Design: a Design is a separate file with
//! its own life.
//!
//! # A Spec file
//!
//! The front matter has `grounds` (references to Findings, by alias or identifier,
//! and never a hash) and `accept`, the acceptance declaration. The body has a
//! `## Contract` section, which is the Contract and is required; any other section
//! (`## Design Notes`, say) is kept in the file and read by people, and is outside
//! the hash.
//!
//! ```markdown
//! ---
//! grounds:
//!   - finding/slow-login        # at least one; a reference, not a hash
//!   - finding/session-dropped
//! accept:
//!   pins:                       # invariants the code must keep
//!     - id: PIN-LOGIN-CACHE     # PIN-* or UT-*; carried by a test as `@pin: PIN-...`
//!       contract: "The session is reused for five minutes"
//!     - id: UT-LOGIN-EXPIRY
//!       contract: "An expired session is not reused"
//!   hyp:                        # hypotheses, fixed by hash at CS, measured at CM
//!     - id: HYP-FASTER-LOGIN
//!       metric: "median login time"
//!       target: "under 2 s"
//! ---
//!
//! ## Contract
//! A session that is younger than five minutes is reused without a new sign-in;
//! an older one is not.
//!
//! ## Design Notes
//! Free notes. Correcting a typo here never removes an approval.
//! ```
//!
//! Gate CS asks for at least one pin and at least one hypothesis, so a Spec whose
//! `accept` is empty parses (it is a draft) but cannot be approved. The identifier of
//! a pin is `PIN-` or `UT-` followed by capital letters, digits, `_` or `-`; a
//! hypothesis is `HYP-` and the same.
//!
//! # Hashes
//!
//! The Spec has two hashes, both computed here:
//!
//! - `contract_hash` covers `grounds`, the accepted pins and hypotheses and the
//!   Contract section. Gate CS approves it; downstream stages record it.
//! - `hyp_hash` covers the hypotheses only. Gate CM checks a measurement against
//!   the `hyp_hash` that the CS approval fixed, so a hypothesis cannot be
//!   quietly rewritten after the fact.
//!
//! Supplementary sections and classification are outside both. Upstream hashes
//! (the hashes of the grounds) are not in the file and not in the hash: they are
//! recorded by the approval event, which is how an upstream change shows up as
//! Stale without rewriting the Spec.

use std::collections::{BTreeMap, BTreeSet};

use super::canon::{Canon, ToCanon};
use super::error::EntityError;
use super::hash::{ContentHash, HashKind};
use super::id::{FindingId, SpecId};
use super::nonempty::{NonBlank, NonEmptyVec};
use super::pin::{HypId, Hypothesis, PinContract, PinId};

/// A Spec.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Spec {
    id: SpecId,
    grounds: NonEmptyVec<FindingId>,
    contract: NonBlank,
    pins: BTreeMap<PinId, PinContract>,
    hyps: BTreeMap<HypId, Hypothesis>,
    contract_hash: ContentHash,
    hyp_hash: ContentHash,
}

impl Spec {
    /// Build a Spec and compute its two hashes.
    ///
    /// The grounds must not repeat a Finding. The order of the grounds is kept
    /// for display but does not enter the hash, so reordering them does not
    /// remove an approval.
    pub fn new(
        id: SpecId,
        grounds: NonEmptyVec<FindingId>,
        contract: NonBlank,
        pins: BTreeMap<PinId, PinContract>,
        hyps: BTreeMap<HypId, Hypothesis>,
    ) -> Result<Self, EntityError> {
        let mut seen = BTreeSet::new();
        for ground in grounds.iter() {
            if !seen.insert(*ground) {
                return Err(EntityError::DuplicateGround {
                    finding: ground.entity(),
                });
            }
        }
        let hyp_canon = Canon::Map(
            hyps.iter()
                .map(|(k, v)| (k.as_str().to_owned(), v.to_canon()))
                .collect(),
        );
        let pin_canon = Canon::Map(
            pins.iter()
                .map(|(k, v)| (k.as_str().to_owned(), v.to_canon()))
                .collect(),
        );
        // `seen` is a BTreeSet, so the grounds enter the hash sorted.
        let grounds_canon = Canon::list(seen.iter().map(ToCanon::to_canon));
        let contract_hash = ContentHash::of(
            HashKind::SpecContract,
            &Canon::map([
                ("grounds", grounds_canon),
                ("pins", pin_canon),
                ("hyps", hyp_canon.clone()),
                ("contract", contract.to_canon()),
            ]),
        );
        let hyp_hash = ContentHash::of(HashKind::SpecHyp, &hyp_canon);
        Ok(Self {
            id,
            grounds,
            contract,
            pins,
            hyps,
            contract_hash,
            hyp_hash,
        })
    }

    /// The identifier.
    pub fn id(&self) -> SpecId {
        self.id
    }

    /// The Findings this Spec stands on, in the order written.
    pub fn grounds(&self) -> &NonEmptyVec<FindingId> {
        &self.grounds
    }

    /// The Contract section.
    pub fn contract(&self) -> &NonBlank {
        &self.contract
    }

    /// The declared PINs and unit-test tags with the sentence each guarantees.
    pub fn pins(&self) -> &BTreeMap<PinId, PinContract> {
        &self.pins
    }

    /// The declared hypotheses.
    pub fn hyps(&self) -> &BTreeMap<HypId, Hypothesis> {
        &self.hyps
    }

    /// The hash gate CS approves.
    pub fn contract_hash(&self) -> &ContentHash {
        &self.contract_hash
    }

    /// The hash of the hypotheses, which gate CM matches against.
    pub fn hyp_hash(&self) -> &ContentHash {
        &self.hyp_hash
    }
}

/// The pins any Spec of the project declares.
///
/// A tag in a test is undeclared when **no** Spec declares it. Comparing it with the
/// pins of the Spec of one item would stop every approval in a project of two Specs,
/// since the tests of the other Spec carry tags that this one does not list; the
/// tests of a project are one set, and the declarations of all its Specs are the
/// other. A Spec that did not parse declares nothing, because there is no Spec to
/// read it from.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DeclaredPins(BTreeSet<PinId>);

impl DeclaredPins {
    /// The pins declared by these Specs.
    pub fn of<'a>(specs: impl IntoIterator<Item = &'a Spec>) -> Self {
        Self(
            specs
                .into_iter()
                .flat_map(|spec| spec.pins().keys().cloned())
                .collect(),
        )
    }

    /// Whether some Spec declares the pin.
    pub fn contains(&self, pin: &PinId) -> bool {
        self.0.contains(pin)
    }
}

#[cfg(test)]
pub(crate) mod test_support {
    use super::*;
    use crate::domain::id::test_support::{fid, sid};

    pub(crate) fn pins(ids: &[&str]) -> BTreeMap<PinId, PinContract> {
        ids.iter()
            .map(|id| {
                (
                    id.parse().unwrap(),
                    PinContract::new(format!("{id} holds")).unwrap(),
                )
            })
            .collect()
    }

    pub(crate) fn hyps(ids: &[&str]) -> BTreeMap<HypId, Hypothesis> {
        ids.iter()
            .map(|id| (id.parse().unwrap(), Hypothesis::new("reruns", "0").unwrap()))
            .collect()
    }

    pub(crate) fn spec(n: u128, grounds: &[u128], pin_ids: &[&str]) -> Spec {
        let mut it = grounds.iter().map(|g| fid(*g));
        let head = it.next().unwrap();
        Spec::new(
            sid(n),
            NonEmptyVec::new(head, it.collect()),
            NonBlank::new("Results are judged on structure, not on text.").unwrap(),
            pins(pin_ids),
            hyps(&["HYP-RERUN-ZERO"]),
        )
        .unwrap()
    }
}

#[cfg(test)]
mod tests {
    use super::test_support::*;
    use super::*;
    use crate::domain::id::test_support::{fid, sid};

    fn with(grounds: &[u128], contract: &str, pin_ids: &[&str], hyp_ids: &[&str]) -> Spec {
        let mut it = grounds.iter().map(|g| fid(*g));
        let head = it.next().unwrap();
        Spec::new(
            sid(1),
            NonEmptyVec::new(head, it.collect()),
            NonBlank::new(contract).unwrap(),
            pins(pin_ids),
            hyps(hyp_ids),
        )
        .unwrap()
    }

    #[test]
    fn a_repeated_ground_is_refused() {
        let err = Spec::new(
            sid(1),
            NonEmptyVec::new(fid(1), vec![fid(2), fid(1)]),
            NonBlank::new("c").unwrap(),
            BTreeMap::new(),
            BTreeMap::new(),
        )
        .unwrap_err();
        assert_eq!(
            err,
            EntityError::DuplicateGround {
                finding: fid(1).entity()
            }
        );
    }

    #[test]
    fn contract_hash_follows_grounds_pins_hyps_and_contract() {
        let base = with(&[1], "c", &["PIN-A"], &["HYP-X"]);
        assert_ne!(
            base.contract_hash(),
            with(&[2], "c", &["PIN-A"], &["HYP-X"]).contract_hash()
        );
        assert_ne!(
            base.contract_hash(),
            with(&[1], "d", &["PIN-A"], &["HYP-X"]).contract_hash()
        );
        assert_ne!(
            base.contract_hash(),
            with(&[1], "c", &["PIN-B"], &["HYP-X"]).contract_hash()
        );
        assert_ne!(
            base.contract_hash(),
            with(&[1], "c", &["PIN-A"], &["HYP-Y"]).contract_hash()
        );
        assert_eq!(
            base.contract_hash(),
            with(&[1], "c", &["PIN-A"], &["HYP-X"]).contract_hash()
        );
    }

    #[test]
    fn hyp_hash_covers_the_hypotheses_only() {
        let base = with(&[1], "c", &["PIN-A"], &["HYP-X"]);
        // Changing anything but the hypotheses leaves it alone.
        assert_eq!(
            base.hyp_hash(),
            with(&[2], "d", &["PIN-B"], &["HYP-X"]).hyp_hash()
        );
        assert_ne!(
            base.hyp_hash(),
            with(&[1], "c", &["PIN-A"], &["HYP-Y"]).hyp_hash()
        );
    }

    #[test]
    fn the_order_of_grounds_does_not_move_the_hash() {
        let a = with(&[1, 2], "c", &["PIN-A"], &["HYP-X"]);
        let b = with(&[2, 1], "c", &["PIN-A"], &["HYP-X"]);
        assert_eq!(a.contract_hash(), b.contract_hash());
        assert_eq!(a.grounds().first(), &fid(1));
        assert_eq!(b.grounds().first(), &fid(2));
    }

    #[test]
    fn the_identifier_is_not_hashed() {
        assert_eq!(
            spec(1, &[1], &["PIN-A"]).contract_hash(),
            spec(2, &[1], &["PIN-A"]).contract_hash()
        );
    }

    // Golden vectors: fixed Spec, fixed digests.
    #[test]
    fn golden_spec_contract_hash() {
        let s = with(
            &[1, 2],
            "Timeouts are reported, never hidden.\n",
            &["PIN-CACHE-TTL", "UT-TTL-EDGE"],
            &["HYP-RERUN-ZERO"],
        );
        assert_eq!(
            s.contract_hash().to_string(),
            "sha256:a0e13ea0f1b486aa2f3e5d917111b992488f1a911be3aad8aeee7024300ba1e7"
        );
    }

    #[test]
    fn golden_spec_hyp_hash() {
        let s = with(&[1], "x", &["PIN-A"], &["HYP-RERUN-ZERO"]);
        assert_eq!(
            s.hyp_hash().to_string(),
            "sha256:3555daf7ee3d379f52f6d686d148dd7f9e1d1cefea2cfc3f34ea64ade66fd3be"
        );
    }
}
