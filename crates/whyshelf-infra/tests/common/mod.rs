//! Helpers shared by the integration tests: a real Git repository in a temporary
//! directory, with a settings file and fictional documents.
//!
//! Everything here is fictional: the project is `demo`, the people are `Test Author`
//! and `alice`, the documents are about a slow login.
#![allow(dead_code)]

use std::path::{Path, PathBuf};

use tempfile::TempDir;
use tokio::process::Command;

/// Run `git` in `dir` with the user's and the system's configuration out of the way,
/// and return what it printed (trimmed). Panics if it fails.
pub async fn git(dir: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
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
    String::from_utf8(output.stdout)
        .expect("git printed text")
        .trim()
        .to_owned()
}

/// Make `dir` a Git repository on `main`, with a local author.
pub async fn init_repo(dir: &Path) {
    git(dir, &["init", "-q", "-b", "main"]).await;
    git(dir, &["config", "user.name", "Test Author"]).await;
    git(dir, &["config", "user.email", "author@example.invalid"]).await;
    git(dir, &["config", "commit.gpgsign", "false"]).await;
}

/// Write a file, with its directories.
pub async fn write(root: &Path, rel: &str, text: &str) {
    let path = root.join(rel);
    tokio::fs::create_dir_all(path.parent().expect("a file has a parent"))
        .await
        .expect("directories can be made");
    tokio::fs::write(path, text)
        .await
        .expect("a file can be written");
}

/// Stage everything and commit; returns the new revision.
pub async fn commit_all(root: &Path, message: &str) -> String {
    git(root, &["add", "-A"]).await;
    git(root, &["commit", "-q", "-m", message]).await;
    git(root, &["rev-parse", "HEAD"]).await
}

/// A project in a temporary directory with a Git repository, and a private store
/// in another one.
pub struct Project {
    dir: TempDir,
    private: TempDir,
}

impl Project {
    /// The project root.
    pub fn root(&self) -> &Path {
        self.dir.path()
    }

    /// The user's data dir the private store of this project goes in.
    pub fn dirs(&self) -> whyshelf_infra::config::UserDirs {
        whyshelf_infra::config::UserDirs::default().with_data_dir(self.private.path())
    }

    /// The private store: `<data dir>/<project>`.
    pub fn private(&self) -> PathBuf {
        self.private.path().join("demo")
    }
}

/// A Git repository with `.whyshelf.toml` (the new documents go to the repo store),
/// a `.gitignore` for the cache, and one commit. `extra` is appended to the
/// settings file.
pub async fn project(extra: &str) -> Project {
    let dir = tempfile::tempdir().expect("a temporary directory");
    let private = tempfile::tempdir().expect("a temporary directory");
    init_repo(dir.path()).await;
    let settings = format!(
        "version = 1\nproject = \"demo\"\n\n[test]\ncommand = \"true\"\n\n[stores]\ndefault = \"repo\"\n\n[tags.official]\narea = [\"auth\", \"build\"]\n\n{extra}\n"
    );
    write(dir.path(), ".whyshelf.toml", &settings).await;
    write(dir.path(), ".gitignore", ".whyshelf/cache/\n").await;
    commit_all(dir.path(), "Start the project").await;
    Project { dir, private }
}

/// A Finding of the usage-observation type.
pub fn finding_text(title: &str, body: &str) -> String {
    format!(
        "---\nfinding_type: usage_observation\ntitle: \"{title}\"\nevidence: measured\n\
         context: \"Cold start, three testers\"\nreproducibility: reproducible\n---\n\n{body}\n"
    )
}

/// A Spec standing on the given grounds (written as the file writes them).
pub fn spec_text(grounds: &[&str], contract: &str) -> String {
    let list: String = grounds.iter().map(|g| format!("  - \"{g}\"\n")).collect();
    format!(
        "---\ngrounds:\n{list}accept:\n  pins:\n    - id: PIN-LOGIN-CACHE\n      contract: \"The session is reused for five minutes\"\n---\n\n## Contract\n{contract}\n"
    )
}

/// The `.json` files of a directory of a store, by name.
pub async fn json_files(dir: &Path) -> Vec<String> {
    let mut names = Vec::new();
    let Ok(mut listing) = tokio::fs::read_dir(dir).await else {
        return names;
    };
    while let Some(entry) = listing.next_entry().await.expect("a directory entry") {
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.ends_with(".json") {
            names.push(name);
        }
    }
    names.sort();
    names
}

/// Every file name in a directory (temporary names included).
pub async fn all_files(dir: &Path) -> Vec<String> {
    let mut names = Vec::new();
    let Ok(mut listing) = tokio::fs::read_dir(dir).await else {
        return names;
    };
    while let Some(entry) = listing.next_entry().await.expect("a directory entry") {
        names.push(entry.file_name().to_string_lossy().into_owned());
    }
    names.sort();
    names
}
