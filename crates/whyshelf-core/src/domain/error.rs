//! Error types of the domain, one variant per reason.
//!
//! A caller branches on the kind of failure, so no error here is a message
//! string or a list of strings. Each constructor of a value with rules returns
//! one of these enums and keeps the cause (`#[source]`) when there is one.

use std::num::ParseIntError;

use thiserror::Error;

use super::id::EntityId;
use super::query::StateFilter;

/// Why a text could not become a value object.
///
/// One enum for all the small newtypes keeps the constructors uniform
/// (`TryFrom` / `FromStr` return it) while still telling a caller exactly which
/// rule was broken.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum ValueError {
    /// A hash text did not start with the `sha256:` scheme.
    #[error("a content hash starts with `sha256:`")]
    HashPrefix,
    /// A hash digest was not 64 characters long.
    #[error("a content hash digest has 64 hex characters, found {len}")]
    HashLength {
        /// Number of characters found after the scheme.
        len: usize,
    },
    /// A hash digest held a character that is not lowercase hex.
    #[error("a content hash digest is lowercase hex")]
    HashNotLowerHex,
    /// An identifier text was not a UUID.
    #[error("an entity id is a UUID")]
    IdNotUuid(#[source] uuid::Error),
    /// An identifier was a UUID of another version than 7.
    #[error("an entity id is a UUID v7, found version {version}")]
    IdNotV7 {
        /// The UUID version that was found.
        version: usize,
    },
    /// A free text that must carry content was empty or only whitespace.
    #[error("the text is empty")]
    BlankText,
    /// A list that must have an element was empty.
    #[error("the list is empty")]
    EmptyList,
    /// An item number was not a positive integer.
    #[error("an item number is a positive integer")]
    ItemNumberNotNumber(#[source] ParseIntError),
    /// An item number was zero.
    #[error("an item number starts at 1")]
    ItemNumberZero,
    /// An alias was empty.
    #[error("an alias is empty")]
    AliasEmpty,
    /// An alias was longer than the limit.
    #[error("an alias has at most {max} characters, found {len}")]
    AliasTooLong {
        /// Characters found.
        len: usize,
        /// The limit.
        max: usize,
    },
    /// An alias held a character outside `a-z 0-9 . _ - /`.
    #[error("an alias holds lowercase letters, digits and `. _ - /`, found `{ch}`")]
    AliasBadChar {
        /// The offending character.
        ch: char,
    },
    /// An alias started or ended with `/`, or held an empty segment.
    #[error("an alias has no empty path segment")]
    AliasBadShape,
    /// A name of a project (in an `ext:` reference) was malformed.
    #[error("a project name holds lowercase letters, digits and `. _ -`")]
    ProjectNameBadShape,
    /// A PIN identifier did not match `PIN-*` or `UT-*`.
    #[error("a pin id looks like PIN-<NAME> or UT-<NAME> (A-Z 0-9 _ -)")]
    PinIdBadShape,
    /// A hypothesis identifier did not match `HYP-*`.
    #[error("a hypothesis id looks like HYP-<NAME> (A-Z 0-9 _ -)")]
    HypIdBadShape,
    /// An option identifier of a Crux was malformed.
    #[error("an option id holds lowercase letters, digits and `. _ -`")]
    OptionIdBadShape,
    /// A tag was empty.
    #[error("a tag is empty")]
    TagEmpty,
    /// A tag held a character outside `a-z 0-9 . _ -` (and the one `:` of an official tag).
    #[error("a tag holds lowercase letters, digits and `. _ -`, found `{ch}`")]
    TagBadChar {
        /// The offending character.
        ch: char,
    },
    /// An official tag (`key:value`) had an empty key or value.
    #[error("an official tag is `key:value` with both parts present")]
    TagBadOfficial,
    /// A tag change did not start with `+` or `-`.
    #[error("a tag change starts with `+` or `-`")]
    TagChangeSign,
    /// A path was empty.
    #[error("a path is empty")]
    PathEmpty,
    /// A path was absolute.
    #[error("a path is relative to the project root")]
    PathAbsolute,
    /// A path held an empty, `.` or `..` segment.
    #[error("a path has no empty, `.` or `..` segment")]
    PathBadSegment,
    /// A path used a backslash or a control character.
    #[error("a path uses `/` and no control character")]
    PathBadChar,
    /// A `file:line` text had no `:line` part.
    #[error("a code location is `path:line`")]
    LocationNoLine,
    /// The line of a code location was not a number.
    #[error("the line of a code location is a number")]
    LocationBadLine(#[source] ParseIntError),
    /// The line of a code location was zero.
    #[error("the line of a code location starts at 1")]
    LocationZeroLine,
    /// The path of a code location was malformed.
    #[error("the path of a code location is malformed")]
    LocationBadPath(#[source] Box<ValueError>),
    /// A URI had no scheme.
    #[error("a URI starts with a scheme such as `https:`")]
    UriNoScheme,
    /// The scheme of a URI held a forbidden character.
    #[error("the scheme of a URI is letters, digits and `+ . -`")]
    UriBadScheme,
    /// A URI had nothing after the scheme.
    #[error("a URI has a part after its scheme")]
    UriEmptyRest,
    /// A URI held whitespace.
    #[error("a URI holds no whitespace")]
    UriWhitespace,
    /// A revision identifier was empty.
    #[error("a revision id is empty")]
    RevisionEmpty,
    /// A revision identifier held whitespace or a control character.
    #[error("a revision id holds no whitespace or control character")]
    RevisionBadChar,
    /// A name of a person was empty.
    #[error("an actor name is empty")]
    ActorEmpty,
    /// A name of a person held a line break or was too long.
    #[error("an actor name is one short line")]
    ActorBadShape,
    /// A timestamp text did not have the shape of RFC 3339.
    #[error("a timestamp has the shape of RFC 3339")]
    TimestampBadShape,
    /// A reference text was empty.
    #[error("a reference is empty")]
    RefEmpty,
    /// A `private:` reference did not hold an entity id.
    #[error("`private:` is followed by an entity id")]
    RefPrivateId(#[source] Box<ValueError>),
    /// An `ext:` reference was not `ext:<project>/<id-or-alias>`.
    #[error("an external reference is `ext:<project>/<id-or-alias>`")]
    RefExternalShape,
    /// A local reference was neither an entity id nor an alias.
    #[error("a reference is an entity id or an alias")]
    RefLocalShape(#[source] Box<ValueError>),
    /// A store name was neither `repo` nor `private`.
    #[error("a store is `repo` or `private`")]
    StoreKind,
    /// A word of a closed vocabulary was not one of its names.
    #[error("`{found}` is not a {what}")]
    UnknownName {
        /// What the word should have named, such as "maturity level".
        what: &'static str,
        /// The text found.
        found: String,
    },
}

/// Why an entity could not be built from its parts.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum EntityError {
    /// A Spec listed the same Finding twice among its grounds.
    #[error("the finding {finding} is listed twice among the grounds")]
    DuplicateGround {
        /// The repeated Finding.
        finding: EntityId,
    },
    /// Two options of a Crux share one identifier.
    #[error("the option id `{id}` is used twice in the crux")]
    DuplicateOptionId {
        /// The repeated identifier.
        id: String,
    },
}

/// Why an item reference could not be resolved.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum ItemRefError {
    /// An item reference was not `<ref>#<number>`.
    #[error("an item reference is `<ref>#<number>`")]
    Shape,
    /// The base of an item reference was malformed.
    #[error("the base of an item reference is malformed")]
    Base(#[source] ValueError),
    /// The number of an item reference was malformed.
    #[error("the number of an item reference is malformed")]
    Number(#[source] ValueError),
    /// A Spec was named but no Design refers to it.
    #[error("no design refers to the spec")]
    NoDesign,
    /// A Spec was named but more than one Design refers to it.
    #[error("{count} designs refer to the spec; name the design")]
    Ambiguous {
        /// How many designs refer to the Spec.
        count: usize,
    },
}

/// Why an event could not be built.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum EventError {
    /// The target of an approval is not the entity its attestation names.
    #[error("the event targets {target} but its attestation is about {subject}")]
    TargetMismatch {
        /// The target given to the event.
        target: EntityId,
        /// The entity the attestation is about.
        subject: EntityId,
    },
    /// A snapshot publication, the root of a store's stream, had heads.
    #[error("a snapshot publication is the root of its stream and has no heads")]
    SnapshotHasHeads,
}

/// Why a `find` query could not be parsed.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum QueryError {
    /// A term had a key the query language does not know.
    #[error("unknown query key `{key}`")]
    UnknownKey {
        /// The key found before the first `:`.
        key: String,
    },
    /// A term had a key but no value.
    #[error("the query key `{key}` has no value")]
    EmptyValue {
        /// The key without a value.
        key: String,
    },
    /// A quoted text was not closed.
    #[error("a quoted text is not closed")]
    UnterminatedQuote,
    /// A quoted search text was empty.
    #[error("a quoted search text is empty")]
    EmptyText,
    /// A quoted text ended with a lone backslash.
    #[error("a backslash ends the query")]
    DanglingEscape,
    /// The value of `kind:` was not an entity kind.
    #[error("`kind:` takes finding, spec, design or item, found `{found}`")]
    BadKind {
        /// The value found.
        found: String,
    },
    /// The value of `state:` was not a state.
    #[error("`state:` takes {}, found `{found}`", StateFilter::words())]
    BadState {
        /// The value found.
        found: String,
    },
    /// The value of `orphan:` was not `true` or `false`.
    #[error("`orphan:` takes true or false, found `{found}`")]
    BadBool {
        /// The value found.
        found: String,
    },
    /// The value of `tag:` was not a tag.
    #[error("`tag:` takes a tag")]
    BadTag(#[source] ValueError),
    /// The value of `owner:` was not a name.
    #[error("`owner:` takes a name")]
    BadOwner(#[source] ValueError),
    /// The value of `grounds:` or `upstream:` was not a reference.
    #[error("`{key}:` takes a reference")]
    BadRef {
        /// The key whose value was bad.
        key: &'static str,
        /// The cause.
        #[source]
        source: ValueError,
    },
}
