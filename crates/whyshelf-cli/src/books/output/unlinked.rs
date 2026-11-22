//! The answer of `unlinked`: the documents no link reaches or that hold none, in
//! three disjoint lists, the documents decided against apart from the others.

use schemars::JsonSchema;
use serde::Serialize;
use whyshelf_core::app::shelf::{Names, UnlinkedEntry, UnlinkedReport};

use super::words::schema_problem;
use super::{HealthOut, Namer, RowOut};

/// A document `unlinked` lists.
#[derive(Debug, Serialize, JsonSchema)]
pub struct UnlinkedEntryOut {
    /// The book it is in.
    pub book: String,
    /// The document.
    pub row: RowOut,
}

/// The documents a decision went against (they are denied, deprecated or retired)
/// that `unlinked` lists, apart from the others.
#[derive(Debug, Serialize, JsonSchema)]
pub struct DecidedAgainstOut {
    /// Those linked neither way.
    pub isolated: Vec<UnlinkedEntryOut>,
    /// Those no link reaches; null when not asked.
    pub no_incoming: Option<Vec<UnlinkedEntryOut>>,
    /// Those that hold no link; null when not asked.
    pub no_outgoing: Option<Vec<UnlinkedEntryOut>>,
}

/// The answer of `unlinked`: three disjoint lists, by the rule of the core's
/// `Linkage` (the manual says where it is).
#[derive(Debug, Serialize, JsonSchema)]
pub struct UnlinkedOut {
    /// The pending and approved documents linked neither way: the place to look
    /// first. Always answered, whatever the flags.
    pub isolated: Vec<UnlinkedEntryOut>,
    /// The problems of the declaration, as `check` gives them; when there are any,
    /// nothing else was read, every list asked for is empty and `health` is null.
    pub schema_problems: Vec<String>,
    /// The pending and approved documents no link reaches; null when not asked
    /// (`--no-outgoing`).
    pub no_incoming: Option<Vec<UnlinkedEntryOut>>,
    /// The pending and approved documents that hold no link; null when not asked
    /// (`--no-incoming`).
    pub no_outgoing: Option<Vec<UnlinkedEntryOut>>,
    /// The documents decided against, in the same three lists.
    pub decided_against: DecidedAgainstOut,
    /// What is wrong around the documents (the files that are no document are
    /// listed here, not above).
    pub health: Option<HealthOut>,
}

/// The answer of `unlinked`.
pub fn unlinked(report: &UnlinkedReport) -> UnlinkedOut {
    let no_names = Names::default();
    let (schema_problems, standing, decided_against, names, health) = match report {
        UnlinkedReport::SchemaInvalid { problems, lists } => (
            problems.iter().map(schema_problem).collect(),
            lists,
            lists,
            &no_names,
            None,
        ),
        UnlinkedReport::Answered {
            standing,
            decided_against,
            names,
            health,
        } => (Vec::new(), standing, decided_against, names, Some(health)),
    };
    let namer = Namer(names);
    let entries = |entries: &[UnlinkedEntry]| -> Vec<UnlinkedEntryOut> {
        entries
            .iter()
            .map(|e| UnlinkedEntryOut {
                book: e.book.to_string(),
                row: namer.row(&e.row),
            })
            .collect()
    };
    let list = |asked: &Option<Vec<UnlinkedEntry>>| asked.as_deref().map(entries);
    UnlinkedOut {
        isolated: entries(&standing.isolated),
        schema_problems,
        no_incoming: list(&standing.no_incoming),
        no_outgoing: list(&standing.no_outgoing),
        decided_against: DecidedAgainstOut {
            isolated: entries(&decided_against.isolated),
            no_incoming: list(&decided_against.no_incoming),
            no_outgoing: list(&decided_against.no_outgoing),
        },
        health: health.map(|h| namer.health(h)),
    }
}
