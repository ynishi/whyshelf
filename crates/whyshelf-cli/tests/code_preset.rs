//! The built-in preset `code`, on the binary and a small project of files: a link
//! that writes `preset = "code"` is one to files that pins only that each is there,
//! so an edit of the file leaves its approval fresh, and the file removed breaks
//! it, with `moves` naming the gone file.

mod common;

use std::fs;

use common::{BOOKS, NOTE, Project, json};

/// A project whose note N-1 links `src/a.rs` through the preset `code`, approved.
fn approved(name: &str) -> Project {
    let note = NOTE.replace(
        "title: \"A note\"\n",
        "title: \"A note\"\ncode: [\"src/a.rs\"]\n",
    );
    let project = Project::with(
        name,
        &format!(
            "{BOOKS}[[book.link]]\nname = \"code\"\ntarget = \"artifact\"\npreset = \"code\"\n"
        ),
        &note,
    );
    fs::create_dir_all(project.0.join("src")).unwrap();
    fs::write(project.0.join("src/a.rs"), "fn a() {}\n").unwrap();
    let out = project.run(&["judge", "approve", "N-1"]);
    assert_eq!(
        out.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    project
}

fn standing(project: &Project) -> (serde_json::Value, String) {
    let row = &json(&project.run(&["get", "N-1"]))["row"];
    (row["standing"].clone(), row["causes"].to_string())
}

#[test]
fn the_preset_code_is_files_by_presence_that_depends() {
    let project = approved("code-map");
    let link = &json(&project.run(&["map"]))["links"][0];
    assert_eq!(link["preset"], "code", "{link}");
    assert_eq!(link["preset_by_default"], false);
    assert_eq!(link["means"], "depends");
    assert_eq!(link["target"]["space"], "file");
    assert_eq!(link["target"]["compare"], "presence");
}

#[test]
fn an_edit_of_the_code_keeps_the_approval_fresh_and_its_removal_breaks_it() {
    let project = approved("code-edit");
    assert_eq!(standing(&project).0, "fresh");

    fs::write(project.0.join("src/a.rs"), "fn a() { b() }\n").unwrap();
    assert_eq!(standing(&project).0, "fresh");
    let stale = project.run(&["stale", "--exit-code"]);
    assert_eq!(stale.status.code(), Some(0));

    fs::remove_file(project.0.join("src/a.rs")).unwrap();
    let (standing, causes) = standing(&project);
    assert_eq!(standing, "broken");
    assert!(causes.contains("artifact_gone"), "{causes}");
    let moves = json(&project.run(&["moves"]));
    let absent = moves["absent"].as_array().unwrap();
    assert_eq!(absent.len(), 1, "{moves}");
    assert_eq!(absent[0]["name"], "src/a.rs");
}
