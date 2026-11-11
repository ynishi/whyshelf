//! The JSON form of every answer: one serde type per command, the same
//! type `schema` describes, so the schema and the output cannot drift apart.
//!
//! A report of the use cases holds domain values; this module writes them as data
//! an agent can read without knowing Rust: a document is `{id, key}`, a state is a
//! word, a cause has a `kind` word, the document it names, and a sentence.

use schemars::{JsonSchema, Schema, schema_for};
use serde::Serialize;
use serde_json::Value;
use whyshelf_core::app::shelf::{
    ApprovedReport, BooksCheck, Created, Declared, DeclaredAbout, DiffItem, DiffReport,
    DownstreamEdge, ErrorClass, FmtReport, GetReport, Health, Initialized, Judged, JudgedAll,
    LeftAloneWhy, LeftOut, LeftOutWhy, MapReport, MovesReport, Names, Row, ShelfError,
    StaleApproved, StaleReport, StatsReport, StatusReport, TraceReport, UnlinkedEntry,
    UnlinkedReport, UpstreamEdge, UpstreamEnd,
};
use whyshelf_core::domain::location::RelPath;
use whyshelf_core::domain::shelf::build::NewDocProblem;
use whyshelf_core::domain::shelf::doc::DocKey;
use whyshelf_core::domain::shelf::schema::BookName;
use whyshelf_core::domain::shelf::snapshot::{
    ItemNow, ItemRole, Retrieval, SeenHash, SnapshotItem,
};
use whyshelf_core::domain::shelf::stats::Origin as ChangedPin;

use super::textdiff;
use whyshelf_core::domain::id::DocId;
use whyshelf_core::domain::shelf::artifact::{Artifact, ArtifactKey, ArtifactState, Comparison};
use whyshelf_core::domain::shelf::build::{DocFailure, DocPresence};
use whyshelf_core::domain::shelf::doc::{
    Doc, FieldValue, LinkState, UnresolvedLink, UnresolvedWhy, normalize,
};
use whyshelf_core::domain::shelf::freshness::{
    Cause, Freshness, FreshnessLevel, Notice, Origin, RootCause, Severity, Standing, StandingKind,
    Successors, UpstreamStatus,
};
use whyshelf_core::domain::shelf::graph::CycleSource;
use whyshelf_core::domain::shelf::ledger::{
    ArtifactPin, DeclaredStatus, FoundBy, FramePin, FrameRefusal, MoveNote, Moved, ReadPin, Record,
    Similarity,
};
use whyshelf_core::domain::shelf::map::MapTarget;
use whyshelf_core::domain::shelf::order::Hold;
use whyshelf_core::domain::shelf::schema::{
    Cardinality, CoverSeverity, EntryName, FieldPresence, FieldType, KeyPattern, PresetOrigin,
    Propagation, SchemaProblem,
};
use whyshelf_infra::shelf::ledger::to_json;

/// A document named in an answer: its identity, and its key when it is valid.
#[derive(Debug, Serialize, JsonSchema)]
pub struct DocRef {
    /// The identity (UUID v7).
    pub id: String,
    /// The key (`book:KEY`), when the document is valid.
    pub key: Option<String>,
}

/// What the shelf has for a document now.
#[derive(Debug, Serialize, JsonSchema)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum PresenceOut {
    /// A valid document.
    Valid {
        /// Its file.
        file: String,
        /// Its hash now.
        hash: String,
    },
    /// The files that carry the identity are invalid.
    Invalid {
        /// The files.
        files: Vec<String>,
        /// What is wrong.
        problems: Vec<String>,
    },
    /// No file carries it.
    Gone,
    /// Unknown: some file's identity could not be read.
    Unknown {
        /// The files whose identity could not be read.
        unreadable: Vec<String>,
    },
}

/// A cause of an approval that does not hold.
#[derive(Debug, Serialize, JsonSchema)]
pub struct CauseOut {
    /// What kind of cause (`self_changed`, `upstream_changed`, `upstream_declared`,
    /// `via`, `expired`, ...).
    pub kind: &'static str,
    /// How strong: `expired`, `stale` or `broken`.
    pub severity: &'static str,
    /// The upstream document the cause names, if it names one.
    pub upstream: Option<DocRef>,
    /// The link it was reached through, if any.
    pub link: Option<String>,
    /// The name of the artifact the cause names, if it names one
    /// (`artifact_changed`, `artifact_gone`, `artifact_arrived`,
    /// `artifact_comparison_changed`): for the space `file`, its path.
    pub artifact: Option<String>,
    /// The space of that artifact, beside `artifact`.
    pub space: Option<String>,
    /// The name `artifact` is read as now, when people declared it moved since it
    /// was pinned (`judge moved`); none when it is read as written. For
    /// `artifact_moved`, `artifact` is the name the link writes and `read_as` the
    /// name it is to write.
    pub read_as: Option<String>,
    /// A sentence for a person.
    pub detail: String,
}

/// Something worth knowing that changes no freshness.
#[derive(Debug, Serialize, JsonSchema)]
pub struct NoticeOut {
    /// What kind (`informs_not_fresh`, `informs_unresolved`, `upstream_expired`,
    /// `self_same_meaning`, `upstream_same_meaning`, `artifact_same_meaning`,
    /// `not_covered`, `artifact_absent`, `artifact_unknown`).
    pub kind: &'static str,
    /// For `not_covered`, how strongly the link expects to reach the document:
    /// `warn` or `error`; null for any other kind.
    pub severity: Option<&'static str>,
    /// The link; empty for a notice about the document itself.
    pub link: String,
    /// The document it names, if it names one.
    pub target: Option<DocRef>,
    /// The name of the artifact it names, if it names one: for the space `file`, a
    /// path under the root.
    pub artifact: Option<String>,
    /// The space of that artifact, beside `artifact`.
    pub space: Option<String>,
    /// For `artifact_same_meaning`, the name the pin of `artifact` is read as now,
    /// when people declared it moved since (`judge moved`); none otherwise.
    pub read_as: Option<String>,
    /// For an edit declared to keep the meaning, the declarations (record ids) that
    /// join the hash pinned to the hash now, in order; empty otherwise.
    pub declared: Vec<String>,
    /// A sentence for a person.
    pub detail: String,
}

/// Who made a decision, when and why, as its record keeps it.
#[derive(Debug, Serialize, JsonSchema)]
pub struct DecisionOut {
    /// The record's id (`judge withdraw` takes it).
    pub judgement: String,
    /// Who decided.
    pub actor: String,
    /// When (unix milliseconds).
    pub at: i64,
    /// Why, as it was given; none when no reason was given.
    pub reason: Option<String>,
}

/// A document as every answer shows it.
#[derive(Debug, Serialize, JsonSchema)]
pub struct RowOut {
    /// The document.
    pub doc: DocRef,
    /// What the shelf has for it.
    pub presence: PresenceOut,
    /// What people decided: `pending`, `approved`, `denied`, `deprecated`, `retired`.
    pub declared: &'static str,
    /// The decision that stands, the one `declared` is read from; none while the
    /// document is pending.
    pub decision: Option<DecisionOut>,
    /// When the approval lapses (unix milliseconds), if it does.
    pub expires: Option<i64>,
    /// Where it stands now: `pending`, `denied`, `deprecated`, `retired`, or for an
    /// approval `fresh`, `expired`, `stale`, `broken`.
    pub standing: &'static str,
    /// For a deprecated document, what replaces it.
    pub successors: Vec<DocRef>,
    /// Why an approval does not hold.
    pub causes: Vec<CauseOut>,
    /// What is worth knowing that changes no freshness.
    pub notices: Vec<NoticeOut>,
}

/// A file that is no valid document.
#[derive(Debug, Serialize, JsonSchema)]
pub struct InvalidOut {
    /// The file.
    pub file: String,
    /// Its identity, if it could be read.
    pub id: Option<String>,
    /// What is wrong.
    pub problems: Vec<String>,
}

/// A file of the ledger that is no record.
#[derive(Debug, Serialize, JsonSchema)]
pub struct UnreadableOut {
    /// The file.
    pub file: String,
    /// Why.
    pub why: String,
}

/// What is wrong around the documents.
#[derive(Debug, Serialize, JsonSchema)]
pub struct HealthOut {
    /// Whether nothing is.
    pub clean: bool,
    /// Files that are no valid document; those whose front matter or identity
    /// could not be read follow the order the declaration writes the books in.
    pub invalid: Vec<InvalidOut>,
    /// Cycles among links or among the frames of approvals.
    pub cycles: Vec<Vec<DocRef>>,
    /// Records of the ledger that contradict one another.
    pub ledger_problems: Vec<String>,
    /// Files of the ledger that are no record.
    pub unreadable_records: Vec<UnreadableOut>,
}

/// One book, counted.
#[derive(Debug, Serialize, JsonSchema)]
pub struct BookOut {
    /// The name.
    pub name: String,
    /// The directory.
    pub dir: String,
    /// Valid documents.
    pub valid: usize,
    /// Files under it that are no valid document.
    pub invalid: usize,
}

/// The answer of `check`.
///
/// Beside it, on stderr, `check` writes one line per entry of `uncovered` of
/// strength `warn` ([`check_warnings`]), since such a gap leaves the check clean
/// and a run that shows the answer only when it fails would show it nowhere.
#[derive(Debug, Serialize, JsonSchema)]
pub struct BooksCheckOut {
    /// Whether nothing is wrong.
    pub clean: bool,
    /// The problems of the declaration; when there are any, nothing else was read.
    pub schema_problems: Vec<String>,
    /// The books, in the order the declaration writes them.
    pub books: Vec<BookOut>,
    /// What is wrong around the documents.
    pub health: Option<HealthOut>,
    /// The documents a covering link is expected to reach and does not, each with
    /// the strength of the link; which strength makes `clean` false is
    /// [`BooksCheck::is_clean`]'s rule.
    pub uncovered: Vec<UncoveredOut>,
    /// The names of pending and approved documents with nothing there (for the
    /// space `file`, paths that name no file); they leave `clean` as it is, since a
    /// name may be of evidence not written yet.
    pub absent: Vec<AbsentOut>,
    /// The names of pending and approved documents their adapter cannot tell about
    /// now; they leave `clean` as it is, and no approval can be made on one.
    pub unknown: Vec<UnknownOut>,
    /// The names links still write that an approval pinned and people declared
    /// moved since (`judge moved`): the approval is stale, and the link is to write
    /// `moved_to`. They leave `clean` as it is.
    pub moved: Vec<MovedNameOut>,
}

/// A name a link writes that an approval pinned, declared moved since.
#[derive(Debug, Serialize, JsonSchema)]
pub struct MovedNameOut {
    /// The document.
    pub doc: DocRef,
    /// The link.
    pub link: String,
    /// The name as written, the one pinned.
    pub name: String,
    /// The space of the name.
    pub space: String,
    /// The name the pin is read as now, which the link is to write.
    pub moved_to: String,
    /// The declarations of the moves (record ids), in order.
    pub declared: Vec<String>,
    /// A sentence for a person.
    pub detail: String,
}

/// The answer of `map`: what the declaration says.
#[derive(Debug, Serialize, JsonSchema)]
pub struct MapOut {
    /// The problems of the declaration, as `check` gives them; when there are any,
    /// there is no book, no link and no drawing.
    pub schema_problems: Vec<String>,
    /// The books, in the order of the declaration.
    pub books: Vec<MapBookOut>,
    /// The links, book by book and within a book in the order of the declaration.
    pub links: Vec<MapLinkOut>,
    /// The books and links drawn as text, one block per book; null when the
    /// declaration cannot be used.
    pub diagram: Option<String>,
}

/// A book of `map`.
#[derive(Debug, Serialize, JsonSchema)]
pub struct MapBookOut {
    /// The name.
    pub name: String,
    /// The directory.
    pub dir: String,
    /// The grammar of its keys.
    pub key: KeyGrammarOut,
    /// What it is for, when the declaration says.
    pub description: Option<String>,
    /// Its fields, in the order of the declaration.
    pub fields: Vec<MapFieldOut>,
}

/// The grammar of the keys of a book.
#[derive(Debug, Serialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum KeyGrammarOut {
    /// `<prefix>-<number or name>`.
    Prefix {
        /// The prefix, without the `-`.
        prefix: String,
    },
    /// A path of lowercase words.
    Slug,
}

/// A field of a book of `map`.
#[derive(Debug, Serialize, JsonSchema)]
pub struct MapFieldOut {
    /// The name.
    pub name: String,
    /// The type as the declaration writes it: `text`, `integer`, `bool`, `list` or
    /// `enum`.
    #[serde(rename = "type")]
    pub ty: &'static str,
    /// The words of an `enum`, sorted, not in the order written: the model keeps
    /// them as a set, as a pin's canon does. Empty for any other type.
    pub words: Vec<String>,
    /// Whether a document must carry it.
    pub required: bool,
}

/// A link of `map`.
#[derive(Debug, Serialize, JsonSchema)]
pub struct MapLinkOut {
    /// The book that holds it.
    pub from: String,
    /// The name.
    pub name: String,
    /// What it points at.
    pub target: MapTargetOut,
    /// `depends`, `informs` or `replaces`.
    pub means: &'static str,
    /// How many a document writes, as the declaration writes it: `one`,
    /// `optional`, `many` or `at-least-one`.
    pub count: &'static str,
    /// Whether every pending or approved document of the target book is expected
    /// to be reached by it, of either strength; false for a link to artifacts.
    pub cover: bool,
    /// How strongly it is expected, beside `cover`: `warn` or `error`; null when
    /// `cover` is false.
    pub cover_severity: Option<&'static str>,
    /// The name of the preset a link to artifacts was expanded from, the one it
    /// names or the default; null when there is none, and for a link into a book.
    /// The values in `target` are the link's, whichever it is.
    pub preset: Option<String>,
    /// Whether `preset` is the default (`[defaults] artifact`), beside `preset`:
    /// false when the link names its preset, or has none.
    pub preset_by_default: bool,
}

/// What a link of `map` points at.
#[derive(Debug, Serialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum MapTargetOut {
    /// The documents of a book (which may be the book that holds the link).
    Book {
        /// The book.
        book: String,
    },
    /// Artifacts, by name in a space.
    Artifact {
        /// The space.
        space: String,
        /// What of each is compared: `bytes`, `lines` or `presence`.
        compare: &'static str,
        /// The prefixes of the lines `lines` leaves out, sorted, not in the order
        /// written: the model keeps them as a set, as a pin's canon does.
        ignore: Vec<String>,
    },
}

/// A name a link to artifacts writes with nothing there.
#[derive(Debug, Serialize, JsonSchema)]
pub struct AbsentOut {
    /// The document.
    pub doc: DocRef,
    /// The link.
    pub link: String,
    /// The name as written (kept from when every name was a path; the same as
    /// `name`).
    pub path: String,
    /// The name as written.
    pub name: String,
    /// The space of the name.
    pub space: String,
    /// A sentence for a person.
    pub detail: String,
}

/// A name a link to artifacts writes that its adapter cannot tell about now.
#[derive(Debug, Serialize, JsonSchema)]
pub struct UnknownOut {
    /// The document.
    pub doc: DocRef,
    /// The link.
    pub link: String,
    /// The name as written.
    pub name: String,
    /// The space of the name.
    pub space: String,
    /// Why, as the adapter says.
    pub why: String,
}

/// A document a covering link is expected to reach and does not.
#[derive(Debug, Serialize, JsonSchema)]
pub struct UncoveredOut {
    /// The document.
    pub doc: DocRef,
    /// The book whose link is expected to reach it.
    pub book: String,
    /// The link.
    pub link: String,
    /// How strongly the link expects to reach it: `warn` or `error`.
    pub severity: &'static str,
}

/// The answer of `status`.
#[derive(Debug, Serialize, JsonSchema)]
pub struct StatusOut {
    /// Every document.
    pub docs: Vec<RowOut>,
    /// What is wrong around them.
    pub health: HealthOut,
}

/// Where the failure of an approval starts.
#[derive(Debug, Serialize, JsonSchema)]
pub struct RootOut {
    /// The documents from the one asked about to the one where it starts.
    pub path: Vec<DocRef>,
    /// What happened there.
    pub cause: CauseOut,
}

/// A link of a document, with what it names.
#[derive(Debug, Serialize, JsonSchema)]
pub struct LinkOut {
    /// The name of the link.
    pub name: String,
    /// What it names: documents, or keys that cannot be followed.
    pub targets: Vec<LinkTargetOut>,
}

/// What one key of a link names.
#[derive(Debug, Serialize, JsonSchema)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum LinkTargetOut {
    /// A document.
    Resolved {
        /// The document.
        doc: DocRef,
    },
    /// A key that cannot be followed.
    Unresolved {
        /// The key as written.
        written: String,
        /// Why.
        why: String,
    },
    /// An artifact (a link with `target = "artifact"`).
    Artifact {
        /// Its name (kept from when every name was a path; the same as `name`).
        path: String,
        /// Its name as written.
        name: String,
        /// Its space.
        space: String,
        /// What it is now: `present`, `absent`, or `unknown` (its adapter cannot tell
        /// now).
        now: &'static str,
        /// The hash of its content now, when `now` is `present`; none otherwise.
        hash: Option<String>,
    },
}

/// A document's content.
#[derive(Debug, Serialize, JsonSchema)]
pub struct DocOut {
    /// The fields, by name.
    pub fields: serde_json::Map<String, Value>,
    /// The links.
    pub links: Vec<LinkOut>,
    /// The body.
    pub body: String,
}

/// The answer of `get`.
#[derive(Debug, Serialize, JsonSchema)]
pub struct GetOut {
    /// The document.
    pub row: RowOut,
    /// Its content, when it is valid.
    pub content: Option<DocOut>,
    /// Where the failure of its approval starts.
    pub roots: Vec<RootOut>,
    /// What is wrong around the documents.
    pub health: HealthOut,
}

/// One link followed in `trace`.
#[derive(Debug, Serialize, JsonSchema)]
pub struct EdgeOut {
    /// The document that holds the link.
    pub from: DocRef,
    /// The link.
    pub link: String,
    /// `depends`, `informs` or `replaces`.
    pub means: &'static str,
    /// How many links away from the document asked about.
    pub depth: usize,
    /// What it names.
    pub to: LinkTargetOut,
    /// Where the other end stands (the end away from the document asked about).
    pub standing: Option<&'static str>,
}

/// The answer of `trace`.
#[derive(Debug, Serialize, JsonSchema)]
pub struct TraceOut {
    /// The document asked about.
    pub root: RowOut,
    /// What it links to, transitively.
    pub upstream: Vec<EdgeOut>,
    /// What links to it, transitively.
    pub downstream: Vec<EdgeOut>,
    /// What is wrong around the documents.
    pub health: HealthOut,
}

/// A document `unlinked` lists.
#[derive(Debug, Serialize, JsonSchema)]
pub struct UnlinkedEntryOut {
    /// The book it is in.
    pub book: String,
    /// The document.
    pub row: RowOut,
}

/// The documents decided against (denied, deprecated, retired) that `unlinked`
/// lists, apart from the others.
#[derive(Debug, Serialize, JsonSchema)]
pub struct DecidedAgainstOut {
    /// Those linked neither way.
    pub isolated: Vec<UnlinkedEntryOut>,
    /// Those no link reaches; null when not asked.
    pub no_incoming: Option<Vec<UnlinkedEntryOut>>,
    /// Those that hold no link; null when not asked.
    pub no_outgoing: Option<Vec<UnlinkedEntryOut>>,
}

/// The answer of `unlinked`: three disjoint lists, by the rule of the core's
/// `Linkage` (the manual says where it is).
#[derive(Debug, Serialize, JsonSchema)]
pub struct UnlinkedOut {
    /// The pending and approved documents linked neither way: the place to look
    /// first. Always answered, whatever the flags.
    pub isolated: Vec<UnlinkedEntryOut>,
    /// The problems of the declaration, as `check` gives them; when there are any,
    /// nothing else was read, every list asked for is empty and `health` is null.
    pub schema_problems: Vec<String>,
    /// The pending and approved documents no link reaches; null when not asked
    /// (`--no-outgoing`).
    pub no_incoming: Option<Vec<UnlinkedEntryOut>>,
    /// The pending and approved documents that hold no link; null when not asked
    /// (`--no-incoming`).
    pub no_outgoing: Option<Vec<UnlinkedEntryOut>>,
    /// The documents decided against, in the same three lists.
    pub decided_against: DecidedAgainstOut,
    /// What is wrong around the documents (the files that are no document are
    /// listed here, not above).
    pub health: Option<HealthOut>,
}

/// One approval that does not hold.
#[derive(Debug, Serialize, JsonSchema)]
pub struct StaleEntryOut {
    /// The document.
    pub row: RowOut,
    /// Where it starts to fail.
    pub roots: Vec<RootOut>,
}

/// The answer of `stale`.
#[derive(Debug, Serialize, JsonSchema)]
pub struct StaleOut {
    /// The approvals that do not hold, strongest first.
    pub stale: Vec<StaleEntryOut>,
    /// Where they start: each document, artifact, cycle or the declaration an event
    /// starts at, with the approvals of `stale` it degrades; the one that degrades
    /// the most first.
    pub origins: Vec<OriginOut>,
    /// The names approved documents pin through a `depends` link that their
    /// adapter cannot tell about now: their freshness is unchanged, whether their
    /// approvals hold cannot be said, and `--exit-code` exits 1 for any.
    pub unknown: Vec<UnknownOut>,
    /// What is wrong around the documents.
    pub health: HealthOut,
}

/// Where the degradations of approvals start, with what they degrade.
#[derive(Debug, Serialize, JsonSchema)]
pub struct OriginOut {
    /// Where it starts.
    pub start: StartOut,
    /// The approvals it degrades, each also an entry of `stale`, by identity.
    pub degraded: Vec<DocRef>,
}

/// The thing an origin is.
#[derive(Debug, Serialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum StartOut {
    /// A document (its key is null when no valid document has it now).
    Doc(DocNamed),
    /// An artifact.
    Artifact(ArtifactNamed),
    /// The declaration of the books: a link to artifacts declares another comparison.
    Declaration,
    /// A cycle (see `health.cycles`).
    Cycle {
        /// `links` or `frames`.
        among: &'static str,
        /// Its smallest member, by identity.
        smallest: DocRef,
    },
}

/// A document an origin names, in `stale` and in `stats`.
#[derive(Debug, Serialize, JsonSchema)]
pub struct DocNamed {
    /// The document.
    pub doc: DocRef,
}

/// An artifact an origin names, in `stale` and in `stats`.
#[derive(Debug, Serialize, JsonSchema)]
pub struct ArtifactNamed {
    /// Its name (kept from when every name was a path; the same as `name`).
    pub path: String,
    /// Its name.
    pub name: String,
    /// Its space.
    pub space: String,
}

impl ArtifactNamed {
    fn of(artifact: &ArtifactKey) -> Self {
        Self {
            path: artifact.name().to_string(),
            name: artifact.name().to_string(),
            space: artifact.space().to_string(),
        }
    }
}

/// The answer of `stats`. The measures and the order of every list are defined on
/// the page of `whyshelf_core::domain::shelf::stats`.
#[derive(Debug, Serialize, JsonSchema)]
pub struct StatsOut {
    /// The `--since` as it was given; null without it.
    pub since: Option<String>,
    /// The approvals, revoked ones included, per document and per book.
    pub approvals: ApprovalsOut,
    /// Per book, the milliseconds from a document's identity to its first approval
    /// that is not revoked.
    pub pending: Vec<BookPendingOut>,
    /// The runs of `judge`: the decisions that share their time and their actor.
    pub runs: RunsOut,
    /// The documents and artifacts whose pinned hash differed under an approval made
    /// again from the approval before it, the most counted first.
    pub origins: Vec<OriginCountOut>,
    /// What is wrong around the documents.
    pub health: HealthOut,
    /// The same measures as text for a person (`jq -r .drawing`), made from this
    /// answer alone, so the same answer is the same text.
    pub drawing: String,
}

/// The answer of `moves`: the names approvals pinned something at that have nothing
/// behind them now, with where each may have moved. It writes nothing.
#[derive(Debug, Serialize, JsonSchema)]
pub struct MovesOut {
    /// Every name a pin of something there, of an approval that stands, is read as
    /// now (through the moves declared) that has nothing behind it now, by space and
    /// then by name.
    pub absent: Vec<MovesAbsentOut>,
    /// The spaces of those names whose adapter cannot suggest, sorted.
    pub cannot_suggest: Vec<String>,
    /// What is wrong around the documents.
    pub health: HealthOut,
    /// The same as text for a person (`jq -r .drawing`), made from this answer alone.
    pub drawing: String,
}

/// One gone name in `moves`.
#[derive(Debug, Serialize, JsonSchema)]
pub struct MovesAbsentOut {
    /// The space.
    pub space: String,
    /// The name, as pins are read now: for the space `file`, a path under the root.
    /// The old name of a `judge moved`.
    pub name: String,
    /// The documents whose approvals that stand pin it, by identity.
    pub pinned_by: Vec<DocRef>,
    /// Where it may have moved: `same_content` first, then `renamed` from the most
    /// alike, then by name; empty when the adapter found none or cannot suggest.
    pub candidates: Vec<MovesCandidateOut>,
    /// What could not be looked at, or why nothing is suggested, in a fixed order of
    /// kinds, each kind once; empty when the adapter looked every way it has.
    pub notes: Vec<MovesNoteOut>,
}

/// A note of a gone name in `moves`.
#[derive(Debug, Serialize, JsonSchema)]
pub struct MovesNoteOut {
    /// One of a closed set: `in_two_spaces` (pins are read as the name in more than
    /// one space; `judge moved` refuses it, so nothing is suggested),
    /// `no_repository`, `no_revision` (no approval that pins it kept one),
    /// `unknown_revision`, `history_unreadable` (renames were not looked for, for
    /// each reason), `scan_truncated`, `unreadable_skipped`, `too_large_skipped`
    /// (the search for the same content stopped at a bound, or passed files over).
    pub kind: &'static str,
    /// A sentence for a person.
    pub detail: String,
}

/// A name a gone artifact may have moved to.
#[derive(Debug, Serialize, JsonSchema)]
pub struct MovesCandidateOut {
    /// The new name: the new name of a `judge moved`.
    pub name: String,
    /// How it was found: `same_content` (what is there has the hash pinned, under the
    /// pin's comparison) or `renamed` (the space's history pairs the two names).
    pub found: &'static str,
    /// For `renamed`, how alike the two are, a percent, when the history says (Git's
    /// similarity score); null otherwise.
    pub similarity: Option<u8>,
    /// Whether something is at the new name now, as the adapter says.
    pub present: bool,
}

/// The approvals, per document and per book.
#[derive(Debug, Serialize, JsonSchema)]
pub struct ApprovalsOut {
    /// Each document with an approval, the most approved again first, then by key.
    pub docs: Vec<DocApprovalsOut>,
    /// Each book in the order of the declaration, then (`book` null) the documents
    /// that are no valid document now.
    pub books: Vec<BookApprovalsOut>,
}

/// The approvals of one document.
#[derive(Debug, Serialize, JsonSchema)]
pub struct DocApprovalsOut {
    /// The document.
    pub doc: DocRef,
    /// Its approvals, revoked or not.
    pub approvals: usize,
    /// Those after an earlier approval of it that is not revoked.
    pub again: usize,
    /// Those revoked.
    pub revoked: usize,
}

/// The approvals of the documents of one book.
#[derive(Debug, Serialize, JsonSchema)]
pub struct BookApprovalsOut {
    /// The book; null for the documents that are no valid document now.
    pub book: Option<String>,
    /// Its documents with an approval.
    pub docs: usize,
    /// Their approvals, revoked or not.
    pub approvals: usize,
    /// Those after an earlier approval of the same document that is not revoked.
    pub again: usize,
    /// Those revoked.
    pub revoked: usize,
}

/// The pending times of one book.
#[derive(Debug, Serialize, JsonSchema)]
pub struct BookPendingOut {
    /// The book; null for the documents that are no valid document now.
    pub book: Option<String>,
    /// How many documents were approved.
    pub count: usize,
    /// The median, nearest-rank, in milliseconds; null when none.
    pub median_ms: Option<i64>,
    /// The 90th percentile, nearest-rank, in milliseconds; null when none.
    pub p90_ms: Option<i64>,
    /// The longest, in milliseconds; null when none.
    pub max_ms: Option<i64>,
}

/// The runs of `judge`.
#[derive(Debug, Serialize, JsonSchema)]
pub struct RunsOut {
    /// How many runs.
    pub count: usize,
    /// The median number of decisions of a run, nearest-rank; null when none.
    pub median: Option<i64>,
    /// The 90th percentile of the same, nearest-rank; null when none.
    pub p90: Option<i64>,
    /// The most decisions of a run; null when none.
    pub max: Option<i64>,
    /// The largest runs, the largest first, then the earliest.
    pub largest: Vec<RunOut>,
}

/// One run.
#[derive(Debug, Serialize, JsonSchema)]
pub struct RunOut {
    /// When (unix milliseconds).
    pub at: i64,
    /// Who.
    pub actor: String,
    /// How many decisions.
    pub size: usize,
}

/// A document or an artifact that changed under approvals made again.
#[derive(Debug, Serialize, JsonSchema)]
pub struct OriginCountOut {
    /// What it is.
    pub origin: ChangedOut,
    /// Under how many approvals made again.
    pub count: usize,
}

/// What an origin over time is: a document or an artifact, written as `stale`'s
/// origins write them.
#[derive(Debug, Serialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ChangedOut {
    /// A document (its key is null when no valid document has it now).
    Doc(DocNamed),
    /// An artifact.
    Artifact(ArtifactNamed),
}

/// What an item of an approval is, and which document or file it names.
#[derive(Debug, Serialize, JsonSchema)]
pub struct ItemOut {
    /// `subject` (the approved document), `upstream` (a document it stood on) or
    /// `artifact` (an artifact it stood on).
    pub role: &'static str,
    /// The document, for a subject or an upstream (an upstream's key is the one it
    /// had when it was pinned).
    pub doc: Option<DocRef>,
    /// The link it was pinned through, for an upstream or an artifact.
    pub link: Option<String>,
    /// The name, for an artifact (kept from when every name was a path; the same as
    /// `name`).
    pub path: Option<String>,
    /// The name, for an artifact.
    pub name: Option<String>,
    /// The space, for an artifact.
    pub space: Option<String>,
    /// For an artifact pinned under `name` and declared moved since (`judge moved`),
    /// the name it is read as now, which what it is now is asked of; none when it is
    /// read as written, and for a document.
    pub read_as: Option<String>,
    /// The hash the approval pinned; none for an artifact that was not there.
    pub approved_hash: Option<String>,
    /// Where its content, as it was approved, can be read back; none when the record
    /// keeps no address.
    pub address: Option<String>,
}

/// Whether the content of an item, as it was approved, came back.
#[derive(Debug, Serialize, JsonSchema)]
pub struct RetrievalOut {
    /// `read`, `address_not_readable` (an address, and nothing behind it: the content
    /// was never committed, or is not in this clone) or `no_address` (the record
    /// keeps none: made outside Git, by an older version, or no file was there).
    pub state: &'static str,
    /// The content, when it was read and is UTF-8 text.
    pub text: Option<String>,
    /// How many bytes were read.
    pub bytes: Option<usize>,
}

/// One item of `get --approved`.
#[derive(Debug, Serialize, JsonSchema)]
pub struct ApprovedItemOut {
    /// What it is.
    pub item: ItemOut,
    /// Its content as it was approved, or why not.
    pub retrieval: RetrievalOut,
}

/// The answer of `get <ref> --approved`: what the approval that stands saw.
#[derive(Debug, Serialize, JsonSchema)]
pub struct ApprovedOut {
    /// The document, as every answer shows it now.
    pub row: RowOut,
    /// The approval (its record's id).
    pub judgement: String,
    /// Who approved.
    pub actor: String,
    /// When (unix milliseconds).
    pub at: i64,
    /// The commit the approval was made at, when the record keeps one (a hint).
    pub revision: Option<String>,
    /// The approved document first, then each upstream, then each artifact.
    pub items: Vec<ApprovedItemOut>,
    /// What is wrong around the documents.
    pub health: HealthOut,
}

/// One item of `diff`.
#[derive(Debug, Serialize, JsonSchema)]
pub struct DiffItemOut {
    /// What it is, with the hash that was approved.
    pub item: ItemOut,
    /// The hash it has now; none when it is no valid document, or, for an artifact,
    /// when `now` is not `present`.
    pub current_hash: Option<String>,
    /// Whether it is not known to be what was approved: its hash differs, it is no
    /// valid document now, or it is an artifact that cannot be told now (`now` is
    /// `unknown`).
    pub changed: bool,
    /// For an artifact, what it is now: `present`, `absent`, or `unknown` (its
    /// adapter cannot tell now); none for a document.
    pub now: Option<&'static str>,
    /// What holds it now, if anything does: a document's file, or the name of an
    /// artifact that is there now (for the space `file`, its path).
    pub file: Option<String>,
    /// Whether the approved content came back: `read`, `address_not_readable`,
    /// `no_address` (the content itself is in `diff`).
    pub retrieval: &'static str,
    /// The unified diff from the approved content to the file now, when both are
    /// UTF-8 text; none otherwise. It agrees with `changed`: a document's diff is of
    /// its normalized text (what its hash is taken over) and is empty when `changed`
    /// is false; an artifact's diff is of its bytes as text.
    pub diff: Option<String>,
}

/// The answer of `diff <ref>`: each item of the approval that stands, then and
/// now.
#[derive(Debug, Serialize, JsonSchema)]
pub struct DiffOut {
    /// The document, as every answer shows it now.
    pub row: RowOut,
    /// The approval (its record's id).
    pub judgement: String,
    /// The commit the approval was made at, when the record keeps one (a hint).
    pub revision: Option<String>,
    /// The approved document first, then each upstream, then each artifact.
    pub items: Vec<DiffItemOut>,
    /// What is wrong around the documents.
    pub health: HealthOut,
}

/// The answer of `judge`: every record written, and every approval left out, each in
/// the order the documents were named.
#[derive(Debug, Serialize, JsonSchema)]
pub struct JudgedAllOut {
    /// The judgements.
    pub judged: Vec<JudgedOut>,
    /// The approvals not made, each `repeats`: one made now would pin exactly what
    /// the approval that stands pins and lapse when it lapses (`--again` makes it
    /// anyway). Empty for every verb but `approve`.
    pub left_out: Vec<LeftOutOut>,
}

/// The answer of `judge approve --stale`: every approval made again, upstream first,
/// and those left out.
#[derive(Debug, Serialize, JsonSchema)]
pub struct StaleApprovedOut {
    /// The judgements, as `judge` answers them, upstream first; empty when nothing
    /// could be approved.
    pub judged: Vec<JudgedOut>,
    /// The approvals that do not hold and were not approved again, by identity.
    pub left_out: Vec<LeftOutOut>,
}

/// An approval that `judge approve` or `judge approve --stale` did not make.
#[derive(Debug, Serialize, JsonSchema)]
pub struct LeftOutOut {
    /// The document.
    pub doc: DocRef,
    /// Why: of `--stale`, `broken` (its approval is broken now), `refused` (the
    /// model refuses its approval now) or `stands_on` (a `depends` link names a
    /// document that is broken, pending or left out); of either, `repeats` (one made
    /// now would pin exactly what the approval that stands pins and lapse when it
    /// lapses; of `--stale`, one stale only through an upstream's upstream the run
    /// approved again on a new hash).
    pub reason: &'static str,
    /// A sentence for a person.
    pub detail: String,
    /// For `stands_on`, the document that holds it back.
    pub upstream: Option<DocRef>,
    /// For `stands_on`, the link that names it.
    pub link: Option<String>,
    /// For `stands_on`, why that document holds it back: `broken`, `pending` or
    /// `left_out` (refused or broken in the run; one that `repeats` stands, and holds
    /// nothing back).
    pub upstream_is: Option<&'static str>,
    /// For `repeats`, the approval that stands (its record's id).
    pub approval: Option<String>,
    /// For `broken`, the kinds of its causes, each once, in the order of `causes`;
    /// empty otherwise.
    pub kinds: Vec<&'static str>,
    /// For `broken`, why its approval does not hold; empty otherwise.
    pub causes: Vec<CauseOut>,
}

/// The answer of `judge same`: every declaration written, in the order the
/// documents and files were named.
#[derive(Debug, Serialize, JsonSchema)]
pub struct DeclaredAllOut {
    /// The declarations.
    pub declared: Vec<DeclaredOut>,
}

/// One declaration that an edit keeps the meaning.
#[derive(Debug, Serialize, JsonSchema)]
pub struct DeclaredOut {
    /// The document it is about, if it is about one.
    pub doc: Option<DocRef>,
    /// The name of the artifact it is about, if it is about one: for the space
    /// `file`, a path under the root.
    pub artifact: Option<String>,
    /// The space of that artifact, beside `artifact`.
    pub space: Option<String>,
    /// The record written, as it is in the ledger (its `from` lists the earlier
    /// hashes, its `to` the hash now).
    pub record: Value,
}

/// The answer of `judge moved`: every declaration of a move written.
#[derive(Debug, Serialize, JsonSchema)]
pub struct MovedAllOut {
    /// The declarations.
    pub moved: Vec<MovedOut>,
}

/// One declaration that an artifact moved.
#[derive(Debug, Serialize, JsonSchema)]
pub struct MovedOut {
    /// The space both names are of.
    pub space: String,
    /// The old name: for the space `file`, a path under the root.
    pub from: String,
    /// The new name.
    pub to: String,
    /// The record written, as it is in the ledger (its `id` is what `judge withdraw`
    /// takes).
    pub record: Value,
}

/// The answer of `judge withdraw`: the revocation written.
#[derive(Debug, Serialize, JsonSchema)]
pub struct WithdrawnOut {
    /// The record written, as it is in the ledger (its `targets` are the records
    /// withdrawn).
    pub record: Value,
}

/// One judgement written.
#[derive(Debug, Serialize, JsonSchema)]
pub struct JudgedOut {
    /// The document judged.
    pub doc: DocRef,
    /// The record written, as it is in the ledger.
    pub record: Value,
    /// For a deprecation, what replaces the document.
    pub successors: Vec<DocRef>,
    /// For an approval, the names of the document's links to artifacts with nothing
    /// there; a `depends` link pinned each as absent.
    pub absent: Vec<AbsentNameOut>,
}

/// A name of a link to artifacts with nothing there.
#[derive(Debug, Serialize, JsonSchema)]
pub struct AbsentNameOut {
    /// The link.
    pub link: String,
    /// The name as written (kept from when every name was a path; the same as
    /// `name`).
    pub path: String,
    /// The name as written.
    pub name: String,
    /// The space of the name.
    pub space: String,
    /// A sentence for a person.
    pub detail: String,
}

/// A key written in a link of a new document that no valid document answers to now.
#[derive(Debug, Serialize, JsonSchema)]
pub struct UnresolvedOut {
    /// The link.
    pub link: String,
    /// The key as written.
    pub written: String,
    /// Why it cannot be followed.
    pub why: String,
}

/// The answer of `new`: the document written.
#[derive(Debug, Serialize, JsonSchema)]
pub struct CreatedOut {
    /// The document.
    pub doc: DocRef,
    /// Its file, relative to the project root.
    pub file: String,
    /// The keys of its links that cannot be followed yet (the document is valid with
    /// them; each is a notice).
    pub unresolved_links: Vec<UnresolvedOut>,
}

/// The answer of `init`: what was written, and what to run next.
#[derive(Debug, Serialize, JsonSchema)]
pub struct InitOut {
    /// The file of the declaration, relative to the project root.
    pub declaration: String,
    /// The documents made beside it, in the order they were written; none for
    /// `init` without `--sample`.
    pub documents: Vec<InitDocOut>,
    /// The commands to run next, in order, as they are typed, each with
    /// `--root=<dir>` when `init` ran with one; none when that root is no UTF-8
    /// text.
    pub next: Vec<String>,
}

/// A document `init` made.
#[derive(Debug, Serialize, JsonSchema)]
pub struct InitDocOut {
    /// Its key (`book:KEY`).
    pub key: String,
    /// Its file, relative to the project root.
    pub path: String,
}

/// The answer of `fmt` and `fmt --check`, the files by their path relative to the
/// project root, in the order the documents are read.
#[derive(Debug, Serialize, JsonSchema)]
pub struct FmtOut {
    /// The files written in the form; empty for `--check`.
    pub rewritten: Vec<String>,
    /// How many files were in the form already (none of them is written).
    pub in_form: usize,
    /// For `--check`, the files not in the form that `fmt` would write in it; null
    /// without `--check`.
    pub not_in_form: Option<Vec<String>>,
    /// The files left as they are, each with why. They are not in the form either,
    /// and are listed apart from `not_in_form`; `--check` exits 1 when either list
    /// has any file, and `fmt` without it exits 0, with a warning for each.
    pub left_alone: Vec<LeftAloneOut>,
}

/// A file `fmt` leaves as it is.
#[derive(Debug, Serialize, JsonSchema)]
pub struct LeftAloneOut {
    /// The file.
    pub file: String,
    /// Why: `unparsed` (its front matter cannot be read), `comment` (its front
    /// matter holds a comment, which the form would lose), `reads_otherwise` (its
    /// text in the form would not read as the file does), `read_only` (the user
    /// cannot write it), `linked` (it has more than one link, which a file written
    /// in its place would not keep) or `changed` (it changed between the reading and
    /// the write).
    pub reason: &'static str,
    /// For `comment`, the line of the file the first one is on.
    pub line: Option<u32>,
    /// A sentence for a person.
    pub detail: String,
}

/// The answer when a command gives none.
#[derive(Debug, Serialize, JsonSchema)]
pub struct ErrorOut {
    /// `request`, `refused` or `failed` (the exit status is 2, 1 or 3).
    pub error: &'static str,
    /// The error and its causes, outermost first.
    pub messages: Vec<String>,
    /// The documents a reference could be, when it names more than one.
    pub candidates: Vec<String>,
    /// Each problem, one per item, when the error has a list of them: the problems
    /// of a declaration that cannot be used, or of a new document that would be
    /// invalid; for `init`, each thing in the way, or the files written before a
    /// failure part way and the one that was not.
    pub problems: Vec<String>,
}

/// The JSON Schema of every answer.
#[derive(Debug, Serialize)]
pub struct SchemasOut {
    /// The version of the answers these schemas describe.
    pub version: u32,
    /// By command.
    pub outputs: serde_json::Map<String, Value>,
}

/// The JSON Schema of every answer, by command.
pub fn schemas() -> SchemasOut {
    let mut outputs = serde_json::Map::new();
    let mut put = |name: &str, schema: Schema| {
        outputs.insert(name.to_owned(), versioned_schema(schema.to_value()));
    };
    put("check", schema_for!(BooksCheckOut));
    put("map", schema_for!(MapOut));
    put("status", schema_for!(StatusOut));
    put("get", schema_for!(GetOut));
    put("get --approved", schema_for!(ApprovedOut));
    put("trace", schema_for!(TraceOut));
    put("unlinked", schema_for!(UnlinkedOut));
    put("stale", schema_for!(StaleOut));
    put("stats", schema_for!(StatsOut));
    put("moves", schema_for!(MovesOut));
    put("diff", schema_for!(DiffOut));
    put("judge", schema_for!(JudgedAllOut));
    put("judge approve --stale", schema_for!(StaleApprovedOut));
    put("judge same", schema_for!(DeclaredAllOut));
    put("judge moved", schema_for!(MovedAllOut));
    put("judge withdraw", schema_for!(WithdrawnOut));
    put("new", schema_for!(CreatedOut));
    put("init", schema_for!(InitOut));
    put("fmt", schema_for!(FmtOut));
    put("error", schema_for!(ErrorOut));
    SchemasOut {
        version: FORMAT_VERSION,
        outputs,
    }
}

/// The version of the JSON every command prints, as the `version` field of each
/// answer.
///
/// Why: a reader (often an agent) can tell which shape it has. Changes within a
/// version only add: a new field, a new word in a field that holds words. Anything
/// else is a new version.
pub const FORMAT_VERSION: u32 = 1;

/// An answer with its `version` field, the one place every printed answer passes.
pub fn versioned(answer: Value) -> Value {
    match answer {
        Value::Object(mut fields) => {
            fields.insert("version".to_owned(), Value::from(FORMAT_VERSION));
            Value::Object(fields)
        }
        other => other,
    }
}

/// A schema of an answer, with the `version` field `versioned` adds.
fn versioned_schema(mut schema: Value) -> Value {
    if let Some(Value::Object(properties)) = schema.get_mut("properties") {
        properties.insert(
            "version".to_owned(),
            serde_json::json!({ "type": "integer", "const": FORMAT_VERSION }),
        );
    }
    if let Some(Value::Array(required)) = schema.get_mut("required") {
        required.push(Value::from("version"));
    }
    schema
}

/// Writes identities as `{id, key}` with the keys of one report.
struct Namer<'a>(&'a Names);

impl Namer<'_> {
    fn doc(&self, id: DocId) -> DocRef {
        DocRef {
            id: id.to_string(),
            key: self.0.key_of(id).map(ToString::to_string),
        }
    }

    fn pin(&self, pin: &FramePin) -> DocRef {
        DocRef {
            id: pin.target().to_string(),
            key: Some(pin.key().to_string()),
        }
    }

    fn row(&self, row: &Row) -> RowOut {
        let (declared, expires) = declared(&row.declared);
        let (successors, causes) = match &row.standing {
            Standing::Deprecated { successors } => (self.successors(successors), Vec::new()),
            Standing::Approved(Freshness::Degraded(degradation)) => (
                Vec::new(),
                degradation.causes().iter().map(|c| self.cause(c)).collect(),
            ),
            _ => (Vec::new(), Vec::new()),
        };
        RowOut {
            doc: self.doc(row.id),
            presence: presence(&row.presence),
            declared,
            decision: row.decision.as_ref().map(|d| DecisionOut {
                judgement: d.id().to_string(),
                actor: d.actor().as_str().to_owned(),
                at: d.at().millis(),
                reason: d.reason().map(|r| r.as_str().to_owned()),
            }),
            expires,
            standing: standing(row.standing.kind()),
            successors,
            causes,
            notices: row.notices.iter().map(|n| self.notice(n)).collect(),
        }
    }

    fn successors(&self, successors: &Successors) -> Vec<DocRef> {
        match successors {
            Successors::None => Vec::new(),
            Successors::Some(ids) => ids.iter().map(|id| self.doc(*id)).collect(),
        }
    }

    fn cause(&self, cause: &Cause) -> CauseOut {
        let severity = severity(cause.severity());
        let out = |kind, upstream: Option<&FramePin>, detail: String| CauseOut {
            kind,
            severity,
            upstream: upstream.map(|p| self.pin(p)),
            link: upstream.map(|p| p.link().to_string()),
            artifact: None,
            space: None,
            read_as: None,
            detail,
        };
        // What was pinned, and beside it the name it is read as after a move.
        let artifact = |kind, read: &ReadPin, detail: String| CauseOut {
            kind,
            severity,
            upstream: None,
            link: Some(read.pin().link().to_string()),
            artifact: Some(read.pin().name().to_string()),
            space: Some(read.pin().space().to_string()),
            read_as: read.moved_away().map(|a| a.name().to_string()),
            detail: match read.moved_away() {
                Some(_) => format!(
                    "{detail} (pinned as {}, declared moved since)",
                    read.pin().artifact()
                ),
                None => detail,
            },
        };
        match cause {
            Cause::SelfChanged { approved, now } => out(
                "self_changed",
                None,
                format!("the document changed since it was approved ({approved} -> {now})"),
            ),
            Cause::SelfNotValid(DocFailure::Invalid(problems)) => out(
                "self_invalid",
                None,
                format!("the document is invalid: {}", join(problems.iter())),
            ),
            Cause::SelfNotValid(DocFailure::Gone) => {
                out("self_gone", None, "no file carries the document".to_owned())
            }
            Cause::SelfNotValid(DocFailure::Unknown(unreadable)) => out(
                "self_unknown",
                None,
                format!(
                    "no readable file carries the document; unreadable: {}",
                    join(unreadable.iter())
                ),
            ),
            Cause::UpstreamNotValid {
                pin,
                failure: DocFailure::Gone,
            } => out("upstream_gone", Some(pin), format!("{} is gone", pin.key())),
            Cause::UpstreamNotValid {
                pin,
                failure: DocFailure::Invalid(_),
            } => out(
                "upstream_invalid",
                Some(pin),
                format!("{} is invalid", pin.key()),
            ),
            Cause::UpstreamNotValid {
                pin,
                failure: DocFailure::Unknown(unreadable),
            } => out(
                "upstream_unknown",
                Some(pin),
                format!(
                    "{} is in no readable file; unreadable: {}",
                    pin.key(),
                    join(unreadable.iter())
                ),
            ),
            Cause::UpstreamChanged { pin, now } => out(
                "upstream_changed",
                Some(pin),
                format!(
                    "{} changed since the approval pinned it ({} -> {now})",
                    pin.key(),
                    pin.hash()
                ),
            ),
            Cause::UpstreamDeclared { pin, status } => out(
                "upstream_declared",
                Some(pin),
                format!("{} is {}", pin.key(), upstream_status(status)),
            ),
            Cause::Via { pin, severity } => out(
                "via",
                Some(pin),
                format!(
                    "{} does not hold ({})",
                    pin.key(),
                    self::severity(*severity)
                ),
            ),
            Cause::LinksChanged {
                link,
                added,
                removed,
                unresolved,
            } => CauseOut {
                kind: "links_changed",
                severity,
                upstream: None,
                link: Some(link.to_string()),
                artifact: None,
                space: None,
                read_as: None,
                detail: format!(
                    "the link `{link}` changed since the approval: {} added, {} removed, {} unresolved",
                    added.len(),
                    removed.len(),
                    unresolved.len()
                ),
            },
            Cause::ArtifactChanged { pin, now } => artifact(
                "artifact_changed",
                pin,
                format!(
                    "{} changed since the approval pinned it ({} -> {now})",
                    pin.read_as(),
                    pinned_hash(pin.pin())
                ),
            ),
            Cause::ArtifactComparisonChanged { pin, now } => artifact(
                "artifact_comparison_changed",
                pin,
                format!(
                    "the link `{}` compares {} as {} now, and it was pinned as {}: the hashes are not compared",
                    pin.pin().link(),
                    pin.read_as(),
                    comparison(now),
                    comparison(pin.pin().comparison())
                ),
            ),
            // The name the link writes is the cause's `artifact`; the name it is to
            // write, the one the pin is read as, is `read_as`.
            Cause::ArtifactMoved { link, written, pin } => CauseOut {
                kind: "artifact_moved",
                severity,
                upstream: None,
                link: Some(link.to_string()),
                artifact: Some(written.name().to_string()),
                space: Some(written.space().to_string()),
                read_as: Some(pin.read_as().name().to_string()),
                detail: moved_detail(link, written, pin.read_as()),
            },
            Cause::ArtifactGone { pin } => artifact(
                "artifact_gone",
                pin,
                format!(
                    "{} was there when it was pinned and has nothing behind it now",
                    pin.read_as()
                ),
            ),
            Cause::ArtifactArrived { pin, now } => artifact(
                "artifact_arrived",
                pin,
                format!(
                    "{} was not there when it was pinned and is now ({now}): look again",
                    pin.read_as()
                ),
            ),
            Cause::ArtifactLinksChanged {
                link,
                added,
                removed,
            } => CauseOut {
                kind: "links_changed",
                severity,
                upstream: None,
                link: Some(link.to_string()),
                artifact: None,
                // The space of the names, when they all agree; none when the link
                // moved to another space since (the detail names each name's space).
                space: {
                    let mut spaces = added.iter().chain(removed).map(|a| a.space());
                    let first = spaces.next();
                    match first {
                        Some(first) if spaces.all(|s| s == first) => Some(first.to_string()),
                        Some(_) | None => None,
                    }
                },
                read_as: None,
                detail: format!(
                    "the link `{link}` to artifacts changed since the approval: {} added ({}), {} removed ({})",
                    added.len(),
                    join(added.iter()),
                    removed.len(),
                    join(removed.iter())
                ),
            },
            Cause::InCycle { .. } => out(
                "in_cycle",
                None,
                "the document is in a cycle (see health.cycles)".to_owned(),
            ),
            Cause::Expired { since } => out(
                "expired",
                None,
                format!("the approval lapsed at {} (unix ms)", since.millis()),
            ),
        }
    }

    fn notice(&self, notice: &Notice) -> NoticeOut {
        match notice {
            Notice::InformsNotFresh {
                link,
                target,
                standing: kind,
            } => NoticeOut {
                kind: "informs_not_fresh",
                severity: None,
                link: link.to_string(),
                target: Some(self.doc(*target)),
                artifact: None,
                space: None,
                read_as: None,
                declared: Vec::new(),
                detail: format!("a document it informs of is {}", standing(*kind)),
            },
            Notice::InformsUnresolved { link, unresolved } => NoticeOut {
                kind: "informs_unresolved",
                severity: None,
                link: link.to_string(),
                target: None,
                artifact: None,
                space: None,
                read_as: None,
                declared: Vec::new(),
                detail: self.unresolved(unresolved),
            },
            Notice::UpstreamExpired { pin, since } => NoticeOut {
                kind: "upstream_expired",
                severity: None,
                link: pin.link().to_string(),
                target: Some(self.pin(pin)),
                artifact: None,
                space: None,
                read_as: None,
                declared: Vec::new(),
                detail: format!(
                    "the approval of {} lapsed at {} (unix ms)",
                    pin.key(),
                    since.millis()
                ),
            },
            Notice::SelfSameMeaning {
                approved,
                now,
                declared,
            } => NoticeOut {
                kind: "self_same_meaning",
                severity: None,
                link: String::new(),
                target: None,
                artifact: None,
                space: None,
                read_as: None,
                declared: declared.iter().map(ToString::to_string).collect(),
                detail: format!(
                    "the document was edited since its approval ({approved} -> {now}), and the edit was declared to keep the meaning"
                ),
            },
            Notice::UpstreamSameMeaning { pin, now, declared } => NoticeOut {
                kind: "upstream_same_meaning",
                severity: None,
                link: pin.link().to_string(),
                target: Some(self.pin(pin)),
                artifact: None,
                space: None,
                read_as: None,
                declared: declared.iter().map(ToString::to_string).collect(),
                detail: format!(
                    "{} was edited since it was pinned ({} -> {now}), and the edit was declared to keep the meaning",
                    pin.key(),
                    pin.hash()
                ),
            },
            Notice::ArtifactSameMeaning { pin, now, declared } => NoticeOut {
                kind: "artifact_same_meaning",
                severity: None,
                link: pin.pin().link().to_string(),
                target: None,
                artifact: Some(pin.pin().name().to_string()),
                space: Some(pin.pin().space().to_string()),
                read_as: pin.moved_away().map(|a| a.name().to_string()),
                declared: declared.iter().map(ToString::to_string).collect(),
                detail: format!(
                    "{} changed since it was pinned (now {now}), and the change was declared to keep the meaning",
                    pin.read_as()
                ),
            },
            Notice::ArtifactAbsent { link, artifact } => NoticeOut {
                kind: "artifact_absent",
                severity: None,
                link: link.to_string(),
                target: None,
                artifact: Some(artifact.name().to_string()),
                space: Some(artifact.space().to_string()),
                read_as: None,
                declared: Vec::new(),
                detail: absent_detail(artifact),
            },
            Notice::ArtifactUnknown {
                link,
                artifact,
                why,
            } => NoticeOut {
                kind: "artifact_unknown",
                severity: None,
                link: link.to_string(),
                target: None,
                artifact: Some(artifact.name().to_string()),
                space: Some(artifact.space().to_string()),
                read_as: None,
                declared: Vec::new(),
                detail: unknown_detail(artifact, why.as_str()),
            },
            Notice::NotCovered {
                book,
                link,
                severity,
            } => NoticeOut {
                kind: "not_covered",
                severity: Some(severity.word()),
                link: link.to_string(),
                target: None,
                artifact: None,
                space: None,
                read_as: None,
                declared: Vec::new(),
                detail: format!(
                    "no document of `{book}` reaches this one through `{link}`, which is declared to cover its book"
                ),
            },
        }
    }

    fn unresolved(&self, link: &UnresolvedLink) -> String {
        let why = match link.why() {
            UnresolvedWhy::Malformed(problem) => format!("not a key: {problem}"),
            UnresolvedWhy::Missing => "no document has this key".to_owned(),
            UnresolvedWhy::Ambiguous(ids) => format!(
                "it names {} documents: {}",
                ids.len(),
                join(ids.iter().map(|id| self.label(*id)))
            ),
            UnresolvedWhy::TargetInvalid(id) => {
                format!("the document it names is invalid ({})", self.label(*id))
            }
        };
        format!("`{}` cannot be followed: {why}", link.written())
    }

    fn label(&self, id: DocId) -> String {
        self.0
            .key_of(id)
            .map_or_else(|| id.to_string(), ToString::to_string)
    }

    fn root(&self, root: &RootCause) -> RootOut {
        RootOut {
            path: root.path.iter().map(|id| self.doc(*id)).collect(),
            cause: self.cause(&root.cause),
        }
    }

    fn health(&self, health: &Health) -> HealthOut {
        HealthOut {
            clean: health.is_clean(),
            invalid: health
                .invalid
                .iter()
                .map(|d| InvalidOut {
                    file: d.origin().to_string(),
                    id: d.id().map(|id| id.to_string()),
                    problems: d.problems().iter().map(ToString::to_string).collect(),
                })
                .collect(),
            cycles: health
                .cycles
                .iter()
                .map(|c| c.ids().into_iter().map(|id| self.doc(id)).collect())
                .collect(),
            ledger_problems: health
                .ledger_problems
                .iter()
                .map(ToString::to_string)
                .collect(),
            unreadable_records: health
                .unreadable_records
                .iter()
                .map(|u| UnreadableOut {
                    file: u.file.to_string(),
                    why: u.why.to_string(),
                })
                .collect(),
        }
    }

    fn target(&self, state: &LinkState) -> LinkTargetOut {
        match state {
            LinkState::Resolved(target) => LinkTargetOut::Resolved {
                doc: self.doc(target.id()),
            },
            LinkState::Unresolved(u) => LinkTargetOut::Unresolved {
                written: u.written().to_owned(),
                why: self.unresolved(u),
            },
        }
    }

    fn content(&self, doc: &Doc) -> DocOut {
        DocOut {
            fields: doc
                .fields()
                .iter()
                .map(|(name, value)| (name.to_string(), field(value)))
                .collect(),
            links: doc
                .links()
                .iter()
                .map(|(name, link)| LinkOut {
                    name: name.to_string(),
                    targets: link.states().iter().map(|s| self.target(s)).collect(),
                })
                .chain(doc.artifact_links().iter().map(|(name, link)| LinkOut {
                    name: name.to_string(),
                    targets: link.artifacts().iter().map(artifact_target).collect(),
                }))
                .collect(),
            body: doc.body().to_owned(),
        }
    }

    fn upstream(&self, edge: &UpstreamEdge) -> EdgeOut {
        let (to, standing) = match &edge.to {
            UpstreamEnd::Doc(row) => (
                LinkTargetOut::Resolved {
                    doc: self.doc(row.id),
                },
                Some(self::standing(row.standing.kind())),
            ),
            UpstreamEnd::Unresolved(u) => (self.target(&LinkState::Unresolved(u.clone())), None),
            UpstreamEnd::Artifact(a) => (artifact_target(a), None),
        };
        EdgeOut {
            from: self.doc(edge.from),
            link: edge.link.to_string(),
            means: means(edge.propagation),
            depth: edge.depth,
            to,
            standing,
        }
    }

    fn downstream(&self, edge: &DownstreamEdge) -> EdgeOut {
        EdgeOut {
            from: self.doc(edge.holder.id),
            link: edge.link.to_string(),
            means: means(edge.propagation),
            depth: edge.depth,
            to: LinkTargetOut::Resolved {
                doc: self.doc(edge.to),
            },
            standing: Some(standing(edge.holder.standing.kind())),
        }
    }
}

/// The lines `check` writes on stderr: one per document a covering link of
/// strength `warn` does not reach, in the order of `uncovered`, then one per name
/// that cannot be told now, in the order of `unknown`, then one per name with
/// nothing there, in the order of `absent`, then one per name a link still writes
/// that was declared moved since an approval pinned it, in the order of `moved`. A
/// gap of strength `error` has none: it fails the check, and the answer is shown.
/// None of them changes the exit.
pub fn check_warnings(check: &BooksCheck) -> Vec<String> {
    let BooksCheck::Checked {
        uncovered,
        unknown,
        absent,
        moved,
        ..
    } = check
    else {
        return Vec::new();
    };
    let gaps = uncovered
        .iter()
        .filter(|u| u.severity == CoverSeverity::Warn)
        .map(|u| {
            let doc = u
                .key
                .as_ref()
                .map_or_else(|| u.doc.to_string(), ToString::to_string);
            format!(
                "warning: {doc} is reached by no {}.{} (cover = \"warn\")",
                u.book, u.link
            )
        });
    let untold = unknown.iter().map(|u| {
        let doc = u
            .key
            .as_ref()
            .map_or_else(|| u.doc.to_string(), ToString::to_string);
        format!(
            "warning: {doc} names {} in {}, which cannot be told: {}",
            u.artifact.name(),
            u.artifact.space(),
            u.why
        )
    });
    let nothing = absent.iter().map(|a| {
        let doc = a
            .key
            .as_ref()
            .map_or_else(|| a.doc.to_string(), ToString::to_string);
        format!(
            "warning: {doc} names {} in {}, which has nothing behind it",
            a.artifact.name(),
            a.artifact.space()
        )
    });
    let moved_away = moved.iter().map(|m| {
        let doc = m
            .key
            .as_ref()
            .map_or_else(|| m.doc.to_string(), ToString::to_string);
        format!(
            "warning: {doc} names {} in {} under `{}`, which was declared moved to {}: write the new name",
            m.artifact.name(),
            m.artifact.space(),
            m.link,
            m.moved_to.name()
        )
    });
    gaps.chain(untold)
        .chain(nothing)
        .chain(moved_away)
        .collect()
}

/// The lines `stale` writes on stderr: one per name an approval stands on that
/// cannot be told now, in the order of `unknown`.
pub fn stale_warnings(report: &StaleReport) -> Vec<String> {
    report
        .unknown
        .iter()
        .map(|u| {
            let doc = u
                .key
                .as_ref()
                .map_or_else(|| u.doc.to_string(), ToString::to_string);
            format!(
                "warning: {doc} pins {} in {}, which cannot be told now, so whether its approval holds cannot be said: {}",
                u.artifact.name(),
                u.artifact.space(),
                u.why
            )
        })
        .collect()
}

/// A problem of the declaration as every answer gives it: its text, then the text
/// of each reason under it (`Error::source`), joined by `: `, so a problem that
/// keeps its reason as a source (a comparison that is none, a directory that is no
/// relative path) says why. The one place a problem of the declaration is written.
fn schema_problem(problem: &SchemaProblem) -> String {
    let mut text = problem.to_string();
    let mut source = std::error::Error::source(problem);
    while let Some(cause) = source {
        text.push_str(": ");
        text.push_str(&cause.to_string());
        source = cause.source();
    }
    text
}

/// The answer of `check`.
pub fn books_check(check: &BooksCheck) -> BooksCheckOut {
    match check {
        BooksCheck::SchemaInvalid(problems) => BooksCheckOut {
            clean: false,
            schema_problems: problems.iter().map(schema_problem).collect(),
            books: Vec::new(),
            health: None,
            uncovered: Vec::new(),
            absent: Vec::new(),
            unknown: Vec::new(),
            moved: Vec::new(),
        },
        BooksCheck::Checked {
            books,
            health,
            uncovered,
            absent,
            unknown,
            moved,
        } => {
            let names = Names::default();
            BooksCheckOut {
                clean: check.is_clean(),
                schema_problems: Vec::new(),
                books: books
                    .iter()
                    .map(|b| BookOut {
                        name: b.name.to_string(),
                        dir: b.dir.as_rel().to_string(),
                        valid: b.tally.valid,
                        invalid: b.tally.invalid,
                    })
                    .collect(),
                health: Some(Namer(&names).health(health)),
                uncovered: uncovered
                    .iter()
                    .map(|u| UncoveredOut {
                        doc: DocRef {
                            id: u.doc.to_string(),
                            key: u.key.as_ref().map(ToString::to_string),
                        },
                        book: u.book.to_string(),
                        link: u.link.to_string(),
                        severity: u.severity.word(),
                    })
                    .collect(),
                absent: absent
                    .iter()
                    .map(|a| AbsentOut {
                        doc: DocRef {
                            id: a.doc.to_string(),
                            key: a.key.as_ref().map(ToString::to_string),
                        },
                        link: a.link.to_string(),
                        path: a.artifact.name().to_string(),
                        name: a.artifact.name().to_string(),
                        space: a.artifact.space().to_string(),
                        detail: absent_detail(&a.artifact),
                    })
                    .collect(),
                unknown: unknown
                    .iter()
                    .map(|u| UnknownOut {
                        doc: DocRef {
                            id: u.doc.to_string(),
                            key: u.key.as_ref().map(ToString::to_string),
                        },
                        link: u.link.to_string(),
                        name: u.artifact.name().to_string(),
                        space: u.artifact.space().to_string(),
                        why: u.why.to_string(),
                    })
                    .collect(),
                moved: moved
                    .iter()
                    .map(|m| MovedNameOut {
                        doc: DocRef {
                            id: m.doc.to_string(),
                            key: m.key.as_ref().map(ToString::to_string),
                        },
                        link: m.link.to_string(),
                        name: m.artifact.name().to_string(),
                        space: m.artifact.space().to_string(),
                        moved_to: m.moved_to.name().to_string(),
                        declared: m.declared.iter().map(ToString::to_string).collect(),
                        detail: moved_detail(&m.link, &m.artifact, &m.moved_to),
                    })
                    .collect(),
            }
        }
    }
}

/// The answer of `map`.
pub fn map(report: &MapReport) -> MapOut {
    let map = match report {
        MapReport::SchemaInvalid(problems) => {
            return MapOut {
                schema_problems: problems.iter().map(schema_problem).collect(),
                books: Vec::new(),
                links: Vec::new(),
                diagram: None,
            };
        }
        MapReport::Mapped(map) => map,
    };
    MapOut {
        schema_problems: Vec::new(),
        books: map
            .books
            .iter()
            .map(|book| MapBookOut {
                name: book.name.to_string(),
                dir: book.dir.as_rel().to_string(),
                key: match &book.key {
                    KeyPattern::Prefixed(prefix) => KeyGrammarOut::Prefix {
                        prefix: prefix.to_string(),
                    },
                    KeyPattern::Slug => KeyGrammarOut::Slug,
                },
                description: book.description.as_ref().map(ToString::to_string),
                fields: book
                    .fields
                    .iter()
                    .map(|(name, decl)| MapFieldOut {
                        name: name.to_string(),
                        ty: field_type(&decl.ty),
                        words: match &decl.ty {
                            FieldType::Enum(words) => {
                                words.iter().map(ToString::to_string).collect()
                            }
                            _ => Vec::new(),
                        },
                        required: decl.presence == FieldPresence::Required,
                    })
                    .collect(),
            })
            .collect(),
        links: map
            .links
            .iter()
            .map(|link| {
                let (cover, preset) = match &link.target {
                    MapTarget::Book { cover, .. } => (*cover, None),
                    MapTarget::Artifact { preset, .. } => (None, preset.as_ref()),
                };
                MapLinkOut {
                    from: link.from.to_string(),
                    name: link.name.to_string(),
                    target: match &link.target {
                        MapTarget::Book { book, .. } => MapTargetOut::Book {
                            book: book.to_string(),
                        },
                        MapTarget::Artifact {
                            space, comparison, ..
                        } => MapTargetOut::Artifact {
                            space: space.to_string(),
                            compare: comparison.kind_word(),
                            ignore: comparison
                                .ignored()
                                .map(|p| p.as_str().to_owned())
                                .collect(),
                        },
                    },
                    means: means(link.propagation),
                    count: count_word(link.cardinality),
                    cover: cover.is_some(),
                    cover_severity: cover.map(CoverSeverity::word),
                    preset: preset.map(|p| p.name().to_string()),
                    preset_by_default: preset.is_some_and(PresetOrigin::is_default),
                }
            })
            .collect(),
        diagram: Some(diagram::draw(map)),
    }
}

/// The answer of `status`.
pub fn status(report: &StatusReport) -> StatusOut {
    let namer = Namer(&report.names);
    StatusOut {
        docs: report.rows.iter().map(|r| namer.row(r)).collect(),
        health: namer.health(&report.health),
    }
}

/// The answer of `get`.
pub fn get(report: &GetReport) -> GetOut {
    let namer = Namer(&report.names);
    GetOut {
        row: namer.row(&report.row),
        content: report.doc.as_ref().map(|d| namer.content(d)),
        roots: report.roots.iter().map(|r| namer.root(r)).collect(),
        health: namer.health(&report.health),
    }
}

/// The answer of `trace`.
pub fn trace(report: &TraceReport) -> TraceOut {
    let namer = Namer(&report.names);
    TraceOut {
        root: namer.row(&report.root),
        upstream: report.upstream.iter().map(|e| namer.upstream(e)).collect(),
        downstream: report
            .downstream
            .iter()
            .map(|e| namer.downstream(e))
            .collect(),
        health: namer.health(&report.health),
    }
}

/// The answer of `unlinked`.
pub fn unlinked(report: &UnlinkedReport) -> UnlinkedOut {
    let no_names = Names::default();
    let (schema_problems, standing, decided_against, names, health) = match report {
        UnlinkedReport::SchemaInvalid { problems, lists } => (
            problems.iter().map(schema_problem).collect(),
            lists,
            lists,
            &no_names,
            None,
        ),
        UnlinkedReport::Answered {
            standing,
            decided_against,
            names,
            health,
        } => (Vec::new(), standing, decided_against, names, Some(health)),
    };
    let namer = Namer(names);
    let entries = |entries: &[UnlinkedEntry]| -> Vec<UnlinkedEntryOut> {
        entries
            .iter()
            .map(|e| UnlinkedEntryOut {
                book: e.book.to_string(),
                row: namer.row(&e.row),
            })
            .collect()
    };
    let list = |asked: &Option<Vec<UnlinkedEntry>>| asked.as_deref().map(entries);
    UnlinkedOut {
        isolated: entries(&standing.isolated),
        schema_problems,
        no_incoming: list(&standing.no_incoming),
        no_outgoing: list(&standing.no_outgoing),
        decided_against: DecidedAgainstOut {
            isolated: entries(&decided_against.isolated),
            no_incoming: list(&decided_against.no_incoming),
            no_outgoing: list(&decided_against.no_outgoing),
        },
        health: health.map(|h| namer.health(h)),
    }
}

/// The answer of `stale`.
pub fn stale(report: &StaleReport) -> StaleOut {
    let namer = Namer(&report.names);
    StaleOut {
        stale: report
            .entries
            .iter()
            .map(|e| StaleEntryOut {
                row: namer.row(&e.row),
                roots: e.roots.iter().map(|r| namer.root(r)).collect(),
            })
            .collect(),
        origins: report
            .origins
            .iter()
            .map(|group| OriginOut {
                start: match &group.origin {
                    Origin::Doc(id) => StartOut::Doc(DocNamed {
                        doc: namer.doc(*id),
                    }),
                    Origin::Artifact(artifact) => StartOut::Artifact(ArtifactNamed::of(artifact)),
                    Origin::Declaration => StartOut::Declaration,
                    Origin::Cycle(cycle) => StartOut::Cycle {
                        among: match cycle.source {
                            CycleSource::Links => "links",
                            CycleSource::Frames => "frames",
                        },
                        smallest: namer.doc(cycle.min),
                    },
                },
                degraded: group.degraded.iter().map(|id| namer.doc(*id)).collect(),
            })
            .collect(),
        unknown: report
            .unknown
            .iter()
            .map(|u| UnknownOut {
                doc: DocRef {
                    id: u.doc.to_string(),
                    key: u.key.as_ref().map(ToString::to_string),
                },
                link: u.link.to_string(),
                name: u.artifact.name().to_string(),
                space: u.artifact.space().to_string(),
                why: u.why.to_string(),
            })
            .collect(),
        health: namer.health(&report.health),
    }
}

/// The answer of `stats`, with `--since` as it was given.
pub fn stats(report: &StatsReport, since: Option<&str>) -> StatsOut {
    let namer = Namer(&report.names);
    let stats = &report.stats;
    let doc = |id: DocId, key: Option<&DocKey>| DocRef {
        id: id.to_string(),
        key: key.map(ToString::to_string),
    };
    let book = |b: Option<&BookName>| b.map(ToString::to_string);
    let mut out = StatsOut {
        since: since.map(str::to_owned),
        approvals: ApprovalsOut {
            docs: stats
                .approvals
                .docs
                .iter()
                .map(|d| DocApprovalsOut {
                    doc: doc(d.doc, d.key.as_ref()),
                    approvals: d.approvals,
                    again: d.again,
                    revoked: d.revoked,
                })
                .collect(),
            books: stats
                .approvals
                .books
                .iter()
                .map(|b| BookApprovalsOut {
                    book: book(b.book.as_ref()),
                    docs: b.docs,
                    approvals: b.approvals,
                    again: b.again,
                    revoked: b.revoked,
                })
                .collect(),
        },
        pending: stats
            .pending
            .iter()
            .map(|p| BookPendingOut {
                book: book(p.book.as_ref()),
                count: p.millis.count,
                median_ms: p.millis.median,
                p90_ms: p.millis.p90,
                max_ms: p.millis.max,
            })
            .collect(),
        runs: RunsOut {
            count: stats.runs.sizes.count,
            median: stats.runs.sizes.median,
            p90: stats.runs.sizes.p90,
            max: stats.runs.sizes.max,
            largest: stats
                .runs
                .largest
                .iter()
                .map(|r| RunOut {
                    at: r.at.millis(),
                    actor: r.actor.to_string(),
                    size: r.size,
                })
                .collect(),
        },
        origins: stats
            .origins
            .iter()
            .map(|o| OriginCountOut {
                origin: match &o.origin {
                    ChangedPin::Doc { doc: id, key } => ChangedOut::Doc(DocNamed {
                        doc: doc(*id, key.as_ref()),
                    }),
                    ChangedPin::Artifact(artifact) => {
                        ChangedOut::Artifact(ArtifactNamed::of(artifact))
                    }
                },
                count: o.count,
            })
            .collect(),
        health: namer.health(&report.health),
        drawing: String::new(),
    };
    out.drawing = summary::draw(&out);
    out
}

impl Namer<'_> {
    fn item(&self, item: &SnapshotItem) -> ItemOut {
        let (role, doc, link, artifact) = match item.role() {
            ItemRole::Subject { id } => ("subject", Some(self.doc(*id)), None, None),
            ItemRole::Upstream { link, target, key } => (
                "upstream",
                Some(DocRef {
                    id: target.to_string(),
                    key: Some(key.to_string()),
                }),
                Some(link.to_string()),
                None,
            ),
            ItemRole::Artifact { link, artifact, .. } => {
                ("artifact", None, Some(link.to_string()), Some(artifact))
            }
        };
        let approved_hash = match item.seen() {
            SeenHash::Doc(hash) => Some(hash.to_string()),
            SeenHash::Artifact(state) => state.hash().map(ToString::to_string),
        };
        ItemOut {
            role,
            doc,
            link,
            path: artifact.map(|a| a.name().to_string()),
            name: artifact.map(|a| a.name().to_string()),
            space: artifact.map(|a| a.space().to_string()),
            // Only a name other than the one pinned: a way back is read as written.
            read_as: item
                .moved()
                .map(|m| m.artifact())
                .filter(|a| Some(*a) != artifact)
                .map(|a| a.name().to_string()),
            approved_hash,
            address: item.address().map(ToString::to_string),
        }
    }
}

/// The answer of `moves`.
pub fn moves(report: &MovesReport) -> MovesOut {
    let namer = Namer(&report.names);
    let absent: Vec<MovesAbsentOut> = report
        .suggestions
        .absent
        .iter()
        .map(|one| MovesAbsentOut {
            space: one.gone().artifact().space().to_string(),
            name: one.gone().name().to_string(),
            pinned_by: one
                .gone()
                .pinned_by()
                .into_iter()
                .map(|id| namer.doc(id))
                .collect(),
            candidates: one
                .candidates()
                .iter()
                .map(|c| {
                    let (found, similarity) = match c.found() {
                        FoundBy::SameContent => ("same_content", None),
                        FoundBy::Renamed { similarity } => {
                            ("renamed", similarity.map(Similarity::percent))
                        }
                    };
                    MovesCandidateOut {
                        name: c.name().to_string(),
                        found,
                        similarity,
                        present: c.present(),
                    }
                })
                .collect(),
            notes: one.notes().iter().map(move_note).collect(),
        })
        .collect();
    let cannot_suggest: Vec<String> = report
        .suggestions
        .cannot_suggest
        .iter()
        .map(ToString::to_string)
        .collect();
    let drawing = moves_drawing(&absent, &cannot_suggest);
    MovesOut {
        absent,
        cannot_suggest,
        health: namer.health(&report.health),
        drawing,
    }
}

/// A note of `moves`, as its kind word and a sentence.
fn move_note(note: &MoveNote) -> MovesNoteOut {
    let join = |items: Vec<&str>| items.join(", ");
    let (kind, detail) = match note {
        MoveNote::InTwoSpaces { spaces } => (
            "in_two_spaces",
            format!(
                "pins are read as this name in more than one space ({}): `judge moved` refuses it, so nothing is suggested",
                join(spaces.iter().map(|s| s.as_str()).collect())
            ),
        ),
        MoveNote::NoRepository => (
            "no_repository",
            "the project is in no repository: renames were not looked for".to_owned(),
        ),
        MoveNote::NoRevision => (
            "no_revision",
            "no approval that pins it kept a revision: renames were not looked for".to_owned(),
        ),
        MoveNote::UnknownRevision { revisions } => (
            "unknown_revision",
            format!(
                "not a commit of this repository ({}): no rename since it was looked for",
                join(revisions.iter().map(|r| r.as_str()).collect())
            ),
        ),
        MoveNote::HistoryUnreadable { detail } => (
            "history_unreadable",
            format!("renames were not looked for: {detail}"),
        ),
        MoveNote::ScanTruncated { read } => (
            "scan_truncated",
            format!("the search for the same content stopped at its bound, after {read} files"),
        ),
        MoveNote::UnreadableSkipped { count } => (
            "unreadable_skipped",
            format!("{count} files or directories could not be read, and were passed over"),
        ),
        MoveNote::TooLargeSkipped { count, limit } => (
            "too_large_skipped",
            format!("{count} files larger than {limit} bytes were not read"),
        ),
    };
    MovesNoteOut { kind, detail }
}

/// The text of `moves`: a block per gone name (its space, name and the documents
/// that pin it, then a line per candidate, then its note), and the spaces that
/// cannot suggest.
fn moves_drawing(absent: &[MovesAbsentOut], cannot_suggest: &[String]) -> String {
    let mut text = String::new();
    if absent.is_empty() {
        text.push_str("no pinned name is gone\n");
    }
    for one in absent {
        let by: Vec<&str> = one
            .pinned_by
            .iter()
            .map(|d| d.key.as_deref().unwrap_or(d.id.as_str()))
            .collect();
        text.push_str(&format!(
            "{} {}  pinned by {}\n",
            one.space,
            one.name,
            by.join(", ")
        ));
        if one.candidates.is_empty() {
            text.push_str("  (no candidate)\n");
        }
        let width = one
            .candidates
            .iter()
            .map(|c| c.name.chars().count())
            .max()
            .unwrap_or(0);
        for c in &one.candidates {
            let how = match c.similarity {
                Some(percent) => format!("{} {percent}%", c.found),
                None => c.found.to_owned(),
            };
            let absent = if c.present {
                ""
            } else {
                " (nothing there now)"
            };
            text.push_str(&format!("  -> {:<width$}  {how}{absent}\n", c.name));
        }
        for note in &one.notes {
            text.push_str(&format!("  note ({}): {}\n", note.kind, note.detail));
        }
    }
    if !cannot_suggest.is_empty() {
        text.push_str(&format!("cannot suggest: {}\n", cannot_suggest.join(", ")));
    }
    text
}

fn retrieval_word(retrieval: &Retrieval) -> &'static str {
    match retrieval {
        Retrieval::Read(_) => "read",
        Retrieval::AddressNotReadable(_) => "address_not_readable",
        Retrieval::NoAddress => "no_address",
    }
}

fn retrieval(retrieval: &Retrieval) -> RetrievalOut {
    let read = match retrieval {
        Retrieval::Read(bytes) => Some(bytes),
        Retrieval::AddressNotReadable(_) | Retrieval::NoAddress => None,
    };
    RetrievalOut {
        state: retrieval_word(retrieval),
        text: read.and_then(|b| std::str::from_utf8(b).ok().map(str::to_owned)),
        bytes: read.map(Vec::len),
    }
}

/// The answer of `get <ref> --approved`.
pub fn approved(report: &ApprovedReport) -> ApprovedOut {
    let namer = Namer(&report.names);
    ApprovedOut {
        row: namer.row(&report.row),
        judgement: report.snapshot.judgement().to_string(),
        actor: report.snapshot.actor().as_str().to_owned(),
        at: report.snapshot.at().millis(),
        revision: report.snapshot.revision().map(ToString::to_string),
        items: report
            .items
            .iter()
            .map(|i| ApprovedItemOut {
                item: namer.item(&i.item),
                retrieval: retrieval(&i.retrieval),
            })
            .collect(),
        health: namer.health(&report.health),
    }
}

/// The answer of `diff <ref>`.
pub fn diff(report: &DiffReport) -> DiffOut {
    let namer = Namer(&report.names);
    DiffOut {
        row: namer.row(&report.row),
        judgement: report.snapshot.judgement().to_string(),
        revision: report.snapshot.revision().map(ToString::to_string),
        items: report.items.iter().map(|i| diff_item(&namer, i)).collect(),
        health: namer.health(&report.health),
    }
}

fn diff_item(namer: &Namer<'_>, item: &DiffItem) -> DiffItemOut {
    let out = namer.item(&item.item);
    let current_hash = match &item.now {
        ItemNow::Doc(hash) => hash.map(|h| h.to_string()),
        ItemNow::Artifact(state) => state.hash().map(ToString::to_string),
    };
    let approved_text = match &item.approved {
        Retrieval::Read(bytes) => std::str::from_utf8(bytes).ok(),
        Retrieval::AddressNotReadable(_) | Retrieval::NoAddress => None,
    };
    let current_text = item
        .current
        .as_deref()
        .and_then(|b| std::str::from_utf8(b).ok());
    let label = out
        .path
        .clone()
        .or_else(|| item.file.as_ref().map(ToString::to_string))
        .or_else(|| out.doc.as_ref().and_then(|d| d.key.clone()))
        .unwrap_or_default();
    // What is now is at the name a declared move took the artifact to.
    let now_label = out.read_as.clone().unwrap_or_else(|| label.clone());
    // The diff says what `changed` says: empty when the hash is the same. A document's
    // hash is taken over its text after the reader's normalization (line breaks,
    // trailing white space), so its diff is taken over the normalized texts too. An
    // artifact's hash is its link's comparison of the file (every byte, or the lines
    // with their white space collapsed, without blank and ignored lines), so when it is the same the diff is
    // empty, and when it is not, the diff is of the texts as they are.
    let is_doc = matches!(item.now, ItemNow::Doc(_));
    let diff = match (approved_text, current_text) {
        (Some(_), Some(_)) if !item.changed => Some(String::new()),
        (Some(then), Some(now)) if is_doc => Some(textdiff::unified(
            &normalize(then),
            &normalize(now),
            &format!("approved/{label}"),
            &format!("now/{now_label}"),
        )),
        (Some(then), Some(now)) => Some(textdiff::unified(
            then,
            now,
            &format!("approved/{label}"),
            &format!("now/{now_label}"),
        )),
        _ => None,
    };
    // An artifact that is there now is held by its name, as `file` has always said:
    // the name it is read as, after a declared move.
    let file = match (&item.now, item.item.role()) {
        (ItemNow::Artifact(ArtifactState::Present(_)), ItemRole::Artifact { artifact, .. }) => {
            Some(
                item.item
                    .moved()
                    .map_or(artifact, |m| m.artifact())
                    .name()
                    .to_string(),
            )
        }
        _ => item.file.as_ref().map(ToString::to_string),
    };
    let now = match &item.now {
        ItemNow::Doc(_) => None,
        ItemNow::Artifact(state) => Some(state_word(state)),
    };
    DiffItemOut {
        current_hash,
        changed: item.changed,
        now,
        file,
        retrieval: retrieval_word(&item.approved),
        diff,
        item: out,
    }
}

/// The answer of `fmt`, or of `fmt --check` when `check`.
pub fn fmt(report: &FmtReport, check: bool) -> FmtOut {
    let paths = |files: &[RelPath]| files.iter().map(ToString::to_string).collect();
    FmtOut {
        rewritten: paths(&report.rewritten),
        in_form: report.in_form.len(),
        not_in_form: check.then(|| paths(&report.not_in_form)),
        left_alone: report
            .left_alone
            .iter()
            .map(|left| LeftAloneOut {
                file: left.file.to_string(),
                reason: match left.why {
                    LeftAloneWhy::Unparsed(_) => "unparsed",
                    LeftAloneWhy::Comment { .. } => "comment",
                    LeftAloneWhy::ReadsOtherwise => "reads_otherwise",
                    LeftAloneWhy::ReadOnly => "read_only",
                    LeftAloneWhy::Linked => "linked",
                    LeftAloneWhy::Changed => "changed",
                },
                line: match left.why {
                    LeftAloneWhy::Comment { line } => Some(line),
                    _ => None,
                },
                detail: left_alone_detail(&left.why),
            })
            .collect(),
    }
}

/// The lines `fmt` writes on stderr: one per file it leaves as it is, in the order
/// of `left_alone`.
pub fn fmt_warnings(report: &FmtReport) -> Vec<String> {
    report
        .left_alone
        .iter()
        .map(|left| {
            format!(
                "warning: {} is left as it is: {}",
                left.file,
                left_alone_detail(&left.why)
            )
        })
        .collect()
}

/// The sentence for a file `fmt` leaves as it is.
fn left_alone_detail(why: &LeftAloneWhy) -> String {
    match why {
        LeftAloneWhy::Unparsed(problem) => format!("its front matter cannot be read: {problem}"),
        LeftAloneWhy::Comment { line } => format!(
            "its front matter holds a comment on line {line}, which the form would lose; remove it, or keep it in the body"
        ),
        LeftAloneWhy::ReadsOtherwise => {
            "its text in the form would not read as the file does".to_owned()
        }
        LeftAloneWhy::ReadOnly => "it cannot be written (read-only)".to_owned(),
        LeftAloneWhy::Linked => "it has more than one link (a hard link), which a file written in its place would not keep".to_owned(),
        LeftAloneWhy::Changed => "it changed after it was read; run fmt again".to_owned(),
    }
}

/// The answer of `judge`.
pub fn judged(all: &JudgedAll) -> JudgedAllOut {
    let names = Names::default();
    let namer = Namer(&names);
    JudgedAllOut {
        judged: all
            .judged
            .iter()
            .map(|judged| namer.judged(judged))
            .collect(),
        left_out: all
            .left_out
            .iter()
            .map(|left| namer.left_out(left))
            .collect(),
    }
}

/// The lines `judge` writes on stderr: one per approval it left out, in the order of
/// `left_out`, each with the flag that makes it anyway.
pub fn judged_warnings(all: &JudgedAll) -> Vec<String> {
    let names = Names::default();
    let namer = Namer(&names);
    all.left_out
        .iter()
        .map(|left| {
            format!(
                "{}; pass --again to approve it anyway",
                left_out_warning(&namer, left)
            )
        })
        .collect()
}

/// The answer of `judge approve --stale`.
pub fn stale_approved(done: &StaleApproved) -> StaleApprovedOut {
    let namer = Namer(&done.names);
    StaleApprovedOut {
        judged: done
            .judged
            .iter()
            .map(|judged| namer.judged(judged))
            .collect(),
        left_out: done
            .left_out
            .iter()
            .map(|left| namer.left_out(left))
            .collect(),
    }
}

/// The line on stderr for an approval left out.
fn left_out_warning(namer: &Namer<'_>, left: &LeftOut) -> String {
    let doc = left
        .key
        .as_ref()
        .map_or_else(|| left.doc.to_string(), ToString::to_string);
    match &left.why {
        LeftOutWhy::Broken(_) => format!(
            "warning: {doc} is broken and was not approved again ({})",
            left_out_kinds(namer, left).join(", ")
        ),
        LeftOutWhy::Refused(_) | LeftOutWhy::StandsOn { .. } | LeftOutWhy::Repeats { .. } => {
            format!(
                "warning: {doc} was not approved again: {}",
                left_out_detail(namer, left)
            )
        }
    }
}

/// The lines `judge approve --stale` writes on stderr: one per approval it left out,
/// in the order of `left_out`, then one per name with nothing there that an approval
/// made pinned as absent, in the order of `judged`.
pub fn stale_approved_warnings(done: &StaleApproved) -> Vec<String> {
    let namer = Namer(&done.names);
    let left = done
        .left_out
        .iter()
        .map(|left| left_out_warning(&namer, left));
    let absent = done.judged.iter().flat_map(|judged| {
        let doc = judged
            .key
            .as_ref()
            .map_or_else(|| judged.subject.to_string(), ToString::to_string);
        judged.absent.iter().map(move |(_, artifact)| {
            format!(
                "warning: {doc} pins {} in {}, which has nothing behind it",
                artifact.name(),
                artifact.space()
            )
        })
    });
    left.chain(absent).collect()
}

/// The kinds of the causes of a broken approval left out, each once, in the order
/// found; none for one left out for another reason.
fn left_out_kinds(namer: &Namer<'_>, left: &LeftOut) -> Vec<&'static str> {
    let LeftOutWhy::Broken(degradation) = &left.why else {
        return Vec::new();
    };
    let mut kinds: Vec<&'static str> = Vec::new();
    for cause in degradation.causes().iter() {
        let kind = namer.cause(cause).kind;
        if !kinds.contains(&kind) {
            kinds.push(kind);
        }
    }
    kinds
}

/// The sentence for an approval left out.
fn left_out_detail(namer: &Namer<'_>, left: &LeftOut) -> String {
    match &left.why {
        LeftOutWhy::Broken(_) => format!(
            "its approval is broken ({})",
            left_out_kinds(namer, left).join(", ")
        ),
        LeftOutWhy::Refused(FrameRefusal::UpstreamNotStanding {
            link,
            target,
            status,
        }) => format!(
            "the link `{link}` names {}, which is {status}: an approval cannot stand on it",
            namer
                .0
                .key_of(*target)
                .map_or_else(|| target.to_string(), ToString::to_string)
        ),
        LeftOutWhy::Refused(refusal) => refusal.to_string(),
        LeftOutWhy::Repeats { approval } => format!(
            "an approval now would pin what its approval {approval} pins and lapse when it lapses: it would only repeat it"
        ),
        LeftOutWhy::StandsOn { held, key } => {
            let upstream = key
                .as_ref()
                .map_or_else(|| held.upstream.to_string(), ToString::to_string);
            let link = &held.link;
            match held.hold {
                Hold::Broken => format!("the link `{link}` names {upstream}, which is broken"),
                Hold::Pending => format!(
                    "the link `{link}` names {upstream}, which is pending: approve it first with `judge approve`"
                ),
                Hold::LeftOut => {
                    format!("the link `{link}` names {upstream}, which was left out of this run")
                }
            }
        }
    }
}

/// The word for why an upstream holds an approval back.
fn hold_word(hold: Hold) -> &'static str {
    match hold {
        Hold::Broken => "broken",
        Hold::Pending => "pending",
        Hold::LeftOut => "left_out",
    }
}

impl Namer<'_> {
    fn left_out(&self, left: &LeftOut) -> LeftOutOut {
        let (upstream, link, upstream_is) = match &left.why {
            LeftOutWhy::StandsOn { held, .. } => (
                Some(self.doc(held.upstream)),
                Some(held.link.to_string()),
                Some(hold_word(held.hold)),
            ),
            LeftOutWhy::Broken(_) | LeftOutWhy::Refused(_) | LeftOutWhy::Repeats { .. } => {
                (None, None, None)
            }
        };
        LeftOutOut {
            doc: DocRef {
                id: left.doc.to_string(),
                key: left.key.as_ref().map(ToString::to_string),
            },
            reason: match left.why {
                LeftOutWhy::Broken(_) => "broken",
                LeftOutWhy::Refused(_) => "refused",
                LeftOutWhy::StandsOn { .. } => "stands_on",
                LeftOutWhy::Repeats { .. } => "repeats",
            },
            detail: left_out_detail(self, left),
            upstream,
            link,
            upstream_is,
            approval: match &left.why {
                LeftOutWhy::Repeats { approval } => Some(approval.to_string()),
                LeftOutWhy::Broken(_) | LeftOutWhy::Refused(_) | LeftOutWhy::StandsOn { .. } => {
                    None
                }
            },
            kinds: left_out_kinds(self, left),
            causes: match &left.why {
                LeftOutWhy::Broken(degradation) => {
                    degradation.causes().iter().map(|c| self.cause(c)).collect()
                }
                LeftOutWhy::Refused(_)
                | LeftOutWhy::StandsOn { .. }
                | LeftOutWhy::Repeats { .. } => Vec::new(),
            },
        }
    }

    fn judged(&self, judged: &Judged) -> JudgedOut {
        JudgedOut {
            doc: DocRef {
                id: judged.subject.to_string(),
                key: judged.key.as_ref().map(ToString::to_string),
            },
            record: to_json(&judged.record.to_canon()),
            successors: judged
                .successors
                .as_ref()
                .map(|s| self.successors(s))
                .unwrap_or_default(),
            absent: judged
                .absent
                .iter()
                .map(|(link, artifact)| AbsentNameOut {
                    link: link.to_string(),
                    path: artifact.name().to_string(),
                    name: artifact.name().to_string(),
                    space: artifact.space().to_string(),
                    detail: absent_detail(artifact),
                })
                .collect(),
        }
    }
}

/// The answer of `judge same`.
pub fn same_meaning(all: &[Declared]) -> DeclaredAllOut {
    DeclaredAllOut {
        declared: all
            .iter()
            .map(|one| {
                let (doc, artifact) = match &one.about {
                    DeclaredAbout::Doc { id, key } => (
                        Some(DocRef {
                            id: id.to_string(),
                            key: key.as_ref().map(ToString::to_string),
                        }),
                        None,
                    ),
                    DeclaredAbout::Artifact { artifact } => (None, Some(artifact)),
                };
                DeclaredOut {
                    doc,
                    artifact: artifact.map(|a| a.name().to_string()),
                    space: artifact.map(|a| a.space().to_string()),
                    record: to_json(&one.record.to_canon()),
                }
            })
            .collect(),
    }
}

/// The answer of `judge moved`.
pub fn moved(all: &[Moved]) -> MovedAllOut {
    MovedAllOut {
        moved: all
            .iter()
            .map(|one| MovedOut {
                space: one.of().space().to_string(),
                from: one.of().from_name().to_string(),
                to: one.of().to_name().to_string(),
                record: to_json(&Record::Moved(one.clone()).to_canon()),
            })
            .collect(),
    }
}

/// The answer of `judge withdraw`.
pub fn withdrawn(record: &Record) -> WithdrawnOut {
    WithdrawnOut {
        record: to_json(&record.to_canon()),
    }
}

/// The answer of a command that gave none.
pub fn error(error: &ShelfError, messages: Vec<String>) -> ErrorOut {
    ErrorOut {
        error: match error.class() {
            ErrorClass::Request => "request",
            ErrorClass::Refused => "refused",
            ErrorClass::Failed => "failed",
        },
        messages,
        candidates: match error {
            ShelfError::Ambiguous { candidates, .. } => {
                candidates.iter().map(ToString::to_string).collect()
            }
            _ => Vec::new(),
        },
        problems: match error {
            ShelfError::Schema(problems) => problems.iter().map(schema_problem).collect(),
            ShelfError::NewDoc(NewDocProblem::Invalid(problems)) => {
                problems.iter().map(ToString::to_string).collect()
            }
            ShelfError::InTheWay { obstacles } => {
                obstacles.iter().map(ToString::to_string).collect()
            }
            ShelfError::PartWay {
                written,
                failed,
                cause,
            } => written
                .iter()
                .map(|path| format!("{path} was written"))
                .chain(std::iter::once(format!(
                    "{failed} was not written: {cause}"
                )))
                .collect(),
            _ => Vec::new(),
        },
    }
}

/// The answer to arguments that are no request.
pub fn usage(message: String) -> ErrorOut {
    ErrorOut {
        error: "request",
        messages: vec![message],
        candidates: Vec::new(),
        problems: Vec::new(),
    }
}

/// The answer when the program cannot make its own request (a template it ships is
/// not one): a run that failed.
pub fn failure(messages: Vec<String>) -> ErrorOut {
    ErrorOut {
        error: "failed",
        messages,
        candidates: Vec::new(),
        problems: Vec::new(),
    }
}

/// A text as one word of a POSIX shell: as it is when it holds nothing the shell
/// reads, otherwise in single quotes (a `'` in it as `'\''`).
fn shell_word(text: &str) -> String {
    let plain = !text.is_empty()
        && text
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "/._-+,:@%=".contains(c));
    if plain {
        text.to_owned()
    } else {
        format!("'{}'", text.replace('\'', r"'\''"))
    }
}

/// The reason the `next` of `init` gives to the approval of the documents it made,
/// for a person to replace with theirs.
const INIT_REASON: &str = "read, and it holds";

/// The root `init` ran with, as the `next` commands are to carry it.
#[derive(Debug, Clone, Copy)]
pub enum NextRoot<'a> {
    /// No `--root`: the commands carry none.
    Current,
    /// `--root` as typed: each command carries `--root=<it>`, one shell word.
    Given(&'a str),
    /// A `--root` that is no UTF-8 text: no command can be written with it, so
    /// none is given (the command line says so on stderr).
    NotText,
}

/// The answer of `init`. `book` is the book `next` makes a first document of, when
/// `init` made none; `root` the `--root` it ran with, which every command of `next`
/// carries.
pub fn initialized(done: &Initialized, book: Option<&str>, root: NextRoot<'_>) -> InitOut {
    let whyshelf = match root {
        NextRoot::Given(root) => format!("whyshelf --root={}", shell_word(root)),
        NextRoot::Current => "whyshelf".to_owned(),
        NextRoot::NotText => String::new(),
    };
    let mut next = vec![format!("{whyshelf} check"), format!("{whyshelf} map")];
    if matches!(root, NextRoot::NotText) {
        next.clear();
    } else if done.docs.is_empty() {
        next.extend(book.map(|book| format!("{whyshelf} new {book} --title \"A first {book}\"")));
    } else {
        let keys: Vec<String> = done.docs.iter().map(|d| d.key.form.to_string()).collect();
        next.push(format!(
            "{whyshelf} judge approve {} --reason \"{INIT_REASON}\"",
            keys.join(" ")
        ));
    }
    InitOut {
        declaration: done.declaration.to_string(),
        documents: done
            .docs
            .iter()
            .map(|d| InitDocOut {
                key: d.key.to_string(),
                path: d.file.to_string(),
            })
            .collect(),
        next,
    }
}

/// The answer of `new`.
pub fn created(created: &Created) -> CreatedOut {
    let names = Names::default();
    let namer = Namer(&names);
    CreatedOut {
        doc: DocRef {
            id: created.id.to_string(),
            key: Some(created.key.to_string()),
        },
        file: created.file.to_string(),
        unresolved_links: created
            .unresolved
            .iter()
            .map(|(link, unresolved)| UnresolvedOut {
                link: link.to_string(),
                written: unresolved.written().to_owned(),
                why: namer.unresolved(unresolved),
            })
            .collect(),
    }
}

fn presence(presence: &DocPresence) -> PresenceOut {
    match presence {
        DocPresence::Valid { origin, hash, .. } => PresenceOut::Valid {
            file: origin.to_string(),
            hash: hash.to_string(),
        },
        DocPresence::Invalid { files, problems } => PresenceOut::Invalid {
            files: files.iter().map(ToString::to_string).collect(),
            problems: problems.iter().map(ToString::to_string).collect(),
        },
        DocPresence::Gone => PresenceOut::Gone,
        DocPresence::Unknown { unreadable } => PresenceOut::Unknown {
            unreadable: unreadable.iter().map(ToString::to_string).collect(),
        },
    }
}

/// The sentence for a name with nothing there.
fn absent_detail(artifact: &ArtifactKey) -> String {
    format!(
        "{artifact} has nothing behind it now: the evidence is not written yet, or the name is wrong"
    )
}

/// The sentence for a link that still writes a name its pin was moved away from.
fn moved_detail(link: &EntryName, written: &ArtifactKey, read_as: &ArtifactKey) -> String {
    format!(
        "the link `{link}` writes {written}, which was declared moved away since the approval pinned it: the pin is read as `{}` now, and the link is to write that name",
        read_as.name()
    )
}

/// The word for what an artifact is now: `present`, `absent` or `unknown`.
fn state_word(state: &ArtifactState) -> &'static str {
    match state {
        ArtifactState::Present(_) => "present",
        ArtifactState::Absent => "absent",
        ArtifactState::Unknown(_) => "unknown",
    }
}

/// The sentence for a name its adapter cannot tell about now.
fn unknown_detail(artifact: &ArtifactKey, why: &str) -> String {
    format!(
        "{artifact} cannot be told about now ({why}): it changes no freshness, and no approval can be made on it"
    )
}

fn declared(status: &DeclaredStatus) -> (&'static str, Option<i64>) {
    match status {
        DeclaredStatus::Pending => ("pending", None),
        DeclaredStatus::Approved { expires, .. } => ("approved", expires.map(|e| e.millis())),
        DeclaredStatus::Denied { .. } => ("denied", None),
        DeclaredStatus::Deprecated { .. } => ("deprecated", None),
        DeclaredStatus::Retired { .. } => ("retired", None),
    }
}

fn standing(kind: StandingKind) -> &'static str {
    match kind {
        StandingKind::Pending => "pending",
        StandingKind::Denied => "denied",
        StandingKind::Deprecated => "deprecated",
        StandingKind::Retired => "retired",
        StandingKind::Approved(FreshnessLevel::Fresh) => "fresh",
        StandingKind::Approved(FreshnessLevel::Expired) => "expired",
        StandingKind::Approved(FreshnessLevel::Stale) => "stale",
        StandingKind::Approved(FreshnessLevel::Broken) => "broken",
    }
}

fn severity(severity: Severity) -> &'static str {
    match severity {
        Severity::Expired => "expired",
        Severity::Stale => "stale",
        Severity::Broken => "broken",
    }
}

fn upstream_status(status: &UpstreamStatus) -> &'static str {
    match status {
        UpstreamStatus::Pending => "pending (its approval was withdrawn)",
        UpstreamStatus::Denied => "denied",
        UpstreamStatus::Retired => "retired",
        UpstreamStatus::Deprecated {
            successors: Successors::None,
        } => "deprecated with no successor",
        UpstreamStatus::Deprecated {
            successors: Successors::Some(_),
        } => "deprecated; follow its successor",
    }
}

fn means(propagation: Propagation) -> &'static str {
    match propagation {
        Propagation::Depends => "depends",
        Propagation::Informs => "informs",
        Propagation::Replaces => "replaces",
    }
}

/// A count as the declaration writes it.
fn count_word(cardinality: Cardinality) -> &'static str {
    match cardinality {
        Cardinality::One => "one",
        Cardinality::Optional => "optional",
        Cardinality::Many => "many",
        Cardinality::AtLeastOne => "at-least-one",
    }
}

/// The type of a field as the declaration writes it.
fn field_type(ty: &FieldType) -> &'static str {
    match ty {
        FieldType::Text => "text",
        FieldType::Integer => "integer",
        FieldType::Bool => "bool",
        FieldType::TextList => "list",
        FieldType::Enum(_) => "enum",
    }
}

fn field(value: &FieldValue) -> Value {
    match value {
        FieldValue::Text(s) => Value::String(s.clone()),
        FieldValue::Integer(n) => Value::from(*n),
        FieldValue::Bool(b) => Value::Bool(*b),
        FieldValue::TextList(items) => {
            Value::Array(items.iter().cloned().map(Value::String).collect())
        }
        FieldValue::Enum(word) => Value::String(word.as_str().to_owned()),
    }
}

fn artifact_target(artifact: &Artifact) -> LinkTargetOut {
    LinkTargetOut::Artifact {
        path: artifact.name().to_string(),
        name: artifact.name().to_string(),
        space: artifact.space().to_string(),
        now: state_word(artifact.state()),
        hash: artifact.state().hash().map(ToString::to_string),
    }
}

/// The hash an artifact pin kept, or `absent`.
fn pinned_hash(pin: &ArtifactPin) -> String {
    pin.seen()
        .state()
        .hash()
        .map_or_else(|| "absent".to_owned(), ToString::to_string)
}

/// A comparison as a person reads it: `bytes`, or `lines` with the prefixes it
/// leaves out.
fn comparison(comparison: &Comparison) -> String {
    let ignored: Vec<&str> = comparison.ignored().map(|p| p.as_str()).collect();
    if ignored.is_empty() {
        comparison.kind_word().to_owned()
    } else {
        format!(
            "{} (ignoring {})",
            comparison.kind_word(),
            ignored.join(" ")
        )
    }
}

fn join<T: std::fmt::Display>(items: impl Iterator<Item = T>) -> String {
    items.map(|i| i.to_string()).collect::<Vec<_>>().join("; ")
}

mod diagram;
mod summary;

#[cfg(test)]
mod comparison_tests;
#[cfg(test)]
mod cover_tests;
#[cfg(test)]
mod map_tests;
#[cfg(test)]
mod same_meaning_tests;
#[cfg(test)]
mod stats_tests;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_schema_covers_every_answer_with_its_version() {
        let schemas = schemas();
        assert_eq!(schemas.version, FORMAT_VERSION);
        let outputs = schemas.outputs;
        for command in [
            "check",
            "map",
            "status",
            "get",
            "get --approved",
            "trace",
            "stale",
            "stats",
            "diff",
            "judge",
            "judge approve --stale",
            "judge same",
            "judge withdraw",
            "new",
            "init",
            "fmt",
            "error",
        ] {
            assert!(outputs.contains_key(command), "{command}");
            let schema = serde_json::to_string(&outputs[command]).unwrap();
            assert!(schema.contains("\"version\""), "{command} has no version");
        }
        let new = serde_json::to_string(&outputs["new"]).unwrap();
        for field in ["doc", "file", "unresolved_links", "written", "why"] {
            assert!(new.contains(&format!("\"{field}\"")), "{field}");
        }
        let init = serde_json::to_string(&outputs["init"]).unwrap();
        for field in ["declaration", "documents", "key", "path", "next"] {
            assert!(init.contains(&format!("\"{field}\"")), "{field}");
        }
        let fmt = serde_json::to_string(&outputs["fmt"]).unwrap();
        for field in [
            "rewritten",
            "in_form",
            "not_in_form",
            "left_alone",
            "reason",
        ] {
            assert!(fmt.contains(&format!("\"{field}\"")), "{field}");
        }
        let judge = serde_json::to_string(&outputs["judge"]).unwrap();
        for field in ["judged", "left_out", "reason", "approval"] {
            assert!(judge.contains(&format!("\"{field}\"")), "{field}");
        }
        let error = serde_json::to_string(&outputs["error"]).unwrap();
        assert!(error.contains("\"problems\""));
        let diff = serde_json::to_string(&outputs["diff"]).unwrap();
        for field in ["approved_hash", "current_hash", "retrieval", "diff"] {
            assert!(diff.contains(field), "{field}");
        }
        let stats = serde_json::to_string(&outputs["stats"]).unwrap();
        for field in [
            "since",
            "approvals",
            "again",
            "revoked",
            "pending",
            "median_ms",
            "p90_ms",
            "runs",
            "largest",
            "origins",
            "health",
            "drawing",
        ] {
            assert!(stats.contains(&format!("\"{field}\"")), "{field}");
        }
        // A cause names the artifact it is about, beside the fields it had.
        let stale = serde_json::to_string(&outputs["stale"]).unwrap();
        for field in ["kind", "severity", "upstream", "link", "detail", "artifact"] {
            assert!(stale.contains(&format!("\"{field}\"")), "{field}");
        }
    }

    #[test]
    fn a_root_is_one_shell_word_in_the_next_commands() {
        assert_eq!(shell_word("../my-project"), "../my-project");
        assert_eq!(shell_word("/tmp/a b"), "'/tmp/a b'");
        assert_eq!(shell_word("it's"), r"'it'\''s'");
        assert_eq!(shell_word(""), "''");
    }

    #[test]
    fn a_retrieval_shows_text_only_when_it_was_read_as_utf8() {
        let read = retrieval(&Retrieval::Read(b"text\n".to_vec()));
        assert_eq!(
            (read.state, read.text.as_deref(), read.bytes),
            ("read", Some("text\n"), Some(5))
        );
        let binary = retrieval(&Retrieval::Read(vec![0xff, 0xfe]));
        assert_eq!((binary.text, binary.bytes), (None, Some(2)));
        let none = retrieval(&Retrieval::NoAddress);
        assert_eq!((none.state, none.bytes), ("no_address", None));
    }
}
