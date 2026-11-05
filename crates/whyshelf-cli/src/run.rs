//! From a parsed command line to an exit code.
//!
//! This is the only place that opens the workspace, prints, and decides the exit
//! status. The workspace is opened by `whyshelf_infra::open`, handed to one
//! command, and **always** closed afterwards, on the way out of a failure as well
//! as of a success: the cache's thread is closed on purpose, never in `Drop`.
//!
//! # The exit status
//!
//! [`ExitStatus::of`] is the one place that says what a run came to. It reads the
//! whole `Result<Outcome, CliError>`, because a check that found something is not an
//! error of the tool: it is an `Ok(Outcome::Checked)` whose report has violations.
//!
//! | status | means | who acts, and on what |
//! |---|---|---|
//! | 0 | success | nothing |
//! | 1 | a check or a gate stopped it | the user, on a file, or the build of whyshelf in use |
//! | 2 | the request is wrong | the user, on the command line, the query or the settings, or by making the first commit when the repository has none (`AppError::NoCommit`) |
//! | 3 | the run failed | whoever runs it, on the machine: git, the disk, the cache, a test run |
//!
//! The arms of the `match`es below name every variant, with no catch-all, so a new
//! error that is added without a place in this table does not build.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::Parser;
use whyshelf_core::app::AppError;
use whyshelf_core::app::Workspace;
use whyshelf_core::app::query::Field;
use whyshelf_infra::config::UserDirs;
use whyshelf_infra::format::{Verdict, census};
use whyshelf_infra::map_dto::schema_json;
use whyshelf_infra::{InfraPorts, OpenError};

use crate::args::{Action, Cli};
use crate::command::{Command, Outcome, execute};
use crate::error::CliError;
use crate::init::init;
use crate::mcp;
use crate::render::{Format, Window, document};

/// What a run came to, as the status a script reads.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExitStatus {
    /// 0: it worked, and nothing was found.
    Success,
    /// 1: a check or a gate stopped it, and a person can clear what it found (a file,
    /// or the build of whyshelf in use).
    Refused,
    /// 2: the request is wrong: the arguments, the query, the fields, the settings.
    Usage,
    /// 3: the run failed: Git, the disk, the cache, a test run, the data.
    Failed,
}

impl ExitStatus {
    /// The number the process exits with.
    pub fn code(self) -> u8 {
        match self {
            Self::Success => 0,
            Self::Refused => 1,
            Self::Usage => 2,
            Self::Failed => 3,
        }
    }

    /// What a run came to.
    pub fn of(result: &Result<Outcome, CliError>) -> Self {
        match result {
            Ok(Outcome::Checked(report)) if !report.passed() => Self::Refused,
            Ok(Outcome::Migrate(found)) => match found.verdict() {
                Verdict::Readable => Self::Success,
                Verdict::Unreadable(_) | Verdict::Incomplete { .. } => Self::Refused,
            },
            Ok(
                Outcome::Checked(_)
                | Outcome::Context(_)
                | Outcome::Added(_)
                | Outcome::Recorded(_)
                | Outcome::Scaffolded(_)
                | Outcome::Approved { .. }
                | Outcome::Got(_)
                | Outcome::Found(_)
                | Outcome::Status(_)
                | Outcome::Trace(_)
                | Outcome::Changelog(_)
                | Outcome::Audit(_)
                | Outcome::Rebuilt { .. }
                | Outcome::Printed(_),
            ) => Self::Success,
            Err(error) => Self::of_error(error),
        }
    }

    fn of_error(error: &CliError) -> Self {
        match error {
            CliError::Argument { .. }
            | CliError::Value(_)
            | CliError::Query(_)
            | CliError::UnknownField(_)
            | CliError::UnknownColumn { .. }
            | CliError::SettingsExist { .. }
            | CliError::NoProjectName { .. } => Self::Usage,
            CliError::App(error) => Self::of_app(error),
            CliError::Open(error) => match error {
                // A settings file that is wrong, or a root that is not there, is the
                // user's to fix.
                OpenError::Config(_) | OpenError::Root { .. } => Self::Usage,
                OpenError::Git(_) | OpenError::Cache(_) | OpenError::Secret(_) => Self::Failed,
            },
            CliError::Format(error) => match error {
                whyshelf_infra::format::FormatError::NotAProject { .. } => Self::Usage,
                whyshelf_infra::format::FormatError::Io { .. } => Self::Failed,
            },
            CliError::Close(_)
            | CliError::Io { .. }
            | CliError::Json(_)
            | CliError::Server(_)
            | CliError::ActorStopped => Self::Failed,
        }
    }

    fn of_app(error: &AppError) -> Self {
        match error {
            // A check or a gate stopped it; what is wrong is in a file, or in the
            // state a person can set right. A conflict in the store is one: the
            // classification is set again to settle it.
            AppError::CheckFailed { .. }
            | AppError::TargetFileMissing { .. }
            | AppError::DirtyWorktree
            | AppError::SecretFound { .. }
            | AppError::LeakFound { .. }
            | AppError::ClassificationConflict { .. }
            | AppError::ObjectPurged { .. } => Self::Refused,
            // The request names something that is not there, is not one thing, is of
            // another kind, or is refused as written.
            AppError::Value(_)
            | AppError::Entity(_)
            | AppError::Query(_)
            | AppError::ItemRef(_)
            | AppError::Vocabulary(_)
            | AppError::NotFound { .. }
            | AppError::ItemNotFound { .. }
            | AppError::Ambiguous { .. }
            | AppError::WrongKind { .. }
            | AppError::NotLocal { .. }
            | AppError::Unresolved { .. }
            | AppError::RevisionRejected { .. }
            | AppError::NoActor
            | AppError::AliasTaken { .. }
            | AppError::AliasForSeveralFiles { .. }
            | AppError::AlreadyRegistered { .. }
            | AppError::PathOccupied { .. }
            | AppError::FileMissing { .. }
            | AppError::KindMismatch { .. }
            | AppError::KindUnknown { .. }
            | AppError::NotADocument { .. }
            | AppError::NotAnApproval { .. }
            | AppError::AlreadyRevoked { .. }
            | AppError::AlreadyPublished { .. }
            | AppError::NotAVersion { .. }
            | AppError::NoCommit
            | AppError::PathExists { .. }
            | AppError::NoSlug => Self::Usage,
            // A port failed (a store that cannot be read, Git, the cache, a test run),
            // or the model broke a rule of its own. A command cannot run on data it
            // cannot read, which includes a file of a version this build does not read.
            AppError::Port { .. } | AppError::Event(_) | AppError::ObjectMissing { .. } => {
                Self::Failed
            }
        }
    }
}

/// Parse the command line, run it, and say how it went.
pub async fn main() -> ExitCode {
    let cli = Cli::parse();
    let result = run(cli).await;
    if let Err(error) = &result {
        eprintln!("{}", error.report());
    }
    ExitCode::from(ExitStatus::of(&result).code())
}

async fn run(cli: Cli) -> Result<Outcome, CliError> {
    let root = cli.root.unwrap_or_else(|| PathBuf::from("."));
    let format = cli.format;
    match cli.command.into_action()? {
        Action::Init(args) => {
            let lines = init(&root, &args).await?;
            print_lines(&lines);
            Ok(Outcome::Printed(lines))
        }
        Action::SchemaMap => {
            let lines = vec![schema_json()?];
            print_lines(&lines);
            Ok(Outcome::Printed(lines))
        }
        Action::Migrate => {
            // The data is read as it is, without a workspace: a check that the data
            // can be read has to work when it cannot.
            let found = census(&root, &UserDirs::from_env()).await?;
            let outcome = Outcome::Migrate(found);
            print_document(&outcome, &BTreeSet::new(), format)?;
            Ok(outcome)
        }
        Action::Mcp => {
            with_workspace(&root, |workspace| async move {
                mcp::serve(workspace).await?;
                Ok(Outcome::Printed(Vec::new()))
            })
            .await
        }
        Action::Run(command) => {
            with_workspace(&root, |mut workspace| async move {
                run_command(&mut workspace, command, format).await
            })
            .await
        }
    }
}

fn print_lines(lines: &[String]) {
    for line in lines {
        println!("{line}");
    }
}

fn print_document(
    outcome: &Outcome,
    fields: &BTreeSet<Field>,
    format: Format,
) -> Result<(), CliError> {
    let doc = document(outcome, fields).window(&Window::default())?;
    println!("{}", doc.render(format)?.trim_end());
    Ok(())
}

/// Open the workspace of the project at `root`, give it to `work`, and close
/// what was opened whatever `work` returns. A failure to close is reported
/// unless `work` failed too, in which case that failure is the one returned and
/// the failure to close is printed beside it.
async fn with_workspace<F, Fut>(root: &Path, work: F) -> Result<Outcome, CliError>
where
    F: FnOnce(Workspace<InfraPorts>) -> Fut,
    Fut: Future<Output = Result<Outcome, CliError>>,
{
    let (workspace, shutdown) = whyshelf_infra::open(root).await?;
    let result = work(workspace).await;
    let closed = shutdown.close().await.map_err(CliError::Close);
    match (result, closed) {
        (Ok(outcome), Ok(())) => Ok(outcome),
        (Ok(_), Err(error)) | (Err(error), Ok(())) => Err(error),
        (Err(error), Err(closing)) => {
            eprintln!("{}", closing.report());
            Err(error)
        }
    }
}

async fn run_command(
    workspace: &mut Workspace<InfraPorts>,
    command: Command,
    format: Format,
) -> Result<Outcome, CliError> {
    let fields: BTreeSet<Field> = match &command {
        Command::Find { page, .. } => page.fields.clone(),
        _ => BTreeSet::new(),
    };
    let done = execute(workspace, command).await?;
    for warning in &done.warnings {
        eprintln!("warning: {warning}");
    }
    let mut doc = document(&done.outcome, &fields).window(&Window::default())?;
    // The context line is the first thing a status says: where the tool is pointed.
    // The tool server opens every answer with it already, and the files the status
    // exports leave it out, since a worktree's path does not belong in a committed file.
    if let Outcome::Status(report) = &done.outcome {
        doc = doc.with_context(report.context());
    }
    println!("{}", doc.render(format)?.trim_end());
    Ok(done.outcome)
}

#[cfg(test)]
mod tests {
    use whyshelf_core::app::check::CheckReport;
    use whyshelf_core::app::ports::PortName;
    use whyshelf_core::domain::approval::Gate;
    use whyshelf_core::domain::lint::CheckViolation;
    use whyshelf_core::domain::reference::Ref;

    use super::*;

    fn status(result: Result<Outcome, CliError>) -> u8 {
        ExitStatus::of(&result).code()
    }

    fn app(error: AppError) -> Result<Outcome, CliError> {
        Err(CliError::App(error))
    }

    fn reference() -> Ref {
        "finding/a".parse().expect("a reference")
    }

    fn id() -> whyshelf_core::domain::id::EntityId {
        "01912345-6789-7abc-8def-0123456789ab".parse().unwrap()
    }

    #[test]
    fn a_check_that_found_something_is_a_refusal_and_one_that_found_nothing_is_a_success() {
        let report = |violations| CheckReport::new(Gate::Cs, id(), violations);
        assert_eq!(status(Ok(Outcome::Checked(report(Vec::new())))), 0);
        let found = report(vec![CheckViolation::EmptyBody]);
        assert_eq!(status(Ok(Outcome::Checked(found))), 1);
        assert_eq!(status(Ok(Outcome::Printed(vec!["x".to_owned()]))), 0);
    }

    #[test]
    fn what_stops_at_a_gate_or_a_check_is_one_and_what_is_wrong_with_the_request_is_two() {
        let failed = AppError::CheckFailed {
            gate: Gate::Ca,
            violations: vec![CheckViolation::TestsFailed],
        };
        assert_eq!(status(app(failed)), 1);
        assert_eq!(status(app(AppError::DirtyWorktree)), 1);
        let conflict = AppError::ClassificationConflict {
            id: id(),
            field: whyshelf_core::domain::classification::ConflictField::Kind,
        };
        assert_eq!(status(app(conflict)), 1);
        assert_eq!(
            status(app(AppError::NotFound {
                reference: reference()
            })),
            2
        );
        assert_eq!(status(app(AppError::NoActor)), 2);
        assert_eq!(status(Err(CliError::UnknownField("colour".to_owned()))), 2);
        let query = whyshelf_core::domain::error::QueryError::UnterminatedQuote;
        assert_eq!(status(Err(CliError::Query(query))), 2);
    }

    #[test]
    fn a_port_that_failed_is_three_and_so_is_a_file_of_a_version_this_build_does_not_read() {
        let failure = whyshelf_infra::wire::WireError::UnsupportedVersion { found: Some(2) };
        let unsupported = AppError::Port {
            port: PortName::Store,
            source: Box::new(failure),
        };
        assert_eq!(status(app(unsupported)), 3);
        assert_eq!(status(Err(CliError::ActorStopped)), 3);
    }

    #[test]
    fn a_settings_file_that_is_wrong_is_two_and_a_directory_that_is_no_project_is_two() {
        let no_id = whyshelf_infra::config::ConfigError::MissingProjectId {
            suggested: whyshelf_infra::config::new_project_id(),
        };
        assert_eq!(status(Err(CliError::Open(OpenError::Config(no_id)))), 2);
        let none = whyshelf_infra::format::FormatError::NotAProject {
            root: PathBuf::from("/nowhere"),
        };
        assert_eq!(status(Err(CliError::Format(none))), 2);
    }

    /// A project directory with these settings and these event files.
    async fn project(settings: &str, events: &[&str]) -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        tokio::fs::write(dir.path().join(".whyshelf.toml"), settings)
            .await
            .unwrap();
        tokio::fs::create_dir_all(dir.path().join(".whyshelf/events"))
            .await
            .unwrap();
        for (index, text) in events.iter().enumerate() {
            tokio::fs::write(
                dir.path().join(format!(".whyshelf/events/{index}.json")),
                text,
            )
            .await
            .unwrap();
        }
        dir
    }

    const SETTINGS: &str = "version = 1\nproject = \"demo\"\nproject_id = \"0192f3a4-5b6c-7d8e-9fa0-b1c2d3e4f506\"\n[test]\ncommand = \"x\"\n";

    async fn migrate_status(dir: &tempfile::TempDir) -> u8 {
        let dirs = UserDirs::default().with_data_dir(dir.path().join("data"));
        let found = census(dir.path(), &dirs).await.unwrap();
        status(Ok(Outcome::Migrate(found)))
    }

    #[tokio::test]
    async fn migrate_is_zero_when_readable_and_one_when_not_or_not_all_was_read() {
        let readable = project(SETTINGS, &["{\"v\": 1}"]).await;
        assert_eq!(migrate_status(&readable).await, 0);
        let unreadable = project(SETTINGS, &["{\"v\": 1}", "{\"v\": 2}"]).await;
        assert_eq!(migrate_status(&unreadable).await, 1);
        let no_id = SETTINGS.replace(
            "project_id = \"0192f3a4-5b6c-7d8e-9fa0-b1c2d3e4f506\"\n",
            "",
        );
        let incomplete = project(&no_id, &["{\"v\": 1}"]).await;
        assert_eq!(migrate_status(&incomplete).await, 1);
        let version_two = project(&SETTINGS.replace("version = 1", "version = 2"), &[]).await;
        assert_eq!(migrate_status(&version_two).await, 1);
    }
}
