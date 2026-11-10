//! The JSON form of every answer of `books`: one serde type per command, the same
//! type `books schema` describes, so the schema and the output cannot drift apart.
//!
//! A report of the use cases holds domain values; this module writes them as data
//! an agent can read without knowing Rust: a document is `{id, key}`, a state is a
//! word, a cause has a `kind` word, the document it names, and a sentence.

use schemars::{JsonSchema, Schema, schema_for};
use serde::Serialize;
use serde_json::Value;
use whyshelf_core::app::shelf::{
    BooksCheck, DownstreamEdge, ErrorClass, GetReport, Health, Judged, Names, Presence, Row,
    ShelfError, StaleReport, StatusReport, TraceReport, UpstreamEdge, UpstreamEnd,
};
use whyshelf_core::domain::id::DocId;
use whyshelf_core::domain::shelf::doc::{
    Doc, FieldValue, FramePin, LinkState, UnresolvedLink, UnresolvedWhy,
};
use whyshelf_core::domain::shelf::freshness::{
    Cause, Freshness, FreshnessLevel, Notice, RootCause, Severity, Standing, StandingKind,
    Successors, UpstreamStatus,
};
use whyshelf_core::domain::shelf::ledger::DeclaredStatus;
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
    /// A sentence for a person.
    pub detail: String,
}

/// Something worth knowing that changes no freshness.
#[derive(Debug, Serialize, JsonSchema)]
pub struct NoticeOut {
    /// What kind (`informs_not_fresh`, `informs_unresolved`, `upstream_expired`).
    pub kind: &'static str,
    /// The link.
    pub link: String,
    /// The document it names, if it names one.
    pub target: Option<DocRef>,
    /// A sentence for a person.
    pub detail: String,
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

/// The answer of `books check`.
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
}

/// The answer of `books status`.
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

/// The answer of `books get`.
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

/// One link followed in `books trace`.
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

/// The answer of `books trace`.
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

/// The answer of `books stale`.
#[derive(Debug, Serialize, JsonSchema)]
pub struct StaleOut {
    /// The approvals that do not hold, strongest first.
    pub stale: Vec<StaleEntryOut>,
    /// What is wrong around the documents.
    pub health: HealthOut,
}

/// The answer of `books judge`.
#[derive(Debug, Serialize, JsonSchema)]
pub struct JudgedOut {
    /// The document judged.
    pub doc: DocRef,
    /// The record written, as it is in the ledger.
    pub record: Value,
    /// For a deprecation, what replaces the document.
    pub successors: Vec<DocRef>,
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
}

/// The JSON Schema of every answer.
#[derive(Debug, Serialize)]
pub struct SchemasOut {
    /// By command.
    pub outputs: serde_json::Map<String, Value>,
}

/// The JSON Schema of every answer, by command.
pub fn schemas() -> SchemasOut {
    let mut outputs = serde_json::Map::new();
    let mut put = |name: &str, schema: Schema| {
        outputs.insert(name.to_owned(), schema.to_value());
    };
    put("books check", schema_for!(BooksCheckOut));
    put("books status", schema_for!(StatusOut));
    put("books get", schema_for!(GetOut));
    put("books trace", schema_for!(TraceOut));
    put("books stale", schema_for!(StaleOut));
    put("books judge", schema_for!(JudgedOut));
    put("error", schema_for!(ErrorOut));
    SchemasOut { outputs }
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
            Standing::Approved(Freshness::Degraded { causes, .. }) => {
                (Vec::new(), causes.iter().map(|c| self.cause(c)).collect())
            }
            _ => (Vec::new(), Vec::new()),
        };
        RowOut {
            doc: self.doc(row.id),
            presence: presence(&row.presence),
            declared,
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
            detail,
        };
        match cause {
            Cause::SelfChanged { approved, now } => out(
                "self_changed",
                None,
                format!("the document changed since it was approved ({approved} -> {now})"),
            ),
            Cause::SelfInvalid { problems } => out(
                "self_invalid",
                None,
                format!("the document is invalid: {}", join(problems.iter())),
            ),
            Cause::SelfGone => out("self_gone", None, "no file carries the document".to_owned()),
            Cause::SelfUnknown { unreadable } => out(
                "self_unknown",
                None,
                format!(
                    "no readable file carries the document; unreadable: {}",
                    join(unreadable.iter())
                ),
            ),
            Cause::UpstreamGone { pin } => {
                out("upstream_gone", Some(pin), format!("{} is gone", pin.key()))
            }
            Cause::UpstreamInvalid { pin } => out(
                "upstream_invalid",
                Some(pin),
                format!("{} is invalid", pin.key()),
            ),
            Cause::UpstreamUnknown { pin, unreadable } => out(
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
                detail: format!(
                    "the link `{link}` changed since the approval: {} added, {} removed, {} unresolved",
                    added.len(),
                    removed.len(),
                    unresolved.len()
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
                detail: format!("a document it informs of is {}", standing(*kind)),
            },
            Notice::InformsUnresolved { link, unresolved } => NoticeOut {
                kind: "informs_unresolved",
                link: link.to_string(),
                target: None,
                detail: self.unresolved(unresolved),
            },
            Notice::UpstreamExpired { pin, since } => NoticeOut {
                kind: "upstream_expired",
                link: pin.link().to_string(),
                target: Some(self.pin(pin)),
                detail: format!(
                    "the approval of {} lapsed at {} (unix ms)",
                    pin.key(),
                    since.millis()
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
            LinkState::Resolved { target } => LinkTargetOut::Resolved {
                doc: self.doc(*target),
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
                .map(|(name, states)| LinkOut {
                    name: name.to_string(),
                    targets: states.iter().map(|s| self.target(s)).collect(),
                })
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

/// The answer of `books check`.
pub fn books_check(check: &BooksCheck) -> BooksCheckOut {
    match check {
        BooksCheck::SchemaInvalid(problems) => BooksCheckOut {
            clean: false,
            schema_problems: problems.iter().map(ToString::to_string).collect(),
            books: Vec::new(),
            health: None,
        },
        BooksCheck::Checked { books, health } => {
            let names = Names::default();
            BooksCheckOut {
                clean: check.is_clean(),
                schema_problems: Vec::new(),
                books: books
                    .iter()
                    .map(|b| BookOut {
                        name: b.name.to_string(),
                        dir: b.dir.as_rel().to_string(),
                        valid: b.valid,
                        invalid: b.invalid,
                    })
                    .collect(),
                health: Some(Namer(&names).health(health)),
            }
        }
    }
}

/// The answer of `books status`.
pub fn status(report: &StatusReport) -> StatusOut {
    let namer = Namer(&report.names);
    StatusOut {
        docs: report.rows.iter().map(|r| namer.row(r)).collect(),
        health: namer.health(&report.health),
    }
}

/// The answer of `books get`.
pub fn get(report: &GetReport) -> GetOut {
    let namer = Namer(&report.names);
    GetOut {
        row: namer.row(&report.row),
        content: report.doc.as_ref().map(|d| namer.content(d)),
        roots: report.roots.iter().map(|r| namer.root(r)).collect(),
        health: namer.health(&report.health),
    }
}

/// The answer of `books trace`.
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

/// The answer of `books stale`.
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
        health: namer.health(&report.health),
    }
}

/// The answer of `books judge`.
pub fn judged(judged: &Judged) -> JudgedOut {
    let names = Names::default();
    let namer = Namer(&names);
    JudgedOut {
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
    }
}

/// The answer to arguments that are no request.
pub fn usage(message: String) -> ErrorOut {
    ErrorOut {
        error: "request",
        messages: vec![message],
        candidates: Vec::new(),
    }
}

fn presence(presence: &Presence) -> PresenceOut {
    match presence {
        Presence::Valid { origin, hash, .. } => PresenceOut::Valid {
            file: origin.to_string(),
            hash: hash.to_string(),
        },
        Presence::Invalid { files, problems } => PresenceOut::Invalid {
            files: files.iter().map(ToString::to_string).collect(),
            problems: problems.iter().map(ToString::to_string).collect(),
        },
        Presence::Gone => PresenceOut::Gone,
        Presence::Unknown { unreadable } => PresenceOut::Unknown {
            unreadable: unreadable.iter().map(ToString::to_string).collect(),
        },
    }
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

fn join<T: std::fmt::Display>(items: impl Iterator<Item = T>) -> String {
    items.map(|i| i.to_string()).collect::<Vec<_>>().join("; ")
}
