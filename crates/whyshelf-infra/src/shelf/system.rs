//! What the domain never makes itself: the time, new identities, and who acts.
//!
//! | adapter | port | from |
//! |---|---|---|
//! | [`SystemClock`] | [`Clock`] | the system time, in unix milliseconds |
//! | [`UuidV7Ids`] | [`IdSource`] | a UUID v7 (sorts by creation, the surrogate key of a record and the identity of a new document) |
//! | [`ProjectActor`] | [`ActorSource`] | the name the caller took from `WHYSHELF_ACTOR`, else Git's `user.name` of the project (asked only when no actor was given on the command line) |
//!
//! Who acts is never read from a file that is committed: a committed file would let
//! anyone write a judgement in another person's name.

use std::path::PathBuf;

use uuid::Uuid;
use whyshelf_core::app::shelf::{ActorSource, Clock, IdSource};
use whyshelf_core::domain::actor::Actor;
use whyshelf_core::domain::error::ValueError;
use whyshelf_core::domain::id::{DocId, EntityId};
use whyshelf_core::domain::shelf::ledger::{JudgedAt, JudgementId};

/// The system clock.
#[derive(Debug, Clone, Copy, Default)]
pub struct SystemClock;

/// The clock cannot fail; the type says so.
#[derive(Debug, thiserror::Error)]
#[error("the clock failed")]
pub enum ClockError {}

impl Clock for SystemClock {
    type Error = ClockError;

    async fn now(&self) -> Result<JudgedAt, ClockError> {
        Ok(JudgedAt::new(jiff::Timestamp::now().as_millisecond()))
    }
}

/// A time a person typed, in one of two forms, and nothing else:
///
/// - an RFC 3339 time with an offset or `Z` (`2026-12-31T00:00:00Z`,
///   `2026-12-31T00:00:00+09:00`, which gives another zone than UTC); a bracketed
///   zone after the offset (`…+09:00[Asia/Tokyo]`) is read and the offset decides;
/// - a date written exactly `YYYY-MM-DD` (`2026-12-31`), the start of that day in
///   UTC.
///
/// A time with no offset (`2026-12-31T23:59:00`, `…[Asia/Tokyo]` alone) is refused:
/// which instant it means depends on a zone it does not say, and reading it as its
/// date alone would drop the time unseen. So is a date in another form
/// (`20261231`).
pub fn parse_time(text: &str) -> Result<JudgedAt, TimeError> {
    let at = match text.parse::<jiff::Timestamp>() {
        Ok(at) => at,
        Err(error) if !is_plain_date(text) => return Err(TimeError::Unreadable(error)),
        Err(_) => text
            .parse::<jiff::civil::Date>()
            .and_then(|date| date.to_zoned(jiff::tz::TimeZone::UTC))
            .map_err(TimeError::NoDay)?
            .timestamp(),
    };
    Ok(JudgedAt::new(at.as_millisecond()))
}

/// Whether a text is written `YYYY-MM-DD`, digits and dashes, whatever the digits.
fn is_plain_date(text: &str) -> bool {
    let bytes = text.as_bytes();
    bytes.len() == 10
        && bytes.iter().enumerate().all(|(i, b)| match i {
            4 | 7 => *b == b'-',
            _ => b.is_ascii_digit(),
        })
}

/// Why a typed time was refused.
#[derive(Debug, thiserror::Error)]
pub enum TimeError {
    /// Neither form: no RFC 3339 time with an offset, and not written `YYYY-MM-DD`.
    #[error(
        "not an RFC 3339 time with an offset or `Z` (`2026-12-31T00:00:00Z`, \
         `2026-12-31T00:00:00+09:00`) nor a date `YYYY-MM-DD`: {0}"
    )]
    Unreadable(#[source] jiff::Error),
    /// Written `YYYY-MM-DD`, but no day of the calendar (`2026-02-30`).
    #[error("not a day of the calendar: {0}")]
    NoDay(#[source] jiff::Error),
}

/// New UUID v7 identities.
#[derive(Debug, Clone, Copy, Default)]
pub struct UuidV7Ids;

/// The domain refused a freshly made identifier (a bug if it ever happens).
#[derive(Debug, thiserror::Error)]
#[error("the domain refused a freshly made UUID v7")]
pub struct IdError(#[source] ValueError);

impl IdSource for UuidV7Ids {
    type Error = IdError;

    async fn new_judgement_id(&self) -> Result<JudgementId, IdError> {
        EntityId::new(Uuid::now_v7())
            .map(JudgementId::new)
            .map_err(IdError)
    }

    async fn new_doc_id(&self) -> Result<DocId, IdError> {
        EntityId::new(Uuid::now_v7())
            .map(DocId::new)
            .map_err(IdError)
    }
}

/// Why no one could be named as the actor.
#[derive(Debug, thiserror::Error)]
pub enum ActorError {
    /// Neither `WHYSHELF_ACTOR` nor Git's `user.name` names anyone.
    #[error("no one to judge as: set WHYSHELF_ACTOR or git config user.name")]
    NoName,
    /// Git could not be run.
    #[error("cannot run git")]
    Git(#[source] std::io::Error),
    /// The name is refused by its rule.
    #[error("`{name}` is not a name of an actor")]
    Refused {
        /// The name.
        name: String,
        /// Why.
        #[source]
        source: ValueError,
    },
}

/// Who acts in a project, when the command line names no one.
///
/// A name given on the command line (`--actor`) wins before this adapter is
/// asked: the application takes it as the actor and never calls the port.
#[derive(Debug, Clone)]
pub struct ProjectActor {
    root: PathBuf,
    declared: Option<String>,
}

impl ProjectActor {
    /// The actor of the project at `root`: `declared` when given (the caller reads
    /// `WHYSHELF_ACTOR`), else Git's `user.name` there.
    pub fn new(root: PathBuf, declared: Option<String>) -> Self {
        Self { root, declared }
    }
}

impl ActorSource for ProjectActor {
    type Error = ActorError;

    async fn actor(&self) -> Result<Actor, ActorError> {
        let name = match self.declared.as_deref().map(str::trim) {
            Some(name) if !name.is_empty() => name.to_owned(),
            _ => {
                let output = tokio::process::Command::new("git")
                    .arg("-C")
                    .arg(&self.root)
                    .args(["config", "--get", "user.name"])
                    .output()
                    .await
                    .map_err(ActorError::Git)?;
                let name = String::from_utf8_lossy(&output.stdout).trim().to_owned();
                if !output.status.success() || name.is_empty() {
                    return Err(ActorError::NoName);
                }
                name
            }
        };
        Actor::new(&name).map_err(|source| ActorError::Refused { name, source })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 2026-11-11T00:00:00Z, in unix milliseconds.
    const NOV_11_UTC: i64 = 1_794_355_200_000;

    fn millis(text: &str) -> i64 {
        parse_time(text)
            .unwrap_or_else(|e| panic!("{text}: {e}"))
            .millis()
    }

    #[test]
    fn a_date_is_the_start_of_that_day_in_utc() {
        assert_eq!(millis("2026-11-11"), NOV_11_UTC);
    }

    #[test]
    fn a_time_with_an_offset_is_that_instant() {
        assert_eq!(millis("2026-11-11T00:00:00Z"), NOV_11_UTC);
        assert_eq!(millis("2026-11-11T09:00:00+09:00"), NOV_11_UTC);
        // Midnight in Tokyo is 15:00 UTC the day before.
        assert_eq!(
            millis("2026-11-11T00:00:00+09:00"),
            NOV_11_UTC - 9 * 3_600_000
        );
        assert_eq!(
            millis("2026-11-11T00:00:00+09:00[Asia/Tokyo]"),
            NOV_11_UTC - 9 * 3_600_000
        );
    }

    #[test]
    fn a_time_with_no_offset_and_a_date_in_another_form_are_refused() {
        for text in [
            "2026-11-11T23:59:00",
            "2026-11-11T23:59",
            "2026-11-11T23:59:00[Asia/Tokyo]",
            "2026-11-11 23:59:00",
            "20261111",
            "2026-1-11",
            "2026-02-30",
            "never",
            "",
        ] {
            assert!(parse_time(text).is_err(), "{text:?} was read");
        }
    }

    #[tokio::test]
    async fn a_declared_actor_wins() {
        let actor = ProjectActor::new(PathBuf::from("."), Some("ana".to_owned()));
        assert_eq!(actor.actor().await.unwrap().as_str(), "ana");
    }

    /// A Git repository in a temporary directory whose own `user.name` is `name`.
    async fn repo_named(name: &str) -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        for args in [&["init", "-q"][..], &["config", "user.name", name][..]] {
            let status = tokio::process::Command::new("git")
                .arg("-C")
                .arg(dir.path())
                .args(args)
                .status()
                .await
                .unwrap();
            assert!(status.success(), "git {args:?}");
        }
        dir
    }

    #[tokio::test]
    async fn the_declared_name_wins_over_gits_user_name() {
        let repo = repo_named("gina").await;
        let actor = ProjectActor::new(repo.path().to_path_buf(), Some("ana".to_owned()));
        assert_eq!(actor.actor().await.unwrap().as_str(), "ana");
    }

    #[tokio::test]
    async fn with_no_declared_name_gits_user_name_is_used() {
        let repo = repo_named("gina").await;
        for declared in [None, Some(String::new()), Some("  ".to_owned())] {
            let actor = ProjectActor::new(repo.path().to_path_buf(), declared.clone());
            assert_eq!(
                actor.actor().await.unwrap().as_str(),
                "gina",
                "declared {declared:?}"
            );
        }
    }

    #[tokio::test]
    async fn no_name_is_read_from_the_projects_files() {
        // Files a committed project could hold, each naming someone else.
        let repo = repo_named("gina").await;
        let whyshelf = repo.path().join(".whyshelf");
        tokio::fs::create_dir_all(&whyshelf).await.unwrap();
        tokio::fs::write(whyshelf.join("actor"), "mallory\n")
            .await
            .unwrap();
        tokio::fs::write(repo.path().join(".env"), "WHYSHELF_ACTOR=mallory\n")
            .await
            .unwrap();
        let actor = ProjectActor::new(repo.path().to_path_buf(), None);
        assert_eq!(actor.actor().await.unwrap().as_str(), "gina");
    }

    #[tokio::test]
    async fn identities_are_v7_and_distinct() {
        let a = UuidV7Ids.new_judgement_id().await.unwrap();
        let b = UuidV7Ids.new_judgement_id().await.unwrap();
        assert_eq!(a.entity().as_uuid().get_version_num(), 7);
        assert_ne!(a, b);
        let d = UuidV7Ids.new_doc_id().await.unwrap();
        let e = UuidV7Ids.new_doc_id().await.unwrap();
        assert_eq!(d.entity().as_uuid().get_version_num(), 7);
        assert_ne!(d, e);
    }
}
