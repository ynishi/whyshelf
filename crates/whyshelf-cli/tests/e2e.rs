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
    assert!(table.stdout.contains("\nID"), "{}", table.stdout);
    assert!(table.stdout.contains("design/session-reuse#1"));
    // It opens with the context line, which says where the tool is pointed.
    let first = table.stdout.lines().next().unwrap();
    assert!(
        first.starts_with("[context: project=demo, worktree=") && first.ends_with("(main)]"),
        "{first}"
    );
    assert_eq!(status["context"]["project"], "demo", "{status}");
    assert_eq!(status["context"]["branch"], "main");
    assert!(status["context"]["revision"].as_str().unwrap().len() >= 40);

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

    // A clone that has no baseline ref (a single-branch CI checkout): the question
    // "has it reached the baseline" is open, and the commands go on.
    let settings = repo.read(".whyshelf.toml").await;
    repo.append(
        ".whyshelf.toml",
        "\n[git]\nbaseline = \"origin/not-fetched\"\n",
    )
    .await;
    let open = repo.run(&["status", "--format", "json"]).await.ok().json();
    let item = row_where(rows(&open, "entities"), "id", "design/session-reuse#1");
    assert_eq!(item["integration"], "Unknown", "{open}");
    assert_eq!(item["approval"], "Approved");
    repo.write(".whyshelf.toml", &settings).await;

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
async fn the_exit_status_is_zero_one_two_or_three_and_each_means_one_thing() {
    let repo = Repo::new().await;

    // 2: the request is wrong. Outside a project nothing can be opened; an unknown
    // command and an unknown format are the parser's.
    let outside = repo.run(&["status"]).await.failed();
    assert_eq!(outside.code, Some(2));
    assert!(
        outside.stderr.contains("cannot open the workspace"),
        "{}",
        outside.stderr
    );
    assert_eq!(repo.run(&["no-such-command"]).await.failed().code, Some(2));
    assert_eq!(
        repo.run(&["status", "--format", "xml"]).await.failed().code,
        Some(2)
    );

    repo.init_project().await;
    // 0: it worked.
    repo.run(&["status"]).await.ok();
    let missing = repo.run(&["get", "finding/nothing"]).await.failed();
    assert_eq!(
        missing.code,
        Some(2),
        "a name that is not there is a wrong request"
    );

    // 1: a check stopped it. The template of a Spec is not a Spec yet.
    repo.run(&[
        "finding",
        "new",
        "--type",
        "usage_observation",
        "--title",
        "Login feels slow",
    ])
    .await
    .ok();
    repo.write(
        "docs/finding/login-feels-slow.md",
        &finding_text("Login feels slow", "It takes eight seconds."),
    )
    .await;
    repo.run(&["check", "cr", "finding/login-feels-slow", "--approve"])
        .await
        .ok();
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
    assert_eq!(refused.code, Some(1));
    let refused = repo
        .run(&["check", "cs", "spec/session-reuse", "--approve"])
        .await
        .failed();
    assert_eq!(
        refused.code,
        Some(1),
        "a refused approval is a refusal of the gate"
    );

    // 3: the run failed: an event file of a version this build does not read cannot
    // be loaded, whatever the command. The message says where to look.
    repo.write(".whyshelf/events/ffff.json", "{\"v\": 2}\n")
        .await;
    let unreadable = repo.run(&["status"]).await.failed();
    assert_eq!(unreadable.code, Some(3), "{}", unreadable.stderr);
    assert!(
        unreadable.stderr.contains("whyshelf migrate"),
        "{}",
        unreadable.stderr
    );
}

#[tokio::test]
async fn migrate_counts_what_is_there_and_does_not_say_newest() {
    let repo = Repo::new().await;
    // Not a project: nothing is read, so nothing is called readable.
    let outside = repo.run(&["migrate"]).await.failed();
    assert_eq!(outside.code, Some(2));
    assert!(
        outside.stderr.contains("not a whyshelf project"),
        "{}",
        outside.stderr
    );

    repo.init_project().await;
    repo.run(&[
        "finding",
        "new",
        "--type",
        "usage_observation",
        "--title",
        "Login feels slow",
    ])
    .await
    .ok();
    let events = repo.run(&["migrate"]).await.ok();
    // The table's shape: what is held, the versions found with how many, and what
    // this build reads.
    let line = events
        .stdout
        .lines()
        .find(|l| l.starts_with("events (repo)"))
        .unwrap_or_else(|| panic!("{}", events.stdout));
    assert!(
        line.contains("v1: 1") && line.contains("reads: v1"),
        "{line}"
    );
    assert!(events.stdout.contains("settings"), "{}", events.stdout);
    assert!(events.stdout.contains("readable"), "{}", events.stdout);
    for word in ["newest", "latest", "up to date"] {
        assert!(!events.stdout.contains(word), "{word} in {}", events.stdout);
    }
    let before = repo.read(".whyshelf.toml").await;

    // A derived file of an old version is shown, with how to renew it, and judges nothing.
    repo.write(".whyshelf/map.json", "{\"version\": 0, \"entities\": {}}\n")
        .await;
    let old_map = repo.run(&["migrate"]).await.ok();
    let map_line = old_map
        .stdout
        .lines()
        .find(|l| l.starts_with("map"))
        .unwrap();
    assert!(
        map_line.contains("v0: 1") && map_line.contains("whyshelf rebuild"),
        "{map_line}"
    );

    // A file of a version this build does not read: counted, and a refusal.
    repo.write(".whyshelf/events/ffff.json", "{\"v\": 2}\n")
        .await;
    repo.write(".whyshelf/events/eeee.json", "not json\n").await;
    let unreadable = repo.run(&["migrate"]).await.failed();
    assert_eq!(unreadable.code, Some(1));
    let line = unreadable
        .stdout
        .lines()
        .find(|l| l.starts_with("events (repo)"))
        .unwrap();
    assert!(
        line.contains("v1: 1") && line.contains("v2: 1") && line.contains("unknown: 1"),
        "{line}"
    );
    assert!(
        unreadable.stdout.contains("unreadable"),
        "{}",
        unreadable.stdout
    );
    let as_json = repo
        .run(&["migrate", "--format", "json"])
        .await
        .failed()
        .json();
    let names: Vec<&str> = rows(&as_json, "versions")
        .iter()
        .map(|r| r["what"].as_str().unwrap())
        .collect();
    assert_eq!(
        &names[..3],
        ["settings", "events (repo)", "events (private)"]
    );
    assert!(
        as_json["verdict"]["verdict"]
            .as_str()
            .unwrap()
            .contains("unreadable")
    );

    // Nothing was written.
    assert_eq!(repo.read(".whyshelf.toml").await, before);
}

#[tokio::test]
async fn the_table_migrate_prints_has_this_shape() {
    let repo = Repo::new().await;
    repo.write(
        ".whyshelf.toml",
        "version = 1\nproject = \"demo\"\nproject_id = \"0192f3a4-5b6c-7d8e-9fa0-b1c2d3e4f506\"\n[test]\ncommand = \"true\"\n",
    )
    .await;
    repo.write(".whyshelf/events/a.json", "{\"v\": 1}\n").await;
    repo.write(".whyshelf/events/b.json", "{\"v\": 1}\n").await;
    repo.write(".whyshelf/map.json", "{\"version\": 0}\n").await;
    let migrate = repo.run(&["migrate"]).await.ok();
    let expected = "\
Versions:
WHAT              FOUND    BUILD      NOTE
settings          v1: 1    reads: v1  -
events (repo)     v1: 2    reads: v1  -
events (private)  no file  reads: v1  -
map               v0: 1    reads: v1  derived: `whyshelf rebuild` writes it again

verdict  readable: every file counted is of a version this build reads
";
    assert_eq!(migrate.stdout, expected);
}

#[tokio::test]
async fn a_settings_file_without_an_id_is_refused_with_the_line_to_add_and_migrate_goes_on() {
    let repo = Repo::new().await;
    repo.init_project().await;
    let settings = repo.read(".whyshelf.toml").await;
    let without: String = settings
        .lines()
        .filter(|l| !l.starts_with("project_id"))
        .map(|l| format!("{l}\n"))
        .collect();
    repo.write(".whyshelf.toml", &without).await;

    let refused = repo.run(&["status"]).await.failed();
    assert_eq!(refused.code, Some(2));
    let line = refused
        .stderr
        .lines()
        .find(|l| l.starts_with("project_id = \""))
        .unwrap_or_else(|| panic!("{}", refused.stderr));
    let id = line
        .trim_start_matches("project_id = \"")
        .trim_end_matches('"');
    // The shape of a UUID, and its version digit: 7.
    assert_eq!(id.len(), 36, "{id}");
    assert_eq!(id.as_bytes()[14], b'7', "{id}");
    assert_eq!(
        repo.read(".whyshelf.toml").await,
        without,
        "the file is not touched"
    );

    // `migrate` does not stop: the private store's row says it was not looked at,
    // and the judgement is that not everything was read.
    let migrate = repo.run(&["migrate"]).await.failed();
    assert_eq!(migrate.code, Some(1));
    let private = migrate
        .stdout
        .lines()
        .find(|l| l.starts_with("events (private)"))
        .unwrap();
    assert!(private.contains("not looked at"), "{private}");
    assert!(migrate.stdout.contains("incomplete"), "{}", migrate.stdout);

    // The printed line is a version-7 id, and printing it changes no file.
    let printed = repo.run(&["init", "--print-project-id"]).await.ok();
    assert!(
        printed.stdout.starts_with("project_id = \""),
        "{}",
        printed.stdout
    );
    assert_eq!(repo.read(".whyshelf.toml").await, without);
}

#[tokio::test]
async fn the_schema_of_the_map_is_printed_without_a_project() {
    let repo = Repo::new().await;
    let schema = repo.run(&["schema", "map"]).await.ok().json();
    assert_eq!(schema["title"], "whyshelf map");
}

/// The names in a directory, sorted.
async fn names_in(dir: &std::path::Path) -> Vec<String> {
    let mut names = Vec::new();
    let mut listing = tokio::fs::read_dir(dir).await.unwrap();
    while let Some(entry) = listing.next_entry().await.unwrap() {
        names.push(entry.file_name().to_string_lossy().into_owned());
    }
    names.sort();
    names
}

#[tokio::test]
async fn two_projects_of_one_name_have_two_private_stores_and_a_worktree_shares_one() {
    let first = Repo::new().await;
    first.init_project().await;
    let data = first.data();
    // A private Finding: the default store of a document, unless the settings say repo.
    let settings = first
        .read(".whyshelf.toml")
        .await
        .replace("default = \"repo\"", "default = \"private\"");
    first.write(".whyshelf.toml", &settings).await;
    first.commit_all("Private by default").await;
    first
        .run(&[
            "finding",
            "new",
            "--type",
            "usage_observation",
            "--title",
            "Login feels slow",
        ])
        .await
        .ok();
    let id_of = |text: &str| {
        text.lines()
            .find_map(|l| l.strip_prefix("project_id = \""))
            .map(|rest| rest.trim_end_matches('"').to_owned())
            .unwrap()
    };
    let one = id_of(&settings);
    assert_eq!(names_in(&data).await, [format!("demo-{one}")]);

    // A second project with the same name and another id, on the same data dir.
    let second = Repo::new().await;
    second.init_project().await;
    let own = second.read(".whyshelf.toml").await;
    second
        .write(
            ".whyshelf.toml",
            &own.replace("default = \"repo\"", "default = \"private\""),
        )
        .await;
    second.commit_all("Private by default").await;
    let two = id_of(&own);
    assert_ne!(one, two);
    let other = second
        .command()
        .env("WHYSHELF_DATA_DIR", &data)
        .args([
            "finding",
            "new",
            "--type",
            "usage_observation",
            "--title",
            "Other",
        ])
        .output()
        .await
        .unwrap();
    assert!(other.status.success());
    let stores = names_in(&data).await;
    let mut expected = vec![format!("demo-{one}"), format!("demo-{two}")];
    expected.sort();
    assert_eq!(stores, expected);

    // A worktree of the first project has the same settings file, so the same store.
    first.commit_all("A Finding").await;
    let worktree = first.root().with_extension("worktree");
    first
        .git(&[
            "worktree",
            "add",
            "-q",
            "-b",
            "topic",
            worktree.to_str().unwrap(),
        ])
        .await;
    let seen = first
        .command()
        .current_dir(&worktree)
        .args(["get", "finding/login-feels-slow", "--format", "json"])
        .output()
        .await
        .unwrap();
    // The Finding lives in the private store, which the worktree finds by the id.
    let text = String::from_utf8_lossy(&seen.stdout).into_owned();
    assert!(
        seen.status.success(),
        "{text}\n{}",
        String::from_utf8_lossy(&seen.stderr)
    );
    let after = names_in(&data).await;
    assert_eq!(
        after.len(),
        2,
        "the worktree made no store of its own: {after:?}"
    );
}

#[tokio::test]
async fn the_status_exported_to_a_file_does_not_carry_the_context_line() {
    let repo = Repo::new().await;
    repo.init_project().await;
    repo.append(".whyshelf.toml", "\n[export]\nstatus_md = true\n")
        .await;
    repo.commit_all("Export the status").await;
    repo.run(&[
        "finding",
        "new",
        "--type",
        "usage_observation",
        "--title",
        "Login feels slow",
    ])
    .await
    .ok();
    let shown = repo.run(&["status"]).await.ok();
    assert!(
        shown.stdout.starts_with("[context: project=demo"),
        "{}",
        shown.stdout
    );
    let markdown = repo.run(&["status", "--format", "markdown"]).await.ok();
    assert!(
        markdown.stdout.starts_with("[context: project=demo"),
        "{}",
        markdown.stdout
    );
    let exported = repo.read(".whyshelf/status.md").await;
    assert!(!exported.contains("[context"), "{exported}");
    assert!(
        !exported.contains(&repo.root().display().to_string()),
        "{exported}"
    );
}

#[tokio::test]
async fn before_the_first_commit_a_command_says_a_commit_is_needed_and_is_a_usage_error() {
    let repo = Repo::new().await;
    repo.run(&["init", "--project", "demo", "--test-command", "true"])
        .await
        .ok();
    let status = repo.run(&["status"]).await.failed();
    assert_eq!(status.code, Some(2), "{}", status.stderr);
    assert!(
        status.stderr.contains("no commit"),
        "the message says what is missing: {}",
        status.stderr
    );

    // With one commit, the same command runs.
    repo.commit_all("Start").await;
    repo.run(&["status"]).await.ok();
}
