//! Stage 3: the Design, a structured decision record and a split into items.
//!
//! A Design lives in its own file, apart from the Spec it implements. A Spec file
//! has no place for a Design: the file reader takes from a Spec only its front
//! matter and its `## Contract`, so a design written under another heading in a
//! Spec file is neither read nor hashed, and is not reported either.
//!
//! # A Design file
//!
//! The front matter holds `spec` (a reference to the Spec, never a hash), the
//! `crux` and the `items`; the body holds free notes, outside the hash.
//!
//! ```markdown
//! ---
//! spec: spec/session-reuse
//! crux:
//!   question: "Where is the session kept?"
//!   options:                          # exactly one chosen, at least one rejected
//!     - id: opt-memory
//!       summary: "Keep it in memory"
//!       chosen: true
//!       reason: "No new storage to secure"
//!     - id: opt-disk
//!       summary: "Keep it on disk"
//!       rejected: "A file to protect and to clean up"   # the reason is the value
//!     - id: opt-server
//!       summary: "Keep it on the server only"
//!       rejected: "Every start would need a round trip"
//! items:
//!   - number: 1                       # the key of the item; stable across edits
//!     summary: "Reuse the session inside the window"
//!     pins: [PIN-LOGIN-CACHE]         # every pin of the Spec is assigned to some item
//!   - number: 2
//!     summary: "Drop the session when it expires"
//!     pins: [UT-LOGIN-EXPIRY]
//! ---
//!
//! ## Design Notes
//! Free notes. They are kept in the file and left out of the hash.
//! ```
//!
//! An option is either `chosen: true` with a `reason`, or `rejected` with its
//! reason as the value. The assignment of pins is checked in both directions
//! ([`Design::check_pin_coverage`]): a pin of the Spec that no item carries, and a
//! pin on an item that the Spec never declared, are both reported.
//!
//! # Crux
//!
//! The [`Crux`] is an ADR in structured form: the question, the options
//! considered, the one chosen with its reason, and the others rejected, each
//! with a reason. A Design with no rejected option is not representable: the
//! rejection list is a [`NonEmptyVec`].
//!
//! # Items
//!
//! An item ([`WorkItem`]) is the smallest unit of implementation. Items sit in a
//! `BTreeMap<ItemNumber, WorkItem>`: the number is the key and is not repeated in
//! the value. The [`ItemId`](super::id::ItemId) belonging to a `(Design, number)`
//! pair is recorded by a registration event, which is why an item keeps its
//! identity when the Design is revised. An item whose number leaves the Design is
//! "gone": it drops out of the cache while its events stay in the log.
//!
//! Every PIN and UT the Spec declared must be assigned to some item, and no item
//! may be assigned a tag the Spec did not declare: [`Design::check_pin_coverage`]
//! checks both directions. Gate CI and the evaluation of a Design both call it.
//!
//! # Hashes
//!
//! `design_hash` covers the Spec reference, the Crux and the items. The Design
//! Notes section and the classification are outside it. The hash of one item
//! covers the Design identifier, the number, the summary and the assigned pins
//! ([`Design::item_hash`]); gate CA approves that hash and records the
//! Design's hash as its upstream.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::str::FromStr;

use super::canon::{Canon, ToCanon};
use super::error::{EntityError, ValueError};
use super::hash::{ContentHash, HashKind};
use super::id::{DesignId, ItemNumber, SpecId};
use super::nonempty::{NonBlank, NonEmptyVec};
use super::pin::PinId;
use super::spec::Spec;

/// The short name of an option in a Crux (`opt-json-field`).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct OptionId(String);

impl OptionId {
    /// The name as text.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for OptionId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl FromStr for OptionId {
    type Err = ValueError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let ok = !s.is_empty()
            && s.chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || ".-_".contains(c));
        if ok {
            Ok(Self(s.to_owned()))
        } else {
            Err(ValueError::OptionIdBadShape)
        }
    }
}

impl ToCanon for OptionId {
    fn to_canon(&self) -> Canon {
        Canon::str(self.0.as_str())
    }
}

/// The option that was chosen, and why.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChosenOption {
    id: OptionId,
    summary: NonBlank,
    reason: NonBlank,
}

impl ChosenOption {
    /// A chosen option; the reason is required.
    pub fn new(id: OptionId, summary: NonBlank, reason: NonBlank) -> Self {
        Self {
            id,
            summary,
            reason,
        }
    }

    /// The option's name.
    pub fn id(&self) -> &OptionId {
        &self.id
    }

    /// What the option is.
    pub fn summary(&self) -> &NonBlank {
        &self.summary
    }

    /// Why it was chosen.
    pub fn reason(&self) -> &NonBlank {
        &self.reason
    }
}

impl ToCanon for ChosenOption {
    fn to_canon(&self) -> Canon {
        Canon::map([
            ("id", self.id.to_canon()),
            ("summary", self.summary.to_canon()),
            ("reason", self.reason.to_canon()),
        ])
    }
}

/// An option that was considered and not taken, with the reason.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RejectedOption {
    id: OptionId,
    summary: NonBlank,
    reason: NonBlank,
}

impl RejectedOption {
    /// A rejected option; the reason is required.
    pub fn new(id: OptionId, summary: NonBlank, reason: NonBlank) -> Self {
        Self {
            id,
            summary,
            reason,
        }
    }

    /// The option's name.
    pub fn id(&self) -> &OptionId {
        &self.id
    }

    /// What the option is.
    pub fn summary(&self) -> &NonBlank {
        &self.summary
    }

    /// Why it was rejected.
    pub fn reason(&self) -> &NonBlank {
        &self.reason
    }
}

impl ToCanon for RejectedOption {
    fn to_canon(&self) -> Canon {
        Canon::map([
            ("id", self.id.to_canon()),
            ("summary", self.summary.to_canon()),
            ("reason", self.reason.to_canon()),
        ])
    }
}

/// A structured decision record: question, chosen option, rejected options.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Crux {
    question: NonBlank,
    chosen: ChosenOption,
    rejected: NonEmptyVec<RejectedOption>,
}

impl Crux {
    /// A Crux. The option names must be distinct across the chosen and the
    /// rejected ones.
    pub fn new(
        question: NonBlank,
        chosen: ChosenOption,
        rejected: NonEmptyVec<RejectedOption>,
    ) -> Result<Self, EntityError> {
        let mut seen = BTreeSet::new();
        seen.insert(chosen.id().clone());
        for option in rejected.iter() {
            if !seen.insert(option.id().clone()) {
                return Err(EntityError::DuplicateOptionId {
                    id: option.id().to_string(),
                });
            }
        }
        Ok(Self {
            question,
            chosen,
            rejected,
        })
    }

    /// The question the decision answers.
    pub fn question(&self) -> &NonBlank {
        &self.question
    }

    /// The chosen option.
    pub fn chosen(&self) -> &ChosenOption {
        &self.chosen
    }

    /// The options that were rejected; at least one.
    pub fn rejected(&self) -> &NonEmptyVec<RejectedOption> {
        &self.rejected
    }
}

impl ToCanon for Crux {
    fn to_canon(&self) -> Canon {
        Canon::map([
            ("question", self.question.to_canon()),
            ("chosen", self.chosen.to_canon()),
            ("rejected", self.rejected.to_canon()),
        ])
    }
}

/// The smallest unit of implementation.
///
/// It has no number and no identifier of its own: the number is the key of the
/// Design's map and the identifier is recorded by the registration event.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkItem {
    summary: NonBlank,
    assigned_pins: BTreeSet<PinId>,
}

impl WorkItem {
    /// An item from its summary and the PINs and UT tags it is responsible for.
    pub fn new(summary: NonBlank, assigned_pins: BTreeSet<PinId>) -> Self {
        Self {
            summary,
            assigned_pins,
        }
    }

    /// What the item does.
    pub fn summary(&self) -> &NonBlank {
        &self.summary
    }

    /// The PINs and UT tags assigned to this item.
    pub fn assigned_pins(&self) -> &BTreeSet<PinId> {
        &self.assigned_pins
    }

    /// The hash gate CA approves, for this item as number `number` of `design`.
    ///
    /// Covers the Design identifier, the number, the summary and the assigned
    /// pins. The number and Design are part of it because the same words under
    /// another number are another item.
    pub fn hash(&self, design: DesignId, number: ItemNumber) -> ContentHash {
        ContentHash::of(
            HashKind::Item,
            &Canon::map([
                ("design", design.to_canon()),
                ("number", number.to_canon()),
                ("summary", self.summary.to_canon()),
                (
                    "pins",
                    Canon::list(self.assigned_pins.iter().map(ToCanon::to_canon)),
                ),
            ]),
        )
    }
}

/// A mismatch between a Spec's declared pins and a Design's assignment.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum CoverageIssue {
    /// A pin the Spec declared that no item is assigned.
    ///
    /// Fix: list the pin in the `pins` of some item.
    Unassigned(PinId),
    /// A pin assigned to an item that the Spec did not declare.
    ///
    /// Fix: remove it from the item, or declare it in the Spec.
    Undeclared {
        /// The pin that is not in the Spec.
        pin: PinId,
        /// The item it was assigned to.
        item: ItemNumber,
    },
}

/// A Design.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Design {
    id: DesignId,
    spec: SpecId,
    crux: Crux,
    items: BTreeMap<ItemNumber, WorkItem>,
    design_hash: ContentHash,
}

impl Design {
    /// Build a Design and compute its hash.
    pub fn new(
        id: DesignId,
        spec: SpecId,
        crux: Crux,
        items: BTreeMap<ItemNumber, WorkItem>,
    ) -> Self {
        let items_canon = Canon::Map(
            items
                .iter()
                .map(|(number, item)| {
                    (
                        number.get().to_string(),
                        Canon::map([
                            ("summary", item.summary.to_canon()),
                            (
                                "pins",
                                Canon::list(item.assigned_pins.iter().map(ToCanon::to_canon)),
                            ),
                        ]),
                    )
                })
                .collect(),
        );
        let design_hash = ContentHash::of(
            HashKind::Design,
            &Canon::map([
                ("spec", spec.to_canon()),
                ("crux", crux.to_canon()),
                ("items", items_canon),
            ]),
        );
        Self {
            id,
            spec,
            crux,
            items,
            design_hash,
        }
    }

    /// The identifier.
    pub fn id(&self) -> DesignId {
        self.id
    }

    /// The Spec this Design implements.
    pub fn spec(&self) -> SpecId {
        self.spec
    }

    /// The decision record.
    pub fn crux(&self) -> &Crux {
        &self.crux
    }

    /// The items by number.
    pub fn items(&self) -> &BTreeMap<ItemNumber, WorkItem> {
        &self.items
    }

    /// The hash gate CI approves.
    pub fn design_hash(&self) -> &ContentHash {
        &self.design_hash
    }

    /// The hash of one item, which gate CA approves; `None` when the number is
    /// not in this Design.
    pub fn item_hash(&self, number: ItemNumber) -> Option<ContentHash> {
        self.items
            .get(&number)
            .map(|item| item.hash(self.id, number))
    }

    /// Check the assignment of pins against the Spec in both directions.
    ///
    /// Returns every mismatch, not the first: a pin the Spec declared that no
    /// item takes, and a pin an item takes that the Spec never declared.
    pub fn check_pin_coverage(&self, spec: &Spec) -> Vec<CoverageIssue> {
        let assigned: BTreeSet<&PinId> = self
            .items
            .values()
            .flat_map(|item| item.assigned_pins.iter())
            .collect();
        let mut issues: Vec<CoverageIssue> = spec
            .pins()
            .keys()
            .filter(|pin| !assigned.contains(pin))
            .map(|pin| CoverageIssue::Unassigned(pin.clone()))
            .collect();
        for (number, item) in &self.items {
            for pin in &item.assigned_pins {
                if !spec.pins().contains_key(pin) {
                    issues.push(CoverageIssue::Undeclared {
                        pin: pin.clone(),
                        item: *number,
                    });
                }
            }
        }
        issues
    }
}

#[cfg(test)]
pub(crate) mod test_support {
    use super::*;
    use crate::domain::id::test_support::{did, sid};

    pub(crate) fn num(n: u32) -> ItemNumber {
        ItemNumber::new(n).unwrap()
    }

    pub(crate) fn crux() -> Crux {
        Crux::new(
            NonBlank::new("Where is the result bound to the commit?").unwrap(),
            ChosenOption::new(
                "opt-json-field".parse().unwrap(),
                NonBlank::new("add a commit field to the result").unwrap(),
                NonBlank::new("reuses the existing delivery path").unwrap(),
            ),
            NonEmptyVec::new(
                RejectedOption::new(
                    "opt-notes".parse().unwrap(),
                    NonBlank::new("store the result in notes").unwrap(),
                    NonBlank::new("notes are easy to lose on fetch").unwrap(),
                ),
                vec![],
            ),
        )
        .unwrap()
    }

    pub(crate) fn item(summary: &str, pins: &[&str]) -> WorkItem {
        WorkItem::new(
            NonBlank::new(summary).unwrap(),
            pins.iter().map(|p| p.parse().unwrap()).collect(),
        )
    }

    pub(crate) fn design(n: u128, spec: u128, items: Vec<(u32, WorkItem)>) -> Design {
        Design::new(
            did(n),
            sid(spec),
            crux(),
            items.into_iter().map(|(k, v)| (num(k), v)).collect(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::test_support::*;
    use super::*;
    use crate::domain::spec::test_support::spec;

    #[test]
    fn option_ids_have_a_shape() {
        assert!("opt-1".parse::<OptionId>().is_ok());
        assert_eq!("Opt".parse::<OptionId>(), Err(ValueError::OptionIdBadShape));
        assert_eq!("".parse::<OptionId>(), Err(ValueError::OptionIdBadShape));
    }

    #[test]
    fn a_crux_with_a_repeated_option_id_is_refused() {
        let chosen = ChosenOption::new(
            "a".parse().unwrap(),
            NonBlank::new("s").unwrap(),
            NonBlank::new("r").unwrap(),
        );
        let rejected = RejectedOption::new(
            "a".parse().unwrap(),
            NonBlank::new("s").unwrap(),
            NonBlank::new("r").unwrap(),
        );
        assert_eq!(
            Crux::new(
                NonBlank::new("q").unwrap(),
                chosen,
                NonEmptyVec::new(rejected, vec![])
            ),
            Err(EntityError::DuplicateOptionId { id: "a".into() })
        );
    }

    #[test]
    fn coverage_is_clean_when_every_pin_is_assigned_once_or_more() {
        let s = spec(1, &[1], &["PIN-A", "UT-B"]);
        let d = design(
            10,
            1,
            vec![(1, item("do a", &["PIN-A"])), (2, item("do b", &["UT-B"]))],
        );
        assert_eq!(d.check_pin_coverage(&s), vec![]);
    }

    #[test]
    fn coverage_reports_both_directions() {
        let s = spec(1, &[1], &["PIN-A", "PIN-B"]);
        let d = design(10, 1, vec![(1, item("x", &["PIN-A", "PIN-Z"]))]);
        let issues = d.check_pin_coverage(&s);
        assert_eq!(
            issues,
            vec![
                CoverageIssue::Unassigned("PIN-B".parse().unwrap()),
                CoverageIssue::Undeclared {
                    pin: "PIN-Z".parse().unwrap(),
                    item: num(1)
                },
            ]
        );
    }

    #[test]
    fn design_hash_follows_spec_crux_and_items_not_the_identifier() {
        let base = design(10, 1, vec![(1, item("x", &["PIN-A"]))]);
        assert_eq!(
            base.design_hash(),
            design(11, 1, vec![(1, item("x", &["PIN-A"]))]).design_hash()
        );
        assert_ne!(
            base.design_hash(),
            design(10, 2, vec![(1, item("x", &["PIN-A"]))]).design_hash()
        );
        assert_ne!(
            base.design_hash(),
            design(10, 1, vec![(1, item("y", &["PIN-A"]))]).design_hash()
        );
        assert_ne!(
            base.design_hash(),
            design(10, 1, vec![(2, item("x", &["PIN-A"]))]).design_hash()
        );
        assert_ne!(
            base.design_hash(),
            design(10, 1, vec![(1, item("x", &["PIN-B"]))]).design_hash()
        );
    }

    #[test]
    fn item_hash_covers_design_number_summary_and_pins() {
        let d = design(
            10,
            1,
            vec![(1, item("x", &["PIN-A"])), (2, item("x", &["PIN-A"]))],
        );
        // Same summary and pins under another number: different hash.
        assert_ne!(d.item_hash(num(1)), d.item_hash(num(2)));
        // Another design, same number, summary and pins: different hash.
        let other = design(11, 1, vec![(1, item("x", &["PIN-A"]))]);
        assert_ne!(d.item_hash(num(1)), other.item_hash(num(1)));
        // Another item's change does not move this item's hash.
        let changed = design(
            10,
            1,
            vec![(1, item("x", &["PIN-A"])), (2, item("z", &["PIN-A"]))],
        );
        assert_eq!(d.item_hash(num(1)), changed.item_hash(num(1)));
        assert_eq!(d.item_hash(num(9)), None);
    }

    // Golden vectors: fixed Design and item, fixed digests.
    #[test]
    fn golden_design_hash() {
        let d = design(
            10,
            1,
            vec![(
                1,
                item(
                    "Parse the result as JSON and match the commit",
                    &["PIN-JSON-PARSE", "PIN-SHA-MATCH"],
                ),
            )],
        );
        assert_eq!(
            d.design_hash().to_string(),
            "sha256:c38d065cb8584b5c8a3b09b18bce438fb6c009cc7d85f9cc4ddeec3be355065d"
        );
    }

    #[test]
    fn golden_item_hash() {
        let d = design(
            10,
            1,
            vec![(
                1,
                item(
                    "Parse the result as JSON and match the commit",
                    &["PIN-JSON-PARSE", "PIN-SHA-MATCH"],
                ),
            )],
        );
        assert_eq!(
            d.item_hash(num(1)).unwrap().to_string(),
            "sha256:6f46cf41eff221739308ba82b48cf23b6d42d976520eba3e8ea44a6a40b9e4d9"
        );
    }
}
