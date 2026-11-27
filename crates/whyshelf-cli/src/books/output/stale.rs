//! The answer of `stale`: the approvals that do not hold, where their degradations
//! start with the commands that can settle each and the approvals the frame refuses,
//! and the lines it writes on stderr.

use schemars::JsonSchema;
use serde::Serialize;
use whyshelf_core::app::shelf::StaleReport;
use whyshelf_core::domain::shelf::freshness::Origin;
use whyshelf_core::domain::shelf::graph::CycleSource;
use whyshelf_core::domain::shelf::untold::untold_causes;

use super::untold::{STALE_UNTOLD, untold_line};
use super::words::{approve_phrase, approve_word};
use super::{
    ArtifactNamed, DocNamed, DocRef, HealthOut, Namer, NextRoot, RootOut, RowOut, UnknownOut,
};

/// One approval that does not hold.
#[derive(Debug, Serialize, JsonSchema)]
pub struct StaleEntryOut {
    /// The document.
    pub row: RowOut,
    /// Where it starts to fail.
    pub roots: Vec<RootOut>,
}

/// The answer of `stale`.
#[derive(Debug, Serialize, JsonSchema)]
pub struct StaleOut {
    /// The approvals that do not hold, strongest first.
    pub stale: Vec<StaleEntryOut>,
    /// Where they start: each document, artifact, cycle or the declaration an event
    /// starts at, with the approvals of `stale` it degrades; the one that degrades
    /// the most first.
    pub origins: Vec<OriginOut>,
    /// The names approved documents pin through a `depends` link that their
    /// adapter cannot tell about now: their freshness is unchanged, whether their
    /// approvals hold cannot be said, and `--exit-code` exits 1 for any.
    pub unknown: Vec<UnknownOut>,
    /// What is wrong around the documents.
    pub health: HealthOut,
}

/// Where the degradations of approvals start, with what they degrade and what can
/// settle them.
#[derive(Debug, Serialize, JsonSchema)]
pub struct OriginOut {
    /// Where it starts.
    pub start: StartOut,
    /// The approvals it degrades, each also an entry of `stale`, by identity.
    pub degraded: Vec<DocRef>,
    /// The commands that can settle it, as they are typed, in the order a person
    /// would try them: read what changed, then declare, then approve. Each is for a
    /// person to run (whyshelf runs none) and to complete: what a person writes
    /// (a reason, a time, a new name) is a word in `<...>`. whyshelf does not guess
    /// what an edit means, so none of them is said for you. Each carries the `--root`
    /// the run had; empty for an origin nothing can be typed for, and when that root
    /// is no UTF-8 text. What settles what is in the manual, under "What settles an
    /// origin". An approval the frame refuses is not here: it is in `withheld`.
    pub next: Vec<String>,
    /// The approvals that would settle it and are not offered in `next`, because the
    /// frame refuses them now, each with why; in the order of the list `next` has.
    /// Empty when the frame refuses none, and whatever the `--root`.
    pub withheld: Vec<WithheldOut>,
}

/// An approval that would settle the origin and is not offered in `next`, because the
/// frame refuses it now. It is told, not offered: no command is written for it.
#[derive(Debug, Serialize, JsonSchema)]
pub struct WithheldOut {
    /// The document the approval would be of.
    pub doc: DocRef,
    /// Which approval: `pending` (of a pending upstream), `as_another` (by a name that
    /// has not approved it) or `until` (again, with a new expiry).
    pub approve: &'static str,
    /// Why the frame refuses it: the sentence `judge approve --stale` gives for a
    /// document it leaves out as refused.
    pub detail: String,
}

/// The thing an origin is.
#[derive(Debug, Serialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum StartOut {
    /// A document (its key is null when no valid document has it now).
    Doc(DocNamed),
    /// An artifact.
    Artifact(ArtifactNamed),
    /// The declaration of the books: a link to artifacts declares another comparison.
    Declaration,
    /// A cycle (see `health.cycles`).
    Cycle {
        /// `links` or `frames`.
        among: &'static str,
        /// Its smallest member, by identity.
        smallest: DocRef,
    },
}

/// The lines `stale` writes on stderr: one per cause that stands behind names an
/// approval stands on that cannot be told now, in the order the causes first
/// appear in `unknown`, each naming the approving document or documents. The rule
/// that gathers is [`untold_causes`]; the answer on stdout keeps every name. Then one
/// per approval the frame refuses (`withheld`), in the order of the origins and, in
/// each, of its `withheld`, as `judge approve --stale` says a document it leaves out
/// as refused, with the kind of approval said.
pub fn stale_warnings(report: &StaleReport) -> Vec<String> {
    let namer = Namer(&report.names);
    let withheld = report.origins.iter().flat_map(|origin| &origin.withheld);
    untold_causes(&report.unknown)
        .into_iter()
        .map(|cause| untold_line(&cause, &STALE_UNTOLD))
        .chain(withheld.map(|withheld| {
            let doc = namer.doc(withheld.approval.doc());
            format!(
                "warning: {} is not offered an approval {}: {}",
                doc.key.unwrap_or(doc.id),
                approve_phrase(&withheld.approval),
                withheld.why
            )
        }))
        .collect()
}

/// The answer of `stale`. `root` is the `--root` the run had, which each command of
/// an origin's `next` carries.
pub fn stale(report: &StaleReport, root: NextRoot<'_>) -> StaleOut {
    let namer = Namer(&report.names);
    StaleOut {
        stale: report
            .entries
            .iter()
            .map(|e| StaleEntryOut {
                row: namer.row(&e.row),
                roots: e.roots.iter().map(|r| namer.root(r)).collect(),
            })
            .collect(),
        origins: report
            .origins
            .iter()
            .map(|settled| OriginOut {
                next: super::next::commands(&settled.next, &report.names, root),
                withheld: settled
                    .withheld
                    .iter()
                    .map(|withheld| WithheldOut {
                        doc: namer.doc(withheld.approval.doc()),
                        approve: approve_word(&withheld.approval),
                        detail: withheld.why.to_string(),
                    })
                    .collect(),
                start: match &settled.group.origin {
                    Origin::Doc(id) => StartOut::Doc(DocNamed {
                        doc: namer.doc(*id),
                    }),
                    Origin::Artifact(artifact) => StartOut::Artifact(ArtifactNamed::of(artifact)),
                    Origin::Declaration => StartOut::Declaration,
                    Origin::Cycle(cycle) => StartOut::Cycle {
                        among: match cycle.source {
                            CycleSource::Links => "links",
                            CycleSource::Frames => "frames",
                        },
                        smallest: namer.doc(cycle.min),
                    },
                },
                degraded: settled
                    .group
                    .degraded
                    .iter()
                    .map(|id| namer.doc(*id))
                    .collect(),
            })
            .collect(),
        unknown: report
            .unknown
            .iter()
            .map(|u| UnknownOut {
                doc: DocRef {
                    id: u.doc.to_string(),
                    key: u.key.as_ref().map(ToString::to_string),
                },
                link: u.link.to_string(),
                name: u.artifact.name().to_string(),
                space: u.artifact.space().to_string(),
                why: u.why.to_string(),
            })
            .collect(),
        health: namer.health(&report.health),
    }
}
