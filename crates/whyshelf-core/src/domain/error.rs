//! Error types of the domain, one variant per reason.
//!
//! A caller branches on the kind of failure, so no error here is a message
//! string or a list of strings. Each constructor of a value with rules returns
//! one of these enums and keeps the cause (`#[source]`) when there is one.

use std::num::ParseIntError;

use thiserror::Error;

use super::approval::Gate;
use super::boundary::Leak;
use super::corpus::Invalidity;
use super::event::{DocKind, EventId};
use super::hash::{BytesHash, ContentHash, TextHash};
use super::id::{DocId, EntityId, ShortId};
use super::judge::Judge;
use super::judgement::{JudgementId, Target};
use super::location::RelPath;
use super::naming::{Place, RefForm};
use super::nonempty::NonEmptyVec;
use super::publication::LeakRemedy;
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
    /// A pin key has too few bytes to be a key.
    #[error("a pin key has at least 16 bytes, found {len}")]
    PinKeyTooShort {
        /// Number of bytes found.
        len: usize,
    },
    /// A key id is empty, too long, or holds a character outside `a-z 0-9 . _ -`.
    #[error("a key id is 1 to 64 characters of lowercase letters, digits and `. _ -`")]
    KidBadShape,
    /// A hunk scheme name is empty, too long, or holds a character outside
    /// `a-z 0-9 . _ -`.
    #[error("a hunk scheme name is 1 to 64 characters of lowercase letters, digits and `. _ -`")]
    SchemeNameBadShape,
    /// A keyring lists one key id twice.
    #[error("the key id `{kid}` is in the keyring twice")]
    KidDuplicate {
        /// The key id.
        kid: String,
    },
    /// The digest of a record was not 16 lowercase hex characters.
    #[error("the digest of a record is 16 lowercase hex characters")]
    RecordDigestShape,
    /// An identifier text was not a UUID.
    #[error("an entity id is a UUID")]
    IdNotUuid(#[source] uuid::Error),
    /// An identifier text was a UUID, but not in the one form an identifier is
    /// written in: lowercase, with hyphens.
    #[error("an entity id is written as a lowercase hyphenated UUID")]
    IdNotCanonical,
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
    /// An alias was 8 to 32 hexadecimal characters, which a short id is written as.
    ///
    /// Refused so that a bare hexadecimal text is never ambiguous between an alias
    /// and a short id. Fix: add a hyphen or a letter outside `a-f`.
    #[error(
        "an alias is not 8 to 32 hexadecimal characters, which is how a short id is written; found {len}"
    )]
    AliasLooksLikeShortId {
        /// Characters found.
        len: usize,
    },
    /// A name of a project (in an `ext:` reference) was malformed.
    #[error("a project name holds lowercase letters, digits and `. _ -`")]
    ProjectNameBadShape,
    /// A PIN identifier is not `<PREFIX>-<NAME>` with a prefix of the vocabulary it
    /// was read in (by default `PIN` or `UT`).
    #[error(
        "a pin id looks like <PREFIX>-<NAME> with a prefix of the pin vocabulary (by default PIN or UT) and a name of A-Z 0-9 _ -"
    )]
    PinIdBadShape,
    /// A pin prefix is not capital letters and digits starting with a letter.
    #[error("a pin prefix is capital letters and digits, starting with a letter")]
    PinPrefixBadShape,
    /// A pin prefix is one that names something else (`HYP`, a hypothesis).
    #[error("`{prefix}` is not available as a pin prefix: it names something else")]
    PinPrefixReserved {
        /// The prefix refused.
        prefix: String,
    },
    /// A pin vocabulary lists the same prefix twice.
    #[error("the pin prefix `{prefix}` is listed twice")]
    PinPrefixDuplicate {
        /// The prefix listed twice.
        prefix: String,
    },
    /// An external tag is empty or holds white space.
    #[error("an external tag is a non-empty text without white space")]
    ExternalTagBadShape,
    /// A pin vocabulary maps an external tag to a pin whose prefix it does not list.
    #[error(
        "the external tag `{tag}` is mapped to `{pin}`, whose prefix the vocabulary does not list"
    )]
    PinMappingOutsideVocabulary {
        /// The external tag.
        tag: String,
        /// The pin it is mapped to.
        pin: String,
    },
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
    /// A short id had fewer characters than the least that cannot match by accident.
    #[error("a short id has at least {min} hexadecimal characters, found {len}")]
    ShortIdTooShort {
        /// Characters found.
        len: usize,
        /// The least.
        min: usize,
    },
    /// A short id had more characters than the hexadecimal digits of a UUID.
    #[error("a short id has at most {max} hexadecimal characters, found {len}")]
    ShortIdTooLong {
        /// Characters found.
        len: usize,
        /// The most.
        max: usize,
    },
    /// A short id held a character that is not lowercase hexadecimal.
    #[error("a short id is lowercase hexadecimal, found `{ch}`")]
    ShortIdBadChar {
        /// The offending character.
        ch: char,
    },
    /// A store name was neither `repo` nor `private`.
    #[error("a store is `repo` or `private`")]
    StoreKind,
    /// A time limit was zero.
    #[error("a time limit is longer than zero")]
    TimeoutZero,
    /// A project id was not a UUID.
    #[error("a project id is a UUID")]
    ProjectIdNotUuid(#[source] uuid::Error),
    /// A glob of `test.files` was refused.
    #[error("the test file glob `{glob}` is refused: {reason}")]
    TestGlob {
        /// The glob as written.
        glob: String,
        /// The rule it breaks.
        reason: GlobProblem,
    },
    /// A word of a closed vocabulary was not one of its names.
    #[error("`{found}` is not a {what}")]
    UnknownName {
        /// What the word should have named, such as "maturity level".
        what: &'static str,
        /// The text found.
        found: String,
    },
}

/// The rule a glob of `test.files` breaks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum GlobProblem {
    /// The glob is empty or only white space.
    #[error("it is empty")]
    Empty,
    /// The glob starts with `/` and so names a place outside the project.
    #[error("it is absolute; globs are relative to the project root")]
    Absolute,
    /// The glob climbs out of the project with a `..` segment.
    #[error("it has a `..` segment")]
    Climbs,
    /// The glob starts with `:` and so would be read as a pathspec magic of its own.
    #[error("it starts with `:`; the magic is added by whyshelf")]
    Magic,
    /// The glob has a control character or a backslash.
    #[error("it has a control character or a backslash")]
    BadChar,
    /// The list of globs is empty, which would leave no test file.
    #[error("the list is empty, which would leave no test file")]
    NoGlobs,
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

/// Why a header and an entity do not make one
/// [`Document`](super::corpus::Document).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum DocumentError {
    /// The header names one kind and the entity is another.
    #[error("the header says {header}, and the entity is a {entity}")]
    KindMismatch {
        /// The kind the header names.
        header: DocKind,
        /// The kind of the entity.
        entity: DocKind,
    },
    /// The header and the entity carry different identities.
    #[error("the header has the id {header}, and the entity has {entity}")]
    IdMismatch {
        /// The id the header names.
        header: DocId,
        /// The id the entity carries.
        entity: DocId,
    },
}

impl DocumentError {
    /// The key of the file the disagreement is about: `kind` or `id`.
    pub fn field(&self) -> &'static str {
        match self {
            Self::KindMismatch { .. } => "kind",
            Self::IdMismatch { .. } => "id",
        }
    }
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

/// Why a text is not a reference where it was written.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum RefError {
    /// The text was empty.
    #[error("a reference is empty")]
    Empty,
    /// A hyphenated UUID that is not an id (an uppercase one, one of another version).
    #[error("a reference that looks like a UUID is not an id")]
    Id(#[source] ValueError),
    /// A short id that is malformed (too short, too long, not lowercase hexadecimal).
    #[error("a short id is malformed")]
    Short(#[source] ValueError),
    /// The text is neither an alias nor a path.
    #[error("a reference is an id, a short id, an alias or a path")]
    Name(#[source] ValueError),
    /// `ext:` was not followed by `<project>/<id>`.
    #[error("an external reference is `ext:<project>/<id>`")]
    ExtShape,
    /// The project of an `ext:` reference is malformed.
    #[error("the project of an external reference is malformed")]
    Project(#[source] ValueError),
    /// The id of an `ext:` reference is malformed.
    #[error("the id of an external reference is malformed")]
    ExtId(#[source] ValueError),
    /// What follows `private:` is neither an id nor a short id.
    #[error("`private:` is followed by an id or a short id")]
    PrivateTarget(#[source] ValueError),
    /// A URL is malformed.
    #[error("a URL is malformed")]
    Url(#[source] ValueError),
    /// What follows `#` is neither digits nor `id:` and a short id.
    #[error("an item is named `#<number>` or `#id:<short id>`, found `#{text}`")]
    ItemSelector {
        /// The text after the `#`.
        text: String,
    },
    /// The number after `#` is not a positive number that fits.
    #[error("an item number is a positive number")]
    ItemNumber(#[source] ValueError),
    /// A short id stands where a document names its upstream or another document.
    ///
    /// Fix: write the full id or the alias.
    #[error("a short id (`{short}`) is not accepted in {place}; write the full id or the alias")]
    ShortIdInDocument {
        /// The short id.
        short: ShortId,
        /// Where it was written.
        place: Place,
    },
    /// A form that does not belong in the place it was written.
    #[error("a {form} is not accepted in {place}")]
    NotAllowedHere {
        /// The form.
        form: RefForm,
        /// Where it was written.
        place: Place,
    },
}

/// Why text may not be written where others read it.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum LeakRefusal {
    /// The check cannot run: this person's private root is set and cannot be read.
    #[error("the leak check cannot run: the private root is set and cannot be read")]
    Uncheckable,
    /// The text names private documents.
    #[error("the text names {} private name(s)", .0.len())]
    Leaks(NonEmptyVec<Leak>),
}

/// Why a publication cannot be planned.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum PublicationError {
    /// No file of the corpus has that id.
    #[error("{doc} is not in the corpus")]
    NotFound {
        /// The document.
        doc: DocId,
    },
    /// The document is public already, and no publication of it is open.
    #[error("{doc} is public already")]
    AlreadyPublic {
        /// The document.
        doc: DocId,
    },
    /// No publication of the document is open, so there is nothing to abort: the
    /// document is in one root only.
    #[error("no publication of {doc} is open")]
    NoneOpen {
        /// The document.
        doc: DocId,
    },
    /// The publication has its `Published` record, which no revocation takes away, so
    /// the only way on is to finish it: nothing of it is undone.
    #[error("the publication of {doc} is already recorded: finish it, it cannot be aborted")]
    AlreadyRecorded {
        /// The document.
        doc: DocId,
    },
    /// The document cannot be trusted as it stands (a private file Git would add is
    /// not among the reasons: that is what a publication ends).
    #[error("{doc} cannot be published: it is invalid as it stands")]
    DocInvalid {
        /// The document.
        doc: DocId,
        /// Why.
        reasons: NonEmptyVec<Invalidity>,
    },
    /// A file stands at the destination already.
    #[error("a file is at {to} already")]
    DestinationTaken {
        /// The destination.
        to: RelPath,
    },
    /// An open publication has its public file elsewhere.
    #[error("the public file of the open publication is at {file}, not at {to}")]
    DestinationDiffers {
        /// Where the public file is.
        file: RelPath,
        /// The destination asked for.
        to: RelPath,
    },
    /// The text of a version to be copied was not given, and its record has not been
    /// purged: it can neither be copied nor checked.
    #[error("the object {text} is neither purged nor given")]
    ObjectUnavailable {
        /// The hash of the text.
        text: TextHash,
    },
    /// The bytes the leak check read are not the bytes of the file that would be
    /// copied: the file changed since they were read. Read it again and plan again.
    #[error("the file is {expected} now, and the text checked was {found}")]
    EvidenceStale {
        /// The bytes the corpus has for the file.
        expected: BytesHash,
        /// The bytes that were given to be checked.
        found: BytesHash,
    },
    /// The leak check cannot run, and nothing is written to the public ledger.
    #[error("the leak check cannot run: the private root is set and cannot be read")]
    LeakUncheckable,
    /// Text that would cross names private documents. Nothing is planned.
    #[error("{} private name(s) would cross", .leaks.len())]
    Leaks {
        /// Every leak found.
        leaks: NonEmptyVec<Leak>,
        /// What to do about it.
        remedy: LeakRemedy,
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
    /// Two records carry one event identifier but are not the same event.
    ///
    /// The identifier covers the kind, the target, the heads and the part of the
    /// body that decides the change, so records of one change agree on the whole
    /// event; if they do not, one of them was altered or came from another
    /// history.
    #[error("two records of the event {id} do not describe the same event")]
    RecordsDiffer {
        /// The identifier both records carry.
        id: EventId,
    },
}

/// Why the fingerprint of a change could not be built.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum FingerprintError {
    /// The change has no hunk, so there is nothing to find again in the baseline.
    ///
    /// An approval of an item whose change is empty has no fingerprint; it is
    /// recorded without one, and its landing is told by the ancestry alone.
    #[error("a change with no hunk has no fingerprint")]
    NoHunks,
    /// The scheme is one this build does not have, so it cannot cut a change into
    /// hunks. A fingerprint under such a scheme can only be read back, never made.
    #[error("this build cannot cut a change into hunks under the scheme `{scheme}`")]
    UnknownScheme {
        /// The name of the scheme.
        scheme: String,
    },
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

/// Why a name, a mail or a tool name of a [`Judge`] could not be built.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum JudgeError {
    /// A name was empty.
    #[error("a judge's name is empty")]
    NameEmpty,
    /// A name held a control character or was too long.
    #[error("a judge's name is one short line")]
    NameBadShape,
    /// A mail address was not `local@domain`.
    #[error("a mail address is `local@domain`, with no white space")]
    EmailBadShape,
    /// The name of a tool was empty.
    #[error("a tool's name is empty")]
    ToolEmpty,
    /// The name of a tool held a control character or was too long.
    #[error("a tool's name is one short line")]
    ToolBadShape,
}

/// Why a judgement could not be built, or could not be given a ledger.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum JudgementError {
    /// The object kept with an approval is not of the version the approval is about.
    #[error("the object is of {object}, and the approval is about {target}")]
    ObjectNotOfTarget {
        /// The content hash the object was given for.
        object: ContentHash,
        /// The hash the approval pins.
        target: ContentHash,
    },
    /// The object kept with an approval or an adoption is not the text its check
    /// read.
    #[error("the object is the text {object}, and the check read {checked}")]
    ObjectNotChecked {
        /// The text hash the object was given for.
        object: TextHash,
        /// The hash of the text the check read.
        checked: TextHash,
    },
    /// An approval or an adoption was asked of a proof that was read back from a
    /// ledger: it says what a check found when an earlier record was written, and a
    /// new record stands on a check made for it.
    #[error("the proof was read back from a ledger; a new record needs a check of its own")]
    ProofRestored,
    /// A retirement was asked for a document or item whose file is there.
    #[error("{target} is present; only what is missing can be retired")]
    TargetPresent {
        /// What was to be retired.
        target: Target,
    },
    /// A publication named a record the public ledger does not hold.
    #[error("the record {record} is not in the public ledger")]
    RecordNotInPublicLedger {
        /// The record.
        record: JudgementId,
    },
    /// A publication named a record that is not an approval, an adoption or a
    /// retirement of the document published or of one of its items.
    #[error("the record {record} is not an approval, adoption or retirement of {doc} or its items")]
    RecordNotOfDoc {
        /// The record.
        record: JudgementId,
        /// The document published.
        doc: DocId,
    },
    /// A publication named a record that a standing revocation takes away.
    #[error("the record {record} is revoked; a publication does not carry it")]
    RecordRevoked {
        /// The record.
        record: JudgementId,
    },
    /// Neither the files nor either ledger know what the judgement is about.
    #[error("nothing is known of {target}: no file, and no record in either ledger")]
    UnknownTarget {
        /// What the judgement is about.
        target: Target,
    },
    /// The files that carry the target sit under both roots, so the ledger is not
    /// settled.
    #[error("{target} sits under both roots; the ledger is not settled")]
    AmbiguousTarget {
        /// What the judgement is about.
        target: Target,
    },
    /// A revocation named a record that is in neither ledger.
    #[error("the record {record} is in neither ledger")]
    UnknownRecord {
        /// The record.
        record: JudgementId,
    },
    /// A revocation named a record that is not an approval, an adoption or a
    /// retirement.
    #[error("the record {record} cannot be revoked")]
    NotRevocable {
        /// The record.
        record: JudgementId,
    },
    /// A revocation named records about different things, which no single ledger
    /// holds.
    #[error("the records revoked are about more than one thing")]
    RevokeSpansTargets,
}

/// Why a revocation could not be built.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum RevokeError {
    /// The attestation or retirement named has no record that is visible and not
    /// revoked, by the one named.
    #[error("nothing to revoke: no visible, unrevoked record matches")]
    NothingToRevoke,
    /// A record named is in neither ledger.
    #[error("the record {record} is in neither ledger")]
    UnknownRecord {
        /// The record.
        record: JudgementId,
    },
    /// A record named is not an approval, an adoption or a retirement. A revocation,
    /// a purge and a publication stand; an approval is made again, an object is put
    /// again.
    #[error("the record {record} cannot be revoked")]
    NotRevocable {
        /// The record.
        record: JudgementId,
    },
    /// A record named is revoked already.
    #[error("the record {record} is revoked already")]
    AlreadyRevoked {
        /// The record.
        record: JudgementId,
    },
    /// A record is another judge's, and the revocation was not forced.
    #[error("the record is {owner}'s; revoking it takes force")]
    RevokeOthers {
        /// Whose record it is.
        owner: Judge,
    },
}

/// Why a past decision could not be taken in as an approval.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum AdoptionRefusal {
    /// The gate records a measurement, which is not taken in.
    #[error("{} records a measurement; a measurement is not adopted", gate.name())]
    GateNotAdoptable {
        /// The gate.
        gate: Gate,
    },
    /// The source was empty.
    #[error("an adoption names where the decision came from")]
    SourceEmpty(#[source] ValueError),
    /// The target is in no file of the corpus.
    #[error("{target} is not in the corpus")]
    TargetUnknown {
        /// What was to be adopted.
        target: Target,
    },
    /// The target is in the corpus and cannot be trusted.
    #[error("{target} is invalid as it stands")]
    TargetInvalid {
        /// What was to be adopted.
        target: Target,
    },
    /// The gate does not approve the kind of the target.
    #[error("{} does not approve {target}", gate.name())]
    GateTargetMismatch {
        /// The gate.
        gate: Gate,
        /// What was to be adopted.
        target: Target,
    },
    /// An upstream the approval would pin is not there to pin, or is not what the
    /// gate stands on.
    #[error("the upstream {upstream} cannot be pinned")]
    UpstreamUnavailable {
        /// The upstream.
        upstream: EntityId,
    },
    /// An upstream the approval would pin is private, and there is no key to pin it
    /// under: its hash must not be written plain.
    #[error("the upstream {upstream} is private and there is no key to pin it under")]
    UpstreamNeedsKey {
        /// The upstream.
        upstream: EntityId,
    },
}
