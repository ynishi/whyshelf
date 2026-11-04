//! `whyshelf init`: the files a project needs before anything else.
//!
//! It writes a minimal `.whyshelf.toml` (the version, the project's name, the
//! test command), creates `.whyshelf/`, and makes sure the cache directory is
//! in `.gitignore`. The cache holds values derived from the worktree it sits in
//! and is never merged, so it must never be committed. A settings file that
//! exists is never touched: `init` refuses before it changes anything.

use std::path::{Path, PathBuf};

use tokio::io::AsyncWriteExt;
use whyshelf_core::domain::reference::ProjectName;
use whyshelf_infra::REPO_STORE_DIR;
use whyshelf_infra::config::{FILE_NAME, SUPPORTED_VERSION};

use crate::args::InitArgs;
use crate::error::CliError;

/// The line `init` keeps in `.gitignore`.
const CACHE_IGNORE: &str = ".whyshelf/cache/";

/// The ways a person may already have written the cache's line.
const CACHE_IGNORE_SPELLINGS: [&str; 4] = [
    ".whyshelf/cache/",
    ".whyshelf/cache",
    "/.whyshelf/cache/",
    "/.whyshelf/cache",
];

fn io(action: &'static str, path: &Path) -> impl FnOnce(std::io::Error) -> CliError {
    let path = path.to_path_buf();
    move |source| CliError::Io {
        action,
        path,
        source,
    }
}

/// The project's name: the argument, else the name of the directory.
async fn project_name(root: &Path, given: Option<&str>) -> Result<ProjectName, CliError> {
    if let Some(name) = given {
        return Ok(name.parse()?);
    }
    let canonical = tokio::fs::canonicalize(root)
        .await
        .map_err(io("resolve", root))?;
    let dir = canonical
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    dir.parse().map_err(|_| CliError::NoProjectName { dir })
}

/// The text of the settings file. A JSON string is a valid TOML basic string, so
/// the command is quoted by the one function that already knows how.
fn settings_text(project: &ProjectName, test_command: &str) -> Result<String, CliError> {
    Ok(format!(
        "version = {SUPPORTED_VERSION}\nproject = {}\n\n[test]\ncommand = {}\n",
        serde_json::to_string(&project.to_string())?,
        serde_json::to_string(test_command)?,
    ))
}

/// Append the cache line to `.gitignore` unless it is there. Returns whether the
/// file was changed.
async fn ignore_cache(root: &Path) -> Result<bool, CliError> {
    let path = root.join(".gitignore");
    let existing = match tokio::fs::read_to_string(&path).await {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(e) => return Err(io("read", &path)(e)),
    };
    if existing
        .lines()
        .any(|line| CACHE_IGNORE_SPELLINGS.contains(&line.trim()))
    {
        return Ok(false);
    }
    let mut file = tokio::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .await
        .map_err(io("open", &path))?;
    let separator = if existing.is_empty() || existing.ends_with('\n') {
        ""
    } else {
        "\n"
    };
    file.write_all(format!("{separator}{CACHE_IGNORE}\n").as_bytes())
        .await
        .map_err(io("write", &path))?;
    file.flush().await.map_err(io("write", &path))?;
    Ok(true)
}

/// Run `init` in `root`; returns what it did, one line each.
pub async fn init(root: &Path, args: &InitArgs) -> Result<Vec<String>, CliError> {
    let settings: PathBuf = root.join(FILE_NAME);
    if tokio::fs::try_exists(&settings)
        .await
        .map_err(io("look at", &settings))?
    {
        return Err(CliError::SettingsExist { path: settings });
    }
    let project = project_name(root, args.project.as_deref()).await?;
    let text = settings_text(&project, &args.test_command)?;

    // `create_new` is the operating system's "only if it does not exist", so a
    // settings file that appears meanwhile is not overwritten either.
    let mut file = tokio::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&settings)
        .await
        .map_err(|e| {
            if e.kind() == std::io::ErrorKind::AlreadyExists {
                CliError::SettingsExist {
                    path: settings.clone(),
                }
            } else {
                io("create", &settings)(e)
            }
        })?;
    file.write_all(text.as_bytes())
        .await
        .map_err(io("write", &settings))?;
    file.flush().await.map_err(io("write", &settings))?;

    let store = root.join(REPO_STORE_DIR);
    tokio::fs::create_dir_all(&store)
        .await
        .map_err(io("create", &store))?;

    let mut done = vec![
        format!("created {FILE_NAME} for project `{project}`"),
        format!("created {REPO_STORE_DIR}/"),
    ];
    if ignore_cache(root).await? {
        done.push(format!("added {CACHE_IGNORE} to .gitignore"));
    }
    Ok(done)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(project: Option<&str>) -> InitArgs {
        InitArgs {
            project: project.map(str::to_owned),
            test_command: "cargo test".to_owned(),
        }
    }

    #[tokio::test]
    async fn init_writes_the_settings_and_keeps_the_cache_out_of_git_once() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        tokio::fs::write(root.join(".gitignore"), "target")
            .await
            .unwrap();

        init(root, &args(Some("demo"))).await.unwrap();
        let settings = tokio::fs::read_to_string(root.join(FILE_NAME))
            .await
            .unwrap();
        assert_eq!(
            settings,
            "version = 1\nproject = \"demo\"\n\n[test]\ncommand = \"cargo test\"\n"
        );
        let store = tokio::fs::metadata(root.join(".whyshelf")).await.unwrap();
        assert!(store.is_dir());
        let ignore = tokio::fs::read_to_string(root.join(".gitignore"))
            .await
            .unwrap();
        assert_eq!(ignore, "target\n.whyshelf/cache/\n");

        // The settings file is never overwritten, and nothing else changes.
        let again = init(root, &args(Some("other"))).await.unwrap_err();
        assert!(matches!(again, CliError::SettingsExist { .. }));
        assert_eq!(
            tokio::fs::read_to_string(root.join(FILE_NAME))
                .await
                .unwrap(),
            settings
        );

        // With the file gone, the cache line is not added a second time.
        tokio::fs::remove_file(root.join(FILE_NAME)).await.unwrap();
        init(root, &args(Some("demo"))).await.unwrap();
        let ignore = tokio::fs::read_to_string(root.join(".gitignore"))
            .await
            .unwrap();
        assert_eq!(ignore.matches(".whyshelf/cache/").count(), 1);
    }

    #[tokio::test]
    async fn the_directory_name_is_the_project_when_it_is_a_valid_name() {
        let parent = tempfile::tempdir().unwrap();
        let named = parent.path().join("my-project");
        tokio::fs::create_dir(&named).await.unwrap();
        init(&named, &args(None)).await.unwrap();
        let settings = tokio::fs::read_to_string(named.join(FILE_NAME))
            .await
            .unwrap();
        assert!(settings.contains("project = \"my-project\""));

        let bad = parent.path().join("My Project");
        tokio::fs::create_dir(&bad).await.unwrap();
        let error = init(&bad, &args(None)).await.unwrap_err();
        assert!(matches!(error, CliError::NoProjectName { .. }));
        assert!(!tokio::fs::try_exists(bad.join(FILE_NAME)).await.unwrap());
    }

    #[tokio::test]
    async fn a_command_with_quotes_stays_one_valid_string() {
        let project: ProjectName = "demo".parse().unwrap();
        let text = settings_text(&project, "sh -c \"echo ok\"\\n").unwrap();
        assert!(text.contains(r#"command = "sh -c \"echo ok\"\\n""#));
    }
}
