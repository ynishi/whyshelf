//! The answer of `units` ([`UnitsOut`]): what does not hold, divided into units of work
//! that share nothing, by the rules on the page of `whyshelf_core::domain::shelf::units`;
//! and the warning of what could join units without being seen ([`units_warnings`]).

use schemars::JsonSchema;
use serde::Serialize;
use whyshelf_core::app::shelf::UnitsReport;
use whyshelf_core::domain::shelf::units::{Unit, UnitArtifact, UnitDoc, UnplacedArtifact};

use super::reach::PlaceOut;
use super::words::{hold_word, unread};
use super::{DocRef, HealthOut, Namer, standing};

/// One document of a unit.
#[derive(Debug, Serialize, JsonSchema)]
pub struct UnitDocOut {
    /// The document.
    pub doc: DocRef,
    /// Where it stands now, in the words of `status`: an approval that no longer holds,
    /// or `pending`.
    pub standing: &'static str,
}

/// An artifact the documents of a unit stand on.
#[derive(Debug, Serialize, JsonSchema)]
pub struct UnitArtifactOut {
    /// Its space.
    pub space: String,
    /// Its name.
    pub name: String,
    /// The documents of the unit that stand on it.
    pub documents: Vec<DocRef>,
    /// Where it lives, as `reach` writes it.
    pub place: PlaceOut,
}

/// An upstream the unit waits on, and why: one outside the documents divided that holds
/// a document of the unit back.
#[derive(Debug, Serialize, JsonSchema)]
pub struct WaitOut {
    /// The upstream.
    pub doc: DocRef,
    /// Why it holds them back, a word of the type of `judge approve`'s `upstream_is`;
    /// here only `pending` (an upstream not divided, without `--pending`) or `awaiting`
    /// (an approval that awaits the approvals of other names). It is the same for every
    /// document of the unit it holds back.
    pub hold: &'static str,
}

/// One unit: documents that can be mended and approved again apart from every other
/// unit.
#[derive(Debug, Serialize, JsonSchema)]
pub struct UnitOut {
    /// Its documents, in the order a run of approvals takes them: a document comes after
    /// the documents of the unit it stands on. A link that only names a document (one
    /// that is not valid, or a key two files write) joins the two and gives no order, so
    /// the one it names is not always first. Nor does an order pass through the chain a
    /// unit waits on: a document that waits on an upstream which stands on another
    /// document of the unit can come before that one.
    pub documents: Vec<UnitDocOut>,
    /// The files its artifacts live in, whole or in part, by path. The files of its
    /// documents and of the declaration are not counted, so two units may share the
    /// file of the declaration. Nor is the file of an artifact of a document whose
    /// approval holds, which is no key (the known limitations of
    /// `whyshelf_core::domain::shelf::units` say what that leaves).
    pub files: Vec<String>,
    /// The artifacts its documents stand on, by space and name.
    pub artifacts: Vec<UnitArtifactOut>,
    /// The upstreams outside the documents divided that hold one of them back, by
    /// identity, each once. What those wait on in turn joins the unit too: the
    /// documents of that chain, their artifacts and their files are shared with no
    /// other unit, and are not listed.
    pub waits_on: Vec<WaitOut>,
}

/// What could join units without the division seeing it, counted. At least one count is
/// more than none.
#[derive(Debug, Serialize, JsonSchema)]
pub struct UnjoinedOut {
    /// The documents whose links cannot be read (their files are not valid) that are
    /// divided, or stood on or named by a divided document or one of the chain.
    pub invalid: usize,
    /// The files whose identity cannot be read: each may be any document.
    pub unreadable: usize,
}

/// An artifact whose place cannot be told.
#[derive(Debug, Serialize, JsonSchema)]
pub struct UnitsUnplacedOut {
    /// Its space.
    pub space: String,
    /// Its name.
    pub name: String,
    /// Why its place cannot be told.
    pub why: String,
    /// The documents that stand on it.
    pub documents: Vec<DocRef>,
}

/// The answer of `units`.
#[derive(Debug, Serialize, JsonSchema)]
pub struct UnitsOut {
    /// The units, by the least identity of their documents. Two units share no file an
    /// artifact lives in, no document, and no upstream they wait on; of an artifact in
    /// `unplaced`, only that no other unit stands on the same name is said.
    pub units: Vec<UnitOut>,
    /// The artifacts whose place cannot be told, by space and name, each with why and the
    /// documents that stand on it: those the documents divided stand on, and those of
    /// every document of a chain, deeper ones included, which are neither documents of a
    /// unit nor in its `waits_on`.
    pub unplaced: Vec<UnitsUnplacedOut>,
    /// What could join units without being seen, counted; left out when nothing could.
    /// When it is there, two units may share what the division cannot tell.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub untold: Option<UnjoinedOut>,
    /// What is wrong around the documents.
    pub health: HealthOut,
}

impl Namer<'_> {
    fn unit_doc(&self, doc: &UnitDoc) -> UnitDocOut {
        UnitDocOut {
            doc: self.doc(doc.id()),
            standing: standing(doc.standing()),
        }
    }

    fn unit_artifact(&self, artifact: &UnitArtifact) -> UnitArtifactOut {
        UnitArtifactOut {
            space: artifact.artifact().space().to_string(),
            name: artifact.artifact().name().to_string(),
            documents: artifact
                .documents()
                .iter()
                .map(|id| self.doc(*id))
                .collect(),
            place: PlaceOut::from(artifact.placement()),
        }
    }

    fn unit(&self, unit: &Unit) -> UnitOut {
        UnitOut {
            documents: unit.documents().iter().map(|d| self.unit_doc(d)).collect(),
            files: unit.files().iter().map(ToString::to_string).collect(),
            artifacts: unit
                .artifacts()
                .iter()
                .map(|a| self.unit_artifact(a))
                .collect(),
            waits_on: unit
                .waits_on()
                .iter()
                .map(|(upstream, hold)| WaitOut {
                    doc: self.doc(*upstream),
                    hold: hold_word(*hold),
                })
                .collect(),
        }
    }

    fn units_unplaced(&self, unplaced: &UnplacedArtifact) -> UnitsUnplacedOut {
        UnitsUnplacedOut {
            space: unplaced.artifact().space().to_string(),
            name: unplaced.artifact().name().to_string(),
            why: unplaced.why().as_str().to_owned(),
            documents: unplaced
                .documents()
                .iter()
                .map(|id| self.doc(*id))
                .collect(),
        }
    }
}

/// The answer of `units`.
pub fn units(report: &UnitsReport) -> UnitsOut {
    let namer = Namer(&report.names);
    UnitsOut {
        units: report.units.units().iter().map(|u| namer.unit(u)).collect(),
        unplaced: report
            .units
            .unplaced()
            .iter()
            .map(|u| namer.units_unplaced(u))
            .collect(),
        untold: report.units.untold().map(|u| UnjoinedOut {
            invalid: u.invalid(),
            unreadable: u.unreadable(),
        }),
        health: namer.health(&report.health),
    }
}

/// The warning lines of `units`, for stderr: one when something could join units
/// without being seen, its counts in the words `reach` says them in.
pub fn units_warnings(report: &UnitsReport) -> Vec<String> {
    report
        .units
        .untold()
        .iter()
        .map(|u| {
            format!(
                "warning: {} may join units told apart",
                unread(u.invalid(), u.unreadable())
            )
        })
        .collect()
}
