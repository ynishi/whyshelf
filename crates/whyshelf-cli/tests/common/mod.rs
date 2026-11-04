//! Helpers shared by the end-to-end tests: a real Git repository in a temporary
//! directory, the built `whyshelf` binary run inside it, and fictional documents.
//!
//! The binary is started as a child process with its own environment: the data
//! dir of the private store and the Git configuration are set on the child, never
//! on the test process. Everything here is fictional: the project is `demo`, the
//! approver is `alice`, the documents are about a slow login.
#![allow(dead_code)]

use std::path::{Path, PathBuf};
use std::process::Stdio;

use serde_json::Value;
use tempfile::TempDir;
use tokio::process::Command;

/// What a finished command printed, and whether it succeeded.
#[derive(Debug)]
pub struct Out {
    /// Exit status zero.
    pub success: bool,
    /// The exit code, if the process had one.
    pub code: Option<i32>,
    /// Standard output.
    pub stdout: String,
    /// Standard error.
    pub stderr: String,
}

impl Out {
    /// Panic with everything it printed unless the command succeeded.
    pub fn ok(self) -> Out {
        assert!(
            self.success,
            "the command failed ({:?})\nstdout:\n{}\nstderr:\n{}",
            self.code, self.stdout, self.stderr
        );
        self
    }

    /// Panic unless the command failed; returns what it printed.
    pub fn failed(self) -> Out {
        assert!(
            !self.success,
            "the command succeeded\nstdout:\n{}",
            self.stdout
        );
        self
    }

    /// Standard output parsed as one JSON value.
    pub fn json(&self) -> Value {
        serde_json::from_str(&self.stdout)
            .unwrap_or_else(|e| panic!("not JSON ({e}):\n{}", self.stdout))
    }
}

/// A Git repository and the data dir of its private store, both temporary.
pub struct Repo {
    dir: TempDir,
    data: TempDir,
}

fn git_command(dir: &Path) -> Command {
    let mut command = Command::new("git");
    command
        .current_dir(dir)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1");
    command
}

impl Repo {
    /// An empty repository on `main` with a local author.
    pub async fn new() -> Repo {
        let repo = Repo {
            dir: tempfile::tempdir().expect("a temporary directory"),
            data: tempfile::tempdir().expect("a temporary directory"),
        };
        repo.git(&["init", "-q", "-b", "main"]).await;
        repo.git(&["config", "user.name", "Test Author"]).await;
        repo.git(&["config", "user.email", "author@example.invalid"])
            .await;
        repo.git(&["config", "commit.gpgsign", "false"]).await;
        repo
    }

    /// The project root.
    pub fn root(&self) -> &Path {
        self.dir.path()
    }

    /// The data dir the private store of the project goes in.
    pub fn data(&self) -> PathBuf {
        self.data.path().to_path_buf()
    }

    /// Run `git`; panics if it fails; returns what it printed (trimmed).
    pub async fn git(&self, args: &[&str]) -> String {
        let output = git_command(self.root())
            .args(args)
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

    /// Run `git`, returning whether it succeeded.
    pub async fn git_succeeds(&self, args: &[&str]) -> bool {
        git_command(self.root())
            .args(args)
            .output()
            .await
            .expect("git can be started")
            .status
            .success()
    }

    /// Stage everything and commit; returns the new revision.
    pub async fn commit_all(&self, message: &str) -> String {
        self.git(&["add", "-A"]).await;
        self.git(&["commit", "-q", "-m", message]).await;
        self.git(&["rev-parse", "HEAD"]).await
    }

    /// Write a file under the root, with its directories.
    pub async fn write(&self, rel: &str, text: &str) {
        let path = self.root().join(rel);
        tokio::fs::create_dir_all(path.parent().expect("a file has a parent"))
            .await
            .expect("directories can be made");
        tokio::fs::write(path, text)
            .await
            .expect("a file can be written");
    }

    /// Read a file under the root.
    pub async fn read(&self, rel: &str) -> String {
        tokio::fs::read_to_string(self.root().join(rel))
            .await
            .unwrap_or_else(|e| panic!("cannot read {rel}: {e}"))
    }

    /// Append to a file under the root.
    pub async fn append(&self, rel: &str, text: &str) {
        let mut current = self.read(rel).await;
        current.push_str(text);
        self.write(rel, &current).await;
    }

    /// The `whyshelf` command, set up the way every test runs it.
    pub fn command(&self) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_whyshelf"));
        command
            .current_dir(self.root())
            .env("WHYSHELF_DATA_DIR", self.data())
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .kill_on_drop(true);
        command
    }

    /// Run `whyshelf` with these arguments.
    pub async fn run(&self, args: &[&str]) -> Out {
        let output = self
            .command()
            .args(args)
            .stdin(Stdio::null())
            .output()
            .await
            .expect("whyshelf can be started");
        Out {
            success: output.status.success(),
            code: output.status.code(),
            stdout: String::from_utf8(output.stdout).expect("whyshelf printed text"),
            stderr: String::from_utf8(output.stderr).expect("whyshelf printed text"),
        }
    }

    /// Run `whyshelf` with these arguments and `--format json`; it must succeed.
    pub async fn json(&self, args: &[&str]) -> Value {
        let mut all = args.to_vec();
        all.extend(["--format", "json"]);
        self.run(&all).await.ok().json()
    }

    /// `whyshelf init`, then the settings every test wants: documents go to the
    /// repo store (so a merge carries them), `area` tags exist, and `alice`
    /// approves. Committed.
    pub async fn init_project(&self) {
        self.run(&["init", "--project", "demo", "--test-command", "true"])
            .await
            .ok();
        self.append(
            ".whyshelf.toml",
            "\n[stores]\ndefault = \"repo\"\n\n[tags.official]\narea = [\"auth\", \"build\"]\n\n[user]\nname = \"alice\"\n",
        )
        .await;
        self.commit_all("Start the project").await;
    }
}

/// A Finding of the usage-observation type, filled in.
pub fn finding_text(title: &str, body: &str) -> String {
    format!(
        "---\nfinding_type: usage_observation\ntitle: \"{title}\"\nevidence: measured\n\
         context: \"Cold start, three testers\"\nreproducibility: reproducible\n---\n\n{body}\n"
    )
}

/// The rows of a table section of a JSON answer.
pub fn rows<'a>(answer: &'a Value, section: &str) -> &'a [Value] {
    answer[section]
        .as_array()
        .unwrap_or_else(|| panic!("no table `{section}` in {answer}"))
}

/// The row of a table whose `column` is `value`.
pub fn row_where<'a>(rows: &'a [Value], column: &str, value: &str) -> &'a Value {
    rows.iter()
        .find(|r| r[column].as_str() == Some(value))
        .unwrap_or_else(|| panic!("no row with {column} = {value} in {rows:?}"))
}
