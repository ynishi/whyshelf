//! From a parsed command line to an exit code.
//!
//! This is the only place that opens the workspace, prints, and decides the exit
//! status. The workspace is opened by `whyshelf_infra::open`, handed to one
//! command, and **always** closed afterwards, on the way out of a failure as well
//! as of a success: the cache's thread is closed on purpose, never in `Drop`.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::Parser;
use whyshelf_core::app::Workspace;
use whyshelf_core::app::query::Field;
use whyshelf_infra::InfraPorts;
use whyshelf_infra::config::SUPPORTED_VERSION;
use whyshelf_infra::map_dto::schema_json;

use crate::args::{Action, Cli};
use crate::command::{Command, Outcome, execute};
use crate::error::CliError;
use crate::init::init;
use crate::mcp;
use crate::render::{Format, Window, document};

/// Parse the command line, run it, and say how it went.
pub async fn main() -> ExitCode {
    let cli = Cli::parse();
    match run(cli).await {
        Ok(code) => code,
        Err(error) => {
            eprintln!("{}", error.report());
            ExitCode::FAILURE
        }
    }
}

async fn run(cli: Cli) -> Result<ExitCode, CliError> {
    let root = cli.root.unwrap_or_else(|| PathBuf::from("."));
    let format = cli.format;
    match cli.command.into_action()? {
        Action::Init(args) => {
            for line in init(&root, &args).await? {
                println!("{line}");
            }
            Ok(ExitCode::SUCCESS)
        }
        Action::SchemaMap => {
            println!("{}", schema_json()?);
            Ok(ExitCode::SUCCESS)
        }
        Action::Migrate => {
            println!(
                "the files are in the newest format (version {SUPPORTED_VERSION}); nothing to do"
            );
            Ok(ExitCode::SUCCESS)
        }
        Action::Mcp => {
            with_workspace(&root, |workspace| async move {
                mcp::serve(workspace).await?;
                Ok(ExitCode::SUCCESS)
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

/// Open the workspace of the project at `root`, give it to `work`, and close
/// what was opened whatever `work` returns. A failure to close is reported
/// unless `work` failed too, in which case that failure is the one returned and
/// the failure to close is printed beside it.
async fn with_workspace<F, Fut>(root: &Path, work: F) -> Result<ExitCode, CliError>
where
    F: FnOnce(Workspace<InfraPorts>) -> Fut,
    Fut: Future<Output = Result<ExitCode, CliError>>,
{
    let (workspace, shutdown) = whyshelf_infra::open(root).await?;
    let result = work(workspace).await;
    let closed = shutdown.close().await.map_err(CliError::Close);
    match (result, closed) {
        (Ok(code), Ok(())) => Ok(code),
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
) -> Result<ExitCode, CliError> {
    let fields: BTreeSet<Field> = match &command {
        Command::Find { page, .. } => page.fields.clone(),
        _ => BTreeSet::new(),
    };
    let done = execute(workspace, command).await?;
    for warning in &done.warnings {
        eprintln!("warning: {warning}");
    }
    let doc = document(&done.outcome, &fields).window(&Window::default())?;
    println!("{}", doc.render(format)?.trim_end());
    // A machine check that found something is not an error of the tool, but the
    // exit status has to tell a script.
    Ok(match &done.outcome {
        Outcome::Checked(report) if !report.passed() => ExitCode::FAILURE,
        _ => ExitCode::SUCCESS,
    })
}
