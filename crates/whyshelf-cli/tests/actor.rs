//! Who judges, on the binary and a small project of files (DEC-34): the actor given
//! with `--actor`, else `WHYSHELF_ACTOR`, else Git's `user.name` of the project, and
//! never a name read from a file of the project.
//!
//! Git runs with the person's own settings shut out: no system file, an empty
//! global file, a home inside the project, and no `GIT_*` variable of the caller,
//! so the tests answer the same on every machine.

mod common;

use std::fs;
use std::path::PathBuf;
use std::process::{Command, Output};

use common::{Project, json};

#[test]
fn the_actor_comes_from_the_variable() {
    let project = Project::new("actor");
    let output = project.run(&["judge", "approve", "N-1"]);
    assert_eq!(output.status.code(), Some(0));
    let answer = json(&output);
    assert_eq!(answer["version"], 1);
    assert_eq!(answer["judged"][0]["record"]["actor"], "tester");
}

/// A home inside the project, holding the empty file that stands for the global
/// Git settings.
fn home(project: &Project) -> PathBuf {
    let home = project.0.join("home");
    fs::create_dir_all(&home).unwrap();
    let global = home.join("gitconfig");
    if fs::metadata(&global).is_err() {
        fs::write(&global, "").unwrap();
    }
    home
}

/// `program` run in an environment where Git reads the project's own settings
/// only, and `WHYSHELF_ACTOR` is unset.
fn isolated(project: &Project, program: &str) -> Command {
    let home = home(project);
    let mut command = Command::new(program);
    for (name, _) in std::env::vars_os() {
        if name.to_string_lossy().starts_with("GIT_") {
            command.env_remove(&name);
        }
    }
    command
        .env_remove("WHYSHELF_ACTOR")
        .env("HOME", &home)
        .env("XDG_CONFIG_HOME", home.join(".config"))
        .env("GIT_CONFIG_GLOBAL", home.join("gitconfig"))
        .env("GIT_CONFIG_NOSYSTEM", "1");
    command
}

/// Git run on the project, refusing to go on when it fails.
fn git(project: &Project, args: &[&str]) {
    let status = isolated(project, "git")
        .arg("-C")
        .arg(&project.0)
        .args(args)
        .status()
        .expect("git runs");
    assert!(status.success(), "git {args:?}");
}

/// The project made a Git repository, with this `user.name` of its own when given.
fn repository(project: &Project, user: Option<&str>) {
    git(project, &["init", "-q"]);
    if let Some(user) = user {
        git(project, &["config", "user.name", user]);
    }
}

/// The binary run on the project in the isolated environment.
fn whyshelf(project: &Project, args: &[&str], var: Option<&str>) -> Output {
    let mut command = isolated(project, env!("CARGO_BIN_EXE_whyshelf"));
    if let Some(var) = var {
        command.env("WHYSHELF_ACTOR", var);
    }
    command
        .arg("--root")
        .arg(&project.0)
        .args(args)
        .output()
        .expect("the binary runs")
}

#[test]
fn the_actor_given_on_the_command_line_wins_over_the_variable() {
    let project = Project::new("actor-flag");
    repository(&project, Some("gina"));
    let output = whyshelf(
        &project,
        &["judge", "approve", "--actor", "ana", "N-1"],
        Some("tester"),
    );
    assert_eq!(output.status.code(), Some(0));
    let answer = json(&output);
    assert_eq!(answer["version"], 1);
    assert_eq!(answer["judged"][0]["record"]["actor"], "ana");
}

#[test]
fn the_variable_wins_over_gits_user_name() {
    let project = Project::new("actor-var");
    repository(&project, Some("gina"));
    let output = whyshelf(&project, &["judge", "approve", "N-1"], Some("tester"));
    assert_eq!(output.status.code(), Some(0));
    let answer = json(&output);
    assert_eq!(answer["version"], 1);
    assert_eq!(answer["judged"][0]["record"]["actor"], "tester");
}

#[test]
fn with_no_variable_gits_user_name_of_the_project_is_used() {
    let project = Project::new("actor-git");
    repository(&project, Some("gina"));
    let output = whyshelf(&project, &["judge", "approve", "N-1"], None);
    assert_eq!(output.status.code(), Some(0));
    let answer = json(&output);
    assert_eq!(answer["version"], 1);
    assert_eq!(answer["judged"][0]["record"]["actor"], "gina");
}

#[test]
fn with_no_actor_the_judgement_is_refused_and_no_file_names_one() {
    let project = Project::new("actor-none");
    // A repository with no `user.name`, so Git stops here and looks no further up.
    repository(&project, None);
    // Files a committed project could hold, each naming someone.
    fs::write(project.0.join(".env"), "WHYSHELF_ACTOR=mallory\n").unwrap();
    fs::write(project.0.join(".whyshelf/actor"), "mallory\n").unwrap();
    // Git itself finds no name here: the key is absent (exit 1), not a failure of
    // git (128), so the refusal below is for no name.
    let lookup = isolated(&project, "git")
        .arg("-C")
        .arg(&project.0)
        .args(["config", "--get", "user.name"])
        .output()
        .expect("git runs");
    assert_eq!(
        lookup.status.code(),
        Some(1),
        "{}",
        String::from_utf8_lossy(&lookup.stderr)
    );
    let before = project.files();
    let output = whyshelf(&project, &["judge", "approve", "N-1"], None);
    let answer = json(&output);
    assert_eq!(output.status.code(), Some(3), "{answer}");
    assert_eq!(answer["version"], 1, "{answer}");
    assert_eq!(answer["error"], "failed", "{answer}");
    assert!(
        answer["messages"].to_string().contains("WHYSHELF_ACTOR"),
        "{answer}"
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(!stdout.contains("mallory"), "{stdout}");
    assert!(!stderr.contains("mallory"), "{stderr}");
    assert!(!project.0.join(".whyshelf/ledger").exists());
    assert_eq!(project.files(), before, "nothing is written");
}
