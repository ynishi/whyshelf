//! A path that names no file, on the binary and a small project of files: `check`
//! lists it and stays clean, a path written in another case than the file's name
//! among them; `judge approve` lists it; `get` and `status` show it as a notice.

mod common;

use std::fs;

use common::{BOOKS, NOTE, Project, json};

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
