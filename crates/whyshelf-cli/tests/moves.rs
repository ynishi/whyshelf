//! `moves` on the binary and a small project in a Git repository: a pinned file moved
//! as it was is named with its new name, found by its content; moved with
//! `git mv` and edited, it is found by Git's rename detection, with a similarity;
//! outside Git, the content is still found and a note says renames were not looked
//! for. The answer is JSON, the exit is 0, and nothing is written, in the project or
//! in its repository.
//!
//! Git runs with the person's own settings shut out, as in `actor.rs`: no system
//! file, an empty global file, a home inside the project, and no `GIT_*` variable of
//! the caller.

mod common;

use std::fs;
use std::path::PathBuf;
use std::process::{Command, Output};

use common::{BOOKS, NOTE, Project, json};

/// Twelve lines, so that an edit of one keeps the file alike to Git.
const CODE: &str = "fn one() {}\nfn two() {}\nfn three() {}\nfn four() {}\nfn five() {}\nfn six() {}\nfn seven() {}\nfn eight() {}\nfn nine() {}\nfn ten() {}\nfn eleven() {}\nfn twelve() {}\n";

/// A home inside the project, holding the empty file that stands for the global Git
/// settings.
fn home(project: &Project) -> PathBuf {
    let home = project.0.join(".home");
    fs::create_dir_all(&home).unwrap();
    let global = home.join("gitconfig");
    if fs::metadata(&global).is_err() {
        fs::write(&global, "").unwrap();
    }
    home
}

/// `program` run where Git reads the project's own settings only.
fn isolated(project: &Project, program: &str) -> Command {
    let home = home(project);
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
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("WHYSHELF_ACTOR", "tester");
    command
}

fn git(project: &Project, args: &[&str]) {
    let status = isolated(project, "git")
        .arg("-C")
        .arg(&project.0)
        .args(["-c", "user.name=t", "-c", "user.email=t@example.com"])
        .args(args)
        .status()
        .expect("git runs");
    assert!(status.success(), "git {args:?}");
}

fn whyshelf(project: &Project, args: &[&str]) -> Output {
    isolated(project, env!("CARGO_BIN_EXE_whyshelf"))
        .arg("--root")
        .arg(&project.0)
        .args(args)
        .output()
        .expect("the binary runs")
}

/// A project whose note stands on `src/a.rs`, committed when `in_git`, and
/// approved.
fn approved(name: &str, in_git: bool) -> Project {
    let books = format!(
        "{BOOKS}[[book.link]]\nname = \"files\"\ntarget = \"artifact\"\nspace = \"file\"\nmeans = \"depends\"\n"
    );
    let note = NOTE.replace(
        "title: \"A note\"\n",
        "title: \"A note\"\nfiles: [\"src/a.rs\"]\n",
    );
    let project = Project::with(name, &books, &note);
    fs::create_dir_all(project.0.join("src")).unwrap();
    fs::write(project.0.join("src/a.rs"), CODE).unwrap();
    if in_git {
        git(&project, &["init", "-q"]);
        git(&project, &["add", "."]);
        git(&project, &["commit", "-q", "-m", "start"]);
    }
    let out = whyshelf(&project, &["judge", "approve", "N-1"]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    project
}

/// `moves`, checked to exit 0 and to write nothing, its answer.
fn moves(project: &Project) -> serde_json::Value {
    let before = project.files();
    let out = whyshelf(project, &["moves"]);
    assert_eq!(
        out.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(project.files(), before, "moves wrote something");
    let answer = json(&out);
    assert_eq!(answer["version"], 1);
    answer
}

/// The one gone name of the answer, checked to be `src/a.rs` pinned by N-1.
fn gone(answer: &serde_json::Value) -> serde_json::Value {
    let absent = answer["absent"].as_array().unwrap();
    assert_eq!(absent.len(), 1, "{answer}");
    let one = &absent[0];
    assert_eq!(one["space"], "file");
    assert_eq!(one["name"], "src/a.rs");
    let by = one["pinned_by"].as_array().unwrap();
    assert_eq!(by.len(), 1);
    assert!(by[0]["key"].as_str().unwrap().ends_with("N-1"), "{by:?}");
    one.clone()
}

#[test]
fn nothing_gone_is_an_empty_answer() {
    let project = approved("moves-none", true);
    let answer = moves(&project);
    assert_eq!(answer["absent"], serde_json::json!([]));
    assert_eq!(answer["cannot_suggest"], serde_json::json!([]));
    assert!(answer["health"].is_object());
}

#[test]
fn a_pinned_file_moved_unchanged_is_named_with_its_new_name() {
    let project = approved("moves-same", true);
    fs::create_dir_all(project.0.join("lib")).unwrap();
    fs::rename(project.0.join("src/a.rs"), project.0.join("lib/b.rs")).unwrap();

    let answer = moves(&project);
    let one = gone(&answer);
    assert_eq!(
        one["candidates"],
        serde_json::json!([
            {"name": "lib/b.rs", "found": "same_content", "similarity": null, "present": true}
        ])
    );
    assert_eq!(one["notes"], serde_json::json!([]));
    assert!(
        answer["drawing"]
            .as_str()
            .unwrap()
            .contains("-> lib/b.rs  same_content")
    );

    // What it names is what `judge moved` takes.
    let out = whyshelf(
        &project,
        &[
            "judge", "moved", "src/a.rs", "lib/b.rs", "--reason", "moved",
        ],
    );
    assert!(out.status.success());
    assert_eq!(moves(&project)["absent"], serde_json::json!([]));
}

#[test]
fn a_pinned_file_moved_with_an_edit_is_named_by_gits_rename_detection() {
    let project = approved("moves-renamed", true);
    git(&project, &["mv", "src/a.rs", "src/b.rs"]);
    fs::write(project.0.join("src/b.rs"), CODE.replace("seven", "seventh")).unwrap();
    // Renames are read from the index: the edit counts once it is added.
    git(&project, &["add", "src/b.rs"]);

    let answer = moves(&project);
    let one = gone(&answer);
    let candidates = one["candidates"].as_array().unwrap();
    assert_eq!(candidates.len(), 1, "{candidates:?}");
    assert_eq!(candidates[0]["name"], "src/b.rs");
    assert_eq!(candidates[0]["found"], "renamed");
    assert_eq!(candidates[0]["present"], true);
    let similarity = candidates[0]["similarity"].as_u64().unwrap();
    assert!((50..100).contains(&similarity), "{similarity}");
    assert_eq!(one["notes"], serde_json::json!([]));
}

#[test]
fn outside_git_the_content_is_found_and_the_note_says_why_renames_were_not() {
    let project = approved("moves-no-git", false);
    fs::rename(project.0.join("src/a.rs"), project.0.join("src/b.rs")).unwrap();
    let one = gone(&moves(&project));
    assert_eq!(one["candidates"][0]["name"], "src/b.rs");
    assert_eq!(one["candidates"][0]["found"], "same_content");
    let notes = one["notes"].as_array().unwrap();
    assert_eq!(notes.len(), 1, "{one}");
    assert_eq!(notes[0]["kind"], "no_revision");
    assert!(notes[0]["detail"].as_str().unwrap().contains("revision"));
}
