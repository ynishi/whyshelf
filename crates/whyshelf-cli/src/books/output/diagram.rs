//! The drawing of `map`: the books and links of the declaration as text, for a
//! person to read at a glance. It draws what the [`ShelfMap`] holds and nothing
//! more, in its order, and is the same for the same declaration.
//!
//! One block per book: a header (name, key grammar, description), then one line
//! per link. Every column is aligned across the whole drawing: the headers with
//! one another, and the links of every block with one another.
//!
//! ```text
//!   decision      DEC-  how it is met
//!   ├─ serves    depends   cover  1..   ──► requirement
//!   └─ replaces  replaces         0..1  ──► decision (own book)
//!   verification  V-    how a decision is checked
//!   ├─ tests     depends          0..   ──► artifact space=file  compare=lines ignore="#","//"
//!   └─ checks    depends   cover  1..   ──► decision
//! ```
//!
//! A count is `1` (one), `0..1` (optional), `0..` (many) or `1..` (at least one).
//! A covering link shows `cover` when its strength is `error` and `cover(warn)`
//! when it is `warn`. The column of `cover` is in every block when any link of the
//! declaration covers, and in none when none does. A link to artifacts expanded
//! from a preset shows `preset=<name>`, and `preset=<name>(default)` when it is the
//! default; its column, after the count, is likewise in every block or in none.
//! The values after `artifact` are the link's, whichever preset gave them:
//!
//! ```text
//!   verification  V-  how a decision is checked
//!   ├─ tests   depends  0..  preset=file(default)  ──► artifact space=file  compare=bytes
//!   └─ checks  depends  1..                        ──► decision
//! ```
//!
//! A link into the book that
//! holds it ends in `(own book)`. Each prefix `ignore` names is a quoted string,
//! escaped as JSON escapes it, so a prefix that holds a comma or a quote reads as
//! one; they are joined by `,`, sorted, as the model keeps them. A book with no
//! link is its header alone.

use whyshelf_core::domain::shelf::map::{LinkMap, MapTarget, ShelfMap};
use whyshelf_core::domain::shelf::schema::{Cardinality, CoverSeverity, KeyPattern};

use super::means;

/// Between two columns.
const GAP: &str = "  ";

/// The drawing of a map, one line after another, each ending in a line break.
pub(super) fn draw(map: &ShelfMap) -> String {
    let headers: Vec<(String, String, Option<&str>)> = map
        .books
        .iter()
        .map(|book| {
            let key = match &book.key {
                KeyPattern::Prefixed(prefix) => format!("{prefix}-"),
                KeyPattern::Slug => "slug".to_owned(),
            };
            (
                book.name.to_string(),
                key,
                book.description.as_ref().map(|d| d.as_str()),
            )
        })
        .collect();
    let name_width = width(headers.iter().map(|(name, _, _)| name.as_str()));
    let key_width = width(headers.iter().map(|(_, key, _)| key.as_str()));

    // The cells of every link, so that each column has one width for the drawing.
    let rows: Vec<[String; 5]> = map
        .links
        .iter()
        .map(|link| {
            let cover = match link.target {
                MapTarget::Book {
                    cover: Some(CoverSeverity::Error),
                    ..
                } => "cover",
                MapTarget::Book {
                    cover: Some(CoverSeverity::Warn),
                    ..
                } => "cover(warn)",
                MapTarget::Book { cover: None, .. } | MapTarget::Artifact { .. } => "",
            };
            let preset = match &link.target {
                MapTarget::Artifact {
                    preset: Some(origin),
                    ..
                } if origin.is_default() => format!("preset={}(default)", origin.name()),
                MapTarget::Artifact {
                    preset: Some(origin),
                    ..
                } => format!("preset={}", origin.name()),
                MapTarget::Artifact { preset: None, .. } | MapTarget::Book { .. } => String::new(),
            };
            [
                link.name.to_string(),
                means(link.propagation).to_owned(),
                cover.to_owned(),
                count(link.cardinality).to_owned(),
                preset,
            ]
        })
        .collect();
    let widths: Vec<usize> = (0..5)
        .map(|i| width(rows.iter().map(|row| row[i].as_str())))
        .collect();

    let mut out = String::new();
    for (book, (name, key, description)) in map.books.iter().zip(&headers) {
        let header = format!(
            "{name:<name_width$}{GAP}{key:<key_width$}{GAP}{}",
            description.unwrap_or("")
        );
        out.push_str(header.trim_end());
        out.push('\n');
        let block: Vec<(&LinkMap, &[String; 5])> = map
            .links
            .iter()
            .zip(&rows)
            .filter(|(link, _)| link.from == book.name)
            .collect();
        for (i, (link, row)) in block.iter().enumerate() {
            let branch = if i + 1 == block.len() {
                "└─"
            } else {
                "├─"
            };
            let cells: Vec<String> = row
                .iter()
                .zip(&widths)
                // A column no link fills (no link covers, or none has a preset) is
                // left out.
                .filter(|(_, w)| **w > 0)
                .map(|(cell, w)| format!("{cell:<w$}"))
                .collect();
            out.push_str(&format!(
                "{branch} {}{GAP}──► {}\n",
                cells.join(GAP),
                target(link)
            ));
        }
    }
    out
}

/// What a link points at, as drawn.
fn target(link: &LinkMap) -> String {
    match &link.target {
        MapTarget::Book { book, .. } if *book == link.from => format!("{book} (own book)"),
        MapTarget::Book { book, .. } => book.to_string(),
        MapTarget::Artifact {
            space, comparison, ..
        } => {
            let ignored: Vec<String> = comparison.ignored().map(|p| quoted(p.as_str())).collect();
            let mut text = format!(
                "artifact space={space}{GAP}compare={}",
                comparison.kind_word()
            );
            if !ignored.is_empty() {
                text.push_str(" ignore=");
                text.push_str(&ignored.join(","));
            }
            text
        }
    }
}

/// A text as a JSON string: in quotes, escaped as JSON escapes it.
fn quoted(text: &str) -> String {
    serde_json::Value::from(text).to_string()
}

/// A count, as drawn.
fn count(cardinality: Cardinality) -> &'static str {
    match cardinality {
        Cardinality::One => "1",
        Cardinality::Optional => "0..1",
        Cardinality::Many => "0..",
        Cardinality::AtLeastOne => "1..",
    }
}

/// The widest of the texts, in characters.
fn width<'a>(texts: impl Iterator<Item = &'a str>) -> usize {
    texts.map(|t| t.chars().count()).max().unwrap_or(0)
}
