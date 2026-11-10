//! `whyshelf books ...`: the commands on a shelf of books, over
//! [`whyshelf_core::app::shelf`] and the adapters of `whyshelf_infra::shelf`.
//!
//! Every command answers in JSON.
//!
//! ```text
//!   books <cmd> --> Project (infra::shelf) --> ShelfApp (app::shelf) --> report
//!                                                                         |
//!                                         output::* (serde) -> stdout, exit status
//! ```
//!
//! `show --approved` and `diff` only read: they bring back the content an approval
//! saw (through Git, by the address the record kept) for a person to read, and
//! never put anything back in place; that is Git's work and a person's. The text of
//! a difference is made here, by [`textdiff`], from the two contents the use case
//! gives as values.
//!
//! The exit status:
//!
//! | exit | meaning |
//! |---|---|
//! | 0 | the answer was given |
//! | 1 | `books check` found something wrong, or a rule of the model refused the judgement (nothing was written) |
//! | 2 | the request is wrong (a reference that names nothing or several documents, a declaration that cannot be used, `show --approved` or `diff` of a document no approval stands about; for `new`, an unknown book, a key that is taken or malformed, a book of slugs with no `--key`, entries the book refuses, each listed in `problems`, or a file already at the path) |
//! | 3 | the run failed (a file could not be read or written) |
//!
//! `new` is the one command that writes a document. Its values arrive as text and
//! are given to the use case as text; the use case reads them by the book's
//! declaration, and the domain judges the document before anything is written.

// `ShelfError` and the refusals of the domain hold lists of problems; they are
// returned once, at the end of a command, so their size costs nothing.
#![allow(clippy::result_large_err)]

pub mod args;
pub mod output;
pub mod textdiff;

use std::io::Write;
use std::path::PathBuf;
use std::process::ExitCode;

use whyshelf_core::app::shelf::{
    ErrorClass, JudgeRequest, Judgement, NewDocRequest, ShelfApp, ShelfError,
};
use whyshelf_core::domain::actor::Actor;
use whyshelf_core::domain::nonempty::NonBlank;
use whyshelf_core::domain::shelf::schema::BookName;
use whyshelf_infra::shelf::Project;
use whyshelf_infra::shelf::system::parse_time;

pub use args::BooksCommand;
use args::Verb;

/// The environment variable that names who judges.
const ACTOR_VAR: &str = "WHYSHELF_ACTOR";

/// The exit status of a `books` command.
#[derive(Debug, Clone, Copy)]
enum Exit {
    Ok = 0,
    Stopped = 1,
    Request = 2,
    Failed = 3,
}

/// Run a `books` command on the project at `root`.
pub async fn run(root: PathBuf, command: BooksCommand) -> ExitCode {
    let project = Project::new(root, std::env::var(ACTOR_VAR).ok());
    let result = match command {
        BooksCommand::Schema => Ok(print(&output::schemas())),
        BooksCommand::Check => ShelfApp::new(project).books_check().await.map(|check| {
            print(&output::books_check(&check));
            if check.is_clean() {
                Exit::Ok
            } else {
                Exit::Stopped
            }
        }),
        BooksCommand::Status => ShelfApp::new(project)
            .status()
            .await
            .map(|r| print(&output::status(&r))),
        BooksCommand::Get { reference } => ShelfApp::new(project)
            .get(&reference)
            .await
            .map(|r| print(&output::get(&r))),
        BooksCommand::Trace { reference } => ShelfApp::new(project)
            .trace(&reference)
            .await
            .map(|r| print(&output::trace(&r))),
        BooksCommand::Stale => ShelfApp::new(project)
            .stale()
            .await
            .map(|r| print(&output::stale(&r))),
        // `--approved` is required by the arguments, so `show` has this one view.
        BooksCommand::Show { reference, .. } => ShelfApp::new(project)
            .show_approved(&reference)
            .await
            .map(|r| print(&output::approved(&r))),
        BooksCommand::Diff { reference } => ShelfApp::new(project)
            .diff(&reference)
            .await
            .map(|r| print(&output::diff(&r))),
        BooksCommand::New {
            book,
            title,
            key,
            entries,
            body,
        } => match new_request(&book, title, key, entries, body) {
            Ok(request) => ShelfApp::new(project)
                .new_doc(request)
                .await
                .map(|created| print(&output::created(&created))),
            Err(message) => {
                eprintln!("error: {message}");
                print(&output::usage(message));
                return ExitCode::from(Exit::Request as u8);
            }
        },
        BooksCommand::Judge(verb) => match judge_requests(verb) {
            Ok(requests) => ShelfApp::new(project)
                .judge_all(requests)
                .await
                .map(|r| print(&output::judged(&r))),
            Err(message) => {
                eprintln!("error: {message}");
                print(&output::usage(message));
                return ExitCode::from(Exit::Request as u8);
            }
        },
    };
    let exit = match result {
        Ok(exit) => exit,
        Err(error) => report(&error),
    };
    ExitCode::from(exit as u8)
}

/// The requests of a `judge` command, one per document named, or why the arguments
/// are not one.
fn judge_requests(verb: Verb) -> Result<Vec<JudgeRequest>, String> {
    let reason = |text: &Option<String>| {
        text.as_deref()
            .map(|t| NonBlank::new(t).map_err(|e| format!("--reason: {e}")))
            .transpose()
    };
    let (judgement, common) = match verb {
        Verb::Approve { common, until } => (
            Judgement::Approve {
                until: until
                    .map(|t| parse_time(&t).map_err(|e| format!("--until `{t}`: {e}")))
                    .transpose()?,
                reason: reason(&common.reason)?,
            },
            common,
        ),
        Verb::Deny { common } => (
            Judgement::Deny {
                reason: reason(&common.reason)?,
            },
            common,
        ),
        Verb::Deprecate { common, successor } => (
            Judgement::Deprecate {
                successor,
                reason: reason(&common.reason)?,
            },
            common,
        ),
        Verb::Retire { common } => (
            Judgement::Retire {
                reason: reason(&common.reason)?,
            },
            common,
        ),
        Verb::Revoke { common } => (
            Judgement::Revoke {
                reason: reason(&common.reason)?
                    .ok_or_else(|| "revoke needs --reason".to_owned())?,
            },
            common,
        ),
    };
    let actor = common
        .actor
        .map(|a| Actor::new(&a).map_err(|e| format!("--actor `{a}`: {e}")))
        .transpose()?;
    Ok(common
        .references
        .into_iter()
        .map(|reference| JudgeRequest {
            judgement: judgement.clone(),
            reference,
            actor: actor.clone(),
        })
        .collect())
}

/// The request of a `new` command, or why the arguments are not one. Every value
/// stays a text here: which becomes a list, a number or a truth value is decided by
/// the book's declaration, in the use case.
fn new_request(
    book: &str,
    title: String,
    key: Option<String>,
    entries: Vec<String>,
    body: Option<String>,
) -> Result<NewDocRequest, String> {
    let book = BookName::new(book).map_err(|e| format!("book `{book}`: {e}"))?;
    let mut pairs = vec![("title".to_owned(), title)];
    for entry in entries {
        let (name, value) = entry
            .split_once('=')
            .ok_or_else(|| format!("`{entry}` is not `<entry>=<value>`"))?;
        pairs.push((name.to_owned(), value.to_owned()));
    }
    Ok(NewDocRequest {
        book,
        key,
        entries: pairs,
        body: body.unwrap_or_default(),
    })
}

/// Print an answer as one JSON document on stdout.
fn print<T: serde::Serialize>(value: &T) -> Exit {
    let mut out = std::io::stdout().lock();
    let written = serde_json::to_writer_pretty(&mut out, value)
        .map_err(std::io::Error::from)
        .and_then(|()| writeln!(out));
    match written {
        Ok(()) => Exit::Ok,
        Err(error) => {
            eprintln!("error: cannot write the answer: {error}");
            Exit::Failed
        }
    }
}

/// Print an error as JSON on stdout and its chain on stderr, and give its exit.
fn report(error: &ShelfError) -> Exit {
    let mut chain = vec![error.to_string()];
    let mut source = std::error::Error::source(error);
    while let Some(cause) = source {
        chain.push(cause.to_string());
        source = cause.source();
    }
    eprintln!("error: {}", chain.join(": "));
    print(&output::error(error, chain));
    match error.class() {
        ErrorClass::Request => Exit::Request,
        ErrorClass::Refused => Exit::Stopped,
        ErrorClass::Failed => Exit::Failed,
    }
}
