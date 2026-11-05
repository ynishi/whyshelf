//! What a project may decide for itself: what a gate demands, and how a pin is named.
//!
//! Two values, both read from the project's settings by the application and handed to
//! the domain, which only applies them:
//!
//! | value | decides | default |
//! |---|---|---|
//! | [`GatePolicy`] | what the machine check of CS demands of a Spec | pins and hypotheses are both required |
//! | [`PinVocabulary`] | which marks are pins, how they are spelled, and which outside tags stand for which pin | the prefixes `PIN` and `UT`, spelled strictly, no outside tags |
//!
//! Each default is the behaviour from before the value existed.
//!
//! # The gate policy
//!
//! ```text
//!   GatePolicy { cs_requires_pins, cs_requires_hypotheses }
//!        |
//!        v
//!   CS check ---- passes ----> Passed ---> Attestation::Cs { .., policy }
//!                                               |
//!                                               v
//!                                  recorded in the approval, in its canonical
//!                                  form, and shown as GateBasis::Cs { policy }
//! ```
//!
//! The settings file is committed, so anyone who can change the repository can relax
//! the policy. That is allowed, and it is why a passed CS carries the policy it
//! passed under: an approval made under a relaxed policy is told from one made under
//! the default by reading the approval, never by asking what the settings say today.
//! Two approvals of one Spec that differ only in the policy are two different
//! attestations.
//!
//! A Spec that declares no pins passes CS when the policy does not require them. CI's
//! check of the pin coverage and CA's check of the pins in the tests then have nothing
//! to compare, and pass on an empty set. CA still runs the tests. CR, CI, CA and CM do
//! not read the policy, so their approvals do not carry one.
//!
//! # The pin vocabulary
//!
//! A pin is written `<PREFIX>-<NAME>`. The vocabulary lists the prefixes a project
//! uses ([`PinPrefix`]), each saying whether its pins are invariants or unit-test
//! tags ([`PinKind`]), how a mark found in code is normalized before it is read
//! ([`PinNormalize`]), and a table from the tags a project already has to the pins
//! they stand for ([`ExternalTag`]).
//!
//! [`PinVocabulary::read`] (and [`PinId::parse_in`]) takes a mark as written and
//! decides, in this order:
//!
//! 1. the mark is an external tag of the table: it is the pin the table names;
//! 2. otherwise the mark is normalized, and it must be a prefix of the vocabulary, a
//!    `-`, and a name of capitals, digits, `_` and `-`: it is that pin;
//! 3. otherwise it is refused. A mark that was meant as a pin and is not one of the
//!    vocabulary is not dropped by the scan: it is carried as a
//!    [`MalformedPin`](super::pin::MalformedPin) to the gate, which reports it.
//!
//! ```text
//!   @pin: AUTH_LOGIN         --table-------------> PIN-AUTH-LOGIN
//!   @pin: pin_login_cache    --UpperDash----------> PIN-LOGIN-CACHE
//!   @pin: UT-PARSE-EMPTY     --default vocabulary-> UT-PARSE-EMPTY
//!   @pin: REQ-LOGIN          --not in vocabulary--> MalformedPin
//! ```
//!
//! [`PinId`]'s `FromStr` reads in the default vocabulary, so what it accepts is what
//! the grammar accepted before the vocabulary existed.

use std::borrow::Borrow;
use std::collections::{BTreeMap, BTreeSet};

use super::canon::{Canon, ToCanon};
use super::error::ValueError;
use super::nonempty::NonEmptyVec;
use super::pin::{PinId, PinKind, is_name};

/// What the machine check of CS demands of a Spec.
///
/// A project may relax either demand. The value is part of what a passed CS
/// records (see the module documentation), so a relaxed approval can be told from
/// the default one afterwards.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct GatePolicy {
    cs_requires_pins: bool,
    cs_requires_hypotheses: bool,
}

impl GatePolicy {
    /// A policy from its two demands.
    pub fn new(cs_requires_pins: bool, cs_requires_hypotheses: bool) -> Self {
        Self {
            cs_requires_pins,
            cs_requires_hypotheses,
        }
    }

    /// Whether CS refuses a Spec that declares no pin.
    pub fn cs_requires_pins(&self) -> bool {
        self.cs_requires_pins
    }

    /// Whether CS refuses a Spec that declares no hypothesis.
    pub fn cs_requires_hypotheses(&self) -> bool {
        self.cs_requires_hypotheses
    }

    /// Whether it demands less than the default does.
    pub fn is_relaxed(&self) -> bool {
        *self != Self::default()
    }
}

impl Default for GatePolicy {
    /// Both demands: the behaviour from before the policy existed.
    fn default() -> Self {
        Self::new(true, true)
    }
}

impl ToCanon for GatePolicy {
    fn to_canon(&self) -> Canon {
        Canon::map([
            ("cs_requires_pins", Canon::Bool(self.cs_requires_pins)),
            (
                "cs_requires_hypotheses",
                Canon::Bool(self.cs_requires_hypotheses),
            ),
        ])
    }
}

/// The part of a pin before the first `-`: `PIN`, `UT`, or a prefix a project adds.
///
/// Capital letters and digits, starting with a letter. It holds no `-` or `_`, so
/// the first `-` of a pin always ends the prefix, and normalizing a mark never
/// changes it. `HYP` is not available: it begins a hypothesis, and a pin that
/// began with it could be read as one.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PinPrefix {
    text: String,
    kind: PinKind,
}

impl PinPrefix {
    /// A prefix and what its pins are.
    pub fn new(text: impl Into<String>, kind: PinKind) -> Result<Self, ValueError> {
        let text = text.into();
        let mut chars = text.chars();
        let shaped = chars.next().is_some_and(|c| c.is_ascii_uppercase())
            && chars.all(|c| c.is_ascii_uppercase() || c.is_ascii_digit());
        if !shaped {
            return Err(ValueError::PinPrefixBadShape);
        }
        if text == "HYP" {
            return Err(ValueError::PinPrefixReserved { prefix: text });
        }
        Ok(Self { text, kind })
    }

    /// The prefix as text, without the `-`.
    pub fn as_str(&self) -> &str {
        &self.text
    }

    /// What the pins of this prefix are.
    pub fn kind(&self) -> PinKind {
        self.kind
    }
}

/// How a mark found in code is normalized before it is read as a pin.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum PinNormalize {
    /// The mark is read as written: capitals, digits, `_` and `-`.
    Strict,
    /// Small letters become capitals and `_` becomes `-`, so `pin_login_cache` is
    /// `PIN-LOGIN-CACHE`.
    UpperDash,
}

impl PinNormalize {
    /// The mark after normalization.
    fn apply(self, mark: &str) -> String {
        match self {
            Self::Strict => mark.to_owned(),
            Self::UpperDash => mark
                .chars()
                .map(|c| {
                    if c == '_' {
                        '-'
                    } else {
                        c.to_ascii_uppercase()
                    }
                })
                .collect(),
        }
    }
}

/// A tag a project already uses for something that is a pin here
/// (`AUTH_LOGIN`), as written in code.
///
/// Any text without white space; it is compared as written, so the table is exact.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ExternalTag(String);

impl ExternalTag {
    /// A tag; empty, or with white space, is refused.
    pub fn new(text: impl Into<String>) -> Result<Self, ValueError> {
        let text = text.into();
        if text.is_empty() || text.chars().any(char::is_whitespace) {
            Err(ValueError::ExternalTagBadShape)
        } else {
            Ok(Self(text))
        }
    }

    /// The tag as written.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Borrow<str> for ExternalTag {
    fn borrow(&self) -> &str {
        &self.0
    }
}

/// Which marks are pins, how they are spelled, and which tags stand for which pin.
/// See the module documentation for how a mark is read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PinVocabulary {
    prefixes: NonEmptyVec<PinPrefix>,
    normalize: PinNormalize,
    mapping: BTreeMap<ExternalTag, PinId>,
}

impl PinVocabulary {
    /// A vocabulary from its parts.
    ///
    /// A prefix may be listed once. A table entry must name a pin of a listed
    /// prefix: a pin the vocabulary would itself refuse cannot be what a tag stands
    /// for. The pin of an entry takes the kind of its prefix here, so a tag and the
    /// pin it stands for, written out, read as the same pin.
    pub fn new(
        prefixes: NonEmptyVec<PinPrefix>,
        normalize: PinNormalize,
        mapping: BTreeMap<ExternalTag, PinId>,
    ) -> Result<Self, ValueError> {
        let mut seen = BTreeSet::new();
        for prefix in prefixes.iter() {
            if !seen.insert(prefix.as_str()) {
                return Err(ValueError::PinPrefixDuplicate {
                    prefix: prefix.as_str().to_owned(),
                });
            }
        }
        let mapping = mapping
            .into_iter()
            .map(
                |(tag, pin)| match prefixes.iter().find(|p| p.as_str() == pin_prefix_of(&pin)) {
                    Some(prefix) => Ok((tag, PinId::from_parts(pin.to_string(), prefix.kind()))),
                    None => Err(ValueError::PinMappingOutsideVocabulary {
                        tag: tag.as_str().to_owned(),
                        pin: pin.to_string(),
                    }),
                },
            )
            .collect::<Result<BTreeMap<_, _>, _>>()?;
        Ok(Self {
            prefixes,
            normalize,
            mapping,
        })
    }

    /// The prefixes of the pins.
    pub fn prefixes(&self) -> &NonEmptyVec<PinPrefix> {
        &self.prefixes
    }

    /// How a mark is normalized.
    pub fn normalize(&self) -> PinNormalize {
        self.normalize
    }

    /// The pin each external tag stands for.
    pub fn mapping(&self) -> &BTreeMap<ExternalTag, PinId> {
        &self.mapping
    }

    /// The pin a mark in code is, if it is one of this vocabulary: first the table,
    /// then the normalized mark against the prefixes.
    pub fn read(&self, mark: &str) -> Result<PinId, ValueError> {
        if let Some(pin) = self.mapping.get(mark) {
            return Ok(pin.clone());
        }
        let text = self.normalize.apply(mark);
        let (prefix, name) = text.split_once('-').ok_or(ValueError::PinIdBadShape)?;
        let prefix = self
            .prefixes
            .iter()
            .find(|p| p.as_str() == prefix)
            .ok_or(ValueError::PinIdBadShape)?;
        if is_name(name) {
            Ok(PinId::from_parts(text, prefix.kind()))
        } else {
            Err(ValueError::PinIdBadShape)
        }
    }
}

/// The prefix of a pin: its text up to the first `-`.
fn pin_prefix_of(pin: &PinId) -> &str {
    pin.as_str()
        .split_once('-')
        .map_or(pin.as_str(), |(prefix, _)| prefix)
}

impl Default for PinVocabulary {
    /// `PIN` and `UT`, read as written, no external tags: the grammar from before the
    /// vocabulary existed.
    fn default() -> Self {
        let prefix = |text: &str, kind| PinPrefix {
            text: text.to_owned(),
            kind,
        };
        Self {
            prefixes: NonEmptyVec::new(
                prefix("PIN", PinKind::Pin),
                vec![prefix("UT", PinKind::UnitTest)],
            ),
            normalize: PinNormalize::Strict,
            mapping: BTreeMap::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vocabulary(
        prefixes: &[(&str, PinKind)],
        normalize: PinNormalize,
        mapping: &[(&str, &str)],
    ) -> Result<PinVocabulary, ValueError> {
        let mut prefixes = prefixes
            .iter()
            .map(|(text, kind)| PinPrefix::new(*text, *kind))
            .collect::<Result<Vec<_>, _>>()?;
        let head = prefixes.remove(0);
        let mapping = mapping
            .iter()
            .map(|(tag, pin)| {
                Ok((
                    ExternalTag::new(*tag)?,
                    PinId::from_parts((*pin).to_owned(), PinKind::Pin),
                ))
            })
            .collect::<Result<BTreeMap<_, _>, ValueError>>()?;
        PinVocabulary::new(NonEmptyVec::new(head, prefixes), normalize, mapping)
    }

    #[test]
    fn the_default_policy_demands_both() {
        let policy = GatePolicy::default();
        assert!(policy.cs_requires_pins() && policy.cs_requires_hypotheses());
        assert!(!policy.is_relaxed());
        assert!(GatePolicy::new(false, true).is_relaxed());
        assert!(GatePolicy::new(true, false).is_relaxed());
    }

    #[test]
    fn a_policy_is_told_from_another_in_its_canonical_form() {
        let forms: BTreeSet<Vec<u8>> = [(true, true), (false, true), (true, false), (false, false)]
            .into_iter()
            .map(|(pins, hyps)| GatePolicy::new(pins, hyps).to_canon().to_bytes())
            .collect();
        assert_eq!(forms.len(), 4);
    }

    #[test]
    fn the_default_vocabulary_is_the_old_grammar() {
        let vocabulary = PinVocabulary::default();
        assert_eq!(
            vocabulary.read("PIN-CACHE-TTL").unwrap().kind(),
            PinKind::Pin
        );
        assert_eq!(vocabulary.read("UT-A_1").unwrap().kind(), PinKind::UnitTest);
        for bad in [
            "", "PIN-", "pin-x", "PIN-a", "PIN X", "HYP-X", "UT-", "PINX", "REQ-X", "-X",
        ] {
            assert_eq!(
                vocabulary.read(bad),
                Err(ValueError::PinIdBadShape),
                "{bad}"
            );
        }
    }

    #[test]
    fn a_custom_prefix_is_accepted_and_the_others_still_are_not() {
        let vocabulary = vocabulary(
            &[("REQ", PinKind::Pin), ("UT", PinKind::UnitTest)],
            PinNormalize::Strict,
            &[],
        )
        .unwrap();
        let pin = vocabulary.read("REQ-LOGIN").unwrap();
        assert_eq!((pin.as_str(), pin.kind()), ("REQ-LOGIN", PinKind::Pin));
        assert!(vocabulary.read("UT-A").is_ok());
        assert_eq!(vocabulary.read("PIN-A"), Err(ValueError::PinIdBadShape));
    }

    #[test]
    fn upper_dash_normalizes_before_it_reads() {
        let vocabulary = vocabulary(
            &[("PIN", PinKind::Pin), ("UT", PinKind::UnitTest)],
            PinNormalize::UpperDash,
            &[],
        )
        .unwrap();
        assert_eq!(
            vocabulary.read("pin_login_cache").unwrap().as_str(),
            "PIN-LOGIN-CACHE"
        );
        assert_eq!(vocabulary.read("ut-a_b").unwrap().kind(), PinKind::UnitTest);
        assert_eq!(
            PinVocabulary::default().read("pin_login_cache"),
            Err(ValueError::PinIdBadShape),
            "strict reading refuses what upper-dash reads"
        );
        assert_eq!(vocabulary.read("req_x"), Err(ValueError::PinIdBadShape));
    }

    #[test]
    fn a_mapped_external_tag_becomes_its_pin() {
        let vocabulary = vocabulary(
            &[("PIN", PinKind::Pin), ("REQ", PinKind::Pin)],
            PinNormalize::Strict,
            &[("AUTH_LOGIN", "REQ-AUTH-LOGIN"), ("old", "REQ-OLD")],
        )
        .unwrap();
        assert_eq!(
            vocabulary.read("AUTH_LOGIN").unwrap().as_str(),
            "REQ-AUTH-LOGIN"
        );
        assert_eq!(vocabulary.read("old").unwrap().as_str(), "REQ-OLD");
        assert_eq!(
            vocabulary.read("AUTH_LOGOUT"),
            Err(ValueError::PinIdBadShape),
            "a tag that is not in the table is not a pin"
        );
    }

    #[test]
    fn a_tag_and_its_pin_written_out_are_one_pin_of_one_kind() {
        // The helper makes every table pin an invariant; `UT` makes unit-test tags.
        let vocabulary = vocabulary(
            &[("PIN", PinKind::Pin), ("UT", PinKind::UnitTest)],
            PinNormalize::Strict,
            &[("OLD_TAG", "UT-PARSE")],
        )
        .unwrap();
        let by_tag = vocabulary.read("OLD_TAG").unwrap();
        let written = vocabulary.read("UT-PARSE").unwrap();
        assert_eq!(by_tag.kind(), PinKind::UnitTest, "the kind of its prefix");
        assert_eq!(by_tag, written);
        assert_eq!(BTreeSet::from([by_tag, written]).len(), 1);
        // A name is one pin whatever kind it was read with.
        assert_eq!(
            PinId::from_parts("REQ-A".to_owned(), PinKind::Pin),
            PinId::from_parts("REQ-A".to_owned(), PinKind::UnitTest)
        );
    }

    #[test]
    fn a_vocabulary_that_contradicts_itself_is_refused() {
        assert_eq!(
            vocabulary(
                &[("PIN", PinKind::Pin), ("PIN", PinKind::UnitTest)],
                PinNormalize::Strict,
                &[]
            ),
            Err(ValueError::PinPrefixDuplicate {
                prefix: "PIN".to_owned()
            })
        );
        // The table names a pin of a prefix the vocabulary does not list.
        assert!(matches!(
            vocabulary(
                &[("PIN", PinKind::Pin)],
                PinNormalize::Strict,
                &[("TAG", "REQ-X")]
            ),
            Err(ValueError::PinMappingOutsideVocabulary { .. })
        ));
    }

    #[test]
    fn prefixes_and_tags_have_a_shape() {
        for bad in ["", "pin", "1X", "A-B", "A_B", "PÏN"] {
            assert_eq!(
                PinPrefix::new(bad, PinKind::Pin),
                Err(ValueError::PinPrefixBadShape),
                "{bad}"
            );
        }
        assert_eq!(
            PinPrefix::new("HYP", PinKind::Pin),
            Err(ValueError::PinPrefixReserved {
                prefix: "HYP".to_owned()
            })
        );
        assert!(PinPrefix::new("REQ2", PinKind::Pin).is_ok());
        for bad in ["", " ", "a b", "a\tb"] {
            assert_eq!(ExternalTag::new(bad), Err(ValueError::ExternalTagBadShape));
        }
        assert_eq!(
            ExternalTag::new("AUTH_LOGIN").unwrap().as_str(),
            "AUTH_LOGIN"
        );
    }
}
