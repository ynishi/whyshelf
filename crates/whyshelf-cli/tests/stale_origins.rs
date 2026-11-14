//! The origin in `stale`, on the binary and a small project of files: `stale` names
//! the document an edit started from, and nothing before an approval; and each
//! origin carries `next`, the commands that can settle it, as they are typed, which
//! run when what a person writes in `<...>` is written.

mod common;

use std::fs;
use std::process::{Command, Output};
use std::thread::sleep;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use common::{BOOKS, NOTE, Project, json};

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

/// An RFC 3339 time in UTC, `seconds` from now.
fn in_seconds(seconds: u64) -> String {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs()
        + seconds;
    let (days, day_secs) = (now / 86_400, now % 86_400);
    // The civil date of a day count (Howard Hinnant's days-to-civil).
    let z = days + 719_468;
    let era = z / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + u64::from(month <= 2);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}Z",
        day_secs / 3_600,
        day_secs % 3_600 / 60,
        day_secs % 60
    )
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
    // The shape only grew: the version stays.
    assert_eq!(schema["version"], 1);
}
