//! A cover over artifacts, on the binary and a small project in a Git repository: a
//! link to files declares `cover = { include = ["src/**/*.rs"] }`, so every file the
//! selection takes is expected to be reached by a document.
//!
//! With `error` strength `check` fails while one file is reached by no document and
//! names it, and passes once a document links it, or once it is exempted with
//! `judge exempt --artifact`; with `warn` the same gap is listed and `check` exits 0;
//! outside a Git repository the names cannot be listed, and an `error` cover fails
//! rather than pass on a listing it could not make. After `judge moved` of an
//! exempted file the exemption is told no longer needed (`not_listed`). `cover` tells
//! the same per link and fails nothing.

mod common;

use std::fs;
use std::path::PathBuf;
use std::process::{Command, Output};

use common::{BOOKS, NOTE, Project, json};

/// A home inside the project, holding the empty file that stands for the global Git
/// settings (Git runs with the person's own settings shut out, as in `moves.rs`).
fn home(project: &Project) -> PathBuf {
    let home = project.0.join(".home");
    fs::create_dir_all(&home).unwrap();
    let global = home.join("gitconfig");
    if fs::metadata(&global).is_err() {
        fs::write(&global, "").unwrap();
    }
    home
}

fn git(project: &Project, args: &[&str]) {
    let home = home(project);
    let mut command = Command::new("git");
    for (name, _) in std::env::vars_os() {
        if name.to_string_lossy().starts_with("GIT_") {
            command.env_remove(&name);
        }
    }
    let status = command
        .env("HOME", &home)
        .env("XDG_CONFIG_HOME", home.join(".config"))
        .env("GIT_CONFIG_GLOBAL", home.join("gitconfig"))
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .arg("-C")
        .arg(&project.0)
        .args(["-c", "user.name=t", "-c", "user.email=t@example.com"])
        .args(args)
        .status()
        .expect("git runs");
    assert!(status.success(), "git {args:?}");
}

/// The declaration of a note book whose link `files` to the space `file` covers the
/// `.rs` files under `src`, with this strength (`error` when none is written).
fn covering(strength: Option<&str>) -> String {
    let strength = strength.map_or_else(String::new, |s| format!("strength = \"{s}\", "));
    format!(
        "{BOOKS}[[book.link]]\nname = \"files\"\ntarget = \"artifact\"\nspace = \"file\"\nmeans = \"depends\"\ncover = {{ {strength}include = [\"src/**/*.rs\"] }}\n"
    )
}

/// The note, its `files` link writing `files`.
fn note_on(files: &[&str]) -> String {
    let quoted: Vec<String> = files.iter().map(|f| format!("\"{f}\"")).collect();
    NOTE.replace(
        "title: \"A note\"\n",
        &format!("title: \"A note\"\nfiles: [{}]\n", quoted.join(", ")),
    )
}

/// A project whose note links `linked`, over `src/a.rs` and `src/b.rs` (and a file
/// outside the selection), in a Git repository when `in_git`.
fn project(name: &str, strength: Option<&str>, linked: &[&str], in_git: bool) -> Project {
    let project = Project::with(name, &covering(strength), &note_on(linked));
    fs::create_dir_all(project.0.join("src")).unwrap();
    fs::create_dir_all(project.0.join("docs")).unwrap();
    for file in ["src/a.rs", "src/b.rs", "docs/not-code.txt"] {
        fs::write(project.0.join(file), "one\n").unwrap();
    }
    if in_git {
        git(&project, &["init", "-q"]);
    }
    project
}

fn relink(project: &Project, files: &[&str]) {
    fs::write(project.0.join("notes/N-1.md"), note_on(files)).unwrap();
}

fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

fn ok(output: &Output) -> serde_json::Value {
    assert_eq!(output.status.code(), Some(0), "{}", stderr(output));
    json(output)
}

const KEPT: &str = "kept on purpose: it is read by nobody yet";

#[test]
fn an_error_cover_fails_check_while_a_file_is_reached_by_no_document_and_passes_once_one_links_it()
{
    let project = project("artifact-cover-error", None, &["src/a.rs"], true);
    let check = project.run(&["check"]);
    assert_eq!(check.status.code(), Some(1), "{}", stderr(&check));
    let answer = json(&check);
    assert_eq!(answer["clean"], false);
    assert_eq!(
        answer["artifact_uncovered"],
        serde_json::json!([{
            "book": "note", "link": "files", "name": "src/b.rs", "space": "file", "severity": "error",
            "of": "name", "whole": null
        }])
    );
    // The file outside the selection is no gap, and the listing is the whole.
    let link = &answer["artifact_cover"][0];
    assert_eq!(link["completeness"], "all");
    assert_eq!(link["selected"], 2);
    assert_eq!(
        (link["reached"].clone(), link["gaps"].clone()),
        (1.into(), 1.into())
    );
    assert_eq!(
        link["selection"],
        serde_json::json!({"include": ["src/**/*.rs"], "exclude": []})
    );
    assert_eq!(
        stderr(&check),
        "",
        "an error gap fails the check and warns of nothing"
    );

    // A document links it: nothing is left to reach.
    relink(&project, &["src/a.rs", "src/b.rs"]);
    let check = project.run(&["check"]);
    let answer = ok(&check);
    assert_eq!(answer["clean"], true);
    assert_eq!(answer["artifact_uncovered"], serde_json::json!([]));
    assert_eq!(answer["artifact_cover"][0]["gaps"], 0);
    assert_eq!(answer["artifact_cover"][0]["reached"], 2);
}

#[test]
fn a_file_exempt_from_an_error_cover_passes_check_and_is_listed_apart() {
    let project = project("artifact-cover-exempt", None, &["src/a.rs"], true);
    assert_eq!(project.run(&["check"]).status.code(), Some(1));

    let done = ok(&project.run(&[
        "judge",
        "exempt",
        "--artifact",
        "src/b.rs",
        "--from",
        "note.files",
        "--reason",
        KEPT,
    ]));
    let one = &done["exempt"][0];
    assert_eq!(one["doc"], serde_json::Value::Null);
    assert_eq!(
        (one["artifact"].clone(), one["space"].clone()),
        ("src/b.rs".into(), "file".into())
    );
    assert_eq!(one["record"]["kind"], "exempt");
    assert_eq!(
        one["record"]["subject"],
        serde_json::json!({"space": "file", "name": "src/b.rs"})
    );
    let id = one["record"]["id"].as_str().unwrap().to_owned();

    let check = project.run(&["check"]);
    let answer = ok(&check);
    assert_eq!(answer["clean"], true);
    assert_eq!(answer["artifact_uncovered"], serde_json::json!([]));
    let listed = &answer["artifact_exempt"][0];
    assert_eq!(listed["name"], "src/b.rs");
    assert_eq!(listed["space"], "file");
    assert_eq!(
        (listed["book"].clone(), listed["link"].clone()),
        ("note".into(), "files".into())
    );
    assert_eq!(listed["severity"], "error");
    assert_eq!(listed["exemption"], id.as_str());
    assert_eq!(listed["actor"], "tester");
    assert_eq!(listed["reason"], KEPT);
    assert_eq!(answer["artifact_cover"][0]["exempt"], 1);
    assert_eq!(
        stderr(&check),
        "",
        "an exemption that is needed warns of nothing"
    );

    // A second exemption of it is refused, and a document linking it makes the
    // first one no longer needed.
    let again = project.run(&[
        "judge",
        "exempt",
        "--artifact",
        "src/b.rs",
        "--from",
        "note.files",
        "--reason",
        KEPT,
    ]);
    assert_eq!(again.status.code(), Some(2), "{}", stderr(&again));
    relink(&project, &["src/a.rs", "src/b.rs"]);
    let check = project.run(&["check"]);
    let answer = ok(&check);
    assert_eq!(answer["artifact_exemption_unneeded"][0]["why"], "reached");
    assert_eq!(
        answer["artifact_exemption_unneeded"][0]["exemption"],
        id.as_str()
    );
    assert!(
        stderr(&check).contains(&format!("withdraw it with `judge withdraw {id}`")),
        "{}",
        stderr(&check)
    );
}

#[test]
fn a_warn_cover_lists_the_same_gap_and_check_exits_0() {
    let project = project("artifact-cover-warn", Some("warn"), &["src/a.rs"], true);
    let check = project.run(&["check"]);
    let answer = ok(&check);
    assert_eq!(answer["clean"], true);
    assert_eq!(answer["artifact_uncovered"][0]["name"], "src/b.rs");
    assert_eq!(answer["artifact_uncovered"][0]["severity"], "warn");
    assert_eq!(
        stderr(&check),
        "warning: `src/b.rs` in `file` is reached by no note.files (cover = \"warn\")\n"
    );
}

#[test]
fn a_cover_whose_names_cannot_be_listed_fails_instead_of_passing_unseen() {
    // No Git repository: the files cannot be listed, so no gap can be ruled out.
    let project = project("artifact-cover-nogit", None, &["src/a.rs"], false);
    let check = project.run(&["check"]);
    assert_eq!(check.status.code(), Some(1), "{}", stderr(&check));
    let answer = json(&check);
    assert_eq!(answer["clean"], false);
    let link = &answer["artifact_cover"][0];
    assert_eq!(link["completeness"], "cannot_list");
    assert!(link["why"].is_string(), "{link}");
    assert_eq!(link["selected"], serde_json::Value::Null);
    assert_eq!(answer["artifact_uncovered"], serde_json::json!([]));

    // An exemption cannot be told either, and is refused.
    let exempt = project.run(&[
        "judge",
        "exempt",
        "--artifact",
        "src/b.rs",
        "--from",
        "note.files",
        "--reason",
        KEPT,
    ]);
    assert_eq!(exempt.status.code(), Some(1), "{}", stderr(&exempt));

    // A `warn` cover of the same project says so and exits 0.
    let project = self::project("artifact-cover-nogit-warn", Some("warn"), &[], false);
    let check = project.run(&["check"]);
    let answer = ok(&check);
    assert_eq!(answer["clean"], true);
    assert_eq!(answer["artifact_cover"][0]["completeness"], "cannot_list");
    assert!(stderr(&check).contains("note.files cannot say which names it leaves unreached"));
}

#[test]
fn a_selection_that_takes_no_file_fails_an_error_cover() {
    let project = project("artifact-cover-nothing", None, &["src/a.rs"], true);
    fs::remove_file(project.0.join("src/a.rs")).unwrap();
    fs::remove_file(project.0.join("src/b.rs")).unwrap();
    let check = project.run(&["check"]);
    assert_eq!(check.status.code(), Some(1), "{}", stderr(&check));
    let link = &json(&check)["artifact_cover"][0];
    assert_eq!(link["completeness"], "all");
    assert_eq!(link["selects_nothing"], true);
}

#[test]
fn after_a_moved_declaration_the_exemption_of_the_old_name_is_not_listed() {
    // Approved while it linked both files, so that a pin stands at `src/b.rs`.
    let project = project(
        "artifact-cover-moved",
        None,
        &["src/a.rs", "src/b.rs"],
        true,
    );
    ok(&project.run(&["judge", "approve", "N-1"]));
    relink(&project, &["src/a.rs"]);
    ok(&project.run(&[
        "judge",
        "exempt",
        "--artifact",
        "src/b.rs",
        "--from",
        "note.files",
        "--reason",
        KEPT,
    ]));
    assert_eq!(project.run(&["check"]).status.code(), Some(0));

    fs::rename(project.0.join("src/b.rs"), project.0.join("src/c.rs")).unwrap();
    ok(&project.run(&[
        "judge", "moved", "src/b.rs", "src/c.rs", "--reason", "renamed",
    ]));
    let check = project.run(&["check"]);
    let answer = json(&check);
    // The exemption does not move: the old name is no longer in the listing.
    let unneeded = &answer["artifact_exemption_unneeded"][0];
    assert_eq!(unneeded["name"], "src/b.rs");
    assert_eq!(unneeded["why"], "not_listed");
    assert_eq!(answer["artifact_exempt"], serde_json::json!([]));
    assert!(stderr(&check).contains("not needed"), "{}", stderr(&check));
    // The new name is a file nobody reaches.
    assert_eq!(answer["artifact_uncovered"][0]["name"], "src/c.rs");
    assert_eq!(check.status.code(), Some(1));
}

#[test]
fn cover_tells_each_name_per_link_and_fails_nothing() {
    let project = project("artifact-cover-command", None, &["src/a.rs"], true);
    ok(&project.run(&[
        "judge",
        "exempt",
        "--artifact",
        "src/b.rs",
        "--from",
        "note.files",
        "--reason",
        KEPT,
    ]));
    relink(&project, &["src/a.rs", "src/gone.rs"]);
    let answer = ok(&project.run(&["cover"]));
    let link = &answer["links"][0];
    assert_eq!(
        (link["book"].clone(), link["link"].clone()),
        ("note".into(), "files".into())
    );
    assert_eq!(link["completeness"], "all");
    assert_eq!(
        link["names"],
        serde_json::json!([
            {"name": "src/a.rs", "stands": "reached", "exemption": null},
            {"name": "src/b.rs", "stands": "exempt", "exemption": answer["exempt"][0]["exemption"]},
        ])
    );
    assert_eq!(answer["uncovered"], serde_json::json!([]));

    // A gap is told, not failed: `check` is the gate.
    fs::write(project.0.join("src/c.rs"), "one\n").unwrap();
    let covered = project.run(&["cover", "--link", "note.files"]);
    let answer = ok(&covered);
    assert_eq!(answer["uncovered"][0]["name"], "src/c.rs");
    assert_eq!(answer["links"].as_array().unwrap().len(), 1);
    assert_eq!(project.run(&["check"]).status.code(), Some(1));

    // A link that covers no artifacts is a wrong request.
    for link in ["note.nowhere", "note.title"] {
        let refused = project.run(&["cover", "--link", link]);
        assert_eq!(
            refused.status.code(),
            Some(2),
            "{link}: {}",
            stderr(&refused)
        );
    }
}

#[test]
fn an_artifact_and_a_document_are_not_exempted_together() {
    let project = project("artifact-cover-both", None, &["src/a.rs"], true);
    let both = project.run(&[
        "judge",
        "exempt",
        "N-1",
        "--artifact",
        "src/b.rs",
        "--from",
        "note.files",
        "--reason",
        KEPT,
    ]);
    assert_eq!(both.status.code(), Some(2), "{}", stderr(&both));
    let neither = project.run(&["judge", "exempt", "--from", "note.files", "--reason", KEPT]);
    assert_eq!(neither.status.code(), Some(2), "{}", stderr(&neither));
}

#[test]
fn a_cover_of_a_strength_alone_on_a_link_to_artifacts_is_a_problem_of_the_declaration() {
    let books = format!(
        "{BOOKS}[[book.link]]\nname = \"files\"\ntarget = \"artifact\"\nspace = \"file\"\nmeans = \"depends\"\ncover = true\n"
    );
    let project = Project::with("artifact-cover-alone", &books, NOTE);
    let check = project.run(&["check"]);
    assert_eq!(check.status.code(), Some(1), "{}", stderr(&check));
    let problems = json(&check)["schema_problems"].clone();
    assert!(
        problems[0]
            .as_str()
            .unwrap()
            .contains("a table of a selection"),
        "{problems}"
    );
    // `cover` and `map` tell a declaration that cannot be used as `check` does.
    assert_eq!(project.run(&["cover"]).status.code(), Some(1));
    assert_eq!(project.run(&["map"]).status.code(), Some(1));
}

#[test]
fn a_selection_a_space_does_not_read_is_a_problem_of_the_declaration() {
    let books = format!(
        "{BOOKS}[[book.link]]\nname = \"files\"\ntarget = \"artifact\"\nspace = \"file\"\nmeans = \"depends\"\ncover = {{ include = [\"src/[\"] }}\n"
    );
    let project = Project::with("artifact-cover-badglob", &books, NOTE);
    let check = project.run(&["check"]);
    assert_eq!(check.status.code(), Some(1), "{}", stderr(&check));
    let problems = json(&check)["schema_problems"].clone();
    assert!(
        problems[0]
            .as_str()
            .unwrap()
            .contains("not one of the space `file`"),
        "{problems}"
    );
}

#[test]
fn map_and_the_schema_show_the_selection_of_a_covering_link() {
    let project = project("artifact-cover-map", Some("warn"), &[], true);
    let map = ok(&project.run(&["map"]));
    let link = &map["links"][0];
    assert_eq!(link["cover"], true);
    assert_eq!(link["cover_severity"], "warn");
    assert_eq!(
        link["selection"],
        serde_json::json!({"include": ["src/**/*.rs"], "exclude": []})
    );
    assert!(
        map["diagram"].as_str().unwrap().contains("cover(warn)"),
        "{}",
        map["diagram"]
    );
    let schema = ok(&project.run(&["schema"]));
    assert!(schema["outputs"]["cover"].is_object());
    assert!(
        schema["outputs"]["map"].to_string().contains("selection"),
        "the schema of `map` has the selection"
    );
    assert!(
        schema["outputs"]["check"]
            .to_string()
            .contains("artifact_uncovered")
    );
}
