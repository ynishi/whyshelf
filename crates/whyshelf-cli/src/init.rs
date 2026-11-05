//! `whyshelf init`: the files a project needs before anything else.
//!
//! It writes a minimal `.whyshelf.toml` (the version, the project's name, the
//! project's identity, the test command and the test files), creates `.whyshelf/`,
//! and makes sure the cache directory is in `.gitignore`. The cache holds values
//! derived from the worktree it sits in and is never merged, so it must never be
//! committed. It also makes sure `.gitattributes` says `.whyshelf/** -text`, so that
//! `core.autocrlf` cannot rewrite the stored objects, which are named by the hash of
//! their bytes. Both files are appended to once and their other lines are never
//! rewritten. `init` makes no commit: every other command stands on a revision, so
//! in a repository with no commit yet the first one has to be made (the files `init`
//! wrote are a fine one) before they run. A settings file that exists is never touched: `init` refuses before it
//! changes anything.
//!
//! The identity is a new UUID version 7 (`project_id`). It is written to the file and
//! committed with it, so every clone and worktree of the project has the same one,
//! and it places the project's private store (see `whyshelf_infra::config`). A
//! settings file copied from a template repository carries the template's id; the
//! copy's line is replaced by the one `whyshelf init --print-project-id` prints. That
//! option only prints, so the tool never edits a settings file that exists.

use std::path::{Path, PathBuf};

use tokio::io::AsyncWriteExt;
use whyshelf_core::domain::error::ValueError;
use whyshelf_core::domain::reference::{ProjectId, ProjectName};
use whyshelf_infra::REPO_STORE_DIR;
use whyshelf_infra::config::{DEFAULT_SCAN_DIRS, FILE_NAME, SUPPORTED_VERSION, new_project_id};

use crate::args::InitArgs;
use crate::error::CliError;

/// The line `init` keeps in `.gitignore`.
const CACHE_IGNORE: &str = ".whyshelf/cache/";

/// The line `init` keeps in `.gitattributes`: the store's files are bytes, never text.
const STORE_ATTRIBUTES: &str = ".whyshelf/** -text";

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

/// The line that gives a settings file its identity.
fn project_id_line(id: ProjectId) -> String {
    format!("project_id = \"{id}\"")
}

/// The text of the settings file. A JSON string is a valid TOML basic string, so
/// the command is quoted by the one function that already knows how.
///
/// `[test] files` is the whole of the scan directories, which is what a file without
/// it would mean, written out so that narrowing it is one edit.
fn settings_text(
    project: &ProjectName,
    project_id: ProjectId,
    test_command: &str,
) -> Result<String, CliError> {
    let files = serde_json::to_string(&DEFAULT_SCAN_DIRS)?;
    Ok(format!(
        "version = {SUPPORTED_VERSION}\nproject = {}\n{}\n\n[test]\ncommand = {}\n\
         # The files whose `@pin:` tags count: globs under paths.scan_dirs. These are the\n\
         # whole scan directories; to narrow them write globs (\"tests/**/*.rs\"). A language\n\
         # that keeps its tests in the file they test (Rust's `#[cfg(test)] mod tests`)\n\
         # cannot be narrowed by file: leave the whole.\n\
         files = {files}\n",
        serde_json::to_string(&project.to_string())?,
        project_id_line(project_id),
        serde_json::to_string(test_command)?,
    ))
}

/// Append `line` to the file `name` of the project root unless a line of it already
/// says it (`says`). The file is created if it is not there; the lines that are there
/// are never rewritten, and a last line without a newline gets one first. Returns
/// whether the file was changed.
async fn ensure_line(
    root: &Path,
    name: &str,
    line: &str,
    says: impl Fn(&str) -> bool,
) -> Result<bool, CliError> {
    let path = root.join(name);
    let existing = match tokio::fs::read_to_string(&path).await {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(e) => return Err(io("read", &path)(e)),
    };
    if existing.lines().any(says) {
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
    file.write_all(format!("{separator}{line}\n").as_bytes())
        .await
        .map_err(io("write", &path))?;
    file.flush().await.map_err(io("write", &path))?;
    Ok(true)
}

/// Keep the cache out of Git.
async fn ignore_cache(root: &Path) -> Result<bool, CliError> {
    ensure_line(root, ".gitignore", CACHE_IGNORE, |line| {
        CACHE_IGNORE_SPELLINGS.contains(&line.trim())
    })
    .await
}

/// Keep Git from rewriting the stored objects. A stored text is addressed by the
/// hash of its bytes, so `core.autocrlf` (or a `text` attribute) turning line endings
/// on checkout would leave an object that no longer matches its name.
async fn keep_store_bytes(root: &Path) -> Result<bool, CliError> {
    ensure_line(root, ".gitattributes", STORE_ATTRIBUTES, |line| {
        let mut words = line.split_whitespace();
        matches!(
            (words.next(), words.next(), words.next()),
            (Some(".whyshelf/**" | "/.whyshelf/**"), Some("-text"), None)
        )
    })
    .await
}

/// Run `init` in `root`; returns what it did, one line each. With
/// `--print-project-id` it does nothing and returns the line to put in a settings
/// file.
pub async fn init(root: &Path, args: &InitArgs) -> Result<Vec<String>, CliError> {
    if args.print_project_id {
        return Ok(vec![project_id_line(new_project_id())]);
    }
    let Some(test_command) = &args.test_command else {
        return Err(CliError::argument("test-command", ValueError::BlankText));
    };
    let settings: PathBuf = root.join(FILE_NAME);
    if tokio::fs::try_exists(&settings)
        .await
        .map_err(io("look at", &settings))?
    {
        return Err(CliError::SettingsExist { path: settings });
    }
    let project = project_name(root, args.project.as_deref()).await?;
    let project_id = new_project_id();
    let text = settings_text(&project, project_id, test_command)?;

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
        format!("created {FILE_NAME} for project `{project}` ({project_id})"),
        format!("created {REPO_STORE_DIR}/"),
    ];
    if ignore_cache(root).await? {
        done.push(format!("added {CACHE_IGNORE} to .gitignore"));
    }
    if keep_store_bytes(root).await? {
        done.push(format!("added {STORE_ATTRIBUTES} to .gitattributes"));
    }
    Ok(done)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(project: Option<&str>) -> InitArgs {
        InitArgs {
            project: project.map(str::to_owned),
            test_command: Some("cargo test".to_owned()),
            print_project_id: false,
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
        // The file is a settings file this build reads, with an identity of version 7
        // and the test files written out as the whole of the scan directories.
        let dirs = whyshelf_infra::config::UserDirs::default().with_data_dir("/data");
        let read = whyshelf_infra::config::parse(&settings, &dirs).unwrap();
        assert_eq!(read.project_id().as_uuid().get_version_num(), 7);
        assert_eq!(
            read.config().test().files().map(|f| f.globs().to_vec()),
            Some(vec!["src".to_owned(), "tests".to_owned()])
        );
        assert_eq!(read.config().test().command().as_str(), "cargo test");
        assert!(settings.contains("# The files whose `@pin:` tags count"));
        assert!(settings.contains("cannot be narrowed by file"));
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
    async fn init_keeps_git_from_rewriting_the_store_and_leaves_other_lines_alone() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        // Somebody's attributes, without a final newline.
        tokio::fs::write(root.join(".gitattributes"), "*.png binary\n*.sh eol=lf")
            .await
            .unwrap();

        let done = init(root, &args(Some("demo"))).await.unwrap();
        let attributes = tokio::fs::read_to_string(root.join(".gitattributes"))
            .await
            .unwrap();
        assert_eq!(
            attributes,
            "*.png binary\n*.sh eol=lf\n.whyshelf/** -text\n"
        );
        assert!(done.iter().any(|line| line.contains(".gitattributes")));

        // Not added a second time, however the line is spaced.
        tokio::fs::remove_file(root.join(FILE_NAME)).await.unwrap();
        tokio::fs::write(
            root.join(".gitattributes"),
            "*.png binary\n.whyshelf/**   -text\n",
        )
        .await
        .unwrap();
        let done = init(root, &args(Some("demo"))).await.unwrap();
        assert_eq!(
            tokio::fs::read_to_string(root.join(".gitattributes"))
                .await
                .unwrap(),
            "*.png binary\n.whyshelf/**   -text\n"
        );
        assert!(!done.iter().any(|line| line.contains(".gitattributes")));
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
        let text = settings_text(&project, new_project_id(), "sh -c \"echo ok\"\\n").unwrap();
        assert!(text.contains(r#"command = "sh -c \"echo ok\"\\n""#));
    }

    #[tokio::test]
    async fn print_project_id_prints_a_version_7_line_and_touches_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let existing = "version = 1\nproject = \"demo\"\n[test]\ncommand = \"x\"\n";
        tokio::fs::write(dir.path().join(FILE_NAME), existing)
            .await
            .unwrap();
        let ask = InitArgs {
            project: None,
            test_command: None,
            print_project_id: true,
        };
        let first = init(dir.path(), &ask).await.unwrap();
        let second = init(dir.path(), &ask).await.unwrap();
        assert_eq!(first.len(), 1);
        assert_ne!(first, second, "a new id each time");
        let id = first[0]
            .strip_prefix("project_id = \"")
            .and_then(|rest| rest.strip_suffix('"'))
            .expect("a project_id line");
        assert_eq!(
            id.parse::<ProjectId>().unwrap().as_uuid().get_version_num(),
            7
        );
        // Neither the settings file nor anything else was written.
        assert_eq!(
            tokio::fs::read_to_string(dir.path().join(FILE_NAME))
                .await
                .unwrap(),
            existing
        );
        assert!(
            !tokio::fs::try_exists(dir.path().join(".whyshelf"))
                .await
                .unwrap()
        );
        assert!(
            !tokio::fs::try_exists(dir.path().join(".gitignore"))
                .await
                .unwrap()
        );
    }
}
