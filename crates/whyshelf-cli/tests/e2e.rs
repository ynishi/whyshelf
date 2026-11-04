//! The `whyshelf` binary, end to end, in a real Git repository.

mod common;

use common::{Repo, finding_text, row_where, rows};

const SPEC: &str = "---\ngrounds:\n  - \"finding/login-feels-slow\"\naccept:\n  pins:\n    - id: PIN-LOGIN-CACHE\n      contract: \"The session is reused for five minutes\"\n  hyp:\n    - id: HYP-FASTER-LOGIN\n      metric: \"median login time\"\n      target: \"under 2 s\"\n---\n\n## Contract\nThe session is reused for five minutes.\n";

const DESIGN: &str = "---\nspec: \"spec/session-reuse\"\ncrux:\n  question: \"Where is the session kept?\"\n  options:\n    - id: opt-memory\n      summary: \"Keep it in memory\"\n      chosen: true\n      reason: \"No new storage to secure\"\n    - id: opt-disk\n      summary: \"Keep it on disk\"\n      rejected: \"A file to protect and to clean up\"\nitems:\n  - number: 1\n    summary: \"Reuse the session inside the window\"\n    pins: [PIN-LOGIN-CACHE]\n---\n\n## Design Notes\nKeep it simple.\n";

const TEST_FILE: &str = "// @pin: PIN-LOGIN-CACHE\n#[test]\nfn the_session_is_reused() {}\n";

#[tokio::test]
async fn from_init_to_an_approved_item_and_a_status() {
    let repo = Repo::new().await;

    // init: the settings, the store directory, and the cache kept out of Git once.
    let init = repo
        .run(&["init", "--project", "demo", "--test-command", "true"])
        .await
        .ok();
    assert!(init.stdout.contains("created .whyshelf.toml"));
    let settings = repo.read(".whyshelf.toml").await;
    assert!(settings.starts_with("version = 1\nproject = \"demo\"\n"));
    assert_eq!(repo.read(".gitignore").await, ".whyshelf/cache/\n");
    let again = repo
        .run(&["init", "--project", "other", "--test-command", "true"])
        .await
        .failed();
    assert!(
        again.stderr.contains("never overwrites"),
        "{}",
        again.stderr
    );
    assert_eq!(repo.read(".whyshelf.toml").await, settings);
    repo.append(
        ".whyshelf.toml",
        "\n[stores]\ndefault = \"repo\"\n\n[tags.official]\narea = [\"auth\", \"build\"]\n\n[user]\nname = \"alice\"\n",
    )
    .await;
    repo.commit_all("Start the project").await;

    // finding new: a template, registered at once; filled in by hand afterwards.
    let created = repo
        .json(&[
            "finding",
            "new",
            "--type",
            "usage_observation",
            "--title",
            "Login feels slow",
        ])
        .await;
    assert_eq!(created["created"]["alias"], "finding/login-feels-slow");
    let path = created["created"]["path"].as_str().unwrap().to_owned();
    repo.write(
        &path,
        &finding_text("Login feels slow", "It takes eight seconds."),
    )
    .await;

    // add: a file written by hand is registered under the alias given; what is
    // registered already is reported, not registered twice.
    repo.write(
        "docs/finding/other.md",
        &finding_text("Other", "Something else."),
    )
    .await;
    let added = repo
        .json(&["add", "docs/finding/other.md", "--alias", "finding/other"])
        .await;
    assert_eq!(rows(&added, "registered").len(), 1);
    let again = repo.json(&["add", "docs"]).await;
    assert!(rows(&again, "registered").is_empty());
    assert_eq!(again["skipped"].as_array().unwrap().len(), 2);

    // CR: the check alone changes nothing; the approval says where it was made.
    let cr = repo
        .run(&["check", "cr", "finding/login-feels-slow"])
        .await
        .ok();
    assert!(cr.stdout.contains("passed"), "{}", cr.stdout);
    let approved = repo
        .run(&["check", "cr", "finding/login-feels-slow", "--approve"])
        .await
        .ok();
    // A CR records no revision, so the line names the gate and nothing about where
    // the working tree is.
    assert!(approved.stdout.contains("recorded"), "{}", approved.stdout);
    assert!(approved.stdout.contains("CR"), "{}", approved.stdout);
    assert!(!approved.stdout.contains("CR at"), "{}", approved.stdout);

    // CS: a template is not a Spec yet, so the gate refuses it; filled in, it passes.
    repo.run(&[
        "spec",
        "new",
        "spec/session-reuse",
        "--from",
        "finding/login-feels-slow",
    ])
    .await
    .ok();
    let refused = repo
        .run(&["check", "cs", "spec/session-reuse"])
        .await
        .failed();
    // An Invalid target is a violation like the others: on standard output, with
    // the exit status of a refused gate.
    assert!(refused.stdout.contains("invalid"), "{}", refused.stdout);
    repo.write("docs/spec/session-reuse.md", SPEC).await;
    repo.run(&["check", "cs", "spec/session-reuse"]).await.ok();
    repo.run(&["check", "cs", "spec/session-reuse", "--approve"])
        .await
        .ok();

    // CI.
    repo.run(&[
        "design",
        "new",
        "design/session-reuse",
        "--spec",
        "spec/session-reuse",
    ])
    .await
    .ok();
    repo.write("docs/design/session-reuse.md", DESIGN).await;
    repo.run(&["check", "ci", "design/session-reuse"])
        .await
        .ok();
    repo.run(&["check", "ci", "design/session-reuse", "--approve"])
        .await
        .ok();

    // CA: the tests carry the pin. A tree with uncommitted work is refused, and
    // there is no way around it; committed, the approval records the revision.
    repo.write("tests/session.rs", TEST_FILE).await;
    let dirty = repo
        .run(&["check", "ca", "design/session-reuse#1", "--approve"])
        .await
        .failed();
    assert!(dirty.stderr.contains("commit first"), "{}", dirty.stderr);
    let revision = repo.commit_all("Implement and approve").await;
    let ca = repo
        .run(&["check", "ca", "design/session-reuse#1"])
        .await
        .ok();
    assert!(ca.stdout.contains("passed"), "{}", ca.stdout);
    let approved = repo
        .run(&["check", "ca", "design/session-reuse#1", "--approve"])
        .await
        .ok();
    assert!(approved.stdout.contains("CA at"), "{}", approved.stdout);
    assert!(approved.stdout.contains(&revision), "{}", approved.stdout);

    // status: every entity with both axes, and the next command.
    let status = repo.json(&["status"]).await;
    let entities = rows(&status, "entities");
    let item = row_where(entities, "id", "design/session-reuse#1");
    assert_eq!(item["kind"], "item");
    assert_eq!(item["approval"], "Approved");
    assert_eq!(item["integration"], "Integrated");
    assert_eq!(item["pins"], "1/1");
    for alias in [
        "finding/login-feels-slow",
        "spec/session-reuse",
        "design/session-reuse",
    ] {
        let row = row_where(entities, "id", alias);
        assert_eq!(row["approval"], "Approved", "{alias}");
        assert_eq!(row["upstream"], "Ok", "{alias}");
    }
    assert_eq!(
        row_where(entities, "id", "finding/other")["approval"],
        "Draft"
    );
    assert!(
        row_where(entities, "id", "spec/session-reuse")["next"]
            .as_str()
            .unwrap()
            .contains("check cm"),
        "every item is integrated: the measurement is next"
    );

    // The table form says the same, in columns.
    let table = repo.run(&["status"]).await.ok();
    assert!(table.stdout.starts_with("ID"), "{}", table.stdout);
    assert!(table.stdout.contains("design/session-reuse#1"));

    // audit: four approvals.
    let audit = repo.json(&["audit"]).await;
    let approvals = rows(&audit, "approvals");
    let gates: Vec<&str> = approvals
        .iter()
        .map(|r| r["gate"].as_str().unwrap())
        .collect();
    assert_eq!(gates.len(), 4, "{gates:?}");
    for gate in ["CR", "CS", "CI", "CA"] {
        assert!(gates.contains(&gate), "{gate} in {gates:?}");
    }
    let ca_event = row_where(approvals, "gate", "CA")["event"]
        .as_str()
        .unwrap()
        .to_owned();

    // CM: a pass is a record of its own.
    let cm = repo
        .run(&["check", "cm", "spec/session-reuse", "pass"])
        .await
        .ok();
    assert!(cm.stdout.contains("CM"), "{}", cm.stdout);
    assert!(!cm.stdout.contains("CM at"), "{}", cm.stdout);
    let wiring = repo
        .run(&["check", "cm", "spec/session-reuse", "fail-wiring"])
        .await
        .failed();
    assert_eq!(
        wiring.code,
        Some(2),
        "a missing --item-hint is a usage error"
    );

    // get, trace, find.
    let got = repo.json(&["get", "design/session-reuse#1"]).await;
    assert_eq!(got["entity"]["kind"], "item");
    let trace = repo.json(&["trace", "spec/session-reuse"]).await;
    let nodes = rows(&trace, "nodes");
    row_where(nodes, "id", "finding/login-feels-slow");
    row_where(nodes, "id", "design/session-reuse");
    let found = repo.json(&["find", "kind:finding", "--limit", "1"]).await;
    assert_eq!(rows(&found, "found").len(), 1);
    assert_eq!(found["page"]["total"], 2);
    assert!(found["next_cursor"].is_string(), "{found}");

    // tag: a change that starts with a hyphen is a change, not a flag.
    repo.run(&["tag", "finding/other", "+area:auth", "+wip"])
        .await
        .ok();
    repo.run(&["tag", "finding/other", "-wip"]).await.ok();
    let tagged = repo.json(&["find", "tag:area:auth", "tag:wip"]).await;
    assert_eq!(tagged["page"]["total"], 0);
    let tagged = repo.json(&["find", "tag:area:auth"]).await;
    assert_eq!(tagged["page"]["total"], 1);

    // revoke: the approval stays in the log, shown as withdrawn, and the item is
    // no longer approved.
    repo.run(&["revoke", &ca_event, "--reason", "the tests were too thin"])
        .await
        .ok();
    let audit = repo.json(&["audit"]).await;
    let revoked = row_where(rows(&audit, "approvals"), "event", &ca_event);
    assert_eq!(revoked["revoked"], "revoked");
    let status = repo.json(&["status"]).await;
    let item = row_where(rows(&status, "entities"), "id", "design/session-reuse#1");
    assert_eq!(item["approval"], "Draft");
}

#[tokio::test]
async fn two_branches_that_each_add_something_merge_and_work() {
    let repo = Repo::new().await;
    repo.init_project().await;

    // Each branch adds a Finding, fills it in and approves it.
    for (branch, title) in [("branch-a", "Alpha finding"), ("branch-b", "Beta finding")] {
        repo.git(&["checkout", "-q", "-b", branch, "main"]).await;
        let created = repo
            .json(&[
                "finding",
                "new",
                "--type",
                "usage_observation",
                "--title",
                title,
            ])
            .await;
        let path = created["created"]["path"].as_str().unwrap().to_owned();
        let alias = created["created"]["alias"].as_str().unwrap().to_owned();
        repo.write(&path, &finding_text(title, "Seen twice.")).await;
        repo.run(&["check", "cr", &alias, "--approve"]).await.ok();
        repo.commit_all(&format!("Add {title}")).await;
    }

    // Merge both into main. Every event is a file of its own, so they add up; the
    // generated map is the one file both branches rewrote, and it may conflict.
    repo.git(&["checkout", "-q", "main"]).await;
    repo.git(&["merge", "-q", "--no-edit", "branch-a"]).await;
    if !repo
        .git_succeeds(&["merge", "-q", "--no-edit", "branch-b"])
        .await
    {
        let conflicted = repo.git(&["diff", "--name-only", "--diff-filter=U"]).await;
        assert_eq!(
            conflicted, ".whyshelf/map.json",
            "only the generated map may conflict"
        );
        repo.git(&["checkout", "--ours", "--", ".whyshelf/map.json"])
            .await;
        repo.git(&["add", ".whyshelf/map.json"]).await;
        repo.git(&["commit", "-q", "--no-edit"]).await;
    }

    // `rebuild` writes the map again from the events, and everything works.
    repo.run(&["rebuild"]).await.ok();
    let status = repo.json(&["status"]).await;
    let entities = rows(&status, "entities");
    for alias in ["finding/alpha-finding", "finding/beta-finding"] {
        let row = row_where(entities, "id", alias);
        assert_eq!(row["approval"], "Approved", "{alias}");
    }
    assert!(
        status["notes"].is_null(),
        "nothing is left to settle: {status}"
    );
    for alias in ["finding/alpha-finding", "finding/beta-finding"] {
        repo.run(&["check", "cr", alias]).await.ok();
    }
    let map = repo.read(".whyshelf/map.json").await;
    assert!(map.contains("finding/alpha-finding") && map.contains("finding/beta-finding"));
    assert!(!map.contains("<<<<<<<"));
    let audit = repo.json(&["audit"]).await;
    assert_eq!(rows(&audit, "approvals").len(), 2);
}

#[tokio::test]
async fn a_command_outside_a_project_fails_with_a_reason_and_a_usage_error_is_two() {
    let repo = Repo::new().await;
    let status = repo.run(&["status"]).await.failed();
    assert_eq!(status.code, Some(1));
    assert!(
        status.stderr.contains("cannot open the workspace"),
        "{}",
        status.stderr
    );
    let usage = repo.run(&["no-such-command"]).await.failed();
    assert_eq!(usage.code, Some(2));
    let bad_format = repo.run(&["status", "--format", "xml"]).await.failed();
    assert_eq!(bad_format.code, Some(2));
}

#[tokio::test]
async fn the_schema_of_the_map_is_printed_without_a_project() {
    let repo = Repo::new().await;
    let schema = repo.run(&["schema", "map"]).await.ok().json();
    assert_eq!(schema["title"], "whyshelf map");
    let migrate = repo.run(&["migrate"]).await.ok();
    assert!(migrate.stdout.contains("nothing to do"));
}
