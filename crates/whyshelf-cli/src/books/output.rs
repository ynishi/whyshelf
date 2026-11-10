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
    DownstreamEdge, ErrorClass, GetReport, Health, Judged, Names, Row, ShelfError, StaleReport,
    StatusReport, TraceReport, UpstreamEdge, UpstreamEnd,
};
use whyshelf_core::domain::shelf::build::NewDocProblem;
use whyshelf_core::domain::shelf::snapshot::{
    ItemNow, ItemRole, Retrieval, SeenHash, SnapshotItem,
};

use super::textdiff;
use whyshelf_core::domain::id::DocId;
use whyshelf_core::domain::location::RelPath;
use whyshelf_core::domain::shelf::artifact::{Artifact, Comparison};
use whyshelf_core::domain::shelf::build::{DocFailure, DocPresence};
use whyshelf_core::domain::shelf::doc::{
    Doc, FieldValue, LinkState, UnresolvedLink, UnresolvedWhy, normalize,
};
use whyshelf_core::domain::shelf::freshness::{
    Cause, Freshness, FreshnessLevel, Notice, Origin, RootCause, Severity, Standing, StandingKind,
    Successors, UpstreamStatus,
};
use whyshelf_core::domain::shelf::graph::CycleSource;
use whyshelf_core::domain::shelf::ledger::{ArtifactPin, DeclaredStatus, FramePin, Record};
use whyshelf_core::domain::shelf::schema::Propagation;
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
    /// The path of the artifact the cause names, if it names one
    /// (`artifact_changed`, `artifact_gone`, `artifact_arrived`,
    /// `artifact_comparison_changed`).
    pub artifact: Option<String>,
    /// A sentence for a person.
    pub detail: String,
}

/// Something worth knowing that changes no freshness.
#[derive(Debug, Serialize, JsonSchema)]
pub struct NoticeOut {
    /// What kind (`informs_not_fresh`, `informs_unresolved`, `upstream_expired`,
    /// `self_same_meaning`, `upstream_same_meaning`, `artifact_same_meaning`,
    /// `not_covered`, `artifact_absent`).
    pub kind: &'static str,
    /// The link; empty for a notice about the document itself.
    pub link: String,
    /// The document it names, if it names one.
    pub target: Option<DocRef>,
    /// The file it names, a path under the root, if it names one.
    pub artifact: Option<String>,
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
    /// Files that are no valid document.
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
#[derive(Debug, Serialize, JsonSchema)]
pub struct BooksCheckOut {
    /// Whether nothing is wrong.
    pub clean: bool,
    /// The problems of the declaration; when there are any, nothing else was read.
    pub schema_problems: Vec<String>,
    /// The books.
    pub books: Vec<BookOut>,
    /// What is wrong around the documents.
    pub health: Option<HealthOut>,
    /// The documents a covering link (`cover = true`) is expected to reach and does
    /// not; any makes `clean` false.
    pub uncovered: Vec<UncoveredOut>,
    /// The paths of pending and approved documents that name no file; they leave
    /// `clean` as it is, since a path may name evidence not written yet.
    pub absent: Vec<AbsentOut>,
}

/// A path a link to artifacts writes that names no file.
#[derive(Debug, Serialize, JsonSchema)]
pub struct AbsentOut {
    /// The document.
    pub doc: DocRef,
    /// The link.
    pub link: String,
    /// The path as written.
    pub path: String,
    /// A sentence for a person.
    pub detail: String,
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
    /// A file that is no document (a link with `target = "artifact"`).
    Artifact {
        /// Its path under the root.
        path: String,
        /// The hash of its content now; none when no file is there.
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
    /// Where they start: each document, file, cycle or the declaration an event
    /// starts at, with the approvals of `stale` it degrades; the one that degrades
    /// the most first.
    pub origins: Vec<OriginOut>,
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
    Doc {
        /// The document.
        doc: DocRef,
    },
    /// A file that is no document.
    Artifact {
        /// Its path from the project root.
        path: String,
    },
    /// The declaration of the books: a link to files declares another comparison.
    Declaration,
    /// A cycle (see `health.cycles`).
    Cycle {
        /// `links` or `frames`.
        among: &'static str,
        /// Its smallest member, by identity.
        smallest: DocRef,
    },
}

/// What an item of an approval is, and which document or file it names.
#[derive(Debug, Serialize, JsonSchema)]
pub struct ItemOut {
    /// `subject` (the approved document), `upstream` (a document it stood on) or
    /// `artifact` (a file it stood on).
    pub role: &'static str,
    /// The document, for a subject or an upstream (an upstream's key is the one it
    /// had when it was pinned).
    pub doc: Option<DocRef>,
    /// The link it was pinned through, for an upstream or an artifact.
    pub link: Option<String>,
    /// The path, for an artifact.
    pub path: Option<String>,
    /// The hash the approval pinned; none for an artifact that had no file.
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
    /// The hash it has now; none when it is no valid document, or no file is there.
    pub current_hash: Option<String>,
    /// Whether the hash is not the one approved.
    pub changed: bool,
    /// The file that holds it now, if one does.
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

/// The answer of `judge`: every record written, in the order the documents
/// were named.
#[derive(Debug, Serialize, JsonSchema)]
pub struct JudgedAllOut {
    /// The judgements.
    pub judged: Vec<JudgedOut>,
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
    /// The file it is about, a path under the root, if it is about one.
    pub artifact: Option<String>,
    /// The record written, as it is in the ledger (its `from` lists the earlier
    /// hashes, its `to` the hash now).
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
    /// For an approval, the paths of the document's links to artifacts that name
    /// no file; a `depends` link pinned each as absent.
    pub absent: Vec<AbsentPathOut>,
}

/// A path of a link to artifacts that names no file.
#[derive(Debug, Serialize, JsonSchema)]
pub struct AbsentPathOut {
    /// The link.
    pub link: String,
    /// The path as written.
    pub path: String,
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
    /// invalid.
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
    put("status", schema_for!(StatusOut));
    put("get", schema_for!(GetOut));
    put("get --approved", schema_for!(ApprovedOut));
    put("trace", schema_for!(TraceOut));
    put("stale", schema_for!(StaleOut));
    put("diff", schema_for!(DiffOut));
    put("judge", schema_for!(JudgedAllOut));
    put("judge same", schema_for!(DeclaredAllOut));
    put("judge withdraw", schema_for!(WithdrawnOut));
    put("new", schema_for!(CreatedOut));
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
            detail,
        };
        let artifact = |kind, pin: &ArtifactPin, detail: String| CauseOut {
            kind,
            severity,
            upstream: None,
            link: Some(pin.link().to_string()),
            artifact: Some(pin.path().to_string()),
            detail,
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
                    "`{}` changed since the approval pinned it ({} -> {now})",
                    pin.path(),
                    pinned_hash(pin)
                ),
            ),
            Cause::ArtifactComparisonChanged { pin, now } => artifact(
                "artifact_comparison_changed",
                pin,
                format!(
                    "the link `{}` compares `{}` as {} now, and it was pinned as {}: the hashes are not compared",
                    pin.link(),
                    pin.path(),
                    comparison(now),
                    comparison(pin.comparison())
                ),
            ),
            Cause::ArtifactGone { pin } => artifact(
                "artifact_gone",
                pin,
                format!(
                    "`{}` was there when it was pinned and is gone (a path is compared as written, ASCII case included)",
                    pin.path()
                ),
            ),
            Cause::ArtifactArrived { pin, now } => artifact(
                "artifact_arrived",
                pin,
                format!(
                    "`{}` was not there when it was pinned and is now ({now}): look again",
                    pin.path()
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
                link: link.to_string(),
                target: Some(self.doc(*target)),
                artifact: None,
                declared: Vec::new(),
                detail: format!("a document it informs of is {}", standing(*kind)),
            },
            Notice::InformsUnresolved { link, unresolved } => NoticeOut {
                kind: "informs_unresolved",
                link: link.to_string(),
                target: None,
                artifact: None,
                declared: Vec::new(),
                detail: self.unresolved(unresolved),
            },
            Notice::UpstreamExpired { pin, since } => NoticeOut {
                kind: "upstream_expired",
                link: pin.link().to_string(),
                target: Some(self.pin(pin)),
                artifact: None,
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
                link: String::new(),
                target: None,
                artifact: None,
                declared: declared.iter().map(ToString::to_string).collect(),
                detail: format!(
                    "the document was edited since its approval ({approved} -> {now}), and the edit was declared to keep the meaning"
                ),
            },
            Notice::UpstreamSameMeaning { pin, now, declared } => NoticeOut {
                kind: "upstream_same_meaning",
                link: pin.link().to_string(),
                target: Some(self.pin(pin)),
                artifact: None,
                declared: declared.iter().map(ToString::to_string).collect(),
                detail: format!(
                    "{} was edited since it was pinned ({} -> {now}), and the edit was declared to keep the meaning",
                    pin.key(),
                    pin.hash()
                ),
            },
            Notice::ArtifactSameMeaning { pin, now, declared } => NoticeOut {
                kind: "artifact_same_meaning",
                link: pin.link().to_string(),
                target: None,
                artifact: Some(pin.path().to_string()),
                declared: declared.iter().map(ToString::to_string).collect(),
                detail: format!(
                    "`{}` changed since it was pinned (now {now}), and the change was declared to keep the meaning",
                    pin.path()
                ),
            },
            Notice::ArtifactAbsent { link, path } => NoticeOut {
                kind: "artifact_absent",
                link: link.to_string(),
                target: None,
                artifact: Some(path.to_string()),
                declared: Vec::new(),
                detail: absent_detail(path),
            },
            Notice::NotCovered { book, link } => NoticeOut {
                kind: "not_covered",
                link: link.to_string(),
                target: None,
                artifact: None,
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

/// The answer of `check`.
pub fn books_check(check: &BooksCheck) -> BooksCheckOut {
    match check {
        BooksCheck::SchemaInvalid(problems) => BooksCheckOut {
            clean: false,
            schema_problems: problems.iter().map(ToString::to_string).collect(),
            books: Vec::new(),
            health: None,
            uncovered: Vec::new(),
            absent: Vec::new(),
        },
        BooksCheck::Checked {
            books,
            health,
            uncovered,
            absent,
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
                        path: a.path.to_string(),
                        detail: absent_detail(&a.path),
                    })
                    .collect(),
            }
        }
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
                    Origin::Doc(id) => StartOut::Doc {
                        doc: namer.doc(*id),
                    },
                    Origin::Artifact(path) => StartOut::Artifact {
                        path: path.to_string(),
                    },
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
        health: namer.health(&report.health),
    }
}

impl Namer<'_> {
    fn item(&self, item: &SnapshotItem) -> ItemOut {
        let (role, doc, link, path) = match item.role() {
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
            ItemRole::Artifact { link, path, .. } => (
                "artifact",
                None,
                Some(link.to_string()),
                Some(path.to_string()),
            ),
        };
        let approved_hash = match item.seen() {
            SeenHash::Doc(hash) => Some(hash.to_string()),
            SeenHash::Artifact(state) => state.hash().map(ToString::to_string),
        };
        ItemOut {
            role,
            doc,
            link,
            path,
            approved_hash,
            address: item.address().map(ToString::to_string),
        }
    }
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
            &format!("now/{label}"),
        )),
        (Some(then), Some(now)) => Some(textdiff::unified(
            then,
            now,
            &format!("approved/{label}"),
            &format!("now/{label}"),
        )),
        _ => None,
    };
    DiffItemOut {
        current_hash,
        changed: item.changed,
        file: item.file.as_ref().map(ToString::to_string),
        retrieval: retrieval_word(&item.approved),
        diff,
        item: out,
    }
}

/// The answer of `judge`.
pub fn judged(all: &[Judged]) -> JudgedAllOut {
    let names = Names::default();
    let namer = Namer(&names);
    JudgedAllOut {
        judged: all
            .iter()
            .map(|judged| JudgedOut {
                doc: DocRef {
                    id: judged.subject.to_string(),
                    key: judged.key.as_ref().map(ToString::to_string),
                },
                record: to_json(&judged.record.to_canon()),
                successors: judged
                    .successors
                    .as_ref()
                    .map(|s| namer.successors(s))
                    .unwrap_or_default(),
                absent: judged
                    .absent
                    .iter()
                    .map(|(link, path)| AbsentPathOut {
                        link: link.to_string(),
                        path: path.to_string(),
                        detail: absent_detail(path),
                    })
                    .collect(),
            })
            .collect(),
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
                    DeclaredAbout::Artifact { path } => (None, Some(path.to_string())),
                };
                DeclaredOut {
                    doc,
                    artifact,
                    record: to_json(&one.record.to_canon()),
                }
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
            ShelfError::Schema(problems) => problems.iter().map(ToString::to_string).collect(),
            ShelfError::NewDoc(NewDocProblem::Invalid(problems)) => {
                problems.iter().map(ToString::to_string).collect()
            }
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

/// The sentence for a path that names no file.
fn absent_detail(path: &RelPath) -> String {
    format!(
        "`{path}` names no file: the evidence is not written yet, or the path is wrong (it is compared as written, ASCII case included)"
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
        path: artifact.path().to_string(),
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

#[cfg(test)]
mod comparison_tests;
#[cfg(test)]
mod cover_tests;
#[cfg(test)]
mod same_meaning_tests;

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
            "status",
            "get",
            "get --approved",
            "trace",
            "stale",
            "diff",
            "judge",
            "judge same",
            "judge withdraw",
            "new",
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
        let error = serde_json::to_string(&outputs["error"]).unwrap();
        assert!(error.contains("\"problems\""));
        let diff = serde_json::to_string(&outputs["diff"]).unwrap();
        for field in ["approved_hash", "current_hash", "retrieval", "diff"] {
            assert!(diff.contains(field), "{field}");
        }
        // A cause names the artifact it is about, beside the fields it had.
        let stale = serde_json::to_string(&outputs["stale"]).unwrap();
        for field in ["kind", "severity", "upstream", "link", "detail", "artifact"] {
            assert!(stale.contains(&format!("\"{field}\"")), "{field}");
        }
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
