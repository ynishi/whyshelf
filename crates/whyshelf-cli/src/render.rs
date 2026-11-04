//! Showing an outcome: as a table, as JSON, as Markdown.
//!
//! An [`Outcome`] is first turned into a [`Doc`], a few named sections that are
//! a table, a list of fields or a list of lines, with the values kept as JSON
//! values. One document renders in the three [`Format`]s, so a command never has
//! three renderers and the formats cannot disagree about what a command says.
//! Nothing here decides anything: a name is looked up, a state is spelled, a
//! list is cut to the window the caller asked for.

use std::collections::BTreeMap;
use std::fmt::Write;

use clap::ValueEnum;
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::{Map, Value, json};
use whyshelf_core::app::check::CheckReport;
use whyshelf_core::app::query::{EntityContent, EntityView, Field, FoundPage, GetResult};
use whyshelf_core::app::view::{
    AuditReport, Changelog, ContextReport, StatusReport, StatusRow, TraceReport,
};
use whyshelf_core::domain::approval::CmResult;
use whyshelf_core::domain::event::{EventBody, StoreKind};
use whyshelf_core::domain::id::EntityId;
use whyshelf_core::domain::lint::{CheckViolation, UpstreamProblem};
use whyshelf_core::domain::revision::Revision;
use whyshelf_core::domain::state::{Approval, EntityState, Integration, Upstream};

use crate::command::Outcome;
use crate::error::CliError;

/// How a result is written.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, ValueEnum, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum Format {
    /// Aligned columns for a terminal.
    #[default]
    Table,
    /// One JSON object, sections by name.
    Json,
    /// Headings, pipe tables and lists.
    Markdown,
}

/// The cut of the first table of a document that a caller asked for.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Window {
    limit: Option<usize>,
    offset: usize,
    fields: Vec<String>,
}

impl Window {
    /// A window of `limit` rows after `cursor`, with only the columns named in
    /// `fields` (all of them when empty).
    ///
    /// The cursor is the one an earlier answer gave; it is opaque to the caller.
    pub fn new(
        limit: Option<usize>,
        cursor: Option<&str>,
        fields: Vec<String>,
    ) -> Result<Self, CliError> {
        let offset = match cursor {
            Some(text) => text
                .parse::<usize>()
                .map_err(|e| CliError::argument("cursor", e))?,
            None => 0,
        };
        Ok(Self {
            limit,
            offset,
            fields,
        })
    }
}

/// A cell, a field or a line is a JSON value so that JSON output keeps lists.
type Cell = Value;

#[derive(Debug)]
enum Body {
    Table {
        columns: Vec<&'static str>,
        rows: Vec<Vec<Cell>>,
    },
    Fields(Vec<(&'static str, Cell)>),
    Lines(Vec<String>),
}

#[derive(Debug)]
struct Section {
    key: &'static str,
    title: &'static str,
    body: Body,
}

/// What a command says, before it is written in a format.
#[derive(Debug, Default)]
pub struct Doc {
    sections: Vec<Section>,
    next_cursor: Option<String>,
}

impl Doc {
    fn table(
        mut self,
        key: &'static str,
        title: &'static str,
        columns: Vec<&'static str>,
        rows: Vec<Vec<Cell>>,
    ) -> Self {
        self.sections.push(Section {
            key,
            title,
            body: Body::Table { columns, rows },
        });
        self
    }

    fn fields(mut self, key: &'static str, fields: Vec<(&'static str, Cell)>) -> Self {
        self.sections.push(Section {
            key,
            title: "",
            body: Body::Fields(fields),
        });
        self
    }

    fn lines(mut self, key: &'static str, title: &'static str, lines: Vec<String>) -> Self {
        if !lines.is_empty() {
            self.sections.push(Section {
                key,
                title,
                body: Body::Lines(lines),
            });
        }
        self
    }

    fn with_next(mut self, cursor: Option<String>) -> Self {
        self.next_cursor = cursor;
        self
    }

    /// Cut the first table to the window: the columns asked for, then `limit`
    /// rows after the cursor. The cursor of the next window is kept.
    pub fn window(mut self, window: &Window) -> Result<Self, CliError> {
        let Some(Section {
            body: Body::Table { columns, rows },
            ..
        }) = self
            .sections
            .iter_mut()
            .find(|s| matches!(s.body, Body::Table { .. }))
        else {
            return Ok(self);
        };
        if !window.fields.is_empty() {
            let keep: Vec<usize> = window
                .fields
                .iter()
                .map(|name| {
                    columns
                        .iter()
                        .position(|c| c.eq_ignore_ascii_case(name))
                        .ok_or_else(|| CliError::UnknownColumn {
                            name: name.clone(),
                            columns: columns.join(", ").to_ascii_lowercase(),
                        })
                })
                .collect::<Result<_, _>>()?;
            *columns = keep.iter().map(|&i| columns[i]).collect();
            for row in rows.iter_mut() {
                *row = keep.iter().map(|&i| row[i].clone()).collect();
            }
        }
        let total = rows.len();
        let start = window.offset.min(total);
        let end = window
            .limit
            .map_or(total, |l| start.saturating_add(l).min(total));
        *rows = rows.drain(start..end).collect();
        if end < total {
            self.next_cursor = Some(end.to_string());
        }
        Ok(self)
    }

    /// Write the document.
    pub fn render(&self, format: Format) -> Result<String, CliError> {
        match format {
            Format::Json => self.json(),
            Format::Table => Ok(self.text(false)),
            Format::Markdown => Ok(self.text(true)),
        }
    }

    fn json(&self) -> Result<String, CliError> {
        let mut object = Map::new();
        for section in &self.sections {
            object.insert(section.key.to_owned(), section.body.json());
        }
        if let Some(cursor) = &self.next_cursor {
            object.insert("next_cursor".to_owned(), json!(cursor));
        }
        Ok(serde_json::to_string_pretty(&Value::Object(object))?)
    }

    fn text(&self, markdown: bool) -> String {
        let titled = self.sections.len() > 1;
        let mut blocks: Vec<String> = Vec::new();
        for section in &self.sections {
            let mut block = String::new();
            if titled && !section.title.is_empty() {
                if markdown {
                    let _ = writeln!(block, "## {}\n", section.title);
                } else {
                    let _ = writeln!(block, "{}:", section.title);
                }
            }
            block.push_str(&section.body.text(markdown));
            blocks.push(block);
        }
        if let Some(cursor) = &self.next_cursor {
            blocks.push(format!("next cursor: {cursor}\n"));
        }
        blocks.join("\n")
    }
}

impl Body {
    fn json(&self) -> Value {
        match self {
            Self::Table { columns, rows } => Value::Array(
                rows.iter()
                    .map(|row| {
                        Value::Object(
                            columns
                                .iter()
                                .zip(row)
                                .map(|(c, v)| (c.to_ascii_lowercase().replace(' ', "_"), v.clone()))
                                .collect(),
                        )
                    })
                    .collect(),
            ),
            Self::Fields(fields) => Value::Object(
                fields
                    .iter()
                    .map(|(k, v)| ((*k).to_owned(), v.clone()))
                    .collect(),
            ),
            Self::Lines(lines) => json!(lines),
        }
    }

    fn text(&self, markdown: bool) -> String {
        let mut out = String::new();
        match self {
            Self::Table { columns, rows } => {
                let grid: Vec<Vec<String>> = rows
                    .iter()
                    .map(|row| row.iter().map(cell_text).collect())
                    .collect();
                if markdown {
                    pipe_table(&mut out, columns, &grid);
                } else {
                    aligned_table(&mut out, columns, &grid);
                }
            }
            Self::Fields(fields) => {
                let width = fields.iter().map(|(k, _)| k.len()).max().unwrap_or(0);
                for (key, value) in fields {
                    if markdown {
                        let _ = writeln!(out, "- **{key}**: {}", cell_text(value));
                    } else {
                        let _ = writeln!(out, "{key:<width$}  {}", cell_text(value));
                    }
                }
            }
            Self::Lines(lines) => {
                for line in lines {
                    if markdown {
                        let _ = writeln!(out, "- {line}");
                    } else {
                        let _ = writeln!(out, "{line}");
                    }
                }
            }
        }
        out
    }
}

fn cell_text(value: &Cell) -> String {
    match value {
        Value::Null => "-".to_owned(),
        Value::String(text) if text.is_empty() => "-".to_owned(),
        Value::String(text) => text.clone(),
        Value::Array(items) if items.is_empty() => "-".to_owned(),
        Value::Array(items) => items.iter().map(cell_text).collect::<Vec<_>>().join(", "),
        other => other.to_string(),
    }
}

fn aligned_table(out: &mut String, columns: &[&str], grid: &[Vec<String>]) {
    let widths: Vec<usize> = (0..columns.len())
        .map(|i| {
            grid.iter()
                .map(|row| row[i].chars().count())
                .chain([columns[i].chars().count()])
                .max()
                .unwrap_or(0)
        })
        .collect();
    let mut line = |cells: &[String]| {
        let text: Vec<String> = cells
            .iter()
            .zip(&widths)
            .map(|(cell, w)| format!("{cell:<w$}"))
            .collect();
        let _ = writeln!(out, "{}", text.join("  ").trim_end());
    };
    let header: Vec<String> = columns.iter().map(|c| c.to_ascii_uppercase()).collect();
    line(&header);
    for row in grid {
        line(row);
    }
}

fn pipe_table(out: &mut String, columns: &[&str], grid: &[Vec<String>]) {
    let escape = |text: &str| text.replace('|', "\\|").replace(['\n', '\r'], " ");
    let _ = writeln!(out, "| {} |", columns.join(" | "));
    let _ = writeln!(out, "|{}", "---|".repeat(columns.len()));
    for row in grid {
        let cells: Vec<String> = row.iter().map(|c| escape(c)).collect();
        let _ = writeln!(out, "| {} |", cells.join(" | "));
    }
}

/// Spell the machine check's finding in a sentence.
pub fn violation_text(violation: &CheckViolation) -> String {
    match violation {
        CheckViolation::EmptyBody => "the Finding has no body".to_owned(),
        CheckViolation::MeasuredDataNotLabelledMeasured => {
            "a measured_data Finding must be labelled `evidence: measured`".to_owned()
        }
        CheckViolation::Upstream { upstream, problem } => {
            let why = match problem {
                UpstreamProblem::Missing => "it does not exist",
                UpstreamProblem::Invalid => "it is invalid",
                UpstreamProblem::NotApproved => "it is not approved",
                UpstreamProblem::Stale => "it is stale",
                UpstreamProblem::Blocked => "it is blocked",
            };
            format!("upstream {upstream} is not ready: {why}")
        }
        CheckViolation::NoPinsDeclared => "the Spec declares no pin".to_owned(),
        CheckViolation::NoHypsDeclared => "the Spec declares no hypothesis".to_owned(),
        CheckViolation::PinCoverage(issue) => format!("pin coverage: {issue:?}"),
        CheckViolation::PinMissingInTests { pin } => {
            format!("the item declares {pin} but no test carries it")
        }
        CheckViolation::PinUndeclaredInTests { pin } => {
            format!("a test carries {pin} but the Spec never declared it")
        }
        CheckViolation::TestsFailed => "the tests did not pass".to_owned(),
        CheckViolation::ItemNotInDesign => "the Design no longer has this item".to_owned(),
        CheckViolation::CsApprovalNotFound => {
            "no CS approval fixed the hypotheses of this Spec".to_owned()
        }
        CheckViolation::HypothesisChanged { fixed, current } => {
            format!("the hypotheses changed after CS fixed them ({fixed} became {current})")
        }
    }
}

fn store_name(store: StoreKind) -> &'static str {
    match store {
        StoreKind::Repo => "repo",
        StoreKind::Private => "private",
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
    out
}

/// What to call an entity: its alias, else its path, else (an item) its
/// Design's name and number, else its identifier.
fn row_name(row: &StatusRow, names: &BTreeMap<EntityId, String>) -> String {
    if let Some(alias) = row.alias() {
        alias.to_string()
    } else if let Some(path) = row.path() {
        path.to_string()
    } else {
        match (row.number(), row.parent()) {
            (Some(number), Some(parent)) => {
                let design = names.get(&parent).cloned().unwrap_or(parent.to_string());
                // An item number is written with its own `#`.
                format!("{design}{number}")
            }
            _ => row.id().to_string(),
        }
    }
}

fn status_doc(report: &StatusReport) -> Doc {
    let mut rows = Vec::new();
    let mut notes = Vec::new();
    let names: BTreeMap<EntityId, String> = report
        .rows()
        .iter()
        .filter(|row| row.parent().is_none())
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
        rows.push(vec![
            json!(name),
            json!(row.kind().name()),
            json!(row.label()),
            json!(approval),
            json!(upstream),
            json!(row.integration().map_or("-", integration_text)),
            json!(row.pins().map_or_else(
                || "-".to_owned(),
                |t| format!("{}/{}", t.found(), t.declared())
            )),
            json!(row.next().to_string()),
        ]);
    }
    for path in report.unregistered() {
        rows.push(vec![
            json!(path.to_string()),
            json!("-"),
            Value::Null,
            json!("Unregistered"),
            json!("-"),
            json!("-"),
            json!("-"),
            json!(StatusReport::next_for_unregistered(path).to_string()),
        ]);
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
            .issues()
            .iter()
            .map(|i| format!("{} store: {:?}", store_name(i.store), i.issue)),
    );
    notes.extend(report.exports().iter().map(|e| {
        format!(
            "{} store: {:?} is {:?}; `whyshelf rebuild` writes it again",
            store_name(e.scope),
            e.kind,
            e.problem
        )
    }));
    Doc::default()
        .table(
            "entities",
            "Entities",
            vec![
                "ID",
                "KIND",
                "TITLE",
                "APPROVAL",
                "UPSTREAM",
                "INTEGRATION",
                "PINS",
                "NEXT",
            ],
            rows,
        )
        .lines("notes", "Notes", notes)
}

fn entity_fields(view: &EntityView) -> Vec<(&'static str, Cell)> {
    let mut fields = vec![
        ("id", json!(view.id().to_string())),
        ("kind", json!(view.kind().name())),
        ("store", json!(store_name(view.store()))),
        ("path", json!(view.path().map(ToString::to_string))),
        ("alias", json!(view.alias().map(ToString::to_string))),
        ("owner", json!(view.owner().map(ToString::to_string))),
        (
            "tags",
            json!(
                view.tags()
                    .iter()
                    .map(ToString::to_string)
                    .collect::<Vec<_>>()
            ),
        ),
    ];
    if let Some(state) = view.state() {
        fields.push(("approval", json!(approval_text(state.approval()))));
        fields.push(("upstream", json!(upstream_text(state.upstream()))));
        fields.push(("reasons", json!(reasons(state))));
    }
    fields.extend(content_fields(view.content()));
    fields.push((
        "approvals",
        json!(
            view.approvals()
                .iter()
                .map(|a| format!(
                    "{} by {} at {} ({})",
                    a.attestation().gate().name(),
                    a.by(),
                    a.at(),
                    a.event()
                ))
                .collect::<Vec<_>>()
        ),
    ));
    fields
}

fn content_fields(content: &EntityContent) -> Vec<(&'static str, Cell)> {
    match content {
        EntityContent::Finding(f) => vec![
            ("finding_type", json!(f.source().type_name())),
            ("title", json!(f.title().as_str())),
            (
                "evidence",
                json!(format!("{:?}", f.evidence()).to_lowercase()),
            ),
            ("body", json!(f.body())),
        ],
        EntityContent::Spec(s) => vec![
            ("contract", json!(s.contract().as_str())),
            (
                "grounds",
                json!(
                    s.grounds()
                        .iter()
                        .map(ToString::to_string)
                        .collect::<Vec<_>>()
                ),
            ),
            (
                "pins",
                json!(
                    s.pins()
                        .iter()
                        .map(|(id, c)| format!("{id}: {}", c.as_str()))
                        .collect::<Vec<_>>()
                ),
            ),
            (
                "hyps",
                json!(
                    s.hyps()
                        .iter()
                        .map(|(id, h)| format!("{id}: {} -> {}", h.metric(), h.target()))
                        .collect::<Vec<_>>()
                ),
            ),
        ],
        EntityContent::Design(d) => vec![
            ("spec", json!(d.spec().to_string())),
            ("question", json!(d.crux().question().as_str())),
            (
                "chosen",
                json!(format!(
                    "{}: {}",
                    d.crux().chosen().id(),
                    d.crux().chosen().summary()
                )),
            ),
            (
                "rejected",
                json!(
                    d.crux()
                        .rejected()
                        .iter()
                        .map(|o| format!("{}: {}", o.id(), o.summary()))
                        .collect::<Vec<_>>()
                ),
            ),
            (
                "items",
                json!(
                    d.items()
                        .iter()
                        .map(|(n, i)| format!(
                            "{n}: {} [{}]",
                            i.summary(),
                            i.assigned_pins()
                                .iter()
                                .map(ToString::to_string)
                                .collect::<Vec<_>>()
                                .join(", ")
                        ))
                        .collect::<Vec<_>>()
                ),
            ),
        ],
        EntityContent::Unparsed(u) => vec![(
            "parse_errors",
            json!(
                u.reasons()
                    .iter()
                    .map(ToString::to_string)
                    .collect::<Vec<_>>()
            ),
        )],
        EntityContent::FileMissing => vec![("file", json!("missing"))],
        EntityContent::Item {
            design,
            number,
            item,
            integration,
            progress,
            ..
        } => vec![
            ("design", json!(design.to_string())),
            ("number", json!(number.to_string())),
            ("summary", json!(item.summary().as_str())),
            ("integration", json!(integration_text(integration))),
            (
                "pins_declared",
                json!(
                    progress
                        .declared()
                        .iter()
                        .map(ToString::to_string)
                        .collect::<Vec<_>>()
                ),
            ),
            (
                "pins_found",
                json!(
                    progress
                        .found()
                        .iter()
                        .map(ToString::to_string)
                        .collect::<Vec<_>>()
                ),
            ),
        ],
    }
}

fn get_doc(result: &GetResult) -> Doc {
    match result {
        GetResult::Entity(view) => Doc::default().fields("entity", entity_fields(view)),
        GetResult::Unresolved(reference) => Doc::default().fields(
            "unresolved",
            vec![
                ("reference", json!(reference.to_string())),
                ("note", json!("this environment cannot look it up")),
            ],
        ),
        GetResult::External(uri) => {
            Doc::default().fields("external", vec![("uri", json!(uri.to_string()))])
        }
    }
}

fn found_doc(page: &FoundPage, fields: &std::collections::BTreeSet<Field>) -> Doc {
    let wants = |f: Field| fields.is_empty() || fields.contains(&f);
    let mut columns = vec!["ID", "KIND"];
    for (field, name) in [
        (Field::Store, "STORE"),
        (Field::Path, "PATH"),
        (Field::Alias, "ALIAS"),
        (Field::Title, "TITLE"),
        (Field::Tags, "TAGS"),
        (Field::Owner, "OWNER"),
    ] {
        if wants(field) {
            columns.push(name);
        }
    }
    if wants(Field::State) {
        columns.extend(["APPROVAL", "UPSTREAM"]);
    }
    let rows = page
        .rows()
        .iter()
        .map(|row| {
            let mut cells = vec![json!(row.id.to_string()), json!(row.kind.name())];
            if wants(Field::Store) {
                cells.push(json!(row.store.map(store_name)));
            }
            if wants(Field::Path) {
                cells.push(json!(row.path.as_ref().map(ToString::to_string)));
            }
            if wants(Field::Alias) {
                cells.push(json!(row.alias.as_ref().map(ToString::to_string)));
            }
            if wants(Field::Title) {
                cells.push(json!(row.title));
            }
            if wants(Field::Tags) {
                cells.push(json!(row.tags.as_ref().map(|t| {
                    t.iter().map(ToString::to_string).collect::<Vec<_>>()
                })));
            }
            if wants(Field::Owner) {
                cells.push(json!(row.owner.as_ref().map(ToString::to_string)));
            }
            if wants(Field::State) {
                cells.push(json!(
                    row.state.as_ref().map(|s| approval_text(s.approval()))
                ));
                cells.push(json!(
                    row.state.as_ref().map(|s| upstream_text(s.upstream()))
                ));
            }
            cells
        })
        .collect();
    Doc::default()
        .table("found", "Found", columns, rows)
        .fields("page", vec![("total", json!(page.total()))])
        .with_next(page.next().map(|c| c.to_string()))
}

fn check_doc(report: &CheckReport) -> Doc {
    Doc::default()
        .fields(
            "check",
            vec![
                ("gate", json!(report.gate().name())),
                ("target", json!(report.target().to_string())),
                (
                    "result",
                    json!(if report.passed() { "passed" } else { "failed" }),
                ),
            ],
        )
        .lines(
            "violations",
            "Violations",
            report.violations().iter().map(violation_text).collect(),
        )
}

fn trace_doc(report: &TraceReport) -> Doc {
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
            let approved: Vec<String> = node
                .approved()
                .iter()
                .map(|a| {
                    format!(
                        "{} by {}{}",
                        revision_text(&a.revision),
                        a.by,
                        if a.integrated { " (integrated)" } else { "" }
                    )
                })
                .collect();
            vec![
                json!(node_name(node)),
                json!(node.kind().name()),
                json!(approval),
                json!(upstream),
                json!(approved),
            ]
        })
        .collect();
    let links = report
        .links()
        .iter()
        .map(|(from, to)| format!("{} -> {}", name_of(*from), name_of(*to)))
        .collect();
    Doc::default()
        .table(
            "nodes",
            "Chain",
            vec!["ID", "KIND", "APPROVAL", "UPSTREAM", "APPROVED AT"],
            rows,
        )
        .lines("links", "Links", links)
}

fn node_name(node: &whyshelf_core::app::view::TraceNode) -> String {
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

fn changelog_doc(log: &Changelog) -> Doc {
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
            vec![
                json!(event.at().to_string()),
                json!(event.id().to_string()),
                json!(event.body().kind_name()),
                json!(detail),
                json!(store_name(entry.store())),
            ]
        })
        .collect();
    Doc::default().table(
        "events",
        "Events",
        vec!["AT", "EVENT", "KIND", "DETAIL", "STORE"],
        rows,
    )
}

fn audit_doc(report: &AuditReport) -> Doc {
    let rows = report
        .rows()
        .iter()
        .map(|row| {
            let result = row.result().map(|r| match r {
                CmResult::Pass => "pass".to_owned(),
                CmResult::FailWiring { item_hint } => format!("fail-wiring: {item_hint}"),
                CmResult::FailHyp { finding } => format!("fail-hyp: {finding}"),
            });
            vec![
                json!(row.at().to_string()),
                json!(row.gate().name()),
                json!(row.subject().to_string()),
                json!(row.by().to_string()),
                json!(row.revision().map(revision_text)),
                json!(row.tests().map(|t| format!("{t:?}").to_lowercase())),
                json!(result),
                json!(row.event().to_string()),
                json!(if row.revoked() { "revoked" } else { "-" }),
            ]
        })
        .collect();
    Doc::default().table(
        "approvals",
        "Approvals",
        vec![
            "AT", "GATE", "SUBJECT", "BY", "REVISION", "TESTS", "RESULT", "EVENT", "REVOKED",
        ],
        rows,
    )
}

fn context_doc(report: &ContextReport) -> Doc {
    let line = report.line();
    Doc::default().fields(
        "context",
        vec![
            ("project", json!(line.project().to_string())),
            ("worktree", json!(line.worktree().to_string())),
            ("revision", json!(line.revision().to_string())),
            ("branch", json!(line.label())),
            ("default_store", json!(store_name(report.default_store()))),
            ("repo_events", json!(report.events_in(StoreKind::Repo))),
            (
                "private_events",
                json!(report.events_in(StoreKind::Private)),
            ),
        ],
    )
}

/// The document an outcome says.
pub fn document(outcome: &Outcome, fields: &std::collections::BTreeSet<Field>) -> Doc {
    match outcome {
        Outcome::Context(report) => context_doc(report),
        Outcome::Added(added) => Doc::default()
            .table(
                "registered",
                "Registered",
                vec!["ID", "PATH"],
                added
                    .registered()
                    .iter()
                    .map(|(id, path)| vec![json!(id.to_string()), json!(path.to_string())])
                    .collect(),
            )
            .lines(
                "skipped",
                "Already registered",
                added.skipped().iter().map(ToString::to_string).collect(),
            ),
        Outcome::Recorded(committed) => Doc::default().fields(
            "recorded",
            vec![(
                "events",
                json!(
                    committed
                        .events()
                        .iter()
                        .map(ToString::to_string)
                        .collect::<Vec<_>>()
                ),
            )],
        ),
        Outcome::Scaffolded(made) => Doc::default().fields(
            "created",
            vec![
                ("id", json!(made.id().to_string())),
                ("alias", json!(made.alias().to_string())),
                ("path", json!(made.path().to_string())),
            ],
        ),
        Outcome::Checked(report) => check_doc(report),
        Outcome::Approved {
            gate,
            event,
            context,
        } => {
            let at = match context.label() {
                Some(label) => format!("{} ({label})", context.revision()),
                None => context.revision().to_string(),
            };
            Doc::default().fields(
                "approved",
                vec![
                    ("recorded", json!(format!("{} at {at}", gate.name()))),
                    ("event", json!(event.to_string())),
                ],
            )
        }
        Outcome::Got(result) => get_doc(result),
        Outcome::Found(page) => found_doc(page, fields),
        Outcome::Status(report) => status_doc(report),
        Outcome::Trace(report) => trace_doc(report),
        Outcome::Changelog(log) => changelog_doc(log),
        Outcome::Audit(report) => audit_doc(report),
        Outcome::Rebuilt { revised } => {
            Doc::default().fields("rebuilt", vec![("revised", json!(revised))])
        }
    }
}
