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
//! runs whyshelf. The private store is `<data dir>/<project>-<project_id>`, and the
//! data dir comes from the user's environment only ([`UserDirs`]).
//!
//! ```toml
//! version = 1
//! project = "demo"
//! project_id = "0192f3a4-5b6c-7d8e-9fa0-b1c2d3e4f506"   # made by `whyshelf init`; required
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
//! pin_pattern = '''@pin:\s*([^\s:"'`)\]}]+)'''   # group 1 is the whole tag; one that is no pin name is reported
//! files = ["src", "tests"]         # the test files: globs under the scan directories
//! timeout = "30m"                  # a test run is stopped after this: "90s", "30m", "2h"
//!
//! [gate]
//! cs_requires_pins = true          # CS refuses a Spec that declares no pin
//! cs_requires_hypotheses = true    # CS refuses a Spec that declares no hypothesis
//!
//! [pins]
//! prefixes = { PIN = "pin", UT = "unit-test" }   # what a pin begins with, and what its pins are
//! normalize = "strict"             # or "upper-dash": `pin_login_cache` reads as PIN-LOGIN-CACHE
//!
//! [pins.mapping]                   # tags the code already has, and the pin each stands for
//! # AUTH_LOGIN = "PIN-AUTH-LOGIN"
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
//! Every table except `project`, `project_id` and `[test]` may be left out; the
//! values shown above are the defaults, except `project`, which names the project,
//! `project_id`, which identifies it, and `test.command`, which has no default
//! because it depends on the language. The two other keys of `[test]` are there to
//! narrow or widen what the defaults do:
//!
//! - `test.files` are the globs (Git pathspec globs, relative to the project root,
//!   `**` crossing directories) of the files whose pin tags count. Left out, every
//!   file under `paths.scan_dirs` counts, and `whyshelf init` writes the scan
//!   directories there so that narrowing it is one edit. A pin is looked for in a file
//!   that is under a scan directory **and** matches a glob. A language that keeps its
//!   tests in the file they test (Rust's `#[cfg(test)] mod tests`) cannot be narrowed
//!   by file: leave the whole of the scan directories.
//! - `test.timeout` is how long the test command may run before it is stopped; a CA
//!   check whose tests did not finish records nothing (the violation is
//!   `TestsTimedOut`). Only the last 64 KiB of the command's output is kept in memory.
//!
//! `[gate]` is the [`GatePolicy`]: what the machine check of CS demands of a Spec.
//! `[pins]` is the [`PinVocabulary`] (see [`whyshelf_core::domain::policy`]): each key of
//! `pins.prefixes` is a prefix (capitals and digits, starting with a capital; not
//! `HYP`) and its value says whether its pins are invariants (`"pin"`) or unit-test
//! tags (`"unit-test"`); `pins.prefixes`, when written, replaces the default `PIN` and
//! `UT` rather than adding to them. `pins.mapping` maps a tag as the code writes it to
//! the pin it stands for, written as a Spec writes a pin (a listed prefix, as written).
//! The vocabulary is read by the scan of the tests and by the reading of a Spec's and a
//! Design's pins alike, so the two never disagree about what a pin is. A value that is
//! refused names its key (`pins.prefixes.req`, `pins.mapping.AUTH_LOGIN`).
//!
//! # Project identity
//!
//! `project` is a name and two projects can share one. `project_id` is a UUID that
//! `whyshelf init` makes (version 7) and the file keeps, so it travels with the
//! repository: every clone and worktree of it finds the same private store, and a
//! different project with the same name does not. Only the UUID's own text ever
//! reaches a path, so a repository still cannot point the tool at a place of its
//! choosing.
//!
//! The tool never rewrites a settings file. A file without `project_id` is refused
//! with the line to add ([`ConfigError::MissingProjectId`]); `whyshelf init
//! --print-project-id` prints such a line, and is what to use after copying the
//! settings of a template repository, since the copy carries the template's id.
//! One person adds the line and commits it: if two people add one each and merge, the
//! private store of the one whose id was dropped stays in a directory that is no
//! longer looked at, and what was in it is unresolved until the directory is moved.
//!
//! # Environment and trust boundary
//!
//! Two sources give the settings, and they are trusted differently.
//!
//! ```text
//!   .whyshelf.toml  (committed; whoever can change the repository can change it)
//!        decides:  project name and id, document and test directories, the test
//!                  command and its time limit, the pin pattern, the test files,
//!                  what CS demands ([gate]), the pin vocabulary ([pins]),
//!                  the baseline ref, the default store of new
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
//! The private store of a project is `<data dir>/<project>-<project_id>`. Nothing is expanded in
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
//! `[gate]` can relax what CS demands, and anyone who can change the repository can
//! relax it: nothing stops a commit that sets `cs_requires_pins = false`. That is
//! allowed, and it is why every CS approval records the policy it passed under: an
//! approval made under a relaxed policy is told from one made under the default by
//! reading the approval, never by what the settings say today, and putting the
//! setting back does not make a relaxed approval look strict. `[pins]` decides which
//! marks are pins. A change of it that changes the pins a Spec declares changes the
//! Spec's hash, so its approvals become stale rather than silently covering other
//! pins, and a Spec that uses a prefix no longer listed does not parse.
//!
//! The same facts, run:
//!
//! ```
//! use std::path::Path;
//! use whyshelf_infra::config::{ConfigError, UserDirs, parse};
//!
//! let id = "0192f3a4-5b6c-7d8e-9fa0-b1c2d3e4f506";
//! let text = format!("version = 1\nproject = \"demo\"\nproject_id = \"{id}\"\n[test]\ncommand = \"make test\"\n");
//! let text = text.as_str();
//!
//! // The private store: the data dir of the environment, then the project and its id.
//! let dirs = UserDirs::default().with_data_dir("/data");
//! assert_eq!(
//!     parse(text, &dirs).unwrap().private_dir(),
//!     Path::new("/data/demo-0192f3a4-5b6c-7d8e-9fa0-b1c2d3e4f506")
//! );
//!
//! // A file without the id is refused with the line to add; nothing is rewritten.
//! let without = "version = 1\nproject = \"demo\"\n[test]\ncommand = \"make test\"\n";
//! let Err(ConfigError::MissingProjectId { suggested }) = parse(without, &dirs) else {
//!     panic!("the id is required");
//! };
//! assert!(format!("project_id = \"{suggested}\"").starts_with("project_id = \""));
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
use std::time::Duration;

use regex::Regex;
use serde::Deserialize;
use whyshelf_core::app::Config;
use whyshelf_core::app::config::{
    ExportSettings, IngestSettings, Paths, PinPattern, PinSearch, TestCommand, TestSettings,
    TestTimeout,
};
use whyshelf_core::domain::actor::Actor;
use whyshelf_core::domain::error::ValueError;
use whyshelf_core::domain::event::StoreKind;
use whyshelf_core::domain::location::{RelPath, TestFileSet};
use whyshelf_core::domain::nonempty::NonEmptyVec;
use whyshelf_core::domain::pin::{PinId, PinKind};
use whyshelf_core::domain::policy::{
    ExternalTag, GatePolicy, PinNormalize, PinPrefix, PinVocabulary,
};
use whyshelf_core::domain::reference::{ProjectId, ProjectName};
use whyshelf_core::domain::tag::TagVocabulary;

/// The name of the settings file at the project root.
pub const FILE_NAME: &str = ".whyshelf.toml";

/// The only version of the file this build reads.
pub const SUPPORTED_VERSION: u32 = 1;

/// The pin tag the project scans for unless it says otherwise.
///
/// The first group is the **whole token** after `@pin:`: everything up to white space,
/// a `:` or a closing quote, bracket or parenthesis. It is then checked as a whole
/// against the shape of a pin name, so a tag that was meant as a pin and is mistyped
/// is reported with its full text instead of being cut short. A pattern that only
/// took the characters a pin may have would cut `PIN-V1.99A` at the dot and report an
/// undeclared `PIN-V1`, a name nobody wrote.
pub const DEFAULT_PIN_PATTERN: &str = r#"@pin:\s*([^\s:"'`)\]}]+)"#;

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
    /// A key that takes one of a few words has another (`pins.normalize`, the kind of
    /// a prefix in `pins.prefixes`).
    #[error("`{field}` is `{found}`; it is one of {allowed}")]
    Word {
        /// The key.
        field: String,
        /// The text found.
        found: String,
        /// The words it may be, listed.
        allowed: String,
    },
    /// The pin pattern is not a regular expression.
    #[error("`test.pin_pattern` does not compile")]
    PinPattern(#[source] regex::Error),
    /// The pin pattern has no group that captures the pin.
    #[error("`test.pin_pattern` needs a group that captures the pin name")]
    PinPatternGroup,
    /// `test.timeout` is not a count and a unit.
    #[error(
        "`test.timeout` is `{found}`; write a whole number and a unit, such as `90s`, `30m` or `2h`"
    )]
    Timeout {
        /// The text found.
        found: String,
    },
    /// The file has no `project_id`. The tool does not rewrite the file; the line to
    /// add is in the message, with a new id made just now.
    #[error(
        "the settings file has no `project_id`; add this line before the first `[table]` and commit it:\n\nproject_id = \"{suggested}\"\n\n(`whyshelf init --print-project-id` prints such a line. A project's private store is placed by this id, so the id must not change once private documents exist.)"
    )]
    MissingProjectId {
        /// A new id, to be added to the file.
        suggested: ProjectId,
    },
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
    project_id: Option<String>,
    #[serde(default)]
    paths: PathsDto,
    test: TestDto,
    #[serde(default)]
    gate: GateDto,
    #[serde(default)]
    pins: PinsDto,
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

/// The directories scanned for pins when the settings name none.
pub const DEFAULT_SCAN_DIRS: [&str; 2] = ["src", "tests"];

impl Default for PathsDto {
    fn default() -> Self {
        Self {
            finding_dir: "docs/finding".to_owned(),
            spec_dir: "docs/spec".to_owned(),
            design_dir: "docs/design".to_owned(),
            scan_dirs: DEFAULT_SCAN_DIRS.iter().map(|d| (*d).to_owned()).collect(),
            watch_dirs: vec!["docs".to_owned()],
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct TestDto {
    command: String,
    pin_pattern: Option<String>,
    files: Option<Vec<String>>,
    timeout: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields, default)]
struct GateDto {
    cs_requires_pins: bool,
    cs_requires_hypotheses: bool,
}

impl Default for GateDto {
    fn default() -> Self {
        let policy = GatePolicy::default();
        Self {
            cs_requires_pins: policy.cs_requires_pins(),
            cs_requires_hypotheses: policy.cs_requires_hypotheses(),
        }
    }
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields, default)]
struct PinsDto {
    /// Left out: the default prefixes. Written: these and only these.
    prefixes: Option<BTreeMap<String, String>>,
    normalize: Option<String>,
    mapping: BTreeMap<String, String>,
}

/// The words of `pins.prefixes` for what the pins of a prefix are.
const PIN_KINDS: [(&str, PinKind); 2] = [("pin", PinKind::Pin), ("unit-test", PinKind::UnitTest)];

/// The words of `pins.normalize`.
const PIN_NORMALIZE: [(&str, PinNormalize); 2] = [
    ("strict", PinNormalize::Strict),
    ("upper-dash", PinNormalize::UpperDash),
];

/// The value a key names with one of a few words.
fn word<T: Copy>(field: &str, found: &str, words: &[(&str, T)]) -> Result<T, ConfigError> {
    words
        .iter()
        .find(|(name, _)| *name == found)
        .map(|(_, value)| *value)
        .ok_or_else(|| ConfigError::Word {
            field: field.to_owned(),
            found: found.to_owned(),
            allowed: words
                .iter()
                .map(|(name, _)| format!("`{name}`"))
                .collect::<Vec<_>>()
                .join(", "),
        })
}

/// The pin vocabulary of `[pins]`.
fn pin_vocabulary(dto: PinsDto) -> Result<PinVocabulary, ConfigError> {
    let prefixes = match dto.prefixes {
        None => PinVocabulary::default().prefixes().clone(),
        Some(table) => {
            let prefixes = table
                .into_iter()
                .map(|(text, kind)| {
                    let field = format!("pins.prefixes.{text}");
                    let kind = word(&field, &kind, &PIN_KINDS)?;
                    value(&field, PinPrefix::new(text, kind))
                })
                .collect::<Result<Vec<_>, _>>()?;
            value("pins.prefixes", NonEmptyVec::from_vec(prefixes))?
        }
    };
    let normalize = match &dto.normalize {
        None => PinNormalize::Strict,
        Some(text) => word("pins.normalize", text, &PIN_NORMALIZE)?,
    };
    // A pin of the table is written as a Spec writes one: a listed prefix, as written.
    let written = value(
        "pins.prefixes",
        PinVocabulary::new(prefixes.clone(), PinNormalize::Strict, BTreeMap::new()),
    )?;
    let mapping = dto
        .mapping
        .into_iter()
        .map(|(tag, pin)| {
            let field = format!("pins.mapping.{tag}");
            Ok((
                value(&field, ExternalTag::new(tag))?,
                value(&field, PinId::parse_in(&written, &pin))?,
            ))
        })
        .collect::<Result<BTreeMap<_, _>, ConfigError>>()?;
    value(
        "pins.mapping",
        PinVocabulary::new(prefixes, normalize, mapping),
    )
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
    project_id: ProjectId,
    private_dir: PathBuf,
    baseline: String,
}

impl Settings {
    /// The application's settings.
    pub fn config(&self) -> &Config {
        &self.config
    }

    /// The project's identity.
    pub fn project_id(&self) -> ProjectId {
        self.project_id
    }

    /// Where the private store is: `<data dir>/<project>-<project_id>` (see
    /// [`UserDirs`]).
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

/// Where the private store of a project is: `<data dir>/<project>-<project_id>`.
///
/// The one place that says so; `whyshelf migrate` places the store the same way. The
/// name is for a person reading the directory, the id is what tells two projects of
/// one name apart, and only the id's own text (hexadecimal and hyphens) is joined.
pub fn private_dir(
    dirs: &UserDirs,
    project: &ProjectName,
    id: ProjectId,
) -> Result<PathBuf, ConfigError> {
    Ok(dirs.data_dir()?.join(format!("{project}-{id}")))
}

/// A new project id, as `whyshelf init` and the message of a missing one make it.
pub fn new_project_id() -> ProjectId {
    ProjectId::new(uuid::Uuid::now_v7())
}

/// `90s`, `30m`, `2h`: a whole number and one unit.
fn timeout(text: &str) -> Result<TestTimeout, ConfigError> {
    let bad = || ConfigError::Timeout {
        found: text.to_owned(),
    };
    let unit = text.chars().last().ok_or_else(bad)?;
    let scale = match unit {
        's' => 1,
        'm' => 60,
        'h' => 3600,
        _ => return Err(bad()),
    };
    let count: u64 = text[..text.len() - 1].parse().map_err(|_| bad())?;
    let seconds = count.checked_mul(scale).ok_or_else(bad)?;
    value(
        "test.timeout",
        TestTimeout::new(Duration::from_secs(seconds)),
    )
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
    let project_id = match &dto.project_id {
        Some(text) => value("project_id", text.parse::<ProjectId>())?,
        None => {
            return Err(ConfigError::MissingProjectId {
                suggested: new_project_id(),
            });
        }
    };

    let paths = Paths::new(
        rel("paths.finding_dir", &dto.paths.finding_dir)?,
        rel("paths.spec_dir", &dto.paths.spec_dir)?,
        rel("paths.design_dir", &dto.paths.design_dir)?,
        rel_list("paths.scan_dirs", &dto.paths.scan_dirs)?,
        rel_list("paths.watch_dirs", &dto.paths.watch_dirs)?,
    );

    let mut search = PinSearch::new(pin_vocabulary(dto.pins)?);
    if let Some(pattern) = dto.test.pin_pattern {
        let compiled = Regex::new(&pattern).map_err(ConfigError::PinPattern)?;
        if compiled.captures_len() < 2 {
            return Err(ConfigError::PinPatternGroup);
        }
        search = search.with_pattern(value("test.pin_pattern", PinPattern::new(pattern))?);
    }
    let mut test = TestSettings::new(value("test.command", TestCommand::new(dto.test.command))?)
        .with_pin_search(search);
    if let Some(globs) = dto.test.files {
        test = test.with_files(value("test.files", TestFileSet::new(globs))?);
    }
    if let Some(limit) = &dto.test.timeout {
        test = test.with_timeout(timeout(limit)?);
    }

    let default_store =
        dto.stores
            .default
            .parse::<StoreKind>()
            .map_err(|_| ConfigError::DefaultStore {
                found: dto.stores.default.clone(),
            })?;

    let mut config = Config::new(project, paths, test)
        .with_gate_policy(GatePolicy::new(
            dto.gate.cs_requires_pins,
            dto.gate.cs_requires_hypotheses,
        ))
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

    let private_dir = private_dir(dirs, config.project(), project_id)?;

    Ok(Settings {
        config,
        project_id,
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

    const ID: &str = "0192f3a4-5b6c-7d8e-9fa0-b1c2d3e4f506";

    const FULL: &str = r#"
version = 1
project = "demo"
project_id = "0192f3a4-5b6c-7d8e-9fa0-b1c2d3e4f506"

[paths]
finding_dir = "notes/finding"
spec_dir = "notes/spec"
design_dir = "notes/design"
scan_dirs = ["src"]
watch_dirs = ["notes"]

[test]
command = "make test"
pin_pattern = '@pin\s+(PIN-[A-Z]+)'
files = ["src/**/*_test.rs"]
timeout = "45m"

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

    /// Parse a text; one that names no `project_id` gets the test one, since most of
    /// these tests are about something else.
    fn parse_full(text: &str) -> Result<Settings, ConfigError> {
        let text = if text.contains("project_id") {
            text.to_owned()
        } else {
            text.replacen(
                "project = ",
                &format!("project_id = \"{ID}\"\nproject = "),
                1,
            )
        };
        parse(&text, &UserDirs::default().with_data_dir("/data"))
    }

    const MINIMAL: &str = "version = 1\nproject = \"demo\"\nproject_id = \"0192f3a4-5b6c-7d8e-9fa0-b1c2d3e4f506\"\n[test]\ncommand = \"x\"\n";

    #[test]
    fn a_full_file_becomes_a_config_and_adapter_settings() {
        let settings = parse_full(FULL).unwrap();
        let config = settings.config();
        assert_eq!(config.project().as_str(), "demo");
        assert_eq!(config.paths().finding_dir().as_str(), "notes/finding");
        assert_eq!(config.paths().scan_dirs().len(), 1);
        assert_eq!(config.test().command().as_str(), "make test");
        assert_eq!(
            config.test().pin_search().pattern().map(PinPattern::as_str),
            Some(r"@pin\s+(PIN-[A-Z]+)"),
            "test.pin_pattern is kept as the project's own pattern"
        );
        assert_eq!(config.default_store(), StoreKind::Repo);
        assert!(config.export().status_md());
        assert!(!config.export().changelog_md());
        assert_eq!(config.ingest().rebuild_threshold(), 50);
        assert_eq!(config.user().map(Actor::as_str), Some("alice"));
        assert_eq!(settings.baseline(), "trunk");
        assert_eq!(
            settings.private_dir(),
            Path::new("/data/demo-0192f3a4-5b6c-7d8e-9fa0-b1c2d3e4f506")
        );
        assert_eq!(settings.project_id().to_string(), ID);
        assert_eq!(
            config.test_files().unwrap().globs(),
            ["src/**/*_test.rs"],
            "test.files narrows the test files"
        );
        assert_eq!(config.test().timeout().get(), Duration::from_secs(45 * 60));
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
        assert_eq!(
            config.test().pin_search(),
            &PinSearch::default(),
            "no key: the scanner's own pattern, the default vocabulary"
        );
        assert_eq!(config.gate_policy(), GatePolicy::default());
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
            Path::new("/fake-home/.local/share/whyshelf/demo-0192f3a4-5b6c-7d8e-9fa0-b1c2d3e4f506")
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

    fn with_tables(tables: &str) -> Result<Settings, ConfigError> {
        parse_full(&format!(
            "version = 1\nproject = \"d\"\n[test]\ncommand = \"x\"\n{tables}"
        ))
    }

    #[test]
    fn the_gate_table_sets_what_cs_demands() {
        let relaxed = with_tables("[gate]\ncs_requires_pins = false\n").unwrap();
        assert_eq!(relaxed.config().gate_policy(), GatePolicy::new(false, true));
        let both =
            with_tables("[gate]\ncs_requires_pins = false\ncs_requires_hypotheses = false\n")
                .unwrap();
        assert_eq!(both.config().gate_policy(), GatePolicy::new(false, false));
        let written = with_tables("[gate]\ncs_requires_pins = true\n").unwrap();
        assert_eq!(written.config().gate_policy(), GatePolicy::default());
        assert!(matches!(
            with_tables("[gate]\ncs_requires_tests = false\n"),
            Err(ConfigError::Parse(_))
        ));
        assert!(matches!(
            with_tables("[gate]\ncs_requires_pins = \"no\"\n"),
            Err(ConfigError::Parse(_))
        ));
    }

    #[test]
    fn the_pins_table_sets_the_vocabulary_of_the_scan() {
        let settings = with_tables(
            "[pins]\nprefixes = { REQ = \"pin\", UT = \"unit-test\" }\nnormalize = \"upper-dash\"\n[pins.mapping]\nAUTH_LOGIN = \"REQ-AUTH-LOGIN\"\n",
        )
        .unwrap();
        let vocabulary = settings.config().test().pin_search().vocabulary();
        let read =
            |mark: &str| PinId::parse_in(vocabulary, mark).map(|p| (p.to_string(), p.kind()));
        assert_eq!(
            read("REQ-LOGIN"),
            Ok(("REQ-LOGIN".to_owned(), PinKind::Pin))
        );
        assert_eq!(
            read("ut_parse"),
            Ok(("UT-PARSE".to_owned(), PinKind::UnitTest))
        );
        assert_eq!(
            read("AUTH_LOGIN"),
            Ok(("REQ-AUTH-LOGIN".to_owned(), PinKind::Pin))
        );
        assert!(
            read("PIN-A").is_err(),
            "written prefixes replace the default"
        );
        assert_eq!(vocabulary.normalize(), PinNormalize::UpperDash);
        // No table: the default vocabulary.
        assert_eq!(
            with_tables("")
                .unwrap()
                .config()
                .test()
                .pin_search()
                .vocabulary(),
            &PinVocabulary::default()
        );
    }

    #[test]
    fn a_refused_value_of_the_pins_table_names_its_key() {
        let field_of = |tables: &str| match with_tables(tables) {
            Err(ConfigError::Value { field, .. } | ConfigError::Word { field, .. }) => field,
            other => panic!("{tables}: {other:?}"),
        };
        assert_eq!(
            field_of("[pins]\nprefixes = { req = \"pin\" }\n"),
            "pins.prefixes.req"
        );
        assert_eq!(
            field_of("[pins]\nprefixes = { HYP = \"pin\" }\n"),
            "pins.prefixes.HYP"
        );
        assert_eq!(
            field_of("[pins]\nprefixes = { REQ = \"invariant\" }\n"),
            "pins.prefixes.REQ"
        );
        assert_eq!(field_of("[pins]\nprefixes = {}\n"), "pins.prefixes");
        assert_eq!(
            field_of("[pins]\nnormalize = \"lower\"\n"),
            "pins.normalize"
        );
        assert_eq!(
            field_of("[pins.mapping]\nAUTH_LOGIN = \"REQ-AUTH-LOGIN\"\n"),
            "pins.mapping.AUTH_LOGIN",
            "a pin of a prefix the vocabulary does not list"
        );
        assert_eq!(
            field_of("[pins.mapping]\nAUTH_LOGIN = \"pin_auth_login\"\n"),
            "pins.mapping.AUTH_LOGIN",
            "the pin of the table is written as a Spec writes it"
        );
        assert_eq!(
            field_of("[pins.mapping]\n\"AUTH LOGIN\" = \"PIN-AUTH-LOGIN\"\n"),
            "pins.mapping.AUTH LOGIN"
        );
        let Err(error) = with_tables("[pins]\nnormalize = \"lower\"\n") else {
            panic!("refused");
        };
        assert_eq!(
            error.to_string(),
            "`pins.normalize` is `lower`; it is one of `strict`, `upper-dash`"
        );
        assert!(matches!(
            with_tables("[pins]\nsuffixes = []\n"),
            Err(ConfigError::Parse(_))
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
        assert_eq!(
            settings.private_dir(),
            dir.path().join("data").join(format!("demo-{ID}"))
        );
        assert!(matches!(
            load(&dir.path().join("none"), &dirs).await,
            Err(ConfigError::Read { .. })
        ));
    }

    #[test]
    fn a_project_of_the_same_name_with_another_id_has_another_private_store() {
        let dirs = UserDirs::default().with_data_dir("/data");
        let other = MINIMAL.replace(ID, "0192f3a4-5b6c-7d8e-9fa0-b1c2d3e4f507");
        let (one, two) = (
            parse(MINIMAL, &dirs).unwrap(),
            parse(&other, &dirs).unwrap(),
        );
        assert_eq!(one.config().project(), two.config().project());
        assert_ne!(one.private_dir(), two.private_dir());
        // The same file in another clone or worktree is the same store.
        assert_eq!(
            one.private_dir(),
            parse(MINIMAL, &dirs).unwrap().private_dir()
        );
    }

    #[tokio::test]
    async fn a_settings_file_without_an_id_says_the_line_to_add_and_is_not_touched() {
        let without = "version = 1\nproject = \"demo\"\n[test]\ncommand = \"x\"\n";
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join(FILE_NAME);
        tokio::fs::write(&file, without).await.unwrap();
        let dirs = UserDirs::default().with_data_dir("/data");
        let Err(error) = load(dir.path(), &dirs).await else {
            panic!("an id is required");
        };
        let ConfigError::MissingProjectId { suggested } = &error else {
            panic!("expected MissingProjectId, got {error:?}");
        };
        // The message carries the line, with a UUID v7, ready to paste.
        let line = format!("project_id = \"{suggested}\"");
        assert!(error.to_string().contains(&line));
        assert_eq!(suggested.as_uuid().get_version_num(), 7);
        let reread: toml::Table = toml::from_str(&line).unwrap();
        let text = reread["project_id"].as_str().unwrap();
        assert_eq!(
            text.parse::<ProjectId>()
                .unwrap()
                .as_uuid()
                .get_version_num(),
            7
        );
        // Not a byte of the file changed.
        assert_eq!(tokio::fs::read_to_string(&file).await.unwrap(), without);
    }

    #[test]
    fn a_project_id_that_is_no_uuid_is_refused_and_a_path_never_reaches_the_store() {
        for bad in ["../x", "demo", "", "0192f3a4"] {
            let text = MINIMAL.replace(ID, bad);
            assert!(
                matches!(
                    parse_full(&text),
                    Err(ConfigError::Value { ref field, .. }) if field == "project_id"
                ),
                "{bad}"
            );
        }
    }

    #[test]
    fn the_test_files_and_the_time_limit_have_defaults_and_refuse_nonsense() {
        let settings = parse_full("version = 1\nproject = \"d\"\n[paths]\nscan_dirs = [\"src\", \"tests\"]\n[test]\ncommand = \"x\"\n").unwrap();
        let config = settings.config();
        // Left out: the whole of the scan directories, and thirty minutes.
        assert_eq!(config.test_files().unwrap().globs(), ["src", "tests"]);
        assert_eq!(config.test().timeout().get(), Duration::from_secs(30 * 60));
        for bad in ["0s", "30", "m", "1d", "-5m", "1.5h", ""] {
            let text = format!(
                "version = 1\nproject = \"d\"\n[test]\ncommand = \"x\"\ntimeout = \"{bad}\"\n"
            );
            assert!(parse_full(&text).is_err(), "{bad}");
        }
        for bad in ["[]", "[\"/abs\"]", "[\"../up\"]", "[\":(top)x\"]"] {
            let text =
                format!("version = 1\nproject = \"d\"\n[test]\ncommand = \"x\"\nfiles = {bad}\n");
            assert!(
                matches!(parse_full(&text), Err(ConfigError::Value { .. })),
                "{bad}"
            );
        }
    }
}
