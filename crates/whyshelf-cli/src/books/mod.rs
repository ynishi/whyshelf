//! `whyshelf ...`: the commands on a shelf of books, over
//! [`whyshelf_core::app::shelf`] and the adapters of `whyshelf_infra::shelf`.
//!
//! Every command answers in JSON.
//!
//! ```text
//!   whyshelf <cmd> --> Project (infra::shelf) --> ShelfApp (app::shelf) --> report
//!                                                                            |
//!                                            output::* (serde) -> stdout, exit status
//! ```
//!
//! `get --approved` and `diff` only read: they bring back the content an approval
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
//! | 1 | `check` found something wrong, `stale --exit-code` found an approval that does not hold, or a rule of the model refused the judgement (nothing was written) |
//! | 2 | the request is wrong (a reference that names nothing or several documents, a declaration that cannot be used, `get --approved` or `diff` of a document no approval stands about; for `new`, an unknown book, a key that is taken or malformed, a book of slugs with no `--key`, entries the book refuses, each listed in `problems`, or a file already at the path; for `judge same`, a path no document links, a `--from` hash nothing pinned, several earlier hashes and none named, or nothing to declare; for `judge withdraw`, an id that is no decision or declaration, or one already withdrawn) |
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

#[cfg(test)]
mod exit_tests;
#[cfg(test)]
mod new_tests;
#[cfg(test)]
mod same_meaning_tests;
#[cfg(test)]
mod stale_tests;

use std::io::Write;
use std::path::PathBuf;
use std::process::ExitCode;

use whyshelf_core::app::shelf::{
    ErrorClass, JudgeRequest, Judgement, NewDocRequest, SameMeaningAbout, SameMeaningRequest,
    ShelfApp, ShelfError, StaleReport, WithdrawRequest,
};
use whyshelf_core::domain::actor::Actor;
use whyshelf_core::domain::hash::ContentHash;
use whyshelf_core::domain::id::EntityId;
use whyshelf_core::domain::nonempty::NonBlank;
use whyshelf_core::domain::shelf::ledger::JudgementId;
use whyshelf_core::domain::shelf::schema::BookName;
use whyshelf_infra::shelf::Project;
use whyshelf_infra::shelf::system::parse_time;

pub use args::BooksCommand;
use args::Verb;

/// The environment variable that names who judges.
const ACTOR_VAR: &str = "WHYSHELF_ACTOR";

/// The exit status of a command.
#[derive(Debug, Clone, Copy)]
enum Exit {
    Ok = 0,
    Stopped = 1,
    Request = 2,
    Failed = 3,
}

/// Run a command on the project at `root`.
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
        BooksCommand::Get {
            reference,
            approved: false,
        } => ShelfApp::new(project)
            .get(&reference)
            .await
            .map(|r| print(&output::get(&r))),
        BooksCommand::Get {
            reference,
            approved: true,
        } => ShelfApp::new(project)
            .show_approved(&reference)
            .await
            .map(|r| print(&output::approved(&r))),
        BooksCommand::Trace { reference } => ShelfApp::new(project)
            .trace(&reference)
            .await
            .map(|r| print(&output::trace(&r))),
        BooksCommand::Stale { exit_code } => {
            ShelfApp::new(project)
                .stale()
                .await
                .map(|r| match print(&output::stale(&r)) {
                    Exit::Ok => stale_exit(&r, exit_code),
                    failed => failed,
                })
        }
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
        BooksCommand::Judge(verb) => match judge_run(verb) {
            Ok(JudgeRun::Decide(requests)) => ShelfApp::new(project)
                .judge_all(requests)
                .await
                .map(|r| print(&output::judged(&r))),
            Ok(JudgeRun::Same(requests)) => ShelfApp::new(project)
                .same_meaning(requests)
                .await
                .map(|r| print(&output::same_meaning(&r))),
            Ok(JudgeRun::Withdraw(request)) => ShelfApp::new(project)
                .withdraw(request)
                .await
                .map(|r| print(&output::withdrawn(&r))),
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

/// What a `judge` command asks for: decisions, or declarations that an edit keeps
/// the meaning. They are two use cases, with requests of their own.
enum JudgeRun {
    Decide(Vec<JudgeRequest>),
    Same(Vec<SameMeaningRequest>),
    Withdraw(WithdrawRequest),
}

/// The requests of a `judge` command, one per document or file named, or why the
/// arguments are not one.
fn judge_run(verb: Verb) -> Result<JudgeRun, String> {
    let reason = |text: &Option<String>| {
        text.as_deref()
            .map(|t| NonBlank::new(t).map_err(|e| format!("--reason: {e}")))
            .transpose()
    };
    let actor_of = |actor: Option<String>| {
        actor
            .map(|a| Actor::new(&a).map_err(|e| format!("--actor `{a}`: {e}")))
            .transpose()
    };
    let (judgement, common) = match verb {
        Verb::Same {
            references,
            artifacts,
            from,
            reason,
            actor,
        } => {
            let reason = NonBlank::new(&reason).map_err(|e| format!("--reason: {e}"))?;
            let actor = actor_of(actor)?;
            let about: Vec<SameMeaningAbout> = references
                .into_iter()
                .map(SameMeaningAbout::Doc)
                .chain(artifacts.into_iter().map(SameMeaningAbout::Artifact))
                .collect();
            let from = from
                .map(|h| {
                    h.parse::<ContentHash>()
                        .map_err(|e| format!("--from `{h}`: {e}"))
                })
                .transpose()?;
            if from.is_some() && about.len() != 1 {
                return Err("--from names the earlier hash of one document or file".to_owned());
            }
            return Ok(JudgeRun::Same(
                about
                    .into_iter()
                    .map(|about| SameMeaningRequest {
                        about,
                        from,
                        reason: reason.clone(),
                        actor: actor.clone(),
                    })
                    .collect(),
            ));
        }
        Verb::Withdraw {
            records,
            reason,
            actor,
        } => {
            let records = records
                .iter()
                .map(|r| {
                    r.parse::<EntityId>()
                        .map(JudgementId::new)
                        .map_err(|e| format!("`{r}` is not the id of a record: {e}"))
                })
                .collect::<Result<Vec<_>, _>>()?;
            return Ok(JudgeRun::Withdraw(WithdrawRequest {
                records,
                reason: NonBlank::new(&reason).map_err(|e| format!("--reason: {e}"))?,
                actor: actor_of(actor)?,
            }));
        }
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
    let actor = actor_of(common.actor)?;
    Ok(JudgeRun::Decide(
        common
            .references
            .into_iter()
            .map(|reference| JudgeRequest {
                judgement: judgement.clone(),
                reference,
                actor: actor.clone(),
            })
            .collect(),
    ))
}

/// The request of a `new` command, or why the arguments are not one. Every value
/// stays a text here: which becomes a list, a number or a truth value is decided by
/// the book's declaration, in the use case.
fn new_request(
    book: &str,
    title: Option<String>,
    key: Option<String>,
    entries: Vec<String>,
    body: Option<String>,
) -> Result<NewDocRequest, String> {
    let book = BookName::new(book).map_err(|e| format!("book `{book}`: {e}"))?;
    // The title is an entry like any other: the book's declaration says whether it
    // is there and whether it must be.
    let mut pairs: Vec<(String, String)> = title
        .into_iter()
        .map(|title| ("title".to_owned(), title))
        .collect();
    // `name+=text` is one item of a list, exactly as written; `name=text` an entry.
    let mut items = Vec::new();
    for entry in entries {
        let (name, value) = entry
            .split_once('=')
            .ok_or_else(|| format!("`{entry}` is not `<entry>=<value>` or `<entry>+=<item>`"))?;
        match name.strip_suffix('+') {
            Some("") => return Err(format!("`{entry}` names no entry before `+=`")),
            Some(name) => items.push((name.to_owned(), value.to_owned())),
            None => pairs.push((name.to_owned(), value.to_owned())),
        }
    }
    Ok(NewDocRequest {
        book,
        key,
        entries: pairs,
        items,
        body: body.unwrap_or_default(),
    })
}

/// The exit of `stale` once its answer is printed: with `--exit-code`, 1 when any
/// approval does not hold; otherwise 0.
fn stale_exit(report: &StaleReport, exit_code: bool) -> Exit {
    if exit_code && !report.entries.is_empty() {
        Exit::Stopped
    } else {
        Exit::Ok
    }
}

/// Print an answer as one JSON document on stdout.
fn print<T: serde::Serialize>(value: &T) -> Exit {
    let mut out = std::io::stdout().lock();
    let written = serde_json::to_value(value)
        .map(output::versioned)
        .and_then(|answer| serde_json::to_writer_pretty(&mut out, &answer))
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
    exit_of(error.class())
}

/// The exit status of each class of error: the one place it is given.
fn exit_of(class: ErrorClass) -> Exit {
    match class {
        ErrorClass::Request => Exit::Request,
        ErrorClass::Refused => Exit::Stopped,
        ErrorClass::Failed => Exit::Failed,
    }
}
