//! Who judges, on the binary and a small project of files: `WHYSHELF_ACTOR` names
//! the actor of a judgement.

mod common;

use common::{Project, json};

#[test]
fn the_actor_comes_from_the_variable() {
    let project = Project::new("actor");
    let output = project.run(&["judge", "approve", "N-1"]);
    assert_eq!(output.status.code(), Some(0));
    let answer = json(&output);
    assert_eq!(answer["version"], 1);
    assert_eq!(answer["judged"][0]["record"]["actor"], "tester");
}
