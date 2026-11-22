//! A document as every answer shows it: its row ([`RowOut`]: what the shelf has for
//! it, what people decided and where it stands now), where the failure of its approval
//! starts, what a link of it names, and what is wrong around the documents
//! ([`HealthOut`]).

use schemars::JsonSchema;
use serde::Serialize;
use std::collections::BTreeSet;

use whyshelf_core::app::shelf::{Health, Row};
use whyshelf_core::domain::actor::Actor;
use whyshelf_core::domain::shelf::artifact::Artifact;
use whyshelf_core::domain::shelf::build::DocPresence;
use whyshelf_core::domain::shelf::doc::LinkState;
use whyshelf_core::domain::shelf::freshness::{
    Against, Freshness, RootCause, Standing, StandingKind, Successors,
};
use whyshelf_core::domain::shelf::ledger::{DeclaredStatus, Quorum};

use super::words::{ran_by, state_word};
use super::{CauseOut, DocRef, Namer, NoticeOut, readers, standing};

/// What the shelf has for a document now.
#[derive(Debug, Serialize, JsonSchema)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum PresenceOut {
    /// A valid document.
    Valid {
        /// Its file.
        file: String,
        /// Its hash now.
        hash: String,
    },
    /// The files that carry the identity are invalid.
    Invalid {
        /// The files.
        files: Vec<String>,
        /// What is wrong.
        problems: Vec<String>,
    },
    /// No file carries it.
    Gone,
    /// Unknown: some file's identity could not be read.
    Unknown {
        /// The files whose identity could not be read.
        unreadable: Vec<String>,
    },
}

/// Who made a decision, when and why, as its record keeps it.
#[derive(Debug, Serialize, JsonSchema)]
pub struct DecisionOut {
    /// The record's id (`judge withdraw` takes it).
    pub judgement: String,
    /// Who decided.
    pub actor: String,
    /// Who ran the command, when not the actor; null when the actor ran it.
    pub ran_by: Option<String>,
    /// When (unix milliseconds).
    pub at: i64,
    /// Why, as it was given; none when no reason was given.
    pub reason: Option<String>,
}

/// A document as every answer shows it.
#[derive(Debug, Serialize, JsonSchema)]
pub struct RowOut {
    /// The document.
    pub doc: DocRef,
    /// What the shelf has for it.
    pub presence: PresenceOut,
    /// What people decided: `pending`, `approved`, `denied`, `deprecated`, `retired`.
    pub declared: &'static str,
    /// The decision that stands, the one `declared` is read from; none while the
    /// document is pending.
    pub decision: Option<DecisionOut>,
    /// When the approval lapses (unix milliseconds), if it does.
    pub expires: Option<i64>,
    /// Where it stands now: `pending`, `denied`, `deprecated`, `retired`, or for an
    /// approval `fresh`, `awaiting` (fresh, and awaiting the approvals of other
    /// names), `expired`, `stale`, `broken`.
    pub standing: &'static str,
    /// For an approval whose content fewer different names read than its rule of who
    /// may decide asks for, at any freshness, who read it and how many are asked for;
    /// left out otherwise, as it always is where no rule asks for more than one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub awaiting: Option<AwaitingOut>,
    /// For an approval, at any freshness and in every project, the names that read
    /// the content it stands on now; left out for a document with no approval.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub readers: Option<ReadersOut>,
    /// For a deprecated document, what replaces it.
    pub successors: Vec<DocRef>,
    /// Why an approval does not hold.
    pub causes: Vec<CauseOut>,
    /// What is worth knowing that changes no freshness.
    pub notices: Vec<NoticeOut>,
}

/// Who read the content of an approval that awaits the approvals of other names.
#[derive(Debug, Serialize, JsonSchema)]
pub struct AwaitingOut {
    /// The names whose reading counts now, each once; when every reading lapsed, the
    /// name of the approval that stands alone, which the row's `readers.counted` does
    /// not have.
    pub readers: Vec<String>,
    /// How many different names the rule asks for.
    pub need: usize,
}

impl AwaitingOut {
    /// The quorum of `standing` when it is short; none otherwise.
    pub(super) fn of(standing: &Standing) -> Option<Self> {
        match standing.quorum()? {
            Quorum::Met => None,
            Quorum::Short { readers, need } => Some(Self {
                readers: readers.iter().map(|a| a.as_str().to_owned()).collect(),
                need: need.count(),
            }),
        }
    }
}

/// Who read the content an approval stands on, as `status --readers` asks it.
#[derive(Debug, Serialize, JsonSchema)]
pub struct ReadersOut {
    /// How many names count, as the word of `ReaderCount` (`output::readers`).
    pub count: &'static str,
    /// The names whose own latest reading has not lapsed, each once.
    pub counted: Vec<String>,
    /// The names whose own latest reading has lapsed, each once; they are not counted.
    pub lapsed: Vec<String>,
}

impl ReadersOut {
    /// The readers of `standing` when it is an approval; none otherwise.
    pub(super) fn of(standing: &Standing) -> Option<Self> {
        let read_by = standing.read_by()?;
        let names = |set: &BTreeSet<Actor>| set.iter().map(|a| a.as_str().to_owned()).collect();
        Some(Self {
            count: readers(read_by.count()),
            counted: names(read_by.counted()),
            lapsed: names(read_by.lapsed()),
        })
    }
}

/// A file that is no valid document.
#[derive(Debug, Serialize, JsonSchema)]
pub struct InvalidOut {
    /// The file.
    pub file: String,
    /// Its identity, if it could be read.
    pub id: Option<String>,
    /// What is wrong.
    pub problems: Vec<String>,
}

/// A file of the ledger that is no record.
#[derive(Debug, Serialize, JsonSchema)]
pub struct UnreadableOut {
    /// The file.
    pub file: String,
    /// Why.
    pub why: String,
}

/// What is wrong around the documents.
#[derive(Debug, Serialize, JsonSchema)]
pub struct HealthOut {
    /// Whether nothing is.
    pub clean: bool,
    /// Files that are no valid document; those whose front matter or identity
    /// could not be read follow the order the declaration writes the books in.
    pub invalid: Vec<InvalidOut>,
    /// Cycles among links or among the frames of approvals.
    pub cycles: Vec<Vec<DocRef>>,
    /// Records of the ledger that contradict one another.
    pub ledger_problems: Vec<String>,
    /// Files of the ledger that are no record.
    pub unreadable_records: Vec<UnreadableOut>,
}

/// Where the failure of an approval starts.
#[derive(Debug, Serialize, JsonSchema)]
pub struct RootOut {
    /// The documents from the one asked about to the one where it starts.
    pub path: Vec<DocRef>,
    /// What happened there.
    pub cause: CauseOut,
}

/// A link of a document, with what it names.
#[derive(Debug, Serialize, JsonSchema)]
pub struct LinkOut {
    /// The name of the link.
    pub name: String,
    /// What it names: documents, or keys that cannot be followed.
    pub targets: Vec<LinkTargetOut>,
}

/// What one key of a link names.
#[derive(Debug, Serialize, JsonSchema)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum LinkTargetOut {
    /// A document.
    Resolved {
        /// The document.
        doc: DocRef,
    },
    /// A key that cannot be followed.
    Unresolved {
        /// The key as written.
        written: String,
        /// Why.
        why: String,
    },
    /// An artifact (a link with `target = "artifact"`).
    Artifact {
        /// Its name (kept from when every name was a path; the same as `name`).
        path: String,
        /// Its name as written.
        name: String,
        /// Its space.
        space: String,
        /// What it is now: `present`, `absent`, or `unknown` (its adapter cannot tell
        /// now).
        now: &'static str,
        /// The hash of its content now, when `now` is `present`; none otherwise.
        hash: Option<String>,
    },
}

impl Namer<'_> {
    pub(super) fn row(&self, row: &Row) -> RowOut {
        let (declared, expires) = declared(&row.declared);
        let (successors, causes) = match &row.standing {
            Standing::Against(Against::Deprecated { successors }) => {
                (self.successors(successors), Vec::new())
            }
            Standing::Approved {
                freshness: Freshness::Degraded(degradation),
                ..
            } => (
                Vec::new(),
                degradation.causes().iter().map(|c| self.cause(c)).collect(),
            ),
            _ => (Vec::new(), Vec::new()),
        };
        RowOut {
            doc: self.doc(row.id),
            presence: presence(&row.presence),
            declared,
            decision: row.decision.as_ref().map(|d| DecisionOut {
                judgement: d.id().to_string(),
                actor: d.actor().as_str().to_owned(),
                ran_by: ran_by(d.attribution()),
                at: d.at().millis(),
                reason: d.reason().map(|r| r.as_str().to_owned()),
            }),
            expires,
            standing: standing(row.standing.kind()),
            awaiting: AwaitingOut::of(&row.standing),
            readers: ReadersOut::of(&row.standing),
            successors,
            causes,
            notices: row.notices.iter().map(|n| self.notice(n)).collect(),
        }
    }

    pub(super) fn successors(&self, successors: &Successors) -> Vec<DocRef> {
        match successors {
            Successors::None => Vec::new(),
            Successors::Some(ids) => ids.iter().map(|id| self.doc(*id)).collect(),
        }
    }

    pub(super) fn root(&self, root: &RootCause) -> RootOut {
        RootOut {
            path: root.path.iter().map(|id| self.doc(*id)).collect(),
            cause: self.cause(&root.cause),
        }
    }

    pub(super) fn health(&self, health: &Health) -> HealthOut {
        HealthOut {
            clean: health.is_clean(),
            invalid: health
                .invalid
                .iter()
                .map(|d| InvalidOut {
                    file: d.origin().to_string(),
                    id: d.id().map(|id| id.to_string()),
                    problems: d.problems().iter().map(ToString::to_string).collect(),
                })
                .collect(),
            cycles: health
                .cycles
                .iter()
                .map(|c| c.ids().into_iter().map(|id| self.doc(id)).collect())
                .collect(),
            ledger_problems: health
                .ledger_problems
                .iter()
                .map(ToString::to_string)
                .collect(),
            unreadable_records: health
                .unreadable_records
                .iter()
                .map(|u| UnreadableOut {
                    file: u.file.to_string(),
                    why: u.why.to_string(),
                })
                .collect(),
        }
    }

    pub(super) fn target(&self, state: &LinkState) -> LinkTargetOut {
        match state {
            LinkState::Resolved(target) => LinkTargetOut::Resolved {
                doc: self.doc(target.id()),
            },
            LinkState::Unresolved(u) => LinkTargetOut::Unresolved {
                written: u.written().to_owned(),
                why: u.to_string(),
            },
        }
    }
}

fn presence(presence: &DocPresence) -> PresenceOut {
    match presence {
        DocPresence::Valid { origin, hash, .. } => PresenceOut::Valid {
            file: origin.to_string(),
            hash: hash.to_string(),
        },
        DocPresence::Invalid { files, problems } => PresenceOut::Invalid {
            files: files.iter().map(ToString::to_string).collect(),
            problems: problems.iter().map(ToString::to_string).collect(),
        },
        DocPresence::Gone => PresenceOut::Gone,
        DocPresence::Unknown { unreadable } => PresenceOut::Unknown {
            unreadable: unreadable.iter().map(ToString::to_string).collect(),
        },
    }
}

fn declared(status: &DeclaredStatus) -> (&'static str, Option<i64>) {
    match status {
        DeclaredStatus::Pending => ("pending", None),
        DeclaredStatus::Approved { expires, .. } => ("approved", expires.map(|e| e.millis())),
        // A decision against it is told in the word of its standing, written once.
        DeclaredStatus::Against { status, .. } => (standing(StandingKind::from(*status)), None),
    }
}

pub(super) fn artifact_target(artifact: &Artifact) -> LinkTargetOut {
    LinkTargetOut::Artifact {
        path: artifact.name().to_string(),
        name: artifact.name().to_string(),
        space: artifact.space().to_string(),
        now: state_word(artifact.state()),
        hash: artifact.state().hash().map(ToString::to_string),
    }
}
