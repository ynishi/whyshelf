//! Who decided, on the binary and a small project of files: `get`, `get --approved`
//! and `status` show the decision that stands, with the record's id, actor, time and
//! reason.

mod common;

use common::{Project, json};

#[test]
fn every_row_shows_the_decision_that_stands() {
    let project = Project::new("decision");
    let row = |args: &[&str]| json(&project.run(args))["row"].clone();
    assert_eq!(row(&["get", "N-1"])["decision"], serde_json::Value::Null);

    let output = project.run(&["judge", "approve", "N-1", "--reason", "read it whole"]);
    assert_eq!(output.status.code(), Some(0));
    let record = json(&output)["judged"][0]["record"].clone();
    let status = json(&project.run(&["status"]));
    let in_status = status["docs"][0]["decision"].clone();
    for (args, decision) in [
        (
            &["get", "N-1"][..],
            row(&["get", "N-1"])["decision"].clone(),
        ),
        (
            &["get", "N-1", "--approved"],
            row(&["get", "N-1", "--approved"])["decision"].clone(),
        ),
        (&["status"], in_status),
    ] {
        assert_eq!(decision["judgement"], record["id"], "{args:?}");
        assert_eq!(decision["actor"], "tester", "{args:?}");
        assert_eq!(decision["at"], record["at"], "{args:?}");
        assert_eq!(decision["reason"], "read it whole", "{args:?}");
    }
}
