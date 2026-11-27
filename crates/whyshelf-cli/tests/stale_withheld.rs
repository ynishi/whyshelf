//! The approvals `stale` does not offer because the frame refuses them, on the binary
//! and a small project of files: each origin says them in `withheld`, with the document,
//! which approval it is and why the frame refuses it, and each is a line on stderr. A
//! pending upstream is refused when its link names what cannot be told now and when
//! approving it would close a cycle among the frames of approvals, and the refusal is
//! told again at an origin other than the one it was first told at.

mod common;

use std::fs;

use common::{Project, json, linked_books, note, stderr_lines, succeeded};

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
