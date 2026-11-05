//! The command line: the one description of the commands.
//!
//! Help, the list of commands and the validation of every argument come from
//! the types below (clap's derive), not from a second hand-kept table. Values
//! are parsed straight into the types of `whyshelf-core` (a reference, a tag
//! change, a path), so a malformed argument is refused by the parser with its
//! name, before any use case runs. Structured input is never pasted into the
//! command line: documents are files, and the commands name them.

use std::collections::BTreeSet;
use std::path::PathBuf;

use clap::{Args, Parser, Subcommand, ValueEnum};
use whyshelf_core::app::check::CmInput;
use whyshelf_core::app::query::{Cursor, PageLimit, PageRequest, Target};
use whyshelf_core::app::raw::RawRevision;
use whyshelf_core::app::register::AddOptions;
use whyshelf_core::domain::actor::Actor;
use whyshelf_core::domain::error::ValueError;
use whyshelf_core::domain::event::{DocKind, EventId, StoreKind};
use whyshelf_core::domain::finding::FindingType;
use whyshelf_core::domain::id::Alias;
use whyshelf_core::domain::location::RelPath;
use whyshelf_core::domain::nonempty::NonBlank;
use whyshelf_core::domain::query::Query;
use whyshelf_core::domain::reference::{ItemRef, Ref};
use whyshelf_core::domain::tag::{Tag, TagChange};

use crate::command::{CheckTarget, Command, parse_fields};
use crate::error::CliError;
use crate::render::Format;

/// Ground specs in findings: hash-pinned Finding -> Spec -> Design -> Impl with
/// event-sourced approvals.
#[derive(Debug, Parser)]
#[command(name = "whyshelf", version, about)]
pub struct Cli {
    /// The project root (default: the current directory).
    #[arg(long, global = true, value_name = "DIR")]
    pub root: Option<PathBuf>,
    /// How to write the result.
    #[arg(long, global = true, value_enum, default_value_t = Format::Table)]
    pub format: Format,
    /// What to do.
    #[command(subcommand)]
    pub command: Sub,
}

/// Every command.
#[derive(Debug, Subcommand)]
pub enum Sub {
    /// Write `.whyshelf.toml` (with a new `project_id`), create `.whyshelf/` and keep
    /// the cache out of Git.
    Init(InitArgs),
    /// Show the project, the worktree, the current revision and the stores.
    Context,
    /// Register files or directories of documents.
    Add(AddArgs),
    /// Add (`+tag`) and remove (`-tag`) tags of an entity.
    Tag(TagArgs),
    /// Record that a document's file moved; the identifier stays.
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
    #[command(subcommand)]
    Finding(FindingCommand),
    /// Start a Spec.
    #[command(subcommand)]
    Spec(SpecCommand),
    /// Start a Design.
    #[command(subcommand)]
    Design(DesignCommand),
    /// Run a gate: without `--approve` the machine check, with it the approval.
    #[command(subcommand)]
    Check(CheckCommand),
    /// Withdraw an approval.
    Revoke(RevokeArgs),
    /// Show one entity, its content and its classification.
    Get {
        /// An entity (alias, identifier, `private:<id>`) or an item (`<design>#<n>`).
        target: Target,
    },
    /// Find entities by tags, kind, state, owner, relations and text.
    Find(FindArgs),
    /// The state of every entity and the next command for each.
    Status,
    /// The chain from Finding to Spec to Design to item to approved revision.
    Trace {
        /// The entity.
        target: Ref,
    },
    /// The history of one entity.
    Changelog {
        /// The entity.
        target: Ref,
    },
    /// Who approved which version, and when.
    Audit {
        /// Narrow the approvals to the entities this query matches.
        query: Vec<String>,
    },
    /// Rebuild the cache and the generated files.
    Rebuild,
    /// Print a JSON Schema: of `map.json`, or of what `--format json` prints.
    #[command(subcommand)]
    Schema(SchemaCommand),
    /// Count the versions of the project's data and say whether this build reads
    /// them. It only looks: no file is written.
    Migrate,
    /// Serve the commands as tools over standard input and output.
    Mcp,
}

/// Arguments of `init`.
#[derive(Debug, Args)]
pub struct InitArgs {
    /// The project's name (default: the directory's name, when it is one).
    #[arg(long)]
    pub project: Option<String>,
    /// The command that runs the project's tests.
    #[arg(
        long,
        value_name = "COMMAND",
        required_unless_present = "print_project_id"
    )]
    pub test_command: Option<String>,
    /// Print the line `project_id = "..."` with a new id, and do nothing else: no file
    /// is read or written. For a settings file that lacks one, or one copied from a
    /// template (the copy carries the template's id).
    #[arg(long, conflicts_with_all = ["project", "test_command"])]
    pub print_project_id: bool,
}

/// Arguments of `add`.
#[derive(Debug, Args)]
pub struct AddArgs {
    /// The files or directories to register.
    #[arg(required = true)]
    paths: Vec<RelPath>,
    /// The kind of the document, when the file does not say.
    #[arg(long, value_parser = parse_doc_kind)]
    kind: Option<DocKind>,
    /// A tag to put on every document (repeatable).
    #[arg(long = "tag", value_name = "TAG")]
    tags: Vec<Tag>,
    /// The alias of the document (for one file).
    #[arg(long)]
    alias: Option<Alias>,
    /// Where the documents go (default: the project's setting).
    #[arg(long, value_enum)]
    store: Option<StoreArg>,
}

/// A store, as a choice on the command line.
#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum StoreArg {
    /// The store committed with the repository.
    Repo,
    /// The store outside the repository.
    Private,
}

impl From<StoreArg> for StoreKind {
    fn from(store: StoreArg) -> Self {
        match store {
            StoreArg::Repo => Self::Repo,
            StoreArg::Private => Self::Private,
        }
    }
}

/// Arguments of `tag`.
#[derive(Debug, Args)]
pub struct TagArgs {
    /// The entity.
    target: Ref,
    /// `+tag` to add, `-tag` to remove.
    #[arg(required = true, allow_hyphen_values = true, value_name = "CHANGE")]
    changes: Vec<TagChange>,
}

/// Starting a Finding.
#[derive(Debug, Subcommand)]
pub enum FindingCommand {
    /// Create a Finding file from a template and register it.
    New {
        /// The kind of Finding.
        #[arg(long = "type", value_parser = parse_finding_type)]
        finding_type: FindingType,
        /// Its title.
        #[arg(long, value_parser = parse_non_blank)]
        title: NonBlank,
        /// Its alias (default: `finding/` and the title as a slug).
        #[arg(long)]
        alias: Option<Alias>,
    },
}

/// Starting a Spec.
#[derive(Debug, Subcommand)]
pub enum SpecCommand {
    /// Create a Spec file from a template and register it.
    New {
        /// Its alias.
        alias: Alias,
        /// The Finding it stands on.
        #[arg(long)]
        from: Ref,
    },
}

/// Starting a Design.
#[derive(Debug, Subcommand)]
pub enum DesignCommand {
    /// Create a Design file from a template and register it.
    New {
        /// Its alias.
        alias: Alias,
        /// The Spec it is for.
        #[arg(long)]
        spec: Ref,
    },
}

/// The gates.
#[derive(Debug, Subcommand)]
pub enum CheckCommand {
    /// CR: a Finding is complete and trustworthy.
    Cr(GateArgs),
    /// CS: a Spec follows from its Findings and is bound by its pins.
    Cs(GateArgs),
    /// CI: a Design's approach and split into items.
    Ci(GateArgs),
    /// CA: an item, at one revision of the code.
    Ca(CaArgs),
    /// CM: record what a measurement showed.
    Cm(CmArgs),
}

/// Arguments of CR, CS and CI.
#[derive(Debug, Args)]
pub struct GateArgs {
    /// The entity to check.
    target: Ref,
    /// Record the approval when the check passes.
    #[arg(long)]
    approve: bool,
    /// The approver (default: `user.name` of the settings).
    #[arg(long, value_parser = parse_actor)]
    by: Option<Actor>,
}

/// Arguments of CA.
#[derive(Debug, Args)]
pub struct CaArgs {
    /// The item, as `<design>#<n>`.
    item: ItemRef,
    /// Record the approval when the check passes. The current revision is
    /// recorded; the working tree must be clean.
    #[arg(long)]
    approve: bool,
    /// Approve a past revision instead of the current one.
    #[arg(long, value_parser = parse_revision)]
    revision: Option<RawRevision>,
    /// The approver (default: `user.name` of the settings).
    #[arg(long, value_parser = parse_actor)]
    by: Option<Actor>,
}

/// Arguments of CM.
#[derive(Debug, Args)]
pub struct CmArgs {
    /// The Spec that was measured.
    spec: Ref,
    /// Who read the measurement (default: `user.name` of the settings).
    #[arg(long, value_parser = parse_actor)]
    by: Option<Actor>,
    /// What the measurement showed.
    #[command(subcommand)]
    result: CmResult,
}

/// What a measurement showed; a failure says what the follow-up needs.
#[derive(Debug, Subcommand)]
pub enum CmResult {
    /// The measurement met the hypotheses.
    Pass,
    /// A wiring or implementation gap: an item is missing from the Design.
    FailWiring {
        /// A hint at the missing item.
        #[arg(long, value_parser = parse_non_blank)]
        item_hint: NonBlank,
    },
    /// The hypothesis was wrong: a new Finding records the measurement.
    FailHyp {
        /// The `measured_data` Finding that records the measurement.
        #[arg(long)]
        finding: Ref,
    },
}

/// Arguments of `revoke`.
#[derive(Debug, Args)]
pub struct RevokeArgs {
    /// The `Approved` event to withdraw (the `event` of `audit`).
    approval: EventId,
    /// Why.
    #[arg(long, value_parser = parse_non_blank)]
    reason: NonBlank,
    /// Who withdraws it (default: `user.name` of the settings).
    #[arg(long, value_parser = parse_actor)]
    by: Option<Actor>,
}

/// Arguments of `find`.
#[derive(Debug, Args)]
pub struct FindArgs {
    /// Terms, all of which must match: `tag:area:auth kind:spec state:stale
    /// owner:alice grounds:<ref> upstream:<ref> orphan:true`, and text (quote a
    /// phrase inside the argument).
    query: Vec<String>,
    /// How many rows at most (default 50, at most 200).
    #[arg(long)]
    limit: Option<usize>,
    /// Continue after the last row of the previous page.
    #[arg(long)]
    cursor: Option<Cursor>,
    /// Only these columns: store, path, alias, title, tags, owner, state.
    #[arg(long, value_delimiter = ',')]
    fields: Vec<String>,
}

/// What `schema` can print.
#[derive(Debug, Subcommand)]
pub enum SchemaCommand {
    /// The schema of `.whyshelf/map.json`.
    Map,
    /// The schema of the answer of every command in `--format json`.
    Output,
}

/// What a command line asks for, split by what it needs.
#[derive(Debug)]
pub enum Action {
    /// `init`: no workspace yet.
    Init(InitArgs),
    /// `schema map`: needs nothing.
    SchemaMap,
    /// `schema output`: needs nothing.
    SchemaOutput,
    /// `migrate`: reads the data itself, and opens no workspace.
    Migrate,
    /// `mcp`: serve the commands as tools.
    Mcp,
    /// Everything else: a command on the workspace.
    Run(Command),
}

fn parse_non_blank(text: &str) -> Result<NonBlank, ValueError> {
    NonBlank::new(text)
}

fn parse_actor(text: &str) -> Result<Actor, ValueError> {
    Actor::new(text)
}

fn parse_revision(text: &str) -> Result<RawRevision, ValueError> {
    RawRevision::new(text)
}

fn parse_doc_kind(text: &str) -> Result<DocKind, String> {
    DocKind::from_name(text)
        .ok_or_else(|| format!("one of {}", DocKind::names().collect::<Vec<_>>().join(", ")))
}

fn parse_finding_type(text: &str) -> Result<FindingType, String> {
    FindingType::from_name(text).ok_or_else(|| {
        let names: Vec<&str> = FindingType::ALL.iter().map(|t| t.name()).collect();
        format!("one of {}", names.join(", "))
    })
}

/// The query text a list of words makes: the words, side by side.
pub fn query_of(words: &[String]) -> Result<Query, CliError> {
    Ok(Query::parse(&words.join(" "))?)
}

impl AddArgs {
    fn into_command(self) -> Command {
        let mut options =
            AddOptions::new().with_tags(self.tags.into_iter().collect::<BTreeSet<_>>());
        if let Some(kind) = self.kind {
            options = options.with_kind(kind);
        }
        if let Some(alias) = self.alias {
            options = options.with_alias(alias);
        }
        if let Some(store) = self.store {
            options = options.with_store(store.into());
        }
        Command::Add {
            paths: self.paths,
            options,
        }
    }
}

impl CheckCommand {
    fn into_command(self) -> Command {
        let gate = |target, approve, by| Command::Check {
            target,
            approve,
            by,
        };
        match self {
            Self::Cr(a) => gate(CheckTarget::Cr(a.target), a.approve, a.by),
            Self::Cs(a) => gate(CheckTarget::Cs(a.target), a.approve, a.by),
            Self::Ci(a) => gate(CheckTarget::Ci(a.target), a.approve, a.by),
            Self::Ca(a) => gate(
                CheckTarget::Ca {
                    item: a.item,
                    revision: a.revision,
                },
                a.approve,
                a.by,
            ),
            Self::Cm(a) => {
                let result = match a.result {
                    CmResult::Pass => CmInput::Pass,
                    CmResult::FailWiring { item_hint } => CmInput::FailWiring { item_hint },
                    CmResult::FailHyp { finding } => CmInput::FailHyp { finding },
                };
                gate(
                    CheckTarget::Cm {
                        spec: a.spec,
                        result,
                    },
                    true,
                    a.by,
                )
            }
        }
    }
}

impl Sub {
    /// Split a parsed command line into what it needs, parsing the query texts.
    pub fn into_action(self) -> Result<Action, CliError> {
        let command = match self {
            Self::Init(args) => return Ok(Action::Init(args)),
            Self::Schema(SchemaCommand::Map) => return Ok(Action::SchemaMap),
            Self::Schema(SchemaCommand::Output) => return Ok(Action::SchemaOutput),
            Self::Migrate => return Ok(Action::Migrate),
            Self::Mcp => return Ok(Action::Mcp),
            Self::Context => Command::Context,
            Self::Add(args) => args.into_command(),
            Self::Tag(args) => Command::Tag {
                target: args.target,
                changes: args.changes,
            },
            Self::Mv { target, new_path } => Command::Mv { target, new_path },
            Self::Publish { target } => Command::Publish { target },
            Self::Finding(FindingCommand::New {
                finding_type,
                title,
                alias,
            }) => Command::FindingNew {
                finding_type,
                title,
                alias,
            },
            Self::Spec(SpecCommand::New { alias, from }) => Command::SpecNew { alias, from },
            Self::Design(DesignCommand::New { alias, spec }) => Command::DesignNew { alias, spec },
            Self::Check(check) => check.into_command(),
            Self::Revoke(args) => Command::Revoke {
                approval: args.approval,
                reason: args.reason,
                by: args.by,
            },
            Self::Get { target } => Command::Get { target },
            Self::Find(args) => Command::Find {
                query: query_of(&args.query)?,
                page: PageRequest {
                    limit: args.limit.map_or_else(PageLimit::default, PageLimit::new),
                    cursor: args.cursor,
                    fields: parse_fields(&args.fields)?,
                },
            },
            Self::Status => Command::Status,
            Self::Trace { target } => Command::Trace { target },
            Self::Changelog { target } => Command::Changelog { target },
            Self::Audit { query } => Command::Audit {
                filter: if query.is_empty() {
                    None
                } else {
                    Some(query_of(&query)?)
                },
            },
            Self::Rebuild => Command::Rebuild,
        };
        Ok(Action::Run(command))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::CommandFactory;

    #[test]
    fn the_definition_is_consistent() {
        Cli::command().debug_assert();
    }

    fn parse(args: &[&str]) -> Cli {
        Cli::try_parse_from(std::iter::once("whyshelf").chain(args.iter().copied())).unwrap()
    }

    #[test]
    fn tag_changes_may_start_with_a_hyphen() {
        let cli = parse(&["tag", "finding/a", "+area:auth", "-wip"]);
        let Ok(Action::Run(Command::Tag { changes, .. })) = cli.command.into_action() else {
            panic!("not a tag command");
        };
        assert_eq!(changes.len(), 2);
        assert!(matches!(changes[1], TagChange::Remove(_)));
    }

    #[test]
    fn a_cm_failure_names_what_the_follow_up_needs() {
        assert!(
            Cli::try_parse_from(["whyshelf", "check", "cm", "spec/a", "fail-wiring"]).is_err(),
            "fail-wiring without --item-hint"
        );
        assert!(
            Cli::try_parse_from(["whyshelf", "check", "cm", "spec/a", "fail-hyp"]).is_err(),
            "fail-hyp without --finding"
        );
        let cli = parse(&[
            "check",
            "cm",
            "spec/a",
            "fail-wiring",
            "--item-hint",
            "the cache is not read",
        ]);
        assert!(matches!(
            cli.command.into_action(),
            Ok(Action::Run(Command::Check {
                target: CheckTarget::Cm {
                    result: CmInput::FailWiring { .. },
                    ..
                },
                ..
            }))
        ));
    }

    /// The command line keeps its own input shape for a measurement (a failure
    /// carries its flag, as a subcommand), but the words are the domain's.
    #[test]
    fn the_subcommands_of_a_measurement_are_the_domains_names() {
        use std::collections::BTreeSet;

        use whyshelf_core::domain::approval::CmResultKind;

        let command = <CmResult as Subcommand>::augment_subcommands(clap::Command::new("cm"));
        let offered: BTreeSet<&str> = command
            .get_subcommands()
            .map(clap::Command::get_name)
            .collect();
        let domain: BTreeSet<&str> = CmResultKind::ALL.iter().map(|kind| kind.name()).collect();
        assert_eq!(offered, domain);
    }

    /// The command line keeps its own choice of stores (clap reads the variants),
    /// but the words are the domain's, and each variant is the store of its name.
    #[test]
    fn the_stores_offered_are_the_domains_names() {
        use std::collections::BTreeSet;

        use clap::ValueEnum;

        let offered: BTreeSet<String> = StoreArg::value_variants()
            .iter()
            .map(|store| {
                let name = store
                    .to_possible_value()
                    .expect("a value")
                    .get_name()
                    .to_owned();
                assert_eq!(StoreKind::from(*store).as_str(), name);
                name
            })
            .collect();
        let domain: BTreeSet<String> = StoreKind::ALL
            .iter()
            .map(|store| store.as_str().to_owned())
            .collect();
        assert_eq!(offered, domain);
    }

    #[test]
    fn the_format_is_a_choice() {
        assert_eq!(parse(&["status", "--format", "json"]).format, Format::Json);
        assert!(Cli::try_parse_from(["whyshelf", "status", "--format", "xml"]).is_err());
    }
}
