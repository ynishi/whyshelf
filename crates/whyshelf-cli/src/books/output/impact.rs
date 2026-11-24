//! The answer of `impact` ([`ImpactOut`]): how the two ends of a change were read,
//! and for every document whether its approval held before, holds after, and whether
//! the change adds a cause that makes it not hold.

use schemars::JsonSchema;
use serde::Serialize;
use whyshelf_core::app::shelf::ImpactReport;
use whyshelf_core::domain::id::DocId;
use whyshelf_core::domain::shelf::freshness::{Holding, RootCause, UntoldPin};
use whyshelf_core::domain::shelf::impact::{ImpactEntry, Made};

use super::words::{holding_level, holding_word, made_word, sides_word};
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
    let doc = |id: DocId| {
        let named = after.doc(id);
        if named.key.is_some() {
            named
        } else {
            before.doc(id)
        }
    };
    ImpactOut {
        way: report.way.as_str().to_owned(),
        docs: report
            .entries
            .iter()
            .map(|entry: &ImpactEntry| ImpactDocOut {
                doc: doc(entry.id()),
                before: holding_out(&before, entry.before()),
                after: holding_out(&after, entry.after()),
                changed: entry.changed(),
                made: made_out(&after, entry.made()),
            })
            .collect(),
        health_before: before.health(&report.health_before),
        health_after: after.health(&report.health_after),
    }
}
