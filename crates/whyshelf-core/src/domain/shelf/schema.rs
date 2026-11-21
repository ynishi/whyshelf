//! What a book is: the declaration of its documents, checked once.
//!
//! A project declares its kinds of document as *books* (a directory, a grammar of
//! keys, the fields a front matter may hold and the links between books). The
//! declaration arrives as a [`BookSchemaDecl`] per book, built by an adapter from
//! whatever file it reads, and [`Schema::validate`] is the one place that decides
//! whether the whole of it is sound. It returns every problem at once, so a person
//! fixes a declaration in one pass, and it returns a [`Schema`] only when there is
//! none: nothing downstream re-checks a name, a prefix or a link target. A
//! declaration as a project writes it may name presets of links to artifacts and a
//! default ([`ShelfDecl`]); [`expand`] puts their values in first, and
//! [`Schema::declared`] does both. It may also declare who may decide about the
//! documents of the books ([`Authority`]), checked with them.
//!
//! The key grammar of a book ([`KeyPattern`]) is also decided here and only here:
//! [`KeyPattern::parse`] turns the text of a key into a [`KeyForm`], and both the
//! shelf that reads a document and the lookup of what a person typed call it.

use std::borrow::Borrow;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use thiserror::Error;

use crate::domain::actor::Actor;
use crate::domain::error::ValueError;
use crate::domain::location::RelPath;
use crate::domain::nonempty::{NonBlank, NonEmptyVec, TwoOrMore};
use crate::domain::words::{Join, Mark, listed};

use super::artifact::{
    ComparisonProblem, Selection, SelectionChecks, SelectionProblem, Space, SpaceProblem,
};

/// What a word of a declaration says: the part of a problem that tells which word
/// was refused, and the words it could have been.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WordOf {
    /// The count of a link ([`Cardinality`]).
    Count,
    /// The type of a field ([`ScalarType`]).
    Type,
    /// The meaning of a link or a preset ([`Propagation`]).
    Means,
}

impl WordOf {
    /// The words a declaration may write for it, as a problem lists them: `"one",
    /// "optional"`, each in quotes, in the order of the type's `ALL`.
    fn accepted(self) -> String {
        match self {
            Self::Count => quoted::<Cardinality>(Join::Comma),
            Self::Type => quoted::<ScalarType>(Join::Comma),
            Self::Means => quoted::<Propagation>(Join::Comma),
        }
    }
}

impl fmt::Display for WordOf {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Count => "count",
            Self::Type => "type",
            Self::Means => "meaning",
        })
    }
}

/// Where a word was written, for a problem to say so. A preset has no book.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WordAt {
    /// A word of a link of a book.
    Link {
        /// The book, as written.
        book: String,
        /// The link, as written.
        link: String,
    },
    /// A word of a field of a book.
    Field {
        /// The book, as written.
        book: String,
        /// The field, as written.
        field: String,
    },
    /// A word of a preset of the project.
    Preset {
        /// The preset, as written.
        name: String,
    },
}

/// An entry of a book that is written as a word: where a word was refused, for a
/// problem to say so.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BookEntry {
    /// `without_front_matter`: what a file in the range with no front matter is
    /// ([`WithoutFrontMatter`]).
    WithoutFrontMatter,
    /// `key`: the grammar of the keys ([`KeyWord`], or a table).
    Key,
}

impl BookEntry {
    /// The name of the entry in a declaration: `without_front_matter` or `key`.
    pub fn name(self) -> &'static str {
        match self {
            Self::WithoutFrontMatter => "without_front_matter",
            Self::Key => "key",
        }
    }

    /// What the entry takes, as a problem says it: `"invalid" or "skip"`, or `"slug" or
    /// a table { prefix = "..." }`. The words are those of the type, so a word added
    /// there is said here.
    pub fn accepted(self) -> String {
        match self {
            Self::WithoutFrontMatter => quoted::<WithoutFrontMatter>(Join::Or),
            // The table is one more of the things the entry takes, so a second word
            // is joined with the others and `or` is said once.
            Self::Key => listed(
                KeyWord::ALL
                    .iter()
                    .map(|word| format!("\"{}\"", word.word()))
                    .chain(["a table { prefix = \"...\" }".to_owned()]),
                Mark::Plain,
                Join::Or,
            ),
        }
    }
}

impl fmt::Display for BookEntry {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

/// The declaration of one book, checked.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BookSchema {
    name: BookName,
    range: BookRange,
    key: KeyPattern,
    description: Option<BookDescription>,
    fields: Declared<EntryName, FieldDecl>,
    /// Every link, into books and to artifacts, in one list: a name is one link.
    links: Declared<EntryName, DeclaredLink>,
    /// The preset each link to artifacts was expanded from, for the links that were.
    presets: BTreeMap<EntryName, PresetOrigin>,
}

impl BookSchema {
    /// The name of the book.
    pub fn name(&self) -> &BookName {
        &self.name
    }

    /// What the book is for, when the declaration says.
    pub fn description(&self) -> Option<&BookDescription> {
        self.description.as_ref()
    }

    /// Every link of the book, into books and to artifacts, in the order of the
    /// declaration.
    pub fn all_links(&self) -> &Declared<EntryName, DeclaredLink> {
        &self.links
    }

    /// The directory of the book.
    pub fn dir(&self) -> &BookDir {
        self.range.dir()
    }

    /// The range of the book inside its directory.
    pub fn scope(&self) -> &BookScope {
        self.range.scope()
    }

    /// The directory with its range: what a listing of files and `new` ask whether
    /// a path is a document of the book.
    pub fn range(&self) -> &BookRange {
        &self.range
    }

    /// Whether `path` is a document of the book ([`BookRange::takes`]).
    pub fn takes(&self, path: &RelPath) -> bool {
        self.range.takes(path)
    }

    /// The grammar of the keys of the book.
    pub fn key(&self) -> &KeyPattern {
        &self.key
    }

    /// The fields of the book, by name, in the order of the declaration.
    pub fn fields(&self) -> &Declared<EntryName, FieldDecl> {
        &self.fields
    }

    /// The links of the book into books, by name, in the order of the declaration.
    pub fn links(&self) -> Picked<'_, EntryName, DeclaredLink, LinkKind> {
        Picked::new(&self.links, DeclaredLink::book)
    }

    /// The links of the book to artifacts, by name, in the order of the
    /// declaration. A name is a link into a book or a link to artifacts, never both.
    pub fn artifact_links(&self) -> Picked<'_, EntryName, DeclaredLink, ArtifactLinkKind> {
        Picked::new(&self.links, DeclaredLink::artifact)
    }

    /// The preset a link to artifacts was expanded from ([`expand`]), or none when
    /// it names none and no default applied: what `map` tells beside the values,
    /// which are in [`artifact_links`](Self::artifact_links) whatever their origin.
    pub fn preset_of<Q>(&self, link: &Q) -> Option<&PresetOrigin>
    where
        EntryName: Borrow<Q>,
        Q: Ord + ?Sized,
    {
        self.presets.get(link)
    }
}

/// Where a name was written, for a problem to say so.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NamePlace {
    /// The name of a book.
    Book,
    /// The name of a field of a book.
    Field {
        /// The book, as written.
        book: String,
    },
    /// The name of a link of a book.
    Link {
        /// The book, as written.
        book: String,
    },
    /// The name of a preset.
    Preset,
    /// The target of a link.
    LinkTarget {
        /// The book, as written.
        book: String,
        /// The link, as written.
        link: String,
    },
    /// A word of an enum field.
    EnumWord {
        /// The book, as written.
        book: String,
        /// The field, as written.
        field: String,
    },
    /// The name of a group of actors.
    Group,
}

impl fmt::Display for NamePlace {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Book => f.write_str("the name of a book"),
            Self::Field { book } => write!(f, "a field of book `{book}`"),
            Self::Link { book } => write!(f, "a link of book `{book}`"),
            Self::Preset => f.write_str("the name of a preset"),
            Self::LinkTarget { book, link } => {
                write!(f, "the target of link `{link}` of book `{book}`")
            }
            Self::EnumWord { book, field } => {
                write!(f, "a word of field `{field}` of book `{book}`")
            }
            Self::Group => f.write_str("the name of a group"),
        }
    }
}

/// One thing wrong in a declaration. [`Schema::validate`] returns all of them.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum SchemaProblem {
    /// A name that breaks the grammar of names (or is reserved).
    #[error("{place}: `{text}`: {why}")]
    BadName {
        /// Where it was written.
        place: NamePlace,
        /// The name as written.
        text: String,
        /// What is wrong with it.
        why: NameProblem,
    },
    /// A directory that is not a relative path.
    #[error("book `{book}`: the directory is not a relative path")]
    BadDir {
        /// The book, as written.
        book: String,
        /// Why the path is refused.
        #[source]
        source: ValueError,
    },
    /// A description that is blank or more than one line.
    #[error("book `{book}`: the description {why}")]
    BadDescription {
        /// The book, as written.
        book: String,
        /// What is wrong with it.
        why: DescriptionProblem,
    },
    /// A key prefix that breaks its grammar.
    #[error("book `{book}`: {why}")]
    BadKeyPrefix {
        /// The book, as written.
        book: String,
        /// What is wrong with it.
        why: KeyProblem,
    },
    /// An enum field with no word.
    #[error("book `{book}`: the enum field `{field}` lists no word")]
    EmptyEnum {
        /// The book, as written.
        book: String,
        /// The field, as written.
        field: String,
    },
    /// Two books of one name.
    #[error("the book `{name}` is declared twice")]
    DuplicateBook {
        /// The name.
        name: BookName,
    },
    /// A field written twice in a book.
    #[error("book `{book}`: the field `{name}` is declared twice")]
    DuplicateField {
        /// The book, as written.
        book: String,
        /// The field.
        name: EntryName,
    },
    /// A link written twice in a book.
    #[error("book `{book}`: the link `{name}` is declared twice")]
    DuplicateLink {
        /// The book, as written.
        book: String,
        /// The link.
        name: EntryName,
    },
    /// A name that is a field and a link of one book.
    #[error("book `{book}`: `{name}` is both a field and a link")]
    NameCollision {
        /// The book, as written.
        book: String,
        /// The name.
        name: EntryName,
    },
    /// A link into a book that is not declared.
    #[error("book `{book}`: the link `{link}` points into `{target}`, which is not declared")]
    UnknownLinkTarget {
        /// The book, as written.
        book: String,
        /// The link.
        link: EntryName,
        /// The book it points into.
        target: BookName,
    },
    /// Books that use one prefix for their keys.
    #[error("the books {} use the key prefix `{prefix}`", listed(.books.iter(), Mark::Backtick, Join::And))]
    PrefixClash {
        /// The prefix.
        prefix: KeyPrefix,
        /// The books that use it.
        books: TwoOrMore<BookName>,
    },
    /// Two books whose directories are one and the same, or one under the other.
    #[error("the directories of `{first}` and `{second}` are the same or one is under the other")]
    DirOverlap {
        /// One book.
        first: BookName,
        /// The other.
        second: BookName,
    },
    /// A pattern of `include` or `exclude` that is not a relative glob staying in the
    /// directory of the book.
    #[error("book `{book}`: the `{list}` pattern `{pattern}` {why}")]
    BadScopePattern {
        /// The book, as written.
        book: String,
        /// The list the pattern is in.
        list: ScopeList,
        /// The pattern, as written.
        pattern: String,
        /// What is wrong with it.
        why: PatternProblem,
    },
    /// `include = []`: the book would take no file.
    #[error("book `{book}`: `include` lists no pattern, so the book would take no file")]
    EmptyInclude {
        /// The book, as written.
        book: String,
    },
    /// An entry of a book written as a word the entry does not take: a
    /// `without_front_matter` that is neither `"invalid"` nor `"skip"`, a `key` that
    /// is neither `"slug"` nor a table ([`read_book_word`]).
    #[error(
        "book `{book}`: `{entry}` is \"{word}\", and it takes {}",
        .entry.accepted()
    )]
    UnknownBookWord {
        /// The book, as written.
        book: String,
        /// The entry the word was written in.
        entry: BookEntry,
        /// The word, as written.
        word: String,
    },
    /// A `Replaces` link that points outside its own book.
    #[error(
        "book `{book}`: the link `{link}` replaces, so it points into this book, not `{target}`"
    )]
    ReplacesForeign {
        /// The book, as written.
        book: String,
        /// The link.
        link: EntryName,
        /// The book it points into.
        target: BookName,
    },
    /// A link to artifacts that names no space: there is no space a link has when
    /// it names none.
    #[error(
        "book `{book}`: the link `{link}` points at artifacts and names no `space` they are in"
    )]
    NoSpace {
        /// The book, as written.
        book: String,
        /// The link.
        link: EntryName,
    },
    /// A link to artifacts whose `space` is no space.
    #[error("book `{book}`: the link `{link}`: {why}")]
    BadSpace {
        /// The book, as written.
        book: String,
        /// The link.
        link: EntryName,
        /// Why.
        why: SpaceProblem,
    },
    /// A link to artifacts whose space no adapter offers.
    #[error(
        "book `{book}`: the link `{link}` names the space `{space}`, which no adapter offers (it offers: {offered})"
    )]
    UnknownSpace {
        /// The book, as written.
        book: String,
        /// The link.
        link: EntryName,
        /// The space it names.
        space: Space,
        /// The spaces offered, as one text.
        offered: String,
    },
    /// `space` on a link into a book: only artifacts are in a space.
    #[error(
        "book `{book}`: the link `{link}` points into a book, and only a link to artifacts takes `space`"
    )]
    SpaceOnBookLink {
        /// The book, as written.
        book: String,
        /// The link.
        link: EntryName,
    },
    /// A link to artifacts that says it replaces them: an artifact takes no
    /// document's place, so such a link `depends` or `informs`.
    #[error(
        "book `{book}`: the link `{link}` points at artifacts, so it depends or informs and does not replace"
    )]
    ArtifactReplaces {
        /// The book, as written.
        book: String,
        /// The link.
        link: EntryName,
    },
    /// A book named with the word a link uses for artifacts.
    #[error("`{ARTIFACT_TARGET}` is the target of a link to artifacts and cannot name a book")]
    ReservedBookName,
    /// `compare` on a link into a book: a document is compared by its hash, always.
    #[error(
        "book `{book}`: the link `{link}` points into a book, and only a link to artifacts takes `compare`"
    )]
    CompareOnBookLink {
        /// The book, as written.
        book: String,
        /// The link.
        link: EntryName,
    },
    /// `cover` with a strength alone (`true`, `"error"` or `"warn"`) on a link to
    /// artifacts: the names it expects to be reached are those a selection takes, so
    /// the cover of such a link is a table with an `include`.
    #[error(
        "book `{book}`: the link `{link}` points at artifacts, and its `cover` is a table of a selection (`cover = {{ strength = \"error\", include = [..], exclude = [..] }}`), not a strength alone"
    )]
    CoverNeedsSelection {
        /// The book, as written.
        book: String,
        /// The link.
        link: EntryName,
    },
    /// `cover` as a table of a selection on a link into a book: only a link to
    /// artifacts takes a selection, and a book is covered whole.
    #[error(
        "book `{book}`: the link `{link}` points into a book, and only a link to artifacts takes a selection in its `cover` (a link into a book takes true, false, \"error\" or \"warn\")"
    )]
    SelectionOnBookLink {
        /// The book, as written.
        book: String,
        /// The link.
        link: EntryName,
    },
    /// The selection of a `cover` on a link to artifacts that cannot be made: none to
    /// include, or an empty text.
    #[error("book `{book}`: the selection of the `cover` of the link `{link}`: {why}")]
    BadSelection {
        /// The book, as written.
        book: String,
        /// The link.
        link: EntryName,
        /// Why (said in the message, so it is no source as well).
        why: SelectionProblem,
    },
    /// The selection of a `cover` that the adapter of the space does not read: a text
    /// of it is none of the grammar of the space.
    #[error(
        "book `{book}`: the selection of the `cover` of the link `{link}` is not one of the space `{space}`: {why}"
    )]
    NotASelection {
        /// The book, as written.
        book: String,
        /// The link.
        link: EntryName,
        /// The space of the link.
        space: Space,
        /// Why, as the adapter says it.
        why: NonBlank,
    },
    /// A `cover` on a link whose space lists no names (so that what the cover expects
    /// is not known), or whose selection was not asked about: refused, never taken as
    /// a cover of nothing.
    #[error(
        "book `{book}`: the link `{link}` in the space `{space}` takes no `cover` over a selection: {why}"
    )]
    SpaceCannotList {
        /// The book, as written.
        book: String,
        /// The link.
        link: EntryName,
        /// The space of the link.
        space: Space,
        /// Why, as the adapter says it (`not asked` when nobody asked).
        why: NonBlank,
    },
    /// A `cover` that is no strength ([`CoverSeverity::read`]).
    #[error(
        "book `{book}`: the `cover` of the link `{link}` is \"{value}\", and it takes true, false, \"error\" or \"warn\""
    )]
    BadCover {
        /// The book, as written.
        book: String,
        /// The link.
        link: EntryName,
        /// The word, as written.
        value: String,
    },
    /// A `compare` that is no comparison.
    #[error("book `{book}`: the `compare` of the link `{link}` is not one")]
    BadComparison {
        /// The book, as written.
        book: String,
        /// The link.
        link: EntryName,
        /// Why.
        #[source]
        why: ComparisonProblem,
    },
    /// A link, a `from` or the default that names a preset that is neither
    /// declared by the project nor offered by an adapter.
    #[error(
        "{place} names the preset `{name}`, which is not where it is looked for (the presets of the project, then those the adapters offer; for a `from` of the preset's own name, only those the adapters offer)"
    )]
    UnknownPreset {
        /// Where the name was written.
        place: PresetPlace,
        /// The name, as written.
        name: String,
    },
    /// Presets that each come `from` the next, back to the first.
    #[error("the presets {} come from one another in a cycle", cycle(presets))]
    PresetCycle {
        /// The presets of the cycle: the first is the one of them written first in
        /// the declaration, and each next one is the `from` of the one before it.
        /// The `from` of the last is the first again.
        presets: NonEmptyVec<PresetName>,
    },
    /// A preset whose `space` is no space.
    #[error("the preset `{preset}`: {why}")]
    PresetBadSpace {
        /// The preset, as written.
        preset: String,
        /// Why.
        why: SpaceProblem,
    },
    /// A preset whose `space` no adapter offers.
    #[error(
        "the preset `{preset}` names the space `{space}`, which no adapter offers (it offers: {offered})"
    )]
    PresetUnknownSpace {
        /// The preset, as written.
        preset: String,
        /// The space it names.
        space: Space,
        /// The spaces offered, as one text.
        offered: String,
    },
    /// A preset whose `compare` and `ignore`, as it writes them over those it comes
    /// from, are no comparison.
    #[error("the `compare` of the preset `{preset}` is not one")]
    PresetBadComparison {
        /// The preset, as written.
        preset: String,
        /// Why.
        #[source]
        why: ComparisonProblem,
    },
    /// Two presets of one name declared by the project.
    #[error("the preset `{name}` is declared twice")]
    DuplicatePreset {
        /// The name.
        name: PresetName,
    },
    /// `preset` on a link into a book: a preset holds the values of a link to
    /// artifacts.
    #[error(
        "book `{book}`: the link `{link}` points into a book, and only a link to artifacts takes `preset`"
    )]
    PresetOnBookLink {
        /// The book, as written.
        book: String,
        /// The link, as written.
        link: String,
    },
    /// A preset that says it replaces: it gives the values of a link to artifacts,
    /// which depends or informs. The one value a preset cannot hold (a value of
    /// another kind is no part of a preset at all, and the adapter that reads the
    /// declaration refuses it).
    #[error(
        "the preset `{preset}` means `replaces`, and a preset of links to artifacts depends or informs"
    )]
    PresetReplaces {
        /// The preset, as written.
        preset: String,
    },
    /// A link that says no `means`, and whose preset gives none.
    #[error("book `{book}`: the link `{link}` says no `means`, and no preset gives one")]
    NoMeans {
        /// The book, as written.
        book: String,
        /// The link, as written.
        link: String,
    },
    /// A count, a type or a meaning written as a word the declaration does not
    /// have ([`read_word`]).
    #[error("{}", unknown_word(at, word, *of))]
    UnknownWord {
        /// Where it was written.
        at: WordAt,
        /// The word, as written.
        word: String,
        /// What the word says.
        of: WordOf,
    },
    /// Two groups of one name.
    #[error("the group `{name}` is declared twice")]
    DuplicateGroup {
        /// The name.
        name: GroupName,
    },
    /// A group with no member.
    #[error("the group `{group}` has no member")]
    EmptyGroup {
        /// The group, as written.
        group: String,
    },
    /// A member that is no name of an actor.
    #[error("the group `{group}`: `{member}` is not a name of an actor")]
    BadMember {
        /// The group, as written.
        group: String,
        /// The member, as written.
        member: String,
        /// Why.
        #[source]
        source: ValueError,
    },
    /// A member whose `from` is not before its `until`: it would be in the group at no
    /// time.
    #[error("the group `{group}`: the `from` of `{member}` is not before its `until`")]
    MemberSpan {
        /// The group, as written.
        group: String,
        /// The member.
        member: Actor,
    },
    /// A rule of who may decide that names a group not declared.
    #[error("{at} names the group `{group}`, which is not declared")]
    UnknownGroup {
        /// Where the rule is.
        at: RulePlace,
        /// The group, as written.
        group: String,
    },
    /// A rule of who may decide that names no group for a kind, or as its default.
    #[error("{at}: `{}` names no group", kind.map_or("default", DecideKind::word))]
    NoGroup {
        /// Where the rule is.
        at: RulePlace,
        /// The kind, or none for the default.
        kind: Option<DecideKind>,
    },
    /// A rule of who may decide for a book that is not declared.
    #[error("who may decide is declared for the book `{book}`, which is not declared")]
    DecideOfUnknownBook {
        /// The book, as written.
        book: String,
    },
    /// Two rules of who may decide for one book.
    #[error("who may decide is declared twice for the book `{book}`")]
    DecideTwice {
        /// The book, as written.
        book: String,
    },
    /// A book declares who may decide and `[decide]` is not declared: the records of
    /// no book, and those whose book cannot be told, would be ruled by none.
    #[error(
        "a book declares who may decide, and `[decide]` does not: the records of no book (a move, the same meaning of an artifact) and those whose book cannot be told would escape every rule"
    )]
    NoShelfRule,
    /// A rule's `approvals` that is not from one to [`Approvals::MOST`].
    #[error(
        "{at}: `approvals` is {written}, and it takes a number from 1 to {}",
        Approvals::MOST
    )]
    ApprovalsOutOfRange {
        /// Where the rule is.
        at: RulePlace,
        /// The number, as written.
        written: i64,
    },
    /// A rule that asks for more different names than may approve (or declare the
    /// same meaning) under it, whatever their times: no approval of its book (no
    /// declaration of an edit) could settle.
    #[error(
        "{at} asks for {approvals} different names to {deed}, and only {names} may {deed} under it",
        deed = kind.deed()
    )]
    TooFewApprovers {
        /// Where the rule is.
        at: RulePlace,
        /// The kind the names are too few for: `approve`, or `same` when it names
        /// other groups.
        kind: DecideKind,
        /// What it asks for.
        approvals: Approvals,
        /// How many names may approve.
        names: usize,
    },
    /// `[decide]` asks for fewer names than a book: a declaration of the same meaning
    /// of an artifact, which `[decide]` rules, would keep the approvals of that book
    /// fresh with fewer names than they need.
    #[error(
        "`[decide]` asks for fewer different names ({approvals}) than book `{book}` ({book_approvals}): a declaration of the same meaning of an artifact keeps that book's approvals, and is ruled by `[decide]`"
    )]
    ShelfApprovalsBelowBook {
        /// What `[decide]` asks for.
        approvals: Approvals,
        /// The book that asks the most.
        book: BookName,
        /// What it asks for.
        book_approvals: Approvals,
    },
    /// `[decide]` reads fewer records than a book that asks for more than one name:
    /// its `since` is later than the book's, or the book has none. A declaration of
    /// the same meaning of an artifact
    /// made before `[decide]`'s `since` would be read by no rule, so one name would
    /// keep that book's approvals fresh however many names it asks for.
    #[error(
        "`[decide]` reads the records made from {since}, and book `{book}` reads earlier ones: a declaration of the same meaning of an artifact made before then would be ruled by none, and keep that book's approvals with one name"
    )]
    ShelfSinceAfterBook {
        /// `[decide]`'s `since`.
        since: RuleTime,
        /// The first book, by name, that reads earlier records.
        book: BookName,
    },
}

/// What [`SchemaProblem::UnknownWord`] says: where the word was written, which word
/// it was, and the words that would have been read.
fn unknown_word(at: &WordAt, word: &str, of: WordOf) -> String {
    // The words of the type, as the declaration has them: whether a place takes a
    // word of them is the check of that place ([`SchemaProblem::PresetReplaces`]).
    let accepted = of.accepted();
    match at {
        WordAt::Link { book, link } => format!(
            "book `{book}`: the {of} of the link `{link}` is \"{word}\", and it takes one of {accepted}"
        ),
        WordAt::Field { book, field } => format!(
            "book `{book}`: the {of} of the field `{field}` is \"{word}\", and it takes one of {accepted}"
        ),
        WordAt::Preset { name } => {
            format!("preset `{name}`: the {of} is \"{word}\", and it takes one of {accepted}")
        }
    }
}

/// The presets of a cycle, as a problem says them: `` `a` -> `b` -> `a` ``.
fn cycle(presets: &NonEmptyVec<PresetName>) -> String {
    presets
        .iter()
        .chain([presets.first()])
        .map(|name| format!("`{name}`"))
        .collect::<Vec<_>>()
        .join(" -> ")
}

/// Whether a file belongs to a book: what `Schema::membership` answers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Membership<'a> {
    /// The file is under the directory of this book.
    In(&'a BookSchema),
    /// The file is under no book's directory.
    Outside,
}

/// The books of a project, checked as a whole, and who may decide about their
/// documents.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Schema {
    books: Declared<BookName, BookSchema>,
    authority: Authority,
}

/// The spaces offered, as a problem says them: `` `a`, `b` ``, or `none`.
fn offered(spaces: &BTreeSet<Space>) -> String {
    if spaces.is_empty() {
        "none".to_owned()
    } else {
        listed(spaces, Mark::Backtick, Join::Comma)
    }
}

impl Schema {
    /// The declaration of a project, checked as a whole.
    ///
    /// Every problem is returned, not the first: a name that breaks the grammar, a
    /// reserved name, a book, a field or a link declared twice, a name that is both
    /// a field and a link, a link into a book that is not declared, an enum with no
    /// word, two books that use one key prefix, two directories that are the same or
    /// nest, a `Replaces` link that leaves its own book, a link to artifacts that
    /// replaces, one that names no space, a text that is no space, or a space not in
    /// `spaces` (the spaces the adapters offer, given as a value), a book named
    /// [`ARTIFACT_TARGET`], a description that is no [`BookDescription`], a
    /// pattern of `include` or `exclude` that is no [`Pattern`], an `include` with
    /// none, a `without_front_matter` that is no [`WithoutFrontMatter`], and a type
    /// written as a word that is no [`ScalarType`] ([`SchemaProblem::UnknownWord`]). A
    /// [`Schema`] exists only when there is none, so nothing after this re-checks a
    /// declaration. The books, and the fields and links of each, are kept in the
    /// order the declaration writes them, once ([`Declared`]).
    ///
    /// It takes declarations with no preset left in them: those of a project come
    /// through [`Schema::declared`], which [`expand`]s them first. Nothing here has
    /// asked an adapter whether it reads a selection, so a `cover` of a link to
    /// artifacts is refused, as the problem of a space that cannot list
    /// ([`SchemaProblem::SpaceCannotList`]); [`Schema::declared`] is given the answers.
    // The problems are the answer, read once per run; it is not a hot path.
    #[allow(clippy::result_large_err)]
    pub fn validate(
        decls: Vec<BookSchemaDecl>,
        spaces: &BTreeSet<Space>,
    ) -> Result<Self, NonEmptyVec<SchemaProblem>> {
        Self::validate_expanded(Expanded::plain(decls), spaces, &SelectionChecks::new())
    }

    /// The selections of the `cover` of the links to artifacts of a declaration, as
    /// the (space, selection)s the adapters are to be asked about before
    /// [`declared`](Self::declared) can check them.
    ///
    /// The declaration is read as `declared` reads it: [`expand`]ed first (the
    /// problems of the expansion are dropped here, and told once by `declared`), and
    /// each link's `cover` read by the same reader `validate` uses. A link whose space
    /// is not one the adapters offer, or whose `cover` is refused for another reason,
    /// is not asked about (`declared` tells that problem, and one cause is not told
    /// twice). Two links that declare one selection in one space ask one question.
    pub fn selection_asks(
        decl: &ShelfDecl,
        spaces: &BTreeSet<Space>,
        builtins: &BuiltinPresets,
    ) -> BTreeSet<(Space, Selection)> {
        let (expanded, _told_by_declared) = expand_all(decl.clone(), builtins, spaces);
        asked_selections(expanded.books(), spaces)
    }

    /// The declaration of a project as it is written, with its presets and its
    /// default: [`expand`]ed with the presets the adapters offer (`builtins`), then
    /// checked as [`validate`](Self::validate) checks it, against `spaces`. What
    /// expanded is checked even when the expansion has problems (a link it could not
    /// expand is left out of the check, its problem told once), and the problems of
    /// both are returned at once, those of the expansion first. The schema keeps,
    /// for each link to artifacts, the preset it came from
    /// ([`BookSchema::preset_of`]).
    ///
    /// `selection_checks` is what the adapters answered for each
    /// [`selection_asks`](Self::selection_asks): a `cover` on a link to artifacts is
    /// kept only when its space reads its selection; one it does not read, a space that
    /// lists no names, and a selection that has no answer (read as "not asked") are
    /// problems, so that a cover that cannot be evaluated is never taken as one of
    /// nothing.
    ///
    /// Who may decide ([`DecideDecl`]) is read with it, against the books the
    /// declaration writes, and its problems come last ([`Authority`]).
    #[allow(clippy::result_large_err)]
    pub fn declared(
        mut decl: ShelfDecl,
        builtins: &BuiltinPresets,
        spaces: &BTreeSet<Space>,
        selection_checks: &SelectionChecks,
    ) -> Result<Self, NonEmptyVec<SchemaProblem>> {
        let decide = std::mem::take(&mut decl.decide);
        let written: BTreeSet<String> = decl.books.iter().map(|b| b.name.clone()).collect();
        let (expanded, problems) = expand_all(decl, builtins, spaces);
        let checked = Self::validate_expanded(expanded, spaces, selection_checks);
        let mut ruled = Vec::new();
        let authority = read_authority(decide, &written, &mut ruled);
        let checked = match (NonEmptyVec::from_vec(problems), checked) {
            (Err(_), checked) => checked,
            (Ok(problems), Ok(_)) => Err(problems),
            (Ok(problems), Err(more)) => Err(problems.extended(more.into_vec())),
        };
        match (checked, NonEmptyVec::from_vec(ruled)) {
            (Ok(schema), Err(_)) => Ok(Self {
                authority,
                ..schema
            }),
            (Ok(_), Ok(ruled)) => Err(ruled),
            (Err(problems), ruled) => {
                Err(problems.extended(ruled.map_or_else(|_| Vec::new(), NonEmptyVec::into_vec)))
            }
        }
    }

    /// The (space, selection)s of the `cover` of the links to artifacts the books
    /// declare: what a command that reads an artifact cover asks the adapters to list.
    /// Two links that declare one selection in one space are one.
    pub fn cover_selections(&self) -> BTreeSet<(Space, Selection)> {
        self.books
            .values()
            .flat_map(|book| book.artifact_links().values())
            .filter_map(|kind| {
                let cover = kind.cover.as_ref()?;
                Some((kind.space.clone(), cover.selection().clone()))
            })
            .collect()
    }

    /// Who may decide about the documents of the books ([`Authority`]); unruled
    /// when the declaration says nothing of it.
    pub fn authority(&self) -> &Authority {
        &self.authority
    }

    #[allow(clippy::result_large_err)]
    fn validate_expanded(
        expanded: Expanded,
        spaces: &BTreeSet<Space>,
        selection_checks: &SelectionChecks,
    ) -> Result<Self, NonEmptyVec<SchemaProblem>> {
        let (decls, origins) = expanded.into_parts();
        let mut problems = Vec::new();
        let drafts: Vec<Draft> = decls
            .into_iter()
            .map(|decl| Draft::read(decl, spaces, selection_checks, &origins, &mut problems))
            .collect();

        let declared = declared_names(&drafts, &mut problems);
        check_links(&drafts, &declared, &mut problems);
        check_prefixes(&drafts, &mut problems);
        check_dirs(&drafts, &mut problems);

        if let Ok(problems) = NonEmptyVec::from_vec(problems) {
            return Err(problems);
        }
        let key_of: BTreeMap<BookName, KeyPattern> = drafts
            .iter()
            .filter_map(|d| Some((d.name.clone()?, d.key.clone()?)))
            .collect();
        let mut books = Declared::default();
        for book in drafts
            .into_iter()
            .filter_map(|draft| draft.into_schema(&key_of))
        {
            books.insert(book.name.clone(), book);
        }
        Ok(Self {
            books,
            authority: Authority::default(),
        })
    }

    /// The book a file belongs to: the one whose directory it is under.
    ///
    /// The one entry of the question; the shelf asks it of every file it reads.
    pub(super) fn membership(&self, path: &RelPath) -> Membership<'_> {
        self.books
            .values()
            .find(|book| book.takes(path))
            .map_or(Membership::Outside, Membership::In)
    }

    /// The book a file belongs to, or none when it is under no book's directory: the
    /// answer the shelf reads every file by, for a use case that asks what book a file
    /// it read is of.
    pub fn book_of(&self, path: &RelPath) -> Option<&BookSchema> {
        match self.membership(path) {
            Membership::In(book) => Some(book),
            Membership::Outside => None,
        }
    }

    /// The directory and range of every book, in the order of the declaration: what
    /// a listing of the documents is given.
    pub fn ranges(&self) -> Vec<BookRange> {
        self.books
            .values()
            .map(|book| book.range().clone())
            .collect()
    }

    /// A book by name, given as a [`BookName`] or as the text of one.
    pub fn book<Q>(&self, name: &Q) -> Option<&BookSchema>
    where
        BookName: Borrow<Q>,
        Q: Ord + ?Sized,
    {
        self.books.get(name)
    }

    /// The books, by name, in the order of the declaration.
    pub fn books(&self) -> &Declared<BookName, BookSchema> {
        &self.books
    }
}

mod decide;
mod decl;
mod declared;
mod field;
mod key;
mod link;
mod name;
mod preset;
mod scope;
mod validate;
mod word;

use scope::read_scope;
pub use scope::{
    BookRange, BookScope, Pattern, PatternProblem, ScopeDecl, ScopeList, WithoutFrontMatter,
};

use decide::read_authority;
pub use decide::{
    Approvals, Authority, BookRuleDecl, DecideDecl, DecideKind, DecideRule, Group, GroupDecl,
    Member, MemberDecl, Permission, RuleDecl, RulePlace, RuleTime,
};
pub use declared::{Declared, Picked};
pub use preset::{
    BuiltinPresets, DefaultsDecl, Expanded, PresetDecl, PresetOrigin, PresetPlace, PresetValues,
    ShelfDecl, WrittenBookDecl, WrittenLinkDecl, WrittenPresetValues, expand,
};
use preset::{Origins, expand_all};

pub use decl::{
    BookSchemaDecl, CompareDecl, FieldEntryDecl, FieldTypeDecl, KeyPatternDecl, LinkEntryDecl,
    LinkTargetDecl,
};
pub use field::{FieldDecl, FieldPresence, FieldType, ScalarType};
pub(super) use key::parse_tail;
pub use key::{KeyForm, KeyName, KeyPattern, KeyPrefix, KeyProblem, KeyTail, KeyWord, Slug};
pub use link::{
    ARTIFACT_TARGET, ArtifactCover, ArtifactLinkKind, ArtifactPropagation, Cardinality, CoverDecl,
    CoverSeverity, DeclaredLink, LinkKind, Propagation,
};
pub use name::{
    BookDescription, BookDir, BookName, DescriptionProblem, EntryName, FieldWord, GroupName,
    NameProblem, PresetName,
};
pub(super) use name::{ID_ENTRY, KEY_ENTRY};
use validate::{Draft, asked_selections, check_dirs, check_links, check_prefixes, declared_names};
pub use word::{BookWord, DeclWord, Words, read_book_word, read_word};
use word::{quoted, report};

#[cfg(test)]
use super::artifact::Comparison;
#[cfg(test)]
use crate::domain::id::ShortId;
#[cfg(test)]
use crate::domain::nonempty::NonEmptySet;
#[cfg(test)]
use validate::read_key_pattern;

#[cfg(test)]
mod comparison_tests;
#[cfg(test)]
mod cover_tests;
#[cfg(test)]
mod preset_tests;
#[cfg(test)]
mod scope_tests;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod word_tests;
