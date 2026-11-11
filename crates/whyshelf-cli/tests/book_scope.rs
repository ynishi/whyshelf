//! The range of a book, on the binary: a book brought into a directory that already
//! holds files that are no documents. `include` and `exclude` say which files are the
//! book's, `without_front_matter = "skip"` says what a file with no front matter is,
//! and `check` tells what was skipped.

mod common;

use std::fs;

use common::{BOOKS, NOTE, Project, json};

/// The project: a book `note` over `notes/` with one real document (`N-1`), three
/// Markdown files with no front matter beside it, and two more in `notes/archive/`.
fn crowded(name: &str, settings: &str) -> Project {
    let project = Project::with(name, &format!("{BOOKS}{settings}"), NOTE);
    let notes = project.0.join("notes");
    for file in ["readme.md", "todo.md", "scratch.md"] {
        fs::write(notes.join(file), "Just notes, no front matter.\n").unwrap();
    }
    fs::create_dir_all(notes.join("archive")).unwrap();
    for file in ["a.md", "b.md"] {
        fs::write(notes.join("archive").join(file), "# Archive\n\ntext\n").unwrap();
    }
    project
}

/// The settings, written under the `[[book]]` of `BOOKS` (before its fields, which
/// TOML would take for the field's own keys): appended to the book's header.
fn declared(settings: &str) -> String {
    BOOKS.replacen(
        "key = { prefix = \"N\" }\n",
        &format!("key = {{ prefix = \"N\" }}\n{settings}"),
        1,
    )
}

fn crowded_with(name: &str, settings: &str) -> Project {
    let project = crowded(name, "");
    fs::write(project.0.join(".whyshelf/books.toml"), declared(settings)).unwrap();
    project
}

fn stderr(output: &std::process::Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

#[test]
fn with_no_setting_every_file_without_a_front_matter_is_invalid() {
    let project = crowded("scope-default", "");
    let out = project.run(&["check"]);
    assert_eq!(out.status.code(), Some(1));
    let answer = json(&out);
    assert_eq!(answer["clean"], false);
    let book = &answer["books"][0];
    assert_eq!(
        (book["valid"].as_u64(), book["invalid"].as_u64()),
        (Some(1), Some(5))
    );
    assert_eq!(book["skipped"], 0);
    assert_eq!(book["skipped_files"], serde_json::json!([]));
    assert!(!stderr(&out).contains("note:"), "{}", stderr(&out));
}

#[test]
fn an_exclude_and_skip_make_the_check_clean_and_tell_what_was_skipped() {
    let project = crowded_with(
        "scope-skip",
        "exclude = [\"archive/**\"]\nwithout_front_matter = \"skip\"\n",
    );
    let out = project.run(&["check"]);
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    let answer = json(&out);
    assert_eq!(answer["clean"], true);
    let book = &answer["books"][0];
    assert_eq!(
        (book["valid"].as_u64(), book["invalid"].as_u64()),
        (Some(1), Some(0))
    );
    assert_eq!(book["skipped"], 3);
    assert_eq!(
        book["skipped_files"],
        serde_json::json!([
            {"path": "notes/readme.md", "why": "no_front_matter"},
            {"path": "notes/scratch.md", "why": "no_front_matter"},
            {"path": "notes/todo.md", "why": "no_front_matter"},
        ])
    );
    // The two files of `archive/` are out of the range: neither read nor listed.
    assert!(!answer.to_string().contains("archive"), "{answer}");
    assert_eq!(
        stderr(&out).lines().collect::<Vec<_>>(),
        ["note: book note skipped 3 files with no front matter"]
    );

    // The skipped files are no documents: `status` lists the one real document, and
    // `fmt --check` has nothing to say about the others.
    let status = json(&project.run(&["status"]));
    assert_eq!(status["docs"].as_array().map(Vec::len), Some(1), "{status}");
    let fmt = project.run(&["fmt", "--check"]);
    assert_eq!(fmt.status.code(), Some(0), "{}", json(&fmt));

    // The map shows the range.
    let map = json(&project.run(&["map"]));
    assert_eq!(
        map["books"][0]["exclude"],
        serde_json::json!(["archive/**"])
    );
    // `include` is not written: null, as every field not written is.
    assert_eq!(
        map["books"][0].get("include"),
        Some(&serde_json::Value::Null),
        "{map}"
    );
    assert_eq!(map["books"][0]["without_front_matter"], "skip");
}

#[test]
fn skip_alone_skips_the_files_of_the_subdirectory_too() {
    let project = crowded_with("scope-skip-only", "without_front_matter = \"skip\"\n");
    let out = project.run(&["check"]);
    // The two files of `archive/` have no front matter either, and are skipped too.
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(json(&out)["books"][0]["skipped"], 5);
    assert_eq!(
        stderr(&out).trim(),
        "note: book note skipped 5 files with no front matter"
    );
}

#[test]
fn a_front_matter_that_is_broken_is_still_invalid_when_files_with_none_are_skipped() {
    let project = crowded_with(
        "scope-broken",
        "exclude = [\"archive/**\"]\nwithout_front_matter = \"skip\"\n",
    );
    let notes = project.0.join("notes");
    // Opened and never closed; a mark before the opening line; YAML that is no
    // mapping; a `----` for the opening.
    fs::write(notes.join("unclosed.md"), "---\nid: x\ntitle: t\n").unwrap();
    fs::write(notes.join("bom.md"), "\u{feff}---\nid: x\n---\n").unwrap();
    fs::write(notes.join("list.md"), "---\n- a\n---\n").unwrap();
    fs::write(notes.join("dashes.md"), "----\nid: x\n----\n").unwrap();
    let out = project.run(&["check"]);
    assert_eq!(out.status.code(), Some(1));
    let answer = json(&out);
    let book = &answer["books"][0];
    assert_eq!(
        (book["valid"].as_u64(), book["invalid"].as_u64()),
        (Some(1), Some(4))
    );
    assert_eq!(book["skipped"], 3);
    let text = answer["health"]["invalid"].to_string();
    for file in ["unclosed.md", "bom.md", "list.md", "dashes.md"] {
        assert!(text.contains(file), "{file} is not told invalid: {text}");
    }
    assert!(
        stderr(&out).contains("note: book note skipped 3 files with no front matter"),
        "{}",
        stderr(&out)
    );
}

#[test]
fn what_an_exclude_leaves_out_is_not_looked_at_even_when_it_could_not_be_read() {
    let project = crowded_with(
        "scope-unlooked",
        "exclude = [\"archive/**\"]\nwithout_front_matter = \"skip\"\n",
    );
    let archive = project.0.join("notes/archive");
    // A name that is no path of the project (on Unix, a backslash is a character of
    // a name), and a directory no one may read.
    fs::write(archive.join("a\\b.md"), "x\n").unwrap();
    let locked = archive.join("locked");
    fs::create_dir_all(&locked).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&locked, fs::Permissions::from_mode(0o000)).unwrap();
    }
    let out = project.run(&["check"]);
    let fmt = project.run(&["fmt", "--check"]);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&locked, fs::Permissions::from_mode(0o755)).unwrap();
    }
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    assert_eq!(json(&out)["clean"], true);
    assert_eq!(fmt.status.code(), Some(0), "{}", stderr(&fmt));

    // In the range, the same name fails the run, as it did before a range.
    fs::write(project.0.join("notes/a\\b.md"), "x\n").unwrap();
    let out = project.run(&["check"]);
    assert_eq!(out.status.code(), Some(3), "{}", stderr(&out));
}

#[test]
fn a_thematic_break_in_the_middle_of_a_file_is_no_front_matter() {
    let project = crowded_with("scope-hr", "without_front_matter = \"skip\"\n");
    fs::write(
        project.0.join("notes/hr.md"),
        "# Title\n\nsome text\n\n---\n\nid: not-a-front-matter\n",
    )
    .unwrap();
    let out = project.run(&["check"]);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(json(&out)["books"][0]["skipped"], 6);
}

#[test]
fn a_range_that_is_not_sound_is_a_problem_of_the_declaration() {
    for (settings, text) in [
        ("exclude = [\"a/[b\"]\n", "a/[b"),
        ("include = [\"../x/*.md\"]\n", "../x/*.md"),
        ("include = [\"/abs/*.md\"]\n", "/abs/*.md"),
        ("without_front_matter = \"ignore\"\n", "ignore"),
    ] {
        let project = crowded_with("scope-bad", settings);
        let out = project.run(&["check"]);
        assert_eq!(out.status.code(), Some(1), "{settings}");
        let answer = json(&out);
        let problems = answer["schema_problems"].to_string();
        assert!(
            problems.contains("`note`") || problems.contains("book `note`"),
            "{problems}"
        );
        assert!(problems.contains(text), "{settings}: {problems}");
    }
}

#[test]
fn new_into_a_path_the_range_does_not_take_is_refused_and_writes_nothing() {
    let project = crowded_with(
        "scope-new",
        "exclude = [\"N-2.md\", \"archive/**\"]\nwithout_front_matter = \"skip\"\n",
    );
    let before = project.files();
    let out = project.run(&["new", "note", "--title", "Second", "--key", "N-2"]);
    assert_eq!(out.status.code(), Some(2), "{}", stderr(&out));
    // The refusal names the book, the file and the patterns.
    assert_eq!(
        json(&out)["messages"],
        serde_json::json!([
            "the key cannot be the new document's",
            "the book `note` does not take `notes/N-2.md`, the file of the key `N-2`: \
             include every `.md` file (not written); exclude `N-2.md`, `archive/**`",
        ])
    );
    assert_eq!(json(&out)["error"], "request");
    assert_eq!(project.files(), before);
    // A key the range takes is written as always.
    let ok = project.run(&["new", "note", "--title", "Third", "--key", "N-3"]);
    assert_eq!(ok.status.code(), Some(0), "{}", json(&ok));
    assert!(project.0.join("notes/N-3.md").exists());
}

#[test]
fn a_link_to_a_key_whose_file_is_excluded_resolves_as_no_document() {
    let books = format!(
        "{}[[book.link]]\nname = \"see\"\ntarget = \"note\"\nmeans = \"informs\"\n",
        declared("exclude = [\"hidden/**\"]\n")
    );
    let project = Project::with("scope-link", &books, NOTE);
    let notes = project.0.join("notes");
    fs::create_dir_all(notes.join("hidden")).unwrap();
    fs::write(
        notes.join("hidden/N-2.md"),
        "---\nid: \"01a1c4f0-0000-7000-8000-000000000002\"\nkey: \"N-2\"\ntitle: \"Hidden\"\n---\n",
    )
    .unwrap();
    fs::write(
        notes.join("N-3.md"),
        "---\nid: \"01a1c4f0-0000-7000-8000-000000000003\"\nkey: \"N-3\"\ntitle: \"Links out\"\nsee: [\"N-2\"]\n---\n",
    )
    .unwrap();
    let out = project.run(&["check"]);
    assert_eq!(out.status.code(), Some(0), "{}", json(&out));
    // `N-2` is no document of the book: only N-1 and N-3 are counted.
    assert_eq!(json(&out)["books"][0]["valid"], 2);
    // The link to `N-2` resolves to nothing: no document has the key, exactly as
    // a link to a key no file writes.
    let got = json(&project.run(&["get", "N-3"]));
    assert_eq!(
        got["content"]["links"],
        serde_json::json!([{
            "name": "see",
            "targets": [{
                "state": "unresolved",
                "why": "`N-2` cannot be followed: no document has this key",
                "written": "N-2",
            }],
        }])
    );
    assert_eq!(got["row"]["notices"][0]["kind"], "informs_unresolved");
    assert_eq!(got["health"]["clean"], true);
    let missing = project.run(&["get", "N-2"]);
    assert_eq!(missing.status.code(), Some(2));
}
