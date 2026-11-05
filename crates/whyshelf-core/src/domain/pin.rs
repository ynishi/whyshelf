//! Acceptance declarations of a Spec: PINs, unit-test tags and hypotheses.
//!
//! A Spec does not contain test code. It declares *what* will bind the contract:
//!
//! - a **PIN** (`PIN-*`) is an invariant the code must keep, carried by a test
//!   that has the tag `@pin: PIN-*` in a comment;
//! - a **UT** tag (`UT-*`) is the same mechanism for an ordinary unit test;
//! - a **HYP** (`HYP-*`) is a hypothesis registered in advance about which metric
//!   moves and how, fixed by hash when the Spec is approved and measured later
//!   (gate CM).
//!
//! `PIN` and `UT` are the default prefixes of pins; a project's own are in its
//! [`PinVocabulary`], which also says how a mark in code is read.
//!
//! The tests are written in the implementation stage together with the code, and
//! the approval of a work item (gate CA) checks both directions: every PIN assigned
//! to the item exists in the tests, and no tag in the tests is undeclared.

use std::fmt;
use std::str::FromStr;

use super::canon::{Canon, ToCanon};
use super::error::ValueError;
use super::location::CodeLocation;
use super::nonempty::NonBlank;
use super::policy::PinVocabulary;

fn is_name_char(c: char) -> bool {
    c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_' || c == '-'
}

/// Whether a text is a name: at least one character, all of them name characters.
pub(super) fn is_name(s: &str) -> bool {
    !s.is_empty() && s.chars().all(is_name_char)
}

fn has_name_after(s: &str, prefix: &str) -> bool {
    s.strip_prefix(prefix).is_some_and(is_name)
}

/// What a [`PinId`] binds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum PinKind {
    /// An invariant of the code (`PIN-*`).
    Pin,
    /// A unit-test tag (`UT-*`).
    UnitTest,
}

/// The name of a PIN or unit-test tag: `<PREFIX>-<NAME>`, by default `PIN-<NAME>` or
/// `UT-<NAME>`.
///
/// `<PREFIX>` is one of a [`PinVocabulary`]; `<NAME>` is upper-case letters, digits,
/// `_` and `-`. A name is made by reading a mark in a vocabulary
/// ([`parse_in`](Self::parse_in)), or in the default one (`FromStr`), and equality
/// (with order and hash) is that of the name read alone: the kind follows from the
/// prefix, so two pins of one name are one pin, whichever way each was read.
#[derive(Debug, Clone)]
pub struct PinId {
    text: String,
    kind: PinKind,
}

impl PartialEq for PinId {
    fn eq(&self, other: &Self) -> bool {
        self.text == other.text
    }
}

impl Eq for PinId {}

impl PartialOrd for PinId {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for PinId {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.text.cmp(&other.text)
    }
}

impl std::hash::Hash for PinId {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.text.hash(state);
    }
}

impl PinId {
    /// The pin a mark is in `vocabulary`: the table first, then the normalized
    /// mark against the prefixes (see [`policy`](super::policy)).
    pub fn parse_in(vocabulary: &PinVocabulary, mark: &str) -> Result<Self, ValueError> {
        vocabulary.read(mark)
    }

    /// A pin from a text the vocabulary has checked and the kind of its prefix.
    pub(super) fn from_parts(text: String, kind: PinKind) -> Self {
        Self { text, kind }
    }

    /// The name as text.
    pub fn as_str(&self) -> &str {
        &self.text
    }

    /// What the pin is, by its prefix: an invariant (`PIN-*` by default) or a
    /// unit-test tag (`UT-*`).
    pub fn kind(&self) -> PinKind {
        self.kind
    }
}

impl fmt::Display for PinId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.text)
    }
}

impl FromStr for PinId {
    type Err = ValueError;

    /// The name in the default vocabulary: `PIN-` or `UT-` and a name, as written.
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::parse_in(&PinVocabulary::default(), s)
    }
}

impl ToCanon for PinId {
    fn to_canon(&self) -> Canon {
        Canon::str(self.text.as_str())
    }
}

/// A pin tag found in a test whose name is not a [`PinId`]: `@pin: PIN-foo`.
///
/// The scan does not drop it. A tag that was meant as a pin and does not count is the
/// kind of mistake that makes a check pass for the wrong reason, so it is carried to
/// the gate, which reports it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MalformedPin {
    location: CodeLocation,
    text: NonBlank,
}

impl MalformedPin {
    /// The tag at a place, with the text it named.
    pub fn new(location: CodeLocation, text: NonBlank) -> Self {
        Self { location, text }
    }

    /// Where the tag was found.
    pub fn location(&self) -> &CodeLocation {
        &self.location
    }

    /// The text it named.
    pub fn text(&self) -> &NonBlank {
        &self.text
    }
}

/// The name of a hypothesis: `HYP-<NAME>`.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct HypId(String);

impl HypId {
    /// The name as text.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for HypId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl FromStr for HypId {
    type Err = ValueError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if has_name_after(s, "HYP-") {
            Ok(Self(s.to_owned()))
        } else {
            Err(ValueError::HypIdBadShape)
        }
    }
}

impl ToCanon for HypId {
    fn to_canon(&self) -> Canon {
        Canon::str(self.0.as_str())
    }
}

/// The sentence a PIN guarantees.
///
/// The map of a Spec is `BTreeMap<PinId, PinContract>`: the identifier is the
/// key and is not repeated in the value.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PinContract(NonBlank);

impl PinContract {
    /// A contract sentence; blank is refused.
    pub fn new(sentence: impl Into<String>) -> Result<Self, ValueError> {
        NonBlank::new(sentence).map(Self)
    }

    /// The sentence.
    pub fn as_str(&self) -> &str {
        self.0.as_str()
    }
}

impl ToCanon for PinContract {
    fn to_canon(&self) -> Canon {
        self.0.to_canon()
    }
}

/// A hypothesis registered before the work: which metric moves, and to what.
///
/// Like [`PinContract`], the identifier is the map key and not part of the value.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Hypothesis {
    metric: NonBlank,
    target: NonBlank,
}

impl Hypothesis {
    /// A hypothesis from a metric and the target value; blank parts are refused.
    pub fn new(metric: impl Into<String>, target: impl Into<String>) -> Result<Self, ValueError> {
        Ok(Self {
            metric: NonBlank::new(metric)?,
            target: NonBlank::new(target)?,
        })
    }

    /// The metric that is measured.
    pub fn metric(&self) -> &str {
        self.metric.as_str()
    }

    /// The value the metric should reach.
    pub fn target(&self) -> &str {
        self.target.as_str()
    }
}

impl ToCanon for Hypothesis {
    fn to_canon(&self) -> Canon {
        Canon::map([
            ("metric", self.metric.to_canon()),
            ("target", self.target.to_canon()),
        ])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pin_ids_follow_the_documented_shape() {
        assert_eq!(
            "PIN-CACHE-TTL".parse::<PinId>().unwrap().kind(),
            PinKind::Pin
        );
        assert_eq!("UT-A_1".parse::<PinId>().unwrap().kind(), PinKind::UnitTest);
        for bad in [
            "", "PIN-", "pin-x", "PIN-a", "PIN X", "HYP-X", "UT-", "PINX",
        ] {
            assert_eq!(
                bad.parse::<PinId>(),
                Err(ValueError::PinIdBadShape),
                "{bad}"
            );
        }
    }

    #[test]
    fn hyp_ids_follow_the_documented_shape() {
        assert!("HYP-REBUILD-ZERO".parse::<HypId>().is_ok());
        for bad in ["", "HYP-", "HYP-a", "PIN-X"] {
            assert_eq!(
                bad.parse::<HypId>(),
                Err(ValueError::HypIdBadShape),
                "{bad}"
            );
        }
    }

    #[test]
    fn contracts_and_hypotheses_need_content() {
        assert_eq!(PinContract::new(" "), Err(ValueError::BlankText));
        assert_eq!(Hypothesis::new("", "0"), Err(ValueError::BlankText));
        assert_eq!(Hypothesis::new("m", " "), Err(ValueError::BlankText));
        let h = Hypothesis::new("reruns", "0").unwrap();
        assert_eq!((h.metric(), h.target()), ("reruns", "0"));
    }
}
