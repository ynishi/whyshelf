//! The default preset `file`, on the binary and small projects of files: a project
//! with `[defaults] artifact = "file"` behaves as one whose links write
//! `space = "file"`, and its ledger records hold no preset.

mod common;

use std::fs;

use common::{BOOKS, NOTE, Project, json};

#[test]
fn a_project_with_the_default_preset_file_behaves_as_one_that_writes_the_space() {
    let note = NOTE.replace(
        "title: \"A note\"\n",
        "title: \"A note\"\nfiles: [\"src/a.rs\"]\n",
    );
    let written = Project::with(
        "written",
        &format!(
            "{BOOKS}[[book.link]]\nname = \"files\"\ntarget = \"artifact\"\nspace = \"file\"\nmeans = \"depends\"\n"
        ),
        &note,
    );
    let defaulted = Project::with(
        "defaulted",
        &format!(
            "[defaults]\nartifact = \"file\"\n{BOOKS}[[book.link]]\nname = \"files\"\ntarget = \"artifact\"\n"
        ),
        &note,
    );
    let both = [&written, &defaulted];
    for project in both {
        fs::create_dir_all(project.0.join("src")).unwrap();
        fs::write(project.0.join("src/a.rs"), "fn a() {}\n").unwrap();
    }

    // The map tells the same link, and the preset beside it where it came from one.
    let links: Vec<serde_json::Value> = both
        .iter()
        .map(|p| json(&p.run(&["map"]))["links"][0].clone())
        .collect();
    assert_eq!(links[0]["target"], links[1]["target"]);
    assert_eq!(links[0]["means"], links[1]["means"]);
    assert_eq!(
        (&links[0]["preset"], &links[0]["preset_by_default"]),
        (&serde_json::Value::Null, &serde_json::json!(false))
    );
    assert_eq!(
        (&links[1]["preset"], &links[1]["preset_by_default"]),
        (&serde_json::json!("file"), &serde_json::json!(true))
    );

    let standing = |project: &Project| {
        let row = &json(&project.run(&["get", "N-1"]))["row"];
        (row["standing"].clone(), row["causes"].clone())
    };
    for project in both {
        assert_eq!(
            project.run(&["judge", "approve", "N-1"]).status.code(),
            Some(0)
        );
    }
    assert_eq!(standing(&written), standing(&defaulted));
    assert_eq!(
        json(&written.run(&["check"])),
        json(&defaulted.run(&["check"]))
    );

    // An edit of the file makes both approvals stale, for the same cause; the
    // approval of the default names no preset in the ledger.
    for project in both {
        fs::write(project.0.join("src/a.rs"), "fn a() { b() }\n").unwrap();
    }
    let (a, b) = (standing(&written), standing(&defaulted));
    assert_eq!(a, b);
    assert_eq!(a.0, "stale");
    assert!(a.1.to_string().contains("artifact_changed"), "{}", a.1);
    let ledger: Vec<String> = defaulted
        .files()
        .into_iter()
        .filter(|(path, _)| path.to_string_lossy().contains("ledger"))
        .filter_map(|(_, bytes)| Some(String::from_utf8(bytes?).unwrap()))
        .collect();
    assert_eq!(ledger.len(), 1);
    assert!(!ledger[0].contains("preset"), "{}", ledger[0]);
}
