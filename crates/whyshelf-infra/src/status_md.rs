//! The Markdown of the status and the changelog.
//!
//! Both are committed files that a reader sees in a pull request, so they say
//! only what is public: the status is the public part of the report (the
//! application removes the private entities before it hands it over), and the
//! changelog lists only the events of the store it is written for, never those
//! of the private store that a published entity had before it was published. The
//! working directory of the machine is not printed either.
//!
//! Neither file has a marker here: [`crate::export`] puts it on the first line.

use std::fmt::Write;

use whyshelf_core::app::projection::ChangelogDoc;
use whyshelf_core::app::view::{StatusReport, StatusRow};
use whyshelf_core::domain::event::{Event, EventBody};
use whyshelf_core::domain::query::StateFilter;
use whyshelf_core::domain::state::{EntityState, Integration};

/// A table cell: no pipe, no line break.
fn cell(text: &str) -> String {
    text.replace('|', "\\|").replace(['\n', '\r'], " ")
}

fn name_of(row: &StatusRow) -> String {
    if let Some(alias) = row.alias() {
        return alias.to_string();
    }
    if let Some(path) = row.path() {
        return path.to_string();
    }
    match (row.number(), row.parent()) {
        (Some(number), Some(parent)) => format!("{number} of {parent}"),
        _ => row.id().to_string(),
    }
}

/// The upstream cell: the axis, and "unresolved" beside it when something the
/// entity stands on cannot be read where the report was made. The words are the
/// domain's ([`StateFilter`]).
fn upstream_cell(state: &EntityState) -> String {
    let axis = StateFilter::upstream(state.upstream()).name();
    if state.completeness().is_partial() {
        format!("{axis}, {}", StateFilter::Unresolved.name())
    } else {
        axis.to_owned()
    }
}

fn integration_name(integration: &Integration) -> &'static str {
    match integration {
        Integration::Unapproved => "unapproved",
        Integration::ApprovedPending { .. } => "pending",
        Integration::Integrated { .. } => "integrated",
    }
}

/// The status as Markdown, without a marker.
pub fn render_status(report: &StatusReport) -> String {
    let mut out = String::from("# whyshelf status\n\n");
    let _ = writeln!(out, "Project: `{}`\n", report.context().project());
    out.push_str("| entity | kind | approval | upstream | integration | pins | next |\n");
    out.push_str("|---|---|---|---|---|---|---|\n");
    for row in report.rows() {
        let (approval, upstream) = row.state().map_or(("missing", "-".to_owned()), |state| {
            (
                StateFilter::approval(state.approval()).name(),
                upstream_cell(state),
            )
        });
        let integration = row.integration().map_or("-", integration_name);
        let pins = row.pins().map_or_else(
            || "-".to_owned(),
            |t| format!("{}/{}", t.found(), t.declared()),
        );
        let mut name = cell(&name_of(row));
        if let Some(label) = row.label() {
            let _ = write!(name, " ({})", cell(label));
        }
        let _ = writeln!(
            out,
            "| {name} | {} | {approval} | {upstream} | {integration} | {pins} | {} |",
            row.kind().name(),
            cell(&row.next().to_string()),
        );
    }
    let sections: [(&str, Vec<String>); 3] = [
        (
            "Not registered",
            report
                .unregistered()
                .iter()
                .map(ToString::to_string)
                .collect(),
        ),
        (
            "File missing",
            report.missing().iter().map(ToString::to_string).collect(),
        ),
        (
            "Changed since the last ingest",
            report
                .changed_since_ingest()
                .iter()
                .map(ToString::to_string)
                .collect(),
        ),
    ];
    for (title, paths) in sections {
        if !paths.is_empty() {
            let _ = writeln!(out, "\n## {title}\n");
            for path in paths {
                let _ = writeln!(out, "- `{path}`");
            }
        }
    }
    if !report.issues().is_empty() {
        let _ = writeln!(
            out,
            "\n## Needs a decision\n\n{} classification conflict(s) or collision(s); `whyshelf status` lists them.",
            report.issues().len()
        );
    }
    out
}

fn describe(event: &Event) -> String {
    match event.body() {
        EventBody::Approved { by, attestation } => {
            format!("Approved {} by {by}", attestation.gate().name())
        }
        EventBody::Revoked { by, .. } => format!("Revoked by {by}"),
        EventBody::Moved { old, new } => format!("Moved `{old}` to `{new}`"),
        other => other.kind_name().to_owned(),
    }
}

/// The changelog as Markdown, without a marker: one section per entity, with
/// the events of this store in the order of the event graph.
pub fn render_changelog(doc: &ChangelogDoc) -> String {
    let mut out = String::from("# whyshelf changelog\n");
    for (id, changelog) in doc.entries() {
        let entries: Vec<_> = changelog
            .entries()
            .iter()
            .filter(|entry| entry.store() == doc.scope())
            .collect();
        if entries.is_empty() {
            continue;
        }
        let _ = writeln!(out, "\n## `{id}`\n");
        for entry in entries {
            let event = entry.event();
            let short: String = event.id().hash().hex().chars().take(12).collect();
            let _ = writeln!(out, "- {} {} (`{short}`)", event.at(), describe(event));
        }
    }
    out
}
