//! What the command line says of an upstream, from each way it stands, on the binary
//! and a small project of files: N-1 stands on N-2, and a test for each way N-2 is
//! declared or stands checks, together, whether a new approval of N-1 is refused (and
//! as which decision), how strong the cause is, what the origin at N-2 says to type
//! (the event), and the words that tell it. The groups and their words are the model's
//! and the surface's (`upstream_status`, in `cause.rs`); this is the one place they
//! are read together, from the declaration of a person to the line the binary prints.

mod common;

use std::fs;

use common::{BOOKS, Project, json};

/// Notes that stand on other notes (`on`) and replace others (`replaces`).
fn linked_books(rule: &str, decide: &str) -> String {
    format!(
        "{rule}{BOOKS}{decide}[[book.link]]\nname = \"on\"\ntarget = \"note\"\nmeans = \"depends\"\n\
         [[book.link]]\nname = \"replaces\"\ntarget = \"note\"\nmeans = \"replaces\"\n"
    )
}

/// A note `N-<n>` with these entries after its title.
fn note(n: u32, entries: &str) -> String {
    format!(
        "---\nid: \"01a1c4f0-0000-7000-8000-00000000000{n}\"\nkey: \"N-{n}\"\ntitle: \"Note {n}\"\n{entries}---\nThe body.\n"
    )
}

/// N-1 standing on N-2, with nothing decided.
fn project(name: &str) -> Project {
    let project = Project::with(name, &linked_books("", ""), &note(1, "on: [N-2]\n"));
    fs::write(project.0.join("notes/N-2.md"), note(2, "")).unwrap();
    project
}

fn succeeded(project: &Project, args: &[&str]) {
    let output = project.run(args);
    assert_eq!(
        output.status.code(),
        Some(0),
        "{args:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

/// What the binary is to say of N-2 as the upstream of N-1.
struct Says {
    /// The decision a new approval of N-1 is refused for, in its words; none when it
    /// is made.
    refusal: Option<&'static str>,
    /// Where N-1 stands.
    standing: &'static str,
    /// The causes of N-1, in order: each kind, strength and the words its sentence
    /// begins with; none when N-1 holds.
    causes: &'static [(&'static str, &'static str, &'static str)],
    /// The origin at N-2: what each command it says to type holds, in order (none
    /// when it says none); `None` when there is no origin.
    origin: Option<&'static [&'static str]>,
}

/// N-1 as `status` shows it.
fn row_of_n1(project: &Project) -> serde_json::Value {
    let status = json(&project.run(&["status"]));
    status["docs"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["doc"]["key"] == "note:N-1")
        .unwrap_or_else(|| panic!("no N-1: {status}"))
        .clone()
}

/// Read what the binary says, and then try a new approval of N-1 as `actor` (which
/// writes it when it is made, so it is the last).
fn says(project: &Project, actor: &str, says: &Says) {
    let row = row_of_n1(project);
    assert_eq!(row["standing"], says.standing, "{row}");
    let causes: Vec<(&str, &str, &str)> = row["causes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| {
            (
                c["kind"].as_str().unwrap(),
                c["severity"].as_str().unwrap(),
                c["detail"].as_str().unwrap(),
            )
        })
        .collect();
    assert_eq!(causes.len(), says.causes.len(), "the causes: {row}");
    for ((kind, severity, detail), (want_kind, want_severity, want_words)) in
        causes.iter().zip(says.causes)
    {
        // The hashes a sentence ends with are not the words.
        let words = detail.split(" (sha256:").next().unwrap();
        assert_eq!(
            (*kind, *severity, words),
            (*want_kind, *want_severity, *want_words),
            "{row}"
        );
    }

    let stale = json(&project.run(&["stale"]));
    let origins = stale["origins"].as_array().unwrap();
    match says.origin {
        None => assert!(origins.is_empty(), "{stale}"),
        Some(commands) => {
            assert_eq!(origins.len(), 1, "{stale}");
            assert_eq!(origins[0]["start"]["doc"]["key"], "note:N-2", "{stale}");
            let next: Vec<&str> = origins[0]["next"]
                .as_array()
                .unwrap()
                .iter()
                .map(|line| line.as_str().unwrap())
                .collect();
            assert_eq!(next.len(), commands.len(), "{next:?}");
            for (line, fragment) in next.iter().zip(commands) {
                assert!(line.contains(fragment), "{line} lacks {fragment}");
            }
        }
    }

    let approved = project.run(&["judge", "approve", "N-1", "--actor", actor]);
    match says.refusal {
        None => assert_eq!(approved.status.code(), Some(0), "{}", json(&approved)),
        Some(decision) => {
            let answer = json(&approved);
            assert_eq!(approved.status.code(), Some(1), "{answer}");
            assert_eq!(answer["error"], "refused", "{answer}");
            let messages = answer["messages"].to_string();
            assert!(
                messages.contains(&format!(
                    "which is {decision}: an approval cannot stand on it"
                )),
                "{answer}"
            );
        }
    }
}

#[test]
fn an_upstream_no_decision_stands_about_is_pending_and_is_approved() {
    let project = project("words-pending");
    succeeded(&project, &["judge", "approve", "N-1"]);
    says(
        &project,
        "ana",
        &Says {
            refusal: None,
            standing: "stale",
            causes: &[(
                "upstream_declared",
                "stale",
                "note:N-2 is pending (no decision stands about it)",
            )],
            origin: Some(&["judge approve note:N-2 --reason"]),
        },
    );
}

#[test]
fn an_upstream_approved_and_fresh_says_nothing() {
    let project = project("words-fresh");
    succeeded(&project, &["judge", "approve", "N-2", "N-1"]);
    says(
        &project,
        "ana",
        &Says {
            refusal: None,
            standing: "fresh",
            causes: &[],
            origin: None,
        },
    );
}

#[test]
fn an_upstream_denied_refuses_an_approval_and_breaks_what_stands_on_it() {
    let project = project("words-denied");
    succeeded(&project, &["judge", "approve", "N-2", "N-1"]);
    succeeded(&project, &["judge", "deny", "N-2"]);
    says(
        &project,
        "ana",
        &Says {
            refusal: Some("denied"),
            standing: "broken",
            causes: &[("upstream_declared", "broken", "note:N-2 is denied")],
            origin: Some(&[]),
        },
    );
}

#[test]
fn an_upstream_retired_refuses_an_approval_and_breaks_what_stands_on_it() {
    let project = project("words-retired");
    succeeded(&project, &["judge", "approve", "N-2", "N-1"]);
    succeeded(&project, &["judge", "retire", "N-2"]);
    says(
        &project,
        "ana",
        &Says {
            refusal: Some("retired"),
            standing: "broken",
            causes: &[("upstream_declared", "broken", "note:N-2 is retired")],
            origin: Some(&[]),
        },
    );
}

#[test]
fn an_upstream_deprecated_with_none_to_replace_it_breaks_what_stands_on_it() {
    let project = project("words-unreplaced");
    succeeded(&project, &["judge", "approve", "N-2", "N-1"]);
    succeeded(&project, &["judge", "deprecate", "N-2"]);
    says(
        &project,
        "ana",
        &Says {
            refusal: Some("deprecated"),
            standing: "broken",
            causes: &[(
                "upstream_declared",
                "broken",
                "note:N-2 is deprecated with no successor",
            )],
            origin: Some(&[]),
        },
    );
}

#[test]
fn an_upstream_deprecated_with_a_successor_is_followed_and_still_refuses_an_approval() {
    let project = project("words-superseded");
    fs::write(project.0.join("notes/N-3.md"), note(3, "replaces: [N-2]\n")).unwrap();
    succeeded(&project, &["judge", "approve", "N-2", "N-1"]);
    succeeded(&project, &["judge", "deprecate", "N-2"]);
    says(
        &project,
        "ana",
        &Says {
            refusal: Some("deprecated"),
            standing: "stale",
            causes: &[(
                "upstream_declared",
                "stale",
                "note:N-2 is deprecated; follow its successor",
            )],
            origin: Some(&[]),
        },
    );
}

#[test]
fn an_upstream_stale_passes_it_on_and_is_not_a_decision() {
    let project = project("words-stale");
    succeeded(&project, &["judge", "approve", "N-2", "N-1"]);
    fs::write(
        project.0.join("notes/N-2.md"),
        note(2, "").replace("The body.", "An edited body."),
    )
    .unwrap();
    says(
        &project,
        "ana",
        &Says {
            refusal: None,
            standing: "stale",
            causes: &[
                (
                    "upstream_changed",
                    "stale",
                    "note:N-2 changed since the approval pinned it",
                ),
                ("via", "stale", "note:N-2 does not hold (stale)"),
            ],
            origin: Some(&[
                "diff note:N-2",
                "judge same note:N-2",
                "judge approve --stale",
            ]),
        },
    );
}

#[test]
fn an_upstream_short_of_names_awaits_them_and_is_not_a_decision() {
    // Two different names of `maintainers` must approve a note.
    let rule = "[[group]]\nname = \"maintainers\"\nmembers = [\"ana\", \"bo\"]\n\n\
                [decide]\ndefault = [\"maintainers\"]\napprovals = 2\n\n";
    let decide = "[book.decide]\napprove = [\"maintainers\"]\napprovals = 2\n";
    let project = Project::with(
        "words-short",
        &linked_books(rule, decide),
        &note(1, "on: [N-2]\n"),
    );
    fs::write(project.0.join("notes/N-2.md"), note(2, "")).unwrap();
    // N-2 is approved by ana alone, and N-1 by both.
    succeeded(&project, &["judge", "approve", "N-2", "--actor", "ana"]);
    succeeded(&project, &["judge", "approve", "N-1", "--actor", "ana"]);
    succeeded(&project, &["judge", "approve", "N-1", "--actor", "bo"]);
    says(
        &project,
        "bo",
        &Says {
            refusal: None,
            standing: "stale",
            causes: &[(
                "upstream_declared",
                "stale",
                "note:N-2 is unseconded (its approval awaits the approvals of other names its rule asks for)",
            )],
            origin: Some(&["judge approve note:N-2 --actor"]),
        },
    );
}
