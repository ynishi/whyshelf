//! `whyshelf mcp`: the commands as tools of a Model Context Protocol server over
//! standard input and output.
//!
//! Each tool is one command of the command line with the same name and the same
//! meaning (`whyshelf_status` is `whyshelf status`), and runs the same
//! [`Command`] through the same [`execute`](crate::command::execute). The
//! server does not touch the workspace itself: it sends the command to the
//! [`WorkspaceActor`], which owns the workspace and runs one command at a time.
//!
//! **Every answer starts with the context line**, `[context: project=..,
//! worktree=.., revision=..]`, an error answer included, so a caller can never
//! take the answer for the answer about another project or checkout. The rest is
//! the result in the requested format (Markdown unless asked otherwise), then the
//! warnings, if any. When the context line cannot be read, the line says
//! `[context: unavailable]` and the reason is a warning of the answer.
//!
//! **A tool that lists** (`find`, `status`, `trace`, `changelog`, `audit`) takes
//! `limit`, `cursor` and `fields`, so that an answer cannot flood the caller's
//! context. `find` is paged by the use case (the cursor is the last identifier
//! returned); the other lists are cut to the window after the result is built (the
//! cursor is a position, and `fields` names columns). An answer that has more says
//! `next cursor` (`next_cursor` in JSON).
//!
//! `init`, `schema`, `migrate` and `mcp` have no tool: they act before there is a
//! workspace, or on the files rather than on the project's history.

use std::collections::BTreeSet;
use std::error::Error;
use std::str::FromStr;

use rmcp::handler::server::wrapper::Parameters;
use rmcp::{ServerHandler, ServiceExt, tool, tool_handler, tool_router};
use schemars::JsonSchema;
use serde::Deserialize;
use whyshelf_core::app::Workspace;
use whyshelf_core::app::check::CmInput;
use whyshelf_core::app::ports::Ports;
use whyshelf_core::app::query::{Cursor, Field, PageLimit, PageRequest, Target};
use whyshelf_core::app::raw::RawRevision;
use whyshelf_core::app::register::AddOptions;
use whyshelf_core::app::view::ContextLine;
use whyshelf_core::domain::actor::Actor;
use whyshelf_core::domain::event::{DocKind, EventId, StoreKind};
use whyshelf_core::domain::finding::FindingType;
use whyshelf_core::domain::id::Alias;
use whyshelf_core::domain::location::RelPath;
use whyshelf_core::domain::nonempty::NonBlank;
use whyshelf_core::domain::reference::ItemRef;
use whyshelf_core::domain::tag::{Tag, TagChange};

use crate::actor::{ActorHandle, Answer, WorkspaceActor};
use crate::args::query_of;
use crate::command::{CheckTarget, Command, parse_fields};
use crate::error::CliError;
use crate::render::{Format, Window, document};

/// What the first line of an answer says when the context could not be read.
const UNAVAILABLE: &str = "[context: unavailable]";

/// The first line of an answer, and the warning that says why when the context
/// line could not be read.
fn head_of(context: Result<ContextLine, String>) -> (String, Option<String>) {
    match context {
        Ok(line) => (line.to_string(), None),
        Err(failure) => (UNAVAILABLE.to_owned(), Some(failure)),
    }
}

/// The warnings of an answer as the text that follows the result; nothing when
/// there are none.
fn warnings_block(warnings: &[String]) -> String {
    if warnings.is_empty() {
        return String::new();
    }
    let mut text = String::from("\nwarnings:\n");
    for warning in warnings {
        text.push_str(&format!("- {warning}\n"));
    }
    text
}

/// How an answer is written.
#[derive(Debug, Default, Deserialize, JsonSchema)]
struct Rendering {
    /// How to write the answer; default: markdown.
    format: Option<Format>,
}

/// How a list is cut, and how it is written.
#[derive(Debug, Default, Deserialize, JsonSchema)]
struct Listing {
    /// How to write the answer; default: markdown.
    format: Option<Format>,
    /// How many rows at most.
    limit: Option<usize>,
    /// Where to continue: the `next cursor` of the previous answer.
    cursor: Option<String>,
    /// Only these columns. For `whyshelf_find`: store, path, alias, title, tags,
    /// owner, state. For the other lists: the names of the columns of the table.
    fields: Option<Vec<String>>,
}

/// Arguments without any of their own.
#[derive(Debug, Deserialize, JsonSchema)]
struct NoArgs {
    #[serde(flatten)]
    rendering: Rendering,
}

/// Arguments of `whyshelf_add`.
#[derive(Debug, Deserialize, JsonSchema)]
struct AddArgs {
    /// Files or directories to register, relative to the project root.
    paths: Vec<String>,
    /// The kind of the documents, when the file does not say.
    kind: Option<KindChoice>,
    /// Tags to put on every document, such as `area:auth`.
    tags: Option<Vec<String>>,
    /// The alias of the document (for one file).
    alias: Option<String>,
    /// Where the documents go; default: the project's setting.
    store: Option<StoreChoice>,
    #[serde(flatten)]
    rendering: Rendering,
}

/// The kinds of document, as the tool input offers them.
///
/// The input shape is the tool server's own; the words are the domain's
/// (`DocKind::name`), and a test keeps the two sets the same.
#[derive(Debug, Clone, Copy, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
enum KindChoice {
    /// A Finding file.
    Finding,
    /// A Spec file.
    Spec,
    /// A Design file.
    Design,
}

impl From<KindChoice> for DocKind {
    fn from(kind: KindChoice) -> Self {
        match kind {
            KindChoice::Finding => Self::Finding,
            KindChoice::Spec => Self::Spec,
            KindChoice::Design => Self::Design,
        }
    }
}

/// The stores, as the tool input offers them (the words are the domain's:
/// `StoreKind::as_str`).
#[derive(Debug, Clone, Copy, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
enum StoreChoice {
    /// The store committed with the repository.
    Repo,
    /// The store outside the repository, never published.
    Private,
}

impl From<StoreChoice> for StoreKind {
    fn from(store: StoreChoice) -> Self {
        match store {
            StoreChoice::Repo => Self::Repo,
            StoreChoice::Private => Self::Private,
        }
    }
}

/// The types of Finding, as the tool input offers them (the words are the
/// domain's: `FindingType::name`).
#[derive(Debug, Clone, Copy, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
enum FindingTypeChoice {
    /// A measurement.
    MeasuredData,
    /// What users did and what went wrong.
    UsageObservation,
    /// A document from outside.
    ExternalSource,
    /// A look at existing code.
    CodeObservation,
    /// A limit set from outside the code.
    DomainConstraint,
}

impl From<FindingTypeChoice> for FindingType {
    fn from(finding_type: FindingTypeChoice) -> Self {
        match finding_type {
            FindingTypeChoice::MeasuredData => Self::MeasuredData,
            FindingTypeChoice::UsageObservation => Self::UsageObservation,
            FindingTypeChoice::ExternalSource => Self::ExternalSource,
            FindingTypeChoice::CodeObservation => Self::CodeObservation,
            FindingTypeChoice::DomainConstraint => Self::DomainConstraint,
        }
    }
}

/// Arguments of `whyshelf_tag`.
#[derive(Debug, Deserialize, JsonSchema)]
struct TagArgs {
    /// The entity: an alias or an identifier.
    target: String,
    /// `+tag` to add, `-tag` to remove, such as `["+area:auth", "-wip"]`.
    changes: Vec<String>,
    #[serde(flatten)]
    rendering: Rendering,
}

/// Arguments of `whyshelf_mv`.
#[derive(Debug, Deserialize, JsonSchema)]
struct MvArgs {
    /// The entity.
    target: String,
    /// Where its file is now.
    new_path: String,
    #[serde(flatten)]
    rendering: Rendering,
}

/// Arguments of a command on one entity.
#[derive(Debug, Deserialize, JsonSchema)]
struct TargetArgs {
    /// The entity: an alias or an identifier.
    target: String,
    #[serde(flatten)]
    rendering: Rendering,
}

/// Arguments of a list about one entity.
#[derive(Debug, Deserialize, JsonSchema)]
struct TargetListArgs {
    /// The entity: an alias or an identifier.
    target: String,
    #[serde(flatten)]
    listing: Listing,
}

/// Arguments of `whyshelf_finding_new`.
#[derive(Debug, Deserialize, JsonSchema)]
struct FindingNewArgs {
    /// The type of the Finding.
    finding_type: FindingTypeChoice,
    /// The title.
    title: String,
    /// The alias; default: `finding/` and the title as a slug.
    alias: Option<String>,
    #[serde(flatten)]
    rendering: Rendering,
}

/// Arguments of `whyshelf_spec_new`.
#[derive(Debug, Deserialize, JsonSchema)]
struct SpecNewArgs {
    /// The new Spec's alias.
    alias: String,
    /// The Finding it stands on.
    from: String,
    #[serde(flatten)]
    rendering: Rendering,
}

/// Arguments of `whyshelf_design_new`.
#[derive(Debug, Deserialize, JsonSchema)]
struct DesignNewArgs {
    /// The new Design's alias.
    alias: String,
    /// The Spec it is for.
    spec: String,
    #[serde(flatten)]
    rendering: Rendering,
}

/// The gates.
#[derive(Debug, Clone, Copy, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
enum Gate {
    /// A Finding.
    Cr,
    /// A Spec.
    Cs,
    /// A Design.
    Ci,
    /// An item.
    Ca,
    /// A measurement.
    Cm,
}

/// What a measurement showed.
#[derive(Debug, Clone, Copy, Deserialize, JsonSchema)]
enum Measured {
    /// The hypotheses were met.
    #[serde(rename = "pass")]
    Pass,
    /// A wiring or implementation gap; needs `item_hint`.
    #[serde(rename = "fail-wiring")]
    FailWiring,
    /// The hypothesis was wrong; needs `finding`.
    #[serde(rename = "fail-hyp")]
    FailHyp,
}

/// Arguments of `whyshelf_check`.
#[derive(Debug, Deserialize, JsonSchema)]
struct CheckArgs {
    /// The gate; the values are listed in the schema with what each one checks.
    gate: Gate,
    /// What to check: a reference of the kind the gate takes (an item as
    /// `<design>#<n>`).
    target: String,
    /// Record the approval when the check passes (not for `cm`, which always
    /// records).
    #[serde(default)]
    approve: bool,
    /// `ca` only: approve a past revision instead of the current one.
    revision: Option<String>,
    /// `cm` only: what the measurement showed.
    result: Option<Measured>,
    /// `cm` with `fail-wiring`: a hint at the missing item.
    item_hint: Option<String>,
    /// `cm` with `fail-hyp`: the Finding that records the measurement.
    finding: Option<String>,
    /// The approver; default: `user.name` of the settings.
    by: Option<String>,
    #[serde(flatten)]
    rendering: Rendering,
}

/// Arguments of `whyshelf_revoke`.
#[derive(Debug, Deserialize, JsonSchema)]
struct RevokeArgs {
    /// The `Approved` event to withdraw (the `event` of `whyshelf_audit`).
    approval: String,
    /// Why.
    reason: String,
    /// Who withdraws it; default: `user.name` of the settings.
    by: Option<String>,
    #[serde(flatten)]
    rendering: Rendering,
}

/// Arguments of `whyshelf_find`.
#[derive(Debug, Deserialize, JsonSchema)]
struct FindArgs {
    /// Terms, all of which must match: `tag:area:auth kind:spec state:stale
    /// owner:alice grounds:<ref> upstream:<ref> orphan:true`, and text.
    #[serde(default)]
    query: String,
    #[serde(flatten)]
    listing: Listing,
}

/// Arguments of `whyshelf_audit`.
#[derive(Debug, Deserialize, JsonSchema)]
struct AuditArgs {
    /// Narrow the approvals to the entities this query matches (as `find`).
    query: Option<String>,
    #[serde(flatten)]
    listing: Listing,
}

fn parse<T>(name: &'static str, text: &str) -> Result<T, CliError>
where
    T: FromStr,
    T::Err: Error + Send + Sync + 'static,
{
    text.parse().map_err(|e| CliError::argument(name, e))
}

fn parse_all<T>(name: &'static str, texts: &[String]) -> Result<Vec<T>, CliError>
where
    T: FromStr,
    T::Err: Error + Send + Sync + 'static,
{
    texts.iter().map(|t| parse(name, t)).collect()
}

fn non_blank(name: &'static str, text: &str) -> Result<NonBlank, CliError> {
    NonBlank::new(text).map_err(|e| CliError::argument(name, e))
}

fn actor(by: Option<&str>) -> Result<Option<Actor>, CliError> {
    by.map(|name| Actor::new(name).map_err(|e| CliError::argument("by", e)))
        .transpose()
}

fn required<'a>(
    name: &'static str,
    value: Option<&'a str>,
    gate: &str,
) -> Result<&'a str, CliError> {
    value.ok_or_else(|| {
        CliError::argument(
            name,
            std::io::Error::other(format!("{gate} needs `{name}`")),
        )
    })
}

impl AddArgs {
    fn command(&self) -> Result<Command, CliError> {
        let mut options = AddOptions::new().with_tags(
            parse_all::<Tag>("tags", self.tags.as_deref().unwrap_or_default())?
                .into_iter()
                .collect(),
        );
        if let Some(kind) = self.kind {
            options = options.with_kind(kind.into());
        }
        if let Some(alias) = &self.alias {
            options = options.with_alias(parse::<Alias>("alias", alias)?);
        }
        if let Some(store) = self.store {
            options = options.with_store(store.into());
        }
        Ok(Command::Add {
            paths: parse_all::<RelPath>("paths", &self.paths)?,
            options,
        })
    }
}

impl CheckArgs {
    fn command(&self) -> Result<Command, CliError> {
        let target = match self.gate {
            Gate::Cr => CheckTarget::Cr(parse("target", &self.target)?),
            Gate::Cs => CheckTarget::Cs(parse("target", &self.target)?),
            Gate::Ci => CheckTarget::Ci(parse("target", &self.target)?),
            Gate::Ca => CheckTarget::Ca {
                item: parse::<ItemRef>("target", &self.target)?,
                revision: self
                    .revision
                    .as_deref()
                    .map(|r| RawRevision::new(r).map_err(|e| CliError::argument("revision", e)))
                    .transpose()?,
            },
            Gate::Cm => {
                let result = self.result.ok_or_else(|| {
                    CliError::argument("result", std::io::Error::other("cm needs `result`"))
                })?;
                CheckTarget::Cm {
                    spec: parse("target", &self.target)?,
                    result: match result {
                        Measured::Pass => CmInput::Pass,
                        Measured::FailWiring => CmInput::FailWiring {
                            item_hint: non_blank(
                                "item_hint",
                                required("item_hint", self.item_hint.as_deref(), "fail-wiring")?,
                            )?,
                        },
                        Measured::FailHyp => CmInput::FailHyp {
                            finding: parse(
                                "finding",
                                required("finding", self.finding.as_deref(), "fail-hyp")?,
                            )?,
                        },
                    },
                }
            }
        };
        Ok(Command::Check {
            target,
            approve: self.approve,
            by: actor(self.by.as_deref())?,
        })
    }
}

/// The server: tools that send commands to the workspace's task.
#[derive(Debug, Clone)]
pub struct WhyshelfServer {
    actor: ActorHandle,
}

impl WhyshelfServer {
    /// A server that talks to this task.
    pub fn new(actor: ActorHandle) -> Self {
        Self { actor }
    }

    /// Run a command and write the answer: the context line, the result, the
    /// warnings. An error is an answer too, and starts with the context line as
    /// well.
    async fn answer(
        &self,
        command: Result<Command, CliError>,
        format: Option<Format>,
        window: Window,
        fields: BTreeSet<Field>,
    ) -> Result<String, String> {
        let command = match command {
            Ok(command) => command,
            // Nothing to run, but the caller still learns where it was pointed.
            Err(error) => return Err(self.with_context(error.report()).await),
        };
        let Answer { result, context } = self.actor.call(command).await.map_err(|e| e.report())?;
        let (head, context_failure) = head_of(context);
        let body = result.and_then(|done| {
            let doc = document(&done.outcome, &fields).window(&window)?;
            let mut text = doc.render(format.unwrap_or(Format::Markdown))?;
            let mut warnings = done.warnings;
            warnings.extend(context_failure.clone());
            text.push_str(&warnings_block(&warnings));
            Ok(text)
        });
        match body {
            Ok(text) => Ok(format!("{head}\n{text}")),
            Err(error) => {
                let mut text = error.report();
                text.push_str(&warnings_block(
                    &context_failure.into_iter().collect::<Vec<_>>(),
                ));
                Err(format!("{head}\n{text}"))
            }
        }
    }

    async fn with_context(&self, message: String) -> String {
        let (head, failure) = match self.actor.call(Command::Context).await {
            Ok(Answer { context, .. }) => head_of(context),
            Err(error) => (
                UNAVAILABLE.to_owned(),
                Some(format!(
                    "the context line could not be read: {}",
                    error.report()
                )),
            ),
        };
        let mut text = format!("{head}\n{message}");
        text.push_str(&warnings_block(&failure.into_iter().collect::<Vec<_>>()));
        text
    }

    async fn plain(
        &self,
        command: Result<Command, CliError>,
        rendering: &Rendering,
    ) -> Result<String, String> {
        self.answer(
            command,
            rendering.format,
            Window::default(),
            BTreeSet::new(),
        )
        .await
    }

    async fn listed(
        &self,
        command: Result<Command, CliError>,
        listing: &Listing,
    ) -> Result<String, String> {
        let window = Window::new(
            listing.limit,
            listing.cursor.as_deref(),
            listing.fields.clone().unwrap_or_default(),
        );
        match window {
            Ok(window) => {
                self.answer(command, listing.format, window, BTreeSet::new())
                    .await
            }
            Err(error) => Err(self.with_context(error.report()).await),
        }
    }
}

#[tool_router]
impl WhyshelfServer {
    #[tool(
        name = "whyshelf_context",
        description = "Where the tool is pointed: project, worktree, current revision, stores."
    )]
    async fn context(&self, Parameters(args): Parameters<NoArgs>) -> Result<String, String> {
        self.plain(Ok(Command::Context), &args.rendering).await
    }

    #[tool(
        name = "whyshelf_add",
        description = "Register files or directories of documents. Never changes the files."
    )]
    async fn add(&self, Parameters(args): Parameters<AddArgs>) -> Result<String, String> {
        self.plain(args.command(), &args.rendering).await
    }

    #[tool(
        name = "whyshelf_tag",
        description = "Add (`+tag`) and remove (`-tag`) tags of an entity."
    )]
    async fn tag(&self, Parameters(args): Parameters<TagArgs>) -> Result<String, String> {
        let command = parse("target", &args.target).and_then(|target| {
            Ok(Command::Tag {
                target,
                changes: parse_all::<TagChange>("changes", &args.changes)?,
            })
        });
        self.plain(command, &args.rendering).await
    }

    #[tool(
        name = "whyshelf_mv",
        description = "Record that a document's file moved. The identifier stays."
    )]
    async fn mv(&self, Parameters(args): Parameters<MvArgs>) -> Result<String, String> {
        let command = parse("target", &args.target).and_then(|target| {
            Ok(Command::Mv {
                target,
                new_path: parse("new_path", &args.new_path)?,
            })
        });
        self.plain(command, &args.rendering).await
    }

    #[tool(
        name = "whyshelf_publish",
        description = "Move a document from the private store to the repo store."
    )]
    async fn publish(&self, Parameters(args): Parameters<TargetArgs>) -> Result<String, String> {
        let command = parse("target", &args.target).map(|target| Command::Publish { target });
        self.plain(command, &args.rendering).await
    }

    #[tool(
        name = "whyshelf_finding_new",
        description = "Start a Finding from a template and register it."
    )]
    async fn finding_new(
        &self,
        Parameters(args): Parameters<FindingNewArgs>,
    ) -> Result<String, String> {
        let command = (|| {
            Ok(Command::FindingNew {
                finding_type: args.finding_type.into(),
                title: non_blank("title", &args.title)?,
                alias: args
                    .alias
                    .as_deref()
                    .map(|a| parse::<Alias>("alias", a))
                    .transpose()?,
            })
        })();
        self.plain(command, &args.rendering).await
    }

    #[tool(
        name = "whyshelf_spec_new",
        description = "Start a Spec that stands on a Finding, from a template, and register it."
    )]
    async fn spec_new(&self, Parameters(args): Parameters<SpecNewArgs>) -> Result<String, String> {
        let command = parse("alias", &args.alias).and_then(|alias| {
            Ok(Command::SpecNew {
                alias,
                from: parse("from", &args.from)?,
            })
        });
        self.plain(command, &args.rendering).await
    }

    #[tool(
        name = "whyshelf_design_new",
        description = "Start a Design for a Spec, from a template, and register it."
    )]
    async fn design_new(
        &self,
        Parameters(args): Parameters<DesignNewArgs>,
    ) -> Result<String, String> {
        let command = parse("alias", &args.alias).and_then(|alias| {
            Ok(Command::DesignNew {
                alias,
                spec: parse("spec", &args.spec)?,
            })
        });
        self.plain(command, &args.rendering).await
    }

    #[tool(
        name = "whyshelf_check",
        description = "Run a gate (cr, cs, ci, ca, cm). Without `approve` it is the machine check; with it, the approval is recorded when the check passes. `cm` always records."
    )]
    async fn check(&self, Parameters(args): Parameters<CheckArgs>) -> Result<String, String> {
        self.plain(args.command(), &args.rendering).await
    }

    #[tool(
        name = "whyshelf_revoke",
        description = "Withdraw an approval, with the reason."
    )]
    async fn revoke(&self, Parameters(args): Parameters<RevokeArgs>) -> Result<String, String> {
        let command = (|| {
            Ok(Command::Revoke {
                approval: parse::<EventId>("approval", &args.approval)?,
                reason: non_blank("reason", &args.reason)?,
                by: actor(args.by.as_deref())?,
            })
        })();
        self.plain(command, &args.rendering).await
    }

    #[tool(
        name = "whyshelf_get",
        description = "One entity of any kind, content and classification together. Takes an alias, an identifier, `private:<id>` or an item as `<design>#<n>`."
    )]
    async fn get(&self, Parameters(args): Parameters<TargetArgs>) -> Result<String, String> {
        let command = parse::<Target>("target", &args.target).map(|target| Command::Get { target });
        self.plain(command, &args.rendering).await
    }

    #[tool(
        name = "whyshelf_find",
        description = "Find entities by tags, kind, state, owner, relations and text, in one query. Paged by `limit` and `cursor`; `fields` picks columns."
    )]
    async fn find(&self, Parameters(args): Parameters<FindArgs>) -> Result<String, String> {
        let listing = &args.listing;
        let command = (|| {
            let cursor = listing
                .cursor
                .as_deref()
                .map(|c| parse::<Cursor>("cursor", c))
                .transpose()?;
            let names = listing.fields.clone().unwrap_or_default();
            Ok(Command::Find {
                query: query_of(std::slice::from_ref(&args.query))?,
                page: PageRequest {
                    limit: listing
                        .limit
                        .map_or_else(PageLimit::default, PageLimit::new),
                    cursor,
                    fields: parse_fields(&names)?,
                },
            })
        })();
        // The use case pages and picks the columns; nothing is cut again here.
        let fields = match &command {
            Ok(Command::Find { page, .. }) => page.fields.clone(),
            _ => BTreeSet::new(),
        };
        self.answer(command, listing.format, Window::default(), fields)
            .await
    }

    #[tool(
        name = "whyshelf_status",
        description = "The state of every entity and the next command for each. Paged by `limit` and `cursor`; `fields` picks columns."
    )]
    async fn status(&self, Parameters(args): Parameters<Listing>) -> Result<String, String> {
        self.listed(Ok(Command::Status), &args).await
    }

    #[tool(
        name = "whyshelf_trace",
        description = "The chain from Finding to Spec to Design to item to approved revision, around one entity."
    )]
    async fn trace(&self, Parameters(args): Parameters<TargetListArgs>) -> Result<String, String> {
        let command = parse("target", &args.target).map(|target| Command::Trace { target });
        self.listed(command, &args.listing).await
    }

    #[tool(
        name = "whyshelf_changelog",
        description = "The history of one entity, from its events."
    )]
    async fn changelog(
        &self,
        Parameters(args): Parameters<TargetListArgs>,
    ) -> Result<String, String> {
        let command = parse("target", &args.target).map(|target| Command::Changelog { target });
        self.listed(command, &args.listing).await
    }

    #[tool(
        name = "whyshelf_audit",
        description = "Who approved which version, at which revision, and when."
    )]
    async fn audit(&self, Parameters(args): Parameters<AuditArgs>) -> Result<String, String> {
        let command = args
            .query
            .as_deref()
            .filter(|q| !q.trim().is_empty())
            .map(|q| query_of(&[q.to_owned()]))
            .transpose()
            .map(|filter| Command::Audit { filter });
        self.listed(command, &args.listing).await
    }

    #[tool(
        name = "whyshelf_rebuild",
        description = "Rebuild the cache and the generated files (after a merge, say)."
    )]
    async fn rebuild(&self, Parameters(args): Parameters<NoArgs>) -> Result<String, String> {
        self.plain(Ok(Command::Rebuild), &args.rendering).await
    }
}

#[tool_handler(
    name = "whyshelf",
    instructions = "whyshelf grounds specs in findings: Finding -> Spec -> Design -> Impl, hash-pinned, with event-sourced approvals. Every answer starts with the context line; check it names the project you mean. Start with whyshelf_status."
)]
impl ServerHandler for WhyshelfServer {}

/// Serve the workspace's commands over standard input and output until the
/// client closes the connection or the process is interrupted. The workspace is
/// given back to the caller's care only in that it is dropped here, after its
/// task has stopped; closing what the adapters opened is the caller's job.
pub async fn serve<P: Ports + 'static>(workspace: Workspace<P>) -> Result<(), CliError> {
    let (handle, task) = WorkspaceActor::spawn(workspace);
    let running = WhyshelfServer::new(handle)
        .serve(rmcp::transport::stdio())
        .await;
    let outcome = match running {
        Ok(running) => {
            let token = running.cancellation_token();
            let waiting = running.waiting();
            tokio::pin!(waiting);
            let quit = tokio::select! {
                quit = &mut waiting => quit,
                _ = tokio::signal::ctrl_c() => {
                    token.cancel();
                    waiting.await
                }
            };
            quit.map(|_| ()).map_err(|e| CliError::Server(Box::new(e)))
        }
        Err(error) => Err(CliError::Server(Box::new(error))),
    };
    // The server (and with it the last handle) is gone: the task ends and gives
    // the workspace back.
    task.await.map_err(|e| CliError::Server(Box::new(e)))?;
    outcome
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use whyshelf_core::domain::approval::{CmResultKind, Gate as DomainGate};

    use super::*;

    /// The names a tool-input enum offers, as its JSON schema lists them.
    fn schema_names<T: JsonSchema>() -> BTreeSet<String> {
        let schema = serde_json::to_value(schemars::schema_for!(T)).expect("a schema");
        schema["oneOf"]
            .as_array()
            .expect("an enum of names")
            .iter()
            .map(|variant| variant["const"].as_str().expect("a name").to_owned())
            .collect()
    }

    #[test]
    fn a_context_that_could_not_be_read_is_unavailable_and_says_why() {
        let (head, failure) = head_of(Err("the context line could not be read: boom".to_owned()));
        assert_eq!(head, "[context: unavailable]");
        let failure = failure.expect("the reason is kept");
        assert!(failure.contains("boom"), "{failure}");
        let text = warnings_block(&[failure]);
        assert!(
            text.starts_with("\nwarnings:\n- the context line could not be read"),
            "{text}"
        );
        assert_eq!(warnings_block(&[]), "");
    }

    /// The tool server's input shapes are its own (they stay an anti-corruption
    /// layer), but the words are the domain's: the sets are the same.
    #[test]
    fn the_names_of_a_measurement_are_the_domains() {
        let domain: BTreeSet<String> = CmResultKind::ALL
            .iter()
            .map(|kind| kind.name().to_owned())
            .collect();
        assert_eq!(schema_names::<Measured>(), domain);
        for name in &domain {
            serde_json::from_value::<Measured>(serde_json::Value::String(name.clone()))
                .expect("the domain's name is read");
        }
    }

    #[test]
    fn the_names_of_a_kind_a_store_and_a_finding_type_are_the_domains() {
        let kinds: BTreeSet<String> = DocKind::names().map(str::to_owned).collect();
        assert_eq!(schema_names::<KindChoice>(), kinds);
        let stores: BTreeSet<String> = StoreKind::ALL
            .iter()
            .map(|store| store.as_str().to_owned())
            .collect();
        assert_eq!(schema_names::<StoreChoice>(), stores);
        let types: BTreeSet<String> = FindingType::ALL
            .iter()
            .map(|finding_type| finding_type.name().to_owned())
            .collect();
        assert_eq!(schema_names::<FindingTypeChoice>(), types);
        // Each name is read, and read as the domain value of that name.
        for name in &kinds {
            let choice: KindChoice = serde_json::from_value(name.clone().into()).expect("a kind");
            assert_eq!(DocKind::from(choice).name(), name);
        }
        for name in &stores {
            let choice: StoreChoice = serde_json::from_value(name.clone().into()).expect("a store");
            assert_eq!(StoreKind::from(choice).as_str(), name);
        }
        for name in &types {
            let choice: FindingTypeChoice =
                serde_json::from_value(name.clone().into()).expect("a type");
            assert_eq!(FindingType::from(choice).name(), name);
        }
    }

    #[test]
    fn the_names_of_the_gates_are_the_domains_in_lower_case() {
        let domain: BTreeSet<String> = DomainGate::ALL
            .iter()
            .map(|gate| gate.name().to_ascii_lowercase())
            .collect();
        assert_eq!(schema_names::<Gate>(), domain);
    }
}
