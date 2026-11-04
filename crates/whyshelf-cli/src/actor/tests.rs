//! The workspace task keeps a stream in one piece when requests come together.

use std::path::Path;

use tokio::process::Command as Process;
use whyshelf_core::app::query::{GetResult, PageRequest, Target};
use whyshelf_core::app::register::AddOptions;
use whyshelf_core::domain::location::RelPath;
use whyshelf_core::domain::query::Query;
use whyshelf_core::domain::tag::TagChange;
use whyshelf_infra::config::UserDirs;

use super::*;
use crate::command::Outcome;

async fn git(dir: &Path, args: &[&str]) {
    let output = Process::new("git")
        .args(args)
        .current_dir(dir)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .output()
        .await
        .expect("git can be started");
    assert!(
        output.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

async fn write(root: &Path, rel: &str, text: &str) {
    let path = root.join(rel);
    tokio::fs::create_dir_all(path.parent().expect("a file has a parent"))
        .await
        .expect("directories can be made");
    tokio::fs::write(path, text)
        .await
        .expect("a file can be written");
}

const FINDING: &str = "---\nfinding_type: usage_observation\ntitle: \"Login feels slow\"\n\
                       evidence: measured\ncontext: \"Cold start, three testers\"\n\
                       reproducibility: reproducible\n---\n\nIt takes eight seconds.\n";

const SETTINGS: &str = "version = 1\nproject = \"demo\"\n\
                        project_id = \"0192f3a4-5b6c-7d8e-9fa0-b1c2d3e4f506\"\n\n\
                        [test]\ncommand = \"true\"\n\n\
                        [stores]\ndefault = \"repo\"\n\n[tags.official]\narea = [\"auth\", \"build\"]\n";

fn tag(target: &str, change: &str) -> Command {
    Command::Tag {
        target: target.parse().unwrap(),
        changes: vec![change.parse::<TagChange>().unwrap()],
    }
}

#[tokio::test]
async fn two_tag_commands_at_once_leave_one_head_in_the_stream() {
    let project = tempfile::tempdir().unwrap();
    let data = tempfile::tempdir().unwrap();
    let root = project.path();
    git(root, &["init", "-q", "-b", "main"]).await;
    git(root, &["config", "user.name", "Test Author"]).await;
    git(root, &["config", "user.email", "author@example.invalid"]).await;
    git(root, &["config", "commit.gpgsign", "false"]).await;
    write(root, ".whyshelf.toml", SETTINGS).await;
    write(root, ".gitignore", ".whyshelf/cache/\n").await;
    write(root, "docs/finding/slow-login.md", FINDING).await;
    git(root, &["add", "-A"]).await;
    git(root, &["commit", "-q", "-m", "Start"]).await;

    let dirs = UserDirs::default().with_data_dir(data.path());
    let (workspace, shutdown) = whyshelf_infra::open_with(root, &dirs).await.unwrap();
    let (actor, task) = WorkspaceActor::spawn(workspace);

    let options = AddOptions::new().with_alias("finding/slow-login".parse().unwrap());
    let added = actor
        .call(Command::Add {
            paths: vec!["docs".parse::<RelPath>().unwrap()],
            options,
        })
        .await
        .unwrap();
    added.result.unwrap();

    // Both are sent before either is answered. Each is a write that reads the
    // tips of the stream; run side by side they would both name the same head
    // and fork the stream.
    let (first, second) = {
        let (a, b) = (actor.clone(), actor.clone());
        let first =
            tokio::spawn(async move { a.call(tag("finding/slow-login", "+area:auth")).await });
        let second =
            tokio::spawn(async move { b.call(tag("finding/slow-login", "+area:build")).await });
        (
            first.await.unwrap().unwrap(),
            second.await.unwrap().unwrap(),
        )
    };
    first.result.unwrap();
    second.result.unwrap();

    let got = actor
        .call(Command::Get {
            target: "finding/slow-login".parse::<Target>().unwrap(),
        })
        .await
        .unwrap()
        .result
        .unwrap();
    let Outcome::Got(GetResult::Entity(view)) = got.outcome else {
        panic!("not an entity");
    };
    assert_eq!(view.tips().len(), 1, "the stream has one head");
    assert_eq!(view.tags().len(), 2, "both changes are in it");

    // Nothing is left to settle: the tags are not in conflict.
    let found = actor
        .call(Command::Find {
            query: Query::parse("tag:area:auth tag:area:build").unwrap(),
            page: PageRequest::default(),
        })
        .await
        .unwrap()
        .result
        .unwrap();
    let Outcome::Found(page) = found.outcome else {
        panic!("not a page");
    };
    assert_eq!(page.total(), 1);

    drop(actor);
    drop(task.await.unwrap());
    shutdown.close().await.unwrap();
}
