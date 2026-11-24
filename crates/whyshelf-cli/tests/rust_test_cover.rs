//! A cover over tests, on the binary and a small project in a Git repository: a link
//! with the preset `test` declares `cover = { include = ["src/**/*.rs"] }`, so every
//! test of the `.rs` files the selection takes is expected to be reached by a
//! document, which names it by function.
//!
//! `check` fails while a test is reached by no document and names it, and passes
//! once a document names it, or once it is exempted with `judge exempt --artifact`.
//! A file in which a macro may make a test the scan does not see keeps the listing
//! from being whole, so `check` of the error cover is not clean and says which file
//! and what in it; a line of the `exclude` takes the file out of the cover. A
//! document that spells a test otherwise than the source does (`foo` for `r#foo`)
//! does not reach it, though it names the one test.

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

/// The declaration of a note book whose link `cases` names tests (the preset `test`)
/// and covers those of the `.rs` files under `src`, leaving out `exclude`.
fn covering(exclude: &[&str]) -> String {
    let quoted: Vec<String> = exclude.iter().map(|f| format!("\"{f}\"")).collect();
    format!(
        "{BOOKS}[[book.link]]\nname = \"cases\"\ntarget = \"artifact\"\npreset = \"test\"\ncover = {{ include = [\"src/**/*.rs\"], exclude = [{}] }}\n",
        quoted.join(", ")
    )
}

/// The note, its link `cases` writing `tests`.
fn note_on(tests: &[&str]) -> String {
    let quoted: Vec<String> = tests.iter().map(|t| format!("\"{t}\"")).collect();
    NOTE.replace(
        "title: \"A note\"\n",
        &format!("title: \"A note\"\ncases: [{}]\n", quoted.join(", ")),
    )
}

/// Two tests in a module of tests, and a test spelled raw.
const SOURCE: &str = "#[cfg(test)]\nmod tests {\n    #[test]\n    fn one() {}\n\n    #[test]\n    fn two() {}\n\n    #[test]\n    fn r#foo() {}\n}\n";

const ALL: [&str; 3] = [
    "src/a.rs::tests::one",
    "src/a.rs::tests::two",
    "src/a.rs::tests::r#foo",
];

/// A project whose note names `named` under `cases`, over `src/a.rs`, in a Git
/// repository.
fn project(name: &str, exclude: &[&str], named: &[&str]) -> Project {
    let project = Project::with(
        &format!("rust-test-cover-{name}"),
        &covering(exclude),
        &note_on(named),
    );
    fs::create_dir_all(project.0.join("src")).unwrap();
    fs::write(project.0.join("src/a.rs"), SOURCE).unwrap();
    git(&project, &["init", "-q"]);
    project
}

fn relink(project: &Project, tests: &[&str]) {
    fs::write(project.0.join("notes/N-1.md"), note_on(tests)).unwrap();
}

fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

fn ok(output: &Output) -> serde_json::Value {
    assert_eq!(output.status.code(), Some(0), "{}", stderr(output));
    json(output)
}

fn failed(output: &Output) -> serde_json::Value {
    assert_eq!(output.status.code(), Some(1), "{}", stderr(output));
    json(output)
}

#[test]
fn an_error_cover_over_tests_fails_check_while_a_test_is_named_by_no_document_and_passes_once_one_names_it()
 {
    let project = project(
        "gap",
        &[],
        &["src/a.rs::tests::one", "src/a.rs::tests::r#foo"],
    );
    let answer = failed(&project.run(&["check"]));
    assert_eq!(answer["clean"], false);
    assert_eq!(
        answer["artifact_uncovered"],
        serde_json::json!([{
            "book": "note", "link": "cases", "name": "src/a.rs::tests::two", "space": "rust-test", "severity": "error"
        }])
    );
    // The tests of the file are listed whole, by function.
    let link = &answer["artifact_cover"][0];
    assert_eq!(link["completeness"], "all");
    assert_eq!(link["selected"], 3);
    assert_eq!(
        (link["reached"].clone(), link["gaps"].clone()),
        (2.into(), 1.into())
    );

    relink(&project, &ALL);
    let answer = ok(&project.run(&["check"]));
    assert_eq!(answer["clean"], true);
    assert_eq!(answer["artifact_cover"][0]["gaps"], 0);
}

#[test]
fn a_test_exempt_from_the_cover_passes_check() {
    let project = project(
        "exempt",
        &[],
        &["src/a.rs::tests::one", "src/a.rs::tests::r#foo"],
    );
    failed(&project.run(&["check"]));
    let done = ok(&project.run(&[
        "judge",
        "exempt",
        "--artifact",
        "src/a.rs::tests::two",
        "--from",
        "note.cases",
        "--reason",
        "kept on purpose: it checks the fixture alone",
    ]));
    assert_eq!(
        done["exempt"][0]["record"]["subject"],
        serde_json::json!({"space": "rust-test", "name": "src/a.rs::tests::two"})
    );
    let answer = ok(&project.run(&["check"]));
    assert_eq!(answer["artifact_uncovered"], serde_json::json!([]));
    assert_eq!(answer["artifact_cover"][0]["exempt"], 1);
}

#[test]
fn a_file_where_a_macro_may_make_a_test_fails_the_cover_by_name_until_the_exclude_takes_it_out() {
    let project = project("maker", &[], &ALL);
    // Its test is found, but what the macro writes is not seen.
    fs::write(
        project.0.join("src/arch.rs"),
        "arch_lint::check!();\n\n#[test]\nfn holds() {}\n",
    )
    .unwrap();
    let answer = failed(&project.run(&["check"]));
    let link = &answer["artifact_cover"][0];
    assert_eq!(link["completeness"], "partial", "{link}");
    let why = link["why"].as_str().unwrap();
    assert!(
        why.contains("src/arch.rs: the macro call `check!` may make a test"),
        "{why}"
    );

    fs::write(
        project.0.join(".whyshelf/books.toml"),
        covering(&["src/arch.rs"]),
    )
    .unwrap();
    let answer = ok(&project.run(&["check"]));
    assert_eq!(answer["artifact_cover"][0]["completeness"], "all");
    assert_eq!(answer["artifact_cover"][0]["gaps"], 0);
}

#[test]
fn a_document_that_spells_a_test_otherwise_than_the_source_does_not_reach_it() {
    let project = project(
        "spelling",
        &[],
        &[
            "src/a.rs::tests::one",
            "src/a.rs::tests::two",
            "src/a.rs::tests::foo",
        ],
    );
    // `foo` names the test written `r#foo`, and is there; the cover compares the
    // name as written, so the test is a gap.
    let answer = failed(&project.run(&["check"]));
    assert_eq!(
        answer["artifact_uncovered"][0]["name"],
        "src/a.rs::tests::r#foo"
    );
    assert_eq!(
        answer["artifact_uncovered"].as_array().map(Vec::len),
        Some(1)
    );
    relink(&project, &ALL);
    ok(&project.run(&["check"]));
}
