//! `.whyshelf.toml`: the project's settings file.
//!
//! The file is read here and becomes the value [`Config`] of the application
//! plus the few settings only the adapters need ([`Settings`]): where the private
//! store is and which ref is the baseline for "integrated". It is versioned
//! (`version = 1`) and refuses keys it does not know, so a typo is an error and
//! not a setting that silently does nothing.
//!
//! The file is committed with the project, so anyone who can change the
//! repository can change it. It therefore does not say where the private store
//! is: a path written there could point anywhere in the home directory of whoever
//! runs whyshelf. The private store is `<data dir>/<project>`, and the data dir
//! comes from the user's environment only ([`UserDirs`]).
//!
//! ```toml
//! version = 1
//! project = "demo"
//!
//! [paths]
//! finding_dir = "docs/finding"
//! spec_dir = "docs/spec"
//! design_dir = "docs/design"
//! scan_dirs = ["src", "tests"]
//! watch_dirs = ["docs"]            # where files nobody registered are looked for
//!
//! [test]
//! command = "cargo test"
//! pin_pattern = '@pin:\s*(PIN-[A-Z0-9_-]+|UT-[A-Z0-9_-]+)'   # group 1 is the pin
//!
//! [stores]
//! default = "private"              # where a new document goes: "private" or "repo"
//!                                  # (where the private store is, is not set here)
//!
//! [git]
//! baseline = "main"                # a revision is integrated when it is an ancestor of this ref
//!
//! [tags.official]
//! area = ["auth", "build"]
//!
//! [export]
//! status_md = false
//! changelog_md = false
//!
//! [ingest]
//! rebuild_threshold = 200
//!
//! [user]
//! name = "alice"                   # the `by` of an approval when none is given
//! ```
//!
//! Every table except `project` and `[test]` may be left out; the values shown
//! above are the defaults, except `project`, which names the project, and
//! `test.command`, which has no default because it depends on the language.
//!
//! # Environment and trust boundary
//!
//! Two sources give the settings, and they are trusted differently.
//!
//! ```text
//!   .whyshelf.toml  (committed; whoever can change the repository can change it)
//!        decides:  project name, document and test directories, the test command,
//!                  the pin pattern, the baseline ref, the default store of new
//!                  documents, the official tag vocabulary, which Markdown
//!                  files are generated, the ingest threshold, the default
//!                  approver's name
//!        may NOT:  say where the private store is  (`stores.private` is an error)
//!
//!   the user's environment  (this process; not committed)
//!        decides:  the data dir, and so where every private store is
//! ```
//!
//! | variable | meaning |
//! |---|---|
//! | `WHYSHELF_DATA_DIR` | the directory the private stores go in; must be an absolute path, and a relative one is an error rather than a store in an unexpected place |
//! | `XDG_DATA_HOME` | when the first is not set: the data dir is `$XDG_DATA_HOME/whyshelf` (an unset, empty or relative value is skipped) |
//! | `HOME` | when neither of those is usable: the data dir is `$HOME/.local/share/whyshelf` |
//!
//! The private store of a project is `<data dir>/<project>`. Nothing is expanded in
//! these values (no `~`, no `$VAR`). If none of the three gives an absolute path the
//! workspace cannot be opened, even when no document is ever private, because the
//! location is resolved with the settings. Whyshelf reads no other variable. The
//! processes it starts (`git`, the test command) get its environment, and `git` is
//! also told not to take optional locks and not to prompt.
//!
//! **Why the private store is not a setting.** The settings file travels with the
//! repository, so a path in it would let anyone who can change the repository point
//! the next run of whoever uses it at any directory they can write to, or read from.
//! The user's environment is the only thing that places the private store, and the
//! key is refused rather than ignored so that nobody believes it works.
//!
//! **What the file still decides, with that trust.** `test.command` is run through
//! the shell, in the tree being approved, when an item is checked (see
//! [`crate::test_run`]); it has the privileges of the person running the check, as
//! a `Makefile` or a CI script has. `git.baseline` is handed to Git as a ref and
//! `test.pin_pattern` is compiled as a regular expression; neither runs anything.
//! `user.name` is the declared approver: it is a label, and nothing authenticates it.
//!
//! The same facts, run:
//!
//! ```
//! use std::path::Path;
//! use whyshelf_infra::config::{ConfigError, UserDirs, parse};
//!
//! let text = "version = 1\nproject = \"demo\"\n[test]\ncommand = \"make test\"\n";
//!
//! // The private store: the data dir of the environment, then the project.
//! let dirs = UserDirs::default().with_data_dir("/data");
//! assert_eq!(parse(text, &dirs).unwrap().private_dir(), Path::new("/data/demo"));
//!
//! // Without WHYSHELF_DATA_DIR the XDG base directory is next, then HOME.
//! let dirs = UserDirs::default().with_data_home("/xdg").with_home("/srv/alice");
//! assert_eq!(dirs.data_dir().unwrap(), Path::new("/xdg/whyshelf"));
//! let dirs = UserDirs::default().with_home("/srv/alice");
//! assert_eq!(dirs.data_dir().unwrap(), Path::new("/srv/alice/.local/share/whyshelf"));
//!
//! // A committed file cannot place the private store.
//! let placed = format!("{text}[stores]\nprivate = \"/anywhere\"\n");
//! let dirs = UserDirs::default().with_data_dir("/data");
//! assert!(matches!(parse(&placed, &dirs), Err(ConfigError::PrivateInRepo)));
//!
//! // A relative WHYSHELF_DATA_DIR is an error.
//! let dirs = UserDirs::default().with_data_dir("relative");
//! assert!(matches!(dirs.data_dir(), Err(ConfigError::RelativeDataDir { .. })));
//! ```

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use regex::Regex;
use serde::Deserialize;
use whyshelf_core::app::Config;
use whyshelf_core::app::config::{
    ExportSettings, IngestSettings, Paths, PinPattern, TestCommand, TestSettings,
};
use whyshelf_core::domain::actor::Actor;
use whyshelf_core::domain::error::ValueError;
use whyshelf_core::domain::event::StoreKind;
use whyshelf_core::domain::location::RelPath;
use whyshelf_core::domain::tag::TagVocabulary;

/// The name of the settings file at the project root.
pub const FILE_NAME: &str = ".whyshelf.toml";

/// The only version of the file this build reads.
pub const SUPPORTED_VERSION: u32 = 1;

/// The pin tag the project scans for unless it says otherwise.
pub const DEFAULT_PIN_PATTERN: &str = r"@pin:\s*(PIN-[A-Z0-9_-]+|UT-[A-Z0-9_-]+)";

/// Why the settings could not be loaded.
#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    /// The file could not be read.
    #[error("cannot read the settings file {path}")]
    Read {
        /// The file.
        path: PathBuf,
        /// The operating system's error.
        #[source]
        source: std::io::Error,
    },
    /// The file is not valid TOML, or has a key or a value of the wrong kind.
    #[error("the settings file is not valid")]
    Parse(#[source] toml::de::Error),
    /// The file is of a version this build does not read.
    #[error("the settings file is version {found}; this build reads version {SUPPORTED_VERSION}")]
    Version {
        /// The version found.
        found: u32,
    },
    /// A value is refused by the rule of its type.
    #[error("the setting `{field}` is invalid")]
    Value {
        /// The key.
        field: String,
        /// The rule that was broken.
        #[source]
        source: ValueError,
    },
    /// `stores.default` is neither `private` nor `repo`.
    #[error("`stores.default` is `{found}`; it is `private` or `repo`")]
    DefaultStore {
        /// The text found.
        found: String,
    },
    /// The pin pattern is not a regular expression.
    #[error("`test.pin_pattern` does not compile")]
    PinPattern(#[source] regex::Error),
    /// The pin pattern has no group that captures the pin.
    #[error("`test.pin_pattern` needs a group that captures the pin name")]
    PinPatternGroup,
    /// The settings file says where the private store is, which only the user's
    /// environment may say.
    #[error(
        "`stores.private` is not read from the settings file; set {DATA_DIR_VAR} to choose where private stores go"
    )]
    PrivateInRepo,
    /// The data dir set by the user is not an absolute path.
    #[error("{DATA_DIR_VAR} is `{found}`; it must be an absolute path")]
    RelativeDataDir {
        /// The value found.
        found: PathBuf,
    },
    /// No variable of the environment gives a data dir.
    #[error("no data dir: none of {DATA_DIR_VAR}, XDG_DATA_HOME and HOME is an absolute path")]
    NoDataDir,
}

/// The variable a user sets to choose the directory the private stores go in.
pub const DATA_DIR_VAR: &str = "WHYSHELF_DATA_DIR";

/// Where whyshelf keeps the user's own data, as the user's environment says.
///
/// The data dir is, in order: `$WHYSHELF_DATA_DIR`; `$XDG_DATA_HOME/whyshelf`;
/// `$HOME/.local/share/whyshelf` (the XDG Base Directory default). A variable
/// that is unset, empty or relative is skipped, as the XDG specification asks,
/// except `WHYSHELF_DATA_DIR`: a user who sets it to a relative path made a
/// mistake, and that is an error rather than a store in an unexpected place.
/// Nothing is expanded: no `~`, no variables inside the values.
#[derive(Debug, Clone, Default)]
pub struct UserDirs {
    data_dir: Option<PathBuf>,
    data_home: Option<PathBuf>,
    home: Option<PathBuf>,
}

impl UserDirs {
    /// Read the variables of this process's environment.
    pub fn from_env() -> Self {
        let var = |name: &str| std::env::var_os(name).map(PathBuf::from);
        Self {
            data_dir: var(DATA_DIR_VAR),
            data_home: var("XDG_DATA_HOME"),
            home: var("HOME"),
        }
    }

    /// Set the data dir as `WHYSHELF_DATA_DIR` would.
    pub fn with_data_dir(mut self, dir: impl Into<PathBuf>) -> Self {
        self.data_dir = Some(dir.into());
        self
    }

    /// Set the base directory for user data as `XDG_DATA_HOME` would.
    pub fn with_data_home(mut self, dir: impl Into<PathBuf>) -> Self {
        self.data_home = Some(dir.into());
        self
    }

    /// Set the home directory as `HOME` would.
    pub fn with_home(mut self, dir: impl Into<PathBuf>) -> Self {
        self.home = Some(dir.into());
        self
    }

    /// The directory the private stores go in.
    pub fn data_dir(&self) -> Result<PathBuf, ConfigError> {
        if let Some(dir) = self.data_dir.as_ref().filter(|d| !d.as_os_str().is_empty()) {
            return if dir.is_absolute() {
                Ok(dir.clone())
            } else {
                Err(ConfigError::RelativeDataDir { found: dir.clone() })
            };
        }
        let absolute = |dir: &Option<PathBuf>| dir.clone().filter(|d| d.is_absolute());
        if let Some(base) = absolute(&self.data_home) {
            return Ok(base.join("whyshelf"));
        }
        absolute(&self.home)
            .map(|home| home.join(".local").join("share").join("whyshelf"))
            .ok_or(ConfigError::NoDataDir)
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct FileDto {
    version: u32,
    project: String,
    #[serde(default)]
    paths: PathsDto,
    test: TestDto,
    #[serde(default)]
    stores: StoresDto,
    #[serde(default)]
    git: GitDto,
    #[serde(default)]
    tags: TagsDto,
    #[serde(default)]
    export: ExportDto,
    #[serde(default)]
    ingest: IngestDto,
    user: Option<UserDto>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields, default)]
struct PathsDto {
    finding_dir: String,
    spec_dir: String,
    design_dir: String,
    scan_dirs: Vec<String>,
    watch_dirs: Vec<String>,
}

impl Default for PathsDto {
    fn default() -> Self {
        Self {
            finding_dir: "docs/finding".to_owned(),
            spec_dir: "docs/spec".to_owned(),
            design_dir: "docs/design".to_owned(),
            scan_dirs: vec!["src".to_owned(), "tests".to_owned()],
            watch_dirs: vec!["docs".to_owned()],
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct TestDto {
    command: String,
    pin_pattern: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields, default)]
struct StoresDto {
    default: String,
    /// Read only to refuse it with a clear error ([`ConfigError::PrivateInRepo`]).
    private: Option<serde::de::IgnoredAny>,
}

impl Default for StoresDto {
    fn default() -> Self {
        Self {
            default: "private".to_owned(),
            private: None,
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields, default)]
struct GitDto {
    baseline: String,
}

impl Default for GitDto {
    fn default() -> Self {
        Self {
            baseline: "main".to_owned(),
        }
    }
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields, default)]
struct TagsDto {
    official: BTreeMap<String, BTreeSet<String>>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields, default)]
struct ExportDto {
    status_md: bool,
    changelog_md: bool,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields, default)]
struct IngestDto {
    rebuild_threshold: usize,
}

impl Default for IngestDto {
    fn default() -> Self {
        Self {
            rebuild_threshold: whyshelf_core::app::config::DEFAULT_REBUILD_THRESHOLD,
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct UserDto {
    name: String,
}

/// The settings of a project: the application's [`Config`] and what only the
/// adapters need.
#[derive(Debug, Clone)]
pub struct Settings {
    config: Config,
    private_dir: PathBuf,
    baseline: String,
}

impl Settings {
    /// The application's settings.
    pub fn config(&self) -> &Config {
        &self.config
    }

    /// Where the private store is: `<data dir>/<project>` (see [`UserDirs`]).
    pub fn private_dir(&self) -> &Path {
        &self.private_dir
    }

    /// The ref a revision must be an ancestor of to count as integrated.
    pub fn baseline(&self) -> &str {
        &self.baseline
    }

    /// Take the settings out as the application's [`Config`].
    pub fn into_config(self) -> Config {
        self.config
    }
}

fn value<T>(field: &str, result: Result<T, ValueError>) -> Result<T, ConfigError> {
    result.map_err(|source| ConfigError::Value {
        field: field.to_owned(),
        source,
    })
}

fn rel(field: &str, text: &str) -> Result<RelPath, ConfigError> {
    value(field, text.parse())
}

fn rel_list(field: &str, texts: &[String]) -> Result<Vec<RelPath>, ConfigError> {
    texts.iter().map(|text| rel(field, text)).collect()
}

/// Read the settings from the text of a settings file; the private store is
/// placed in the data dir that `dirs` gives.
pub fn parse(text: &str, dirs: &UserDirs) -> Result<Settings, ConfigError> {
    let dto: FileDto = toml::from_str(text).map_err(ConfigError::Parse)?;
    if dto.version != SUPPORTED_VERSION {
        return Err(ConfigError::Version { found: dto.version });
    }
    if dto.stores.private.is_some() {
        return Err(ConfigError::PrivateInRepo);
    }
    let project = value("project", dto.project.parse())?;

    let paths = Paths::new(
        rel("paths.finding_dir", &dto.paths.finding_dir)?,
        rel("paths.spec_dir", &dto.paths.spec_dir)?,
        rel("paths.design_dir", &dto.paths.design_dir)?,
        rel_list("paths.scan_dirs", &dto.paths.scan_dirs)?,
        rel_list("paths.watch_dirs", &dto.paths.watch_dirs)?,
    );

    let pattern = dto
        .test
        .pin_pattern
        .unwrap_or_else(|| DEFAULT_PIN_PATTERN.to_owned());
    let compiled = Regex::new(&pattern).map_err(ConfigError::PinPattern)?;
    if compiled.captures_len() < 2 {
        return Err(ConfigError::PinPatternGroup);
    }
    let test = TestSettings::new(
        value("test.command", TestCommand::new(dto.test.command))?,
        value("test.pin_pattern", PinPattern::new(pattern))?,
    );

    let default_store = match dto.stores.default.as_str() {
        "private" => StoreKind::Private,
        "repo" => StoreKind::Repo,
        other => {
            return Err(ConfigError::DefaultStore {
                found: other.to_owned(),
            });
        }
    };

    let mut config = Config::new(project, paths, test)
        .with_default_store(default_store)
        .with_vocabulary(TagVocabulary::new(dto.tags.official))
        .with_export(ExportSettings::new(
            dto.export.status_md,
            dto.export.changelog_md,
        ))
        .with_ingest(IngestSettings::new(dto.ingest.rebuild_threshold));
    if let Some(user) = dto.user {
        config = config.with_user(value("user.name", Actor::new(&user.name))?);
    }

    let private_dir = dirs.data_dir()?.join(config.project().as_str());

    Ok(Settings {
        config,
        private_dir,
        baseline: dto.git.baseline,
    })
}

/// Read `.whyshelf.toml` at the project root.
pub async fn load(root: &Path, dirs: &UserDirs) -> Result<Settings, ConfigError> {
    let path = root.join(FILE_NAME);
    let text = tokio::fs::read_to_string(&path)
        .await
        .map_err(|source| ConfigError::Read { path, source })?;
    parse(&text, dirs)
}

#[cfg(test)]
mod tests {
    use super::*;

    const FULL: &str = r#"
version = 1
project = "demo"

[paths]
finding_dir = "notes/finding"
spec_dir = "notes/spec"
design_dir = "notes/design"
scan_dirs = ["src"]
watch_dirs = ["notes"]

[test]
command = "make test"
pin_pattern = '@pin\s+(PIN-[A-Z]+)'

[stores]
default = "repo"

[git]
baseline = "trunk"

[tags.official]
area = ["auth", "build"]

[export]
status_md = true
changelog_md = false

[ingest]
rebuild_threshold = 50

[user]
name = "alice"
"#;

    fn parse_full(text: &str) -> Result<Settings, ConfigError> {
        parse(text, &UserDirs::default().with_data_dir("/data"))
    }

    const MINIMAL: &str = "version = 1\nproject = \"demo\"\n[test]\ncommand = \"x\"\n";

    #[test]
    fn a_full_file_becomes_a_config_and_adapter_settings() {
        let settings = parse_full(FULL).unwrap();
        let config = settings.config();
        assert_eq!(config.project().as_str(), "demo");
        assert_eq!(config.paths().finding_dir().as_str(), "notes/finding");
        assert_eq!(config.paths().scan_dirs().len(), 1);
        assert_eq!(config.test().command().as_str(), "make test");
        assert_eq!(config.default_store(), StoreKind::Repo);
        assert!(config.export().status_md());
        assert!(!config.export().changelog_md());
        assert_eq!(config.ingest().rebuild_threshold(), 50);
        assert_eq!(config.user().map(Actor::as_str), Some("alice"));
        assert_eq!(settings.baseline(), "trunk");
        assert_eq!(settings.private_dir(), Path::new("/data/demo"));
        let official: Result<_, _> = "area:auth".parse().map(|t| config.vocabulary().admit(&t));
        assert_eq!(official, Ok(Ok(())));
    }

    #[test]
    fn only_the_project_and_the_test_command_are_required() {
        let settings =
            parse_full("version = 1\nproject = \"demo\"\n[test]\ncommand = \"cargo test\"\n")
                .unwrap();
        let config = settings.config();
        assert_eq!(config.default_store(), StoreKind::Private);
        assert_eq!(config.paths().spec_dir().as_str(), "docs/spec");
        assert_eq!(config.test().pin_pattern().as_str(), DEFAULT_PIN_PATTERN);
        assert_eq!(config.ingest().rebuild_threshold(), 200);
        assert_eq!(settings.baseline(), "main");
        assert!(config.user().is_none());
    }

    #[test]
    fn the_settings_file_cannot_say_where_the_private_store_is() {
        let text = "version = 1\nproject = \"demo\"\n[stores]\nprivate = \"~/.ssh\"\n[test]\ncommand = \"x\"\n";
        assert!(matches!(parse_full(text), Err(ConfigError::PrivateInRepo)));
    }

    #[test]
    fn the_data_dir_falls_back_from_the_own_variable_to_xdg_to_home() {
        let all = UserDirs::default()
            .with_data_dir("/own")
            .with_data_home("/xdg")
            .with_home("/fake-home");
        assert_eq!(all.data_dir().unwrap(), Path::new("/own"));

        let xdg = UserDirs::default()
            .with_data_home("/xdg")
            .with_home("/fake-home");
        assert_eq!(xdg.data_dir().unwrap(), Path::new("/xdg/whyshelf"));

        let home = UserDirs::default().with_home("/fake-home");
        assert_eq!(
            home.data_dir().unwrap(),
            Path::new("/fake-home/.local/share/whyshelf")
        );
        let settings = parse(MINIMAL, &home).unwrap();
        assert_eq!(
            settings.private_dir(),
            Path::new("/fake-home/.local/share/whyshelf/demo")
        );
    }

    #[test]
    fn empty_and_relative_variables_are_skipped_but_a_relative_own_dir_is_refused() {
        let skipped = UserDirs::default()
            .with_data_dir("")
            .with_data_home("relative")
            .with_home("/fake-home");
        assert_eq!(
            skipped.data_dir().unwrap(),
            Path::new("/fake-home/.local/share/whyshelf")
        );
        assert!(matches!(
            UserDirs::default().with_data_dir("relative").data_dir(),
            Err(ConfigError::RelativeDataDir { .. })
        ));
        assert!(matches!(
            parse(MINIMAL, &UserDirs::default().with_home("relative")),
            Err(ConfigError::NoDataDir)
        ));
    }

    #[test]
    fn a_wrong_version_an_unknown_key_and_a_bad_value_are_refused() {
        assert!(matches!(
            parse_full("version = 2\nproject = \"d\"\n[test]\ncommand = \"x\"\n"),
            Err(ConfigError::Version { found: 2 })
        ));
        assert!(matches!(
            parse_full("version = 1\nproject = \"d\"\ncolour = 1\n[test]\ncommand = \"x\"\n"),
            Err(ConfigError::Parse(_))
        ));
        assert!(matches!(
            parse_full("version = 1\nproject = \"Not A Name\"\n[test]\ncommand = \"x\"\n"),
            Err(ConfigError::Value { .. })
        ));
        assert!(matches!(
            parse_full(
                "version = 1\nproject = \"d\"\n[paths]\nspec_dir = \"/abs\"\n[test]\ncommand = \"x\"\n"
            ),
            Err(ConfigError::Value { .. })
        ));
        assert!(matches!(
            parse_full(
                "version = 1\nproject = \"d\"\n[stores]\ndefault = \"both\"\n[test]\ncommand = \"x\"\n"
            ),
            Err(ConfigError::DefaultStore { .. })
        ));
    }

    #[test]
    fn a_pin_pattern_must_compile_and_capture() {
        assert!(matches!(
            parse_full(
                "version = 1\nproject = \"d\"\n[test]\ncommand = \"x\"\npin_pattern = \"(\"\n"
            ),
            Err(ConfigError::PinPattern(_))
        ));
        assert!(matches!(
            parse_full(
                "version = 1\nproject = \"d\"\n[test]\ncommand = \"x\"\npin_pattern = \"@pin\"\n"
            ),
            Err(ConfigError::PinPatternGroup)
        ));
    }

    #[tokio::test]
    async fn load_reads_the_file_at_the_root() {
        let dir = tempfile::tempdir().unwrap();
        tokio::fs::write(dir.path().join(FILE_NAME), MINIMAL)
            .await
            .unwrap();
        let dirs = UserDirs::default().with_data_dir(dir.path().join("data"));
        let settings = load(dir.path(), &dirs).await.unwrap();
        assert_eq!(settings.private_dir(), dir.path().join("data").join("demo"));
        assert!(matches!(
            load(&dir.path().join("none"), &dirs).await,
            Err(ConfigError::Read { .. })
        ));
    }
}
