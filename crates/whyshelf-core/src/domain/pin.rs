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
//! The tests are written in the implementation stage together with the code, and
//! the approval of a work item (gate CA) checks both directions: every PIN assigned
//! to the item exists in the tests, and no tag in the tests is undeclared.

use std::fmt;
use std::str::FromStr;

use super::canon::{Canon, ToCanon};
use super::error::ValueError;
use super::nonempty::NonBlank;

fn is_name_char(c: char) -> bool {
    c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_' || c == '-'
}

fn has_name_after(s: &str, prefix: &str) -> bool {
    s.strip_prefix(prefix)
        .is_some_and(|rest| !rest.is_empty() && rest.chars().all(is_name_char))
}

/// What a [`PinId`] binds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum PinKind {
    /// An invariant of the code (`PIN-*`).
    Pin,
    /// A unit-test tag (`UT-*`).
    UnitTest,
}

/// The name of a PIN or unit-test tag: `PIN-<NAME>` or `UT-<NAME>`.
///
/// `<NAME>` is upper-case letters, digits, `_` and `-`. This is the same shape
/// the code scan looks for after `@pin:`, so a name that parses here can be found
/// in a test.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PinId(String);

impl PinId {
    /// The name as text.
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Whether this is a `PIN-*` or a `UT-*` name.
    pub fn kind(&self) -> PinKind {
        if self.0.starts_with("PIN-") {
            PinKind::Pin
        } else {
            PinKind::UnitTest
        }
    }
}

impl fmt::Display for PinId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl FromStr for PinId {
    type Err = ValueError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if has_name_after(s, "PIN-") || has_name_after(s, "UT-") {
            Ok(Self(s.to_owned()))
        } else {
            Err(ValueError::PinIdBadShape)
        }
    }
}

impl ToCanon for PinId {
    fn to_canon(&self) -> Canon {
        Canon::str(self.0.as_str())
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
