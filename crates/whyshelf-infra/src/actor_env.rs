//! Who is acting: [`ActorSource`] over the environment, the Git configuration and the
//! person's own settings ([`crate::user_settings`]).
//!
//! The domain receives a [`Judge`] as a value; every rule for making one is here, in
//! one place.
//!
//! # The name
//!
//! In order: `WHYSHELF_ACTOR` from the environment; the Git `user.name` of the
//! project. The project's committed settings are never read for a name (a `[user]`
//! there would be the name of everyone who clones the project). A name that is
//! given and refused by the rules of [`PersonName`] is an error, not a reason to
//! try the next source; no name at all is [`ActorError::NoName`], with the two ways
//! to give one.
//!
//! # The mail
//!
//! Not recorded, unless the person's settings say `record_email = true`; then it is
//! the Git `user.email` of the project, if there is one that has the shape of an
//! address. A shared ledger is a shared log, and a person's data does not go in it
//! by default.
//!
//! # The principal
//!
//! ```text
//!   an agent mark?                      WHYSHELF_AGENT=<tool>              -> Agent { tool }
//!     (any one is enough)               the tool server's client's name    -> Agent { tool }
//!                                       a variable the person listed in
//!                                       `agent_env`, set and not empty     -> Agent { tool = its name }
//!   no mark, and the person's settings say `principal = "human"`           -> Human
//!   otherwise                                                             -> Undeclared
//! ```
//!
//! - **An agent mark wins over a declaration of a person.** A tool that runs inside
//!   the person's session is still a tool.
//! - **A person is declared only by the person's settings file**, never by the
//!   environment of the process: an agent can set its own environment. The file can
//!   be written by an agent that runs as the same user, so the declaration is for
//!   the record and for stopping a slip, not a proof.
//! - **Nothing is declared by default**, so that an agent typing at a shell is not
//!   recorded as a person.
//! - **No tool's variable is named here.** The person lists them in `agent_env`;
//!   `WHYSHELF_AGENT` is the one this program defines.
//! - A settings file that is there and cannot be used declares nothing and lists no
//!   variable: the principal is `Undeclared` and no mail is recorded.
//!
//! The route (the command line or the tool server) is not part of the judge; the
//! tool server's adapter gives the client's name, if it has one, to
//! [`EnvActor::with_client`].

use std::collections::BTreeMap;
use std::path::PathBuf;

use whyshelf_core::app::ports::ActorSource;
use whyshelf_core::domain::error::JudgeError;
use whyshelf_core::domain::judge::{Email, Judge, PersonName, Principal, ToolName};

use crate::git::{Git, GitError};
use crate::user_settings::{SettingsFile, UserSettings};

/// The variable that gives the name of the person acting.
pub const ACTOR_VAR: &str = "WHYSHELF_ACTOR";

/// The variable a tool sets to declare that it is an agent; its value is the tool's
/// name.
pub const AGENT_VAR: &str = "WHYSHELF_AGENT";

/// Why the judge could not be made.
#[derive(Debug, thiserror::Error)]
pub enum ActorError {
    /// Git could not be asked.
    #[error("cannot read the Git configuration")]
    Git(#[from] GitError),
    /// No source gave a name.
    #[error("no name for the person acting; set {ACTOR_VAR} or the Git `user.name`")]
    NoName,
    /// A name was given and is refused.
    #[error("the name of the person acting is refused")]
    Name(#[source] JudgeError),
    /// A tool's name is refused.
    #[error("the name of the tool acting is refused")]
    Tool(#[source] JudgeError),
}

/// A snapshot of the environment of the process.
///
/// A value, so that the rules are tested without touching the environment of the
/// test process.
#[derive(Debug, Clone, Default)]
pub struct ProcessEnv(BTreeMap<String, String>);

impl ProcessEnv {
    /// The variables of this process that are valid text.
    pub fn capture() -> Self {
        Self(
            std::env::vars_os()
                .filter_map(|(k, v)| Some((k.into_string().ok()?, v.into_string().ok()?)))
                .collect(),
        )
    }

    /// An environment of these variables.
    pub fn of<'a>(vars: impl IntoIterator<Item = (&'a str, &'a str)>) -> Self {
        Self(
            vars.into_iter()
                .map(|(k, v)| (k.to_owned(), v.to_owned()))
                .collect(),
        )
    }

    /// A variable's value; none when it is unset or empty after trimming.
    fn get(&self, name: &str) -> Option<&str> {
        self.0.get(name).map(|v| v.trim()).filter(|v| !v.is_empty())
    }
}

/// The acting judge, from the environment and the Git configuration of a project.
#[derive(Debug, Clone)]
pub struct EnvActor {
    git: Git,
    env: ProcessEnv,
    settings: Option<UserSettings>,
    client: Option<String>,
}

impl EnvActor {
    /// The judge of the project at `root`, in this environment and with these
    /// settings of the person.
    pub fn new(root: PathBuf, env: ProcessEnv, settings: &SettingsFile) -> Self {
        Self {
            git: Git::new(root),
            env,
            settings: settings.usable(),
            client: None,
        }
    }

    /// The client of the tool server declared itself with this name (its
    /// `clientInfo.name`): that is an agent mark.
    pub fn with_client(mut self, name: impl Into<String>) -> Self {
        self.client = Some(name.into());
        self
    }

    /// The Git setting `key`; none when unset or empty.
    async fn git_config(&self, key: &str) -> Result<Option<String>, GitError> {
        let finished = self.git.finish(["config", "--get", key]).await?;
        // Exit 1: the key is not set. Anything else that is not success is no answer.
        if finished.code != Some(0) {
            return Ok(None);
        }
        let value = String::from_utf8_lossy(&finished.stdout).trim().to_owned();
        Ok((!value.is_empty()).then_some(value))
    }

    /// The tool an agent mark names, if there is a mark.
    fn agent_tool(&self) -> Result<Option<ToolName>, ActorError> {
        let named = self
            .env
            .get(AGENT_VAR)
            .map(str::to_owned)
            .or_else(|| {
                self.client
                    .as_deref()
                    .map(str::trim)
                    .filter(|c| !c.is_empty())
                    .map(str::to_owned)
            })
            .or_else(|| {
                self.settings.as_ref().and_then(|s| {
                    s.agent_env()
                        .iter()
                        .find(|name| self.env.get(name).is_some())
                        .cloned()
                })
            });
        named
            .map(|tool| ToolName::new(&tool).map_err(ActorError::Tool))
            .transpose()
    }

    async fn judge(&self) -> Result<Judge, ActorError> {
        let name = match self.env.get(ACTOR_VAR) {
            Some(name) => name.to_owned(),
            None => self
                .git_config("user.name")
                .await?
                .ok_or(ActorError::NoName)?,
        };
        let name = PersonName::new(&name).map_err(ActorError::Name)?;

        let email = if self.settings.as_ref().is_some_and(|s| s.record_email()) {
            self.git_config("user.email")
                .await?
                .and_then(|mail| Email::new(&mail).ok())
        } else {
            None
        };

        let principal = match self.agent_tool()? {
            Some(tool) => Principal::Agent { tool },
            None if self.settings.as_ref().is_some_and(|s| s.declares_human()) => Principal::Human,
            None => Principal::Undeclared,
        };
        Ok(Judge::new(name, email, principal))
    }
}

impl ActorSource for EnvActor {
    type Error = ActorError;

    async fn actor(&self) -> Result<Judge, ActorError> {
        self.judge().await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::UserDirs;
    use crate::user_settings::SETTINGS_FILE;
    use std::path::Path;

    async fn repo(user: Option<(&str, &str)>) -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        let git = Git::new(dir.path().to_path_buf());
        git.run(["init", "-q"]).await.unwrap();
        if let Some((name, mail)) = user {
            git.run(["config", "user.name", name]).await.unwrap();
            git.run(["config", "user.email", mail]).await.unwrap();
        }
        dir
    }

    async fn settings(text: Option<&str>) -> (tempfile::TempDir, SettingsFile) {
        let dir = tempfile::tempdir().unwrap();
        if let Some(text) = text {
            let sub = dir.path().join("whyshelf");
            tokio::fs::create_dir_all(&sub).await.unwrap();
            tokio::fs::write(sub.join(SETTINGS_FILE), text)
                .await
                .unwrap();
        }
        let file = SettingsFile::load(&UserDirs::default().with_config_home(dir.path())).await;
        (dir, file)
    }

    fn actor(root: &Path, env: ProcessEnv, settings: &SettingsFile) -> EnvActor {
        EnvActor::new(root.to_path_buf(), env, settings)
    }

    #[tokio::test]
    async fn the_name_is_the_variable_then_git_and_nothing_committed() {
        let repo = repo(Some(("Git Person", "git@example.org"))).await;
        // A committed `[user]` is not read, whatever it says.
        tokio::fs::write(
            repo.path().join(".whyshelf.toml"),
            "[user]\nname = \"committed\"\n",
        )
        .await
        .unwrap();
        let (_dir, none) = settings(None).await;

        let from_git = actor(repo.path(), ProcessEnv::default(), &none)
            .actor()
            .await
            .unwrap();
        assert_eq!(from_git.name().as_str(), "Git Person");

        let env = ProcessEnv::of([(ACTOR_VAR, "Env Person")]);
        let from_env = actor(repo.path(), env, &none).actor().await.unwrap();
        assert_eq!(from_env.name().as_str(), "Env Person");
    }

    #[tokio::test]
    async fn no_name_is_an_error_and_a_refused_name_is_not_skipped() {
        let repo = repo(None).await;
        let (_dir, none) = settings(None).await;
        // The test environment may have a global Git identity; only the case with
        // none is checked, and the refused-name case holds either way.
        let bad = ProcessEnv::of([(ACTOR_VAR, "two\nlines")]);
        assert!(matches!(
            actor(repo.path(), bad, &none).actor().await,
            Err(ActorError::Name(_))
        ));
        let unset = actor(repo.path(), ProcessEnv::default(), &none)
            .actor()
            .await;
        assert!(matches!(unset, Ok(_) | Err(ActorError::NoName)));
    }

    #[tokio::test]
    async fn the_mail_is_recorded_only_when_the_person_says_so() {
        let repo = repo(Some(("Ada", "Ada@Example.org"))).await;
        let (_off, off) = settings(None).await;
        let judge = actor(repo.path(), ProcessEnv::default(), &off)
            .actor()
            .await
            .unwrap();
        assert!(judge.email().is_none());

        let (_on, on) = settings(Some("record_email = true\n")).await;
        let judge = actor(repo.path(), ProcessEnv::default(), &on)
            .actor()
            .await
            .unwrap();
        assert_eq!(judge.email().unwrap().as_str(), "ada@example.org");
    }

    #[tokio::test]
    async fn nothing_declared_is_undeclared_and_only_the_settings_declare_a_person() {
        let repo = repo(Some(("Ada", "ada@example.org"))).await;
        let (_none_dir, none) = settings(None).await;
        let judge = actor(repo.path(), ProcessEnv::default(), &none)
            .actor()
            .await
            .unwrap();
        assert_eq!(judge.principal(), &Principal::Undeclared);

        // The environment of the process cannot say "human".
        let env = ProcessEnv::of([("WHYSHELF_PRINCIPAL", "human"), ("PRINCIPAL", "human")]);
        let judge = actor(repo.path(), env, &none).actor().await.unwrap();
        assert_eq!(judge.principal(), &Principal::Undeclared);

        let (_human_dir, human) = settings(Some("principal = \"human\"\n")).await;
        let judge = actor(repo.path(), ProcessEnv::default(), &human)
            .actor()
            .await
            .unwrap();
        assert_eq!(judge.principal(), &Principal::Human);
    }

    #[tokio::test]
    async fn an_agent_mark_wins_over_a_declaration_of_a_person() {
        let repo = repo(Some(("Ada", "ada@example.org"))).await;
        let (_dir, human) = settings(Some(
            "principal = \"human\"\nagent_env = [\"LISTED_TOOL_VAR\"]\n",
        ))
        .await;
        let tool = |judge: &Judge| match judge.principal() {
            Principal::Agent { tool } => tool.as_str().to_owned(),
            other => panic!("not an agent: {other:?}"),
        };

        let by_var = ProcessEnv::of([(AGENT_VAR, "some-tool")]);
        let judge = actor(repo.path(), by_var, &human).actor().await.unwrap();
        assert_eq!(tool(&judge), "some-tool");

        let by_list = ProcessEnv::of([("LISTED_TOOL_VAR", "1")]);
        let judge = actor(repo.path(), by_list, &human).actor().await.unwrap();
        assert_eq!(tool(&judge), "LISTED_TOOL_VAR");

        let judge = actor(repo.path(), ProcessEnv::default(), &human)
            .with_client("mcp-client")
            .actor()
            .await
            .unwrap();
        assert_eq!(tool(&judge), "mcp-client");

        // A listed variable that is empty is not a mark, and an unlisted one is none.
        let empty = ProcessEnv::of([("LISTED_TOOL_VAR", "  "), ("UNLISTED_VAR", "1")]);
        let judge = actor(repo.path(), empty, &human).actor().await.unwrap();
        assert_eq!(judge.principal(), &Principal::Human);
    }

    #[tokio::test]
    async fn settings_that_cannot_be_used_declare_nothing() {
        let repo = repo(Some(("Ada", "ada@example.org"))).await;
        let (_dir, broken) = settings(Some("principal = \"human\"\nrecord_email = [\n")).await;
        let judge = actor(repo.path(), ProcessEnv::default(), &broken)
            .actor()
            .await
            .unwrap();
        assert_eq!(judge.principal(), &Principal::Undeclared);
        assert!(judge.email().is_none());
    }
}
