//! Two branches that took the same keys, put together: the merge result is made by
//! copying the files of one project into another (no Git). `check` finds the keys
//! written twice before anything is merged, and `rekey` renumbers the branch's
//! documents and keeps every approval, the manual's "Working from several branches".
//! The other cases of `rekey` are in `rekey.rs`.

mod common;

use std::collections::BTreeSet;
use std::fs;
use std::path::Path;

use common::{Project, json};

/// Copy every file under `from` to the same place under `to`.
fn copy_tree(from: &Path, to: &Path) {
    fs::create_dir_all(to).unwrap();
    for entry in fs::read_dir(from).unwrap() {
        let path = entry.unwrap().path();
        let target = to.join(path.file_name().unwrap());
        // A directory is what can be listed (the workspace's arch-lint refuses
        // `is_dir`, a blocking call, in every file).
        if fs::read_dir(&path).is_ok() {
            copy_tree(&path, &target);
        } else {
            fs::copy(&path, &target).unwrap();
        }
    }
}

fn names(dir: &Path) -> BTreeSet<String> {
    fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect()
}

fn ok(project: &Project, args: &[&str]) -> serde_json::Value {
    let output = project.run(args);
    assert_eq!(
        output.status.code(),
        Some(0),
        "{args:?}: {}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    json(&output)
}

/// A requirement, a decision that serves it and a verification that checks the
/// decision: what the sample's rules ask for together. Returns the three files.
fn add_group(project: &Project, word: &str) -> [String; 3] {
    let file = |answer: serde_json::Value| answer["file"].as_str().unwrap().to_owned();
    let req = file(ok(
        project,
        &[
            "new",
            "requirement",
            "--title",
            &format!("{word} requirement"),
        ],
    ));
    let dec = file(ok(
        project,
        &[
            "new",
            "decision",
            "--title",
            &format!("{word} decision"),
            "serves=REQ-2",
            "rejected+=a way not taken",
        ],
    ));
    let ver = file(ok(
        project,
        &[
            "new",
            "verification",
            "--title",
            &format!("{word} verification"),
            "method=review",
            "checks=DEC-2",
        ],
    ));
    ok(
        project,
        &["judge", "approve", "--pending", "--reason", "read"],
    );
    [req, dec, ver]
}

#[test]
fn keys_taken_on_two_branches_are_found_on_the_merge_result_and_renumbering_keeps_the_approvals() {
    // The main branch, and a branch cut from it.
    let main = Project::empty("parallel-main");
    ok(&main, &["init", "--template", "sample"]);
    ok(
        &main,
        &["judge", "approve", "--pending", "--reason", "read"],
    );
    let branch = Project::empty("parallel-branch");
    copy_tree(&main.0, &branch.0);

    // Each adds a group, and both take REQ-2, DEC-2 and V-2.
    let landed = add_group(&main, "Landed");
    let local = add_group(&branch, "Local");
    assert_eq!(landed, local);
    assert_eq!(
        landed,
        [
            "books/requirement/REQ-2.md",
            "books/decision/DEC-2.md",
            "books/verification/V-2.md",
        ]
        .map(str::to_owned)
    );

    // The merge result: the branch's files under other names, and its records.
    let ledger = ".whyshelf/ledger";
    let had = names(&main.0.join(ledger));
    for record in names(&branch.0.join(ledger)).difference(&had) {
        fs::copy(
            branch.0.join(ledger).join(record),
            main.0.join(ledger).join(record),
        )
        .unwrap();
    }
    let copied: Vec<String> = local
        .iter()
        .map(|file| file.replace(".md", "-b.md"))
        .collect();
    for (file, to) in local.iter().zip(&copied) {
        fs::copy(branch.0.join(file), main.0.join(to)).unwrap();
    }

    // `check` fails before the merge: each of the six files is invalid for its key.
    let check = main.run(&["check"]);
    assert_eq!(check.status.code(), Some(1));
    let answer = json(&check);
    let invalid: BTreeSet<String> = answer["health"]["invalid"]
        .as_array()
        .unwrap()
        .iter()
        .map(|row| {
            let file = row["file"].as_str().unwrap().to_owned();
            let problems = row["problems"].to_string();
            let key = Path::new(&file)
                .file_stem()
                .unwrap()
                .to_string_lossy()
                .trim_end_matches("-b")
                .to_owned();
            assert!(
                problems.contains(&format!(":{key}` is also written by")),
                "{file}: {problems}"
            );
            file
        })
        .collect();
    let expected: BTreeSet<String> = landed.iter().chain(&copied).cloned().collect();
    assert_eq!(invalid, expected, "{answer}");
    // Each key two files write is told once, with both files and their identities.
    let collisions = answer["collisions"].as_array().unwrap();
    assert_eq!(collisions.len(), 3, "{answer}");
    for collision in collisions {
        let files = collision["files"].as_array().unwrap();
        assert_eq!(files.len(), 2, "{collision}");
        assert!(
            files.iter().all(|file| file["id"].is_string()),
            "{collision}"
        );
    }

    // Renumber the branch's group with `rekey`, the requirement first: the key `new` takes
    // now (the number after the highest a file of the book writes, 2), the file where
    // `new` puts it, the `key:`, and the links of the group that mean it. Which document a
    // link meant is read from the approval of the document that wrote it (the decision
    // on the main branch pinned the main branch's requirement, the branch's decision the
    // branch's), so no word is given.
    let id_of = |file: &str| -> String {
        collisions
            .iter()
            .flat_map(|collision| collision["files"].as_array().unwrap())
            .find(|candidate| candidate["path"] == file)
            .unwrap_or_else(|| panic!("{file} is in no collision: {answer}"))["id"]
            .as_str()
            .unwrap()
            .to_owned()
    };
    let renumbered = [
        ("REQ-2", "REQ-3", "books/requirement/REQ-3.md"),
        ("DEC-2", "DEC-3", "books/decision/DEC-3.md"),
        ("V-2", "V-3", "books/verification/V-3.md"),
    ];
    for ((old, new, to), from) in renumbered.iter().zip(&copied) {
        let answer = ok(&main, &["rekey", &id_of(from)]);
        assert_eq!(answer["written"]["created"], *to, "{answer}");
        assert_eq!(answer["written"]["removed"], from.as_str(), "{answer}");
        assert!(
            answer["moved"]["from_key"].as_str().unwrap().ends_with(old),
            "{answer}"
        );
        assert!(
            answer["moved"]["to_key"].as_str().unwrap().ends_with(new),
            "{answer}"
        );
        // The links that mean the document follow it; the other branch's stay.
        for linked in answer["rewritten"].as_array().unwrap() {
            assert_eq!(linked["why"], "pinned", "{answer}");
        }
        for linked in answer["stayed"].as_array().unwrap() {
            assert_eq!(linked["why"], "pinned", "{answer}");
        }
        // The approval of the document that took the new key holds (the first row; those
        // that link to it hold once their own key is renumbered, below).
        assert_eq!(answer["approvals"][0]["standing"], "fresh", "{answer}");
    }

    ok(&main, &["check"]);
    ok(&main, &["fmt", "--check"]);
    ok(&main, &["stale", "--exit-code"]);
    let status = ok(&main, &["status"]);
    let docs = status["docs"].as_array().unwrap();
    // The sample's four, the landed group and the renumbered group.
    assert_eq!(docs.len(), 10, "{status}");
    for row in docs {
        assert_eq!(row["standing"], "fresh", "{row}");
    }
    let keys: BTreeSet<&str> = docs
        .iter()
        .map(|row| row["doc"]["key"].as_str().unwrap())
        .collect();
    for key in [
        "requirement:REQ-2",
        "requirement:REQ-3",
        "decision:DEC-3",
        "verification:V-3",
    ] {
        assert!(keys.contains(key), "{key}: {status}");
    }
}
