//! Tests of [`Project`] as the port that puts the files of a run on disk: the paths
//! the three writers wrote at are the paths it is given, from the one root.

use std::collections::{BTreeMap, BTreeSet};

use whyshelf_core::app::shelf::{
    Creation, DeclarationText, DeclarationWrite, DocWrite, FileSync, FileWrite, Initialized,
    ShelfApp, ShelfWrite,
};
use whyshelf_core::domain::location::RelPath;
use whyshelf_core::domain::nonempty::NonEmptyVec;
use whyshelf_core::domain::shelf::build::{DropNames, FormEntries, RawValue, Shelf};
use whyshelf_core::domain::shelf::schema::{BookName, BookSchemaDecl, KeyPatternDecl, Schema};

use super::Project;
use super::init::Template;

fn project(dir: &tempfile::TempDir) -> Project {
    Project::new(dir.path(), Some("tester".to_owned()), None, None).unwrap()
}

fn rel(text: &str) -> RelPath {
    text.parse().unwrap()
}

fn files(paths: &[&str]) -> NonEmptyVec<RelPath> {
    let mut paths = paths.iter().map(|p| rel(p));
    NonEmptyVec::new(paths.next().unwrap(), paths.collect())
}

/// The entries of a document of a book that declares nothing, as they are.
fn entries_of(key: &str) -> FormEntries {
    let decls = vec![BookSchemaDecl {
        name: "spec".to_owned(),
        dir: "docs".to_owned(),
        scope: Default::default(),
        key: KeyPatternDecl::Prefixed {
            prefix: "S".to_owned(),
        },
        description: None,
        fields: Vec::new(),
        links: Vec::new(),
    }];
    let schema = Schema::validate(decls, &BTreeSet::new()).unwrap();
    let entries = BTreeMap::from([("key".to_owned(), RawValue::Text(key.to_owned()))]);
    Shelf::form_entries(
        &schema,
        &BookName::new("spec").unwrap(),
        &entries,
        &DropNames::none(),
    )
}

#[tokio::test]
async fn the_files_the_three_writers_wrote_are_put_on_disk_through_the_project() {
    let dir = tempfile::tempdir().unwrap();
    let project = project(&dir);
    assert_eq!(
        project
            .declaration_writer()
            .create(&DeclarationText::new("[books]\n"))
            .await
            .unwrap(),
        Creation::Created
    );
    assert_eq!(
        project
            .file_writer()
            .create(&rel("data/deep/table.csv"), b"a,b\n")
            .await
            .unwrap(),
        Creation::Created
    );
    assert_eq!(
        project
            .doc_writer()
            .create(&rel("docs/S-1.md"), &entries_of("S-1"), "Body.\n")
            .await
            .unwrap(),
        Creation::Created
    );
    // Each path is the one its writer wrote at, from the root of the project.
    project
        .file_syncer()
        .persist(&files(&[
            ".whyshelf/books.toml",
            "data/deep/table.csv",
            "docs/S-1.md",
        ]))
        .await
        .unwrap();
    assert_eq!(
        tokio::fs::read(dir.path().join("data/deep/table.csv"))
            .await
            .unwrap(),
        b"a,b\n"
    );
}

#[tokio::test]
async fn a_path_that_no_writer_wrote_cannot_be_put_on_disk() {
    let dir = tempfile::tempdir().unwrap();
    let project = project(&dir);
    let error = project
        .persist(&files(&["docs/nowhere.md"]))
        .await
        .unwrap_err();
    assert!(error.step.contains("nowhere.md"), "{}", error.step);
    assert_eq!(error.source.kind(), std::io::ErrorKind::NotFound);
}

/// Every file under `from`, copied under `to` (over what is there).
async fn copy(from: &std::path::Path, to: &std::path::Path) {
    tokio::fs::create_dir_all(to).await.unwrap();
    let mut entries = tokio::fs::read_dir(from).await.unwrap();
    while let Some(entry) = entries.next_entry().await.unwrap() {
        let (path, target) = (entry.path(), to.join(entry.file_name()));
        if entry.file_name() == ".git" {
            continue;
        }
        let kind = entry.file_type().await.unwrap();
        // arch-lint: allow(no-sync-io) reason="`kind` is a FileType fetched with `.await`: a pure accessor, no I/O"
        if kind.is_dir() {
            Box::pin(copy(&path, &target)).await;
        } else {
            tokio::fs::copy(&path, &target).await.unwrap();
        }
    }
}

/// Whether `git` can be run here; the Git case passes without it.
async fn git_runs() -> bool {
    tokio::process::Command::new("git")
        .arg("--version")
        .output()
        .await
        .is_ok_and(|out| out.status.success())
}

/// Git run on `dir`, with no `GIT_*` variable of the caller.
async fn git(dir: &std::path::Path, args: &[&str]) {
    let mut command = tokio::process::Command::new("git");
    for (name, _) in std::env::vars_os() {
        if name.to_string_lossy().starts_with("GIT_") {
            command.env_remove(&name);
        }
    }
    let out = command
        .arg("-C")
        .arg(dir)
        .args(["-c", "user.name=t", "-c", "user.email=t@example.com"])
        .args(["-c", "core.autocrlf=false"])
        .args(args)
        .output()
        .await
        .unwrap();
    assert!(out.status.success(), "git {args:?}: {out:?}");
}

/// Both entries assemble the same adapters on the tree they read: a project for a
/// change through two directories, and one through Git, writes under its root as
/// `Project::new` does, names the way it was asked, and reads each end where it is
/// (an approval made before an edit holds at the end before, and the change made it
/// not hold).
#[tokio::test]
async fn a_project_for_a_change_is_assembled_as_one_of_the_root_and_reads_both_ends() {
    use super::OpenError;
    use super::state::{ChangeAsk, GitAfter};

    use whyshelf_core::app::shelf::{JudgeRequest, Judgement};
    use whyshelf_core::domain::shelf::freshness::{Holding, HoldsAs};
    use whyshelf_core::domain::shelf::impact::Made;

    let dir = tempfile::tempdir().unwrap();
    let (before, after) = (dir.path().join("before"), dir.path().join("after"));
    tokio::fs::create_dir_all(&after).await.unwrap();
    let mut app = ShelfApp::new(Project::new(&after, Some("t".to_owned()), None, None).unwrap());
    app.init(Template::Sample.request().unwrap()).await.unwrap();
    let approve = JudgeRequest {
        judgement: Judgement::Approve {
            until: None,
            reason: None,
            again: false,
        },
        reference: "F-1".parse().unwrap(),
        by: Default::default(),
    };
    app.judge_all(vec![approve]).await.unwrap();
    copy(&after, &before).await;
    // The finding is edited after its approval, at the side after only.
    let finding = after.join("books/finding/F-1.md");
    let mut text = tokio::fs::read_to_string(&finding).await.unwrap();
    text.push_str("\nAn edit after the approval.\n");
    tokio::fs::write(&finding, text).await.unwrap();

    // Read the two ends the wrong way round, and the finding would hold after and not
    // before: the side before is the copy, the side after the edited root.
    let made_of_finding = |report: &whyshelf_core::app::shelf::ImpactReport| {
        let key = |id| report.names_after.key_of(id).map(ToString::to_string);
        let entry = report
            .entries
            .iter()
            .find(|e| key(e.id()).is_some_and(|k| k.ends_with("F-1")))
            .expect("the finding");
        (entry.before().clone(), entry.made().clone())
    };
    let ask = ChangeAsk::Files {
        before: before.clone(),
        after: None,
    };
    let project = Project::for_change(&after, ask, Some("t".to_owned()), None, None)
        .await
        .unwrap();
    assert_eq!(
        project.books_file(),
        Project::new(&after, None, None, None).unwrap().books_file()
    );
    let report = ShelfApp::new(project).impact().await.unwrap();
    assert_eq!(
        report.way.as_str(),
        format!("files: {} -> the project root", before.display())
    );
    let (was, made) = made_of_finding(&report);
    assert_eq!(was, Holding::Holds(HoldsAs::Fresh));
    assert!(matches!(made, Made::Yes(_)), "{made:?}");

    // The same change through Git: the approval committed, then the edit.
    if git_runs().await {
        git(&before, &["init", "-q"]).await;
        git(&before, &["add", "-A"]).await;
        git(&before, &["commit", "-q", "-m", "approved"]).await;
        copy(&after, &before).await;
        git(&before, &["commit", "-q", "-am", "edited"]).await;
        let ask = ChangeAsk::Git {
            since: "HEAD~1".to_owned(),
            until: GitAfter::Commit("HEAD".to_owned()),
        };
        let project = Project::for_change(&before, ask, Some("t".to_owned()), None, None)
            .await
            .unwrap();
        let report = ShelfApp::new(project).impact().await.unwrap();
        assert_eq!(report.way.as_str(), "git: HEAD~1 -> HEAD");
        let (was, made) = made_of_finding(&report);
        assert_eq!(was, Holding::Holds(HoldsAs::Fresh));
        assert!(matches!(made, Made::Yes(_)), "{made:?}");
    }

    // A directory that is not there, and Git outside a repository, are not opened.
    let ask = ChangeAsk::Files {
        before: dir.path().join("nowhere"),
        after: None,
    };
    let opened = Project::for_change(&after, ask, None, None, None).await;
    assert!(
        matches!(opened, Err(OpenError::NoDirectory { .. })),
        "{opened:?}"
    );
    let ask = ChangeAsk::Git {
        since: "HEAD".to_owned(),
        until: GitAfter::Index,
    };
    let opened = Project::for_change(dir.path(), ask, None, None, None).await;
    assert!(
        matches!(
            opened,
            Err(OpenError::NotARepository { .. } | OpenError::NotACommit { .. })
        ),
        "{opened:?}"
    );
}

/// A directory of a side that is not there, or is a file, is an error of the request;
/// one that cannot be looked at for another reason is the run's, with its cause.
#[tokio::test]
async fn a_directory_of_a_side_is_no_directory_only_when_nothing_or_a_file_is_there() {
    use super::OpenError;
    use super::state::ChangeAsk;

    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("a-file");
    tokio::fs::write(&file, "x").await.unwrap();
    let ask = |before: std::path::PathBuf| ChangeAsk::Files {
        before,
        after: None,
    };
    for before in [dir.path().join("nowhere"), file.clone(), file.join("below")] {
        let opened = Project::for_change(dir.path(), ask(before.clone()), None, None, None).await;
        assert!(
            matches!(&opened, Err(OpenError::NoDirectory { path }) if *path == before),
            "{opened:?}"
        );
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let locked = dir.path().join("locked");
        tokio::fs::create_dir(&locked).await.unwrap();
        tokio::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o000))
            .await
            .unwrap();
        let opened =
            Project::for_change(dir.path(), ask(locked.join("inner")), None, None, None).await;
        tokio::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o755))
            .await
            .unwrap();
        // One run as a user every directory is open to looks into it, and finds nothing.
        assert!(
            matches!(
                opened,
                Err(OpenError::Unreadable { .. } | OpenError::NoDirectory { .. })
            ),
            "{opened:?}"
        );
        if let Err(OpenError::Unreadable { source, .. }) = opened {
            assert_eq!(source.kind(), std::io::ErrorKind::PermissionDenied);
        }
    }
}

#[tokio::test]
async fn a_run_of_init_through_the_project_puts_its_files_on_disk_and_each_is_whole() {
    let dir = tempfile::tempdir().unwrap();
    let mut app = ShelfApp::new(project(&dir));
    let Initialized {
        declaration,
        files: others,
        docs,
        ..
    } = app.init(Template::Sample.request().unwrap()).await.unwrap();
    let mut written: Vec<RelPath> = vec![declaration];
    written.extend(others);
    written.extend(docs.iter().map(|d| d.file.clone()));
    // The files `init` wrote are all there and may be put on disk again, by the
    // paths the run knew them by.
    let all = NonEmptyVec::from_vec(written).unwrap();
    app.ports().persist(&all).await.unwrap();
    for path in all.iter() {
        assert!(
            tokio::fs::metadata(dir.path().join(path.as_str()))
                .await
                .unwrap()
                .len()
                > 0,
            "{path}"
        );
    }
}
