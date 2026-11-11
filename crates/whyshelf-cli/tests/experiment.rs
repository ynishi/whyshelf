//! The template `experiment`, on the binary and an empty directory: `init
//! --template experiment` makes a project that `check` finds clean with no warning
//! and no name with nothing behind it, its plan's table written beside the
//! documents, and `next` names the plan's approval; a second `init` finds the
//! table in the way. The plan approved before the run takes a result after it,
//! which names the decision, the plan and the log; the table edited after the
//! plan's approval makes the plan stale, and the result with it.

mod common;

use std::fs;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use common::{Project, json};

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
        "whyshelf-experiment-{name}-{}-{count}-{nanos}",
        std::process::id()
    ));
    fs::create_dir_all(&dir).unwrap();
    Project(dir)
}

fn stderr(output: &std::process::Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

/// The binary run on `project`, checked to exit 0.
fn ran(project: &Project, args: &[&str]) -> serde_json::Value {
    let output = project.run(args);
    assert_eq!(
        output.status.code(),
        Some(0),
        "{args:?}: {}",
        stderr(&output)
    );
    json(&output)
}

/// Where the document of this key stands, and why.
fn standing_of(project: &Project, key: &str) -> (serde_json::Value, String) {
    let row = &ran(project, &["get", key])["row"];
    (row["standing"].clone(), row["causes"].to_string())
}

#[test]
fn init_experiment_checks_clean_with_its_table_and_names_the_plan_to_approve() {
    let project = empty("init");
    let answer = ran(&project, &["init", "--template", "experiment"]);
    let keys: Vec<&str> = answer["documents"]
        .as_array()
        .unwrap()
        .iter()
        .map(|d| d["key"].as_str().unwrap())
        .collect();
    assert_eq!(
        keys,
        [
            "finding:F-1",
            "requirement:REQ-1",
            "decision:DEC-1",
            "verification:V-1"
        ]
    );
    assert_eq!(
        answer["files"],
        serde_json::json!(["measurements/save-time.csv"])
    );
    fs::read(project.0.join("measurements/save-time.csv")).unwrap();
    // The plan, which stands on DEC-1 and REQ-1, and F-1, only grounded on.
    let whyshelf = format!("whyshelf --root={}", project.0.display());
    assert_eq!(
        answer["next"][2],
        format!("{whyshelf} judge approve --pending F-1 V-1 --reason \"read, and it holds\"")
    );

    let check = project.run(&["check"]);
    assert_eq!(check.status.code(), Some(0), "{}", stderr(&check));
    let answer = json(&check);
    assert_eq!(answer["clean"], true);
    assert_eq!(answer["absent"], serde_json::json!([]));
    assert_eq!(answer["uncovered"], serde_json::json!([]));
    assert_eq!(stderr(&check), "", "check warns");
    assert_eq!(project.run(&["fmt", "--check"]).status.code(), Some(0));

    // A second one finds the table in the way too, after the declaration.
    let output = project.run(&["init", "--template", "experiment"]);
    assert_eq!(output.status.code(), Some(2));
    let problems = &json(&output)["problems"];
    assert_eq!(problems.as_array().unwrap().len(), 6, "{problems}");
    assert_eq!(problems[1], "measurements/save-time.csv is already there");
}

#[test]
fn an_experiment_s_plan_approved_goes_stale_when_its_table_is_edited_and_takes_a_result() {
    let project = empty("run");
    ran(&project, &["init", "--template", "experiment"]);
    // Before the run: the plan, with what it stands on.
    ran(
        &project,
        &[
            "judge",
            "approve",
            "--pending",
            "F-1",
            "V-1",
            "--reason",
            "read, and it holds",
        ],
    );
    assert_eq!(standing_of(&project, "V-1").0, "fresh");

    // After the run: the log, and the result, which names the decision, its plan
    // and the log, and no sheet.
    fs::write(
        project.0.join("measurements/run-1.txt"),
        "size_mb,slowest_seconds\n1,0.04\n",
    )
    .unwrap();
    let made = ran(
        &project,
        &[
            "new",
            "verification",
            "--title",
            "Result: the four sizes timed",
            "method=result",
            "checks+=DEC-1",
            "plans+=V-1",
            "log+=measurements/run-1.txt",
        ],
    );
    assert_eq!(made["doc"]["key"], "verification:V-2");
    assert_eq!(made["unresolved_links"], serde_json::json!([]));
    ran(
        &project,
        &[
            "judge",
            "approve",
            "V-2",
            "--reason",
            "the run is as planned",
        ],
    );
    assert_eq!(standing_of(&project, "V-2").0, "fresh");
    assert_eq!(ran(&project, &["check"])["clean"], true);

    // The table edited after the plan's approval: the plan is stale, and the
    // result through it.
    fs::write(
        project.0.join("measurements/save-time.csv"),
        "size_mb,saves,slowest_expected_seconds\n1,5,0.5\n",
    )
    .unwrap();
    let (standing, causes) = standing_of(&project, "V-1");
    assert_eq!(standing, "stale");
    assert!(causes.contains("artifact_changed"), "{causes}");
    assert!(causes.contains("\"sheet\""), "{causes}");
    assert_eq!(standing_of(&project, "V-2").0, "stale");
    assert_eq!(
        project.run(&["stale", "--exit-code"]).status.code(),
        Some(1)
    );
}
