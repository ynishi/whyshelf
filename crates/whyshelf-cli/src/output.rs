//! What each command says, as types.
//!
//! An [`Outcome`] becomes one [`Output`]: a struct per command, whose fields are
//! the keys of `--format json` and whose types are the shapes (a table is a list of
//! rows, a row is an object with one key per column). The JSON is the serialization
//! of that struct and `whyshelf schema output` is its schema, so neither is written
//! by hand anywhere. The two text formats read the same values: each output lists
//! its [`Section`]s (a table, a list of fields, a list of lines) from its own fields,
//! and the [`Doc`](crate::render::Doc) writes them as aligned columns or Markdown.
//!
//! ```text
//! Outcome --document--> Output (Serialize + JsonSchema) --json-->  --format json
//!                           |
//!                           +--sections--> Section (text) --------> --format table | markdown
//! ```
//!
//! Nothing here decides anything: a name is looked up, a state is spelled. The keys
//! of an object are written out in sorted order, as they always were, and
//! `tests/golden/outputs.txt` pins every key and word of the three formats.
//!
//! A table can be cut to the columns a caller asked for. The keys left out of a
//! row are the only difference from the full row, so the shape is the row type's
//! whichever columns are shown.

use std::borrow::Cow;
use std::collections::{BTreeMap, BTreeSet};

use schemars::{JsonSchema, Schema, SchemaGenerator};
use serde::ser::{Error as _, SerializeMap, SerializeSeq};
use serde::{Serialize, Serializer};
use whyshelf_core::app::check::CheckReport;
use whyshelf_core::app::query::{EntityContent, EntityView, Field, FoundPage, GetResult};
use whyshelf_core::app::view::{
    AuditDetail, AuditReport, Changelog, ContextLine, ContextReport, RowDetail, StatusReport,
    StatusRow, TraceNode, TraceReport,
};
use whyshelf_core::domain::approval::CmResult;
use whyshelf_core::domain::classification::ClassificationIssue;
use whyshelf_core::domain::event::{EventBody, StoreKind};
use whyshelf_core::domain::id::EntityId;
use whyshelf_core::domain::lint::{CheckViolation, UpstreamProblem};
use whyshelf_core::domain::revision::{IntegrationFact, Revision};
use whyshelf_core::domain::state::{Approval, Completeness, EntityState, Integration, Upstream};
use whyshelf_infra::format::{Census, Found, Projection, Source, Verdict};

use crate::command::Outcome;
use crate::error::CliError;

// ---- the shared parts --------------------------------------------------------

/// The key a column has in JSON: its label in lower case, with `_` for a space.
fn key_of(label: &str) -> String {
    label.to_ascii_lowercase().replace(' ', "_")
}

/// A cell of text: `-` for nothing.
fn dash(text: &str) -> String {
    if text.is_empty() {
        "-".to_owned()
    } else {
        text.to_owned()
    }
}

/// A cell of text for a value that may be absent.
fn dash_opt(text: Option<&str>) -> String {
    text.map_or_else(|| "-".to_owned(), dash)
}

/// A cell of text for a list: the items, side by side, or `-`.
fn dash_list(items: &[String]) -> String {
    if items.is_empty() {
        "-".to_owned()
    } else {
        items.join(", ")
    }
}

/// A part of the text of an answer.
#[derive(Debug)]
pub struct Section {
    /// Its heading when the answer has several parts; empty for none.
    pub title: &'static str,
    /// What it holds.
    pub block: Block,
}

/// What a section of text holds.
#[derive(Debug)]
pub enum Block {
    /// Columns and rows.
    Table {
        /// The labels of the columns shown.
        columns: Vec<&'static str>,
        /// The text of the cells, a row each.
        grid: Vec<Vec<String>>,
    },
    /// Names and values.
    Fields(Vec<(&'static str, String)>),
    /// Lines.
    Lines(Vec<String>),
}

impl Section {
    fn fields(fields: Vec<(&'static str, String)>) -> Self {
        Self {
            title: "",
            block: Block::Fields(fields),
        }
    }

    /// A list of lines under a heading; nothing at all when there are none.
    fn lines(title: &'static str, lines: &[String]) -> Option<Self> {
        (!lines.is_empty()).then(|| Self {
            title,
            block: Block::Lines(lines.to_vec()),
        })
    }
}

/// A row of a table: an object with one key per column.
pub trait Row: Serialize + JsonSchema {
    /// The labels of the columns, in the order they are shown. The key of a column
    /// is its label in lower case with `_` for a space.
    const COLUMNS: &'static [&'static str];

    /// The text of the cells, one per column.
    fn cells(&self) -> Vec<String>;
}

/// The rows of one table, and the columns of them that are shown.
#[derive(Debug)]
pub struct Table<R> {
    rows: Vec<R>,
    shown: Vec<usize>,
}

impl<R: Row> Table<R> {
    /// All the columns of these rows.
    fn new(rows: Vec<R>) -> Self {
        Self {
            rows,
            shown: (0..R::COLUMNS.len()).collect(),
        }
    }

    /// Only the columns whose label `keep` accepts, in their order.
    fn only(mut self, keep: impl Fn(&str) -> bool) -> Self {
        self.shown = (0..R::COLUMNS.len())
            .filter(|&i| keep(R::COLUMNS[i]))
            .collect();
        self
    }

    fn section(&self, title: &'static str) -> Section {
        Section {
            title,
            block: Block::Table {
                columns: self.shown.iter().map(|&i| R::COLUMNS[i]).collect(),
                grid: self
                    .rows
                    .iter()
                    .map(|row| {
                        let cells = row.cells();
                        self.shown.iter().map(|&i| cells[i].clone()).collect()
                    })
                    .collect(),
            },
        }
    }
}

/// A table whose rows can be cut: the part of an [`Output`] a
/// [`Window`](crate::render::Window) acts on.
pub trait Windowed {
    /// Keep only the columns with these names (any case), in this order.
    fn select(&mut self, names: &[String]) -> Result<(), CliError>;

    /// Keep the rows from `offset` on, at most `limit` of them; the offset of the
    /// next window when there are more.
    fn cut(&mut self, offset: usize, limit: Option<usize>) -> Option<usize>;
}

impl<R: Row> Windowed for Table<R> {
    fn select(&mut self, names: &[String]) -> Result<(), CliError> {
        let shown = names
            .iter()
            .map(|name| {
                self.shown
                    .iter()
                    .copied()
                    .find(|&i| R::COLUMNS[i].eq_ignore_ascii_case(name))
                    .ok_or_else(|| CliError::UnknownColumn {
                        name: name.clone(),
                        columns: self
                            .shown
                            .iter()
                            .map(|&i| R::COLUMNS[i])
                            .collect::<Vec<_>>()
                            .join(", ")
                            .to_ascii_lowercase(),
                    })
            })
            .collect::<Result<Vec<_>, _>>()?;
        self.shown = shown;
        Ok(())
    }

    fn cut(&mut self, offset: usize, limit: Option<usize>) -> Option<usize> {
        let total = self.rows.len();
        let start = offset.min(total);
        let end = limit.map_or(total, |l| start.saturating_add(l).min(total));
        self.rows = self.rows.drain(start..end).collect();
        (end < total).then_some(end)
    }
}

/// A row as it is written: all its keys, or those of the columns shown.
struct Shown<'a, R> {
    row: &'a R,
    shown: &'a [usize],
}

impl<R: Row> Serialize for Shown<'_, R> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        if self.shown.len() == R::COLUMNS.len() {
            return self.row.serialize(serializer);
        }
        // The row's type says what its keys are; the columns left out are dropped.
        let serde_json::Value::Object(mut keys) =
            serde_json::to_value(self.row).map_err(S::Error::custom)?
        else {
            return Err(S::Error::custom("a row is an object"));
        };
        let mut map = serializer.serialize_map(Some(self.shown.len()))?;
        for &i in self.shown {
            let key = key_of(R::COLUMNS[i]);
            let value = keys
                .remove(&key)
                .ok_or_else(|| S::Error::custom(format!("a row has no key `{key}`")))?;
            map.serialize_entry(&key, &value)?;
        }
        map.end()
    }
}

impl<R: Row> Serialize for Table<R> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut seq = serializer.serialize_seq(Some(self.rows.len()))?;
        for row in &self.rows {
            seq.serialize_element(&Shown {
                row,
                shown: &self.shown,
            })?;
        }
        seq.end()
    }
}

/// The schema of a table is that of its rows in a list: with some columns left
/// out, a row has fewer of the keys.
impl<R: JsonSchema> JsonSchema for Table<R> {
    fn inline_schema() -> bool {
        <Vec<R>>::inline_schema()
    }

    fn schema_name() -> Cow<'static, str> {
        <Vec<R>>::schema_name()
    }

    fn json_schema(generator: &mut SchemaGenerator) -> Schema {
        <Vec<R>>::json_schema(generator)
    }
}

/// What an output can say in text, and which of its tables can be cut.
pub trait Shape {
    /// The parts of the text, in order.
    fn sections(&self) -> Vec<Section>;

    /// The table a window acts on, if the output has one.
    fn table(&mut self) -> Option<&mut dyn Windowed> {
        None
    }

    /// The line that says where the tool is pointed, if the output has it; it is
    /// the first thing the text says.
    fn context(&self) -> Option<&ContextLineOut> {
        None
    }
}

// ---- the whole answer --------------------------------------------------------

/// The line that says where the tool is pointed, as a JSON object.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct ContextLineOut {
    /// The project's name.
    project: String,
    /// The working directory.
    worktree: String,
    /// The current revision.
    revision: String,
    /// A name for the line of work (a branch, say).
    branch: Option<String>,
    /// The line as text.
    #[serde(skip)]
    #[schemars(skip)]
    line: String,
}

impl ContextLineOut {
    /// The line, as it is written in JSON and as text.
    pub fn of(line: &ContextLine) -> Self {
        Self {
            project: line.project().to_string(),
            worktree: line.worktree().to_string(),
            revision: line.revision().to_string(),
            branch: line.label().map(str::to_owned),
            line: line.to_string(),
        }
    }

    /// The line as text.
    pub fn text(&self) -> &str {
        &self.line
    }
}

/// An answer: what the command says, with the cursor of the next window after it.
#[derive(Debug, Serialize, JsonSchema)]
pub struct Document {
    /// What the command says.
    #[serde(flatten)]
    pub output: Output,
    /// Where to continue, when there is more.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_cursor: Option<String>,
}

/// Declare [`Output`]: one variant for each answer, and the dispatch to its shape.
macro_rules! outputs {
    ($($(#[$doc:meta])* $variant:ident($ty:ty)),+ $(,)?) => {
        /// What a command says: one shape for each kind of answer.
        #[derive(Debug, Serialize, JsonSchema)]
        #[serde(untagged)]
        pub enum Output {
            $($(#[$doc])* $variant($ty)),+
        }

        impl Shape for Output {
            fn sections(&self) -> Vec<Section> {
                match self { $(Self::$variant(output) => output.sections()),+ }
            }

            fn context(&self) -> Option<&ContextLineOut> {
                match self { $(Self::$variant(output) => output.context()),+ }
            }

            fn table(&mut self) -> Option<&mut dyn Windowed> {
                match self { $(Self::$variant(output) => output.table()),+ }
            }
        }
    };
}

outputs! {
    /// `context`.
    Context(ContextOut),
    /// `add`.
    Added(AddedOut),
    /// `tag`, `mv`, `publish` and `revoke`.
    Recorded(RecordedOut),
    /// `finding new`, `spec new` and `design new`.
    Created(CreatedOut),
    /// `check` without `--approve`.
    Checked(CheckedOut),
    /// `check` with `--approve`.
    Approved(ApprovedOut),
    /// `get` of an entity (boxed: it is far larger than the other answers).
    Entity(Box<EntityOut>),
    /// `get` of a reference this environment cannot look up.
    Unresolved(UnresolvedOut),
    /// `get` of an address outside the project.
    External(ExternalOut),
    /// `find`.
    Found(FoundOut),
    /// `status`.
    Status(StatusOut),
    /// `trace`.
    Trace(TraceOut),
    /// `changelog`.
    Changelog(ChangelogOut),
    /// `audit`.
    Audit(AuditOut),
    /// `rebuild`.
    Rebuilt(RebuiltOut),
    /// `migrate`.
    Migrate(MigrateOut),
    /// Lines that are printed as they are.
    Lines(LinesOut),
}

impl Output {
    /// Put the context line in the output. Only a status says it (it is the first
    /// thing a status says); any other output is left as it is.
    pub fn with_context(&mut self, line: &ContextLine) {
        if let Self::Status(status) = self {
            status.context = Some(ContextLineOut::of(line));
        }
    }
}

/// The output an outcome says.
pub fn output(outcome: &Outcome, fields: &BTreeSet<Field>) -> Output {
    match outcome {
        Outcome::Context(report) => Output::Context(ContextOut::of(report)),
        Outcome::Added(added) => Output::Added(AddedOut {
            registered: Table::new(
                added
                    .registered()
                    .iter()
                    .map(|(id, path)| RegisteredRow {
                        id: id.to_string(),
                        path: path.to_string(),
                    })
                    .collect(),
            ),
            skipped: added.skipped().iter().map(ToString::to_string).collect(),
        }),
        Outcome::Recorded(committed) => Output::Recorded(RecordedOut {
            recorded: RecordedFields {
                events: committed.events().iter().map(ToString::to_string).collect(),
            },
        }),
        Outcome::Scaffolded(made) => Output::Created(CreatedOut {
            created: CreatedFields {
                id: made.id().to_string(),
                alias: made.alias().to_string(),
                path: made.path().to_string(),
            },
        }),
        Outcome::Checked(report) => Output::Checked(CheckedOut::of(report)),
        Outcome::Approved {
            gate,
            event,
            attestation,
        } => {
            // What was recorded, not where the working tree is now: only a CA
            // records a revision, and it may be an older one than the current.
            let recorded = match attestation.revision() {
                Some(revision) => format!("{} at {}", gate.name(), revision_text(revision)),
                None => gate.name().to_owned(),
            };
            Output::Approved(ApprovedOut {
                approved: ApprovedFields {
                    recorded,
                    event: event.to_string(),
                },
            })
        }
        Outcome::Got(result) => match result {
            GetResult::Entity(view) => Output::Entity(Box::new(EntityOut {
                entity: EntityFields::of(view),
            })),
            GetResult::Unresolved(reference) => Output::Unresolved(UnresolvedOut {
                unresolved: UnresolvedFields {
                    reference: reference.to_string(),
                    note: "this environment cannot look it up".to_owned(),
                },
            }),
            GetResult::External(uri) => Output::External(ExternalOut {
                external: ExternalFields {
                    uri: uri.to_string(),
                },
            }),
        },
        Outcome::Found(page) => Output::Found(FoundOut::of(page, fields)),
        Outcome::Status(report) => Output::Status(StatusOut::of(report)),
        Outcome::Trace(report) => Output::Trace(TraceOut::of(report)),
        Outcome::Changelog(log) => Output::Changelog(ChangelogOut::of(log)),
        Outcome::Audit(report) => Output::Audit(AuditOut::of(report)),
        Outcome::Rebuilt { revised } => Output::Rebuilt(RebuiltOut {
            rebuilt: RebuiltFields { revised: *revised },
        }),
        Outcome::Migrate(found) => Output::Migrate(MigrateOut::of(found)),
        Outcome::Printed(lines) => Output::Lines(LinesOut {
            lines: lines.clone(),
        }),
    }
}

// ---- context -----------------------------------------------------------------

/// The answer of `context`.
#[derive(Debug, Serialize, JsonSchema)]
pub struct ContextOut {
    /// Where the tool is pointed, and the stores.
    context: ContextFields,
}

/// Where the tool is pointed, and the stores.
#[derive(Debug, Serialize, JsonSchema)]
struct ContextFields {
    /// The project's name.
    project: String,
    /// The working directory.
    worktree: String,
    /// The current revision.
    revision: String,
    /// A name for the line of work (a branch, say).
    branch: Option<String>,
    /// Where a new document goes.
    default_store: String,
    /// How many events the repo store holds.
    repo_events: usize,
    /// How many events the private store holds.
    private_events: usize,
}

impl ContextOut {
    fn of(report: &ContextReport) -> Self {
        let line = report.line();
        Self {
            context: ContextFields {
                project: line.project().to_string(),
                worktree: line.worktree().to_string(),
                revision: line.revision().to_string(),
                branch: line.label().map(str::to_owned),
                default_store: report.default_store().as_str().to_owned(),
                repo_events: report.events_in(StoreKind::Repo),
                private_events: report.events_in(StoreKind::Private),
            },
        }
    }
}

impl Shape for ContextOut {
    fn sections(&self) -> Vec<Section> {
        let c = &self.context;
        vec![Section::fields(vec![
            ("project", c.project.clone()),
            ("worktree", c.worktree.clone()),
            ("revision", c.revision.clone()),
            ("branch", dash_opt(c.branch.as_deref())),
            ("default_store", c.default_store.clone()),
            ("repo_events", c.repo_events.to_string()),
            ("private_events", c.private_events.to_string()),
        ])]
    }
}

// ---- add ---------------------------------------------------------------------

/// The answer of `add`.
#[derive(Debug, Serialize, JsonSchema)]
pub struct AddedOut {
    /// The files registered.
    registered: Table<RegisteredRow>,
    /// The files that were registered already; absent when there are none.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    skipped: Vec<String>,
}

/// A file registered.
#[derive(Debug, Serialize, JsonSchema)]
struct RegisteredRow {
    /// The new identifier.
    id: String,
    /// The path of the file.
    path: String,
}

impl Row for RegisteredRow {
    const COLUMNS: &'static [&'static str] = &["ID", "PATH"];

    fn cells(&self) -> Vec<String> {
        vec![dash(&self.id), dash(&self.path)]
    }
}

impl Shape for AddedOut {
    fn sections(&self) -> Vec<Section> {
        let mut sections = vec![self.registered.section("Registered")];
        sections.extend(Section::lines("Already registered", &self.skipped));
        sections
    }

    fn table(&mut self) -> Option<&mut dyn Windowed> {
        Some(&mut self.registered)
    }
}

// ---- tag, mv, publish, revoke ------------------------------------------------

/// The answer of a command that records a change.
#[derive(Debug, Serialize, JsonSchema)]
pub struct RecordedOut {
    /// The events written.
    recorded: RecordedFields,
}

/// The events written.
#[derive(Debug, Serialize, JsonSchema)]
struct RecordedFields {
    /// Their identifiers.
    events: Vec<String>,
}

impl Shape for RecordedOut {
    fn sections(&self) -> Vec<Section> {
        vec![Section::fields(vec![(
            "events",
            dash_list(&self.recorded.events),
        )])]
    }
}

// ---- finding new, spec new, design new ---------------------------------------

/// The answer of a command that starts a document.
#[derive(Debug, Serialize, JsonSchema)]
pub struct CreatedOut {
    /// The document made.
    created: CreatedFields,
}

/// The document made.
#[derive(Debug, Serialize, JsonSchema)]
struct CreatedFields {
    /// Its identifier.
    id: String,
    /// Its alias.
    alias: String,
    /// Where its file is.
    path: String,
}

impl Shape for CreatedOut {
    fn sections(&self) -> Vec<Section> {
        let c = &self.created;
        vec![Section::fields(vec![
            ("id", c.id.clone()),
            ("alias", c.alias.clone()),
            ("path", c.path.clone()),
        ])]
    }
}

// ---- check -------------------------------------------------------------------

/// The answer of a gate that only checks.
#[derive(Debug, Serialize, JsonSchema)]
pub struct CheckedOut {
    /// Which gate, on what, and whether it passed.
    check: CheckFields,
    /// What the check found wrong, in words; absent when it passed.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    violations: Vec<String>,
}

/// Which gate, on what, and whether it passed.
#[derive(Debug, Serialize, JsonSchema)]
struct CheckFields {
    /// The gate.
    gate: String,
    /// The entity checked.
    target: String,
    /// `passed` or `failed`.
    result: String,
}

impl CheckedOut {
    fn of(report: &CheckReport) -> Self {
        Self {
            check: CheckFields {
                gate: report.gate().name().to_owned(),
                target: report.target().to_string(),
                result: if report.passed() { "passed" } else { "failed" }.to_owned(),
            },
            violations: report.violations().iter().map(violation_text).collect(),
        }
    }
}

impl Shape for CheckedOut {
    fn sections(&self) -> Vec<Section> {
        let c = &self.check;
        let mut sections = vec![Section::fields(vec![
            ("gate", c.gate.clone()),
            ("target", c.target.clone()),
            ("result", c.result.clone()),
        ])];
        sections.extend(Section::lines("Violations", &self.violations));
        sections
    }
}

/// The answer of a gate that recorded an approval or a measurement.
#[derive(Debug, Serialize, JsonSchema)]
pub struct ApprovedOut {
    /// What was recorded.
    approved: ApprovedFields,
}

/// What was recorded.
#[derive(Debug, Serialize, JsonSchema)]
struct ApprovedFields {
    /// The gate, and for a CA the revision it was made at.
    recorded: String,
    /// The identifier of the event.
    event: String,
}

impl Shape for ApprovedOut {
    fn sections(&self) -> Vec<Section> {
        vec![Section::fields(vec![
            ("recorded", self.approved.recorded.clone()),
            ("event", self.approved.event.clone()),
        ])]
    }
}

// ---- get ---------------------------------------------------------------------

/// The answer of `get` for an entity.
#[derive(Debug, Serialize, JsonSchema)]
pub struct EntityOut {
    /// The entity.
    entity: EntityFields,
}

/// An entity: its classification, its state when its file can be read, what its
/// kind adds, and its approvals.
#[derive(Debug, Serialize, JsonSchema)]
struct EntityFields {
    /// The identifier.
    id: String,
    /// The kind.
    kind: String,
    /// The store.
    store: String,
    /// Where its file is; none for an item.
    path: Option<String>,
    /// Its alias.
    alias: Option<String>,
    /// Its owner.
    owner: Option<String>,
    /// Its tags.
    tags: Vec<String>,
    /// The two axes; absent when the file cannot be read.
    #[serde(flatten)]
    state: Option<StateFields>,
    /// What the kind adds.
    #[serde(flatten)]
    content: ContentFields,
    /// Who approved it, when, and the event.
    approvals: Vec<String>,
}

/// The state of an entity.
#[derive(Debug, Serialize, JsonSchema)]
struct StateFields {
    /// The approval axis.
    approval: String,
    /// The upstream axis.
    upstream: String,
    /// `Complete` or `Unresolved`.
    completeness: String,
    /// Why the state is not plain, each reason on its own.
    reasons: Vec<String>,
}

/// What each kind of entity adds to the fields of every entity.
#[derive(Debug, Serialize, JsonSchema)]
#[serde(untagged)]
enum ContentFields {
    /// A Finding.
    Finding {
        /// Its type.
        finding_type: String,
        /// Its title.
        title: String,
        /// Its evidence label.
        evidence: String,
        /// Its text.
        body: String,
    },
    /// A Spec.
    Spec {
        /// The contract.
        contract: String,
        /// The Findings it stands on.
        grounds: Vec<String>,
        /// The pins it declares.
        pins: Vec<String>,
        /// The hypotheses it declares.
        hyps: Vec<String>,
    },
    /// A Design.
    Design {
        /// The Spec it is for.
        spec: String,
        /// The question of its crux.
        question: String,
        /// The option chosen.
        chosen: String,
        /// The options rejected.
        rejected: Vec<String>,
        /// Its items.
        items: Vec<String>,
    },
    /// A file that cannot be read.
    Unparsed {
        /// What is wrong with it.
        parse_errors: Vec<String>,
    },
    /// A registered file that is not there.
    Missing {
        /// `missing`.
        file: String,
    },
    /// An item of a Design.
    Item {
        /// The Design.
        design: String,
        /// The number inside it.
        number: String,
        /// What it is to do.
        summary: String,
        /// Whether its approved revision reached the baseline.
        integration: String,
        /// The pins it declares.
        pins_declared: Vec<String>,
        /// The pins the tests carry.
        pins_found: Vec<String>,
    },
}

impl EntityFields {
    fn of(view: &EntityView) -> Self {
        let state = view.state().map(|state| StateFields {
            approval: approval_text(state.approval()).to_owned(),
            upstream: upstream_text(state.upstream()).to_owned(),
            completeness: completeness_text(state.completeness()).to_owned(),
            reasons: reasons(state),
        });
        Self {
            id: view.id().to_string(),
            kind: view.kind().name().to_owned(),
            store: view.store().as_str().to_owned(),
            path: view.path().map(ToString::to_string),
            alias: view.alias().map(ToString::to_string),
            owner: view.owner().map(ToString::to_string),
            tags: view.tags().iter().map(ToString::to_string).collect(),
            state,
            content: ContentFields::of(view.content()),
            approvals: view
                .approvals()
                .iter()
                .map(|a| {
                    format!(
                        "{} by {} at {} ({})",
                        a.record().attestation().gate().name(),
                        a.record().by(),
                        a.at(),
                        a.record().event()
                    )
                })
                .collect(),
        }
    }
}

fn strings(items: &mut dyn Iterator<Item = String>) -> Vec<String> {
    items.collect()
}

impl ContentFields {
    fn of(content: &EntityContent) -> Self {
        match content {
            EntityContent::Finding(f) => Self::Finding {
                finding_type: f.source().type_name().to_owned(),
                title: f.title().as_str().to_owned(),
                evidence: format!("{:?}", f.evidence()).to_lowercase(),
                body: f.body().to_owned(),
            },
            EntityContent::Spec(s) => Self::Spec {
                contract: s.contract().as_str().to_owned(),
                grounds: strings(&mut s.grounds().iter().map(ToString::to_string)),
                pins: strings(
                    &mut s
                        .pins()
                        .iter()
                        .map(|(id, c)| format!("{id}: {}", c.as_str())),
                ),
                hyps: strings(
                    &mut s
                        .hyps()
                        .iter()
                        .map(|(id, h)| format!("{id}: {} -> {}", h.metric(), h.target())),
                ),
            },
            EntityContent::Design(d) => Self::Design {
                spec: d.spec().to_string(),
                question: d.crux().question().as_str().to_owned(),
                chosen: format!(
                    "{}: {}",
                    d.crux().chosen().id(),
                    d.crux().chosen().summary()
                ),
                rejected: strings(
                    &mut d
                        .crux()
                        .rejected()
                        .iter()
                        .map(|o| format!("{}: {}", o.id(), o.summary())),
                ),
                items: strings(&mut d.items().iter().map(|(n, i)| {
                    format!(
                        "{n}: {} [{}]",
                        i.summary(),
                        i.assigned_pins()
                            .iter()
                            .map(ToString::to_string)
                            .collect::<Vec<_>>()
                            .join(", ")
                    )
                })),
            },
            EntityContent::Unparsed(u) => Self::Unparsed {
                parse_errors: strings(&mut u.reasons().iter().map(ToString::to_string)),
            },
            EntityContent::FileMissing => Self::Missing {
                file: "missing".to_owned(),
            },
            EntityContent::Item {
                design,
                number,
                item,
                integration,
                progress,
                ..
            } => Self::Item {
                design: design.to_string(),
                number: number.to_string(),
                summary: item.summary().as_str().to_owned(),
                integration: integration_text(integration).to_owned(),
                pins_declared: strings(&mut progress.declared().iter().map(ToString::to_string)),
                pins_found: strings(&mut progress.found().iter().map(ToString::to_string)),
            },
        }
    }

    /// The fields of the text, in the order they are shown.
    fn fields(&self) -> Vec<(&'static str, String)> {
        match self {
            Self::Finding {
                finding_type,
                title,
                evidence,
                body,
            } => vec![
                ("finding_type", dash(finding_type)),
                ("title", dash(title)),
                ("evidence", dash(evidence)),
                ("body", dash(body)),
            ],
            Self::Spec {
                contract,
                grounds,
                pins,
                hyps,
            } => vec![
                ("contract", dash(contract)),
                ("grounds", dash_list(grounds)),
                ("pins", dash_list(pins)),
                ("hyps", dash_list(hyps)),
            ],
            Self::Design {
                spec,
                question,
                chosen,
                rejected,
                items,
            } => vec![
                ("spec", dash(spec)),
                ("question", dash(question)),
                ("chosen", dash(chosen)),
                ("rejected", dash_list(rejected)),
                ("items", dash_list(items)),
            ],
            Self::Unparsed { parse_errors } => vec![("parse_errors", dash_list(parse_errors))],
            Self::Missing { file } => vec![("file", dash(file))],
            Self::Item {
                design,
                number,
                summary,
                integration,
                pins_declared,
                pins_found,
            } => vec![
                ("design", dash(design)),
                ("number", dash(number)),
                ("summary", dash(summary)),
                ("integration", dash(integration)),
                ("pins_declared", dash_list(pins_declared)),
                ("pins_found", dash_list(pins_found)),
            ],
        }
    }
}

impl Shape for EntityOut {
    fn sections(&self) -> Vec<Section> {
        let e = &self.entity;
        let mut fields = vec![
            ("id", dash(&e.id)),
            ("kind", dash(&e.kind)),
            ("store", dash(&e.store)),
            ("path", dash_opt(e.path.as_deref())),
            ("alias", dash_opt(e.alias.as_deref())),
            ("owner", dash_opt(e.owner.as_deref())),
            ("tags", dash_list(&e.tags)),
        ];
        if let Some(state) = &e.state {
            fields.push(("approval", dash(&state.approval)));
            fields.push(("upstream", dash(&state.upstream)));
            fields.push(("completeness", dash(&state.completeness)));
            fields.push(("reasons", dash_list(&state.reasons)));
        }
        fields.extend(e.content.fields());
        fields.push(("approvals", dash_list(&e.approvals)));
        vec![Section::fields(fields)]
    }
}

/// The answer of `get` for a reference this environment cannot look up.
#[derive(Debug, Serialize, JsonSchema)]
pub struct UnresolvedOut {
    /// The reference.
    unresolved: UnresolvedFields,
}

/// A reference that cannot be looked up here.
#[derive(Debug, Serialize, JsonSchema)]
struct UnresolvedFields {
    /// The reference.
    reference: String,
    /// Why nothing is shown.
    note: String,
}

impl Shape for UnresolvedOut {
    fn sections(&self) -> Vec<Section> {
        vec![Section::fields(vec![
            ("reference", self.unresolved.reference.clone()),
            ("note", self.unresolved.note.clone()),
        ])]
    }
}

/// The answer of `get` for an address outside the project.
#[derive(Debug, Serialize, JsonSchema)]
pub struct ExternalOut {
    /// The address.
    external: ExternalFields,
}

/// An address outside the project.
#[derive(Debug, Serialize, JsonSchema)]
struct ExternalFields {
    /// The address.
    uri: String,
}

impl Shape for ExternalOut {
    fn sections(&self) -> Vec<Section> {
        vec![Section::fields(vec![("uri", self.external.uri.clone())])]
    }
}

// ---- find --------------------------------------------------------------------

/// The answer of `find`.
#[derive(Debug, Serialize, JsonSchema)]
pub struct FoundOut {
    /// One row per entity of this page, with the columns asked for.
    found: Table<FoundRow>,
    /// How many entities matched in all.
    page: PageFields,
}

/// How many entities matched.
#[derive(Debug, Serialize, JsonSchema)]
struct PageFields {
    /// The number of entities that matched.
    total: usize,
}

/// An entity found. A column that was not asked for is left out of the row; one
/// that was asked for and has no value is null.
#[derive(Debug, Serialize, JsonSchema)]
struct FoundRow {
    /// The identifier.
    id: String,
    /// The kind.
    kind: String,
    /// The store.
    store: Option<String>,
    /// Where its file is.
    path: Option<String>,
    /// Its alias.
    alias: Option<String>,
    /// Its title or summary.
    title: Option<String>,
    /// Its tags.
    tags: Option<Vec<String>>,
    /// Its owner.
    owner: Option<String>,
    /// The approval axis.
    approval: Option<String>,
    /// The upstream axis.
    upstream: Option<String>,
}

impl Row for FoundRow {
    const COLUMNS: &'static [&'static str] = &[
        "ID", "KIND", "STORE", "PATH", "ALIAS", "TITLE", "TAGS", "OWNER", "APPROVAL", "UPSTREAM",
    ];

    fn cells(&self) -> Vec<String> {
        vec![
            dash(&self.id),
            dash(&self.kind),
            dash_opt(self.store.as_deref()),
            dash_opt(self.path.as_deref()),
            dash_opt(self.alias.as_deref()),
            dash_opt(self.title.as_deref()),
            dash_list(self.tags.as_deref().unwrap_or_default()),
            dash_opt(self.owner.as_deref()),
            dash_opt(self.approval.as_deref()),
            dash_opt(self.upstream.as_deref()),
        ]
    }
}

impl FoundOut {
    fn of(page: &FoundPage, fields: &BTreeSet<Field>) -> Self {
        let wants = |f: Field| fields.is_empty() || fields.contains(&f);
        // The identifier and the kind are always there; the rest is asked for.
        let shown = |label: &str| match label {
            "STORE" => wants(Field::Store),
            "PATH" => wants(Field::Path),
            "ALIAS" => wants(Field::Alias),
            "TITLE" => wants(Field::Title),
            "TAGS" => wants(Field::Tags),
            "OWNER" => wants(Field::Owner),
            "APPROVAL" | "UPSTREAM" => wants(Field::State),
            _ => true,
        };
        let rows = page
            .rows()
            .iter()
            .map(|row| FoundRow {
                id: row.id.to_string(),
                kind: row.kind.name().to_owned(),
                store: row.store.as_ref().map(|s| s.as_str().to_owned()),
                path: row.path.as_ref().map(ToString::to_string),
                alias: row.alias.as_ref().map(ToString::to_string),
                title: row.title.clone(),
                tags: row
                    .tags
                    .as_ref()
                    .map(|t| t.iter().map(ToString::to_string).collect()),
                owner: row.owner.as_ref().map(ToString::to_string),
                approval: row
                    .state
                    .as_ref()
                    .map(|s| approval_text(s.approval()).to_owned()),
                upstream: row
                    .state
                    .as_ref()
                    .map(|s| upstream_text(s.upstream()).to_owned()),
            })
            .collect();
        Self {
            found: Table::new(rows).only(shown),
            page: PageFields {
                total: page.total(),
            },
        }
    }
}

impl Shape for FoundOut {
    fn sections(&self) -> Vec<Section> {
        vec![
            self.found.section("Found"),
            Section::fields(vec![("total", self.page.total.to_string())]),
        ]
    }

    fn table(&mut self) -> Option<&mut dyn Windowed> {
        Some(&mut self.found)
    }
}

// ---- status ------------------------------------------------------------------

/// The answer of `status`.
#[derive(Debug, Serialize, JsonSchema)]
pub struct StatusOut {
    /// Where the tool is pointed; absent when the caller already knows (the tool
    /// server opens every answer with it).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    context: Option<ContextLineOut>,
    /// One row per entity, and per file nobody registered.
    entities: Table<StatusRowOut>,
    /// What else the person has to settle; absent when nothing.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    notes: Vec<String>,
}

/// An entity in the status.
#[derive(Debug, Serialize, JsonSchema)]
struct StatusRowOut {
    /// Its alias, else its path, else the name of its Design and its number.
    id: String,
    /// The kind; `-` for a file nobody registered.
    kind: String,
    /// The title of a Finding or the summary of an item.
    title: Option<String>,
    /// The approval axis; `Missing` when the file is gone, `Unregistered` for a
    /// file nobody registered.
    approval: String,
    /// The upstream axis.
    upstream: String,
    /// Whether an approved item reached the baseline; `-` for any other row.
    integration: String,
    /// Pins found of those declared, as `found/declared`; `-` when there are none.
    pins: String,
    /// The command to run next.
    next: String,
}

impl Row for StatusRowOut {
    const COLUMNS: &'static [&'static str] = &[
        "ID",
        "KIND",
        "TITLE",
        "APPROVAL",
        "UPSTREAM",
        "INTEGRATION",
        "PINS",
        "NEXT",
    ];

    fn cells(&self) -> Vec<String> {
        vec![
            dash(&self.id),
            dash(&self.kind),
            dash_opt(self.title.as_deref()),
            dash(&self.approval),
            dash(&self.upstream),
            dash(&self.integration),
            dash(&self.pins),
            dash(&self.next),
        ]
    }
}

/// What to call an entity: its alias, else its path, else (an item) its
/// Design's name and number, else its identifier.
fn row_name(row: &StatusRow, names: &BTreeMap<EntityId, String>) -> String {
    if let Some(alias) = row.alias() {
        alias.to_string()
    } else if let Some(path) = row.path() {
        path.to_string()
    } else {
        match row.detail() {
            RowDetail::Item(item) => {
                let parent = item.parent();
                let design = names.get(&parent).cloned().unwrap_or(parent.to_string());
                // An item number is written with its own `#`.
                format!("{design}{}", item.number())
            }
            _ => row.id().to_string(),
        }
    }
}

impl StatusOut {
    fn of(report: &StatusReport) -> Self {
        let mut rows = Vec::new();
        let mut notes = Vec::new();
        let names: BTreeMap<EntityId, String> = report
            .rows()
            .iter()
            .filter(|row| !matches!(row.detail(), RowDetail::Item(_)))
            .map(|row| (row.id(), row_name(row, &BTreeMap::new())))
            .collect();
        for row in report.rows() {
            let (approval, upstream) = row.state().map_or(("Missing", "-"), |s| {
                (approval_text(s.approval()), upstream_text(s.upstream()))
            });
            let name = row_name(row, &names);
            if let Some(state) = row.state() {
                notes.extend(reasons(state).into_iter().map(|r| format!("{name}: {r}")));
            }
            let integration = match row.detail() {
                RowDetail::Item(item) => integration_text(item.integration()),
                _ => "-",
            };
            rows.push(StatusRowOut {
                id: name,
                kind: row.kind().name(),
                title: row.label().map(str::to_owned),
                approval: approval.to_owned(),
                upstream: upstream.to_owned(),
                integration: integration.to_owned(),
                pins: row.pins().map_or_else(
                    || "-".to_owned(),
                    |t| format!("{}/{}", t.found(), t.declared()),
                ),
                next: row.next().to_string(),
            });
        }
        for path in report.unregistered() {
            rows.push(StatusRowOut {
                id: path.to_string(),
                kind: "-".to_owned(),
                title: None,
                approval: "Unregistered".to_owned(),
                upstream: "-".to_owned(),
                integration: "-".to_owned(),
                pins: "-".to_owned(),
                next: StatusReport::next_for_unregistered(path).to_string(),
            });
        }
        notes.extend(
            report
                .missing()
                .iter()
                .map(|p| format!("file missing: {p}")),
        );
        notes.extend(
            report
                .changed_since_ingest()
                .iter()
                .map(|p| format!("changed since the last ingest: {p}")),
        );
        notes.extend(
            report
                .unreadable()
                .iter()
                .map(|p| format!("path not listed: {p}")),
        );
        notes.extend(
            report
                .issues()
                .iter()
                .map(|i| format!("{} store: {}", i.store.as_str(), issue_text(&i.issue))),
        );
        notes.extend(report.exports().iter().map(|e| {
            format!(
                "{} store: {:?} is {:?}; `whyshelf rebuild` writes it again",
                e.scope.as_str(),
                e.kind,
                e.problem
            )
        }));
        Self {
            context: None,
            entities: Table::new(rows),
            notes,
        }
    }
}

impl Shape for StatusOut {
    fn sections(&self) -> Vec<Section> {
        let mut sections = vec![self.entities.section("Entities")];
        sections.extend(Section::lines("Notes", &self.notes));
        sections
    }

    fn context(&self) -> Option<&ContextLineOut> {
        self.context.as_ref()
    }

    fn table(&mut self) -> Option<&mut dyn Windowed> {
        Some(&mut self.entities)
    }
}

// ---- trace -------------------------------------------------------------------

/// The answer of `trace`.
#[derive(Debug, Serialize, JsonSchema)]
pub struct TraceOut {
    /// The chain, one row per entity.
    nodes: Table<TraceRowOut>,
    /// Which entity stands on which, as `from -> to`; absent when none.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    links: Vec<String>,
}

/// An entity in a trace.
#[derive(Debug, Serialize, JsonSchema)]
struct TraceRowOut {
    /// Its alias, else its path, else its number and identifier.
    id: String,
    /// The kind.
    kind: String,
    /// The approval axis.
    approval: String,
    /// The upstream axis.
    upstream: String,
    /// The revisions it was approved at, with who approved and whether each
    /// reached the baseline.
    approved_at: Vec<String>,
}

impl Row for TraceRowOut {
    const COLUMNS: &'static [&'static str] = &["ID", "KIND", "APPROVAL", "UPSTREAM", "APPROVED AT"];

    fn cells(&self) -> Vec<String> {
        vec![
            dash(&self.id),
            dash(&self.kind),
            dash(&self.approval),
            dash(&self.upstream),
            dash_list(&self.approved_at),
        ]
    }
}

fn node_name(node: &TraceNode) -> String {
    if let Some(alias) = node.alias() {
        alias.to_string()
    } else if let Some(path) = node.path() {
        path.to_string()
    } else if let Some(number) = node.number() {
        format!("{number} ({})", node.id())
    } else {
        node.id().to_string()
    }
}

impl TraceOut {
    fn of(report: &TraceReport) -> Self {
        let name_of = |id| {
            report
                .nodes()
                .iter()
                .find(|n| n.id() == id)
                .map_or_else(|| id.to_string(), node_name)
        };
        let rows = report
            .nodes()
            .iter()
            .map(|node| {
                let (approval, upstream) = node.state().map_or(("Missing", "-"), |s| {
                    (approval_text(s.approval()), upstream_text(s.upstream()))
                });
                TraceRowOut {
                    id: node_name(node),
                    kind: node.kind().name().to_owned(),
                    approval: approval.to_owned(),
                    upstream: upstream.to_owned(),
                    approved_at: node
                        .approved()
                        .iter()
                        .map(|a| {
                            format!(
                                "{} by {}{}",
                                revision_text(&a.revision),
                                a.by,
                                integrated_text(a.integrated)
                            )
                        })
                        .collect(),
                }
            })
            .collect();
        let links = report
            .links()
            .iter()
            .map(|(from, to)| format!("{} -> {}", name_of(*from), name_of(*to)))
            .collect();
        Self {
            nodes: Table::new(rows),
            links,
        }
    }
}

impl Shape for TraceOut {
    fn sections(&self) -> Vec<Section> {
        let mut sections = vec![self.nodes.section("Chain")];
        sections.extend(Section::lines("Links", &self.links));
        sections
    }

    fn table(&mut self) -> Option<&mut dyn Windowed> {
        Some(&mut self.nodes)
    }
}

// ---- changelog ---------------------------------------------------------------

/// The answer of `changelog`.
#[derive(Debug, Serialize, JsonSchema)]
pub struct ChangelogOut {
    /// The events of the entity, in the order of the event graph.
    events: Table<EventRowOut>,
}

/// An event in a history.
#[derive(Debug, Serialize, JsonSchema)]
struct EventRowOut {
    /// The time shown for it.
    at: String,
    /// The event's identifier.
    event: String,
    /// What kind of event it is.
    kind: String,
    /// Who and what, for an approval, a revocation and a move; else empty.
    detail: String,
    /// The store it is in.
    store: String,
}

impl Row for EventRowOut {
    const COLUMNS: &'static [&'static str] = &["AT", "EVENT", "KIND", "DETAIL", "STORE"];

    fn cells(&self) -> Vec<String> {
        vec![
            dash(&self.at),
            dash(&self.event),
            dash(&self.kind),
            dash(&self.detail),
            dash(&self.store),
        ]
    }
}

impl ChangelogOut {
    fn of(log: &Changelog) -> Self {
        let rows = log
            .entries()
            .iter()
            .map(|entry| {
                let event = entry.event();
                let detail = match event.body() {
                    EventBody::Approved { by, attestation } => {
                        format!("{} by {by}", attestation.gate().name())
                    }
                    EventBody::Revoked { by, reason, .. } => format!("by {by}: {reason}"),
                    EventBody::Moved { old, new } => format!("{old} -> {new}"),
                    _ => String::new(),
                };
                EventRowOut {
                    at: entry.at().to_string(),
                    event: event.id().to_string(),
                    kind: event.body().kind_name().to_owned(),
                    detail,
                    store: entry.store().as_str().to_owned(),
                }
            })
            .collect();
        Self {
            events: Table::new(rows),
        }
    }
}

impl Shape for ChangelogOut {
    fn sections(&self) -> Vec<Section> {
        vec![self.events.section("Events")]
    }

    fn table(&mut self) -> Option<&mut dyn Windowed> {
        Some(&mut self.events)
    }
}

// ---- audit -------------------------------------------------------------------

/// The answer of `audit`.
#[derive(Debug, Serialize, JsonSchema)]
pub struct AuditOut {
    /// One row per approval, including those withdrawn.
    approvals: Table<AuditRowOut>,
}

/// An approval in the audit.
#[derive(Debug, Serialize, JsonSchema)]
struct AuditRowOut {
    /// The time shown for it.
    at: String,
    /// The gate.
    gate: String,
    /// The entity approved.
    subject: String,
    /// Who approved.
    by: String,
    /// A CA: the revision it was made at; else null.
    revision: Option<String>,
    /// A CA: how the tests went; else null.
    tests: Option<String>,
    /// A CM: what the measurement showed; else null.
    result: Option<String>,
    /// The `Approved` event.
    event: String,
    /// `revoked` when a later event withdrew it, else `-`.
    revoked: String,
}

impl Row for AuditRowOut {
    const COLUMNS: &'static [&'static str] = &[
        "AT", "GATE", "SUBJECT", "BY", "REVISION", "TESTS", "RESULT", "EVENT", "REVOKED",
    ];

    fn cells(&self) -> Vec<String> {
        vec![
            dash(&self.at),
            dash(&self.gate),
            dash(&self.subject),
            dash(&self.by),
            dash_opt(self.revision.as_deref()),
            dash_opt(self.tests.as_deref()),
            dash_opt(self.result.as_deref()),
            dash(&self.event),
            dash(&self.revoked),
        ]
    }
}

impl AuditOut {
    fn of(report: &AuditReport) -> Self {
        let rows = report
            .rows()
            .iter()
            .map(|row| {
                let (revision, tests, result) = match row.detail() {
                    AuditDetail::Ca { revision, tests } => (
                        Some(revision_text(revision)),
                        Some(format!("{tests:?}").to_lowercase()),
                        None,
                    ),
                    AuditDetail::Cm { result } => (
                        None,
                        None,
                        Some(match result {
                            CmResult::Pass => "pass".to_owned(),
                            CmResult::FailWiring { item_hint } => {
                                format!("fail-wiring: {item_hint}")
                            }
                            CmResult::FailHyp { finding } => format!("fail-hyp: {finding}"),
                        }),
                    ),
                    AuditDetail::Plain => (None, None, None),
                };
                AuditRowOut {
                    at: row.at().to_string(),
                    gate: row.gate().name().to_owned(),
                    subject: row.subject().to_string(),
                    by: row.by().to_string(),
                    revision,
                    tests,
                    result,
                    event: row.event().to_string(),
                    revoked: if row.revoked() { "revoked" } else { "-" }.to_owned(),
                }
            })
            .collect();
        Self {
            approvals: Table::new(rows),
        }
    }
}

impl Shape for AuditOut {
    fn sections(&self) -> Vec<Section> {
        vec![self.approvals.section("Approvals")]
    }

    fn table(&mut self) -> Option<&mut dyn Windowed> {
        Some(&mut self.approvals)
    }
}

// ---- rebuild -----------------------------------------------------------------

/// The answer of `rebuild`.
#[derive(Debug, Serialize, JsonSchema)]
pub struct RebuiltOut {
    /// What the rebuild did.
    rebuilt: RebuiltFields,
}

/// What the rebuild did.
#[derive(Debug, Serialize, JsonSchema)]
struct RebuiltFields {
    /// How many entities were revised.
    revised: usize,
}

impl Shape for RebuiltOut {
    fn sections(&self) -> Vec<Section> {
        vec![Section::fields(vec![(
            "revised",
            self.rebuilt.revised.to_string(),
        )])]
    }
}

// ---- migrate -----------------------------------------------------------------

/// The answer of `migrate`.
#[derive(Debug, Serialize, JsonSchema)]
pub struct MigrateOut {
    /// What was found, set against what this build reads.
    versions: Table<VersionRowOut>,
    /// The verdict.
    verdict: VerdictFields,
}

/// The verdict of a census.
#[derive(Debug, Serialize, JsonSchema)]
struct VerdictFields {
    /// Whether this build reads everything that was found.
    verdict: String,
}

/// One kind of file in the census.
#[derive(Debug, Serialize, JsonSchema)]
struct VersionRowOut {
    /// What kind of file.
    what: String,
    /// The versions found and how many files of each.
    found: String,
    /// The versions this build reads.
    build: String,
    /// A note; empty for none.
    note: String,
}

impl Row for VersionRowOut {
    const COLUMNS: &'static [&'static str] = &["WHAT", "FOUND", "BUILD", "NOTE"];

    fn cells(&self) -> Vec<String> {
        vec![
            dash(&self.what),
            dash(&self.found),
            dash(&self.build),
            dash(&self.note),
        ]
    }
}

impl MigrateOut {
    /// The versions found, set against the ones this build reads.
    ///
    /// The words are the census's: what was found and what is read. Nothing says a
    /// format is "newest", because this build only knows what it reads.
    fn of(found: &Census) -> Self {
        let reads = |versions: Vec<u32>| {
            let list: Vec<String> = versions.iter().map(|v| format!("v{v}")).collect();
            format!("reads: {}", list.join(", "))
        };
        let counted = |counts: Option<&BTreeMap<Found, usize>>| match counts {
            Some(counts) if !counts.is_empty() => counts
                .iter()
                .map(|(found, count)| format!("{found}: {count}"))
                .collect::<Vec<_>>()
                .join("  "),
            _ => "no file".to_owned(),
        };
        let mut rows = Vec::new();
        for source in Source::ALL {
            let found_text = match found.unplaced().iter().find(|u| u.source == source) {
                Some(unplaced) => format!(
                    "not looked at: the place cannot be decided ({})",
                    unplaced.reason.describe()
                ),
                None => counted(found.sources().get(&source)),
            };
            rows.push(VersionRowOut {
                what: source.name().to_owned(),
                found: found_text,
                build: reads(source.reads()),
                note: String::new(),
            });
        }
        for projection in Projection::ALL {
            let Some(counts) = found.projections().get(&projection) else {
                continue;
            };
            rows.push(VersionRowOut {
                what: projection.name().to_owned(),
                found: counted(Some(counts)),
                build: reads(projection.reads()),
                note: "derived: `whyshelf rebuild` writes it again".to_owned(),
            });
        }
        let verdict = match found.verdict() {
            Verdict::Readable => {
                "readable: every file counted is of a version this build reads".to_owned()
            }
            Verdict::Unreadable(groups) => {
                let list: Vec<String> = groups
                    .iter()
                    .map(|g| format!("{} {} ({} file(s))", g.source.name(), g.found, g.count))
                    .collect();
                format!("unreadable by this build: {}", list.join("; "))
            }
            Verdict::Incomplete {
                unplaced,
                unreadable,
            } => {
                let list: Vec<String> = unplaced
                    .iter()
                    .map(|u| format!("{} ({})", u.source.name(), u.reason.describe()))
                    .collect();
                let mut text = format!("incomplete: not looked at: {}", list.join("; "));
                if !unreadable.is_empty() {
                    let groups: Vec<String> = unreadable
                        .iter()
                        .map(|g| format!("{} {} ({} file(s))", g.source.name(), g.found, g.count))
                        .collect();
                    text.push_str(&format!(
                        "; unreadable by this build: {}",
                        groups.join("; ")
                    ));
                }
                text
            }
        };
        Self {
            versions: Table::new(rows),
            verdict: VerdictFields { verdict },
        }
    }
}

impl Shape for MigrateOut {
    fn sections(&self) -> Vec<Section> {
        vec![
            self.versions.section("Versions"),
            Section::fields(vec![("verdict", self.verdict.verdict.clone())]),
        ]
    }

    fn table(&mut self) -> Option<&mut dyn Windowed> {
        Some(&mut self.versions)
    }
}

// ---- lines -------------------------------------------------------------------

/// Lines that are printed as they are.
#[derive(Debug, Serialize, JsonSchema)]
pub struct LinesOut {
    /// The lines; absent when there are none.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    lines: Vec<String>,
}

impl Shape for LinesOut {
    fn sections(&self) -> Vec<Section> {
        Section::lines("", &self.lines).into_iter().collect()
    }
}

// ---- the words ---------------------------------------------------------------

/// Spell the machine check's finding in a sentence.
pub fn violation_text(violation: &CheckViolation) -> String {
    match violation {
        CheckViolation::TargetInvalid { reasons } => {
            let why: Vec<String> = reasons.iter().map(ToString::to_string).collect();
            format!("the target is invalid: {}", why.join("; "))
        }
        CheckViolation::EmptyBody => "the Finding has no body".to_owned(),
        CheckViolation::MeasuredDataNotLabelledMeasured => {
            "a measured_data Finding must be labelled `evidence: measured`".to_owned()
        }
        CheckViolation::Upstream { upstream, problem } => {
            let why = match problem {
                UpstreamProblem::Invalid => "it is invalid",
                UpstreamProblem::Unresolved => "it cannot be read here",
                UpstreamProblem::NotApproved => "it is not approved",
                UpstreamProblem::Stale => "it is stale",
                UpstreamProblem::Blocked => "it is blocked",
            };
            format!("upstream {upstream} is not ready: {why}")
        }
        CheckViolation::UpstreamUnverifiable { upstream } => {
            format!("the pin of upstream {upstream} cannot be verified here")
        }
        CheckViolation::NoPinsDeclared => "the Spec declares no pin".to_owned(),
        CheckViolation::NoHypsDeclared => "the Spec declares no hypothesis".to_owned(),
        CheckViolation::PinCoverage(issue) => format!("pin coverage: {issue:?}"),
        CheckViolation::PinMissingInTests { pin } => {
            format!("the item declares {pin} but no test carries it")
        }
        CheckViolation::PinUndeclaredInTests { pin } => {
            format!("a test carries {pin} but no Spec of the project declares it")
        }
        CheckViolation::MalformedPin { location, text } => {
            format!("{location}: `{}` is not a pin name", text.as_str())
        }
        CheckViolation::TestsFailed => "the tests did not pass".to_owned(),
        CheckViolation::TestsTimedOut { after } => format!(
            "the tests did not finish within {}: raise `test.timeout` or make them faster",
            duration_text(*after)
        ),
        CheckViolation::ItemNotInDesign => "the Design no longer has this item".to_owned(),
        CheckViolation::MeasurementNotMeasuredData { finding } => {
            format!("{finding} is not a measured_data Finding")
        }
        CheckViolation::CsApprovalNotFound => {
            "no CS approval fixed the hypotheses of this Spec".to_owned()
        }
        CheckViolation::HypothesisChanged { fixed, current } => {
            format!("the hypotheses changed after CS fixed them ({fixed} became {current})")
        }
    }
}

fn approval_text(approval: &Approval) -> &'static str {
    match approval {
        Approval::Invalid(_) => "Invalid",
        Approval::Draft => "Draft",
        Approval::Approved => "Approved",
        Approval::Modified { .. } => "Modified",
    }
}

fn upstream_text(upstream: &Upstream) -> &'static str {
    match upstream {
        Upstream::Ok => "Ok",
        Upstream::Stale(_) => "Stale",
        Upstream::Blocked(_) => "Blocked",
    }
}

fn integration_text(integration: &Integration) -> &'static str {
    match integration {
        Integration::Unapproved => "Unapproved",
        Integration::ApprovedPending { .. } => "Pending",
        Integration::Integrated { .. } => "Integrated",
        Integration::Unknown { .. } => "Unknown",
    }
}

/// A time limit the way the settings write it: the largest unit that divides it.
fn duration_text(span: std::time::Duration) -> String {
    let seconds = span.as_secs();
    match seconds {
        s if s != 0 && s % 3600 == 0 => format!("{}h", s / 3600),
        s if s != 0 && s % 60 == 0 => format!("{}m", s / 60),
        s if span.subsec_nanos() == 0 => format!("{s}s"),
        _ => format!("{}ms", span.as_millis()),
    }
}

/// What the status says of a classification problem.
fn issue_text(issue: &ClassificationIssue) -> String {
    match issue {
        ClassificationIssue::FileDiffers { entity, drift } => format!(
            "{entity}: the file says {} {}, the store has {}; the store is kept (change it with `tag`, or edit the file)",
            drift.field, drift.file, drift.store
        ),
        other => format!("{other:?}"),
    }
}

fn integrated_text(fact: IntegrationFact) -> &'static str {
    match fact {
        IntegrationFact::Integrated => " (integrated)",
        IntegrationFact::NotIntegrated => "",
        IntegrationFact::Unknown { .. } => " (integration unknown here)",
    }
}

fn revision_text(revision: &Revision) -> String {
    match revision {
        Revision::Commit(id) => id.to_string(),
        Revision::Patch { base, .. } => format!("{base}+patch"),
    }
}

/// Why a state is not plain: each reason on its own.
fn reasons(state: &EntityState) -> Vec<String> {
    let mut out = Vec::new();
    if let Approval::Invalid(list) = state.approval() {
        out.extend(list.iter().map(ToString::to_string));
    }
    match state.upstream() {
        Upstream::Ok => {}
        Upstream::Stale(list) => out.extend(list.iter().map(ToString::to_string)),
        Upstream::Blocked(list) => out.extend(list.iter().map(ToString::to_string)),
    }
    // Partial is a fact of its own: it is listed whatever the axes say.
    out.extend(
        state
            .completeness()
            .reasons()
            .map(|reason| format!("unresolved: {reason}")),
    );
    out
}

fn completeness_text(completeness: &Completeness) -> &'static str {
    match completeness {
        Completeness::Complete => "Complete",
        Completeness::Partial(_) => "Unresolved",
    }
}

#[cfg(test)]
mod tests {
    use whyshelf_core::domain::nonempty::NonEmptyVec;
    use whyshelf_core::domain::state::{BlockReason, UnresolvedReason};

    use super::*;

    fn id(last: u8) -> EntityId {
        format!("01912345-6789-7abc-8def-0123456789a{last}")
            .parse()
            .expect("a test identifier")
    }

    fn partial(root: u8, upstream: u8) -> Completeness {
        Completeness::from_reasons(BTreeSet::from([UnresolvedReason::new(
            id(root),
            id(upstream),
        )]))
    }

    #[test]
    fn a_partial_state_lists_its_reasons_beside_the_ones_of_the_axes() {
        // A public Draft upstream blocks, and another upstream cannot be read.
        let state = EntityState::new(
            Approval::Draft,
            Upstream::Blocked(NonEmptyVec::new(
                BlockReason::UpstreamUnapproved { upstream: id(1) },
                vec![],
            )),
            partial(2, 2),
        );
        let lines = reasons(&state);
        assert_eq!(lines.len(), 2, "{lines:?}");
        assert!(lines[0].contains(&id(1).to_string()) && lines[0].contains("not approved"));
        assert!(lines[1].starts_with("unresolved: "));
        assert!(lines[1].contains(&id(2).to_string()));
    }

    #[test]
    fn a_complete_state_adds_no_line() {
        let state = EntityState::new(Approval::Draft, Upstream::Ok, Completeness::Complete);
        assert!(reasons(&state).is_empty());
        assert_eq!(completeness_text(&Completeness::Complete), "Complete");
        assert_eq!(completeness_text(&partial(2, 3)), "Unresolved");
    }

    #[test]
    fn the_route_to_an_unreadable_entity_is_told() {
        let state = EntityState::new(Approval::Approved, Upstream::Ok, partial(2, 3));
        let lines = reasons(&state);
        assert!(
            lines[0].contains(&format!("through {}", id(3))),
            "{lines:?}"
        );
    }

    /// A CA approval can name an older revision. What is shown is the revision it
    /// recorded, whatever the working tree is at now; a gate with no revision shows
    /// none.
    #[test]
    fn an_approval_shows_the_revision_it_recorded() {
        use whyshelf_core::domain::approval::{Attestation, CaChange, Gate, Pinned, TestOutcome};
        use whyshelf_core::domain::event::EventId;
        use whyshelf_core::domain::hash::ContentHash;
        use whyshelf_core::domain::id::{DesignId, ItemId};
        use whyshelf_core::domain::revision::RevisionId;

        let old = "a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2";
        let hash = ContentHash::raw(b"x");
        let ca = Attestation::Ca {
            item: Pinned::new(ItemId::new(id(1)), hash),
            design: Pinned::new(DesignId::new(id(2)), hash.into()),
            revision: Revision::Commit(RevisionId::from_validated(old.to_owned()).unwrap()),
            tests: TestOutcome::Passed,
            change: CaChange::NotRecorded,
        };
        let event: EventId = format!("sha256:{}", "0".repeat(64)).parse().unwrap();
        let shown = |gate, attestation| {
            let outcome = Outcome::Approved {
                gate,
                event,
                attestation,
            };
            serde_json::to_string(&output(&outcome, &BTreeSet::new())).expect("JSON")
        };
        let ca_text = shown(Gate::Ca, ca);
        assert!(ca_text.contains(&format!("CA at {old}")), "{ca_text}");
        let cr = Attestation::Cr {
            finding: Pinned::new(
                whyshelf_core::domain::id::FindingId::new(id(3)),
                ContentHash::raw(b"y"),
            ),
        };
        let cr_text = shown(Gate::Cr, cr);
        assert!(
            cr_text.contains("CR") && !cr_text.contains(" at "),
            "{cr_text}"
        );
    }

    #[test]
    fn an_invalid_target_is_told_with_its_reasons() {
        use whyshelf_core::domain::state::InvalidReason;

        let text = violation_text(&CheckViolation::TargetInvalid {
            reasons: NonEmptyVec::new(InvalidReason::DanglingRef { upstream: id(1) }, vec![]),
        });
        assert!(text.starts_with("the target is invalid: "), "{text}");
        assert!(text.contains(&id(1).to_string()), "{text}");
    }

    #[test]
    fn a_check_says_in_words_that_an_upstream_cannot_be_read() {
        let text = violation_text(&CheckViolation::Upstream {
            upstream: id(1),
            problem: UpstreamProblem::Unresolved,
        });
        assert!(text.ends_with("it cannot be read here"), "{text}");
    }

    /// The keys a row type declares in its schema.
    fn schema_keys<T: JsonSchema>() -> BTreeSet<String> {
        let schema = serde_json::to_value(schemars::schema_for!(T)).expect("a schema");
        schema["properties"]
            .as_object()
            .expect("an object with properties")
            .keys()
            .cloned()
            .collect()
    }

    /// The labels of the columns are what the text shows and the keys of a row are
    /// what the JSON shows: the two are one list.
    fn the_columns_are_the_keys<R: Row>() {
        let columns: BTreeSet<String> = R::COLUMNS.iter().map(|label| key_of(label)).collect();
        assert_eq!(columns.len(), R::COLUMNS.len(), "a column twice");
        assert_eq!(columns, schema_keys::<R>(), "{:?}", R::COLUMNS);
    }

    #[test]
    fn the_columns_of_every_table_are_the_keys_of_its_rows() {
        the_columns_are_the_keys::<RegisteredRow>();
        the_columns_are_the_keys::<FoundRow>();
        the_columns_are_the_keys::<StatusRowOut>();
        the_columns_are_the_keys::<TraceRowOut>();
        the_columns_are_the_keys::<EventRowOut>();
        the_columns_are_the_keys::<AuditRowOut>();
        the_columns_are_the_keys::<VersionRowOut>();
    }

    fn registered(names: &[&str]) -> Table<RegisteredRow> {
        Table::new(
            names
                .iter()
                .map(|name| RegisteredRow {
                    id: format!("id-{name}"),
                    path: format!("docs/{name}.md"),
                })
                .collect(),
        )
    }

    #[test]
    fn a_table_cut_to_some_columns_keeps_the_keys_of_those_columns_in_json_and_text() {
        let mut table = registered(&["a", "b"]);
        table.select(&["path".to_owned()]).expect("a column");
        assert_eq!(
            serde_json::to_string(&table).expect("JSON"),
            r#"[{"path":"docs/a.md"},{"path":"docs/b.md"}]"#
        );
        let Block::Table { columns, grid } = table.section("T").block else {
            panic!("a table");
        };
        assert_eq!(columns, ["PATH"]);
        assert_eq!(grid, [["docs/a.md"], ["docs/b.md"]]);
    }

    #[test]
    fn a_column_that_does_not_exist_is_refused_and_the_ones_there_are_listed() {
        let mut table = registered(&["a"]);
        let error = table.select(&["nope".to_owned()]).unwrap_err();
        let CliError::UnknownColumn { name, columns } = error else {
            panic!("an unknown column");
        };
        assert_eq!((name.as_str(), columns.as_str()), ("nope", "id, path"));
    }

    #[test]
    fn a_window_cuts_rows_and_says_where_the_next_one_starts() {
        let mut table = registered(&["a", "b", "c"]);
        assert_eq!(table.cut(1, Some(1)), Some(2));
        let json = serde_json::to_value(&table).expect("JSON");
        assert_eq!(json.as_array().map(Vec::len), Some(1));
        assert_eq!(json[0]["path"], "docs/b.md");
        // The last window has no successor, and an offset past the end is empty.
        let mut table = registered(&["a", "b", "c"]);
        assert_eq!(table.cut(2, Some(5)), None);
        assert_eq!(table.rows.len(), 1);
        assert_eq!(table.cut(9, None), None);
        assert!(table.rows.is_empty());
    }

    #[test]
    fn a_line_output_with_no_lines_says_nothing_in_either_form() {
        let out = Output::Lines(LinesOut { lines: Vec::new() });
        assert!(out.sections().is_empty());
        assert_eq!(serde_json::to_string(&out).expect("JSON"), "{}");
    }
}
