//! Who ran the command, on the binary and a small project of files (DEC-91): when it
//! is not the actor, `--ran-by`, else `WHYSHELF_RAN_BY`, else no one; never Git's
//! `user.name` and never a file of the project. The record keeps it as `ran_by` only
//! when it is another name than the actor's, and the answers that show a record's
//! actor show it beside the actor, `null` when the actor ran the command.
//!
//! Every run here sets the variables it means to and removes the other, so what the
//! caller's environment holds changes no answer.

mod common;

use std::fs;
use std::path::PathBuf;
use std::process::{Command, Output};

use common::{Project, json};

/// The binary run on the project as `actor` (`WHYSHELF_ACTOR`) and with `ran_by`
/// (`WHYSHELF_RAN_BY`), each only when given.
fn whyshelf(project: &Project, args: &[&str], actor: Option<&str>, ran_by: Option<&str>) -> Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_whyshelf"));
    command
        .env_remove("WHYSHELF_ACTOR")
        .env_remove("WHYSHELF_RAN_BY");
    if let Some(actor) = actor {
        command.env("WHYSHELF_ACTOR", actor);
    }
    if let Some(ran_by) = ran_by {
        command.env("WHYSHELF_RAN_BY", ran_by);
    }
    command
        .arg("--root")
        .arg(&project.0)
        .args(args)
        .output()
        .expect("the binary runs")
}

/// As `whyshelf`, as the person `tester` and with no `ran_by` from the environment.
fn as_tester(project: &Project, args: &[&str]) -> Output {
    whyshelf(project, args, Some("tester"), None)
}

/// The records of the project's ledger, as their files hold them.
fn stored(project: &Project) -> Vec<serde_json::Value> {
    let mut records = Vec::new();
    for entry in fs::read_dir(project.0.join(".whyshelf/ledger")).unwrap() {
        let text = fs::read_to_string(entry.unwrap().path()).unwrap();
        records.push(serde_json::from_str(&text).unwrap());
    }
    records
}

/// What `judge approve N-1` writes with these flags and variables, as the answer
/// shows the record and as its file holds it (they are one): the `actor` and the
/// `ran_by` of the record, `None` when it has no such key.
fn approved_as(
    project: &Project,
    flags: &[&str],
    actor: Option<&str>,
    ran_by: Option<&str>,
) -> (String, Option<String>) {
    let mut args = vec!["judge", "approve", "N-1"];
    args.extend_from_slice(flags);
    let output = whyshelf(project, &args, actor, ran_by);
    assert_eq!(output.status.code(), Some(0), "{}", json(&output));
    let record = json(&output)["judged"][0]["record"].clone();
    let file = stored(project).remove(0);
    assert_eq!(record, file, "the answer shows the record as it is stored");
    (
        record["actor"].as_str().unwrap().to_owned(),
        record.get("ran_by").map(|r| r.as_str().unwrap().to_owned()),
    )
}

fn names(actor: &str, ran_by: Option<&str>) -> (String, Option<String>) {
    (actor.to_owned(), ran_by.map(str::to_owned))
}

#[test]
fn a_ran_by_given_on_the_command_line_is_kept_beside_the_actor() {
    let project = Project::new("ran-by-flag");
    let got = approved_as(
        &project,
        &["--actor", "ana", "--ran-by", "a coding agent"],
        Some("tester"),
        None,
    );
    assert_eq!(got, names("ana", Some("a coding agent")));
}

#[test]
fn the_variable_names_who_ran_the_command() {
    let project = Project::new("ran-by-var");
    let got = approved_as(&project, &[], Some("tester"), Some("a coding agent"));
    assert_eq!(got, names("tester", Some("a coding agent")));
}

#[test]
fn the_command_line_wins_over_the_variable_for_who_ran_the_command() {
    let project = Project::new("ran-by-both");
    let got = approved_as(&project, &["--ran-by", "ci"], Some("tester"), Some("bot"));
    assert_eq!(got, names("tester", Some("ci")));
}

#[test]
fn a_ran_by_that_is_the_actor_is_one_name_whoever_gave_either() {
    for (flags, actor, ran_by) in [
        (&["--ran-by", "tester"][..], Some("tester"), None),
        (&["--actor", "ana", "--ran-by", "ana"], Some("tester"), None),
        (&[], Some("tester"), Some("tester")),
        (&["--ran-by", " tester "], Some("tester"), None),
    ] {
        let project = Project::new("ran-by-same");
        let got = approved_as(&project, flags, actor, ran_by);
        assert_eq!(got.1, None, "{flags:?} {ran_by:?}");
    }
}

/// A Git repository in the project, whose own `user.name` is `name`, with the
/// person's own settings shut out: no system file, an empty global file, a home inside
/// the project and no `GIT_*` variable of the caller.
fn repository(project: &Project, name: &str) -> Command {
    let home: PathBuf = project.0.join("home");
    fs::create_dir_all(&home).unwrap();
    fs::write(home.join("gitconfig"), "").unwrap();
    let isolated = |program: &str| {
        let mut command = Command::new(program);
        for (name, _) in std::env::vars_os() {
            if name.to_string_lossy().starts_with("GIT_") {
                command.env_remove(&name);
            }
        }
        command
            .env("HOME", &home)
            .env("XDG_CONFIG_HOME", home.join(".config"))
            .env("GIT_CONFIG_GLOBAL", home.join("gitconfig"))
            .env("GIT_CONFIG_NOSYSTEM", "1");
        command
    };
    for args in [&["init", "-q"][..], &["config", "user.name", name][..]] {
        let status = isolated("git")
            .arg("-C")
            .arg(&project.0)
            .args(args)
            .status()
            .expect("git runs");
        assert!(status.success(), "git {args:?}");
    }
    isolated(env!("CARGO_BIN_EXE_whyshelf"))
}

#[test]
fn gits_user_name_and_the_projects_files_are_not_who_ran_the_command() {
    let project = Project::new("ran-by-git");
    let mut command = repository(&project, "gina");
    // Files a committed project could hold, each naming someone as who ran it.
    fs::write(project.0.join(".env"), "WHYSHELF_RAN_BY=mallory\n").unwrap();
    fs::write(project.0.join(".whyshelf/ran_by"), "mallory\n").unwrap();
    // The actor is Git's, and the record names no one else.
    let output = command
        .env_remove("WHYSHELF_ACTOR")
        .env_remove("WHYSHELF_RAN_BY")
        .arg("--root")
        .arg(&project.0)
        .args(["judge", "approve", "N-1"])
        .output()
        .expect("the binary runs");
    assert_eq!(output.status.code(), Some(0), "{}", json(&output));
    let record = json(&output)["judged"][0]["record"].clone();
    assert_eq!(record["actor"], "gina");
    assert!(record.get("ran_by").is_none(), "{record}");
}

#[test]
fn a_blank_variable_names_no_one_and_a_blank_flag_is_a_wrong_request() {
    let project = Project::new("ran-by-blank");
    let got = approved_as(&project, &[], Some("tester"), Some("   "));
    assert_eq!(got, names("tester", None));

    let project = Project::new("ran-by-blank-flag");
    let before = project.files();
    let output = as_tester(&project, &["judge", "approve", "N-1", "--ran-by", "  "]);
    let answer = json(&output);
    assert_eq!(output.status.code(), Some(2), "{answer}");
    assert!(
        answer["messages"].to_string().contains("--ran-by"),
        "{answer}"
    );
    assert_eq!(project.files(), before, "nothing is written");
}

#[test]
fn every_judge_verb_takes_a_ran_by() {
    let project = Project::new("ran-by-help");
    for verb in [
        "approve",
        "deny",
        "deprecate",
        "retire",
        "revoke",
        "same",
        "moved",
        "exempt",
        "withdraw",
    ] {
        let output = as_tester(&project, &["judge", verb, "--help"]);
        let help = String::from_utf8_lossy(&output.stdout);
        assert!(help.contains("--ran-by <NAME>"), "judge {verb}: {help}");
    }
}

#[test]
fn a_revocation_keeps_who_ran_the_command_too() {
    let project = Project::new("ran-by-withdraw");
    let approved = json(&as_tester(&project, &["judge", "approve", "N-1"]));
    let id = approved["judged"][0]["record"]["id"].as_str().unwrap();
    let output = whyshelf(
        &project,
        &["judge", "withdraw", id, "--reason", "read it wrong"],
        Some("tester"),
        Some("a coding agent"),
    );
    assert_eq!(output.status.code(), Some(0), "{}", json(&output));
    let record = json(&output)["record"].clone();
    assert_eq!(record["kind"], "revocation");
    assert_eq!(record["actor"], "tester");
    assert_eq!(record["ran_by"], "a coding agent");
}

#[test]
fn the_answers_show_who_ran_the_command_beside_the_actor_and_null_when_it_is_the_actor() {
    let project = Project::new("ran-by-answers");
    let row = |args: &[&str]| json(&as_tester(&project, args));
    approved_as(
        &project,
        &["--ran-by", "a coding agent", "--reason", "read it whole"],
        Some("tester"),
        None,
    );
    let status = row(&["status"]);
    let approved = row(&["get", "N-1", "--approved"]);
    let shown = [
        status["docs"][0]["decision"].clone(),
        row(&["get", "N-1"])["row"]["decision"].clone(),
        approved["row"]["decision"].clone(),
    ];
    for decision in shown {
        assert_eq!(decision["actor"], "tester", "{decision}");
        assert_eq!(decision["ran_by"], "a coding agent", "{decision}");
    }
    assert_eq!(approved["actor"], "tester");
    assert_eq!(approved["ran_by"], "a coding agent");

    // A decision the actor ran: null, not a missing key.
    let project = Project::new("ran-by-answers-one");
    as_tester(&project, &["judge", "approve", "N-1"]);
    let status = json(&as_tester(&project, &["status"]));
    let decision = status["docs"][0]["decision"].as_object().unwrap();
    assert_eq!(decision["ran_by"], serde_json::Value::Null);
    let approved = json(&as_tester(&project, &["get", "N-1", "--approved"]));
    assert_eq!(approved["ran_by"], serde_json::Value::Null);
}

#[test]
fn the_exemptions_check_lists_show_who_ran_the_command_beside_the_actor() {
    // The project `init --sample` writes: F-2 is a finding no requirement grounds.
    let project = Project::empty("ran-by-exempt");
    assert_eq!(
        as_tester(&project, &["init", "--sample"]).status.code(),
        Some(0)
    );
    assert_eq!(
        as_tester(&project, &["new", "finding", "--title", "A second finding"])
            .status
            .code(),
        Some(0)
    );
    let exempt = |ran_by: Option<&str>| {
        let output = whyshelf(
            &project,
            &[
                "judge",
                "exempt",
                "F-2",
                "--from",
                "requirement.grounds",
                "--reason",
                "kept on purpose",
            ],
            Some("tester"),
            ran_by,
        );
        assert_eq!(output.status.code(), Some(0), "{}", json(&output));
        json(&output)
    };
    let done = exempt(Some("a coding agent"));
    assert_eq!(done["exempt"][0]["record"]["actor"], "tester");
    assert_eq!(done["exempt"][0]["record"]["ran_by"], "a coding agent");

    let answer = json(&as_tester(&project, &["check"]));
    assert_eq!(answer["exempt"][0]["actor"], "tester");
    assert_eq!(answer["exempt"][0]["ran_by"], "a coding agent");

    // Once the link reaches the document, the exemption that is not needed any more
    // says who made it in the same way.
    let req = project.0.join("books/requirement/REQ-1.md");
    let text = fs::read_to_string(&req).unwrap();
    assert!(text.contains("grounds: [\"F-1\"]"), "{text}");
    fs::write(
        &req,
        text.replace("grounds: [\"F-1\"]", "grounds: [\"F-1\", \"F-2\"]"),
    )
    .unwrap();
    let answer = json(&as_tester(&project, &["check"]));
    assert_eq!(answer["exemption_unneeded"][0]["actor"], "tester");
    assert_eq!(answer["exemption_unneeded"][0]["ran_by"], "a coding agent");
}
