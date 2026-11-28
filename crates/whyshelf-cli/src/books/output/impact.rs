//! The answer of `impact` ([`ImpactOut`]): how the two ends of a change were read,
//! and for every document whether its approval held before, holds after, and whether
//! the change adds a cause that makes it not hold; with `--within`, the gate's
//! verdict of every document ([`WithinOut`]).

use schemars::JsonSchema;
use serde::Serialize;
use whyshelf_core::app::shelf::ImpactReport;
use whyshelf_core::domain::id::DocId;
use whyshelf_core::domain::location::RelPath;
use whyshelf_core::domain::nonempty::NonEmptySet;
use whyshelf_core::domain::shelf::build::MayHold;
use whyshelf_core::domain::shelf::freshness::{Holding, RootCause, UntoldPin};
use whyshelf_core::domain::shelf::impact::{ImpactEntry, Made};
use whyshelf_core::domain::shelf::within::{
    Away, ClaimUntold, InsideClaim, UnprovenWhy, VerdictKind, Within, WithinEntry, Witness,
};

use super::reach::UntoldOut;
use super::words::{holding_level, holding_word, inside_word, made_word, sides_word, witness_word};
use super::{DocRef, HealthOut, Namer, RootOut};

/// A name an approval stands on that its adapter could not tell about.
#[derive(Debug, Serialize, JsonSchema)]
pub struct UntoldPinOut {
    /// The link.
    pub link: String,
    /// The space of the name.
    pub space: String,
    /// The name as written.
    pub name: String,
    /// Why, as the adapter says.
    pub why: String,
}

/// Whether the approval of a document holds at one end, as `stale` reads it.
#[derive(Debug, Serialize, JsonSchema)]
pub struct HoldingOut {
    /// `not_there` (no file and no record at that end), `pending`, `against` (a
    /// decision against it stands), `holds`, `cannot_tell` (it holds as far as can be
    /// told, and stands on names that cannot be told) or `does_not_hold`.
    pub holding: &'static str,
    /// For `holds` and `cannot_tell`, `fresh` or `awaiting`; for `against`, the
    /// decision (`denied`, `deprecated`, `retired`); for `does_not_hold`, how strongly
    /// (`expired`, `stale`, `broken`); otherwise null.
    pub level: Option<&'static str>,
    /// For `does_not_hold`, where it starts.
    pub roots: Vec<RootOut>,
    /// The names it stands on that cannot be told.
    pub untold: Vec<UntoldPinOut>,
}

/// Whether the change adds a cause that makes the approval not hold.
#[derive(Debug, Serialize, JsonSchema)]
pub struct MadeOut {
    /// `yes`, `no` or `cannot_tell`.
    pub made: &'static str,
    /// For `cannot_tell`, which end could not tell: `before`, `after` or `both`;
    /// otherwise null.
    pub side: Option<&'static str>,
    /// For `yes`, the causes added that stand on what the end before could tell: where
    /// it starts.
    pub certain: Vec<RootOut>,
    /// The causes added that stand on a name the end before could not tell, which may
    /// have been there before the change.
    pub uncertain: Vec<RootOut>,
    /// For `cannot_tell`, the names that could not be told.
    pub untold: Vec<UntoldPinOut>,
}

/// One document.
#[derive(Debug, Serialize, JsonSchema)]
pub struct ImpactDocOut {
    /// The document, by the key it has after the change (before it, when it has none
    /// after).
    pub doc: DocRef,
    /// Its approval before the change.
    pub before: HoldingOut,
    /// Its approval after the change.
    pub after: HoldingOut,
    /// Whether the two differ.
    pub changed: bool,
    /// Whether the change adds a cause that makes it not hold.
    pub made: MadeOut,
}

/// The answer of `impact`.
#[derive(Debug, Serialize, JsonSchema)]
pub struct ImpactOut {
    /// How the two ends were read, as asked (`git: <since> -> <until>`, `git: <since>
    /// -> the index`, `files: <before> -> <after>`).
    pub way: String,
    /// Every document either end lists, by identity.
    pub docs: Vec<ImpactDocOut>,
    /// What is wrong around the documents before the change.
    pub health_before: HealthOut,
    /// What is wrong around them after it.
    pub health_after: HealthOut,
    /// With `--within`, whether the change stays inside the reach of the documents
    /// named; otherwise null.
    pub within: Option<WithinOut>,
}

/// Whether a change stays inside the reach of the documents named, as the project
/// stood before it.
#[derive(Debug, Serialize, JsonSchema)]
pub struct WithinOut {
    /// The documents named.
    pub named: Vec<DocRef>,
    /// Their reach before the change.
    pub reach_before: ReachBeforeOut,
    /// Every document `docs` of the answer lists, with its verdict.
    pub docs: Vec<WithinDocOut>,
    /// What cannot be read at either end, which keeps the gate from passing; null when
    /// everything can be read.
    pub unreadable: Option<UnreadableOut>,
    /// Whether the gate passes: the rule is on the page of `within` in the library.
    pub passes: bool,
}

/// What cannot be read at the two ends of a change: each may be a document, or a
/// record about one, that the change touched.
#[derive(Debug, Serialize, JsonSchema)]
pub struct UnreadableOut {
    /// The files of the books before the change whose identity cannot be read.
    pub files_before: Vec<String>,
    /// The files of the books after the change whose identity cannot be read.
    pub files_after: Vec<String>,
    /// The files of the books before the change left in a merge conflict whose sides
    /// show different identities.
    pub sided_before: Vec<String>,
    /// The files of the books after the change left in a merge conflict whose sides
    /// show different identities.
    pub sided_after: Vec<String>,
    /// The files of the ledger before the change that are no record.
    pub records_before: Vec<String>,
    /// The files of the ledger after the change that are no record.
    pub records_after: Vec<String>,
}

/// The reach of the documents named, on the shelf before the change.
#[derive(Debug, Serialize, JsonSchema)]
pub struct ReachBeforeOut {
    /// The documents named, and every document whose `Depends` links reach one of
    /// them, transitively.
    pub reached: Vec<DocRef>,
    /// What may depend on the named and could not be read, counted; null when nothing
    /// may.
    pub untold: Option<UntoldOut>,
}

/// One document's verdict.
#[derive(Debug, Serialize, JsonSchema)]
pub struct WithinDocOut {
    /// The document, by the key it has after the change (before it, when it has none
    /// after).
    pub doc: DocRef,
    /// Whether the verdict passes.
    pub passes: bool,
    /// The verdict, with what it carries; the table is on the page of `within` in the
    /// library.
    pub verdict: VerdictOut,
}

/// The gate's verdict of one document. Each way the change is shown to reach it
/// (`witnesses`) is one of `adds_cause` (a cause that makes its approval not hold),
/// `removes_cause` (such a cause taken away), `decided` (what was decided about it
/// differs), `on_shelf` (it is on the shelf at one end only), `edited` (its content or
/// what its links name differs) and `recorded` (a record about it is at one end only,
/// or not the same at both).
#[derive(Debug, Serialize, JsonSchema)]
#[serde(tag = "verdict", rename_all = "snake_case")]
pub enum VerdictOut {
    /// The change does not reach it.
    NotClaimed,
    /// It is inside the reach, whatever the change does to it.
    Inside {
        /// How: `named` or `reached`.
        by: &'static str,
        /// What the change does to it.
        claim: InsideClaimOut,
    },
    /// The change reaches it outside the reach.
    Outside {
        /// How the change is shown to reach it.
        witnesses: Vec<&'static str>,
    },
    /// The change reaches it, and it cannot be shown outside the reach.
    Unproven {
        /// Why: `not_before` or `reach_untold`.
        place: AwayOut,
        /// How the change is shown to reach it.
        witnesses: Vec<&'static str>,
    },
    /// Whether the change reaches it cannot be told, and it is not inside the reach.
    Undetermined {
        /// Where it stands.
        place: AwayOut,
        /// What cannot be told.
        untold: ClaimUntoldOut,
    },
}

/// What the change does to a document inside the reach.
#[derive(Debug, Serialize, JsonSchema)]
#[serde(tag = "claim", rename_all = "snake_case")]
pub enum InsideClaimOut {
    /// It reaches it.
    Witnessed {
        /// How the change is shown to reach it.
        witnesses: Vec<&'static str>,
    },
    /// Whether it does cannot be told.
    CannotTell {
        /// What cannot be told.
        untold: ClaimUntoldOut,
    },
}

/// Where a document that is not inside the reach stands.
#[derive(Debug, Serialize, JsonSchema)]
#[serde(tag = "place", rename_all = "snake_case")]
pub enum AwayOut {
    /// It was not on the shelf before the change.
    NotBefore,
    /// The walk before could not read everything, so it may be inside: what it could
    /// not read, counted.
    ReachUntold(UntoldOut),
    /// Outside the reach.
    Outside,
}

impl From<Away> for AwayOut {
    fn from(away: Away) -> Self {
        match away {
            Away::NotBefore => Self::NotBefore,
            Away::ReachUntold(walk) => Self::ReachUntold(UntoldOut::from(walk)),
            Away::Outside => Self::Outside,
        }
    }
}

impl From<UnprovenWhy> for AwayOut {
    fn from(why: UnprovenWhy) -> Self {
        match why {
            UnprovenWhy::NotBefore => Self::NotBefore,
            UnprovenWhy::ReachUntold(walk) => Self::ReachUntold(UntoldOut::from(walk)),
        }
    }
}

/// What cannot be told of whether a change reaches a document, each way kept apart.
#[derive(Debug, Serialize, JsonSchema)]
pub struct ClaimUntoldOut {
    /// What cannot be told of whether the change adds a cause; null when it can.
    pub adds: Option<MadeUntoldOut>,
    /// What cannot be told of whether the change takes a cause away; null when it can.
    pub removes: Option<MadeUntoldOut>,
    /// Whether the document is invalid at both ends, so its content cannot be compared.
    pub edit: bool,
    /// When the document is on the shelf at neither end and a file may hold it, the
    /// files that may at each end; otherwise null. Boxed, as the rarer answer, so that
    /// a verdict that holds none is not as large as one that does.
    pub shelf: Option<Box<UnknownAtOut>>,
}

/// What `impact` could not tell, read one way.
#[derive(Debug, Serialize, JsonSchema)]
pub struct MadeUntoldOut {
    /// Which end of the change could not tell: `before`, `after` or `both`.
    pub side: &'static str,
    /// The names that could not be told.
    pub untold: Vec<UntoldPinOut>,
    /// The causes that stand on a name the other end could not tell.
    pub uncertain: Vec<RootOut>,
}

/// The files that may hold a document at each end, in the form of the presence of a
/// document that cannot be told.
#[derive(Debug, Serialize, JsonSchema)]
pub struct UnknownAtOut {
    /// Before the change; both lists empty when the document is gone there.
    pub before: MayHoldOut,
    /// After the change; both lists empty when the document is gone there.
    pub after: MayHoldOut,
}

/// The files that may hold a document no file is known to carry, at one end.
#[derive(Debug, Default, Serialize, JsonSchema)]
pub struct MayHoldOut {
    /// The files left in a merge conflict a side of which shows the document.
    pub shown: Vec<String>,
    /// The files whose identity could not be read, which may hold any.
    pub unreadable: Vec<String>,
}

/// A document of the answer: by the key it has after the change, or before it when it
/// has none after. The one place the key is picked.
fn doc_of(before: &Namer<'_>, after: &Namer<'_>, id: DocId) -> DocRef {
    let named = after.doc(id);
    if named.key.is_some() {
        named
    } else {
        before.doc(id)
    }
}

fn witnesses_out(witnesses: &NonEmptySet<Witness>) -> Vec<&'static str> {
    witnesses.iter().map(|w| witness_word(*w)).collect()
}

/// The files that may hold the document at one end, none when there are none.
fn may_hold_out(may: Option<&MayHold>) -> MayHoldOut {
    may.map(|may| MayHoldOut {
        shown: path_texts(may.shown()),
        unreadable: path_texts(may.any()),
    })
    .unwrap_or_default()
}

fn claim_untold_out(before: &Namer<'_>, after: &Namer<'_>, untold: &ClaimUntold) -> ClaimUntoldOut {
    ClaimUntoldOut {
        // The change read forwards: its causes are of the end after.
        adds: untold.adds().map(|why| MadeUntoldOut {
            side: sides_word(why.side()),
            untold: why.untold().iter().map(untold_out).collect(),
            uncertain: roots_out(after, why.uncertain()),
        }),
        // The change read backwards: its causes are of the end before.
        removes: untold.removes().map(|why| MadeUntoldOut {
            side: sides_word(why.side()),
            untold: why.untold().iter().map(untold_out).collect(),
            uncertain: roots_out(before, why.uncertain()),
        }),
        edit: untold.edit().is_some(),
        shelf: untold.shelf().map(|at| {
            Box::new(UnknownAtOut {
                before: may_hold_out(at.before()),
                after: may_hold_out(at.after()),
            })
        }),
    }
}

fn within_doc_out(before: &Namer<'_>, after: &Namer<'_>, entry: &WithinEntry) -> WithinDocOut {
    let verdict = entry.verdict();
    WithinDocOut {
        doc: doc_of(before, after, entry.id()),
        passes: verdict.passes(),
        verdict: match verdict.kind() {
            VerdictKind::NotClaimed { .. } => VerdictOut::NotClaimed,
            VerdictKind::Inside { by, claim, .. } => VerdictOut::Inside {
                by: inside_word(*by),
                claim: match claim {
                    InsideClaim::Witnessed(witnesses) => InsideClaimOut::Witnessed {
                        witnesses: witnesses_out(witnesses),
                    },
                    InsideClaim::CannotTell(untold) => InsideClaimOut::CannotTell {
                        untold: claim_untold_out(before, after, untold),
                    },
                },
            },
            VerdictKind::Outside { witnesses, .. } => VerdictOut::Outside {
                witnesses: witnesses_out(witnesses),
            },
            VerdictKind::Unproven { why, witnesses, .. } => VerdictOut::Unproven {
                place: AwayOut::from(*why),
                witnesses: witnesses_out(witnesses),
            },
            VerdictKind::Undetermined { place, untold, .. } => VerdictOut::Undetermined {
                place: AwayOut::from(*place),
                untold: claim_untold_out(before, after, untold),
            },
        },
    }
}

/// The files, as paths of the project.
fn path_texts(files: &[RelPath]) -> Vec<String> {
    files.iter().map(ToString::to_string).collect()
}

fn within_out(before: &Namer<'_>, after: &Namer<'_>, within: &Within) -> WithinOut {
    WithinOut {
        named: within
            .named()
            .iter()
            .map(|id| doc_of(before, after, *id))
            .collect(),
        reach_before: ReachBeforeOut {
            reached: within
                .reached_before()
                .iter()
                .map(|id| doc_of(before, after, *id))
                .collect(),
            untold: within.untold_before().copied().map(UntoldOut::from),
        },
        docs: within
            .entries()
            .iter()
            .map(|entry| within_doc_out(before, after, entry))
            .collect(),
        unreadable: within.unreadable().map(|found| UnreadableOut {
            files_before: path_texts(found.files_before()),
            files_after: path_texts(found.files_after()),
            sided_before: path_texts(found.sided_before()),
            sided_after: path_texts(found.sided_after()),
            records_before: path_texts(found.records_before()),
            records_after: path_texts(found.records_after()),
        }),
        passes: within.passes(),
    }
}

fn untold_out(pin: &UntoldPin) -> UntoldPinOut {
    UntoldPinOut {
        link: pin.link.to_string(),
        space: pin.artifact.space().to_string(),
        name: pin.artifact.name().to_string(),
        why: pin.why.to_string(),
    }
}

fn roots_out(namer: &Namer<'_>, roots: &[RootCause]) -> Vec<RootOut> {
    roots.iter().map(|root| namer.root(root)).collect()
}

fn holding_out(namer: &Namer<'_>, holding: &Holding) -> HoldingOut {
    let (roots, untold): (&[RootCause], Vec<&UntoldPin>) = match holding {
        Holding::DoesNotHold { roots, untold, .. } => (roots, untold.iter().collect()),
        Holding::CannotTell { untold, .. } => (&[], untold.iter().collect()),
        Holding::NotThere | Holding::Pending | Holding::Against(_) | Holding::Holds(_) => {
            (&[], Vec::new())
        }
    };
    HoldingOut {
        holding: holding_word(holding),
        level: holding_level(holding),
        roots: roots_out(namer, roots),
        untold: untold.into_iter().map(untold_out).collect(),
    }
}

fn made_out(namer: &Namer<'_>, made: &Made) -> MadeOut {
    let mut out = MadeOut {
        made: made_word(made),
        side: None,
        certain: Vec::new(),
        uncertain: Vec::new(),
        untold: Vec::new(),
    };
    match made {
        Made::Yes(yes) => {
            out.certain = yes.certain().iter().map(|root| namer.root(root)).collect();
            out.uncertain = roots_out(namer, yes.uncertain());
        }
        Made::CannotTell(why) => {
            out.side = Some(sides_word(why.side()));
            out.uncertain = roots_out(namer, why.uncertain());
            out.untold = why.untold().iter().map(untold_out).collect();
        }
        Made::No => {}
    }
    out
}

/// The answer of `impact`.
pub fn impact(report: &ImpactReport) -> ImpactOut {
    let before = Namer(&report.names_before);
    let after = Namer(&report.names_after);
    ImpactOut {
        way: report.way.as_str().to_owned(),
        docs: report
            .entries
            .iter()
            .map(|entry: &ImpactEntry| ImpactDocOut {
                doc: doc_of(&before, &after, entry.id()),
                before: holding_out(&before, entry.before()),
                after: holding_out(&after, entry.after()),
                changed: entry.changed(),
                made: made_out(&after, entry.made()),
            })
            .collect(),
        health_before: before.health(&report.health_before),
        health_after: after.health(&report.health_after),
        within: report
            .within
            .as_ref()
            .map(|within| within_out(&before, &after, within)),
    }
}
