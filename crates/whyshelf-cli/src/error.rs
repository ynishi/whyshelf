//! The errors of the command line, and how they are shown.
//!
//! Every failure of a command ends up as a [`CliError`], which keeps the cause of
//! each (the application's error, an adapter's, the operating system's) as a
//! source rather than flattening it into text. [`CliError::report`] is the only
//! place that turns one into the lines a person reads: the message, then each
//! cause, then (for a refused approval) every violation the machine check found.

use std::error::Error;
use std::fmt::Write;
use std::path::PathBuf;

use whyshelf_core::app::AppError;
use whyshelf_core::domain::error::{QueryError, ValueError};
use whyshelf_infra::OpenError;
use whyshelf_infra::cache_sqlite::CacheError;

use crate::render;

/// Why a command failed.
#[derive(Debug, thiserror::Error)]
pub enum CliError {
    /// An argument is not a valid value of its type.
    #[error("invalid value for `{name}`")]
    Argument {
        /// The argument's name.
        name: &'static str,
        /// Why the value is refused.
        #[source]
        source: Box<dyn Error + Send + Sync>,
    },
    /// A value of a domain type is refused by its rule.
    #[error(transparent)]
    Value(#[from] ValueError),
    /// A query text does not parse.
    #[error("the query does not parse")]
    Query(#[from] QueryError),
    /// `--fields` names a column that does not exist.
    #[error("`{0}` is not a field; the fields are store, path, alias, title, tags, owner, state")]
    UnknownField(String),
    /// `fields` names a column the result does not have.
    #[error("`{name}` is not a column of this result; the columns are {columns}")]
    UnknownColumn {
        /// The name asked for.
        name: String,
        /// The columns there are.
        columns: String,
    },
    /// A use case failed.
    #[error(transparent)]
    App(#[from] AppError),
    /// The workspace could not be opened.
    #[error("cannot open the workspace")]
    Open(#[from] OpenError),
    /// The cache could not be closed.
    #[error("cannot close the cache")]
    Close(#[source] CacheError),
    /// `init` found the settings file already there.
    #[error("{path} exists already; `init` never overwrites a settings file")]
    SettingsExist {
        /// The settings file.
        path: PathBuf,
    },
    /// `init` has no valid project name: the directory's name is not one.
    #[error("the directory name `{dir}` is not a project name; pass `--project`")]
    NoProjectName {
        /// The directory's name.
        dir: String,
    },
    /// A file or directory could not be read or written.
    #[error("cannot {action} {path}")]
    Io {
        /// What was being done.
        action: &'static str,
        /// The path.
        path: PathBuf,
        /// The operating system's error.
        #[source]
        source: std::io::Error,
    },
    /// The result could not be turned into JSON.
    #[error("cannot render the result as JSON")]
    Json(#[from] serde_json::Error),
    /// The tool server failed.
    #[error("the tool server failed")]
    Server(#[source] Box<dyn Error + Send + Sync>),
    /// The task that owns the workspace is gone.
    #[error("the workspace task has stopped")]
    ActorStopped,
}

impl CliError {
    /// An argument that is refused, with the reason kept.
    pub fn argument<E>(name: &'static str, source: E) -> Self
    where
        E: Error + Send + Sync + 'static,
    {
        Self::Argument {
            name,
            source: Box::new(source),
        }
    }

    /// The message, each cause on a line of its own, and the violations of a
    /// refused approval.
    pub fn report(&self) -> String {
        let mut out = format!("error: {self}");
        out.push_str(&chain(self));
        if let Self::App(AppError::CheckFailed { violations, .. }) = self {
            for violation in violations {
                let _ = write!(out, "\n  - {}", render::violation_text(violation));
            }
        }
        out
    }
}

/// The causes of an error, one per line, indented.
pub fn chain(error: &dyn Error) -> String {
    let mut out = String::new();
    let mut source = error.source();
    while let Some(cause) = source {
        let _ = write!(out, "\n  caused by: {cause}");
        source = cause.source();
    }
    out
}
