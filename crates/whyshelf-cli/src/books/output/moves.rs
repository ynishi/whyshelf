//! The answer of `moves`: the names approvals pinned something at that have nothing
//! behind them now, where each may have moved, and the same as text for a person.

use schemars::JsonSchema;
use serde::Serialize;
use whyshelf_core::app::shelf::MovesReport;
use whyshelf_core::domain::shelf::ledger::{FoundBy, MoveNote, Similarity};

use super::words::plural;
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
    /// The moves of a prefix that several gone names share and that stand against what
    /// is there now, for the names each moves, in the order the moves were tried: first
    /// a move that a candidate found the best way carries, then the one the most gone
    /// names share, then by the move. Whether the actor is admitted to declare one is
    /// not tried. They are alternatives, each held alone against what is there now (see
    /// [`MovesPrefixOut`]).
    pub prefixes: Vec<MovesPrefixOut>,
    /// The moves of a prefix that gone names share and that do not stand against what
    /// is there now, with how many names under each cannot be moved, in the order the
    /// moves were tried, as `prefixes`.
    pub refused_prefixes: Vec<MovesRefusedPrefixOut>,
    /// How many moves of a prefix that gone names share were not tried, for the bound
    /// of one answer.
    pub untried_prefixes: usize,
    /// What is wrong around the documents.
    pub health: HealthOut,
    /// The same as text for a person (`jq -r .drawing`), made from this answer alone.
    pub drawing: String,
}

/// A move of a prefix that stands against what is there now.
///
/// The moves offered are alternatives, each held alone against what is there now, not
/// together: two that stand may move the same names (`app` to `domain`, and `app::x`
/// to `domain::x` when nothing else is left under `app`; or one prefix to two places).
/// After one is declared, another that moves the same names has nothing under it
/// (or only what the first did not move), and `judge moved --prefix` holds it again
/// then.
#[derive(Debug, Serialize, JsonSchema)]
pub struct MovesPrefixOut {
    /// The prefix the names are under now: the `from` of `judge moved --prefix`.
    pub from: String,
    /// The prefix they are moved under: the `to` of `judge moved --prefix`.
    pub to: String,
    /// The names it moves, as they are now.
    pub moved: Vec<MovesMovedOut>,
    /// The spaces whose adapter takes no prefix: names of them may be under it, and
    /// are not moved (as `judge moved --prefix` gives them).
    pub not_asked: Vec<String>,
}

/// A name a move of a prefix moves.
#[derive(Debug, Serialize, JsonSchema)]
pub struct MovesMovedOut {
    /// The space.
    pub space: String,
    /// The name, as pins are read now.
    pub name: String,
}

/// A move of a prefix that does not stand against what is there now.
#[derive(Debug, Serialize, JsonSchema)]
pub struct MovesRefusedPrefixOut {
    /// The prefix the names are under now.
    pub from: String,
    /// The prefix they are moved under.
    pub to: String,
    /// How many names under the prefix cannot be moved; `judge moved --prefix` lists
    /// each with why.
    pub refused: usize,
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
    /// alike, then `same_end`, then by name; empty when the adapter found none or
    /// cannot suggest.
    pub candidates: Vec<MovesCandidateOut>,
    /// What could not be looked at, or why nothing is suggested, in a fixed order of
    /// kinds, each kind once; empty when the adapter looked every way it has.
    pub notes: Vec<MovesNoteOut>,
}

/// A note of a gone name in `moves`. Its `kind` is a word of the versioned wire, not a
/// name of the domain: the one match that writes a note (`move_note`) turns the sum type
/// `MoveNote` into it, so the domain may rename a variant and the word stays.
#[derive(Debug, Serialize, JsonSchema)]
pub struct MovesNoteOut {
    /// One of a closed set: `in_two_spaces` (pins are read as the name in more than
    /// one space; `judge moved` refuses it, so nothing is suggested),
    /// `no_repository`, `no_revision` (no approval that pins it kept one),
    /// `unknown_revision`, `history_unreadable` (renames were not looked for, for
    /// each reason), `search_unavailable` (the space has nothing to search in here:
    /// no name was looked for), `scan_truncated`, `unreadable_skipped`,
    /// `too_large_skipped` (the search stopped at a bound, or passed over entries it
    /// could not read or that were too large: files, directories or pages),
    /// `candidates_cut` (more names were found than are kept),
    /// `no_content` (every pin of it compares presence only: there is no content to
    /// search for, and only renames were looked for).
    pub kind: &'static str,
    /// A sentence for a person.
    pub detail: String,
}

/// A name a gone artifact may have moved to. This answer is the flat wire form of
/// version 1, made from the sum type `FoundBy` with one match, so that fields are only
/// ever added within a version.
#[derive(Debug, Serialize, JsonSchema)]
pub struct MovesCandidateOut {
    /// The new name: the new name of a `judge moved`.
    pub name: String,
    /// How it was found: `same_content` (what is there has the hash pinned, under the
    /// pin's comparison), `renamed` (the space's history pairs the two names) or
    /// `same_end` (the name ends as the gone one does, and nothing more is known).
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
                        FoundBy::SameEnd => ("same_end", None),
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
    let prefixes: Vec<MovesPrefixOut> = report
        .suggestions
        .prefixes
        .iter()
        .map(|one| MovesPrefixOut {
            from: one.of().from().to_string(),
            to: one.of().to().to_string(),
            moved: one
                .moved()
                .iter()
                .map(|name| MovesMovedOut {
                    space: name.space().to_string(),
                    name: name.name().to_string(),
                })
                .collect(),
            not_asked: one.not_asked().iter().map(ToString::to_string).collect(),
        })
        .collect();
    let refused_prefixes: Vec<MovesRefusedPrefixOut> = report
        .suggestions
        .refused_prefixes
        .iter()
        .map(|one| MovesRefusedPrefixOut {
            from: one.of().from().to_string(),
            to: one.of().to().to_string(),
            refused: one.refused(),
        })
        .collect();
    let untried_prefixes = report.suggestions.untried_prefixes;
    let drawing = moves_drawing(
        &absent,
        &cannot_suggest,
        &prefixes,
        &refused_prefixes,
        untried_prefixes,
    );
    MovesOut {
        absent,
        cannot_suggest,
        prefixes,
        refused_prefixes,
        untried_prefixes,
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
        MoveNote::SearchUnavailable { detail } => (
            "search_unavailable",
            format!("no name was looked for: {detail}"),
        ),
        MoveNote::ScanTruncated { read } => (
            "scan_truncated",
            format!("the search stopped at its bound, after {read} entries"),
        ),
        MoveNote::UnreadableSkipped { count } => (
            "unreadable_skipped",
            format!(
                "{count} entries (files, directories or pages) could not be read, and were passed over"
            ),
        ),
        MoveNote::TooLargeSkipped { count, limit } => (
            "too_large_skipped",
            format!("{count} entries (files or pages) larger than {limit} bytes were not read"),
        ),
        MoveNote::CandidatesCut { found } => (
            "candidates_cut",
            format!("{found} names were found, and the candidates are the first of them in the order shown"),
        ),
        MoveNote::NoContent => (
            "no_content",
            "it is pinned by its presence only, so there is no content to search for: only renames were looked for".to_owned(),
        ),
    };
    MovesNoteOut { kind, detail }
}

/// The text of `moves`: a block per gone name (its space, name and the documents
/// that pin it, then a line per candidate, then its note), a line per move of a
/// prefix that stands and one per move that is refused, how many were not tried, and
/// the spaces that cannot suggest.
fn moves_drawing(
    absent: &[MovesAbsentOut],
    cannot_suggest: &[String],
    prefixes: &[MovesPrefixOut],
    refused_prefixes: &[MovesRefusedPrefixOut],
    untried_prefixes: usize,
) -> String {
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
    for one in prefixes {
        let names = one.moved.len();
        let not_asked = if one.not_asked.is_empty() {
            String::new()
        } else {
            format!(", not asked: {}", one.not_asked.join(", "))
        };
        text.push_str(&format!(
            "prefix {} -> {} ({names} name{}{not_asked})\n",
            one.from,
            one.to,
            plural(names)
        ));
    }
    for one in refused_prefixes {
        // None refused by name: nothing is under the prefix.
        let why = if one.refused == 0 {
            "nothing is under it".to_owned()
        } else {
            format!(
                "{} name{} cannot be moved",
                one.refused,
                plural(one.refused)
            )
        };
        text.push_str(&format!(
            "refused prefix {} -> {} ({why})\n",
            one.from, one.to
        ));
    }
    if untried_prefixes > 0 {
        text.push_str(&format!(
            "{untried_prefixes} more move{} of a prefix not tried\n",
            plural(untried_prefixes)
        ));
    }
    if !cannot_suggest.is_empty() {
        text.push_str(&format!("cannot suggest: {}\n", cannot_suggest.join(", ")));
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;

    fn refused(from: &str, to: &str, refused: usize) -> MovesRefusedPrefixOut {
        MovesRefusedPrefixOut {
            from: from.to_owned(),
            to: to.to_owned(),
            refused,
        }
    }

    fn standing(from: &str, to: &str, not_asked: &[&str]) -> MovesPrefixOut {
        MovesPrefixOut {
            from: from.to_owned(),
            to: to.to_owned(),
            moved: vec![MovesMovedOut {
                space: "rust-item".to_owned(),
                name: format!("{from}::Foo"),
            }],
            not_asked: not_asked.iter().map(|s| (*s).to_owned()).collect(),
        }
    }

    #[test]
    fn a_prefix_that_stands_says_the_spaces_not_asked_when_there_are_any() {
        let text = moves_drawing(
            &[],
            &[],
            &[
                standing("a", "b", &[]),
                standing("c", "d", &["rust-test", "js-test"]),
            ],
            &[],
            0,
        );
        assert!(text.contains("prefix a -> b (1 name)\n"), "{text}");
        assert!(
            text.contains("prefix c -> d (1 name, not asked: rust-test, js-test)\n"),
            "{text}"
        );
    }

    #[test]
    fn a_refused_prefix_says_how_many_names_cannot_move_or_that_nothing_is_under_it() {
        let text = moves_drawing(
            &[],
            &[],
            &[],
            &[
                refused("a", "b", 0),
                refused("c", "d", 1),
                refused("e", "f", 2),
            ],
            3,
        );
        assert!(
            text.contains("refused prefix a -> b (nothing is under it)\n"),
            "{text}"
        );
        assert!(
            text.contains("refused prefix c -> d (1 name cannot be moved)\n"),
            "{text}"
        );
        assert!(
            text.contains("refused prefix e -> f (2 names cannot be moved)\n"),
            "{text}"
        );
        assert!(
            text.contains("3 more moves of a prefix not tried\n"),
            "{text}"
        );
    }
}
