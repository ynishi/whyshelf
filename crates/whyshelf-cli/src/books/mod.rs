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
//! | 1 | `check` found something wrong (or `map` or `unlinked` a declaration that cannot be used), `stale --exit-code` found an approval that does not hold, `fmt --check` found a file not in the form (one it would write, or one it would leave alone), or a rule of the model refused the judgement (nothing was written; for `judge moved`, something still at the old name, nothing at the new one, or a name that cannot be told now) |
//! | 2 | the request is wrong (a reference that names nothing or several documents, a declaration that cannot be used, a `--book` of `unlinked` that is not declared, `get --approved` or `diff` of a document no approval stands about; for `new`, an unknown book, a key that is taken or malformed, a book of slugs with no `--key`, entries the book refuses, each listed in `problems`, or a file already at the path; for `init`, anything at the declaration's path or a document's, or a file that already writes a key the template gives, each listed in `problems`, a declaration template that is not sound, or a file that came to be at a path between the look and the write (the files written before are listed); for `judge same`, a path no document links, a `--from` hash nothing pinned, several earlier hashes and none named, or nothing to declare; for `judge moved`, two names that are one, an old name no pin of an approval that stands is read as now (one only a link writes, or one already moved away), or pins read as it in two spaces; for `judge withdraw`, an id that is no decision or declaration, or one already withdrawn) |
//! | 3 | the run failed (a file could not be read or written; for `init`, part way too, with the files written listed in `problems`; or a template `init` ships cannot be read) |
//!
//! `init` writes the declaration and the documents of a template of
//! `whyshelf_infra::shelf::init`, each where nothing is, or nothing at all.
//! `new` is the one command that writes a new document of a person's. Its values arrive as text
//! and are given to the use case as text; the use case reads them by the book's
//! declaration, and the domain judges the document before anything is written.
//! `fmt` is the one command that writes over a document's file, in the form `new`
//! writes, and only when that changes no reading of it.

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
    ApproveChosenRequest, Choose, ErrorClass, JudgeRequest, Judgement, MapReport, MovedRequest,
    NewDocRequest, PendingScope, SameMeaningAbout, SameMeaningRequest, ShelfApp, ShelfError,
    StaleReport, UnlinkedAsk, UnlinkedFilter, UnlinkedReport, WithdrawRequest,
};
use whyshelf_core::domain::actor::Actor;
use whyshelf_core::domain::hash::ContentHash;
use whyshelf_core::domain::id::EntityId;
use whyshelf_core::domain::nonempty::{NonBlank, NonEmptyVec};
use whyshelf_core::domain::shelf::ledger::JudgementId;
use whyshelf_core::domain::shelf::schema::BookName;
use whyshelf_infra::shelf::Project;
use whyshelf_infra::shelf::init;
use whyshelf_infra::shelf::system::parse_time;

pub use args::BooksCommand;
use args::{Common, Verb};

/// The environment variable that names who judges.
const ACTOR_VAR: &str = "WHYSHELF_ACTOR";

/// The environment variable that names cargo's build directory, where the docs of
/// the space `rust-item` are looked for first.
const CARGO_TARGET_DIR_VAR: &str = "CARGO_TARGET_DIR";

/// The exit status of a command.
#[derive(Debug, Clone, Copy)]
enum Exit {
    Ok = 0,
    Stopped = 1,
    Request = 2,
    Failed = 3,
}

/// The answer to arguments that are no command: `--help` and `--version` as the
/// text they are, with exit 0; any other as the error JSON of a request on stdout,
/// its text on stderr, and exit 2, as every wrong request is answered.
pub fn refuse_arguments(error: &clap::Error) -> ExitCode {
    use clap::error::ErrorKind;
    match error.kind() {
        ErrorKind::DisplayHelp | ErrorKind::DisplayVersion => {
            // clap writes help to stdout; a failure to write it is no answer to give.
            let _ = error.print();
            ExitCode::from(Exit::Ok as u8)
        }
        _ => {
            let text = error.render().to_string();
            eprint!("{text}");
            // The sentence is what comes before the usage and the tip, on one line.
            let message = text
                .lines()
                .take_while(|line| !line.starts_with("Usage:"))
                .map(str::trim)
                .filter(|line| {
                    !line.is_empty()
                        && !line.starts_with("tip:")
                        && !line.starts_with("For more information")
                })
                .collect::<Vec<_>>()
                .join(" ");
            let message = message.trim_start_matches("error: ").to_owned();
            let message = if message.is_empty()
                || error.kind() == ErrorKind::DisplayHelpOnMissingArgumentOrSubcommand
            {
                "a command is needed (see --help)".to_owned()
            } else {
                message
            };
            print(&output::usage(message));
            ExitCode::from(Exit::Request as u8)
        }
    }
}

/// Run a command on the project at `root`.
///
/// `root` is the `--root` given, if one was (the current directory otherwise): the
/// `next` commands of `init` carry it as it was typed.
pub async fn run(root: Option<PathBuf>, command: BooksCommand) -> ExitCode {
    let typed_root = root.clone();
    let project = Project::new(
        root.unwrap_or_else(|| PathBuf::from(".")),
        std::env::var(ACTOR_VAR).ok(),
        std::env::var_os(CARGO_TARGET_DIR_VAR).map(PathBuf::from),
    );
    let result = match command {
        BooksCommand::Init { sample } => {
            let (request, book) = if sample {
                match init::sample() {
                    Ok(request) => (request, None),
                    Err(error) => return failed_run(&error),
                }
            } else {
                (init::minimal(), Some(init::MINIMAL_BOOK))
            };
            let root = match &typed_root {
                None => output::NextRoot::Current,
                Some(root) => root
                    .to_str()
                    .map_or(output::NextRoot::NotText, output::NextRoot::Given),
            };
            ShelfApp::new(project).init(request).await.map(|done| {
                let exit = print(&output::initialized(&done, book, root));
                if let (output::NextRoot::NotText, Some(typed)) = (root, &typed_root) {
                    eprintln!(
                        "warning: no next command is given: the root {} is no UTF-8 text, so a command cannot be written with it",
                        typed.display()
                    );
                }
                exit
            })
        }
        BooksCommand::Schema => Ok(print(&output::schemas())),
        BooksCommand::Check => ShelfApp::new(project).books_check().await.map(|check| {
            print(&output::books_check(&check));
            for warning in output::check_warnings(&check) {
                eprintln!("{warning}");
            }
            if check.is_clean() {
                Exit::Ok
            } else {
                Exit::Stopped
            }
        }),
        BooksCommand::Map => {
            ShelfApp::new(project)
                .map()
                .await
                .map(|r| match print(&output::map(&r)) {
                    Exit::Ok => map_exit(&r),
                    failed => failed,
                })
        }
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
        BooksCommand::Unlinked {
            book,
            no_incoming,
            no_outgoing,
        } => match unlinked_filter(book.as_deref(), no_incoming, no_outgoing) {
            Ok(filter) => ShelfApp::new(project).unlinked(&filter).await.map(|r| {
                match print(&output::unlinked(&r)) {
                    Exit::Ok => unlinked_exit(&r),
                    failed => failed,
                }
            }),
            Err(message) => {
                eprintln!("error: {message}");
                print(&output::usage(message));
                return ExitCode::from(Exit::Request as u8);
            }
        },
        BooksCommand::Stale { exit_code } => ShelfApp::new(project).stale().await.map(|r| {
            let exit = print(&output::stale(&r));
            for warning in output::stale_warnings(&r) {
                eprintln!("{warning}");
            }
            match exit {
                Exit::Ok => stale_exit(&r, exit_code),
                failed => failed,
            }
        }),
        BooksCommand::Stats { since } => {
            match since
                .as_deref()
                .map(|t| parse_time(t).map_err(|e| format!("--since `{t}`: {e}")))
                .transpose()
            {
                Ok(at) => ShelfApp::new(project)
                    .stats(at)
                    .await
                    .map(|r| print(&output::stats(&r, since.as_deref()))),
                Err(message) => {
                    eprintln!("error: {message}");
                    print(&output::usage(message));
                    return ExitCode::from(Exit::Request as u8);
                }
            }
        }
        BooksCommand::Moves => ShelfApp::new(project)
            .moves()
            .await
            .map(|r| print(&output::moves(&r))),
        BooksCommand::Diff { reference } => ShelfApp::new(project)
            .diff(&reference)
            .await
            .map(|r| print(&output::diff(&r))),
        BooksCommand::Fmt { check } => {
            let mut app = ShelfApp::new(project);
            let report = if check {
                app.fmt_check().await
            } else {
                app.fmt().await
            };
            report.map(|r| {
                let exit = print(&output::fmt(&r, check));
                for warning in output::fmt_warnings(&r) {
                    eprintln!("{warning}");
                }
                match exit {
                    Exit::Ok if check && !r.all_in_form() => Exit::Stopped,
                    other => other,
                }
            })
        }
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
            Ok(JudgeRun::Decide(requests)) => {
                ShelfApp::new(project).judge_all(requests).await.map(|r| {
                    let exit = print(&output::judged(&r));
                    for warning in output::judged_warnings(&r) {
                        eprintln!("{warning}");
                    }
                    exit
                })
            }
            Ok(JudgeRun::Chosen(request)) => ShelfApp::new(project)
                .approve_chosen(request)
                .await
                .map(|r| {
                    let exit = print(&output::chosen_approved(&r));
                    for warning in output::chosen_approved_warnings(&r) {
                        eprintln!("{warning}");
                    }
                    exit
                }),
            Ok(JudgeRun::Same(requests)) => ShelfApp::new(project)
                .same_meaning(requests)
                .await
                .map(|r| print(&output::same_meaning(&r))),
            Ok(JudgeRun::Moved(requests)) => ShelfApp::new(project)
                .moved(requests)
                .await
                .map(|r| print(&output::moved(&r))),
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

/// What a `judge` command asks for: decisions, declarations that an edit keeps the
/// meaning or that an artifact moved, or a withdrawal. Each is a use case, with
/// requests of its own.
enum JudgeRun {
    Decide(Vec<JudgeRequest>),
    Chosen(ApproveChosenRequest),
    Same(Vec<SameMeaningRequest>),
    Moved(Vec<MovedRequest>),
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
        Verb::Moved {
            from,
            to,
            reason,
            actor,
        } => {
            return Ok(JudgeRun::Moved(vec![MovedRequest {
                from,
                to,
                reason: NonBlank::new(&reason).map_err(|e| format!("--reason: {e}"))?,
                actor: actor_of(actor)?,
            }]));
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
        Verb::Approve {
            references,
            stale,
            pending,
            again,
            reason: text,
            actor,
            until,
        } => {
            let until = until
                .map(|t| parse_time(&t).map_err(|e| format!("--until `{t}`: {e}")))
                .transpose()?;
            if stale || pending {
                // No reference is every pending document.
                let scope = if references.is_empty() {
                    PendingScope::All
                } else {
                    PendingScope::UpstreamOf(
                        NonEmptyVec::from_vec(references).map_err(|e| e.to_string())?,
                    )
                };
                let choose = match (stale, pending, scope) {
                    (true, false, PendingScope::UpstreamOf(_)) => {
                        return Err(
                            "--stale takes no document; name documents with --pending".to_owned()
                        );
                    }
                    (true, false, PendingScope::All) => Choose::Stale,
                    (false, _, scope) => Choose::Pending(scope),
                    (true, true, scope) => Choose::StaleAndPending(scope),
                };
                // clap keeps --reason beside --stale and --pending, so the `None` arm
                // below is not reached; it is kept so that the request, whose reason
                // is required, is still made from the type alone should the arguments
                // change.
                let flag = if stale { "--stale" } else { "--pending" };
                return Ok(JudgeRun::Chosen(ApproveChosenRequest {
                    choose,
                    reason: reason(&text)?.ok_or_else(|| format!("{flag} needs --reason"))?,
                    actor: actor_of(actor)?,
                    until,
                }));
            }
            (
                Judgement::Approve {
                    until,
                    reason: reason(&text)?,
                    again,
                },
                Common {
                    references,
                    reason: text,
                    actor,
                },
            )
        }
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

/// The filter of an `unlinked` command, or why the arguments are not one (clap keeps
/// the two flags from being given together).
fn unlinked_filter(
    book: Option<&str>,
    no_incoming: bool,
    no_outgoing: bool,
) -> Result<UnlinkedFilter, String> {
    let book = book
        .map(|b| BookName::new(b).map_err(|e| format!("--book `{b}`: {e}")))
        .transpose()?;
    let ask = match (no_incoming, no_outgoing) {
        (true, _) => UnlinkedAsk::NoIncoming,
        (false, true) => UnlinkedAsk::NoOutgoing,
        (false, false) => UnlinkedAsk::Both,
    };
    Ok(UnlinkedFilter { book, ask })
}

/// The exit of `unlinked` once its answer is printed: 1 for a declaration that
/// cannot be used, as `check` gives for one; otherwise 0, whatever it lists.
fn unlinked_exit(report: &UnlinkedReport) -> Exit {
    match report {
        UnlinkedReport::SchemaInvalid { .. } => Exit::Stopped,
        UnlinkedReport::Answered { .. } => Exit::Ok,
    }
}

/// The exit of `stale` once its answer is printed: with `--exit-code`, 1 when any
/// approval does not hold, or when one stands on a name that cannot be told now (a
/// gate cannot say it holds); otherwise 0.
fn stale_exit(report: &StaleReport, exit_code: bool) -> Exit {
    if exit_code && (!report.entries.is_empty() || !report.unknown.is_empty()) {
        Exit::Stopped
    } else {
        Exit::Ok
    }
}

/// The exit of `map` once its answer is printed: 1 for a declaration that cannot be
/// used, as `check` gives for one; otherwise 0.
fn map_exit(report: &MapReport) -> Exit {
    match report {
        MapReport::SchemaInvalid(_) => Exit::Stopped,
        MapReport::Mapped(_) => Exit::Ok,
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

/// The end of a command when the program cannot make what it runs on (a template
/// `init` ships that is no request): a failure of this program, told as every
/// failed run is.
fn failed_run(error: &dyn std::error::Error) -> ExitCode {
    let mut chain = vec![error.to_string()];
    let mut source = error.source();
    while let Some(cause) = source {
        chain.push(cause.to_string());
        source = cause.source();
    }
    eprintln!("error: {}", chain.join(": "));
    print(&output::failure(chain));
    ExitCode::from(Exit::Failed as u8)
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
