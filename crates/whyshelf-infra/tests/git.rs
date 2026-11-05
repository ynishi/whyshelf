//! The Git adapters against a real repository: revisions, the clean tree, the
//! baseline, the pins in the tests of a revision, and the test run at a revision.

mod common;

use std::path::Path;
use std::time::Duration;

use common::{all_files, commit_all, git, init_repo, write};
use whyshelf_core::app::config::{PinPattern, TestCommand, TestTimeout};
use whyshelf_core::app::ports::{CodeScanPort, RevisionPort, TestRunPort};
use whyshelf_core::app::raw::RawRevision;
use whyshelf_core::domain::approval::{TestOutcome, TestsRun};
use whyshelf_core::domain::location::{RelPath, TestFileSet};
use whyshelf_core::domain::revision::{IntegrationFact, RevisionId, UnknownIntegration};
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
    assert_eq!(port.current().await.unwrap().unwrap().as_str(), head);
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
async fn before_the_first_commit_there_is_no_current_revision_and_that_is_no_failure() {
    let dir = tempfile::tempdir().unwrap();
    init_repo(dir.path()).await;
    let port = revision_port(dir.path());
    assert_eq!(port.current().await.unwrap(), None);

    // A directory that is no repository is still a failure.
    let outside = tempfile::tempdir().unwrap();
    assert!(revision_port(outside.path()).current().await.is_err());
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

    let integrated = IntegrationFact::Integrated;
    assert_eq!(port.integration(&rev(&base)).await.unwrap(), integrated);
    assert_eq!(
        port.integration(&rev(&topic)).await.unwrap(),
        IntegrationFact::NotIntegrated
    );

    git(dir.path(), &["checkout", "-q", "main"]).await;
    git(
        dir.path(),
        &["merge", "-q", "--no-ff", "-m", "Merge the topic", "topic"],
    )
    .await;
    assert_eq!(port.integration(&rev(&topic)).await.unwrap(), integrated);
}

fn unknown(reason: UnknownIntegration) -> IntegrationFact {
    IntegrationFact::Unknown { reason }
}

#[tokio::test]
async fn a_clone_without_the_baseline_or_the_commit_cannot_say_and_that_is_not_an_error() {
    let dir = repo().await;
    let head = git(dir.path(), &["rev-parse", "HEAD"]).await;

    // The baseline ref is not in this clone (a single-branch CI checkout).
    let no_such_baseline = GitRevision::new(dir.path().to_path_buf(), "no-such-ref".to_owned());
    assert_eq!(
        no_such_baseline.integration(&rev(&head)).await.unwrap(),
        unknown(UnknownIntegration::BaselineUnresolved)
    );
    // A baseline that is not a commit (a tag of a tree, a tree) is not one either.
    let tree = GitRevision::new(dir.path().to_path_buf(), "HEAD^{tree}".to_owned());
    assert_eq!(
        tree.integration(&rev(&head)).await.unwrap(),
        unknown(UnknownIntegration::BaselineUnresolved)
    );
    // The text of the baseline cannot be read as an option.
    let option = GitRevision::new(dir.path().to_path_buf(), "--all".to_owned());
    assert_eq!(
        option.integration(&rev(&head)).await.unwrap(),
        unknown(UnknownIntegration::BaselineUnresolved)
    );
    // The approved commit is not in this clone.
    assert_eq!(
        revision_port(dir.path())
            .integration(&rev(&"0".repeat(40)))
            .await
            .unwrap(),
        unknown(UnknownIntegration::RevisionAbsent)
    );
    // A revision that is not even a full hash is still a mistake of the caller.
    assert!(matches!(
        revision_port(dir.path()).integration(&rev("main")).await,
        Err(GitError::NotFullHash { .. })
    ));
}

#[tokio::test]
async fn a_shallow_clone_with_the_baseline_but_not_the_old_commit_is_unknown() {
    let source = repo().await;
    let old = git(source.path(), &["rev-parse", "HEAD"]).await;
    write(source.path(), "later.txt", "later\n").await;
    commit_all(source.path(), "Later").await;

    let parent = tempfile::tempdir().unwrap();
    let clone = parent.path().join("shallow");
    let url = format!("file://{}", source.path().display());
    git(
        parent.path(),
        &[
            "clone", "-q", "--depth", "1", "--branch", "main", &url, "shallow",
        ],
    )
    .await;
    let port = revision_port(&clone);
    assert_eq!(
        port.integration(&rev(&old)).await.unwrap(),
        unknown(UnknownIntegration::RevisionAbsent)
    );
    let head = git(&clone, &["rev-parse", "HEAD"]).await;
    assert_eq!(
        port.integration(&rev(&head)).await.unwrap(),
        IntegrationFact::Integrated
    );
}

fn pattern() -> PinPattern {
    PinPattern::new(DEFAULT_PIN_PATTERN).unwrap()
}

fn dirs(names: &[&str]) -> Vec<RelPath> {
    names.iter().map(|n| n.parse().unwrap()).collect()
}

/// Every file under these directories is a test file.
fn whole(names: &[&str]) -> TestFileSet {
    TestFileSet::whole(&dirs(names)).unwrap()
}

fn globs(names: &[&str]) -> TestFileSet {
    TestFileSet::new(names.iter().map(|n| (*n).to_owned()).collect()).unwrap()
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
    let tests = dirs(&["tests"]);

    let at_first = scan
        .pins_at(&rev(&first), &tests, &whole(&["tests"]), &pattern())
        .await
        .unwrap()
        .pins;
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
        .pins_at(&rev(&second), &tests, &whole(&["tests"]), &pattern())
        .await
        .unwrap()
        .pins;
    assert!(at_second.keys().any(|p| p.as_str() == "PIN-LATER"));
    assert!(!at_second.keys().any(|p| p.as_str() == "PIN-DRAFT"));

    // Only the directories asked for are read.
    let wide = scan
        .pins_at(
            &rev(&second),
            &dirs(&["tests", "src"]),
            &whole(&["tests", "src"]),
            &pattern(),
        )
        .await
        .unwrap()
        .pins;
    assert!(wide.keys().any(|p| p.as_str() == "PIN-NOT-IN-TESTS"));
    assert!(
        scan.pins_at(&rev(&second), &[], &whole(&["tests"]), &pattern())
            .await
            .unwrap()
            .pins
            .is_empty()
    );
    assert!(
        scan.pins_at(
            &rev(&second),
            &dirs(&["nowhere"]),
            &whole(&["nowhere"]),
            &pattern()
        )
        .await
        .unwrap()
        .pins
        .is_empty()
    );
}

/// What one `git show` per file finds: the way pins were read before one `git grep`
/// read them all. The result of the single call must be the same.
async fn pins_the_slow_way(
    dir: &Path,
    revision: &str,
    scan_dirs: &[&str],
) -> std::collections::BTreeMap<String, Vec<String>> {
    let re = regex::Regex::new(DEFAULT_PIN_PATTERN).unwrap();
    let mut listing_args = vec!["ls-tree", "-r", "--name-only", revision, "--"];
    listing_args.extend(scan_dirs);
    let listing = git(dir, &listing_args).await;
    let mut found: std::collections::BTreeMap<String, Vec<String>> = Default::default();
    for path in listing.lines() {
        let text = git(dir, &["show", &format!("{revision}:{path}")]).await;
        for (index, line) in text.lines().enumerate() {
            for captures in re.captures_iter(line) {
                let name = captures[1].to_owned();
                if name.parse::<whyshelf_core::domain::pin::PinId>().is_ok() {
                    found
                        .entry(name)
                        .or_default()
                        .push(format!("{path}:{}", index + 1));
                }
            }
        }
    }
    found
}

#[tokio::test]
async fn one_git_grep_finds_what_a_show_per_file_found() {
    let dir = repo().await;
    write(
        dir.path(),
        "tests/a.rs",
        "// @pin: PIN-A\n\n\n// @pin: UT-B @pin: PIN-A\nfn x() {}\n",
    )
    .await;
    write(
        dir.path(),
        "tests/deep/b.rs",
        "no tag here\n  // @pin: PIN-C\n",
    )
    .await;
    write(dir.path(), "src/lib.rs", "// @pin: UT-D\n").await;
    write(dir.path(), "docs/readme.md", "@pin: PIN-ELSEWHERE\n").await;
    // A binary file with a tag-like text is skipped, as the old reading skipped it.
    let mut binary = b"\0\0@pin: PIN-BIN\n".to_vec();
    binary.extend_from_slice(&[0u8; 8]);
    tokio::fs::write(dir.path().join("tests/blob.bin"), binary)
        .await
        .unwrap();
    let head = commit_all(dir.path(), "Tests").await;

    let scan = GitCodeScan::new(dir.path().to_path_buf());
    let got = scan
        .pins_at(
            &rev(&head),
            &dirs(&["tests", "src"]),
            &whole(&["tests", "src"]),
            &pattern(),
        )
        .await
        .unwrap();
    let got: std::collections::BTreeMap<String, Vec<String>> = got
        .pins
        .iter()
        .map(|(pin, at)| {
            (
                pin.to_string(),
                at.iter().map(ToString::to_string).collect(),
            )
        })
        .collect();
    let mut expected = pins_the_slow_way(dir.path(), &head, &["tests", "src"]).await;
    expected.remove("PIN-BIN");
    assert_eq!(got, expected);
    assert!(
        got.contains_key("PIN-C") && got.contains_key("UT-D") && !got.contains_key("PIN-ELSEWHERE")
    );
}

#[tokio::test]
async fn only_the_test_files_the_globs_name_are_read_and_a_tag_outside_them_is_not_counted() {
    let dir = repo().await;
    write(dir.path(), "tests/a_test.rs", "// @pin: PIN-IN-TEST\n").await;
    write(dir.path(), "tests/helper.rs", "// @pin: PIN-IN-HELPER\n").await;
    write(dir.path(), "src/deep/x_test.rs", "// @pin: UT-DEEP\n").await;
    write(dir.path(), "src/lib.rs", "// @pin: PIN-IN-SOURCE\n").await;
    write(
        dir.path(),
        "other/y_test.rs",
        "// @pin: PIN-OUTSIDE-SCAN-DIRS\n",
    )
    .await;
    let head = commit_all(dir.path(), "Files").await;
    let scan = GitCodeScan::new(dir.path().to_path_buf());
    let names = |files: &TestFileSet| {
        let scan = &scan;
        let head = rev(&head);
        let files = files.clone();
        async move {
            scan.pins_at(&head, &dirs(&["src", "tests"]), &files, &pattern())
                .await
                .unwrap()
                .pins
                .keys()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
        }
    };

    // Left to the default: the whole of the scan directories.
    assert_eq!(
        names(&whole(&["src", "tests"])).await,
        ["PIN-IN-HELPER", "PIN-IN-SOURCE", "PIN-IN-TEST", "UT-DEEP"]
    );
    // Narrowed by a glob: `**` crosses directories, `*` does not.
    assert_eq!(
        names(&globs(&["**/*_test.rs"])).await,
        ["PIN-IN-TEST", "UT-DEEP"],
        "a glob never reaches outside the scan directories"
    );
    assert_eq!(names(&globs(&["tests/*_test.rs"])).await, ["PIN-IN-TEST"]);
    assert_eq!(
        names(&globs(&["tests/*.rs", "src/lib.rs"])).await,
        ["PIN-IN-HELPER", "PIN-IN-SOURCE", "PIN-IN-TEST"]
    );
    // A glob that names nothing finds nothing, and is no error.
    assert!(names(&globs(&["nothing/**"])).await.is_empty());
}

#[tokio::test]
async fn a_pin_in_a_mod_tests_of_a_source_file_is_counted_by_the_default_settings() {
    let dir = repo().await;
    write(
        dir.path(),
        "src/session.rs",
        "pub fn reuse() {}\n\n#[cfg(test)]\nmod tests {\n    // @pin: PIN-SESSION-REUSE\n    #[test]\n    fn reuses() {}\n}\n",
    )
    .await;
    let head = commit_all(dir.path(), "Session").await;
    let scan = GitCodeScan::new(dir.path().to_path_buf());
    // The settings as `init` writes them and as a file without `files` means them: the
    // whole of the scan directories, where Rust puts its unit tests.
    let found = scan
        .pins_at(
            &rev(&head),
            &dirs(&["src", "tests"]),
            &whole(&["src", "tests"]),
            &pattern(),
        )
        .await
        .unwrap();
    let at = &found.pins[&"PIN-SESSION-REUSE".parse().unwrap()];
    assert_eq!(at[0].to_string(), "src/session.rs:5");
}

#[tokio::test]
async fn a_tag_that_is_no_pin_name_is_reported_and_not_dropped() {
    let dir = repo().await;
    write(
        dir.path(),
        "tests/a.rs",
        "// @pin: PIN-foo\n// @pin: PIN-OK\n// @pin: see below\n// @pin:UT-X\n// @pin: PIN-V1.99A\n// @pin: PIN-S) and @pin: PIN-T\n",
    )
    .await;
    let head = commit_all(dir.path(), "Tags").await;
    let scan = GitCodeScan::new(dir.path().to_path_buf());
    let found = scan
        .pins_at(
            &rev(&head),
            &dirs(&["tests"]),
            &whole(&["tests"]),
            &pattern(),
        )
        .await
        .unwrap();
    let pins: Vec<String> = found.pins.keys().map(ToString::to_string).collect();
    // `PIN-V1.99A` is not cut at the dot: there is no undeclared `PIN-V1`, a name
    // nobody wrote. A closing parenthesis ends a tag.
    assert_eq!(pins, ["PIN-OK", "PIN-S", "PIN-T", "UT-X"]);
    let malformed: Vec<(String, String)> = found
        .malformed
        .iter()
        .map(|m| (m.location().to_string(), m.text().as_str().to_owned()))
        .collect();
    assert_eq!(
        malformed,
        [
            ("tests/a.rs:1".to_owned(), "PIN-foo".to_owned()),
            ("tests/a.rs:3".to_owned(), "see".to_owned()),
            ("tests/a.rs:5".to_owned(), "PIN-V1.99A".to_owned())
        ]
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
            &dirs(&["tests"]),
            &whole(&["tests"]),
            &PinPattern::new("@pin").unwrap(),
        )
        .await;
    assert!(matches!(result, Err(GitError::PatternGroup)));
}

fn command(text: &str) -> TestCommand {
    TestCommand::new(text).unwrap()
}

fn limit(seconds: u64) -> TestTimeout {
    TestTimeout::new(Duration::from_secs(seconds)).unwrap()
}

/// The outcome of a run that finished within its time.
fn finished(run: TestsRun) -> TestOutcome {
    match run {
        TestsRun::Finished(outcome) => outcome,
        other => panic!("the run did not finish: {other:?}"),
    }
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
    let run = |revision: &str, text: &str| {
        let (runner, revision, text) = (runner.clone(), rev(revision), command(text));
        async move { finished(runner.run(&revision, &text, limit(60)).await.unwrap()) }
    };
    assert_eq!(run(&second, "test -f new.txt").await, TestOutcome::Passed);
    assert_eq!(run(&second, "test -f old.txt").await, TestOutcome::Failed);

    // At the older revision: a temporary checkout that has the old state.
    assert_eq!(
        run(&first, "test -f old.txt && test ! -f new.txt").await,
        TestOutcome::Passed
    );
    assert_eq!(run(&first, "exit 3").await, TestOutcome::Failed);

    // The checkouts are gone, in Git's records and on disk.
    assert_eq!(worktrees(dir.path()).await, 1);
    let leftovers = all_files(&dir.path().join(".whyshelf").join("cache")).await;
    assert!(
        leftovers.iter().all(|name| !name.starts_with("rev-")),
        "{leftovers:?}"
    );
}

#[tokio::test]
async fn a_project_in_a_subdirectory_is_tested_in_the_same_subdirectory_of_the_checkout() {
    let dir = repo().await;
    write(dir.path(), "tools/app/old.txt", "old\n").await;
    let first = commit_all(dir.path(), "Old state").await;
    git(dir.path(), &["rm", "-q", "tools/app/old.txt"]).await;
    write(dir.path(), "tools/app/new.txt", "new\n").await;
    commit_all(dir.path(), "New state").await;

    // The project root is not the top of the repository.
    let runner = GitTestRun::new(dir.path().join("tools").join("app"));
    let ran = runner
        .run(
            &rev(&first),
            &command("test -f old.txt && test ! -f new.txt && test ! -f README.md"),
            limit(60),
        )
        .await
        .unwrap();
    assert_eq!(finished(ran), TestOutcome::Passed);
    assert_eq!(worktrees(dir.path()).await, 1);
}

#[tokio::test]
async fn a_dirty_tree_at_head_is_tested_in_a_checkout_of_the_commit() {
    let (dir, _, second) = two_revisions().await;
    write(dir.path(), "dirty.txt", "not committed\n").await;
    let runner = GitTestRun::new(dir.path().to_path_buf());
    // What is tested is the revision, not the edits on top of it.
    let ran = runner
        .run(
            &rev(&second),
            &command("test ! -f dirty.txt && test -f new.txt"),
            limit(60),
        )
        .await
        .unwrap();
    assert_eq!(finished(ran), TestOutcome::Passed);
    assert_eq!(worktrees(dir.path()).await, 1);
}

#[tokio::test]
async fn runs_at_the_same_revision_at_the_same_time_do_not_collide() {
    let (dir, first, _) = two_revisions().await;
    let runner = GitTestRun::new(dir.path().to_path_buf());
    let old = rev(&first);
    let slow = command("sleep 0.3; test -f old.txt");
    let (a, b, c) = tokio::join!(
        runner.run(&old, &slow, limit(60)),
        runner.run(&old, &slow, limit(60)),
        runner.run(&old, &slow, limit(60))
    );
    assert_eq!(
        (
            finished(a.unwrap()),
            finished(b.unwrap()),
            finished(c.unwrap())
        ),
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
    let result = runner
        .run(&rev(&"0".repeat(40)), &command("true"), limit(60))
        .await;
    assert!(result.is_err());
    assert_eq!(worktrees(dir.path()).await, 1);
}

#[tokio::test]
async fn a_run_that_outlasts_its_time_is_stopped_with_what_it_started_and_has_no_outcome() {
    let (dir, _, second) = two_revisions().await;
    let runner = GitTestRun::new(dir.path().to_path_buf());
    let marker = dir.path().join("grandchild-survived");
    // The command starts a child that would write a file after the limit; stopping
    // the command must stop the child too.
    let script = format!("(sleep 3; echo alive > '{}') & sleep 30", marker.display());
    let started = std::time::Instant::now();
    let ran = runner
        .run(
            &rev(&second),
            &command(&script),
            TestTimeout::new(Duration::from_millis(500)).unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(
        ran,
        TestsRun::TimedOut {
            after: Duration::from_millis(500)
        }
    );
    assert!(
        started.elapsed() < Duration::from_secs(10),
        "it did not wait for the command"
    );
    tokio::time::sleep(Duration::from_secs(4)).await;
    assert!(
        !tokio::fs::try_exists(&marker).await.unwrap(),
        "the child the command started was left running"
    );
    assert_eq!(worktrees(dir.path()).await, 1);
}

#[tokio::test]
async fn a_command_that_prints_a_great_deal_finishes_and_its_status_is_its_outcome() {
    let (dir, _, second) = two_revisions().await;
    let runner = GitTestRun::new(dir.path().to_path_buf());
    // About 20 MB on each stream: far more than the pipes hold, so a run that did not
    // read them would never end, and one that kept them would hold them all.
    let noisy = "i=0; while [ $i -lt 20000 ]; do printf '%01000d\\n' 0; printf '%01000d\\n' 0 >&2; i=$((i+1)); done; exit 4";
    let ran = runner
        .run(&rev(&second), &command(noisy), limit(120))
        .await
        .unwrap();
    assert_eq!(ran, TestsRun::Finished(TestOutcome::Failed));
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
