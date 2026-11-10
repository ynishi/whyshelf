//! The promises of the command line, on the binary and a small project of files:
//! every answer is JSON with its version, a wrong argument included; reading
//! commands create and change no file; `check` exits 1 when it finds something
//! wrong, and `map` when the declaration cannot be used; a gap of a `warn` cover
//! leaves `check` at 0, listed in the answer and warned of on stderr; `unlinked`
//! lists the documents no link reaches and those that hold none; `WHYSHELF_ACTOR`
//! names who judges.
//!
//! Each project is made under the system's temporary directory with the standard
//! library only, and removed when the test ends. One test reads this repository's
//! own books, with a reading command only.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::time::{SystemTime, UNIX_EPOCH};

const BOOKS: &str = r#"[[book]]
name = "note"
dir = "notes"
key = { prefix = "N" }
[[book.field]]
name = "title"
type = "text"
required = true
"#;

const NOTE: &str = "---\nid: \"01a1c4f0-0000-7000-8000-000000000001\"\nkey: \"N-1\"\ntitle: \"A note\"\n---\nThe body.\n";

/// A project directory that is removed when it goes out of scope.
struct Project(PathBuf);

impl Project {
    fn new(name: &str) -> Self {
        Self::with(name, BOOKS, NOTE)
    }

    /// A project with this declaration of books and this text as `notes/N-1.md`.
    fn with(name: &str, books: &str, note: &str) -> Self {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("a clock after 1970")
            .as_nanos();
        let dir =
            std::env::temp_dir().join(format!("whyshelf-{name}-{}-{nanos}", std::process::id()));
        fs::create_dir_all(dir.join(".whyshelf")).unwrap();
        fs::create_dir_all(dir.join("notes")).unwrap();
        fs::write(dir.join(".whyshelf/books.toml"), books).unwrap();
        fs::write(dir.join("notes/N-1.md"), note).unwrap();
        Self(dir)
    }

    fn run(&self, args: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_whyshelf"))
            .arg("--root")
            .arg(&self.0)
            .args(args)
            .env("WHYSHELF_ACTOR", "tester")
            .output()
            .expect("the binary runs")
    }

    /// Every file under the project with its bytes, and every directory (with none),
    /// so that a directory made empty counts too.
    fn files(&self) -> BTreeMap<PathBuf, Option<Vec<u8>>> {
        fn walk(dir: &Path, out: &mut BTreeMap<PathBuf, Option<Vec<u8>>>) {
            for entry in fs::read_dir(dir).unwrap() {
                let path = entry.unwrap().path();
                // A directory is what can be listed (the workspace's arch-lint refuses
                // `is_dir`, a blocking call, in every file).
                if fs::read_dir(&path).is_ok() {
                    out.insert(path.clone(), None);
                    walk(&path, out);
                } else {
                    out.insert(path.clone(), Some(fs::read(&path).unwrap()));
                }
            }
        }
        let mut out = BTreeMap::new();
        walk(&self.0, &mut out);
        out
    }
}

impl Drop for Project {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn json(output: &Output) -> serde_json::Value {
    serde_json::from_slice(&output.stdout).unwrap_or_else(|e| {
        panic!(
            "not JSON ({e}): {}",
            String::from_utf8_lossy(&output.stdout)
        )
    })
}

const READS: [&[&str]; 11] = [
    &["check"],
    &["map"],
    &["status"],
    &["unlinked"],
    &["stale"],
    &["stale", "--exit-code"],
    &["get", "N-1"],
    &["trace", "N-1"],
    &["get", "N-1", "--approved"],
    &["diff", "N-1"],
    &["schema"],
];

#[test]
fn every_answer_is_json_with_its_version() {
    let project = Project::new("json");
    assert_eq!(
        project.run(&["judge", "approve", "N-1"]).status.code(),
        Some(0)
    );
    for args in READS {
        let output = project.run(args);
        assert_eq!(json(&output)["version"], 1, "{args:?}");
    }
}

#[test]
fn a_wrong_argument_is_the_error_json_of_a_request() {
    let project = Project::new("args");
    for args in [&["bogus"][..], &["get"], &["stale", "--nope"], &[]] {
        let output = project.run(args);
        assert_eq!(output.status.code(), Some(2), "{args:?}");
        let answer = json(&output);
        assert_eq!(answer["error"], "request", "{args:?}");
        assert_eq!(answer["version"], 1, "{args:?}");
        assert!(
            !answer["messages"][0].as_str().unwrap().is_empty(),
            "{args:?}"
        );
        assert!(
            !output.stderr.is_empty(),
            "the text goes to stderr: {args:?}"
        );
    }
    for args in [&["--help"][..], &["-h"], &["--version"], &["-V"]] {
        let output = project.run(args);
        assert_eq!(output.status.code(), Some(0), "{args:?}");
        assert!(!output.stdout.is_empty(), "text on stdout: {args:?}");
        assert!(output.stderr.is_empty(), "nothing on stderr: {args:?}");
        assert!(serde_json::from_slice::<serde_json::Value>(&output.stdout).is_err());
    }
    // There is no `help` command: it is a command that does not exist.
    let output = project.run(&["help"]);
    assert_eq!(output.status.code(), Some(2));
    assert_eq!(json(&output)["error"], "request");
    // A value missing for `--root` names it, and no hint is left in the message.
    let output = Command::new(env!("CARGO_BIN_EXE_whyshelf"))
        .arg("--root")
        .output()
        .unwrap();
    let message = json(&output)["messages"][0].as_str().unwrap().to_owned();
    assert!(message.contains("--root"), "{message}");
    assert!(!message.contains("For more information"), "{message}");
}

#[test]
fn no_argument_at_all_is_the_error_json_of_a_request() {
    let project = Project::new("bare");
    let output = Command::new(env!("CARGO_BIN_EXE_whyshelf"))
        .current_dir(&project.0)
        .output()
        .expect("the binary runs");
    assert_eq!(output.status.code(), Some(2));
    let answer = json(&output);
    assert_eq!(answer["error"], "request");
    assert_eq!(answer["messages"][0], "a command is needed (see --help)");
    assert!(!output.stderr.is_empty(), "the help goes to stderr");
}

#[test]
fn reading_commands_create_and_change_no_file() {
    let project = Project::new("reads");
    // Before any approval: no ledger directory exists, and none may be made.
    let before = project.files();
    for args in READS {
        project.run(args);
    }
    assert_eq!(project.files(), before, "before an approval");
    assert!(!project.0.join(".whyshelf/ledger").exists());

    assert_eq!(
        project.run(&["judge", "approve", "N-1"]).status.code(),
        Some(0)
    );
    let before = project.files();
    for args in READS {
        project.run(args);
    }
    assert_eq!(project.files(), before, "after an approval");
}

#[test]
fn check_exits_1_when_it_finds_something_wrong() {
    let project = Project::new("check");
    assert_eq!(project.run(&["check"]).status.code(), Some(0));
    fs::write(
        project.0.join("notes/N-2.md"),
        "---\nkey: \"N-2\"\n---\nno id\n",
    )
    .unwrap();
    let output = project.run(&["check"]);
    assert_eq!(output.status.code(), Some(1));
    assert_eq!(json(&output)["clean"], false);
}

#[test]
fn a_warn_cover_lists_the_document_it_does_not_reach_and_leaves_check_clean() {
    // Notes see other notes, and every note is expected to be seen; N-1 is alone.
    let covering = |cover: &str| {
        format!(
            "{BOOKS}[[book.link]]\nname = \"see\"\ntarget = \"note\"\nmeans = \"informs\"\ncover = {cover}\n"
        )
    };

    let project = Project::with("cover-warn", &covering("\"warn\""), NOTE);
    let output = project.run(&["check"]);
    assert_eq!(output.status.code(), Some(0));
    let check = json(&output);
    assert_eq!(check["clean"], true, "{check}");
    assert_eq!(
        check["uncovered"],
        serde_json::json!([{
            "doc": { "id": "01a1c4f0-0000-7000-8000-000000000001", "key": "note:N-1" },
            "book": "note",
            "link": "see",
            "severity": "warn",
        }])
    );
    // The gap is warned of on stderr, one line; stdout is the JSON answer alone.
    assert_eq!(
        String::from_utf8_lossy(&output.stderr),
        "warning: note:N-1 is reached by no note.see (cover = \"warn\")\n"
    );
    assert!(!String::from_utf8_lossy(&output.stdout).contains("warning:"));
    let map = json(&project.run(&["map"]));
    assert_eq!(map["links"][0]["cover"], true);
    assert_eq!(map["links"][0]["cover_severity"], "warn");

    let project = Project::with("cover-error", &covering("true"), NOTE);
    let output = project.run(&["check"]);
    assert_eq!(output.status.code(), Some(1));
    let check = json(&output);
    assert_eq!(check["clean"], false);
    assert_eq!(check["uncovered"][0]["severity"], "error");
    // An error gap fails the answer, and writes no warning.
    assert_eq!(String::from_utf8_lossy(&output.stderr), "");
}

#[test]
fn map_answers_the_books_the_links_and_their_drawing() {
    let books = BOOKS.replace(
        "key = { prefix = \"N\" }\n",
        "key = { prefix = \"N\" }\ndescription = \"what was noted\"\n",
    ) + "[[book.link]]\nname = \"see\"\ntarget = \"note\"\nmeans = \"informs\"\n";
    let project = Project::with("map", &books, NOTE);
    let output = project.run(&["map"]);
    assert_eq!(output.status.code(), Some(0));
    let answer = json(&output);
    assert_eq!(answer["version"], 1);
    assert_eq!(answer["schema_problems"], serde_json::json!([]));
    assert_eq!(answer["books"][0]["name"], "note");
    assert_eq!(answer["books"][0]["description"], "what was noted");
    assert_eq!(answer["links"][0]["name"], "see");
    assert_eq!(answer["links"][0]["target"]["book"], "note");
    assert_eq!(
        answer["diagram"],
        "note  N-  what was noted\n└─ see  informs  0..  ──► note (own book)\n"
    );
}

#[test]
fn map_of_a_declaration_that_cannot_be_used_is_its_problems_as_check_gives_them() {
    let books = BOOKS.replace(
        "key = { prefix = \"N\" }\n",
        "key = { prefix = \"N\" }\ndescription = \"\"\"\ntwo\nlines\"\"\"\n",
    );
    let project = Project::with("map-unusable", &books, NOTE);
    let check = project.run(&["check"]);
    let output = project.run(&["map"]);
    assert_eq!(output.status.code(), check.status.code());
    assert_eq!(output.status.code(), Some(1));
    let answer = json(&output);
    assert_eq!(answer["version"], 1);
    assert_eq!(answer["schema_problems"], json(&check)["schema_problems"]);
    assert!(
        answer["schema_problems"][0]
            .as_str()
            .unwrap()
            .contains("more than one line"),
        "{answer}"
    );
    assert_eq!(answer["diagram"], serde_json::Value::Null);
    assert_eq!(answer["books"], serde_json::json!([]));
}

/// The keys of a list of `unlinked`, or none when it is null.
fn unlinked_keys(list: &serde_json::Value) -> Option<Vec<String>> {
    list.as_array().map(|entries| {
        entries
            .iter()
            .map(|e| e["row"]["doc"]["key"].as_str().unwrap().to_owned())
            .collect()
    })
}

#[test]
fn unlinked_isolates_a_lone_document_and_lists_a_linked_pair_as_each_fits() {
    // A note may answer a topic, and a topic is about notes.
    let books = format!(
        "{BOOKS}[[book.link]]\nname = \"answers\"\ntarget = \"topic\"\nmeans = \"informs\"\n\
         [[book]]\nname = \"topic\"\ndir = \"topics\"\nkey = {{ prefix = \"T\" }}\n\
         [[book.field]]\nname = \"title\"\ntype = \"text\"\nrequired = true\n\
         [[book.link]]\nname = \"about\"\ntarget = \"note\"\nmeans = \"informs\"\n"
    );
    let project = Project::with("unlinked", &books, NOTE);
    fs::create_dir_all(project.0.join("topics")).unwrap();
    let output = project.run(&["unlinked"]);
    assert_eq!(output.status.code(), Some(0));
    let answer = json(&output);
    assert_eq!(answer["version"], 1);
    let none = Some(Vec::<String>::new());
    assert_eq!(
        unlinked_keys(&answer["isolated"]),
        Some(vec!["note:N-1".to_owned()])
    );
    assert_eq!(answer["isolated"][0]["book"], "note");
    assert_eq!(answer["isolated"][0]["row"]["standing"], "pending");
    // The lists are disjoint: an isolated document is in no other.
    assert_eq!(unlinked_keys(&answer["no_incoming"]), none);
    assert_eq!(unlinked_keys(&answer["no_outgoing"]), none);
    assert_eq!(unlinked_keys(&answer["decided_against"]["isolated"]), none);
    assert_eq!(answer["health"]["clean"], true);

    fs::write(
        project.0.join("topics/T-1.md"),
        "---\nid: \"01a1c4f0-0000-7000-8000-000000000002\"\nkey: \"T-1\"\ntitle: \"A topic\"\nabout: [N-1]\n---\nIt is about N-1.\n",
    )
    .unwrap();
    let answer = json(&project.run(&["unlinked"]));
    assert_eq!(unlinked_keys(&answer["isolated"]), none);
    assert_eq!(
        unlinked_keys(&answer["no_incoming"]),
        Some(vec!["topic:T-1".to_owned()])
    );
    assert_eq!(
        unlinked_keys(&answer["no_outgoing"]),
        Some(vec!["note:N-1".to_owned()])
    );

    let answer = json(&project.run(&["unlinked", "--no-outgoing"]));
    assert_eq!(answer["no_incoming"], serde_json::Value::Null);
    assert_eq!(
        answer["decided_against"]["no_incoming"],
        serde_json::Value::Null
    );
    assert_eq!(
        unlinked_keys(&answer["no_outgoing"]),
        Some(vec!["note:N-1".to_owned()])
    );
    assert_eq!(unlinked_keys(&answer["isolated"]), none);

    let answer = json(&project.run(&["unlinked", "--no-incoming", "--book", "topic"]));
    assert_eq!(
        unlinked_keys(&answer["no_incoming"]),
        Some(vec!["topic:T-1".to_owned()])
    );
    assert_eq!(answer["no_outgoing"], serde_json::Value::Null);

    let output = project.run(&["unlinked", "--book", "nothing"]);
    assert_eq!(output.status.code(), Some(2));
    let answer = json(&output);
    assert_eq!(answer["error"], "request");
    assert_eq!(answer["messages"][0], "there is no book `nothing`");
}

#[test]
fn unlinked_leaves_out_a_book_its_declaration_cuts_off() {
    // The one book declares no link and no link points into it.
    let project = Project::new("unlinked-bare");
    for args in [
        &["unlinked"][..],
        &["unlinked", "--no-incoming"],
        &["unlinked", "--no-outgoing"],
    ] {
        let answer = json(&project.run(args));
        assert_eq!(answer["isolated"], serde_json::json!([]), "{args:?}");
    }
}

#[test]
fn unlinked_of_a_declaration_that_cannot_be_used_is_its_problems_with_the_lists_asked_for() {
    let books = BOOKS.replace(
        "key = { prefix = \"N\" }\n",
        "key = { prefix = \"N\" }\ndescription = \"\"\"\ntwo\nlines\"\"\"\n",
    );
    let project = Project::with("unlinked-unusable", &books, NOTE);
    let check = project.run(&["check"]);
    let output = project.run(&["unlinked", "--no-incoming"]);
    assert_eq!(output.status.code(), Some(1));
    let answer = json(&output);
    assert_eq!(answer["schema_problems"], json(&check)["schema_problems"]);
    assert_eq!(answer["isolated"], serde_json::json!([]));
    assert_eq!(answer["no_incoming"], serde_json::json!([]));
    assert_eq!(answer["no_outgoing"], serde_json::Value::Null);
    assert_eq!(answer["decided_against"]["isolated"], serde_json::json!([]));
    assert_eq!(
        answer["decided_against"]["no_incoming"],
        serde_json::json!([])
    );
    assert_eq!(
        answer["decided_against"]["no_outgoing"],
        serde_json::Value::Null
    );
    assert_eq!(answer["health"], serde_json::Value::Null);
}

/// Run the binary on this repository's root, read-only.
fn on_this_repository(args: &[&str]) -> serde_json::Value {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let output = Command::new(env!("CARGO_BIN_EXE_whyshelf"))
        .arg("--root")
        .arg(&root)
        .args(args)
        .output()
        .expect("the binary runs");
    assert_eq!(output.status.code(), Some(0), "{args:?}");
    json(&output)
}

#[test]
fn unlinked_on_this_repository_agrees_with_trace_and_lists_each_document_once() {
    let answer = on_this_repository(&["unlinked"]);
    let lists = [
        &answer["isolated"],
        &answer["no_incoming"],
        &answer["no_outgoing"],
        &answer["decided_against"]["isolated"],
        &answer["decided_against"]["no_incoming"],
        &answer["decided_against"]["no_outgoing"],
    ];
    let mut seen = std::collections::BTreeSet::new();
    for list in lists {
        for key in unlinked_keys(list).expect("every list is asked for") {
            assert!(seen.insert(key.clone()), "{key} is in two lists");
        }
    }
    let isolated = unlinked_keys(&answer["isolated"])
        .into_iter()
        .chain(unlinked_keys(&answer["decided_against"]["isolated"]))
        .flatten();
    for key in isolated {
        let traced = on_this_repository(&["trace", &key]);
        assert_eq!(traced["upstream"], serde_json::json!([]), "{key}");
        assert_eq!(traced["downstream"], serde_json::json!([]), "{key}");
    }
}

#[test]
fn the_actor_comes_from_the_variable() {
    let project = Project::new("actor");
    let output = project.run(&["judge", "approve", "N-1"]);
    assert_eq!(output.status.code(), Some(0));
    let answer = json(&output);
    assert_eq!(answer["version"], 1);
    assert_eq!(answer["judged"][0]["record"]["actor"], "tester");
}

#[test]
fn a_path_that_names_no_file_is_told_and_case_counts() {
    let books = format!(
        "{BOOKS}[[book.link]]\nname = \"files\"\ntarget = \"artifact\"\nspace = \"file\"\nmeans = \"depends\"\n"
    );
    let note = NOTE.replace(
        "title: \"A note\"\n",
        "title: \"A note\"\nfiles: [\"Justfile\", \"justfile\", \"later.rs\"]\n",
    );
    let project = Project::with("absent", &books, &note);
    fs::write(project.0.join("Justfile"), "check:\n").unwrap();

    let told = |answer: &serde_json::Value, field: &str| -> Vec<String> {
        answer[field]
            .as_array()
            .unwrap()
            .iter()
            .map(|a| a["path"].as_str().unwrap().to_owned())
            .collect()
    };
    let check = json(&project.run(&["check"]));
    assert_eq!(check["clean"], true, "absent paths leave the check clean");
    assert_eq!(told(&check, "absent"), ["justfile", "later.rs"]);
    assert_eq!(check["absent"][0]["space"], "file");
    assert_eq!(check["unknown"], serde_json::json!([]));
    assert!(
        check["absent"][0]["detail"]
            .as_str()
            .unwrap()
            .contains("has nothing behind it now")
    );
    assert_eq!(check["absent"][0]["name"], "justfile");

    let approved = json(&project.run(&["judge", "approve", "N-1"]));
    assert_eq!(
        told(&approved["judged"][0], "absent"),
        ["justfile", "later.rs"]
    );

    let in_notices = |notices: &serde_json::Value| -> Vec<String> {
        notices
            .as_array()
            .unwrap()
            .iter()
            .filter(|n| n["kind"] == "artifact_absent")
            .map(|n| n["artifact"].as_str().unwrap().to_owned())
            .collect()
    };
    let got = json(&project.run(&["get", "N-1"]));
    assert_eq!(in_notices(&got["row"]["notices"]), ["justfile", "later.rs"]);
    let status = json(&project.run(&["status"]));
    assert_eq!(
        in_notices(&status["docs"][0]["notices"]),
        ["justfile", "later.rs"]
    );
}

#[test]
fn stale_names_where_the_degradations_start() {
    let project = Project::new("origins");
    assert_eq!(
        json(&project.run(&["stale"]))["origins"],
        serde_json::json!([])
    );
    assert_eq!(
        project.run(&["judge", "approve", "N-1"]).status.code(),
        Some(0)
    );
    fs::write(
        project.0.join("notes/N-1.md"),
        NOTE.replace("The body.", "An edited body."),
    )
    .unwrap();
    let origins = json(&project.run(&["stale"]))["origins"].clone();
    assert_eq!(origins.as_array().map(Vec::len), Some(1), "{origins}");
    assert_eq!(origins[0]["start"]["kind"], "doc");
    assert_eq!(origins[0]["start"]["doc"]["key"], "note:N-1");
    assert_eq!(origins[0]["degraded"][0]["key"], "note:N-1");
}

#[test]
fn every_row_shows_the_decision_that_stands() {
    let project = Project::new("decision");
    let row = |args: &[&str]| json(&project.run(args))["row"].clone();
    assert_eq!(row(&["get", "N-1"])["decision"], serde_json::Value::Null);

    let output = project.run(&["judge", "approve", "N-1", "--reason", "read it whole"]);
    assert_eq!(output.status.code(), Some(0));
    let record = json(&output)["judged"][0]["record"].clone();
    let status = json(&project.run(&["status"]));
    let in_status = status["docs"][0]["decision"].clone();
    for (args, decision) in [
        (
            &["get", "N-1"][..],
            row(&["get", "N-1"])["decision"].clone(),
        ),
        (
            &["get", "N-1", "--approved"],
            row(&["get", "N-1", "--approved"])["decision"].clone(),
        ),
        (&["status"], in_status),
    ] {
        assert_eq!(decision["judgement"], record["id"], "{args:?}");
        assert_eq!(decision["actor"], "tester", "{args:?}");
        assert_eq!(decision["at"], record["at"], "{args:?}");
        assert_eq!(decision["reason"], "read it whole", "{args:?}");
    }
}
