//! The person's own settings: what only the person decides.
//!
//! A project's committed settings (`.whyshelf.toml`, see [`crate::config`]) can be
//! changed by anyone who can change the repository, so they decide nothing that
//! switches a protection off or says who someone is. Those few things are the
//! person's own, in one file in the person's own configuration directory
//! ([`UserDirs::config_dir`]):
//!
//! ```text
//! $XDG_CONFIG_HOME/whyshelf/settings.toml      else  $HOME/.config/whyshelf/settings.toml
//! ```
//!
//! ```toml
//! record_email = false          # put the git mail beside the name in a record (default: no)
//! principal = "human"           # declare that a person works here (default: nothing)
//! agent_env = ["MY_AGENT_VAR"]  # names of environment variables a tool sets in its child processes
//! private_root = "/path/to/private/documents"   # or "none"; an absolute path
//! ```
//!
//! | key | decides | read by |
//! |---|---|---|
//! | `record_email` | whether a record carries a mail address; a shared ledger is a shared log, so it does not by default | [`crate::actor_env`] |
//! | `principal` | the declaration of a person, the only one that counts: a process's own environment cannot declare one, since an agent can set its own | [`crate::actor_env`] |
//! | `agent_env` | which environment variables tell that an agent is running; no tool's variable is named in the code, the person lists them | [`crate::actor_env`] |
//! | `private_root` | where the person's private documents are, which also decides whether the check for leaks has anything to check | [`private_root`] |
//!
//! Unknown keys are refused, so a typo is an error and not a setting that silently
//! does nothing. Nothing is expanded: no `~`, no variables inside a value.
//!
//! # The private root
//!
//! [`private_root`] gives the three values of [`PrivateRoot`], and where each came
//! from, so that the commands that show them ([`PrivateRootStatus`]) can say what
//! was read:
//!
//! | the settings | the value |
//! |---|---|
//! | there is no place to look (no usable `XDG_CONFIG_HOME` or `HOME`), or the place has no file, or the file has no `private_root`, or it says `"none"` | [`PrivateRoot::NotUsedHere`] |
//! | `private_root` is a path and the directory can be listed | [`PrivateRoot::Present`] |
//! | `private_root` is a path and the directory cannot be listed (missing, no permission, a disk that is not attached) | [`PrivateRoot::Absent`] |
//! | the file is there and cannot be read or understood (no permission, not TOML, an unknown key, a relative path) | [`PrivateRoot::Absent`] |
//!
//! The last row is the one to notice: a file that cannot be read cannot be told to
//! hold no private root, so it is not read as "none". A check for leaks that
//! passed because the settings were unreadable would pass exactly when it should
//! not. A person whose environment never reads this file (another
//! `XDG_CONFIG_HOME`, a container) gets [`PrivateRoot::NotUsedHere`] all the same;
//! that is why the value and its source are shown by the commands.

use std::io;
use std::path::{Path, PathBuf};

use serde::Deserialize;
use whyshelf_core::domain::boundary::PrivateRoot;

use crate::config::UserDirs;

/// The name of the settings file in the person's configuration directory.
pub const SETTINGS_FILE: &str = "settings.toml";

/// What `private_root` says.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PrivateRootSetting {
    /// The key is not there.
    Unset,
    /// The person wrote `"none"`: no private documents here.
    Nowhere,
    /// An absolute path.
    At(PathBuf),
}

/// The person's settings, as the file holds them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UserSettings {
    record_email: bool,
    human: bool,
    agent_env: Vec<String>,
    private_root: PrivateRootSetting,
}

impl UserSettings {
    /// Settings with nothing set: the same as a file with no keys.
    pub fn empty() -> Self {
        Self {
            record_email: false,
            human: false,
            agent_env: Vec::new(),
            private_root: PrivateRootSetting::Unset,
        }
    }

    /// Whether a record carries the git mail.
    pub fn record_email(&self) -> bool {
        self.record_email
    }

    /// Whether the person declared `principal = "human"`.
    pub fn declares_human(&self) -> bool {
        self.human
    }

    /// The names of the environment variables that tell an agent is running, in the
    /// order the person wrote them.
    pub fn agent_env(&self) -> &[String] {
        &self.agent_env
    }

    /// What `private_root` says.
    pub fn private_root(&self) -> &PrivateRootSetting {
        &self.private_root
    }
}

/// Why a settings file that is there could not be used.
#[derive(Debug, thiserror::Error)]
pub enum SettingsProblem {
    /// The file could not be read.
    #[error("cannot read the file")]
    Read(#[source] io::Error),
    /// The file is not valid TOML, or has a key or a value of the wrong kind.
    #[error("the file is not valid")]
    Parse(#[source] toml::de::Error),
    /// A value is refused by the rule of its key.
    #[error("{key}: {detail}")]
    Invalid {
        /// The key.
        key: &'static str,
        /// What is wrong.
        detail: String,
    },
}

/// The person's settings file, as far as it could be found and read.
///
/// The four cases are kept apart because they mean different things to the check
/// for leaks (see the module documentation).
#[derive(Debug)]
pub enum SettingsFile {
    /// No usable configuration directory in the environment.
    NoPlace,
    /// The configuration directory has no settings file.
    Missing {
        /// Where the file would be.
        path: PathBuf,
    },
    /// The file was read.
    Read {
        /// The file.
        path: PathBuf,
        /// What it holds.
        settings: UserSettings,
    },
    /// The file is there and could not be used.
    Unreadable {
        /// The file.
        path: PathBuf,
        /// Why.
        problem: SettingsProblem,
    },
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct FileDto {
    #[serde(default)]
    record_email: bool,
    principal: Option<PrincipalDto>,
    #[serde(default)]
    agent_env: Vec<String>,
    private_root: Option<String>,
}

#[derive(Debug, Deserialize)]
enum PrincipalDto {
    #[serde(rename = "human")]
    Human,
}

impl SettingsFile {
    /// Read the settings file of the environment `dirs` describes.
    pub async fn load(dirs: &UserDirs) -> Self {
        let Some(dir) = dirs.config_dir() else {
            return Self::NoPlace;
        };
        Self::load_from(dir.join(SETTINGS_FILE)).await
    }

    /// Read the settings file at `path`.
    pub async fn load_from(path: PathBuf) -> Self {
        let bytes = match tokio::fs::read(&path).await {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                return Self::Missing { path };
            }
            Err(error) => {
                return Self::Unreadable {
                    path,
                    problem: SettingsProblem::Read(error),
                };
            }
        };
        match parse(&bytes) {
            Ok(settings) => Self::Read { path, settings },
            Err(problem) => Self::Unreadable { path, problem },
        }
    }

    /// The file this came from, if there is a place for one.
    pub fn path(&self) -> Option<&Path> {
        match self {
            Self::NoPlace => None,
            Self::Missing { path } | Self::Read { path, .. } | Self::Unreadable { path, .. } => {
                Some(path)
            }
        }
    }

    /// The settings that can be relied on: those of a file that was read, and none
    /// when there is no file. `None` for a file that is there and cannot be used.
    pub fn usable(&self) -> Option<UserSettings> {
        match self {
            Self::NoPlace | Self::Missing { .. } => Some(UserSettings::empty()),
            Self::Read { settings, .. } => Some(settings.clone()),
            Self::Unreadable { .. } => None,
        }
    }
}

fn parse(bytes: &[u8]) -> Result<UserSettings, SettingsProblem> {
    let text = std::str::from_utf8(bytes).map_err(|_| SettingsProblem::Invalid {
        key: "file",
        detail: "the file is not UTF-8".to_owned(),
    })?;
    let dto: FileDto = toml::from_str(text).map_err(SettingsProblem::Parse)?;
    for name in &dto.agent_env {
        if !is_env_name(name) {
            return Err(SettingsProblem::Invalid {
                key: "agent_env",
                detail: format!("`{name}` is not the name of an environment variable"),
            });
        }
    }
    let private_root = match dto.private_root.as_deref() {
        None => PrivateRootSetting::Unset,
        Some("none") => PrivateRootSetting::Nowhere,
        Some(path) => {
            let path = PathBuf::from(path);
            if !path.is_absolute() {
                return Err(SettingsProblem::Invalid {
                    key: "private_root",
                    detail: format!(
                        "`{}` is not an absolute path (and not `none`)",
                        path.display()
                    ),
                });
            }
            PrivateRootSetting::At(path)
        }
    };
    Ok(UserSettings {
        record_email: dto.record_email,
        human: dto
            .principal
            .is_some_and(|p| matches!(p, PrincipalDto::Human)),
        agent_env: dto.agent_env,
        private_root,
    })
}

/// Whether a text can be the name of an environment variable: a letter or `_`, then
/// letters, digits and `_`.
fn is_env_name(name: &str) -> bool {
    let mut chars = name.chars();
    chars
        .next()
        .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// The value of [`PrivateRoot`] in this environment, with where it came from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrivateRootStatus {
    value: PrivateRoot,
    path: Option<PathBuf>,
    source: Option<PathBuf>,
}

impl PrivateRootStatus {
    /// The value, for the domain.
    pub fn value(&self) -> PrivateRoot {
        self.value
    }

    /// The directory the settings name, if they name one (also when it cannot be
    /// read).
    pub fn path(&self) -> Option<&Path> {
        self.path.as_deref()
    }

    /// The settings file the value came from; `None` when there was no file to
    /// come from (no place for one, or none in the place).
    pub fn source(&self) -> Option<&Path> {
        self.source.as_deref()
    }
}

/// Decide [`PrivateRoot`] from the person's settings and the disk; the table is in
/// the module documentation.
///
/// "Can be read" is: the directory can be listed.
pub async fn private_root(settings: &SettingsFile) -> PrivateRootStatus {
    let status = |value, path: Option<PathBuf>, source: Option<&Path>| PrivateRootStatus {
        value,
        path,
        source: source.map(Path::to_path_buf),
    };
    match settings {
        SettingsFile::NoPlace | SettingsFile::Missing { .. } => {
            status(PrivateRoot::NotUsedHere, None, None)
        }
        SettingsFile::Unreadable { path, .. } => status(PrivateRoot::Absent, None, Some(path)),
        SettingsFile::Read { path, settings } => match settings.private_root() {
            PrivateRootSetting::Unset | PrivateRootSetting::Nowhere => {
                status(PrivateRoot::NotUsedHere, None, Some(path))
            }
            PrivateRootSetting::At(root) => {
                let value = match tokio::fs::read_dir(root).await {
                    Ok(_) => PrivateRoot::Present,
                    Err(_) => PrivateRoot::Absent,
                };
                status(value, Some(root.clone()), Some(path))
            }
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dirs(config_home: &Path) -> UserDirs {
        UserDirs::default().with_config_home(config_home)
    }

    async fn write_settings(config_home: &Path, text: &str) {
        let dir = config_home.join("whyshelf");
        tokio::fs::create_dir_all(&dir).await.unwrap();
        tokio::fs::write(dir.join(SETTINGS_FILE), text)
            .await
            .unwrap();
    }

    #[test]
    fn the_config_dir_follows_the_xdg_order_and_skips_what_is_not_absolute() {
        let both = UserDirs::default()
            .with_config_home("/cfg")
            .with_home("/srv/alice");
        assert_eq!(both.config_dir().unwrap(), Path::new("/cfg/whyshelf"));
        let home = UserDirs::default().with_home("/srv/alice");
        assert_eq!(
            home.config_dir().unwrap(),
            Path::new("/srv/alice/.config/whyshelf")
        );
        let relative = UserDirs::default()
            .with_config_home("cfg")
            .with_home("/srv/alice");
        assert_eq!(
            relative.config_dir().unwrap(),
            Path::new("/srv/alice/.config/whyshelf")
        );
        assert!(UserDirs::default().config_dir().is_none());
    }

    #[tokio::test]
    async fn no_place_and_no_file_are_not_used_here() {
        let none = SettingsFile::load(&UserDirs::default()).await;
        assert!(matches!(none, SettingsFile::NoPlace));
        let status = private_root(&none).await;
        assert_eq!(status.value(), PrivateRoot::NotUsedHere);
        assert!(status.source().is_none());

        let dir = tempfile::tempdir().unwrap();
        let missing = SettingsFile::load(&dirs(dir.path())).await;
        assert!(matches!(missing, SettingsFile::Missing { .. }));
        assert_eq!(
            private_root(&missing).await.value(),
            PrivateRoot::NotUsedHere
        );
    }

    #[tokio::test]
    async fn a_file_without_a_root_or_with_none_is_not_used_here() {
        for text in ["record_email = true\n", "private_root = \"none\"\n", ""] {
            let dir = tempfile::tempdir().unwrap();
            write_settings(dir.path(), text).await;
            let file = SettingsFile::load(&dirs(dir.path())).await;
            let status = private_root(&file).await;
            assert_eq!(status.value(), PrivateRoot::NotUsedHere, "{text:?}");
            assert_eq!(
                status.source(),
                file.path(),
                "the file it was read from is named"
            );
        }
    }

    #[tokio::test]
    async fn a_root_that_can_be_listed_is_present_and_one_that_cannot_is_absent() {
        let dir = tempfile::tempdir().unwrap();
        let root = tempfile::tempdir().unwrap();
        write_settings(
            dir.path(),
            &format!("private_root = {:?}\n", root.path().display().to_string()),
        )
        .await;
        let file = SettingsFile::load(&dirs(dir.path())).await;
        let status = private_root(&file).await;
        assert_eq!(status.value(), PrivateRoot::Present);
        assert_eq!(status.path(), Some(root.path()));

        let gone = dir.path().join("not-attached");
        write_settings(
            dir.path(),
            &format!("private_root = {:?}\n", gone.display().to_string()),
        )
        .await;
        let file = SettingsFile::load(&dirs(dir.path())).await;
        let status = private_root(&file).await;
        assert_eq!(status.value(), PrivateRoot::Absent);
        assert_eq!(status.path(), Some(gone.as_path()));
    }

    #[tokio::test]
    async fn a_file_that_cannot_be_used_makes_the_root_absent_not_unused() {
        for text in [
            "private_root = [\n",
            "private_roots = \"/x\"\n",
            "private_root = \"relative/dir\"\n",
            "principal = \"humen\"\n",
            "agent_env = [\"NOT A NAME\"]\n",
        ] {
            let dir = tempfile::tempdir().unwrap();
            write_settings(dir.path(), text).await;
            let file = SettingsFile::load(&dirs(dir.path())).await;
            assert!(matches!(file, SettingsFile::Unreadable { .. }), "{text:?}");
            assert!(file.usable().is_none());
            assert_eq!(
                private_root(&file).await.value(),
                PrivateRoot::Absent,
                "{text:?}"
            );
        }
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn a_file_that_is_there_and_cannot_be_read_makes_the_root_absent() {
        use std::os::unix::fs::PermissionsExt;

        let dir = tempfile::tempdir().unwrap();
        write_settings(dir.path(), "private_root = \"none\"\n").await;
        let path = dir.path().join("whyshelf").join(SETTINGS_FILE);
        tokio::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o000))
            .await
            .unwrap();
        // A user who may read everything (root) gets no refusal to test.
        if tokio::fs::read(&path).await.is_ok() {
            return;
        }
        let file = SettingsFile::load(&dirs(dir.path())).await;
        assert!(matches!(
            file,
            SettingsFile::Unreadable {
                problem: SettingsProblem::Read(_),
                ..
            }
        ));
        assert_eq!(private_root(&file).await.value(), PrivateRoot::Absent);
    }

    #[tokio::test]
    async fn every_key_is_read() {
        let dir = tempfile::tempdir().unwrap();
        write_settings(
            dir.path(),
            "record_email = true\nprincipal = \"human\"\nagent_env = [\"A_VAR\", \"B\"]\nprivate_root = \"none\"\n",
        )
        .await;
        let settings = SettingsFile::load(&dirs(dir.path()))
            .await
            .usable()
            .unwrap();
        assert!(settings.record_email());
        assert!(settings.declares_human());
        assert_eq!(settings.agent_env(), ["A_VAR", "B"]);
        assert_eq!(settings.private_root(), &PrivateRootSetting::Nowhere);
    }
}
