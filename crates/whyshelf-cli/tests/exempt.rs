//! `judge exempt`, on the binary: a document a covering link does not reach is
//! exempted with a reason, and `check` is clean and lists the exemption apart; once
//! the link reaches it, `check` gives `exemption_unneeded` in its answer and on
//! stderr, and exits 0; `judge withdraw` takes the exemption back; a refused
//! exemption exits 2 and writes nothing. On the project `init --sample` writes
//! (whose `grounds` cover is `warn`), and on a small project of notes whose cover is
//! `true`.

mod common;

use std::fs;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use common::{BOOKS, NOTE, Project, json};

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
        "whyshelf-exempt-{name}-{}-{count}-{nanos}",
        std::process::id()
    ));
    fs::create_dir_all(&dir).unwrap();
    Project(dir)
}

fn stderr(output: &std::process::Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

fn ok(output: &std::process::Output) -> serde_json::Value {
    assert_eq!(output.status.code(), Some(0), "{}", stderr(output));
    json(output)
}

const KEPT: &str = "kept on purpose: it grounds nothing yet";

/// The sample, with a second finding, F-2, that no requirement grounds.
fn sample_with_a_second_finding() -> Project {
    let project = empty("sample");
    ok(&project.run(&["init", "--sample"]));
    ok(&project.run(&["new", "finding", "--title", "A second finding"]));
    project
}

#[test]
fn a_finding_exempt_from_a_warn_cover_is_listed_apart_and_unneeded_once_grounded() {
    let project = sample_with_a_second_finding();
    let check = project.run(&["check"]);
    assert_eq!(json(&check)["uncovered"][0]["doc"]["key"], "finding:F-2");
    assert!(stderr(&check).contains("finding:F-2 is reached by no"));

    let done = ok(&project.run(&[
        "judge",
        "exempt",
        "F-2",
        "--from",
        "requirement.grounds",
        "--reason",
        KEPT,
    ]));
    let one = &done["exempt"][0];
    assert_eq!(one["doc"]["key"], "finding:F-2");
    assert_eq!(one["book"], "requirement");
    assert_eq!(one["link"], "grounds");
    assert_eq!(one["record"]["kind"], "exempt");
    let id = one["record"]["id"].as_str().unwrap().to_owned();

    let check = project.run(&["check"]);
    let answer = ok(&check);
    assert_eq!(answer["clean"], true);
    assert_eq!(answer["uncovered"], serde_json::json!([]));
    assert_eq!(answer["exemption_unneeded"], serde_json::json!([]));
    let listed = &answer["exempt"][0];
    assert_eq!(listed["doc"]["key"], "finding:F-2");
    assert_eq!(listed["book"], "requirement");
    assert_eq!(listed["link"], "grounds");
    assert_eq!(listed["severity"], "warn");
    assert_eq!(listed["exemption"], id.as_str());
    assert_eq!(listed["actor"], "tester");
    assert!(listed["at"].as_i64().unwrap() > 0);
    assert_eq!(listed["reason"], KEPT);
    assert_eq!(stderr(&check), "", "an exemption needed warns of nothing");

    // REQ-1 grounds F-2 too: the exemption is not needed any more.
    let req = project.0.join("books/requirement/REQ-1.md");
    let text = fs::read_to_string(&req).unwrap();
    assert!(text.contains("grounds: [\"F-1\"]"), "{text}");
    fs::write(
        &req,
        text.replace("grounds: [\"F-1\"]", "grounds: [\"F-1\", \"F-2\"]"),
    )
    .unwrap();
    let check = project.run(&["check"]);
    let answer = ok(&check);
    assert_eq!(answer["exempt"], serde_json::json!([]));
    assert_eq!(answer["exemption_unneeded"][0]["why"], "reached");
    assert_eq!(answer["exemption_unneeded"][0]["exemption"], id.as_str());
    // The full line, every document by its key.
    assert_eq!(
        stderr(&check),
        format!(
            "warning: {id} exempts finding:F-2 from `requirement.grounds` and is not needed: `requirement.grounds` reaches it now, from requirement:REQ-1; withdraw it with `judge withdraw {id}`\n"
        )
    );
}

/// Notes see other notes, and every note is expected to be seen (`cover = true`);
/// N-1 is alone.
fn notes_covered() -> Project {
    let books = format!(
        "{BOOKS}[[book.link]]\nname = \"see\"\ntarget = \"note\"\nmeans = \"informs\"\ncover = true\n"
    );
    Project::with("exempt-true", &books, NOTE)
}

fn exempt_n1(project: &Project) -> std::process::Output {
    project.run(&[
        "judge", "exempt", "N-1", "--from", "note.see", "--reason", KEPT,
    ])
}

#[test]
fn a_note_exempt_from_a_true_cover_leaves_check_clean_until_it_is_seen() {
    let project = notes_covered();
    assert_eq!(project.run(&["check"]).status.code(), Some(1));

    let done = ok(&exempt_n1(&project));
    let id = done["exempt"][0]["record"]["id"]
        .as_str()
        .unwrap()
        .to_owned();

    let check = project.run(&["check"]);
    let answer = ok(&check);
    assert_eq!(answer["clean"], true, "{answer}");
    assert_eq!(answer["uncovered"], serde_json::json!([]));
    assert_eq!(answer["exempt"][0]["severity"], "error");
    assert_eq!(answer["exempt"][0]["doc"]["key"], "note:N-1");
    assert_eq!(stderr(&check), "");
    // The document tells it too.
    let get = ok(&project.run(&["get", "N-1"]));
    assert_eq!(get["row"]["notices"][0]["kind"], "exempt");
    assert_eq!(get["row"]["notices"][0]["declared"][0], id.as_str());

    // N-1 and N-2 see each other: N-1 is reached, and its exemption is not needed.
    fs::write(
        project.0.join("notes/N-1.md"),
        NOTE.replace("title: \"A note\"\n", "title: \"A note\"\nsee: [\"N-2\"]\n"),
    )
    .unwrap();
    fs::write(
        project.0.join("notes/N-2.md"),
        "---\nid: \"01a1c4f0-0000-7000-8000-000000000002\"\nkey: \"N-2\"\ntitle: \"Another\"\nsee: [\"N-1\"]\n---\nThe body.\n",
    )
    .unwrap();
    let check = project.run(&["check"]);
    assert_eq!(check.status.code(), Some(0), "{}", stderr(&check));
    let answer = json(&check);
    assert_eq!(answer["clean"], true);
    let unneeded = &answer["exemption_unneeded"][0];
    assert_eq!(unneeded["doc"]["key"], "note:N-1");
    assert_eq!(unneeded["book"], "note");
    assert_eq!(unneeded["link"], "see");
    assert_eq!(unneeded["exemption"], id.as_str());
    assert_eq!(unneeded["why"], "reached");
    assert_eq!(unneeded["reason"], KEPT);
    assert_eq!(
        stderr(&check),
        format!(
            "warning: {id} exempts note:N-1 from `note.see` and is not needed: `note.see` reaches it now, from note:N-2; withdraw it with `judge withdraw {id}`\n"
        )
    );
    let get = ok(&project.run(&["get", "N-1"]));
    let kinds: Vec<&str> = get["row"]["notices"]
        .as_array()
        .unwrap()
        .iter()
        .map(|n| n["kind"].as_str().unwrap())
        .collect();
    // Beside it, only that N-2, which it informs of, is pending.
    assert_eq!(kinds, ["exemption_unneeded", "informs_not_fresh"]);
    assert_eq!(get["row"]["notices"][0]["why"], "reached");

    // Withdrawn, it is gone from the answer and the warning.
    ok(&project.run(&["judge", "withdraw", &id, "--reason", "seen now"]));
    let check = project.run(&["check"]);
    let answer = ok(&check);
    assert_eq!(answer["exempt"], serde_json::json!([]));
    assert_eq!(answer["exemption_unneeded"], serde_json::json!([]));
    assert_eq!(stderr(&check), "");
}

#[test]
fn an_exemption_withdrawn_leaves_the_gap_as_it_was() {
    let project = notes_covered();
    let id = ok(&exempt_n1(&project))["exempt"][0]["record"]["id"]
        .as_str()
        .unwrap()
        .to_owned();
    ok(&project.run(&["judge", "withdraw", &id, "--reason", "a mistake"]));
    let check = project.run(&["check"]);
    assert_eq!(check.status.code(), Some(1));
    assert_eq!(json(&check)["uncovered"][0]["doc"]["key"], "note:N-1");
    assert_eq!(json(&check)["exempt"], serde_json::json!([]));
}

#[test]
fn an_exemption_unneeded_at_once_exits_2_and_writes_nothing() {
    let sample = sample_with_a_second_finding();
    let before = sample.files();
    // REQ-1 grounds F-1: the exemption would be unneeded the moment it is written.
    let out = sample.run(&[
        "judge",
        "exempt",
        "F-1",
        "--from",
        "requirement.grounds",
        "--reason",
        KEPT,
    ]);
    assert_eq!(out.status.code(), Some(2), "{}", stderr(&out));
    assert_eq!(json(&out)["error"], "request");
    assert!(
        stderr(&out).contains("requirement:REQ-1"),
        "{}",
        stderr(&out)
    );
    assert_eq!(sample.files(), before);

    // F-2 denied: no covering link expects to reach it.
    ok(&sample.run(&["judge", "deny", "F-2", "--reason", "not a finding"]));
    let before = sample.files();
    let out = sample.run(&[
        "judge",
        "exempt",
        "F-2",
        "--from",
        "requirement.grounds",
        "--reason",
        KEPT,
    ]);
    assert_eq!(out.status.code(), Some(2), "{}", stderr(&out));
    assert!(stderr(&out).contains("decided against"), "{}", stderr(&out));
    assert_eq!(sample.files(), before);
}

#[test]
fn a_refused_exemption_exits_2_and_writes_nothing() {
    let project = notes_covered();
    ok(&exempt_n1(&project));
    let before = project.files();
    let refused: [&[&str]; 5] = [
        // A second exemption of the same document from the same link.
        &[
            "judge", "exempt", "N-1", "--from", "note.see", "--reason", KEPT,
        ],
        // No such link, and no such book.
        &[
            "judge",
            "exempt",
            "N-1",
            "--from",
            "note.nope",
            "--reason",
            KEPT,
        ],
        &[
            "judge",
            "exempt",
            "N-1",
            "--from",
            "nobook.see",
            "--reason",
            KEPT,
        ],
        // Not `<book>.<link>`, and a blank reason.
        &["judge", "exempt", "N-1", "--from", "see", "--reason", KEPT],
        &[
            "judge", "exempt", "N-1", "--from", "note.see", "--reason", "  ",
        ],
    ];
    for args in refused {
        let out = project.run(args);
        assert_eq!(out.status.code(), Some(2), "{args:?}: {}", stderr(&out));
        assert_eq!(json(&out)["error"], "request", "{args:?}");
        assert_eq!(project.files(), before, "{args:?} wrote something");
    }

    // A link declared with no cover, and one into another book, on the sample.
    let sample = sample_with_a_second_finding();
    let before = sample.files();
    // Each cause is told once, in the messages and on stderr.
    for (from, cause) in [
        ("requirement.replaces", "the link is declared with no cover"),
        (
            "decision.serves",
            "the link points into `requirement`, and the document is of `finding`",
        ),
        (
            "requirement.nope",
            "no link of that name into a book is declared",
        ),
    ] {
        let out = sample.run(&["judge", "exempt", "F-2", "--from", from, "--reason", KEPT]);
        assert_eq!(out.status.code(), Some(2), "{from}: {}", stderr(&out));
        assert_eq!(
            json(&out)["messages"],
            serde_json::json!([
                "the exemption is refused",
                format!("{from} cannot exempt it"),
                cause
            ]),
            "{from}"
        );
        assert_eq!(
            stderr(&out),
            format!("error: the exemption is refused: {from} cannot exempt it: {cause}\n")
        );
        assert_eq!(sample.files(), before, "{from} wrote something");
    }
}

/// The notes project with N-1 exempt from `note.see`, and the exemption's id.
fn exempt_project(name: &str, books: &str) -> (Project, String) {
    let project = Project::with(name, books, NOTE);
    let id = ok(&exempt_n1(&project))["exempt"][0]["record"]["id"]
        .as_str()
        .unwrap()
        .to_owned();
    (project, id)
}

fn notes_books() -> String {
    format!(
        "{BOOKS}[[book.link]]\nname = \"see\"\ntarget = \"note\"\nmeans = \"informs\"\ncover = true\n"
    )
}

/// The one unneeded exemption `check` lists after `change`, with its stderr.
fn unneeded_after(
    name: &str,
    change: impl FnOnce(&Project),
) -> (serde_json::Value, String, String) {
    let (project, id) = exempt_project(name, &notes_books());
    change(&project);
    let check = project.run(&["check"]);
    let answer = json(&check);
    let listed = answer["exemption_unneeded"].as_array().unwrap();
    assert_eq!(listed.len(), 1, "{answer}");
    assert_eq!(listed[0]["exemption"], id.as_str());
    (listed[0].clone(), stderr(&check), id)
}

#[test]
fn every_why_of_an_unneeded_exemption_is_told_on_the_binary() {
    let books = |p: &Project| p.0.join(".whyshelf/books.toml");

    // The cover is taken off the link.
    let (unneeded, _, _) = unneeded_after("why-no-cover", |p| {
        fs::write(
            books(p),
            notes_books().replace("cover = true", "cover = false"),
        )
        .unwrap();
    });
    assert_eq!(unneeded["why"], "no_cover");

    // The book is renamed: no link `note.see` is declared any more.
    let (unneeded, warning, id) = unneeded_after("why-renamed", |p| {
        fs::write(
            books(p),
            notes_books()
                .replace("name = \"note\"", "name = \"memo\"")
                .replace("target = \"note\"", "target = \"memo\""),
        )
        .unwrap();
    });
    assert_eq!(unneeded["why"], "not_declared");
    assert_eq!(unneeded["doc"]["key"], "memo:N-1");
    assert_eq!(
        warning,
        format!(
            "warning: {id} exempts memo:N-1 from `note.see` and is not needed: no link `note.see` into a book is declared now; withdraw it with `judge withdraw {id}`\n"
        )
    );

    // The link points into another book.
    let (unneeded, _, _) = unneeded_after("why-other-book", |p| {
        fs::create_dir_all(p.0.join("memos")).unwrap();
        fs::write(
            books(p),
            notes_books().replace("target = \"note\"", "target = \"memo\"")
                + "[[book]]\nname = \"memo\"\ndir = \"memos\"\nkey = { prefix = \"M\" }\n",
        )
        .unwrap();
    });
    assert_eq!(unneeded["why"], "other_book");
    assert!(
        unneeded["detail"]
            .as_str()
            .unwrap()
            .ends_with("`note.see` points into `memo` now, and the document is of `note`"),
        "{unneeded}"
    );

    // The note is invalid: it has no key, and is named by its identity.
    let (unneeded, warning, id) = unneeded_after("why-invalid", |p| {
        fs::write(
            p.0.join("notes/N-1.md"),
            NOTE.replace("title: \"A note\"\n", ""),
        )
        .unwrap();
    });
    assert_eq!(unneeded["why"], "not_valid");
    assert_eq!(unneeded["doc"]["key"], serde_json::Value::Null);
    let n1 = "01a1c4f0-0000-7000-8000-000000000001";
    assert_eq!(
        warning,
        format!(
            "warning: {id} exempts {n1} from `note.see` and is not needed: the document is not valid now: its files are invalid; withdraw it with `judge withdraw {id}`\n"
        )
    );

    // The book's range leaves the note out (#75): no file is said to be gone.
    let (unneeded, _, _) = unneeded_after("why-out-of-range", |p| {
        fs::write(
            books(p),
            notes_books().replace(
                "key = { prefix = \"N\" }\n",
                "key = { prefix = \"N\" }\nexclude = [\"N-1.md\"]\n",
            ),
        )
        .unwrap();
    });
    assert_eq!(unneeded["why"], "not_valid");
    assert!(
        unneeded["detail"]
            .as_str()
            .unwrap()
            .ends_with("the document is not valid now: no file in the books' ranges carries it"),
        "{unneeded}"
    );

    // The note is denied.
    let (unneeded, _, _) = unneeded_after("why-denied", |p| {
        ok(&p.run(&["judge", "deny", "N-1", "--reason", "not a note"]));
    });
    assert_eq!(unneeded["why"], "decided_against");
}

#[test]
fn an_exemption_from_one_of_two_covering_links_leaves_the_other_a_gap() {
    let books = format!(
        "{}[[book.link]]\nname = \"cites\"\ntarget = \"note\"\nmeans = \"informs\"\ncover = true\n",
        notes_books()
    );
    let (project, _) = exempt_project("two-links", &books);
    let check = project.run(&["check"]);
    assert_eq!(check.status.code(), Some(1));
    let answer = json(&check);
    assert_eq!(answer["uncovered"][0]["link"], "cites");
    assert_eq!(answer["uncovered"].as_array().unwrap().len(), 1);
    assert_eq!(answer["exempt"][0]["link"], "see");
    // The notice of the gap names its book.
    let get = ok(&project.run(&["get", "N-1"]));
    let gap = get["row"]["notices"]
        .as_array()
        .unwrap()
        .iter()
        .find(|n| n["kind"] == "not_covered")
        .unwrap()
        .clone();
    assert_eq!(gap["book"], "note");
    assert_eq!(gap["link"], "cites");

    ok(&project.run(&[
        "judge",
        "exempt",
        "N-1",
        "--from",
        "note.cites",
        "--reason",
        KEPT,
    ]));
    let answer = ok(&project.run(&["check"]));
    assert_eq!(answer["clean"], true);
    assert_eq!(answer["exempt"].as_array().unwrap().len(), 2);
}

#[test]
fn an_exemption_withdrawn_twice_is_refused_and_one_may_be_made_again() {
    let (project, id) = exempt_project("withdraw-twice", &notes_books());
    ok(&project.run(&["judge", "withdraw", &id, "--reason", "a mistake"]));
    let before = project.files();
    let again = project.run(&["judge", "withdraw", &id, "--reason", "a mistake"]);
    assert_eq!(again.status.code(), Some(2), "{}", stderr(&again));
    assert_eq!(project.files(), before);

    let second = ok(&exempt_n1(&project));
    let second = second["exempt"][0]["record"]["id"].as_str().unwrap();
    assert_ne!(second, id);
    let answer = ok(&project.run(&["check"]));
    assert_eq!(answer["exempt"][0]["exemption"], second);
}

#[test]
fn a_document_named_twice_in_a_run_is_refused_as_such() {
    let project = notes_covered();
    let before = project.files();
    let out = project.run(&[
        "judge", "exempt", "N-1", "N-1", "--from", "note.see", "--reason", KEPT,
    ]);
    assert_eq!(out.status.code(), Some(2), "{}", stderr(&out));
    assert_eq!(
        stderr(&out),
        "error: the exemption is refused: an earlier exemption of this run already exempts it from note.see\n"
    );
    assert_eq!(project.files(), before);
}

#[test]
fn two_exemptions_of_one_note_from_one_link_are_warned_of_as_repeated() {
    let (project, id) = exempt_project("repeated", &notes_books());
    // A merged branch brings a second exemption of N-1 from `note.see`: the same
    // record under another id (the last digit of the UUID v7 changed).
    let ledger = project.0.join(".whyshelf/ledger");
    let text = fs::read_to_string(ledger.join(format!("{id}.json"))).unwrap();
    let last = id.chars().last().unwrap();
    let other = format!(
        "{}{}",
        &id[..id.len() - 1],
        if last == '0' { '1' } else { '0' }
    );
    fs::write(
        ledger.join(format!("{other}.json")),
        text.replace(&id, &other),
    )
    .unwrap();
    let (first, second) = if id < other {
        (&id, &other)
    } else {
        (&other, &id)
    };

    let check = project.run(&["check"]);
    let answer = ok(&check);
    assert_eq!(answer["clean"], true);
    assert_eq!(answer["exempt"].as_array().unwrap().len(), 2);
    let repeated = &answer["exemption_repeated"][0];
    assert_eq!(repeated["doc"]["key"], "note:N-1");
    assert_eq!(repeated["exemptions"], serde_json::json!([first, second]));
    assert_eq!(
        stderr(&check),
        format!(
            "warning: 2 exemptions exempt note:N-1 from `note.see` ({first}, {second}): one is enough, withdraw the others, with `judge withdraw <id>`\n"
        )
    );
}

#[test]
fn a_document_no_file_carries_names_nothing_even_with_an_exemption_of_it() {
    // N-1 is known to the ledger only by its exemption, once its file is gone: an
    // exemption is no decision, so the reference names no document (exit 2).
    let (project, _) = exempt_project("gone", &notes_books());
    fs::remove_file(project.0.join("notes/N-1.md")).unwrap();
    let before = project.files();
    for reference in ["N-1", "01a1c4f0-0000-7000-8000-000000000001"] {
        let out = project.run(&[
            "judge", "exempt", reference, "--from", "note.see", "--reason", KEPT,
        ]);
        assert_eq!(out.status.code(), Some(2), "{}", stderr(&out));
        assert_eq!(json(&out)["error"], "request");
        assert!(
            stderr(&out).contains("names no document"),
            "{}",
            stderr(&out)
        );
        assert_eq!(project.files(), before);
    }
}
