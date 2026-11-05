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
//!   - id: 0192f3a4-5b6c-7d8e-8f90-123456789abc   # the identity of the item (a UUID v7)
//!     number: 1                       # the human-facing key; it may be changed freely
//!     summary: "Reuse the session inside the window"
//!     pins: [PIN-LOGIN-CACHE]         # every pin of the Spec is assigned to some item
//!   - id: 0192f3a4-5b6c-7d8e-8f90-123456789abd
//!     number: 2
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
//! `BTreeMap<ItemNumber, WorkItem>`: the number is the human-facing key and is not
//! repeated in the value. The identity of an item is its
//! [`ItemId`], which the file writes as the `id:` of the item
//! and which does not change when the item is renumbered or the Design is revised.
//! An item written without an `id:` has none in the file; for it the registration
//! event records the identifier that goes with a `(Design, number)` pair. An item
//! whose number leaves the Design is "gone": it drops out of the cache
//! while its events stay in the log.
//!
//! Every PIN and UT the Spec declared must be assigned to some item, and no item
//! may be assigned a tag the Spec did not declare: [`Design::check_pin_coverage`]
//! checks both directions. Gate CI and the evaluation of a Design both call it.
//!
//! # Items that cannot be trusted
//!
//! [`Design::item_issues`] lists, for the identity of the items, what a Design
//! file taken as a document of the corpus cannot stand on. The rule is here and only
//! here; the corpus attaches each issue to the document as a reason.
//!
//! | situation | reported as |
//! |---|---|
//! | an item has no `id:` | [`ItemIssue::UnidentifiedItem`] |
//! | two items of one Design have one `id:` | [`ItemIssue::DuplicateItemId`] |
//! | an item has the `id:` of its own Design | [`ItemIssue::ItemIdIsDesignId`] |
//! | two items of one Design have one number | not representable here: the number is the key of the map, so the file reader refuses the file with `UnparsedReason::DuplicateItemNumber` |
//!
//! # Hashes
//!
//! `design_hash` covers the Spec reference, the Crux and the items. The Design
//! Notes section and the classification are outside it. The hash of one item
//! covers the Design identifier, the summary and the assigned pins
//! ([`Design::item_hash`]); gate CA approves that hash and records the
//! Design's hash as its upstream.
//!
//! The number of an item is in neither hash, and neither is its `id:`. Renumbering
//! items, or writing their `id:` into a file, does not change a hash, so an approval
//! stays. For the Design hash this means the items are taken as a set: the hash does
//! not depend on which number or which position an item has, only on what the items
//! say. Two items that say the same thing have one item hash, and both count in the
//! Design hash: adding a copy of an item changes it.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::str::FromStr;

use super::canon::{Canon, ToCanon};
use super::error::{EntityError, ValueError};
use super::hash::{ContentHash, HashKind};
use super::id::{DesignId, ItemId, ItemNumber, SpecId};
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
/// It has no number of its own: the number is the key of the Design's map. It
/// carries the identifier its file wrote as `id:`, if the file wrote one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkItem {
    id: Option<ItemId>,
    summary: NonBlank,
    assigned_pins: BTreeSet<PinId>,
}

impl WorkItem {
    /// An item from its summary and the PINs and UT tags it is responsible for,
    /// with no identifier written in the file.
    pub fn new(summary: NonBlank, assigned_pins: BTreeSet<PinId>) -> Self {
        Self {
            id: None,
            summary,
            assigned_pins,
        }
    }

    /// The same item with the identifier its file wrote as `id:`.
    ///
    /// The identifier is no part of the item's hash: writing it into a file that
    /// had none leaves every hash as it was.
    #[must_use]
    pub fn with_id(mut self, id: ItemId) -> Self {
        self.id = Some(id);
        self
    }

    /// The identifier the file wrote as `id:`; `None` for an item written without
    /// one.
    pub fn id(&self) -> Option<ItemId> {
        self.id
    }

    /// The entries of the canonical form of what the item says: the summary and the
    /// assigned pins, and nothing about where it sits or how it is named. Both the
    /// item hash and the Design hash are made of these.
    fn content_entries(&self) -> [(&'static str, Canon); 2] {
        [
            ("summary", self.summary.to_canon()),
            (
                "pins",
                Canon::list(self.assigned_pins.iter().map(ToCanon::to_canon)),
            ),
        ]
    }

    /// What the item does.
    pub fn summary(&self) -> &NonBlank {
        &self.summary
    }

    /// The PINs and UT tags assigned to this item.
    pub fn assigned_pins(&self) -> &BTreeSet<PinId> {
        &self.assigned_pins
    }

    /// The hash gate CA approves, for this item as an item of `design`.
    ///
    /// Covers the Design identifier, the summary and the assigned pins. The Design
    /// is part of it because the same words in another Design are another item.
    /// Neither the number nor the `id:` is: the number is a name that may be
    /// changed, as a classification is, and the identity is what an approval is
    /// kept under, not what it is about.
    pub fn hash(&self, design: DesignId) -> ContentHash {
        ContentHash::of(
            HashKind::Item,
            &Canon::map(
                std::iter::once(("design", design.to_canon())).chain(self.content_entries()),
            ),
        )
    }
}

/// A fault in the identity of the items of a Design.
///
/// Each is a fact about the Design file as a document; none says which item is
/// right. They are found by [`Design::item_issues`].
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ItemIssue {
    /// An item has no `id:`: it was added by hand.
    ///
    /// Fix: write an `id:` (a UUID v7) into the item; importing does it.
    #[error("the item {number} has no `id:`")]
    UnidentifiedItem {
        /// The number of the item.
        number: ItemNumber,
    },
    /// Two or more items of one Design carry the same `id:`.
    ///
    /// Fix: give all but one a new `id:`, or remove the copies.
    #[error("the items {} carry one id {id}", join_numbers(.numbers))]
    DuplicateItemId {
        /// The shared identifier.
        id: ItemId,
        /// The numbers of the items that carry it, in order; at least two.
        numbers: NonEmptyVec<ItemNumber>,
    },
    /// An item carries the `id:` of its own Design: identities are one namespace.
    ///
    /// Fix: give the item a new `id:`.
    #[error("the item {number} carries the id of its own Design")]
    ItemIdIsDesignId {
        /// The number of the item.
        number: ItemNumber,
    },
}

/// The numbers of the items an issue names, as `#1, #3`.
fn join_numbers(numbers: &NonEmptyVec<ItemNumber>) -> String {
    numbers
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join(", ")
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
        // The items are a set: sorted by their bytes, so that neither the number nor
        // the position of an item (which follows the number) is in the hash.
        let mut item_forms: Vec<(Vec<u8>, Canon)> = items
            .values()
            .map(|item| {
                let form = Canon::map(item.content_entries());
                (form.to_bytes(), form)
            })
            .collect();
        item_forms.sort_by(|a, b| a.0.cmp(&b.0));
        let items_canon = Canon::list(item_forms.into_iter().map(|(_, form)| form));
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
        self.items.get(&number).map(|item| item.hash(self.id))
    }

    /// What a Design file taken as a document cannot stand on in the identity of its
    /// items: every item with no `id:`, and every `id:` that more than one item
    /// carries. Empty when each item has an identity of its own.
    ///
    /// The old pipeline, which gives an item its identifier by a registration event,
    /// does not ask: an item with no `id:` is fine there. The items with no `id:`
    /// come first, by number; then the shared identifiers, by identifier.
    pub fn item_issues(&self) -> Vec<ItemIssue> {
        let mut issues = Vec::new();
        let mut carriers: BTreeMap<ItemId, Vec<ItemNumber>> = BTreeMap::new();
        for (number, item) in &self.items {
            match item.id {
                Some(id) => {
                    if id.entity() == self.id.entity() {
                        issues.push(ItemIssue::ItemIdIsDesignId { number: *number });
                    }
                    carriers.entry(id).or_default().push(*number);
                }
                None => issues.push(ItemIssue::UnidentifiedItem { number: *number }),
            }
        }
        for (id, numbers) in carriers {
            if numbers.len() > 1
                && let Ok(numbers) = NonEmptyVec::from_vec(numbers)
            {
                issues.push(ItemIssue::DuplicateItemId { id, numbers });
            }
        }
        issues
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

    /// An item that carries the `id:` built from `id_n`.
    pub(crate) fn identified(id_n: u128, summary: &str, pins: &[&str]) -> WorkItem {
        item(summary, pins).with_id(crate::domain::id::test_support::iid(id_n))
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
    use crate::domain::id::test_support::iid;
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
            design(10, 1, vec![(1, item("x", &["PIN-B"]))]).design_hash()
        );
        // Another item added: a different set of items.
        assert_ne!(
            base.design_hash(),
            design(
                10,
                1,
                vec![(1, item("x", &["PIN-A"])), (2, item("x", &["PIN-A"]))]
            )
            .design_hash()
        );
    }

    #[test]
    fn renumbering_items_keeps_the_item_hash_and_the_design_hash() {
        let before = design(
            10,
            1,
            vec![(1, item("a", &["PIN-A"])), (2, item("b", &["PIN-B"]))],
        );
        // One item takes another number.
        let moved = design(
            10,
            1,
            vec![(1, item("a", &["PIN-A"])), (7, item("b", &["PIN-B"]))],
        );
        assert_eq!(before.design_hash(), moved.design_hash());
        assert_eq!(before.item_hash(num(2)), moved.item_hash(num(7)));
        assert_eq!(before.item_hash(num(1)), moved.item_hash(num(1)));
        // The two items swap their numbers, so each sits where the other was.
        let swapped = design(
            10,
            1,
            vec![(1, item("b", &["PIN-B"])), (2, item("a", &["PIN-A"]))],
        );
        assert_eq!(before.design_hash(), swapped.design_hash());
        assert_eq!(before.item_hash(num(1)), swapped.item_hash(num(2)));
        assert_eq!(before.item_hash(num(2)), swapped.item_hash(num(1)));
    }

    #[test]
    fn writing_the_item_ids_into_a_design_changes_no_hash() {
        let bare = design(10, 1, vec![(1, item("a", &["PIN-A"]))]);
        let identified = design(10, 1, vec![(1, identified(5, "a", &["PIN-A"]))]);
        assert_eq!(bare.design_hash(), identified.design_hash());
        assert_eq!(bare.item_hash(num(1)), identified.item_hash(num(1)));
    }

    #[test]
    fn item_hash_covers_design_summary_and_pins_not_the_number() {
        let d = design(
            10,
            1,
            vec![(1, item("x", &["PIN-A"])), (2, item("x", &["PIN-A"]))],
        );
        // Same summary and pins under another number: the same hash.
        assert_eq!(d.item_hash(num(1)), d.item_hash(num(2)));
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

    #[test]
    fn an_item_keeps_the_id_its_file_wrote() {
        assert_eq!(item("a", &[]).id(), None);
        assert_eq!(identified(5, "a", &[]).id(), Some(iid(5)));
    }

    #[test]
    fn items_that_each_carry_an_id_have_no_issue() {
        let d = design(
            10,
            1,
            vec![
                (1, identified(1, "a", &["PIN-A"])),
                (2, identified(2, "a", &["PIN-A"])),
            ],
        );
        assert_eq!(d.item_issues(), vec![]);
    }

    #[test]
    fn an_item_without_an_id_is_unidentified() {
        let d = design(
            10,
            1,
            vec![
                (1, identified(1, "a", &[])),
                (2, item("b", &[])),
                (4, item("c", &[])),
            ],
        );
        assert_eq!(
            d.item_issues(),
            vec![
                ItemIssue::UnidentifiedItem { number: num(2) },
                ItemIssue::UnidentifiedItem { number: num(4) },
            ]
        );
    }

    #[test]
    fn items_sharing_an_id_are_named_with_their_numbers() {
        let d = design(
            10,
            1,
            vec![
                (1, identified(7, "a", &[])),
                (2, identified(8, "b", &[])),
                (3, identified(7, "c", &[])),
                (5, identified(7, "d", &[])),
            ],
        );
        assert_eq!(
            d.item_issues(),
            vec![ItemIssue::DuplicateItemId {
                id: iid(7),
                numbers: NonEmptyVec::new(num(1), vec![num(3), num(5)]),
            }]
        );
    }

    #[test]
    fn unidentified_and_duplicated_items_are_both_reported() {
        let d = design(
            10,
            1,
            vec![
                (1, identified(7, "a", &[])),
                (2, identified(7, "b", &[])),
                (3, item("c", &[])),
            ],
        );
        let issues = d.item_issues();
        assert_eq!(issues.len(), 2);
        assert!(issues.contains(&ItemIssue::UnidentifiedItem { number: num(3) }));
    }

    #[test]
    fn an_item_with_the_id_of_its_own_design_is_reported() {
        let d = design(
            10,
            1,
            vec![(1, identified(10, "a", &[])), (2, identified(8, "b", &[]))],
        );
        assert_eq!(
            d.item_issues(),
            vec![ItemIssue::ItemIdIsDesignId { number: num(1) }]
        );
    }

    #[test]
    fn an_issue_says_what_is_wrong() {
        assert_eq!(
            ItemIssue::UnidentifiedItem { number: num(2) }.to_string(),
            "the item #2 has no `id:`"
        );
        assert_eq!(
            ItemIssue::DuplicateItemId {
                id: iid(7),
                numbers: NonEmptyVec::new(num(1), vec![num(3)]),
            }
            .to_string(),
            format!("the items #1, #3 carry one id {}", iid(7))
        );
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
            "sha256:7c2185da3e0b737c1fc6d7a4052d1e2707230cc126e4f6901ea3edd322c34497"
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
            "sha256:3c9c1ecad56feb81d3eb72fb4d00c4503038b1e7bbb810fa50cfe80af1a21294"
        );
    }
}
