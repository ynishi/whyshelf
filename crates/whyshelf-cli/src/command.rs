//! The commands as values, and the one function that runs them.
//!
//! A [`Command`] is what a person (or a tool client) asked for, already parsed
//! into the types of `whyshelf-core`; [`execute`] maps it onto exactly one use
//! case of the [`Workspace`] and returns an [`Outcome`]. The command line and
//! the tool server both end here, so a command and its tool cannot drift apart,
//! and nothing in this crate decides anything the use cases decide.
//!
//! Before any command but `rebuild`, [`execute`] brings the log up to date with
//! the files (`ingest`): the document files are edited by hand, and an edit
//! becomes history at the next command, so a gate always looks at what is
//! recorded. What ingest could not record, and what a subscriber failed at, come
//! back as warnings next to the outcome; they never fail the command.

use std::collections::BTreeSet;
use std::error::Error;

use whyshelf_core::app::Workspace;
use whyshelf_core::app::check::{Approved, CheckReport, CmInput};
use whyshelf_core::app::commit::Committed;
use whyshelf_core::app::ingest::IngestReport;
use whyshelf_core::app::ports::Ports;
use whyshelf_core::app::query::{Field, FoundPage, GetResult, PageRequest, Target};
use whyshelf_core::app::raw::RawRevision;
use whyshelf_core::app::register::{AddOptions, Added};
use whyshelf_core::app::scaffold::Scaffolded;
use whyshelf_core::app::view::{AuditReport, Changelog, ContextReport, StatusReport, TraceReport};
use whyshelf_core::domain::actor::Actor;
use whyshelf_core::domain::approval::{Attestation, Gate};
use whyshelf_core::domain::event::EventId;
use whyshelf_core::domain::finding::FindingType;
use whyshelf_core::domain::id::Alias;
use whyshelf_core::domain::location::RelPath;
use whyshelf_core::domain::nonempty::{NonBlank, NonEmptyVec};
use whyshelf_core::domain::query::Query;
use whyshelf_core::domain::reference::{ItemRef, Ref};
use whyshelf_core::domain::tag::TagChange;

use whyshelf_infra::format::Census;

use crate::error::{CliError, chain};

/// Which gate to run, with what it is run on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CheckTarget {
    /// CR on a Finding.
    Cr(Ref),
    /// CS on a Spec.
    Cs(Ref),
    /// CI on a Design.
    Ci(Ref),
    /// CA on an item, at the current revision or a named one.
    Ca {
        /// The item.
        item: ItemRef,
        /// A past revision to approve, instead of the current one.
        revision: Option<RawRevision>,
    },
    /// CM on a Spec: always a record, never a dry run.
    Cm {
        /// The Spec.
        spec: Ref,
        /// What the measurement showed.
        result: CmInput,
    },
}

/// One command that needs the workspace.
#[derive(Debug, Clone)]
pub enum Command {
    /// Where the tool is pointed.
    Context,
    /// Register files.
    Add {
        /// The files or directories.
        paths: Vec<RelPath>,
        /// Kind, tags, alias and store.
        options: AddOptions,
    },
    /// Add and remove tags.
    Tag {
        /// The entity.
        target: Ref,
        /// The edits, at least one.
        changes: Vec<TagChange>,
    },
    /// Record a new path of a document.
    Mv {
        /// The entity.
        target: Ref,
        /// Where its file is now.
        new_path: RelPath,
    },
    /// Move a document from the private store to the repo store.
    Publish {
        /// The entity.
        target: Ref,
    },
    /// Start a Finding.
    FindingNew {
        /// Its type.
        finding_type: FindingType,
        /// Its title.
        title: NonBlank,
        /// Its alias; derived from the title when absent.
        alias: Option<Alias>,
    },
    /// Start a Spec.
    SpecNew {
        /// Its alias.
        alias: Alias,
        /// The Finding it stands on.
        from: Ref,
    },
    /// Start a Design.
    DesignNew {
        /// Its alias.
        alias: Alias,
        /// The Spec it is for.
        spec: Ref,
    },
    /// Run a gate; with `approve`, record the approval when the check passes.
    Check {
        /// Which gate, on what.
        target: CheckTarget,
        /// Record the approval.
        approve: bool,
        /// The approver, when not the one in the settings.
        by: Option<Actor>,
    },
    /// Withdraw an approval.
    Revoke {
        /// The `Approved` event.
        approval: EventId,
        /// Why.
        reason: NonBlank,
        /// Who withdraws it, when not the one in the settings.
        by: Option<Actor>,
    },
    /// One entity, content and classification.
    Get {
        /// The entity or item.
        target: Target,
    },
    /// Many entities, by one query.
    Find {
        /// The query.
        query: Query,
        /// Limit, cursor and fields.
        page: PageRequest,
    },
    /// The status of every entity.
    Status,
    /// The chain around one entity.
    Trace {
        /// The entity.
        target: Ref,
    },
    /// The history of one entity.
    Changelog {
        /// The entity.
        target: Ref,
    },
    /// Who approved what.
    Audit {
        /// Narrow the approvals to the entities this query matches.
        filter: Option<Query>,
    },
    /// Rebuild the cache and the generated files.
    Rebuild,
}

/// What a command produced.
#[derive(Debug)]
pub enum Outcome {
    /// The context.
    Context(ContextReport),
    /// Files registered.
    Added(Added),
    /// A change recorded (tags, path, publication, revocation).
    Recorded(Committed),
    /// A new document started.
    Scaffolded(Scaffolded),
    /// A machine check, not approved.
    Checked(CheckReport),
    /// An approval or a measurement recorded.
    Approved {
        /// The gate.
        gate: Gate,
        /// The event recorded.
        event: EventId,
        /// What was recorded: for a CA, the revision it was made at, which is not
        /// necessarily the one the working tree is at now.
        attestation: Attestation,
    },
    /// One entity.
    Got(GetResult),
    /// A page of entities.
    Found(FoundPage),
    /// The status.
    Status(StatusReport),
    /// The trace.
    Trace(TraceReport),
    /// The history.
    Changelog(Changelog),
    /// The approvals.
    Audit(AuditReport),
    /// What a rebuild did.
    Rebuilt {
        /// How many entities were revised.
        revised: usize,
    },
    /// The versions of the project's data, counted (`migrate`). It needs no
    /// workspace, so [`execute`] never makes one.
    Migrate(Census),
    /// Lines for standard output, as they are (`init`, `schema map`): nothing in them
    /// is judged.
    Printed(Vec<String>),
}

/// An outcome and the warnings that came with it.
#[derive(Debug)]
pub struct Done {
    /// The result.
    pub outcome: Outcome,
    /// Things that went wrong without failing the command, one per entry.
    pub warnings: Vec<String>,
}

/// Parse the names of the columns a caller wants from `find`.
pub fn parse_fields<S: AsRef<str>>(names: &[S]) -> Result<BTreeSet<Field>, CliError> {
    names
        .iter()
        .map(|name| match name.as_ref() {
            "store" => Ok(Field::Store),
            "path" => Ok(Field::Path),
            "alias" => Ok(Field::Alias),
            "title" => Ok(Field::Title),
            "tags" => Ok(Field::Tags),
            "owner" => Ok(Field::Owner),
            "state" => Ok(Field::State),
            other => Err(CliError::UnknownField(other.to_owned())),
        })
        .collect()
}

fn warning(error: &dyn Error) -> String {
    format!("{error}{}", chain(error))
}

fn committed_warnings(committed: &Committed, into: &mut Vec<String>) {
    into.extend(committed.warnings().iter().map(|w| warning(w)));
}

fn ingest_warnings(report: &IngestReport, into: &mut Vec<String>) {
    for blocked in report.blocked() {
        into.push(format!(
            "{} was not recorded: {}",
            blocked.path(),
            warning(blocked.error())
        ));
    }
    into.extend(report.warnings().iter().map(|w| warning(w)));
}

fn non_empty<T>(items: Vec<T>) -> Result<NonEmptyVec<T>, CliError> {
    Ok(NonEmptyVec::from_vec(items)?)
}

/// Run one command on the workspace.
///
/// Writes take the workspace by `&mut`, so one workspace never runs two writes
/// at once; the tool server keeps that true across requests by giving the
/// workspace to a single task (see [`crate::actor`]).
pub async fn execute<P: Ports>(
    workspace: &mut Workspace<P>,
    command: Command,
) -> Result<Done, CliError> {
    let mut warnings = Vec::new();
    if !matches!(command, Command::Rebuild) {
        let report = workspace.ingest().await?;
        ingest_warnings(&report, &mut warnings);
    }
    let outcome = run(workspace, command, &mut warnings).await?;
    Ok(Done { outcome, warnings })
}

async fn run<P: Ports>(
    ws: &mut Workspace<P>,
    command: Command,
    warnings: &mut Vec<String>,
) -> Result<Outcome, CliError> {
    match command {
        Command::Context => Ok(Outcome::Context(ws.context().await?)),
        Command::Add { paths, options } => {
            let added = ws.add(&paths, &options).await?;
            committed_warnings(added.committed(), warnings);
            Ok(Outcome::Added(added))
        }
        Command::Tag { target, changes } => {
            let committed = ws.tag(&target, non_empty(changes)?).await?;
            committed_warnings(&committed, warnings);
            Ok(Outcome::Recorded(committed))
        }
        Command::Mv { target, new_path } => recorded(ws.mv(&target, new_path).await?, warnings),
        Command::Publish { target } => recorded(ws.publish(&target).await?, warnings),
        Command::FindingNew {
            finding_type,
            title,
            alias,
        } => scaffolded(ws.finding_new(finding_type, title, alias).await?, warnings),
        Command::SpecNew { alias, from } => scaffolded(ws.spec_new(alias, &from).await?, warnings),
        Command::DesignNew { alias, spec } => {
            scaffolded(ws.design_new(alias, &spec).await?, warnings)
        }
        Command::Check {
            target,
            approve,
            by,
        } => check(ws, target, approve, by, warnings).await,
        Command::Revoke {
            approval,
            reason,
            by,
        } => recorded(ws.revoke(approval, reason, by).await?, warnings),
        Command::Get { target } => Ok(Outcome::Got(ws.get(&target).await?)),
        Command::Find { query, page } => Ok(Outcome::Found(ws.find(&query, &page).await?)),
        Command::Status => Ok(Outcome::Status(ws.status().await?)),
        Command::Trace { target } => Ok(Outcome::Trace(ws.trace(&target).await?)),
        Command::Changelog { target } => Ok(Outcome::Changelog(ws.changelog(&target).await?)),
        Command::Audit { filter } => Ok(Outcome::Audit(ws.audit(filter.as_ref()).await?)),
        Command::Rebuild => {
            let report = ws.rebuild().await?;
            ingest_warnings(&report, warnings);
            Ok(Outcome::Rebuilt {
                revised: report.revised().len(),
            })
        }
    }
}

fn recorded(committed: Committed, warnings: &mut Vec<String>) -> Result<Outcome, CliError> {
    committed_warnings(&committed, warnings);
    Ok(Outcome::Recorded(committed))
}

fn scaffolded(scaffolded: Scaffolded, warnings: &mut Vec<String>) -> Result<Outcome, CliError> {
    committed_warnings(scaffolded.committed(), warnings);
    Ok(Outcome::Scaffolded(scaffolded))
}

fn approved(
    gate: Gate,
    approved: Approved,
    warnings: &mut Vec<String>,
) -> Result<Outcome, CliError> {
    committed_warnings(approved.committed(), warnings);
    Ok(Outcome::Approved {
        gate,
        event: approved.event(),
        attestation: approved.attestation().clone(),
    })
}

async fn check<P: Ports>(
    ws: &mut Workspace<P>,
    target: CheckTarget,
    approve: bool,
    by: Option<Actor>,
    warnings: &mut Vec<String>,
) -> Result<Outcome, CliError> {
    match (target, approve) {
        (CheckTarget::Cr(r), false) => Ok(Outcome::Checked(ws.check_cr(&r).await?)),
        (CheckTarget::Cs(r), false) => Ok(Outcome::Checked(ws.check_cs(&r).await?)),
        (CheckTarget::Ci(r), false) => Ok(Outcome::Checked(ws.check_ci(&r).await?)),
        (CheckTarget::Ca { item, revision }, false) => Ok(Outcome::Checked(
            ws.check_ca(&item, revision.as_ref()).await?,
        )),
        (CheckTarget::Cr(r), true) => {
            let done = ws.approve_cr(&r, by).await?;
            approved(Gate::Cr, done, warnings)
        }
        (CheckTarget::Cs(r), true) => {
            let done = ws.approve_cs(&r, by).await?;
            approved(Gate::Cs, done, warnings)
        }
        (CheckTarget::Ci(r), true) => {
            let done = ws.approve_ci(&r, by).await?;
            approved(Gate::Ci, done, warnings)
        }
        (CheckTarget::Ca { item, revision }, true) => {
            let done = ws.approve_ca(&item, revision.as_ref(), by).await?;
            approved(Gate::Ca, done, warnings)
        }
        (CheckTarget::Cm { spec, result }, _) => {
            let done = ws.record_cm(&spec, &result, by).await?;
            approved(Gate::Cm, done, warnings)
        }
    }
}
