//! The reading commands, on the binary and a small project of files: they create and
//! change no file, before an approval and after one.

mod common;

use common::{Project, READS};

#[test]
fn reading_commands_create_and_change_no_file() {
    let project = Project::new("reads");
    // Before any approval: no ledger directory exists, and none may be made.
    let before = project.files();
    for args in READS {
        project.run(args);
    }
    assert_eq!(project.files(), before, "before an approval");
    assert!(!project.0.join(".whyshelf/ledger").exists());

    assert_eq!(
        project.run(&["judge", "approve", "N-1"]).status.code(),
        Some(0)
    );
    let before = project.files();
    for args in READS {
        project.run(args);
    }
    assert_eq!(project.files(), before, "after an approval");
}
