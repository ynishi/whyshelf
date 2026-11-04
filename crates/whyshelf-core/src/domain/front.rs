//! What the front matter of a document file may hold, and what to do with each key.
//!
//! A document file is read as the kind it is registered as. Every key of its front
//! matter then falls in one of four places, and none is dropped without a word:
//!
//! | the key | is | becomes |
//! |---|---|---|
//! | one the kind reads (`finding_type`, `grounds`, `crux`, ...) | content | the entity, and its hash |
//! | `tags`, `kind`, `owner`, `alias` | the file's [`FileClassification`] | imported once at registration, then compared ([`ClassificationImport`]) |
//! | one that another kind reads | a [`ForeignSection`] | `UnparsedReason::ForeignSection`: the file is Invalid |
//! | none of these | an [`UnknownKey`] | `UnparsedReason::UnknownKey`: the file is Invalid |
//!
//! A hash covers the keys a kind reads and nothing else, so changing a classification
//! key, or the text of a section the kind does not read, cannot remove an approval
//! (see [`hash`](super::hash)). A key that a kind reads and the file lacks is the
//! parser's `MissingField`, not this module's.
//!
//! **Classification lives in the events.** The tool never rewrites a file, so a file's
//! classification keys are a way to *start* a classification (a note written before
//! the document was registered), not a second place to keep it. At registration they
//! are taken in once. After that the store is the truth, and a file that says
//! something else is shown as a [`ClassDrift`] in the status, for a person to decide
//! which is right.
//!
//! **A file that has no registered kind yet** (a candidate found in a watched
//! directory) is told by [`DocKind::guess`], which looks at the one key that names each
//! kind. Only a single answer registers anything; two or none ask the person for a
//! kind.

use std::collections::BTreeSet;
use std::fmt;

use super::actor::Actor;
use super::classification::{DocClass, KindState};
use super::event::DocKind;
use super::id::Alias;
use super::state::UnparsedReason;
use super::tag::Tag;

/// The keys that classify a document instead of being part of it.
pub const CLASSIFICATION_KEYS: [&str; 4] = ["tags", "kind", "owner", "alias"];

impl DocKind {
    /// Every key this kind of document reads.
    pub fn keys(self) -> &'static [&'static str] {
        match self {
            Self::Finding => &[
                "finding_type",
                "title",
                "evidence",
                "measured_at",
                "revision",
                "environment",
                "context",
                "reproducibility",
                "source",
                "version",
                "maturity",
                "ref",
                "method",
                "authority",
                "scope",
                "review_condition",
            ],
            Self::Spec => &["grounds", "accept"],
            Self::Design => &["spec", "crux", "items"],
        }
    }

    /// The one key that names the kind: a file that has it is of that kind.
    fn naming_key(self) -> &'static str {
        match self {
            Self::Finding => "finding_type",
            Self::Spec => "grounds",
            Self::Design => "spec",
        }
    }

    /// Which kind a file with these keys is, when nothing says.
    ///
    /// For a file that is not registered yet. Once it is, the registered kind reads
    /// it, whatever its keys say.
    pub fn guess<'a>(keys: impl IntoIterator<Item = &'a str>) -> Guess {
        let keys: BTreeSet<&str> = keys.into_iter().collect();
        let mut named = [Self::Finding, Self::Spec, Self::Design]
            .into_iter()
            .filter(|kind| keys.contains(kind.naming_key()));
        match (named.next(), named.next()) {
            (Some(kind), None) => Guess::One(kind),
            (Some(_), Some(_)) => Guess::Several,
            (None, _) => Guess::None,
        }
    }
}

/// What [`DocKind::guess`] could tell.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Guess {
    /// Exactly one kind fits.
    One(DocKind),
    /// More than one fits.
    Several,
    /// None fits.
    #[default]
    None,
}

/// A key of a document that another kind reads.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ForeignSection {
    kind: DocKind,
    key: String,
}

impl ForeignSection {
    /// The key, and the kind that reads it.
    pub fn new(kind: DocKind, key: impl Into<String>) -> Self {
        Self {
            kind,
            key: key.into(),
        }
    }

    /// The kind that reads the key.
    pub fn kind(&self) -> DocKind {
        self.kind
    }

    /// The key.
    pub fn key(&self) -> &str {
        &self.key
    }
}

/// A key that no kind reads and that classifies nothing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnknownKey {
    key: String,
}

impl UnknownKey {
    /// The key.
    pub fn new(key: impl Into<String>) -> Self {
        Self { key: key.into() }
    }

    /// The key.
    pub fn key(&self) -> &str {
        &self.key
    }
}

/// The keys of a front matter, sorted into what they are for a registered kind.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SortedKeys {
    foreign: Vec<ForeignSection>,
    unknown: Vec<UnknownKey>,
}

impl SortedKeys {
    /// Sort the keys of a file that is read as `registered`. The order of the keys is
    /// kept in what is returned.
    pub fn of<'a>(registered: DocKind, keys: impl IntoIterator<Item = &'a str>) -> Self {
        let mut sorted = Self::default();
        for key in keys {
            if registered.keys().contains(&key) || CLASSIFICATION_KEYS.contains(&key) {
                continue;
            }
            let owner = [DocKind::Finding, DocKind::Spec, DocKind::Design]
                .into_iter()
                .find(|kind| kind.keys().contains(&key));
            match owner {
                Some(kind) => sorted.foreign.push(ForeignSection::new(kind, key)),
                None => sorted.unknown.push(UnknownKey::new(key)),
            }
        }
        sorted
    }

    /// The keys of another kind.
    pub fn foreign(&self) -> &[ForeignSection] {
        &self.foreign
    }

    /// The keys of no kind.
    pub fn unknown(&self) -> &[UnknownKey] {
        &self.unknown
    }

    /// Each as the reason the file cannot be an entity.
    pub fn reasons(&self, registered: DocKind) -> Vec<UnparsedReason> {
        let foreign = self
            .foreign
            .iter()
            .map(|section| UnparsedReason::ForeignSection {
                section: section.kind,
                registered,
                key: section.key.clone(),
            });
        let unknown = self
            .unknown
            .iter()
            .map(|unknown| UnparsedReason::UnknownKey {
                key: unknown.key.clone(),
            });
        foreign.chain(unknown).collect()
    }
}

/// The classification a file carries in its front matter. A key the file does not
/// have is `None`: it says nothing, which is not the same as saying "none".
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FileClassification {
    /// `tags`.
    pub tags: Option<BTreeSet<Tag>>,
    /// `kind`.
    pub kind: Option<DocKind>,
    /// `owner`.
    pub owner: Option<Actor>,
    /// `alias`.
    pub alias: Option<Alias>,
}

impl FileClassification {
    /// Whether the file says nothing about its classification.
    pub fn is_empty(&self) -> bool {
        self.tags.is_none() && self.kind.is_none() && self.owner.is_none() && self.alias.is_none()
    }
}

/// A value of the classification, by name.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ClassField {
    /// The tags.
    Tags,
    /// The kind.
    Kind,
    /// The owner.
    Owner,
    /// The alias.
    Alias,
}

impl fmt::Display for ClassField {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Tags => "tags",
            Self::Kind => "kind",
            Self::Owner => "owner",
            Self::Alias => "alias",
        })
    }
}

/// A value the file says and the store does not agree with.
///
/// Both sides are kept as the text a person reads, so that the status can show them
/// side by side.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClassDrift {
    /// Which value.
    pub field: ClassField,
    /// What the file says.
    pub file: String,
    /// What the store has.
    pub store: String,
}

/// What a file's classification comes to.
#[derive(Debug, Clone, Copy)]
pub enum Stored<'a> {
    /// The file is being registered, as this kind. Nothing is stored yet.
    New {
        /// The kind it is registered as.
        kind: DocKind,
    },
    /// The file belongs to a registered document, whose classification is this.
    Registered(&'a DocClass),
}

/// The decision about a file's classification keys: what to take in, what to warn
/// about.
///
/// Taking in happens **once**, at registration; against a registered document there
/// is nothing to take in, only the differences to show. Whatever the registration
/// could not take in (an alias another document holds, a tag the project's
/// vocabulary does not admit) is no different from any other disagreement: the next
/// load shows it as a drift.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ClassificationImport {
    tags: BTreeSet<Tag>,
    owner: Option<Actor>,
    alias: Option<Alias>,
    drift: Vec<ClassDrift>,
}

impl ClassificationImport {
    /// Decide, for a file's keys and the classification it meets.
    pub fn of(file: &FileClassification, stored: Stored<'_>) -> Self {
        match stored {
            Stored::New { kind } => Self {
                tags: file.tags.clone().unwrap_or_default(),
                owner: file.owner.clone(),
                alias: file.alias.clone(),
                drift: match file.kind {
                    Some(said) if said != kind => vec![ClassDrift {
                        field: ClassField::Kind,
                        file: said.to_string(),
                        store: kind.to_string(),
                    }],
                    _ => Vec::new(),
                },
            },
            Stored::Registered(class) => Self {
                drift: drift_against(file, class),
                ..Self::default()
            },
        }
    }

    /// The tags to take in.
    pub fn tags(&self) -> &BTreeSet<Tag> {
        &self.tags
    }

    /// The owner to take in.
    pub fn owner(&self) -> Option<&Actor> {
        self.owner.as_ref()
    }

    /// The alias to take in.
    pub fn alias(&self) -> Option<&Alias> {
        self.alias.as_ref()
    }

    /// What disagrees, to warn about.
    pub fn drift(&self) -> &[ClassDrift] {
        &self.drift
    }
}

fn list<T: fmt::Display>(items: impl IntoIterator<Item = T>) -> String {
    let items: Vec<String> = items.into_iter().map(|item| item.to_string()).collect();
    if items.is_empty() {
        "none".to_owned()
    } else {
        items.join(", ")
    }
}

fn drift_against(file: &FileClassification, class: &DocClass) -> Vec<ClassDrift> {
    let mut drift = Vec::new();
    if let (Some(said), KindState::Known(has)) = (file.kind, class.kind())
        && said != *has
    {
        drift.push(ClassDrift {
            field: ClassField::Kind,
            file: said.to_string(),
            store: has.to_string(),
        });
    }
    if let Some(said) = &file.tags
        && said != class.tags()
    {
        drift.push(ClassDrift {
            field: ClassField::Tags,
            file: list(said),
            store: list(class.tags()),
        });
    }
    if let (Some(said), Some(has)) = (&file.owner, class.owner().agreed())
        && has.as_ref() != Some(said)
    {
        drift.push(ClassDrift {
            field: ClassField::Owner,
            file: said.to_string(),
            store: has
                .as_ref()
                .map_or_else(|| "none".to_owned(), ToString::to_string),
        });
    }
    if let (Some(said), Some(has)) = (&file.alias, class.alias().agreed())
        && has.as_ref() != Some(said)
    {
        drift.push(ClassDrift {
            field: ClassField::Alias,
            file: said.to_string(),
            store: has
                .as_ref()
                .map_or_else(|| "none".to_owned(), ToString::to_string),
        });
    }
    drift
}

#[cfg(test)]
mod tests {
    use super::*;

    fn keys(text: &str) -> Vec<&str> {
        text.split_whitespace().collect()
    }

    #[test]
    fn a_guess_needs_the_one_key_that_names_a_kind() {
        assert_eq!(
            DocKind::guess(keys("finding_type title evidence")),
            Guess::One(DocKind::Finding)
        );
        assert_eq!(
            DocKind::guess(keys("grounds accept")),
            Guess::One(DocKind::Spec)
        );
        assert_eq!(
            DocKind::guess(keys("spec crux items")),
            Guess::One(DocKind::Design)
        );
        assert_eq!(DocKind::guess(keys("grounds spec")), Guess::Several);
        assert_eq!(DocKind::guess(keys("finding_type spec")), Guess::Several);
        assert_eq!(DocKind::guess(keys("title tags")), Guess::None);
        assert_eq!(DocKind::guess(keys("")), Guess::None);
    }

    #[test]
    fn a_key_is_content_classification_foreign_or_unknown() {
        let sorted = SortedKeys::of(
            DocKind::Spec,
            keys("grounds accept tags owner crux items typo_key"),
        );
        assert_eq!(
            sorted.foreign(),
            [
                ForeignSection::new(DocKind::Design, "crux"),
                ForeignSection::new(DocKind::Design, "items"),
            ]
        );
        assert_eq!(sorted.unknown(), [UnknownKey::new("typo_key")]);
        assert_eq!(
            sorted.reasons(DocKind::Spec),
            vec![
                UnparsedReason::ForeignSection {
                    section: DocKind::Design,
                    registered: DocKind::Spec,
                    key: "crux".to_owned()
                },
                UnparsedReason::ForeignSection {
                    section: DocKind::Design,
                    registered: DocKind::Spec,
                    key: "items".to_owned()
                },
                UnparsedReason::UnknownKey {
                    key: "typo_key".to_owned()
                },
            ]
        );
        // The keys a kind reads, and the classification keys, are nothing to report.
        assert!(
            SortedKeys::of(
                DocKind::Design,
                keys("spec crux items tags kind owner alias")
            )
            .reasons(DocKind::Design)
            .is_empty()
        );
    }

    #[test]
    fn every_key_of_a_kind_belongs_to_that_kind_alone() {
        let kinds = [DocKind::Finding, DocKind::Spec, DocKind::Design];
        for kind in kinds {
            assert!(kind.keys().contains(&kind.naming_key()));
            for key in kind.keys() {
                assert!(!CLASSIFICATION_KEYS.contains(key), "{key}");
                for other in kinds.into_iter().filter(|other| *other != kind) {
                    assert!(!other.keys().contains(key), "{key}");
                }
            }
        }
    }

    #[test]
    fn a_new_document_takes_in_what_its_file_says_and_flags_a_kind_it_contradicts() {
        let file = FileClassification {
            tags: Some(BTreeSet::from(["area:auth".parse().unwrap()])),
            kind: Some(DocKind::Spec),
            owner: Some(Actor::new("alice").unwrap()),
            alias: Some("finding/slow".parse().unwrap()),
        };
        let import = ClassificationImport::of(
            &file,
            Stored::New {
                kind: DocKind::Finding,
            },
        );
        assert_eq!(import.tags().len(), 1);
        assert_eq!(import.owner().map(Actor::as_str), Some("alice"));
        assert_eq!(
            import.alias().map(ToString::to_string).as_deref(),
            Some("finding/slow")
        );
        assert_eq!(import.drift().len(), 1);
        assert_eq!(import.drift()[0].field, ClassField::Kind);

        let silent = ClassificationImport::of(
            &FileClassification::default(),
            Stored::New {
                kind: DocKind::Finding,
            },
        );
        assert!(silent.tags().is_empty() && silent.drift().is_empty());
    }
}
