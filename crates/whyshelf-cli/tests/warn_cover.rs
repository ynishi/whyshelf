//! A cover of `warn`, on the binary and a small project of files: a gap of a `warn`
//! cover leaves `check` at 0, listed in the answer and warned of on stderr; a gap of
//! a cover of `true` exits 1 and warns of nothing.

mod common;

use common::{BOOKS, NOTE, Project, json};

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
