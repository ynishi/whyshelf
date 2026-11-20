//! The answer of `moves`: the names approvals pinned something at that have nothing
//! behind them now, where each may have moved, and the same as text for a person.

use schemars::JsonSchema;
use serde::Serialize;
use whyshelf_core::app::shelf::MovesReport;
use whyshelf_core::domain::shelf::ledger::{FoundBy, MoveNote, Similarity};

use super::{DocRef, HealthOut, Namer};

/// The answer of `moves`: the names approvals pinned something at that have nothing
/// behind them now, with where each may have moved. It writes nothing.
#[derive(Debug, Serialize, JsonSchema)]
pub struct MovesOut {
    /// Every name a pin of something there, of an approval that stands, is read as
    /// now (through the moves declared) that has nothing behind it now, by space and
    /// then by name.
    pub absent: Vec<MovesAbsentOut>,
    /// The spaces of those names whose adapter cannot suggest, sorted.
    pub cannot_suggest: Vec<String>,
    /// What is wrong around the documents.
    pub health: HealthOut,
    /// The same as text for a person (`jq -r .drawing`), made from this answer alone.
    pub drawing: String,
}

/// One gone name in `moves`.
#[derive(Debug, Serialize, JsonSchema)]
pub struct MovesAbsentOut {
    /// The space.
    pub space: String,
    /// The name, as pins are read now: for the space `file`, a path under the root.
    /// The old name of a `judge moved`.
    pub name: String,
    /// The documents whose approvals that stand pin it, by identity.
    pub pinned_by: Vec<DocRef>,
    /// Where it may have moved: `same_content` first, then `renamed` from the most
    /// alike, then by name; empty when the adapter found none or cannot suggest.
    pub candidates: Vec<MovesCandidateOut>,
    /// What could not be looked at, or why nothing is suggested, in a fixed order of
    /// kinds, each kind once; empty when the adapter looked every way it has.
    pub notes: Vec<MovesNoteOut>,
}

/// A note of a gone name in `moves`.
#[derive(Debug, Serialize, JsonSchema)]
pub struct MovesNoteOut {
    /// One of a closed set: `in_two_spaces` (pins are read as the name in more than
    /// one space; `judge moved` refuses it, so nothing is suggested),
    /// `no_repository`, `no_revision` (no approval that pins it kept one),
    /// `unknown_revision`, `history_unreadable` (renames were not looked for, for
    /// each reason), `scan_truncated`, `unreadable_skipped`, `too_large_skipped`
    /// (the search for the same content stopped at a bound, or passed files over),
    /// `no_content` (every pin of it compares presence only: there is no content to
    /// search for, and only renames were looked for).
    pub kind: &'static str,
    /// A sentence for a person.
    pub detail: String,
}

/// A name a gone artifact may have moved to.
#[derive(Debug, Serialize, JsonSchema)]
pub struct MovesCandidateOut {
    /// The new name: the new name of a `judge moved`.
    pub name: String,
    /// How it was found: `same_content` (what is there has the hash pinned, under the
    /// pin's comparison) or `renamed` (the space's history pairs the two names).
    pub found: &'static str,
    /// For `renamed`, how alike the two are, a percent, when the history says (Git's
    /// similarity score); null otherwise.
    pub similarity: Option<u8>,
    /// Whether something is at the new name now, as the adapter says.
    pub present: bool,
}

/// The answer of `moves`.
pub fn moves(report: &MovesReport) -> MovesOut {
    let namer = Namer(&report.names);
    let absent: Vec<MovesAbsentOut> = report
        .suggestions
        .absent
        .iter()
        .map(|one| MovesAbsentOut {
            space: one.gone().artifact().space().to_string(),
            name: one.gone().name().to_string(),
            pinned_by: one
                .gone()
                .pinned_by()
                .into_iter()
                .map(|id| namer.doc(id))
                .collect(),
            candidates: one
                .candidates()
                .iter()
                .map(|c| {
                    let (found, similarity) = match c.found() {
                        FoundBy::SameContent => ("same_content", None),
                        FoundBy::Renamed { similarity } => {
                            ("renamed", similarity.map(Similarity::percent))
                        }
                    };
                    MovesCandidateOut {
                        name: c.name().to_string(),
                        found,
                        similarity,
                        present: c.present(),
                    }
                })
                .collect(),
            notes: one.notes().iter().map(move_note).collect(),
        })
        .collect();
    let cannot_suggest: Vec<String> = report
        .suggestions
        .cannot_suggest
        .iter()
        .map(ToString::to_string)
        .collect();
    let drawing = moves_drawing(&absent, &cannot_suggest);
    MovesOut {
        absent,
        cannot_suggest,
        health: namer.health(&report.health),
        drawing,
    }
}

/// A note of `moves`, as its kind word and a sentence.
fn move_note(note: &MoveNote) -> MovesNoteOut {
    let join = |items: Vec<&str>| items.join(", ");
    let (kind, detail) = match note {
        MoveNote::InTwoSpaces { spaces } => (
            "in_two_spaces",
            format!(
                "pins are read as this name in more than one space ({}): `judge moved` refuses it, so nothing is suggested",
                join(spaces.iter().map(|s| s.as_str()).collect())
            ),
        ),
        MoveNote::NoRepository => (
            "no_repository",
            "the project is in no repository: renames were not looked for".to_owned(),
        ),
        MoveNote::NoRevision => (
            "no_revision",
            "no approval that pins it kept a revision: renames were not looked for".to_owned(),
        ),
        MoveNote::UnknownRevision { revisions } => (
            "unknown_revision",
            format!(
                "not a commit of this repository ({}): no rename since it was looked for",
                join(revisions.iter().map(|r| r.as_str()).collect())
            ),
        ),
        MoveNote::HistoryUnreadable { detail } => (
            "history_unreadable",
            format!("renames were not looked for: {detail}"),
        ),
        MoveNote::ScanTruncated { read } => (
            "scan_truncated",
            format!("the search for the same content stopped at its bound, after {read} files"),
        ),
        MoveNote::UnreadableSkipped { count } => (
            "unreadable_skipped",
            format!("{count} files or directories could not be read, and were passed over"),
        ),
        MoveNote::TooLargeSkipped { count, limit } => (
            "too_large_skipped",
            format!("{count} files larger than {limit} bytes were not read"),
        ),
        MoveNote::NoContent => (
            "no_content",
            "it is pinned by its presence only, so there is no content to search for: only renames were looked for".to_owned(),
        ),
    };
    MovesNoteOut { kind, detail }
}

/// The text of `moves`: a block per gone name (its space, name and the documents
/// that pin it, then a line per candidate, then its note), and the spaces that
/// cannot suggest.
fn moves_drawing(absent: &[MovesAbsentOut], cannot_suggest: &[String]) -> String {
    let mut text = String::new();
    if absent.is_empty() {
        text.push_str("no pinned name is gone\n");
    }
    for one in absent {
        let by: Vec<&str> = one
            .pinned_by
            .iter()
            .map(|d| d.key.as_deref().unwrap_or(d.id.as_str()))
            .collect();
        text.push_str(&format!(
            "{} {}  pinned by {}\n",
            one.space,
            one.name,
            by.join(", ")
        ));
        if one.candidates.is_empty() {
            text.push_str("  (no candidate)\n");
        }
        let width = one
            .candidates
            .iter()
            .map(|c| c.name.chars().count())
            .max()
            .unwrap_or(0);
        for c in &one.candidates {
            let how = match c.similarity {
                Some(percent) => format!("{} {percent}%", c.found),
                None => c.found.to_owned(),
            };
            let absent = if c.present {
                ""
            } else {
                " (nothing there now)"
            };
            text.push_str(&format!("  -> {:<width$}  {how}{absent}\n", c.name));
        }
        for note in &one.notes {
            text.push_str(&format!("  note ({}): {}\n", note.kind, note.detail));
        }
    }
    if !cannot_suggest.is_empty() {
        text.push_str(&format!("cannot suggest: {}\n", cannot_suggest.join(", ")));
    }
    text
}
