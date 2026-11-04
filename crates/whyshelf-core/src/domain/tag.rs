//! Tags: the classification labels a tool puts on entities.
//!
//! Tags are written in one place only (the `tag` command, an event), never in a
//! file, and a `#tag` in a body is not picked up. Two groups:
//!
//! - an **official** tag is `key:value` (`area:auth`). The project's vocabulary
//!   (the `[tags.official]` table of the configuration) lists the allowed
//!   values of each key; a value outside it is refused by
//!   [`TagVocabulary::admit`];
//! - an **extended** tag is a free label without a colon (`wip-2026q4`).
//!
//! Changing the tags of an entity never changes its hash, so it never removes an
//! approval.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::str::FromStr;

use super::canon::{Canon, ToCanon};
use super::error::{QueryError, ValueError};

fn check_label(s: &str) -> Result<(), ValueError> {
    if s.is_empty() {
        return Err(ValueError::TagEmpty);
    }
    match s
        .chars()
        .find(|c| !(c.is_ascii_lowercase() || c.is_ascii_digit() || ".-_".contains(*c)))
    {
        Some(ch) => Err(ValueError::TagBadChar { ch }),
        None => Ok(()),
    }
}

/// An official tag, `key:value`.
///
/// Syntax is checked on construction; membership of the vocabulary is checked by
/// [`TagVocabulary::admit`].
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct OfficialTag {
    key: String,
    value: String,
}

impl OfficialTag {
    /// The group, for example `area`.
    pub fn key(&self) -> &str {
        &self.key
    }

    /// The value inside the group, for example `auth`.
    pub fn value(&self) -> &str {
        &self.value
    }
}

/// A free label without a colon.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ExtendedTag(String);

impl ExtendedTag {
    /// The label.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// A classification label, official or extended.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Tag {
    /// `key:value`, checked against the project's vocabulary before it is used.
    Official(OfficialTag),
    /// A free label.
    Extended(ExtendedTag),
}

impl Tag {
    /// Whether this tag is official.
    pub fn is_official(&self) -> bool {
        matches!(self, Self::Official(_))
    }
}

impl fmt::Display for Tag {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Official(t) => write!(f, "{}:{}", t.key, t.value),
            Self::Extended(t) => f.write_str(&t.0),
        }
    }
}

impl FromStr for Tag {
    type Err = ValueError;

    /// Syntax only: the first `:` makes the tag official. Whether an official
    /// tag is in the vocabulary is [`TagVocabulary::admit`]'s question.
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.split_once(':') {
            Some((key, value)) => {
                if key.is_empty() || value.is_empty() {
                    return Err(ValueError::TagBadOfficial);
                }
                check_label(key)?;
                check_label(value)?;
                Ok(Self::Official(OfficialTag {
                    key: key.to_owned(),
                    value: value.to_owned(),
                }))
            }
            None => {
                check_label(s)?;
                Ok(Self::Extended(ExtendedTag(s.to_owned())))
            }
        }
    }
}

impl ToCanon for Tag {
    fn to_canon(&self) -> Canon {
        Canon::str(self.to_string())
    }
}

/// One edit of an entity's tags: `+tag` adds, `-tag` removes.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum TagChange {
    /// Put the tag on.
    Add(Tag),
    /// Take the tag off.
    Remove(Tag),
}

impl TagChange {
    /// The tag this change is about.
    pub fn tag(&self) -> &Tag {
        match self {
            Self::Add(tag) | Self::Remove(tag) => tag,
        }
    }
}

impl FromStr for TagChange {
    type Err = ValueError;

    /// Parses `+area:auth` or `-wip`.
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if let Some(rest) = s.strip_prefix('+') {
            Ok(Self::Add(rest.parse()?))
        } else if let Some(rest) = s.strip_prefix('-') {
            Ok(Self::Remove(rest.parse()?))
        } else {
            Err(ValueError::TagChangeSign)
        }
    }
}

impl ToCanon for TagChange {
    fn to_canon(&self) -> Canon {
        let (op, tag) = match self {
            Self::Add(tag) => ("add", tag),
            Self::Remove(tag) => ("remove", tag),
        };
        Canon::map([("op", Canon::str(op)), ("tag", tag.to_canon())])
    }
}

/// Why a tag is not allowed by the vocabulary.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum VocabularyError {
    /// The key of an official tag is not defined.
    #[error("the official tag key `{key}` is not defined")]
    UnknownKey {
        /// The undefined key.
        key: String,
    },
    /// The value is not one of those defined for the key.
    #[error("`{value}` is not a defined value of `{key}`")]
    UnknownValue {
        /// The key.
        key: String,
        /// The value that is not defined for it.
        value: String,
    },
}

/// The official tags a project defines.
///
/// A tag that is official but not defined here cannot be put on an entity. Extended
/// tags are always allowed.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TagVocabulary {
    official: BTreeMap<String, BTreeSet<String>>,
}

impl TagVocabulary {
    /// A vocabulary from `key -> allowed values`.
    pub fn new(official: BTreeMap<String, BTreeSet<String>>) -> Self {
        Self { official }
    }

    /// Check a tag against the vocabulary.
    pub fn admit(&self, tag: &Tag) -> Result<(), VocabularyError> {
        let Tag::Official(official) = tag else {
            return Ok(());
        };
        match self.official.get(official.key()) {
            None => Err(VocabularyError::UnknownKey {
                key: official.key().to_owned(),
            }),
            Some(values) if values.contains(official.value()) => Ok(()),
            Some(_) => Err(VocabularyError::UnknownValue {
                key: official.key().to_owned(),
                value: official.value().to_owned(),
            }),
        }
    }
}

/// A tag condition in a query, parsed from the text after `tag:`.
pub(crate) fn query_tag(s: &str) -> Result<Tag, QueryError> {
    s.parse().map_err(QueryError::BadTag)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vocab() -> TagVocabulary {
        TagVocabulary::new(BTreeMap::from([(
            "area".to_owned(),
            BTreeSet::from(["auth".to_owned(), "build".to_owned()]),
        )]))
    }

    #[test]
    fn official_and_extended_tags_parse() {
        let official = "area:auth".parse::<Tag>().unwrap();
        assert!(official.is_official());
        assert_eq!(official.to_string(), "area:auth");
        let Tag::Official(parts) = &official else {
            panic!("expected an official tag");
        };
        assert_eq!((parts.key(), parts.value()), ("area", "auth"));
        let extended = "wip-2026q4".parse::<Tag>().unwrap();
        assert!(!extended.is_official());
        assert_eq!(extended.to_string(), "wip-2026q4");
    }

    #[test]
    fn malformed_tags_are_refused_by_reason() {
        assert_eq!("".parse::<Tag>(), Err(ValueError::TagEmpty));
        assert_eq!(":x".parse::<Tag>(), Err(ValueError::TagBadOfficial));
        assert_eq!("x:".parse::<Tag>(), Err(ValueError::TagBadOfficial));
        assert_eq!("A".parse::<Tag>(), Err(ValueError::TagBadChar { ch: 'A' }));
        assert_eq!(
            "a:b:c".parse::<Tag>(),
            Err(ValueError::TagBadChar { ch: ':' })
        );
        assert_eq!(
            "a b".parse::<Tag>(),
            Err(ValueError::TagBadChar { ch: ' ' })
        );
    }

    #[test]
    fn the_vocabulary_gates_official_tags_only() {
        let v = vocab();
        assert_eq!(v.admit(&"area:auth".parse().unwrap()), Ok(()));
        assert_eq!(v.admit(&"anything".parse().unwrap()), Ok(()));
        assert_eq!(
            v.admit(&"area:ledger".parse().unwrap()),
            Err(VocabularyError::UnknownValue {
                key: "area".into(),
                value: "ledger".into()
            })
        );
        assert_eq!(
            v.admit(&"team:core".parse().unwrap()),
            Err(VocabularyError::UnknownKey { key: "team".into() })
        );
    }

    #[test]
    fn tag_changes_need_a_sign() {
        assert_eq!(
            "+area:auth".parse::<TagChange>().unwrap(),
            TagChange::Add("area:auth".parse().unwrap())
        );
        assert_eq!(
            "-wip".parse::<TagChange>().unwrap(),
            TagChange::Remove("wip".parse().unwrap())
        );
        assert_eq!("wip".parse::<TagChange>(), Err(ValueError::TagChangeSign));
        assert_eq!("+".parse::<TagChange>(), Err(ValueError::TagEmpty));
    }

    #[test]
    fn add_and_remove_have_distinct_canonical_forms() {
        let t: Tag = "x".parse().unwrap();
        assert_ne!(
            TagChange::Add(t.clone()).to_canon(),
            TagChange::Remove(t).to_canon()
        );
    }
}
