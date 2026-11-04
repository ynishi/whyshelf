//! The Git adapters against a real repository: revisions, the clean tree, the
//! baseline, the pins in the tests of a revision, and the test run at a revision.

mod common;

use std::path::Path;

use common::{all_files, commit_all, git, init_repo, write};
use whyshelf_core::app::config::{PinPattern, TestCommand};
use whyshelf_core::app::ports::{CodeScanPort, RevisionPort, TestRunPort};
use whyshelf_core::app::raw::RawRevision;
use whyshelf_core::domain::approval::TestOutcome;
use whyshelf_core::domain::revision::RevisionId;
use whyshelf_infra::config::DEFAULT_PIN_PATTERN;
use whyshelf_infra::git::{GitCodeScan, GitError, GitRevision};
use whyshelf_infra::test_run::GitTestRun;

async fn repo() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    init_repo(dir.path()).await;
    write(dir.path(), "README.md", "demo\n").await;
    commit_all(dir.path(), "Start").await;
    dir
}

fn rev(text: &str) -> RevisionId {
    RevisionId::from_validated(text.to_owned()).unwrap()
}

fn revision_port(dir: &Path) -> GitRevision {
    GitRevision::new(dir.to_path_buf(), "main".to_owned())
}

#[tokio::test]
async fn the_current_revision_and_the_branch_are_the_repositorys() {
    let dir = repo().await;
    let head = git(dir.path(), &["rev-parse", "HEAD"]).await;
    let port = revision_port(dir.path());
    assert_eq!(port.current().await.unwrap().as_str(), head);
    assert_eq!(port.label().await.unwrap().as_deref(), Some("main"));
    assert_eq!(
        port.worktree().await.unwrap().as_str(),
        dir.path().display().to_string()
    );

    git(dir.path(), &["checkout", "-q", "--detach"]).await;
    assert_eq!(
        port.label().await.unwrap(),
        None,
        "a detached head has no name"
    );
}

#[tokio::test]
async fn a_text_is_a_revision_when_it_is_a_full_hash() {
    let dir = repo().await;
    let head = git(dir.path(), &["rev-parse", "HEAD"]).await;
    let port = revision_port(dir.path());
    let ok = port
        .validate(&RawRevision::new(&head).unwrap())
        .await
        .unwrap();
    assert_eq!(ok.unwrap().as_str(), head);
    for bad in ["main", "abc1234", "v1.0", "HEAD~1"] {
        assert!(
            port.validate(&RawRevision::new(bad).unwrap())
                .await
                .unwrap()
                .is_none()
        );
    }
}

#[tokio::test]
async fn the_tree_is_clean_unless_something_outside_the_store_changed() {
    let dir = repo().await;
    let port = revision_port(dir.path());
    assert!(port.is_clean().await.unwrap());

    // The tool's own files do not count: recording an approval must not make the
    // next one fail with "commit first".
    write(dir.path(), ".whyshelf/events/0000.json", "{}\n").await;
    write(dir.path(), ".whyshelf/map.json", "{}\n").await;
    assert!(port.is_clean().await.unwrap());

    write(dir.path(), "new.txt", "x\n").await;
    assert!(!port.is_clean().await.unwrap(), "an untracked file");
    git(dir.path(), &["add", "new.txt"]).await;
    git(dir.path(), &["commit", "-q", "-m", "Add"]).await;
    assert!(port.is_clean().await.unwrap());

    write(dir.path(), "README.md", "changed\n").await;
    assert!(!port.is_clean().await.unwrap(), "a tracked file");
}

#[tokio::test]
async fn a_revision_is_integrated_when_it_is_in_the_baseline() {
    let dir = repo().await;
    let base = git(dir.path(), &["rev-parse", "HEAD"]).await;
    git(dir.path(), &["checkout", "-q", "-b", "topic"]).await;
    write(dir.path(), "topic.txt", "t\n").await;
    let topic = commit_all(dir.path(), "On the topic").await;
    let port = revision_port(dir.path());

    assert!(port.is_integrated(&rev(&base)).await.unwrap());
    assert!(!port.is_integrated(&rev(&topic)).await.unwrap());

    git(dir.path(), &["checkout", "-q", "main"]).await;
    git(
        dir.path(),
        &["merge", "-q", "--no-ff", "-m", "Merge the topic", "topic"],
    )
    .await;
    assert!(port.is_integrated(&rev(&topic)).await.unwrap());
}

#[tokio::test]
async fn an_unknown_baseline_or_revision_is_an_error_not_a_no() {
    let dir = repo().await;
    let head = git(dir.path(), &["rev-parse", "HEAD"]).await;
    let no_such_baseline = GitRevision::new(dir.path().to_path_buf(), "no-such-ref".to_owned());
    assert!(matches!(
        no_such_baseline.is_integrated(&rev(&head)).await,
        Err(GitError::Failed { .. })
    ));
    assert!(matches!(
        revision_port(dir.path()).is_integrated(&rev("main")).await,
        Err(GitError::NotFullHash { .. })
    ));
}

fn pattern() -> PinPattern {
    PinPattern::new(DEFAULT_PIN_PATTERN).unwrap()
}

#[tokio::test]
async fn pins_are_read_from_the_revision_not_from_the_working_tree() {
    let dir = repo().await;
    write(
        dir.path(),
        "tests/login.rs",
        "// @pin: PIN-LOGIN-CACHE\nfn a() {}\n\n// @pin: UT-LOGIN-RETRY and @pin: PIN-LOGIN-CACHE\nfn b() {}\n",
    )
    .await;
    write(
        dir.path(),
        "src/notes.txt",
        "mentions @pin: PIN-NOT-IN-TESTS\n",
    )
    .await;
    let first = commit_all(dir.path(), "Add a test").await;
    write(dir.path(), "tests/other.rs", "// @pin: PIN-LATER\n").await;
    let second = commit_all(dir.path(), "Add another").await;
    // Not committed: must not be seen.
    write(dir.path(), "tests/uncommitted.rs", "// @pin: PIN-DRAFT\n").await;

    let scan = GitCodeScan::new(dir.path().to_path_buf());
    let tests: Vec<whyshelf_core::domain::location::RelPath> = vec!["tests".parse().unwrap()];

    let at_first = scan
        .pins_at(&rev(&first), &tests, &pattern())
        .await
        .unwrap();
    let names: Vec<&str> = at_first.keys().map(|p| p.as_str()).collect();
    assert_eq!(names, ["PIN-LOGIN-CACHE", "UT-LOGIN-RETRY"]);
    let cache: Vec<String> = at_first
        .iter()
        .find(|(pin, _)| pin.as_str() == "PIN-LOGIN-CACHE")
        .unwrap()
        .1
        .iter()
        .map(ToString::to_string)
        .collect();
    assert_eq!(cache, ["tests/login.rs:1", "tests/login.rs:4"]);

    let at_second = scan
        .pins_at(&rev(&second), &tests, &pattern())
        .await
        .unwrap();
    assert!(at_second.keys().any(|p| p.as_str() == "PIN-LATER"));
    assert!(!at_second.keys().any(|p| p.as_str() == "PIN-DRAFT"));

    // Only the directories asked for are read.
    let both: Vec<_> = vec!["tests".parse().unwrap(), "src".parse().unwrap()];
    let wide = scan
        .pins_at(&rev(&second), &both, &pattern())
        .await
        .unwrap();
    assert!(wide.keys().any(|p| p.as_str() == "PIN-NOT-IN-TESTS"));
    assert!(
        scan.pins_at(&rev(&second), &[], &pattern())
            .await
            .unwrap()
            .is_empty()
    );
    let missing: Vec<_> = vec!["nowhere".parse().unwrap()];
    assert!(
        scan.pins_at(&rev(&second), &missing, &pattern())
            .await
            .unwrap()
            .is_empty()
    );
}

#[tokio::test]
async fn a_pin_pattern_without_a_group_is_refused() {
    let dir = repo().await;
    let head = git(dir.path(), &["rev-parse", "HEAD"]).await;
    let scan = GitCodeScan::new(dir.path().to_path_buf());
    let result = scan
        .pins_at(
            &rev(&head),
            &["tests".parse().unwrap()],
            &PinPattern::new("@pin").unwrap(),
        )
        .await;
    assert!(matches!(result, Err(GitError::PatternGroup)));
}

fn command(text: &str) -> TestCommand {
    TestCommand::new(text).unwrap()
}

/// Two commits: the first has `old.txt`, the second has `new.txt` instead.
async fn two_revisions() -> (tempfile::TempDir, String, String) {
    let dir = repo().await;
    write(dir.path(), "old.txt", "old\n").await;
    let first = commit_all(dir.path(), "Old state").await;
    git(dir.path(), &["rm", "-q", "old.txt"]).await;
    write(dir.path(), "new.txt", "new\n").await;
    let second = commit_all(dir.path(), "New state").await;
    (dir, first, second)
}

async fn worktrees(dir: &Path) -> usize {
    git(dir, &["worktree", "list"]).await.lines().count()
}

#[tokio::test]
async fn tests_run_in_the_working_tree_at_head_and_in_a_checkout_at_an_older_revision() {
    let (dir, first, second) = two_revisions().await;
    let runner = GitTestRun::new(dir.path().to_path_buf());

    // At HEAD, clean: the working tree itself.
    assert_eq!(
        runner
            .run(&rev(&second), &command("test -f new.txt"))
            .await
            .unwrap(),
        TestOutcome::Passed
    );
    assert_eq!(
        runner
            .run(&rev(&second), &command("test -f old.txt"))
            .await
            .unwrap(),
        TestOutcome::Failed
    );

    // At the older revision: a temporary checkout that has the old state.
    assert_eq!(
        runner
            .run(
                &rev(&first),
                &command("test -f old.txt && test ! -f new.txt")
            )
            .await
            .unwrap(),
        TestOutcome::Passed
    );
    assert_eq!(
        runner.run(&rev(&first), &command("exit 3")).await.unwrap(),
        TestOutcome::Failed
    );

    // The checkouts are gone, in Git's records and on disk.
    assert_eq!(worktrees(dir.path()).await, 1);
    let leftovers = all_files(&dir.path().join(".whyshelf").join("cache")).await;
    assert!(
        leftovers.iter().all(|name| !name.starts_with("rev-")),
        "{leftovers:?}"
    );
}

#[tokio::test]
async fn a_dirty_tree_at_head_is_tested_in_a_checkout_of_the_commit() {
    let (dir, _, second) = two_revisions().await;
    write(dir.path(), "dirty.txt", "not committed\n").await;
    let runner = GitTestRun::new(dir.path().to_path_buf());
    // What is tested is the revision, not the edits on top of it.
    assert_eq!(
        runner
            .run(
                &rev(&second),
                &command("test ! -f dirty.txt && test -f new.txt")
            )
            .await
            .unwrap(),
        TestOutcome::Passed
    );
    assert_eq!(worktrees(dir.path()).await, 1);
}

#[tokio::test]
async fn runs_at_the_same_revision_at_the_same_time_do_not_collide() {
    let (dir, first, _) = two_revisions().await;
    let runner = GitTestRun::new(dir.path().to_path_buf());
    let old = rev(&first);
    let slow = command("sleep 0.3; test -f old.txt");
    let (a, b, c) = tokio::join!(
        runner.run(&old, &slow),
        runner.run(&old, &slow),
        runner.run(&old, &slow)
    );
    assert_eq!(
        (a.unwrap(), b.unwrap(), c.unwrap()),
        (
            TestOutcome::Passed,
            TestOutcome::Passed,
            TestOutcome::Passed
        )
    );
    assert_eq!(worktrees(dir.path()).await, 1);
}

#[tokio::test]
async fn a_revision_that_does_not_exist_is_an_error() {
    let (dir, _, _) = two_revisions().await;
    let runner = GitTestRun::new(dir.path().to_path_buf());
    let result = runner.run(&rev(&"0".repeat(40)), &command("true")).await;
    assert!(result.is_err());
    assert_eq!(worktrees(dir.path()).await, 1);
}

#[tokio::test]
async fn prune_clears_the_record_of_a_checkout_whose_directory_is_gone() {
    let (dir, first, _) = two_revisions().await;
    let stale = dir.path().join(".whyshelf").join("cache").join("rev-stale");
    tokio::fs::create_dir_all(stale.parent().unwrap())
        .await
        .unwrap();
    git(
        dir.path(),
        &[
            "worktree",
            "add",
            "-q",
            "--detach",
            stale.to_str().unwrap(),
            &first,
        ],
    )
    .await;
    tokio::fs::remove_dir_all(&stale).await.unwrap();
    assert_eq!(
        worktrees(dir.path()).await,
        2,
        "the record outlives the directory"
    );

    revision_port(dir.path()).prune_worktrees().await.unwrap();
    assert_eq!(worktrees(dir.path()).await, 1);
}
