//! The origin in `stale`, on the binary and a small project of files: `stale` names
//! the document an edit started from, and nothing before an approval.

mod common;

use std::fs;

use common::{NOTE, Project, json};

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
