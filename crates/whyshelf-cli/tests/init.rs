//! `init`, on the binary and an empty directory: `init --sample` makes a project
//! that `check` finds clean with no warning, that `map` draws as four books, that
//! `trace` follows from V-1 to F-1, and whose documents are in the form; a second
//! `init` exits 2 and changes no file; `init` alone makes a declaration `check` finds
//! clean. `--template` writes what its shorthand writes (`--sample`, and `minimal`
//! as `init` alone), `qa-first` a project `check` finds clean that takes a
//! verification of no decision, and a name that is none, or `--sample` with it, is
//! refused with exit 2 and nothing written.

mod common;

use std::fs;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use common::{Project, json};

/// How many directories this file's tests have made, to keep two apart.
static MADE: AtomicUsize = AtomicUsize::new(0);

/// An empty directory, removed when the test ends.
fn empty(name: &str) -> Project {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("a clock after 1970")
        .as_nanos();
    let count = MADE.fetch_add(1, Ordering::SeqCst);
    let dir = std::env::temp_dir().join(format!(
        "whyshelf-init-{name}-{}-{count}-{nanos}",
        std::process::id()
    ));
    fs::create_dir_all(&dir).unwrap();
    Project(dir)
}

fn stderr(output: &std::process::Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

#[test]
fn init_sample_makes_a_project_that_checks_clean_maps_four_books_and_traces_one_chain() {
    let project = empty("sample");
    let output = project.run(&["init", "--sample"]);
    assert_eq!(output.status.code(), Some(0), "{}", stderr(&output));
    let answer = json(&output);
    assert_eq!(answer["version"], 1);
    assert_eq!(answer["declaration"], ".whyshelf/books.toml");
    let keys: Vec<&str> = answer["documents"]
        .as_array()
        .unwrap()
        .iter()
        .map(|d| d["key"].as_str().unwrap())
        .collect();
    assert_eq!(
        keys,
        [
            "finding:F-1",
            "requirement:REQ-1",
            "decision:DEC-1",
            "verification:V-1"
        ]
    );
    assert_eq!(answer["documents"][3]["path"], "books/verification/V-1.md");
    for path in answer["documents"].as_array().unwrap() {
        fs::read(project.0.join(path["path"].as_str().unwrap())).unwrap();
    }
    let next: Vec<&str> = answer["next"]
        .as_array()
        .unwrap()
        .iter()
        .map(|n| n.as_str().unwrap())
        .collect();
    // The tests run with `--root <dir>`, and each next command carries it.
    let whyshelf = format!("whyshelf --root={}", project.0.display());
    assert_eq!(
        next[..2],
        [format!("{whyshelf} check"), format!("{whyshelf} map")]
    );
    assert!(
        next[2].starts_with(&format!(
            "{whyshelf} judge approve --pending F-1 V-1 --reason "
        )),
        "{next:?}"
    );

    let check = project.run(&["check"]);
    assert_eq!(check.status.code(), Some(0));
    assert_eq!(json(&check)["clean"], true);
    assert_eq!(json(&check)["uncovered"], serde_json::json!([]));
    assert_eq!(json(&check)["absent"], serde_json::json!([]));
    assert_eq!(stderr(&check), "", "check warns");

    let map = project.run(&["map"]);
    assert_eq!(map.status.code(), Some(0));
    assert_eq!(stderr(&map), "");
    let map = json(&map);
    let books: Vec<&str> = map["books"]
        .as_array()
        .unwrap()
        .iter()
        .map(|b| b["name"].as_str().unwrap())
        .collect();
    assert_eq!(
        books,
        ["finding", "requirement", "decision", "verification"]
    );

    let trace = project.run(&["trace", "V-1"]);
    assert_eq!(trace.status.code(), Some(0));
    let reached: Vec<String> = json(&trace)["upstream"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|edge| edge["to"]["doc"]["key"].as_str().map(str::to_owned))
        .collect();
    assert!(reached.contains(&"finding:F-1".to_owned()), "{reached:?}");

    assert_eq!(project.run(&["fmt", "--check"]).status.code(), Some(0));

    // Nothing is approved.
    let status = json(&project.run(&["status"]));
    assert!(
        status["docs"]
            .as_array()
            .unwrap()
            .iter()
            .all(|row| row["declared"] == "pending"),
        "{status}"
    );
}

#[test]
fn a_second_init_exits_2_names_what_is_in_the_way_and_changes_no_file() {
    let project = empty("again");
    assert_eq!(project.run(&["init", "--sample"]).status.code(), Some(0));
    let before = project.files();
    for args in [&["init", "--sample"][..], &["init"][..]] {
        let output = project.run(args);
        assert_eq!(output.status.code(), Some(2), "{args:?}");
        let answer = json(&output);
        assert_eq!(answer["error"], "request");
        assert_eq!(
            answer["problems"][0],
            ".whyshelf/books.toml is already there"
        );
        assert_eq!(project.files(), before, "{args:?}");
    }
    let answer = json(&project.run(&["init", "--sample"]));
    assert_eq!(answer["problems"].as_array().unwrap().len(), 5);
    assert_eq!(
        answer["problems"][4],
        "books/verification/V-1.md is already there"
    );
}

#[test]
fn a_document_file_in_the_way_refuses_and_writes_no_declaration() {
    let project = empty("doc-in-the-way");
    fs::create_dir_all(project.0.join("books/decision")).unwrap();
    fs::write(project.0.join("books/decision/DEC-1.md"), "mine\n").unwrap();
    let before = project.files();
    let output = project.run(&["init", "--sample"]);
    assert_eq!(output.status.code(), Some(2));
    assert_eq!(
        json(&output)["problems"],
        serde_json::json!(["books/decision/DEC-1.md is already there"])
    );
    assert_eq!(project.files(), before);
}

/// `init --sample` where something that is no file of the user's is at DEC-1's path:
/// refused with exit 2, naming the path, and nothing written.
fn refused_for(project: &Project) {
    let output = project.run(&["init", "--sample"]);
    assert_eq!(output.status.code(), Some(2), "{}", stderr(&output));
    assert_eq!(
        json(&output)["problems"],
        serde_json::json!(["books/decision/DEC-1.md is already there"])
    );
    // Nothing was written: the project holds `books/decision/DEC-1.md` alone (a link
    // to nothing cannot be read, so the entries are listed, not their bytes).
    let names = |dir: &str| -> Vec<String> {
        let mut names: Vec<String> = fs::read_dir(project.0.join(dir))
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        names
    };
    assert_eq!(names("."), ["books"]);
    assert_eq!(names("books"), ["decision"]);
    assert_eq!(names("books/decision"), ["DEC-1.md"]);
}

#[test]
fn a_directory_at_a_document_path_is_in_the_way() {
    let project = empty("dir-in-the-way");
    fs::create_dir_all(project.0.join("books/decision/DEC-1.md")).unwrap();
    refused_for(&project);
}

#[cfg(unix)]
#[test]
fn a_link_to_nothing_at_a_document_path_is_in_the_way() {
    let project = empty("link-in-the-way");
    fs::create_dir_all(project.0.join("books/decision")).unwrap();
    std::os::unix::fs::symlink(
        project.0.join("nowhere"),
        project.0.join("books/decision/DEC-1.md"),
    )
    .unwrap();
    refused_for(&project);
}

/// `init --sample` where something that is no directory is where `books/finding`
/// goes: refused with exit 2, naming it, and nothing written.
fn refused_for_the_finding_directory(project: &Project) {
    let output = project.run(&["init", "--sample"]);
    assert_eq!(output.status.code(), Some(2), "{}", stderr(&output));
    assert_eq!(
        json(&output)["problems"],
        serde_json::json!(["books/finding is already there"])
    );
    let mut names: Vec<String> = fs::read_dir(project.0.join("books"))
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    assert_eq!(names, ["finding"]);
    assert!(fs::read_dir(project.0.join(".whyshelf")).is_err());
}

#[test]
fn a_file_where_a_book_directory_goes_is_in_the_way() {
    let project = empty("file-as-dir");
    fs::create_dir_all(project.0.join("books")).unwrap();
    fs::write(project.0.join("books/finding"), "mine\n").unwrap();
    refused_for_the_finding_directory(&project);
}

#[cfg(unix)]
#[test]
fn a_link_to_nothing_where_a_book_directory_goes_is_in_the_way() {
    let project = empty("link-as-dir");
    fs::create_dir_all(project.0.join("books")).unwrap();
    std::os::unix::fs::symlink("/nowhere/at/all", project.0.join("books/finding")).unwrap();
    refused_for_the_finding_directory(&project);
}

#[test]
fn a_file_where_a_book_directory_goes_that_gets_no_document_is_in_the_way() {
    // qa-first declares `decision` and writes no decision; minimal writes no note.
    for (template, dir) in [("qa-first", "books/decision"), ("minimal", "books/note")] {
        let project = empty("file-as-empty-book");
        fs::create_dir_all(project.0.join("books")).unwrap();
        fs::write(project.0.join(dir), "mine\n").unwrap();
        let before = project.files();
        let output = project.run(&["init", "--template", template]);
        assert_eq!(
            output.status.code(),
            Some(2),
            "{template}: {}",
            stderr(&output)
        );
        let answer = json(&output);
        assert_eq!(answer["error"], "request");
        assert_eq!(
            answer["problems"],
            serde_json::json!([format!("{dir} is already there")]),
            "{template}"
        );
        assert_eq!(project.files(), before, "{template}");
    }
}

#[test]
fn the_next_commands_carry_a_root_that_starts_with_a_dash() {
    let parent = empty("dash-root");
    fs::create_dir_all(parent.0.join("-proj")).unwrap();
    let run = |args: &[&str]| {
        std::process::Command::new(env!("CARGO_BIN_EXE_whyshelf"))
            .current_dir(&parent.0)
            .args(args)
            .env("WHYSHELF_ACTOR", "tester")
            .output()
            .expect("the binary runs")
    };
    let output = run(&["--root=-proj", "init"]);
    assert_eq!(output.status.code(), Some(0), "{}", stderr(&output));
    let answer = json(&output);
    assert_eq!(answer["next"][0], "whyshelf --root=-proj check");
    // The command, as given, runs.
    let check = run(&["--root=-proj", "check"]);
    assert_eq!(check.status.code(), Some(0), "{}", stderr(&check));
    fs::read(parent.0.join("-proj/.whyshelf/books.toml")).unwrap();
}

#[test]
fn init_alone_writes_a_declaration_that_checks_clean() {
    let project = empty("minimal");
    let output = project.run(&["init"]);
    assert_eq!(output.status.code(), Some(0), "{}", stderr(&output));
    let answer = json(&output);
    assert_eq!(answer["documents"], serde_json::json!([]));
    let declaration = fs::read_to_string(project.0.join(".whyshelf/books.toml")).unwrap();
    assert!(declaration.contains("name = \"note\""));
    let check = project.run(&["check"]);
    assert_eq!(check.status.code(), Some(0));
    assert_eq!(json(&check)["clean"], true);
    assert_eq!(stderr(&check), "");
    // The next command it names makes a first note.
    let new = project.run(&["new", "note", "--title", "A first note"]);
    assert_eq!(new.status.code(), Some(0), "{}", stderr(&new));
}

/// What a project holds, each file by its path under the root, as text with its
/// `id` line left out (each run of `init` gives new identities), and the answer of
/// `init` with its `--root` left out.
fn written(project: &Project, answer: &serde_json::Value) -> (Vec<(String, String)>, String) {
    let files = project
        .files()
        .into_iter()
        .filter_map(|(path, bytes)| {
            let text = String::from_utf8(bytes?).unwrap();
            let text: Vec<&str> = text.lines().filter(|l| !l.starts_with("id: ")).collect();
            let path = path.strip_prefix(&project.0).unwrap().display().to_string();
            Some((path, text.join("\n")))
        })
        .collect();
    let answer = answer
        .to_string()
        .replace(&format!("--root={}", project.0.display()), "--root=");
    (files, answer)
}

#[test]
fn a_template_named_writes_what_its_shorthand_writes() {
    for (named, shorthand) in [
        (
            &["init", "--template", "sample"][..],
            &["init", "--sample"][..],
        ),
        (&["init", "--template", "minimal"][..], &["init"][..]),
    ] {
        let by_name = empty("by-name");
        let by_shorthand = empty("by-shorthand");
        let one = by_name.run(named);
        let other = by_shorthand.run(shorthand);
        assert_eq!(one.status.code(), Some(0), "{}", stderr(&one));
        assert_eq!(other.status.code(), Some(0), "{}", stderr(&other));
        assert_eq!(
            written(&by_name, &json(&one)),
            written(&by_shorthand, &json(&other)),
            "{named:?}"
        );
    }
}

#[test]
fn init_qa_first_checks_clean_with_requirements_listed_and_takes_a_verification_of_no_decision() {
    let project = empty("qa-first");
    let output = project.run(&["init", "--template", "qa-first"]);
    assert_eq!(output.status.code(), Some(0), "{}", stderr(&output));
    let answer = json(&output);
    let keys: Vec<&str> = answer["documents"]
        .as_array()
        .unwrap()
        .iter()
        .map(|d| d["key"].as_str().unwrap())
        .collect();
    assert_eq!(
        keys,
        ["finding:F-1", "requirement:REQ-1", "verification:V-1"]
    );
    // No `depends` link stands on any of the three, so `next` names each.
    let whyshelf = format!("whyshelf --root={}", project.0.display());
    assert_eq!(
        answer["next"][2],
        format!("{whyshelf} judge approve --pending F-1 REQ-1 V-1 --reason \"read, and it holds\"")
    );
    let declaration = fs::read_to_string(project.0.join(".whyshelf/books.toml")).unwrap();
    assert!(declaration.contains("count = \"many\""));

    let check = project.run(&["check"]);
    assert_eq!(check.status.code(), Some(0), "{}", stderr(&check));
    let answer = json(&check);
    assert_eq!(answer["clean"], true);
    assert_eq!(answer["absent"], serde_json::json!([]));
    assert_eq!(answer["uncovered"].as_array().unwrap().len(), 1);
    assert_eq!(answer["uncovered"][0]["doc"]["key"], "requirement:REQ-1");
    assert_eq!(answer["uncovered"][0]["severity"], "warn");
    assert_eq!(
        stderr(&check),
        "warning: requirement:REQ-1 is reached by no decision.serves (cover = \"warn\")\n"
    );
    assert_eq!(project.run(&["fmt", "--check"]).status.code(), Some(0));

    // A verification that checks no decision is valid there.
    let new = project.run(&[
        "new",
        "verification",
        "--title",
        "Save twice at once",
        "method=review",
    ]);
    assert_eq!(new.status.code(), Some(0), "{}", stderr(&new));
    assert_eq!(json(&new)["doc"]["key"], "verification:V-2");
    let check = project.run(&["check"]);
    assert_eq!(check.status.code(), Some(0), "{}", stderr(&check));
    assert_eq!(json(&check)["health"]["invalid"], serde_json::json!([]));
}

#[test]
fn an_unknown_template_or_one_with_sample_is_refused_with_exit_2_and_nothing_written() {
    let project = empty("unknown-template");
    let before = project.files();
    let output = project.run(&["init", "--template", "qa_first"]);
    assert_eq!(output.status.code(), Some(2));
    let answer = json(&output);
    assert_eq!(answer["error"], "request");
    let message = answer["messages"][0].as_str().unwrap();
    for name in ["minimal", "sample", "qa-first", "experiment"] {
        assert!(message.contains(name), "{message}");
    }
    assert_eq!(project.files(), before);

    let output = project.run(&["init", "--sample", "--template", "qa-first"]);
    assert_eq!(output.status.code(), Some(2));
    let answer = json(&output);
    assert_eq!(answer["error"], "request");
    assert!(
        answer["messages"][0]
            .as_str()
            .unwrap()
            .contains("cannot be used with"),
        "{answer}"
    );
    assert_eq!(project.files(), before);
}
