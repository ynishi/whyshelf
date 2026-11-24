//! `impact`, on the binary and one small project: a note that stands on the bytes of a
//! file, approved, and the file then edited. Read through two directories and through
//! Git (two commits, and a commit and the index), the edit makes the note's approval
//! not hold (`yes`), and a note that stands on nothing changed answers `no`; a name
//! the side after cannot tell answers `cannot_tell`; both ways at once, neither, a
//! revision that names no commit and Git outside a repository are request errors
//! (exit 2); a declaration through a link out of the root, which two sides cannot read
//! alike, stops the reading (exit 3); and nothing is written.
//!
//! Each test writes its own project; the tests that need Git pass without it.

mod common;

use std::fs;
use std::path::Path;
use std::process::Command;

use common::{Project, json};
use serde_json::Value;

const BOOKS: &str = r#"[defaults]
artifact = "file"

[[book]]
name = "note"
dir = "notes"
key = { prefix = "N" }
[[book.field]]
name = "title"
type = "line"
required = true
[[book.link]]
name = "data"
target = "artifact"
preset = "file"
"#;

/// The note `N-<n>`, standing on the file `data`.
fn note(project: &Path, n: u8, data: &str) {
    let text = format!(
        "---\nid: \"01a1c4f0-0000-7000-8000-0000000000{n:02}\"\nkey: \"N-{n}\"\ntitle: \"note {n}\"\ndata: [\"{data}\"]\n---\nThe body.\n"
    );
    fs::write(project.join(format!("notes/N-{n}.md")), text).unwrap();
}

/// The project: `N-1` on `a.txt`, `N-2` on `b.txt`, both approved.
fn project(name: &str) -> Project {
    let project = Project::with(name, BOOKS, "");
    fs::remove_file(project.0.join("notes/N-1.md")).unwrap();
    fs::write(project.0.join("a.txt"), "one\n").unwrap();
    fs::write(project.0.join("b.txt"), "two\n").unwrap();
    note(&project.0, 1, "a.txt");
    note(&project.0, 2, "b.txt");
    let approved = project.run(&["judge", "approve", "N-1", "N-2"]);
    assert_eq!(approved.status.code(), Some(0), "{approved:?}");
    project
}

/// Every file and directory under `from`, copied under `to`.
fn copy(from: &Path, to: &Path) {
    fs::create_dir_all(to).unwrap();
    for entry in fs::read_dir(from).unwrap() {
        let path = entry.unwrap().path();
        let target = to.join(path.file_name().unwrap());
        if fs::read_dir(&path).is_ok() {
            copy(&path, &target);
        } else {
            fs::copy(&path, &target).unwrap();
        }
    }
}

/// Whether `git` can be run here.
fn git_installed() -> bool {
    Command::new("git")
        .arg("--version")
        .output()
        .is_ok_and(|out| out.status.success())
}

/// Git run on `dir` with no `GIT_*` variable of the caller.
fn git(dir: &Path, args: &[&str]) {
    let mut command = Command::new("git");
    for (name, _) in std::env::vars_os() {
        if name.to_string_lossy().starts_with("GIT_") {
            command.env_remove(&name);
        }
    }
    let status = command
        .arg("-C")
        .arg(dir)
        .args(["-c", "user.name=t", "-c", "user.email=t@example.com"])
        .args(["-c", "core.autocrlf=false", "-c", "init.defaultBranch=main"])
        .args(args)
        .status()
        .expect("git runs");
    assert!(status.success(), "git {args:?}");
}

/// The entry of the note `key` in an answer of `impact`.
fn entry<'a>(answer: &'a Value, key: &str) -> &'a Value {
    answer["docs"]
        .as_array()
        .unwrap()
        .iter()
        .find(|doc| doc["doc"]["key"] == key)
        .unwrap_or_else(|| panic!("{key} in {answer}"))
}

#[test]
fn an_edit_read_through_two_directories_makes_the_approval_on_it_not_hold() {
    let project = project("impact-files");
    let before = project.0.with_file_name(format!(
        "{}-before",
        project.0.file_name().unwrap().to_string_lossy()
    ));
    copy(&project.0, &before);
    fs::write(project.0.join("a.txt"), "one, edited\n").unwrap();
    let files = project.files();
    let output = project.run(&["impact", "--before", before.to_str().unwrap()]);
    let answer = json(&output);
    fs::remove_dir_all(&before).unwrap();
    assert_eq!(output.status.code(), Some(0), "{answer}");
    assert_eq!(project.files(), files, "nothing is written");
    assert_eq!(
        answer["way"],
        format!("files: {} -> the project root", before.display())
    );
    let one = entry(&answer, "note:N-1");
    assert_eq!(one["before"]["holding"], "holds");
    assert_eq!(one["after"]["holding"], "does_not_hold");
    assert_eq!(one["changed"], true);
    assert_eq!(one["made"]["made"], "yes", "{one}");
    assert_eq!(
        one["made"]["certain"][0]["cause"]["kind"],
        "artifact_changed"
    );
    let two = entry(&answer, "note:N-2");
    assert_eq!(two["made"]["made"], "no");
    assert_eq!(two["changed"], false);
}

#[test]
fn an_edit_read_through_git_makes_the_approval_on_it_not_hold() {
    if !git_installed() {
        return;
    }
    let project = project("impact-git");
    git(&project.0, &["init", "-q"]);
    git(&project.0, &["add", "-A"]);
    git(&project.0, &["commit", "-q", "-m", "approved"]);
    fs::write(project.0.join("a.txt"), "one, edited\n").unwrap();
    git(&project.0, &["commit", "-q", "-am", "edited"]);
    let files = project.files();
    let output = project.run(&["impact", "--since", "HEAD~1"]);
    let answer = json(&output);
    assert_eq!(output.status.code(), Some(0), "{answer}");
    assert_eq!(answer["way"], "git: HEAD~1 -> HEAD");
    assert_eq!(entry(&answer, "note:N-1")["made"]["made"], "yes");
    assert_eq!(entry(&answer, "note:N-2")["made"]["made"], "no");
    // What is staged, against the commit before it.
    fs::write(project.0.join("b.txt"), "two, staged\n").unwrap();
    git(&project.0, &["add", "b.txt"]);
    let output = project.run(&["impact", "--since", "HEAD", "--index"]);
    let answer = json(&output);
    assert_eq!(output.status.code(), Some(0), "{answer}");
    assert_eq!(answer["way"], "git: HEAD -> the index");
    assert_eq!(entry(&answer, "note:N-1")["made"]["made"], "no");
    assert_eq!(entry(&answer, "note:N-2")["made"]["made"], "yes");
    // `.git` changes as Git keeps its own state; the project's files do not.
    let unchanged = |files: &std::collections::BTreeMap<std::path::PathBuf, Option<Vec<u8>>>| {
        files
            .iter()
            .filter(|(path, _)| !path.starts_with(project.0.join(".git")))
            .filter(|(path, _)| !path.ends_with("b.txt"))
            .map(|(path, bytes)| (path.clone(), bytes.clone()))
            .collect::<Vec<_>>()
    };
    assert_eq!(
        unchanged(&project.files()),
        unchanged(&files),
        "nothing is written"
    );
}

#[cfg(unix)]
#[test]
fn a_name_the_side_after_cannot_tell_cannot_tell_whether_the_change_made_it_not_hold() {
    let project = project("impact-untold");
    let before = project.0.with_file_name(format!(
        "{}-before",
        project.0.file_name().unwrap().to_string_lossy()
    ));
    copy(&project.0, &before);
    // After, the file is a link out of the project: it cannot be told.
    let outside = before.with_file_name(format!(
        "{}-outside",
        project.0.file_name().unwrap().to_string_lossy()
    ));
    fs::write(&outside, "one\n").unwrap();
    fs::remove_file(project.0.join("a.txt")).unwrap();
    std::os::unix::fs::symlink(&outside, project.0.join("a.txt")).unwrap();
    let output = project.run(&["impact", "--before", before.to_str().unwrap()]);
    let answer = json(&output);
    fs::remove_dir_all(&before).unwrap();
    fs::remove_file(&outside).unwrap();
    assert_eq!(output.status.code(), Some(0), "{answer}");
    let one = entry(&answer, "note:N-1");
    assert_eq!(one["after"]["holding"], "cannot_tell", "{one}");
    assert_eq!(one["made"]["made"], "cannot_tell");
    assert_eq!(one["made"]["side"], "after");
    assert_eq!(one["made"]["untold"][0]["name"], "a.txt");
}

#[test]
fn a_change_not_named_as_one_way_is_a_request_error() {
    let project = project("impact-request");
    for args in [
        &["impact"][..],
        &["impact", "--since", "HEAD", "--before", "."],
        &["impact", "--until", "HEAD"],
        &["impact", "--since", "HEAD", "--until", "HEAD", "--index"],
    ] {
        let output = project.run(args);
        assert_eq!(output.status.code(), Some(2), "{args:?}");
    }
    // Git outside a repository, and a directory that is not there.
    let output = project.run(&["impact", "--since", "HEAD"]);
    assert_eq!(output.status.code(), Some(2));
    assert_eq!(json(&output)["error"], "request", "{}", json(&output));
    let output = project.run(&["impact", "--before", "/no/such/directory/here"]);
    assert_eq!(output.status.code(), Some(2));
    if !git_installed() {
        return;
    }
    git(&project.0, &["init", "-q"]);
    git(&project.0, &["add", "-A"]);
    git(&project.0, &["commit", "-q", "-m", "one"]);
    let output = project.run(&["impact", "--since", "no-such-revision"]);
    assert_eq!(output.status.code(), Some(2));
}

/// A Git that cannot be asked (here, a `git` on the path that cannot be run) is no
/// error of the request: the run failed (exit 3).
#[cfg(unix)]
#[test]
fn a_git_that_cannot_be_run_is_a_failed_run_and_no_request_error() {
    use std::os::unix::fs::PermissionsExt;
    let project = project("impact-git-fails");
    let bin = project.0.join("fake-bin");
    fs::create_dir_all(&bin).unwrap();
    let fake = bin.join("git");
    fs::write(&fake, "not a program\n").unwrap();
    fs::set_permissions(&fake, fs::Permissions::from_mode(0o644)).unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_whyshelf"))
        .arg("--root")
        .arg(&project.0)
        .args(["impact", "--since", "HEAD"])
        .env("WHYSHELF_ACTOR", "tester")
        .env("PATH", &bin)
        .output()
        .expect("the binary runs");
    assert_eq!(output.status.code(), Some(3), "{output:?}");
    assert_ne!(json(&output)["error"], "request", "{}", json(&output));
}

#[cfg(unix)]
#[test]
fn a_declaration_two_sides_cannot_read_alike_stops_the_reading() {
    let project = project("impact-leaves");
    let before = project.0.with_file_name(format!(
        "{}-before",
        project.0.file_name().unwrap().to_string_lossy()
    ));
    copy(&project.0, &before);
    // Before, the directory of whyshelf's files is a link out of that side.
    let outside = before.with_file_name(format!(
        "{}-whyshelf",
        project.0.file_name().unwrap().to_string_lossy()
    ));
    fs::rename(before.join(".whyshelf"), &outside).unwrap();
    std::os::unix::fs::symlink(&outside, before.join(".whyshelf")).unwrap();
    let output = project.run(&["impact", "--before", before.to_str().unwrap()]);
    fs::remove_dir_all(&before).unwrap();
    fs::remove_dir_all(&outside).unwrap();
    assert_eq!(output.status.code(), Some(3), "{output:?}");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("the two sides cannot be read alike"),
        "{stderr}"
    );
}
