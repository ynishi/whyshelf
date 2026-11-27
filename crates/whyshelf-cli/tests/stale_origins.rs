//! The origin in `stale`, on the binary and a small project of files: `stale` names
//! the document an edit started from, and nothing before an approval; and each
//! origin carries `next`, the commands that can settle it, as they are typed, which
//! run when what a person writes in `<...>` is written.

mod common;

use std::fs;
use std::process::{Command, Output};
use std::thread::sleep;
use std::time::Duration;

use common::{BOOKS, NOTE, Project, in_seconds, json};

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

/// The words a POSIX shell reads from a line of plain and single-quoted words.
fn shell_words(line: &str) -> Vec<String> {
    let mut words = Vec::new();
    let mut word: Option<String> = None;
    let mut chars = line.chars();
    let mut quoted = false;
    while let Some(c) = chars.next() {
        match c {
            '\'' => {
                quoted = !quoted;
                word.get_or_insert_with(String::new);
            }
            '\\' if !quoted => word.get_or_insert_with(String::new).extend(chars.next()),
            c if c.is_whitespace() && !quoted => words.extend(word.take()),
            c => word.get_or_insert_with(String::new).push(c),
        }
    }
    assert!(!quoted, "a quote is left open in {line}");
    words.extend(word);
    words
}

/// The `next` of the one origin of `stale`, each command as its words after the
/// program and its `--root`, which every command carries as the run had it.
fn next_of(project: &Project) -> Vec<Vec<String>> {
    let answer = json(&project.run(&["stale"]));
    let origins = answer["origins"].as_array().unwrap();
    assert_eq!(origins.len(), 1, "{answer}");
    origins[0]["next"]
        .as_array()
        .unwrap()
        .iter()
        .map(|line| {
            let words = shell_words(line.as_str().unwrap());
            assert_eq!(words[0], "whyshelf", "{line}");
            assert_eq!(
                words[1],
                format!("--root={}", project.0.to_str().unwrap()),
                "{line}"
            );
            words[2..].to_vec()
        })
        .collect()
}

fn words(list: &[&str]) -> Vec<String> {
    list.iter().map(|w| (*w).to_owned()).collect()
}

/// Run one command of a `next`, in the project, with each word in `<...>` replaced
/// by the next of `fills`, as a person writes it.
fn run_next(project: &Project, command: &[String], fills: &[&str]) -> Output {
    let mut fills = fills.iter();
    let args: Vec<String> = command
        .iter()
        .map(|word| {
            if word.starts_with('<') && word.ends_with('>') {
                (*fills.next().expect("a word written for each placeholder")).to_owned()
            } else {
                word.clone()
            }
        })
        .collect();
    assert!(fills.next().is_none(), "a word written for no placeholder");
    Command::new(env!("CARGO_BIN_EXE_whyshelf"))
        .arg("--root")
        .arg(&project.0)
        .args(&args)
        .env("WHYSHELF_ACTOR", "tester")
        .output()
        .expect("the binary runs")
}

fn succeeded(output: &Output) {
    assert_eq!(
        output.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

/// The lines a run wrote on stderr.
fn stderr_lines(output: &Output) -> Vec<String> {
    String::from_utf8_lossy(&output.stderr)
        .lines()
        .map(str::to_owned)
        .collect()
}

fn origins_of(project: &Project) -> serde_json::Value {
    json(&project.run(&["stale"]))["origins"].clone()
}

/// A project of notes `N-1` .. that stand on one another (`on`) and on files.
fn linked_books() -> String {
    format!(
        "{BOOKS}[[book.link]]\nname = \"on\"\ntarget = \"note\"\nmeans = \"depends\"\n\
         [[book.link]]\nname = \"files\"\ntarget = \"artifact\"\nspace = \"file\"\nmeans = \"depends\"\n"
    )
}

fn note(n: u32, entries: &str) -> String {
    format!(
        "---\nid: \"01a1c4f0-0000-7000-8000-00000000000{n}\"\nkey: \"N-{n}\"\ntitle: \"Note {n}\"\n{entries}---\nThe body.\n"
    )
}

/// `N-1` approved, then its body edited.
fn edited() -> Project {
    let project = Project::new("next-edited");
    succeeded(&project.run(&["judge", "approve", "N-1"]));
    fs::write(
        project.0.join("notes/N-1.md"),
        NOTE.replace("The body.", "An edited body."),
    )
    .unwrap();
    project
}

#[test]
fn an_edited_document_is_read_declared_about_and_approved_again_by_its_next() {
    let project = edited();
    let next = next_of(&project);
    assert_eq!(
        next,
        [
            words(&["diff", "note:N-1"]),
            words(&[
                "judge",
                "same",
                "note:N-1",
                "--reason",
                "<why the meaning is kept>"
            ]),
            words(&["judge", "approve", "--stale", "--reason", "<why they hold>"]),
        ]
    );
    // The first is a reading: it runs as it is, and writes nothing.
    let before = project.files();
    let diff = run_next(&project, &next[0], &[]);
    succeeded(&diff);
    assert_eq!(json(&diff)["version"], 1);
    assert_eq!(project.files(), before);
    // The second, with the reason a person gives, settles the origin.
    succeeded(&run_next(
        &project,
        &next[1],
        &["only the body's last word"],
    ));
    assert_eq!(origins_of(&project), serde_json::json!([]));
}

#[test]
fn the_last_of_the_next_of_an_edit_approves_again_and_settles_it() {
    let project = edited();
    let next = next_of(&project);
    succeeded(&run_next(&project, &next[2], &["read, and it holds"]));
    assert_eq!(origins_of(&project), serde_json::json!([]));
}

#[test]
fn a_gone_file_is_looked_for_and_declared_moved_and_never_approved_again() {
    let project = Project::with(
        "next-gone",
        &linked_books(),
        &note(1, "files: [\"src/a.rs\"]\n"),
    );
    fs::create_dir_all(project.0.join("src")).unwrap();
    fs::write(project.0.join("src/a.rs"), "fn one() {}\n").unwrap();
    succeeded(&project.run(&["judge", "approve", "N-1"]));
    fs::remove_file(project.0.join("src/a.rs")).unwrap();

    let next = next_of(&project);
    assert_eq!(
        next,
        [
            words(&["moves"]),
            words(&[
                "judge",
                "moved",
                "src/a.rs",
                "<new name>",
                "--reason",
                "<why it is the same thing>"
            ]),
        ]
    );
    // `moves` runs as it is and names the gone file.
    let found = run_next(&project, &next[0], &[]);
    succeeded(&found);
    assert_eq!(json(&found)["absent"][0]["name"], "src/a.rs");
    // The file is at another name: the second, with it and a reason, settles it.
    fs::write(project.0.join("src/b.rs"), "fn one() {}\n").unwrap();
    succeeded(&run_next(&project, &next[1], &["src/b.rs", "renamed"]));
    // The link still writes the old name: that is the origin now, and there is no
    // command for it, since the link is a file to write.
    let origins = origins_of(&project);
    assert_eq!(origins[0]["start"]["doc"]["key"], "note:N-1", "{origins}");
    assert_eq!(origins[0]["next"], serde_json::json!([]));
    fs::write(
        project.0.join("notes/N-1.md"),
        note(1, "files: [\"src/b.rs\"]\n"),
    )
    .unwrap();
    assert_eq!(origins_of(&project), serde_json::json!([]));
}

#[test]
fn a_lapsed_approval_is_approved_again_with_a_new_expiry_and_not_by_stale() {
    let project = Project::new("next-lapsed");
    succeeded(&project.run(&[
        "judge",
        "approve",
        "N-1",
        "--until",
        &in_seconds(3),
        "--reason",
        "for now",
    ]));
    sleep(Duration::from_millis(3_500));
    let next = next_of(&project);
    // `--stale` would approve with no expiry: it is not offered for a lapse.
    assert_eq!(
        next,
        [words(&[
            "judge",
            "approve",
            "note:N-1",
            "--until",
            "<time>",
            "--reason",
            "<why it holds>"
        ])]
    );
    succeeded(&run_next(
        &project,
        &next[0],
        &["2999-01-01", "read again, and it holds"],
    ));
    assert_eq!(origins_of(&project), serde_json::json!([]));
}

#[test]
fn a_pending_upstream_is_approved_and_stale_is_not_offered() {
    let project = Project::with("next-pending", &linked_books(), &note(1, "on: [N-2]\n"));
    fs::write(project.0.join("notes/N-2.md"), note(2, "")).unwrap();
    // N-1 is approved standing on an N-2 nobody decided about.
    succeeded(&project.run(&["judge", "approve", "N-1"]));
    let next = next_of(&project);
    assert_eq!(
        next,
        [words(&[
            "judge",
            "approve",
            "note:N-2",
            "--reason",
            "<why it holds>"
        ])]
    );
    succeeded(&run_next(&project, &next[0], &["read, and it holds"]));
    assert_eq!(origins_of(&project), serde_json::json!([]));
}

#[test]
fn a_pending_upstream_whose_link_names_a_denied_document_is_not_offered_its_approval() {
    // N-1 is approved standing on an N-2 nobody decided about, and N-2 stands on N-3,
    // which is denied: the frame refuses an approval of N-2.
    let project = Project::with(
        "next-pending-denied",
        &linked_books(),
        &note(1, "on: [N-2]\n"),
    );
    fs::write(project.0.join("notes/N-2.md"), note(2, "on: [N-3]\n")).unwrap();
    fs::write(project.0.join("notes/N-3.md"), note(3, "")).unwrap();
    succeeded(&project.run(&["judge", "deny", "N-3", "--reason", "not a note"]));
    succeeded(&project.run(&["judge", "approve", "N-1"]));
    let stale = project.run(&["stale"]);
    let origins = json(&stale)["origins"].clone();
    assert_eq!(origins[0]["start"]["doc"]["key"], "note:N-2", "{origins}");
    assert_eq!(next_of(&project), Vec::<Vec<String>>::new());
    // It is told why, in the answer and on stderr, and not offered as a command.
    let denied = "the link `on` names note:N-3, which is denied: an approval cannot stand on it";
    let withheld = &origins[0]["withheld"];
    assert_eq!(withheld.as_array().map(Vec::len), Some(1), "{origins}");
    assert_eq!(withheld[0]["doc"]["key"], "note:N-2", "{origins}");
    assert_eq!(withheld[0]["approve"], "pending", "{origins}");
    assert_eq!(withheld[0]["detail"], denied, "{origins}");
    assert_eq!(
        stderr_lines(&stale),
        [format!(
            "warning: note:N-2 is not offered an approval as pending: {denied}"
        )]
    );
    // The approval it no longer offers is refused when it is run.
    assert_ne!(
        project.run(&["judge", "approve", "N-2"]).status.code(),
        Some(0)
    );
}

/// The books of [`linked_books`], and a link `items` to names of the preset `rustdoc`,
/// of crates whose docs nobody built: names that cannot be told.
fn untold_books() -> String {
    format!(
        "{}[[book.link]]\nname = \"items\"\ntarget = \"artifact\"\npreset = \"rustdoc\"\n",
        linked_books()
    )
}

#[test]
fn a_pending_upstream_whose_link_names_what_cannot_be_told_is_not_offered_its_approval() {
    // N-1 is approved standing on an N-2 nobody decided about, and N-2 stands on an
    // item of a crate whose docs are not built: the frame refuses an approval of N-2.
    let project = Project::with(
        "next-pending-untold",
        &untold_books(),
        &note(1, "on: [N-2]\n"),
    );
    fs::write(
        project.0.join("notes/N-2.md"),
        note(2, "items: [\"nodocs::Thing\"]\n"),
    )
    .unwrap();
    succeeded(&project.run(&["judge", "approve", "N-1"]));
    let stale = project.run(&["stale"]);
    let origins = json(&stale)["origins"].clone();
    assert_eq!(origins.as_array().map(Vec::len), Some(1), "{origins}");
    assert_eq!(origins[0]["start"]["doc"]["key"], "note:N-2", "{origins}");
    assert_eq!(origins[0]["next"], serde_json::json!([]), "{origins}");
    let withheld = &origins[0]["withheld"];
    assert_eq!(withheld.as_array().map(Vec::len), Some(1), "{origins}");
    assert_eq!(withheld[0]["doc"]["key"], "note:N-2", "{origins}");
    assert_eq!(withheld[0]["approve"], "pending", "{origins}");
    let detail = withheld[0]["detail"].as_str().unwrap();
    assert!(detail.contains("cannot be told about now"), "{detail}");
    let lines = stderr_lines(&stale);
    assert_eq!(lines.len(), 1, "{lines:?}");
    assert_eq!(
        lines[0],
        format!("warning: note:N-2 is not offered an approval as pending: {detail}")
    );
}

#[test]
fn a_pending_upstream_whose_approval_would_close_a_cycle_of_frames_says_so() {
    // N-1 and N-2 are approved standing on N-3, which nobody decided about. Then the
    // links turn round: N-1 no longer names N-3, and N-3 names N-1. No cycle of links
    // stands, but an approval of N-3 would pin N-1, whose approval pinned N-3.
    let project = Project::with(
        "next-frames-cycle",
        &linked_books(),
        &note(1, "on: [N-3]\n"),
    );
    fs::write(project.0.join("notes/N-2.md"), note(2, "on: [N-3]\n")).unwrap();
    fs::write(project.0.join("notes/N-3.md"), note(3, "")).unwrap();
    succeeded(&project.run(&["judge", "approve", "N-1", "N-2"]));
    fs::write(project.0.join("notes/N-1.md"), note(1, "")).unwrap();
    fs::write(project.0.join("notes/N-3.md"), note(3, "on: [N-1]\n")).unwrap();

    let stale = project.run(&["stale"]);
    let answer = json(&stale);
    assert_eq!(
        answer["health"]["cycles"],
        serde_json::json!([]),
        "{answer}"
    );
    let origins = answer["origins"].as_array().unwrap();
    let at_n3: Vec<_> = origins
        .iter()
        .filter(|o| o["start"]["doc"]["key"] == "note:N-3")
        .collect();
    assert_eq!(at_n3.len(), 1, "{answer}");
    let degraded: Vec<_> = at_n3[0]["degraded"]
        .as_array()
        .unwrap()
        .iter()
        .map(|d| d["key"].as_str().unwrap().to_owned())
        .collect();
    assert!(degraded.contains(&"note:N-2".to_owned()), "{answer}");
    assert_eq!(at_n3[0]["next"], serde_json::json!([]), "{answer}");
    let withheld = &at_n3[0]["withheld"];
    assert_eq!(withheld.as_array().map(Vec::len), Some(1), "{answer}");
    assert_eq!(withheld[0]["doc"]["key"], "note:N-3", "{answer}");
    assert_eq!(withheld[0]["approve"], "pending", "{answer}");
    let detail = withheld[0]["detail"].as_str().unwrap();
    assert!(
        detail.starts_with("the document is in a cycle of approvals' frames: "),
        "{detail}"
    );
    let lines = stderr_lines(&stale);
    assert_eq!(lines.len(), 1, "{lines:?}");
    assert_eq!(
        lines[0],
        format!("warning: note:N-3 is not offered an approval as pending: {detail}")
    );
}

#[test]
fn a_refusal_told_at_one_origin_is_told_at_the_origin_it_keeps_an_approval_from_too() {
    // N-4 stands on N-3 and is approved; N-3 is then denied. N-1 stands on the pending
    // N-2, which stands on the denied N-3. Two origins: N-3, where N-4's approval
    // fails, with nothing to settle and nothing to withhold; and N-2, where the
    // refusal that keeps N-2 from being approved is told again.
    let project = Project::with("next-told-twice", &linked_books(), &note(1, "on: [N-2]\n"));
    fs::write(project.0.join("notes/N-2.md"), note(2, "on: [N-3]\n")).unwrap();
    fs::write(project.0.join("notes/N-3.md"), note(3, "")).unwrap();
    fs::write(project.0.join("notes/N-4.md"), note(4, "on: [N-3]\n")).unwrap();
    // An approval standing on the pending N-3 can be made before N-3 is denied.
    succeeded(&project.run(&["judge", "approve", "N-4"]));
    succeeded(&project.run(&["judge", "deny", "N-3", "--reason", "not a note"]));
    succeeded(&project.run(&["judge", "approve", "N-1"]));

    let stale = project.run(&["stale"]);
    let answer = json(&stale);
    let at = |key: &str| -> serde_json::Value {
        let found: Vec<_> = answer["origins"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|o| o["start"]["doc"]["key"] == key)
            .collect();
        assert_eq!(found.len(), 1, "{key}: {answer}");
        found[0].clone()
    };
    // (a) The origin at N-3 has nothing to settle and nothing withheld.
    let n3 = at("note:N-3");
    assert_eq!(n3["next"], serde_json::json!([]), "{answer}");
    assert_eq!(n3["withheld"], serde_json::json!([]), "{answer}");
    // (b) N-4's entry says N-3 is decided against: it is denied.
    let n4 = answer["stale"]
        .as_array()
        .unwrap()
        .iter()
        .find(|e| e["row"]["doc"]["key"] == "note:N-4")
        .unwrap_or_else(|| panic!("an entry for N-4: {answer}"));
    assert_eq!(n4["roots"][0]["cause"]["kind"], "upstream_declared", "{n4}");
    assert_eq!(
        n4["roots"][0]["cause"]["detail"], "note:N-3 is denied",
        "{n4}"
    );
    assert_eq!(
        n4["roots"][0]["cause"]["upstream"]["key"], "note:N-3",
        "{n4}"
    );
    // (c) The origin at N-2 says why N-2 is not offered an approval.
    let denied = "the link `on` names note:N-3, which is denied: an approval cannot stand on it";
    let n2 = at("note:N-2");
    assert_eq!(n2["next"], serde_json::json!([]), "{answer}");
    let withheld = &n2["withheld"];
    assert_eq!(withheld.as_array().map(Vec::len), Some(1), "{answer}");
    assert_eq!(withheld[0]["doc"]["key"], "note:N-2", "{answer}");
    assert_eq!(withheld[0]["approve"], "pending", "{answer}");
    assert_eq!(withheld[0]["detail"], denied, "{answer}");
    // (d) And on stderr, once.
    assert_eq!(
        stderr_lines(&stale),
        [format!(
            "warning: note:N-2 is not offered an approval as pending: {denied}"
        )]
    );
}

#[test]
fn the_diff_is_of_the_approval_that_holds_the_cause_and_not_one_that_sees_it_through() {
    // N-1 stands on N-2, which stands on N-3; N-3 is edited. N-1 and N-2 are stale
    // through it, and only N-3 holds the cause: its `diff` shows the edit, and the
    // `diff` of N-1, the smallest identity under the origin, shows nothing of it.
    let project = Project::with("next-via", &linked_books(), &note(1, "on: [N-2]\n"));
    fs::write(project.0.join("notes/N-2.md"), note(2, "on: [N-3]\n")).unwrap();
    fs::write(project.0.join("notes/N-3.md"), note(3, "")).unwrap();
    succeeded(&project.run(&["judge", "approve", "N-3", "N-2", "N-1"]));
    fs::write(
        project.0.join("notes/N-3.md"),
        note(3, "").replace("The body.", "An edited body."),
    )
    .unwrap();
    let answer = json(&project.run(&["stale"]));
    assert_eq!(
        answer["origins"][0]["degraded"].as_array().map(Vec::len),
        Some(3),
        "{answer}"
    );
    let next = next_of(&project);
    assert_eq!(next[0], words(&["diff", "note:N-3"]));
    let diff = run_next(&project, &next[0], &[]);
    succeeded(&diff);
    // Its subject is the edited document, changed since it was approved.
    let item = &json(&diff)["items"][0];
    assert_eq!(item["item"]["doc"]["key"], "note:N-3", "{item}");
    assert_eq!(item["changed"], true, "{item}");
    // The `diff` of N-1 has no change to show: its own text is as it was approved.
    let below = project.run(&["diff", "note:N-1"]);
    succeeded(&below);
    assert_eq!(json(&below)["items"][0]["changed"], false);
}

#[test]
fn the_schema_of_stale_has_next_for_each_origin() {
    let project = Project::new("next-schema");
    let schema = json(&project.run(&["schema"]));
    let origin = &schema["outputs"]["stale"]["$defs"]["OriginOut"];
    assert_eq!(origin["properties"]["next"]["type"], "array");
    assert_eq!(origin["properties"]["next"]["items"]["type"], "string");
    assert!(
        origin["required"]
            .as_array()
            .unwrap()
            .contains(&serde_json::json!("next")),
        "{origin}"
    );
    // What is withheld is told apart from `next`: a document, a word and a sentence.
    assert_eq!(origin["properties"]["withheld"]["type"], "array");
    assert!(
        origin["required"]
            .as_array()
            .unwrap()
            .contains(&serde_json::json!("withheld")),
        "{origin}"
    );
    let withheld = &schema["outputs"]["stale"]["$defs"]["WithheldOut"];
    let properties = withheld["properties"].as_object().unwrap();
    for name in ["doc", "approve", "detail"] {
        assert!(properties.contains_key(name), "{name} in {withheld}");
    }
    assert_eq!(properties["approve"]["type"], "string");
    assert_eq!(properties["detail"]["type"], "string");
    // The shape only grew: the version stays.
    assert_eq!(schema["version"], 1);
}
