//! The names of a space a selection takes, as its adapter lists them, and what the
//! rules of coverage read of that: the values of an ask that an adapter may answer
//! (the ask `NameList` of the contract in the application).
//!
//! A [`Selection`] says which names of a space are expected to be reached: opaque to
//! the domain, which keeps the texts and never reads them (the grammar is the
//! space's, and only its adapter reads one). What the adapter answers is a
//! [`SelectionCheck`] (can the space read this selection at all) and a
//! [`NameListing`] (the names that are there now and that the selection takes). A
//! listing that is not the whole answer is never counted as one: the rules read the
//! answers through [`ListedNames::of`] alone, which says how complete each is
//! ([`Completeness`]).
//!
//! | value | what it is | who makes it |
//! |---|---|---|
//! | [`Selection`] | the texts to include (at least one) and to exclude, as a link declares them | a declaration of a link ([`ArtifactCover`](super::super::schema::ArtifactCover)) |
//! | [`SelectionCheck`] | whether the space reads the selection: yes, not a selection of it (why), or the space lists nothing (why) | the adapter, without I/O |
//! | [`NameListing`] | the names the selection takes that are there now: all of them, some of them (why), all but the parts it could not list whole (each a [`ListingPart`]: its key, why, and the names found in it or that it could not be read, [`PartRead`]), or none can be listed (why) | the adapter |
//! | [`Listings`] | what was asked, by (space, selection): nothing, or each answer | the application, for the commands that read an artifact cover |
//! | [`Completeness`] | whether a listing is the whole: all, partial (why), cannot list (why), or not asked | [`ListedNames::of`] |
//! | [`ListedNames`] | for each (space, selection), its completeness and, when all, its names; when in parts, its names and its parts, for a cover that takes each part whole | [`ListedNames::of`], the only reader of the variants of [`NameListing`] |
//!
//! ```text
//!   adapter ──select──► SelectionCheck ──┐
//!           ──list────► NameListing ─────┼─► Listings ──ListedNames::of──► ListedNames
//!                                        │    (space, selection) ->            ├─ completeness
//!   a command that does not ask ─────────┘     NameListing | NotAsked          └─ names, when all
//! ```
//!
//! The key is (space, selection), not a link: two links that declare the same
//! selection in one space share one listing, and the same answer.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use thiserror::Error;

use crate::domain::nonempty::{NonBlank, NonEmptyVec, TextLiteral};

use super::{ArtifactKey, ArtifactName, Space};

/// Which list of a [`Selection`] a text is in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SelectionList {
    /// The texts that take names.
    Include,
    /// The texts that leave names out.
    Exclude,
}

impl fmt::Display for SelectionList {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Include => "include",
            Self::Exclude => "exclude",
        })
    }
}

/// Why a selection cannot be made.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum SelectionProblem {
    /// No text to include: a selection takes nothing it does not name.
    #[error("a selection includes at least one text")]
    NoInclude,
    /// A text of a list is empty.
    #[error("the text at {at} of `{list}` is empty")]
    EmptyText {
        /// The list.
        list: SelectionList,
        /// The place of the text in the list, counted from 0.
        at: usize,
    },
}

/// The names of a space a link expects to be reached, as it declares them: the texts
/// to include (at least one) and to exclude.
///
/// Opaque: the domain checks only that a text is not empty and that one is to be
/// included, and compares the texts as written. What a text means is the grammar of
/// the space, read by its adapter ([`SelectionCheck`]).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Selection {
    include: Vec<String>,
    exclude: Vec<String>,
}

impl Selection {
    /// A selection of these texts, refused when none is to be included or a text is
    /// empty.
    pub fn new(include: Vec<String>, exclude: Vec<String>) -> Result<Self, SelectionProblem> {
        if include.is_empty() {
            return Err(SelectionProblem::NoInclude);
        }
        for (list, texts) in [
            (SelectionList::Include, &include),
            (SelectionList::Exclude, &exclude),
        ] {
            if let Some(at) = texts.iter().position(String::is_empty) {
                return Err(SelectionProblem::EmptyText { list, at });
            }
        }
        Ok(Self { include, exclude })
    }

    /// The texts to include, as written; never none.
    pub fn include(&self) -> &[String] {
        &self.include
    }

    /// The texts to leave out, as written.
    pub fn exclude(&self) -> &[String] {
        &self.exclude
    }
}

/// A selection that is not one of the space: the adapter cannot read a text of it.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[error("{why}")]
pub struct NotASelection {
    why: NonBlank,
}

impl NotASelection {
    /// Not a selection of the space, for this reason.
    pub fn new(why: NonBlank) -> Self {
        Self { why }
    }

    /// Why.
    pub fn why(&self) -> &NonBlank {
        &self.why
    }
}

/// What a space's adapter says of a [`Selection`], with no I/O: whether it reads it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SelectionCheck {
    /// The adapter reads the selection.
    Ok,
    /// A text of it is none of the space's grammar, with why.
    NotASelection(NonBlank),
    /// The space lists no names: no selection of it is read, with why.
    CannotList(NonBlank),
}

impl From<NotASelection> for SelectionCheck {
    fn from(not: NotASelection) -> Self {
        Self::NotASelection(not.why)
    }
}

/// What the adapters said of each selection asked about, by (space, selection): the
/// answers [`Schema::declared`](super::super::schema::Schema::declared) reads a
/// selection of a `cover` against. A selection that has no answer is read as
/// [`SelectionCheck::CannotList`], so that it is refused.
pub type SelectionChecks = BTreeMap<(Space, Selection), SelectionCheck>;

/// What a space's adapter lists of the names a [`Selection`] takes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NameListing {
    /// Every name there is now that the selection takes.
    All(BTreeSet<ArtifactName>),
    /// Some of them, and others may be there: the listing was cut at a bound, with
    /// why. It is never counted as all of them.
    Some {
        /// The names found.
        found: BTreeSet<ArtifactName>,
        /// Why it stopped.
        why: NonBlank,
    },
    /// Every name of what the adapter could list whole (`found`), and the parts it
    /// could not, each told apart ([`ListingPart`]): a part may hold names the adapter
    /// does not see, so the listing is never counted as all of them. Only a cover that
    /// takes each part whole, through the one link it declares for parts, reads past
    /// that ([`ArtifactCover::parts_whole`](super::super::schema::ArtifactCover::parts_whole));
    /// to any other, it is a listing that is not the whole. What keeps the whole
    /// selection from being listed (a bound, a listing of the state left short) is
    /// [`Some`](Self::Some), never parts.
    Parted {
        /// The names of what was listed whole.
        found: BTreeSet<ArtifactName>,
        /// The parts that could not be, each once, in the adapter's order.
        parts: NonEmptyVec<ListingPart>,
    },
    /// No listing could be made (the space lists nothing, or nothing here to list
    /// from), with why.
    CannotList(NonBlank),
}

/// A part of what a selection takes that its adapter could not list whole: the
/// artifact the part is (its key, in a space the adapter names in its own words, opaque
/// here as every name is), why it is not whole, and what was read of it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ListingPart {
    key: ArtifactKey,
    why: NonBlank,
    read: PartRead,
}

impl ListingPart {
    /// The part `key`, not listed whole for `why`, of which `read` was read.
    pub fn new(key: ArtifactKey, why: NonBlank, read: PartRead) -> Self {
        Self { key, why, read }
    }

    /// The artifact the part is, as a whole.
    pub fn key(&self) -> &ArtifactKey {
        &self.key
    }

    /// Why it is not listed whole.
    pub fn why(&self) -> &NonBlank {
        &self.why
    }

    /// What was read of it.
    pub fn read(&self) -> &PartRead {
        &self.read
    }
}

/// What an adapter read of a part it could not list whole.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PartRead {
    /// It was read, and these names were found in it (none, it may be), beside what
    /// may make others the adapter does not see.
    Found(BTreeSet<ArtifactName>),
    /// It could not be read at all.
    Unread,
}

/// What was asked of the adapters for a command that reads an artifact cover: the
/// answer for each (space, selection), or that nothing was asked.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Listings {
    /// The command does not read an artifact cover, and asked no adapter. Not the
    /// same as a space that cannot list: nothing was asked.
    NotAsked,
    /// The answers, each for a (space, selection) asked.
    Asked(BTreeMap<(Space, Selection), NameListing>),
}

/// Whether a listing is the whole of what is there.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Completeness {
    /// Every name the selection takes is in it.
    All,
    /// Some names are in it, and others may be missing, with why.
    Partial(NonBlank),
    /// The space could not be listed, with why.
    CannotList(NonBlank),
    /// Nothing was asked for this selection.
    NotAsked,
}

impl Completeness {
    /// Whether the listing is the whole.
    pub fn is_all(&self) -> bool {
        matches!(self, Self::All)
    }
}

impl fmt::Display for Completeness {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::All => f.write_str("every name is listed"),
            Self::Partial(why) => write!(f, "the names listed are not all there are: {why}"),
            Self::CannotList(why) => write!(f, "the names cannot be listed: {why}"),
            Self::NotAsked => f.write_str("the names were not asked for"),
        }
    }
}

/// The listings of a command, read: each (space, selection) with how complete its
/// listing is and, when it is the whole, its names.
///
/// [`ListedNames::of`] is the one reader of the variants of [`NameListing`]: the rule
/// of coverage and the reading of an exemption are given this value, and never match
/// a listing themselves.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ListedNames {
    of: BTreeMap<(Space, Selection), Read>,
}

/// One listing, read: how complete it is, the names it holds whole (all of them, or
/// those beside its parts), and its parts.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Read {
    completeness: Completeness,
    names: BTreeSet<ArtifactName>,
    parts: Vec<ListingPart>,
}

/// How many parts the reason of a listing in parts names, in the order the adapter
/// gave them; the others are counted, so that the reason stays small whatever a
/// selection takes.
const PARTS_SHOWN: usize = 5;

/// What is said of a listing in parts when what says it cannot be made (it never
/// cannot: it starts with words).
const IN_PARTS: TextLiteral = TextLiteral::new("parts of it are not listed whole");

/// Why a listing in parts is not the whole, as a cover that does not take its parts
/// whole is told it: the first few parts, each with why, and how many more.
fn parts_why(parts: &[ListingPart]) -> NonBlank {
    let shown: Vec<String> = parts
        .iter()
        .take(PARTS_SHOWN)
        .map(|part| format!("{} ({})", part.key(), part.why()))
        .collect();
    let more = parts.len().saturating_sub(PARTS_SHOWN);
    let mut said = format!("parts of it are not listed whole: {}", shown.join("; "));
    if more > 0 {
        said.push_str(&format!("; and {more} more part(s)"));
    }
    NonBlank::new(said).unwrap_or_else(|_| NonBlank::from(IN_PARTS))
}

impl ListedNames {
    /// The listings read: a selection that was asked is as complete as its answer
    /// says, and one that was not (or nothing was) is [`Completeness::NotAsked`]. A
    /// listing in parts ([`NameListing::Parted`]) is [`Completeness::Partial`], with
    /// its parts named in the reason, and keeps its names and its parts for a cover
    /// that takes each part whole ([`names_and_parts`](Self::names_and_parts)).
    pub fn of(listings: &Listings) -> Self {
        let of = match listings {
            Listings::NotAsked => BTreeMap::new(),
            Listings::Asked(asked) => asked
                .iter()
                .map(|(key, listing)| {
                    let (completeness, names, parts) = match listing {
                        NameListing::All(names) => (Completeness::All, names.clone(), Vec::new()),
                        NameListing::Some { why, .. } => (
                            Completeness::Partial(why.clone()),
                            BTreeSet::new(),
                            Vec::new(),
                        ),
                        NameListing::Parted { found, parts } => {
                            // Each part once, the first an adapter gave of its key.
                            let mut seen = BTreeSet::new();
                            let parts: Vec<ListingPart> = parts
                                .iter()
                                .filter(|part| seen.insert(part.key().clone()))
                                .cloned()
                                .collect();
                            (
                                Completeness::Partial(parts_why(&parts)),
                                found.clone(),
                                parts,
                            )
                        }
                        NameListing::CannotList(why) => (
                            Completeness::CannotList(why.clone()),
                            BTreeSet::new(),
                            Vec::new(),
                        ),
                    };
                    let read = Read {
                        completeness,
                        names,
                        parts,
                    };
                    (key.clone(), read)
                })
                .collect(),
        };
        Self { of }
    }

    /// How complete the listing of `selection` in `space` is.
    pub fn completeness(&self, space: &Space, selection: &Selection) -> Completeness {
        self.entry(space, selection)
            .map_or(Completeness::NotAsked, |read| read.completeness.clone())
    }

    /// The names of `selection` in `space` when the listing is the whole, or else how
    /// complete it is (never [`Completeness::All`]).
    pub fn names(
        &self,
        space: &Space,
        selection: &Selection,
    ) -> Result<&BTreeSet<ArtifactName>, Completeness> {
        match self.entry(space, selection) {
            Some(read) if read.completeness.is_all() => Ok(&read.names),
            Some(read) => Err(read.completeness.clone()),
            None => Err(Completeness::NotAsked),
        }
    }

    /// For a cover that takes each part whole: the names of `selection` in `space`
    /// and the parts its listing could not list whole, when every name is in one of
    /// the two (the listing is the whole, with no part, or is in parts); or else how
    /// complete it is (never [`Completeness::All`]).
    pub fn names_and_parts(
        &self,
        space: &Space,
        selection: &Selection,
    ) -> Result<(&BTreeSet<ArtifactName>, &[ListingPart]), Completeness> {
        match self.entry(space, selection) {
            Some(read) if read.completeness.is_all() || !read.parts.is_empty() => {
                Ok((&read.names, &read.parts))
            }
            Some(read) => Err(read.completeness.clone()),
            None => Err(Completeness::NotAsked),
        }
    }

    /// Every (space, selection) read, with its completeness, in order.
    pub fn completenesses(&self) -> impl Iterator<Item = (&(Space, Selection), &Completeness)> {
        self.of.iter().map(|(key, read)| (key, &read.completeness))
    }

    fn entry(&self, space: &Space, selection: &Selection) -> Option<&Read> {
        // The key owns its parts; looked up by a key made of clones.
        self.of.get(&(space.clone(), selection.clone()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn space(text: &str) -> Space {
        Space::new(text).unwrap()
    }

    fn name(text: &str) -> ArtifactName {
        ArtifactName::new(text).unwrap()
    }

    fn why(text: &str) -> NonBlank {
        NonBlank::new(text).unwrap()
    }

    fn selection(include: &str) -> Selection {
        Selection::new(vec![include.to_owned()], vec![]).unwrap()
    }

    #[test]
    fn a_selection_includes_one_text_at_least_and_no_text_is_empty() {
        assert_eq!(
            Selection::new(vec![], vec!["a".to_owned()]),
            Err(SelectionProblem::NoInclude)
        );
        assert_eq!(
            Selection::new(vec!["a".to_owned(), String::new()], vec![]),
            Err(SelectionProblem::EmptyText {
                list: SelectionList::Include,
                at: 1
            })
        );
        assert_eq!(
            Selection::new(vec!["a".to_owned()], vec![String::new()]),
            Err(SelectionProblem::EmptyText {
                list: SelectionList::Exclude,
                at: 0
            })
        );
        let one = Selection::new(vec!["a".to_owned()], vec!["b".to_owned()]).unwrap();
        assert_eq!(
            (one.include(), one.exclude()),
            (&["a".to_owned()][..], &["b".to_owned()][..])
        );
    }

    #[test]
    fn each_listing_is_as_complete_as_it_says_and_only_the_whole_has_names() {
        let (file, rust) = (space("file"), space("rust-item"));
        let asked = BTreeMap::from([
            (
                (file.clone(), selection("a")),
                NameListing::All(BTreeSet::from([name("a.rs")])),
            ),
            (
                (file.clone(), selection("b")),
                NameListing::Some {
                    found: BTreeSet::from([name("b.rs")]),
                    why: why("cut at 1"),
                },
            ),
            (
                (rust.clone(), selection("a")),
                NameListing::CannotList(why("no")),
            ),
        ]);
        let listed = ListedNames::of(&Listings::Asked(asked));
        assert_eq!(
            listed.completeness(&file, &selection("a")),
            Completeness::All
        );
        assert_eq!(
            listed.names(&file, &selection("a")),
            Ok(&BTreeSet::from([name("a.rs")]))
        );
        assert_eq!(
            listed.names(&file, &selection("b")),
            Err(Completeness::Partial(why("cut at 1")))
        );
        assert_eq!(
            listed.names(&rust, &selection("a")),
            Err(Completeness::CannotList(why("no")))
        );
        // A selection that was not asked of an asked command is not asked either.
        assert_eq!(
            listed.completeness(&file, &selection("c")),
            Completeness::NotAsked
        );
    }

    #[test]
    fn a_listing_in_parts_is_not_the_whole_and_gives_its_names_and_parts_only_to_a_cover_of_parts()
    {
        let (tests, file) = (space("rust-test"), space("file"));
        let part = |path: &str, why_text: &str, read: PartRead| {
            ListingPart::new(
                ArtifactKey::new(file.clone(), name(path)),
                why(why_text),
                read,
            )
        };
        let parts: Vec<ListingPart> = (0..7)
            .map(|i| part(&format!("p{i}.rs"), "a macro", PartRead::Unread))
            .collect();
        let asked = BTreeMap::from([
            (
                (tests.clone(), selection("a")),
                NameListing::Parted {
                    found: BTreeSet::from([name("a.rs::t")]),
                    // `b.rs` given twice is one part, the first given.
                    parts: NonEmptyVec::new(
                        part(
                            "b.rs",
                            "a derive",
                            PartRead::Found(BTreeSet::from([name("b.rs::u")])),
                        ),
                        vec![
                            part("c.rs", "no file", PartRead::Unread),
                            part("b.rs", "again", PartRead::Unread),
                        ],
                    ),
                },
            ),
            (
                (tests.clone(), selection("many")),
                NameListing::Parted {
                    found: BTreeSet::new(),
                    parts: NonEmptyVec::from_vec(parts).unwrap(),
                },
            ),
            (
                (tests.clone(), selection("whole")),
                NameListing::All(BTreeSet::from([name("w.rs::t")])),
            ),
            (
                (tests.clone(), selection("cut")),
                NameListing::Some {
                    found: BTreeSet::from([name("x.rs::t")]),
                    why: why("cut"),
                },
            ),
        ]);
        let listed = ListedNames::of(&Listings::Asked(asked));
        // Not the whole, to every reader that does not take its parts whole; the
        // reason names each part with why.
        let Completeness::Partial(said) = listed.completeness(&tests, &selection("a")) else {
            panic!("a listing in parts is partial");
        };
        assert_eq!(
            said.as_str(),
            "parts of it are not listed whole: `b.rs` in `file` (a derive); `c.rs` in `file` (no file)"
        );
        assert!(listed.names(&tests, &selection("a")).is_err());
        // The first few parts are named, and the others counted.
        let Completeness::Partial(many) = listed.completeness(&tests, &selection("many")) else {
            panic!("a listing in parts is partial");
        };
        assert!(many.as_str().ends_with("; and 2 more part(s)"), "{many}");
        // To a cover that takes its parts whole: the names and the parts.
        let (names, parts) = listed.names_and_parts(&tests, &selection("a")).unwrap();
        assert_eq!(names, &BTreeSet::from([name("a.rs::t")]));
        assert_eq!(
            parts
                .iter()
                .map(|p| p.key().name().as_str())
                .collect::<Vec<_>>(),
            ["b.rs", "c.rs"]
        );
        // A whole listing has no part; one cut at a bound has nothing to give.
        assert_eq!(
            listed.names_and_parts(&tests, &selection("whole")),
            Ok((&BTreeSet::from([name("w.rs::t")]), &[][..]))
        );
        assert_eq!(
            listed.names_and_parts(&tests, &selection("cut")),
            Err(Completeness::Partial(why("cut")))
        );
        assert_eq!(
            listed.names_and_parts(&tests, &selection("none")),
            Err(Completeness::NotAsked)
        );
    }

    #[test]
    fn a_command_that_asked_nothing_has_no_listing_and_is_not_a_space_that_cannot_list() {
        let listed = ListedNames::of(&Listings::NotAsked);
        assert_eq!(
            listed.names(&space("file"), &selection("a")),
            Err(Completeness::NotAsked)
        );
        assert_eq!(listed.completenesses().count(), 0);
        assert_ne!(Completeness::NotAsked, Completeness::CannotList(why("no")));
    }
}
