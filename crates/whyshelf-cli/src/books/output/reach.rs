//! The answer of `reach` ([`ReachOut`]): the documents named and every document that
//! depends on them, the artifacts all of them stand on with where each lives, and the
//! files of the project all of it is in; and the warning of what the walk could not
//! follow ([`reach_warnings`]), the one place its sentence is written.

use std::fmt;

use schemars::JsonSchema;
use serde::Serialize;
use whyshelf_core::app::shelf::{ReachReport, ReachedArtifact, ReachedDoc};
use whyshelf_core::domain::location::RelPath;
use whyshelf_core::domain::nonempty::NonEmptySet;
use whyshelf_core::domain::shelf::placement::Placement;
use whyshelf_core::domain::shelf::reach::Unwalked;

use super::words::unread;
use super::{DocRef, HealthOut, Namer, standing};

/// One document reached.
#[derive(Debug, Serialize, JsonSchema)]
pub struct ReachDocOut {
    /// The document.
    pub doc: DocRef,
    /// Whether it was named; one not named depends on one that was, transitively.
    pub named: bool,
    /// Where it stands now, in the words of `status`.
    pub standing: &'static str,
    /// The files it may be in: its file, the files of an invalid one, or every file
    /// whose identity cannot be read when it may be in any of them; none when it is
    /// gone.
    pub files: Vec<String>,
}

/// Where an artifact lives in the files of the project, as the adapter of its space
/// says.
#[derive(Debug, Serialize, JsonSchema)]
#[serde(tag = "place", rename_all = "snake_case")]
pub enum PlaceOut {
    /// The thing it names is these files, whole.
    Only {
        /// The files, as paths of the project.
        files: Vec<String>,
    },
    /// These files hold part of the thing it names; the rest is outside the files of
    /// the project (the docs of a Rust item are made by the build).
    Partly {
        /// The files, as paths of the project.
        files: Vec<String>,
    },
    /// The adapter cannot say where it lives.
    Unplaced {
        /// Why.
        why: String,
    },
}

/// One artifact the documents reached stand on.
#[derive(Debug, Serialize, JsonSchema)]
pub struct ReachArtifactOut {
    /// Its space.
    pub space: String,
    /// Its name.
    pub name: String,
    /// The documents reached that stand on it.
    pub by: Vec<DocRef>,
    /// Where it lives.
    pub place: PlaceOut,
}

/// An artifact whose place cannot be told.
#[derive(Debug, Serialize, JsonSchema)]
pub struct UnplacedOut {
    /// Its space.
    pub space: String,
    /// Its name.
    pub name: String,
    /// Why.
    pub why: String,
}

/// What may depend on the named and could not be read, counted. At least one count is
/// more than none.
#[derive(Debug, Serialize, JsonSchema)]
pub struct UntoldOut {
    /// The documents not reached whose links cannot be read (their files are not
    /// valid).
    pub invalid: usize,
    /// The files whose identity cannot be read: each may be any document.
    pub unreadable: usize,
}

/// The answer of `reach`.
#[derive(Debug, Serialize, JsonSchema)]
pub struct ReachOut {
    /// The documents named, and every document whose `Depends` links reach one of
    /// them, transitively.
    pub docs: Vec<ReachDocOut>,
    /// The artifacts they stand on (their `Depends` links to artifacts now, and the
    /// pins of the approval that stands about each), by space and name.
    pub artifacts: Vec<ReachArtifactOut>,
    /// The files of the project all of it is in: those of the documents and those of
    /// the places of the artifacts, whole or in part, each once.
    pub files: Vec<String>,
    /// The artifacts whose place cannot be told, with why.
    pub unplaced: Vec<UnplacedOut>,
    /// The documents reached that are gone: no file carries them.
    pub gone: Vec<DocRef>,
    /// What may depend on the named and could not be read, counted; left out when
    /// nothing may. When it is there, the reach may be more than these documents.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub untold: Option<UntoldOut>,
    /// What is wrong around the documents.
    pub health: HealthOut,
}

impl Namer<'_> {
    fn reached_doc(&self, reached: &ReachedDoc) -> ReachDocOut {
        ReachDocOut {
            doc: self.doc(reached.row.id),
            named: reached.named,
            standing: standing(reached.row.standing.kind()),
            files: reached.files.iter().map(ToString::to_string).collect(),
        }
    }

    fn reached_artifact(&self, reached: &ReachedArtifact) -> ReachArtifactOut {
        ReachArtifactOut {
            space: reached.artifact.space().to_string(),
            name: reached.artifact.name().to_string(),
            by: reached.by.iter().map(|id| self.doc(*id)).collect(),
            place: PlaceOut::from(&reached.placement),
        }
    }
}

impl From<&Placement> for PlaceOut {
    /// Where an artifact lives, as every answer that says so writes it.
    fn from(placement: &Placement) -> Self {
        let files = |files: &NonEmptySet<RelPath>| files.iter().map(ToString::to_string).collect();
        match placement {
            Placement::Only(only) => Self::Only { files: files(only) },
            Placement::Partly(part) => Self::Partly { files: files(part) },
            Placement::Unplaced(why) => Self::Unplaced {
                why: why.as_str().to_owned(),
            },
        }
    }
}

/// The answer of `reach`.
pub fn reach(report: &ReachReport) -> ReachOut {
    let namer = Namer(&report.names);
    ReachOut {
        docs: report.docs.iter().map(|d| namer.reached_doc(d)).collect(),
        artifacts: report
            .artifacts
            .iter()
            .map(|a| namer.reached_artifact(a))
            .collect(),
        files: report.files.iter().map(ToString::to_string).collect(),
        unplaced: report
            .unplaced()
            .map(|(artifact, why)| UnplacedOut {
                space: artifact.space().to_string(),
                name: artifact.name().to_string(),
                why: why.as_str().to_owned(),
            })
            .collect(),
        gone: report.gone.iter().map(|id| namer.doc(*id)).collect(),
        untold: report.untold.map(UntoldOut::from),
        health: namer.health(&report.health),
    }
}

/// What a walk could not follow, counted: the one place it is answered, for `reach`
/// and the reach before a change of `impact --within`.
impl From<Unwalked> for UntoldOut {
    fn from(untold: Unwalked) -> Self {
        Self {
            invalid: untold.invalid(),
            unreadable: untold.unreadable(),
        }
    }
}

/// The warning lines of `reach`, for stderr: one when the walk could not follow
/// everything, in the words of [`Untold`].
pub fn reach_warnings(report: &ReachReport) -> Vec<String> {
    report
        .untold
        .iter()
        .map(|untold| format!("warning: {}", Untold(*untold)))
        .collect()
}

/// What a walk could not follow, as one sentence: the one place it is written. A count
/// of none is not said.
struct Untold(Unwalked);

impl fmt::Display for Untold {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{} may depend on the named, so what they reach may be more than this answer says",
            unread(self.0.invalid(), self.0.unreadable())
        )
    }
}
